//! Serialized, version-fenced delegation recovery commands.

use std::str::FromStr;

use chrono::{Duration, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    adapters::persistence::{delivery::enqueue::insert_delivery_on, thread::insert_message_on},
    app_error::{AppError, AppResult},
    entities::{
        delegation::{
            DelegationAuthority, DelegationCommand, DelegationCommandResult, DelegationOperation,
            DeliveryCancellation,
        },
        outreach::OutreachStatus,
        task::{TaskStatus, TaskTransitionReason, TransitionActor},
    },
    task_queue::{DelegationCommandRequest, OutreachTargetIdentity, OutreachTargetRequest},
};

use super::{TransitionAttribution, required_response_count};

const MAX_OUTREACH_DEADLINE_HOURS: i64 = 720;

#[derive(sqlx::FromRow)]
struct LockedTask {
    status: String,
    owner_principal_id: Option<Uuid>,
    owner_principal_kind: Option<String>,
    execution_generation: Option<Uuid>,
    ownership_version: i64,
}

#[derive(sqlx::FromRow)]
struct LockedOutreach {
    status: String,
    version: i64,
    required_threshold_percent: f64,
    created_by_principal_id: Option<Uuid>,
}

#[derive(sqlx::FromRow)]
struct ActorFacts {
    kind: String,
    role: Option<String>,
}

struct MutationOutcome {
    target_id: Option<Uuid>,
    replacement_target_id: Option<Uuid>,
    response_association_ids: Vec<Uuid>,
    delivery_cancellation: DeliveryCancellation,
}

impl MutationOutcome {
    fn plain() -> Self {
        Self {
            target_id: None,
            replacement_target_id: None,
            response_association_ids: Vec::new(),
            delivery_cancellation: DeliveryCancellation::None,
        }
    }
}

fn fingerprint(command: &DelegationCommand) -> AppResult<String> {
    let canonical = serde_json::to_vec(command).map_err(|error| {
        AppError::Internal(format!("Could not encode delegation command: {error}"))
    })?;
    Ok(format!("{:x}", Sha256::digest(canonical)))
}

fn positive_version(value: i64) -> AppResult<u64> {
    u64::try_from(value)
        .ok()
        .filter(|version| *version > 0)
        .ok_or_else(|| AppError::Internal(format!("Invalid outreach version {value}")))
}

fn transition_actor(command: &DelegationCommand, actor_kind: &str) -> TransitionActor {
    match actor_kind {
        "agent" => TransitionActor::Agent(command.actor.principal_id.as_uuid()),
        _ => TransitionActor::Human(command.actor.principal_id.as_uuid()),
    }
}

async fn authorize(
    tx: &mut Transaction<'_, Postgres>,
    command: &DelegationCommand,
    task: &LockedTask,
    outreach: &LockedOutreach,
) -> AppResult<String> {
    let actor = sqlx::query_as::<_, ActorFacts>(
        r#"SELECT principal.kind, member.role
           FROM principals AS principal
           LEFT JOIN company_members AS member
             ON member.company_id = principal.company_id
            AND member.user_id = principal.user_id
           WHERE principal.company_id = $1 AND principal.id = $2"#,
    )
    .bind(command.company_id)
    .bind(command.actor.principal_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?
    .ok_or_else(|| AppError::NotFound("Delegated work not found.".into()))?;

    let is_current_human_owner = actor.kind == "person"
        && task.owner_principal_kind.as_deref() == Some("person")
        && task.owner_principal_id == Some(command.actor.principal_id.as_uuid());
    let is_manager =
        actor.kind == "person" && matches!(actor.role.as_deref(), Some("owner" | "admin"));
    let is_creator = actor.kind == "agent"
        && outreach.created_by_principal_id == Some(command.actor.principal_id.as_uuid())
        && task.owner_principal_kind.as_deref() == Some("agent")
        && task.owner_principal_id == Some(command.actor.principal_id.as_uuid());

    let authority_matches = match command.actor.authority {
        DelegationAuthority::HumanOwner => is_current_human_owner,
        DelegationAuthority::CompanyManager => is_manager,
        DelegationAuthority::OwningAgent => is_creator,
    };
    if !authority_matches {
        return Err(AppError::NotFound("Delegated work not found.".into()));
    }
    if command.actor.authority == DelegationAuthority::OwningAgent
        && matches!(
            command.operation,
            DelegationOperation::CancelOutreach { .. }
                | DelegationOperation::ProceedWithPartial { .. }
                | DelegationOperation::StopTask { .. }
        )
    {
        return Err(AppError::NotFound("Delegated work not found.".into()));
    }
    Ok(actor.kind)
}

async fn stored_result(
    tx: &mut Transaction<'_, Postgres>,
    command: &DelegationCommand,
    expected_fingerprint: &str,
) -> AppResult<Option<DelegationCommandResult>> {
    let row = sqlx::query_as::<_, (String, Value)>(
        r#"SELECT command_fingerprint, result
           FROM delegation_control_commands
           WHERE company_id = $1 AND task_id = $2 AND command_id = $3"#,
    )
    .bind(command.company_id)
    .bind(command.task_id)
    .bind(command.command_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let Some((stored_fingerprint, value)) = row else {
        return Ok(None);
    };
    if stored_fingerprint != expected_fingerprint {
        return Err(AppError::Conflict(
            "Command UUID was already used with different parameters.".into(),
        ));
    }
    serde_json::from_value(value)
        .map(Some)
        .map_err(|error| AppError::Internal(format!("Invalid stored delegation result: {error}")))
}

async fn cancel_delivery(
    tx: &mut Transaction<'_, Postgres>,
    target_id: Uuid,
    detail: &str,
) -> AppResult<DeliveryCancellation> {
    let row = sqlx::query_as::<_, (Uuid, String)>(
        r#"SELECT delivery.id, delivery.status
           FROM task_outreach_targets AS target
           JOIN message_deliveries AS delivery
             ON delivery.company_id = target.company_id AND delivery.id = target.delivery_id
           WHERE target.id = $1
           FOR UPDATE OF delivery"#,
    )
    .bind(target_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let Some((delivery_id, status)) = row else {
        return Ok(DeliveryCancellation::None);
    };
    if matches!(status.as_str(), "pending" | "retryable") {
        sqlx::query(
            r#"UPDATE message_deliveries
               SET status = 'dead_letter', attempt_count = max_attempts,
                   last_error_class = 'superseded', last_error_detail = $2,
                   updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status IN ('pending', 'retryable')"#,
        )
        .bind(delivery_id)
        .bind(detail)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
        return Ok(DeliveryCancellation::UnsentCancelled);
    }
    Ok(
        if matches!(status.as_str(), "sending" | "delivered" | "outcome_unknown") {
            DeliveryCancellation::MayHaveBeenReceived
        } else {
            DeliveryCancellation::None
        },
    )
}

fn merge_delivery_effect(
    current: DeliveryCancellation,
    next: DeliveryCancellation,
) -> DeliveryCancellation {
    if current == DeliveryCancellation::None {
        return next;
    }
    if next == DeliveryCancellation::None || current == next {
        return current;
    }
    DeliveryCancellation::Mixed
}

async fn revoke_internal_child(
    tx: &mut Transaction<'_, Postgres>,
    target_id: Uuid,
    actor: TransitionActor,
) -> AppResult<()> {
    let child = sqlx::query_as::<_, (Uuid, String, Option<Uuid>)>(
        r#"SELECT child.id, child.status, child.execution_generation
           FROM task_outreach_targets AS target
           JOIN message_delivery_parts AS request_part
             ON request_part.company_id = target.company_id
            AND request_part.delivery_id = target.delivery_id
            AND request_part.provider_message_key IS NOT NULL
           JOIN external_messages AS inbound_source
             ON inbound_source.company_id = target.company_id
            AND inbound_source.external_message_key = request_part.provider_message_key
            AND inbound_source.message_id <> target.request_message_id
           JOIN background_tasks AS child
             ON child.company_id = target.company_id
            AND child.source_message_uuid = inbound_source.message_id
           WHERE target.id = $1
           ORDER BY child.created_at, child.id LIMIT 1
           FOR UPDATE OF child"#,
    )
    .bind(target_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let Some((child_id, status, generation)) = child else {
        return Ok(());
    };
    if status == "completed" {
        return Err(AppError::Conflict(
            "The internal delegate already completed; its result wins.".into(),
        ));
    }
    if matches!(
        status.as_str(),
        "pending" | "processing" | "pending_approval" | "waiting_for_third_party_reply"
    ) {
        let attribution =
            TransitionAttribution::new(TaskTransitionReason::DelegationTargetCancelled, actor);
        sqlx::query(&format!(
            r#"UPDATE background_tasks
               SET status = 'stopped', worker_id = NULL, execution_generation = NULL,
                   locked_at = NULL, lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP,
                   {attribution}
               WHERE id = $1"#,
            attribution = attribution.set_clause(),
        ))
        .bind(child_id)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
        if let Some(generation) = generation {
            sqlx::query(
                r#"UPDATE task_attempts
                   SET status = 'failed', stop_reason = 'delegation_cancelled',
                       error = 'Internal delegation was cancelled',
                       finished_at = CURRENT_TIMESTAMP
                   WHERE task_id = $1 AND execution_generation = $2 AND status = 'processing'"#,
            )
            .bind(child_id)
            .bind(generation)
            .execute(&mut **tx)
            .await
            .map_err(AppError::from)?;
        }
    }
    Ok(())
}

async fn wake_task(
    tx: &mut Transaction<'_, Postgres>,
    task_id: Uuid,
    outreach_id: Uuid,
    reason: TaskTransitionReason,
    actor: TransitionActor,
) -> AppResult<()> {
    let attribution = TransitionAttribution::new(reason, actor);
    sqlx::query(&format!(
        r#"UPDATE background_tasks
           SET status = 'pending', run_at = CURRENT_TIMESTAMP, wait_expires_at = NULL,
               worker_id = NULL, execution_generation = NULL, locked_at = NULL,
               lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP, {attribution}
           WHERE id = $1 AND status IN ('waiting_for_third_party_reply', 'pending_approval')
             AND awaited_outreach_id = $2 AND EXISTS (SELECT 1 FROM task_outreaches outreach
                 WHERE outreach.id = $2 AND outreach.company_id = background_tasks.company_id
                 AND outreach.ownership_version = background_tasks.ownership_version
                 AND outreach.created_by_principal_id IS NOT DISTINCT FROM background_tasks.owner_principal_id)"#,
        attribution = attribution.set_clause(),
    ))
    .bind(task_id)
    .bind(outreach_id)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    sqlx::query(
        r#"UPDATE human_approvals SET status = 'expired', updated_at = CURRENT_TIMESTAMP
           WHERE task_id = $1 AND action_type = 'quorum_timeout' AND payload->>'outreach_id' = $2 AND status = 'pending'"#,
    )
    .bind(task_id)
    .bind(outreach_id.to_string())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

async fn response_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    outreach_id: Uuid,
) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar(
        r#"SELECT response_association_id
           FROM task_outreach_targets
           WHERE outreach_id = $1 AND status = 'responded'
           ORDER BY responded_at, id"#,
    )
    .bind(outreach_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)
}

async fn cancel_active_targets(
    tx: &mut Transaction<'_, Postgres>,
    outreach_id: Uuid,
    status: &str,
    detail: &str,
    actor: TransitionActor,
) -> AppResult<DeliveryCancellation> {
    let rows = sqlx::query_as::<_, (Uuid, String)>(
        r#"SELECT id, target_kind FROM task_outreach_targets
           WHERE outreach_id = $1 AND status = 'active'
           ORDER BY id FOR UPDATE"#,
    )
    .bind(outreach_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let mut delivery = DeliveryCancellation::None;
    for (target_id, kind) in rows {
        if kind == "internal_channel" {
            revoke_internal_child(tx, target_id, actor).await?;
        }
        delivery = merge_delivery_effect(delivery, cancel_delivery(tx, target_id, detail).await?);
        sqlx::query(
            "UPDATE task_outreach_targets SET status = $2 WHERE id = $1 AND status = 'active'",
        )
        .bind(target_id)
        .bind(status)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    }
    Ok(delivery)
}

async fn maybe_reach_quorum(
    tx: &mut Transaction<'_, Postgres>,
    command: &DelegationCommand,
    outreach: &LockedOutreach,
    actor: TransitionActor,
) -> AppResult<()> {
    let (eligible, responded): (i64, i64) = sqlx::query_as(
        r#"SELECT COUNT(*) FILTER (WHERE status IN ('active', 'responded'))::bigint,
                  COUNT(*) FILTER (WHERE status = 'responded')::bigint
           FROM task_outreach_targets WHERE outreach_id = $1"#,
    )
    .bind(command.operation.outreach_id())
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let required = required_response_count(eligible, outreach.required_threshold_percent) as i64;
    if eligible > 0 && responded >= required {
        sqlx::query(
            r#"UPDATE task_outreaches SET status = 'threshold_met', updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status IN ('waiting', 'timeout_pending_approval')"#,
        )
        .bind(command.operation.outreach_id())
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
        wake_task(
            tx,
            command.task_id,
            command.operation.outreach_id(),
            TaskTransitionReason::DelegationTargetCancelled,
            actor,
        )
        .await?;
        super::resolve_harness_outreach_on(tx, command.operation.outreach_id()).await?;
    }
    Ok(())
}

async fn insert_replacement(
    tx: &mut Transaction<'_, Postgres>,
    command: &DelegationCommand,
    old_target_id: Uuid,
    replacement: &OutreachTargetRequest,
) -> AppResult<Uuid> {
    let DelegationOperation::ReassignInternalTarget { new_channel_id, .. } = command.operation
    else {
        return Err(AppError::Internal(
            "Replacement supplied for another operation.".into(),
        ));
    };
    if replacement.target
        != (OutreachTargetIdentity::InternalChannel {
            channel_id: new_channel_id,
        })
    {
        return Err(AppError::BadRequest(
            "Replacement target does not match the requested internal channel.".into(),
        ));
    }
    if replacement.delivery.company_id != command.company_id
        || replacement.delivery.task_id != Some(command.task_id)
        || replacement.delivery.message_id != replacement.request.id
        || replacement.delivery.purpose != crate::entities::transport::DeliveryPurpose::Outreach
    {
        return Err(AppError::BadRequest(
            "Replacement request and delivery attribution do not match the delegation command."
                .into(),
        ));
    }
    insert_message_on(tx, &replacement.request).await?;
    insert_delivery_on(tx, &replacement.delivery).await?;
    let address = replacement
        .delivery
        .external_destination
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("Replacement delivery has no destination.".into()))?;
    sqlx::query_scalar(
        r#"INSERT INTO task_outreach_targets
               (outreach_id, company_id, email, target_kind, internal_channel_id,
                delivery_id, request_message_id, status, replaces_target_id)
           VALUES ($1, $2, $3, 'internal_channel', $4, $5, $6, 'active', $7)
           RETURNING id"#,
    )
    .bind(command.operation.outreach_id())
    .bind(command.company_id)
    .bind(address.as_str())
    .bind(new_channel_id)
    .bind(replacement.delivery.id.as_uuid())
    .bind(replacement.request.id.as_uuid())
    .bind(old_target_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)
}

async fn apply_operation(
    tx: &mut Transaction<'_, Postgres>,
    request: &DelegationCommandRequest,
    task: &LockedTask,
    outreach: &LockedOutreach,
    actor: TransitionActor,
) -> AppResult<MutationOutcome> {
    let command = &request.command;
    if task_is_terminal(&task.status) {
        return Err(AppError::Conflict(
            "The owning task already completed or stopped; its terminal result wins.".into(),
        ));
    }
    let outreach_status = OutreachStatus::from_str(&outreach.status).map_err(AppError::Internal)?;
    match command.operation {
        DelegationOperation::ExtendOutreach { expires_at, .. } => {
            if !matches!(
                outreach_status,
                OutreachStatus::Waiting | OutreachStatus::TimeoutPendingApproval
            ) {
                return Err(AppError::Conflict(
                    "This outreach can no longer be extended.".into(),
                ));
            }
            let now = Utc::now();
            if expires_at <= now || expires_at > now + Duration::hours(MAX_OUTREACH_DEADLINE_HOURS)
            {
                return Err(AppError::BadRequest(
                    "The deadline must be in the future and no more than 720 hours away.".into(),
                ));
            }
            sqlx::query(
                r#"UPDATE task_outreaches
                   SET status = 'waiting', expires_at = $2, updated_at = CURRENT_TIMESTAMP
                   WHERE id = $1"#,
            )
            .bind(command.operation.outreach_id())
            .bind(expires_at)
            .execute(&mut **tx)
            .await
            .map_err(AppError::from)?;
            let attribution =
                TransitionAttribution::new(TaskTransitionReason::OutreachExtended, actor);
            sqlx::query(&format!(
                r#"UPDATE background_tasks
                   SET status = CASE WHEN status = 'pending_approval'
                                     THEN 'waiting_for_third_party_reply' ELSE status END,
                       wait_expires_at = $2, updated_at = CURRENT_TIMESTAMP, {attribution}
                   WHERE id = $1 AND status IN (
                       'waiting_for_third_party_reply', 'pending_approval'
                   )"#,
                attribution = attribution.set_clause(),
            ))
            .bind(command.task_id)
            .bind(expires_at)
            .execute(&mut **tx)
            .await
            .map_err(AppError::from)?;
            sqlx::query(
                r#"UPDATE human_approvals SET status = 'expired', updated_at = CURRENT_TIMESTAMP
                   WHERE task_id = $1 AND action_type = 'quorum_timeout' AND status = 'pending'"#,
            )
            .bind(command.task_id)
            .execute(&mut **tx)
            .await
            .map_err(AppError::from)?;
            Ok(MutationOutcome::plain())
        }
        DelegationOperation::CancelTarget { target_id, .. } => {
            if !matches!(
                outreach_status,
                OutreachStatus::Waiting | OutreachStatus::TimeoutPendingApproval
            ) {
                return Err(AppError::Conflict(
                    "This outreach is no longer waiting for individual targets.".into(),
                ));
            }
            let kind: String = sqlx::query_scalar(
                r#"SELECT target_kind FROM task_outreach_targets
                   WHERE company_id = $1 AND outreach_id = $2 AND id = $3 AND status = 'active'
                   FOR UPDATE"#,
            )
            .bind(command.company_id)
            .bind(command.operation.outreach_id())
            .bind(target_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::Conflict("This target can no longer be cancelled.".into()))?;
            if command.actor.authority == DelegationAuthority::OwningAgent
                && kind != "internal_channel"
            {
                return Err(AppError::NotFound("Delegated work not found.".into()));
            }
            if kind == "internal_channel" {
                revoke_internal_child(tx, target_id, actor).await?;
            }
            let delivery =
                cancel_delivery(tx, target_id, "Waiting for this target was cancelled").await?;
            sqlx::query("UPDATE task_outreach_targets SET status = 'cancelled' WHERE id = $1")
                .bind(target_id)
                .execute(&mut **tx)
                .await
                .map_err(AppError::from)?;
            maybe_reach_quorum(tx, command, outreach, actor).await?;
            Ok(MutationOutcome {
                target_id: Some(target_id),
                delivery_cancellation: delivery,
                ..MutationOutcome::plain()
            })
        }
        DelegationOperation::CancelOutreach { outreach_id } => {
            if matches!(
                outreach_status,
                OutreachStatus::Cancelled | OutreachStatus::Completed
            ) {
                return Err(AppError::Conflict(
                    "This outreach is already closed.".into(),
                ));
            }
            let delivery = cancel_active_targets(
                tx,
                outreach_id,
                "cancelled",
                "Waiting for this outreach was cancelled",
                actor,
            )
            .await?;
            sqlx::query("UPDATE task_outreaches SET status = 'cancelled', updated_at = CURRENT_TIMESTAMP WHERE id = $1")
                .bind(outreach_id)
                .execute(&mut **tx)
                .await
                .map_err(AppError::from)?;
            wake_task(
                tx,
                command.task_id,
                outreach_id,
                TaskTransitionReason::DelegationCancelled,
                actor,
            )
            .await?;
            super::resolve_harness_outreach_on(tx, outreach_id).await?;
            Ok(MutationOutcome {
                delivery_cancellation: delivery,
                ..MutationOutcome::plain()
            })
        }
        DelegationOperation::ReassignInternalTarget { target_id, .. } => {
            if !matches!(
                outreach_status,
                OutreachStatus::Waiting | OutreachStatus::TimeoutPendingApproval
            ) {
                return Err(AppError::Conflict(
                    "This outreach is no longer accepting reassignment.".into(),
                ));
            }
            let (kind, old_channel_id): (String, Option<Uuid>) = sqlx::query_as(
                r#"SELECT target_kind, internal_channel_id FROM task_outreach_targets
                   WHERE company_id = $1 AND outreach_id = $2 AND id = $3 AND status = 'active'
                   FOR UPDATE"#,
            )
            .bind(command.company_id)
            .bind(command.operation.outreach_id())
            .bind(target_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::Conflict("This target can no longer be reassigned.".into()))?;
            if kind != "internal_channel" {
                return Err(AppError::BadRequest(
                    "Only internal targets can be reassigned.".into(),
                ));
            }
            let DelegationOperation::ReassignInternalTarget { new_channel_id, .. } =
                command.operation
            else {
                unreachable!("the match arm proves the operation")
            };
            if old_channel_id == Some(new_channel_id) {
                return Err(AppError::BadRequest(
                    "Choose a different internal channel for reassignment.".into(),
                ));
            }
            let replacement = request.replacement.as_ref().ok_or_else(|| {
                AppError::BadRequest("Reassignment requires a prepared replacement request.".into())
            })?;
            revoke_internal_child(tx, target_id, actor).await?;
            let delivery =
                cancel_delivery(tx, target_id, "This internal target was superseded").await?;
            sqlx::query("UPDATE task_outreach_targets SET status = 'superseded' WHERE id = $1")
                .bind(target_id)
                .execute(&mut **tx)
                .await
                .map_err(AppError::from)?;
            let replacement_target_id =
                insert_replacement(tx, command, target_id, replacement).await?;
            Ok(MutationOutcome {
                target_id: Some(target_id),
                replacement_target_id: Some(replacement_target_id),
                delivery_cancellation: delivery,
                ..MutationOutcome::plain()
            })
        }
        DelegationOperation::ProceedWithPartial { outreach_id } => {
            if !matches!(
                outreach_status,
                OutreachStatus::Waiting | OutreachStatus::TimeoutPendingApproval
            ) {
                return Err(AppError::Conflict(
                    "This outreach cannot proceed with partial results.".into(),
                ));
            }
            let snapshot = response_snapshot(tx, outreach_id).await?;
            let delivery = cancel_active_targets(
                tx,
                outreach_id,
                "expired",
                "Partial results were accepted",
                actor,
            )
            .await?;
            sqlx::query("UPDATE task_outreaches SET status = 'proceed_partial', updated_at = CURRENT_TIMESTAMP WHERE id = $1")
                .bind(outreach_id)
                .execute(&mut **tx)
                .await
                .map_err(AppError::from)?;
            wake_task(
                tx,
                command.task_id,
                outreach_id,
                TaskTransitionReason::DelegationPartial,
                actor,
            )
            .await?;
            super::resolve_harness_outreach_on(tx, outreach_id).await?;
            Ok(MutationOutcome {
                response_association_ids: snapshot,
                delivery_cancellation: delivery,
                ..MutationOutcome::plain()
            })
        }
        DelegationOperation::StopTask { outreach_id } => {
            let delivery = cancel_active_targets(
                tx,
                outreach_id,
                "cancelled",
                "The owning task was stopped",
                actor,
            )
            .await?;
            if !matches!(
                outreach_status,
                OutreachStatus::Cancelled | OutreachStatus::Completed
            ) {
                sqlx::query("UPDATE task_outreaches SET status = 'cancelled', updated_at = CURRENT_TIMESTAMP WHERE id = $1")
                    .bind(outreach_id)
                    .execute(&mut **tx)
                    .await
                    .map_err(AppError::from)?;
            }
            let attribution =
                TransitionAttribution::new(TaskTransitionReason::OperatorStopped, actor);
            sqlx::query(&format!(
                r#"UPDATE background_tasks
                   SET status = 'stopped', worker_id = NULL, execution_generation = NULL,
                       locked_at = NULL, lock_expires_at = NULL, wait_expires_at = NULL,
                       updated_at = CURRENT_TIMESTAMP, {attribution}
                   WHERE id = $1"#,
                attribution = attribution.set_clause(),
            ))
            .bind(command.task_id)
            .execute(&mut **tx)
            .await
            .map_err(AppError::from)?;
            if let Some(generation) = task.execution_generation {
                sqlx::query(
                    r#"UPDATE task_attempts SET status = 'failed', stop_reason = 'delegation_cancelled',
                       error = 'Task was stopped by a delegation control', finished_at = CURRENT_TIMESTAMP
                       WHERE task_id = $1 AND execution_generation = $2 AND status = 'processing'"#,
                )
                .bind(command.task_id)
                .bind(generation)
                .execute(&mut **tx)
                .await
                .map_err(AppError::from)?;
            }
            super::supersede_harness_runs_on(
                tx,
                command.company_id,
                command.task_id,
                task.ownership_version as u64,
            )
            .await?;
            sqlx::query("UPDATE task_approval_waits SET state = 'expired' WHERE company_id = $1 AND task_id = $2 AND state = 'waiting'")
                .bind(command.company_id).bind(command.task_id).execute(&mut **tx).await?;
            super::resolve_harness_outreach_on(tx, outreach_id).await?;
            Ok(MutationOutcome {
                delivery_cancellation: delivery,
                ..MutationOutcome::plain()
            })
        }
    }
}

fn task_is_terminal(status: &str) -> bool {
    matches!(status, "completed" | "stopped")
}

pub(crate) async fn execute_delegation_command_on(
    pool: &sqlx::PgPool,
    request: DelegationCommandRequest,
) -> AppResult<DelegationCommandResult> {
    request.command.validate().map_err(AppError::BadRequest)?;
    let fingerprint = fingerprint(&request.command)?;
    let command = &request.command;
    let mut tx = pool.begin().await.map_err(AppError::from)?;

    let outreach = sqlx::query_as::<_, LockedOutreach>(
        r#"SELECT status, version,
                  required_threshold_percent::double precision AS required_threshold_percent,
                  created_by_principal_id
           FROM task_outreaches
           WHERE company_id = $1 AND task_id = $2 AND id = $3 FOR UPDATE"#,
    )
    .bind(command.company_id)
    .bind(command.task_id)
    .bind(command.operation.outreach_id())
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::from)?
    .ok_or_else(|| AppError::NotFound("Delegated work not found.".into()))?;
    let task = sqlx::query_as::<_, LockedTask>(
        r#"SELECT status, owner_principal_id, owner_principal_kind, execution_generation, ownership_version
           FROM background_tasks WHERE company_id = $1 AND id = $2 FOR UPDATE"#,
    )
    .bind(command.company_id)
    .bind(command.task_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::from)?
    .ok_or_else(|| AppError::NotFound("Delegated work not found.".into()))?;
    if let Some(result) = stored_result(&mut tx, command, &fingerprint).await? {
        return Ok(result);
    }
    let current_version = positive_version(outreach.version)?;
    if current_version != command.expected_version {
        return Err(AppError::Conflict(format!(
            "Delegation changed from version {} to {}; refresh and try again.",
            command.expected_version, current_version
        )));
    }
    let actor_kind = authorize(&mut tx, command, &task, &outreach).await?;
    let actor = transition_actor(command, &actor_kind);
    let outcome = apply_operation(&mut tx, &request, &task, &outreach, actor).await?;

    let next_version = current_version
        .checked_add(1)
        .ok_or_else(|| AppError::Conflict("Delegation version exhausted.".into()))?;
    sqlx::query(
        r#"UPDATE task_outreaches SET version = $2, updated_at = CURRENT_TIMESTAMP
           WHERE id = $1 AND version = $3"#,
    )
    .bind(command.operation.outreach_id())
    .bind(
        i64::try_from(next_version)
            .map_err(|_| AppError::Conflict("Delegation version exhausted.".into()))?,
    )
    .bind(outreach.version)
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;
    let task_status_text: String =
        sqlx::query_scalar("SELECT status FROM background_tasks WHERE company_id = $1 AND id = $2")
            .bind(command.company_id)
            .bind(command.task_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(AppError::from)?;
    let result = DelegationCommandResult {
        version: 1,
        command_id: command.command_id,
        task_id: command.task_id,
        outreach_id: command.operation.outreach_id(),
        operation: command.operation.as_str().into(),
        outreach_version: next_version,
        task_status: TaskStatus::from_str(&task_status_text).map_err(AppError::Internal)?,
        target_id: outcome.target_id,
        replacement_target_id: outcome.replacement_target_id,
        response_association_ids: outcome.response_association_ids,
        delivery_cancellation: outcome.delivery_cancellation,
    };
    let result_json = serde_json::to_value(&result).map_err(|error| {
        AppError::Internal(format!("Could not encode delegation result: {error}"))
    })?;
    sqlx::query(
        r#"INSERT INTO delegation_control_commands
               (company_id, task_id, outreach_id, target_id, command_id,
                command_fingerprint, operation, actor_principal_id, actor_kind, authority,
                reason, reason_detail, from_version, to_version, result)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)"#,
    )
    .bind(command.company_id)
    .bind(command.task_id)
    .bind(command.operation.outreach_id())
    .bind(command.operation.target_id())
    .bind(command.command_id)
    .bind(fingerprint)
    .bind(command.operation.as_str())
    .bind(command.actor.principal_id.as_uuid())
    .bind(&actor_kind)
    .bind(command.actor.authority.as_str())
    .bind(command.reason.as_str())
    .bind(command.reason_detail.as_deref())
    .bind(outreach.version)
    .bind(
        i64::try_from(next_version)
            .map_err(|_| AppError::Conflict("Delegation version exhausted.".into()))?,
    )
    .bind(result_json)
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;
    tx.commit().await.map_err(AppError::from)?;
    Ok(result)
}
