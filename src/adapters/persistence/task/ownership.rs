//! Audited task business-ownership transitions.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        task::{
            TaskOwner, TaskOwnershipAuthority, TaskOwnershipCommand, TaskOwnershipEvent,
            TaskOwnershipOperation, TaskOwnershipReason,
        },
        transport::PrincipalId,
    },
};

use super::task_owner_from_db;

#[derive(sqlx::FromRow)]
pub(crate) struct TaskOwnershipEventDb {
    id: Uuid,
    task_id: Uuid,
    company_id: Uuid,
    sequence: i64,
    from_version: i64,
    to_version: i64,
    command_id: Uuid,
    command_fingerprint: String,
    operation: String,
    actor_principal_id: Option<Uuid>,
    previous_owner_principal_id: Option<Uuid>,
    previous_owner_kind: String,
    previous_owner_label: Option<String>,
    new_owner_principal_id: Option<Uuid>,
    new_owner_kind: String,
    new_owner_label: Option<String>,
    reason: String,
    reason_detail: Option<String>,
    handoff_instruction: Option<String>,
    occurred_at: DateTime<Utc>,
}

const EVENT_COLUMNS: &str = r#"id, task_id, company_id, sequence, from_version, to_version,
    command_id, command_fingerprint, operation, actor_principal_id,
    previous_owner_principal_id, previous_owner_kind, previous_owner_label,
    new_owner_principal_id, new_owner_kind, new_owner_label, reason, reason_detail,
    handoff_instruction, occurred_at"#;

impl TryFrom<TaskOwnershipEventDb> for TaskOwnershipEvent {
    type Error = AppError;

    fn try_from(row: TaskOwnershipEventDb) -> AppResult<Self> {
        let context = format!("task ownership event {}", row.id);
        Ok(Self {
            id: row.id,
            task_id: row.task_id,
            company_id: row.company_id,
            sequence: positive_version(row.sequence, &context)?,
            from_version: u64::try_from(row.from_version)
                .map_err(|_| AppError::Internal(format!("Invalid from version on {context}")))?,
            to_version: positive_version(row.to_version, &context)?,
            command_id: row.command_id,
            operation: TaskOwnershipOperation::from_str(&row.operation)
                .map_err(|error| AppError::Internal(format!("{context}: {error}")))?,
            actor_principal_id: row.actor_principal_id.map(PrincipalId::new),
            previous_owner: event_owner(
                row.previous_owner_principal_id,
                &row.previous_owner_kind,
                &context,
            )?,
            previous_owner_label: row.previous_owner_label,
            new_owner: event_owner(row.new_owner_principal_id, &row.new_owner_kind, &context)?,
            new_owner_label: row.new_owner_label,
            reason: TaskOwnershipReason::from_str(&row.reason)
                .map_err(|error| AppError::Internal(format!("{context}: {error}")))?,
            reason_detail: row.reason_detail,
            handoff_instruction: row.handoff_instruction,
            occurred_at: row.occurred_at,
        })
    }
}

fn positive_version(value: i64, context: &str) -> AppResult<u64> {
    let version = u64::try_from(value)
        .map_err(|_| AppError::Internal(format!("Invalid ownership version on {context}")))?;
    if version == 0 {
        return Err(AppError::Internal(format!(
            "Zero ownership version on {context}"
        )));
    }
    Ok(version)
}

fn event_owner(id: Option<Uuid>, kind: &str, context: &str) -> AppResult<TaskOwner> {
    match (id, kind) {
        (Some(id), "human") => Ok(TaskOwner::Human(PrincipalId::new(id))),
        (Some(id), "agent") => Ok(TaskOwner::Agent(PrincipalId::new(id))),
        (None, "unassigned") => Ok(TaskOwner::Unassigned),
        shape => Err(AppError::Internal(format!(
            "Invalid owner snapshot on {context}: {shape:?}"
        ))),
    }
}

fn command_fingerprint(command: &TaskOwnershipCommand) -> AppResult<String> {
    let canonical = serde_json::to_vec(command).map_err(|error| {
        AppError::Internal(format!("Could not encode ownership command: {error}"))
    })?;
    Ok(format!("{:x}", Sha256::digest(canonical)))
}

#[derive(sqlx::FromRow)]
struct LockedTask {
    channel_id: Uuid,
    status: String,
    owner_principal_id: Option<Uuid>,
    owner_principal_kind: Option<String>,
    ownership_version: i64,
    execution_generation: Option<Uuid>,
}

#[derive(Clone, sqlx::FromRow)]
struct PrincipalFacts {
    id: Uuid,
    kind: String,
    display_label: String,
    role: Option<String>,
    eligible_agent: bool,
    eligible_human: bool,
}

async fn principal_facts(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    principal_id: PrincipalId,
) -> AppResult<PrincipalFacts> {
    sqlx::query_as::<_, PrincipalFacts>(
        r#"SELECT principal.id, principal.kind, principal.display_label, member.role,
                  EXISTS (
                      SELECT 1 FROM channel_agents AS assignment
                      WHERE assignment.company_id = principal.company_id
                        AND assignment.channel_id = $2
                        AND assignment.agent_id = principal.agent_id
                  ) AS eligible_agent,
                  (principal.kind = 'person' AND member.user_id IS NOT NULL AND (
                      member.role = 'owner'
                      OR channel.access_mode IN ('team', 'public')
                      OR EXISTS (
                          SELECT 1 FROM channel_principal_grants AS channel_grant
                          WHERE channel_grant.company_id = principal.company_id
                            AND channel_grant.channel_id = $2
                            AND channel_grant.principal_id = principal.id
                            AND channel_grant.capability = 'view'
                      )
                  )) AS eligible_human
           FROM principals AS principal
           JOIN channels AS channel
             ON channel.company_id = principal.company_id AND channel.id = $2
           LEFT JOIN company_members AS member
             ON member.company_id = principal.company_id AND member.user_id = principal.user_id
           WHERE principal.company_id = $1 AND principal.id = $3"#,
    )
    .bind(company_id)
    .bind(channel_id)
    .bind(principal_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?
    .ok_or_else(|| AppError::BadRequest("Ownership principal is not in this company.".into()))
}

fn assert_target_eligible(target: TaskOwner, facts: &PrincipalFacts) -> AppResult<()> {
    match target {
        TaskOwner::Agent(id)
            if id.as_uuid() == facts.id && facts.kind == "agent" && facts.eligible_agent =>
        {
            Ok(())
        }
        TaskOwner::Human(id)
            if id.as_uuid() == facts.id && facts.kind == "person" && facts.eligible_human =>
        {
            Ok(())
        }
        TaskOwner::Unassigned => Ok(()),
        TaskOwner::Agent(_) => Err(AppError::BadRequest(
            "The target agent is not assigned to this task's primary channel.".into(),
        )),
        TaskOwner::Human(_) => Err(AppError::BadRequest(
            "The target teammate may not view this task's channel.".into(),
        )),
    }
}

fn authorize(
    command: &TaskOwnershipCommand,
    current_owner: TaskOwner,
    actor: &PrincipalFacts,
) -> AppResult<()> {
    let is_manager =
        actor.kind == "person" && matches!(actor.role.as_deref(), Some("owner" | "admin"));
    let is_current_owner = current_owner.principal_id() == Some(command.actor.principal_id);
    let authority_valid = match command.actor.authority {
        TaskOwnershipAuthority::Manager => is_manager,
        TaskOwnershipAuthority::CurrentOwner => is_current_owner,
        TaskOwnershipAuthority::UnassignedClaimant => {
            current_owner == TaskOwner::Unassigned && actor.eligible_human
        }
    };
    if !authority_valid {
        return Err(AppError::NotFound("Task not found.".into()));
    }

    match command.operation {
        TaskOwnershipOperation::Claim => {
            if current_owner != TaskOwner::Unassigned
                || command.new_owner.principal_id() != Some(command.actor.principal_id)
            {
                return Err(AppError::Conflict(
                    "Only unassigned work can be claimed, and only for yourself.".into(),
                ));
            }
        }
        TaskOwnershipOperation::Assign => {
            if !is_manager || current_owner != TaskOwner::Unassigned {
                return Err(AppError::Conflict(
                    "Assignment requires manager authority and an unassigned task.".into(),
                ));
            }
        }
        TaskOwnershipOperation::Transfer => {
            if current_owner == TaskOwner::Unassigned || current_owner == command.new_owner {
                return Err(AppError::Conflict(
                    "A transfer requires two different assigned owners.".into(),
                ));
            }
            if !is_manager && !is_current_owner {
                return Err(AppError::NotFound("Task not found.".into()));
            }
        }
        TaskOwnershipOperation::Release => {
            if !is_manager && !is_current_owner {
                return Err(AppError::NotFound("Task not found.".into()));
            }
        }
        TaskOwnershipOperation::InitialAssignment | TaskOwnershipOperation::OwnerRemoved => {
            return Err(AppError::BadRequest(
                "System ownership operations cannot be requested.".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) async fn change_task_ownership_on(
    pool: &sqlx::PgPool,
    command: TaskOwnershipCommand,
) -> AppResult<TaskOwnershipEvent> {
    command.validate().map_err(AppError::BadRequest)?;
    let fingerprint = command_fingerprint(&command)?;
    let mut tx = pool.begin().await.map_err(AppError::from)?;

    if let Some(lease) = command.execution {
        if lease.task_id != command.task_id
            || lease.ownership_version != command.expected_version
            || !super::lock_task_execution_on(&mut tx, command.company_id, lease).await?
        {
            return Err(AppError::Execution(
                crate::app_error::ExecutionFailure::OwnershipLost,
            ));
        }
    } else if command.invocation.is_some() {
        return Err(AppError::BadRequest(
            "An invocation ownership command requires its execution lease".into(),
        ));
    }

    let task = sqlx::query_as::<_, LockedTask>(
        r#"SELECT channel_id, status, owner_principal_id, owner_principal_kind, ownership_version,
                  execution_generation
           FROM background_tasks
           WHERE company_id = $1 AND id = $2
           FOR UPDATE"#,
    )
    .bind(command.company_id)
    .bind(command.task_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::from)?
    .ok_or_else(|| AppError::NotFound("Task not found.".into()))?;

    let existing_query = format!(
        "SELECT {EVENT_COLUMNS} FROM task_ownership_events \
         WHERE company_id = $1 AND task_id = $2 AND command_id = $3"
    );
    if let Some(existing) = sqlx::query_as::<_, TaskOwnershipEventDb>(&existing_query)
        .bind(command.company_id)
        .bind(command.task_id)
        .bind(command.command_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?
    {
        if existing.command_fingerprint != fingerprint {
            return Err(AppError::Conflict(
                "Ownership command id was already used with different parameters.".into(),
            ));
        }
        return existing.try_into();
    }

    let current_version = positive_version(task.ownership_version, "background task")?;
    if current_version != command.expected_version {
        return Err(AppError::Conflict(format!(
            "Ownership changed from version {} to {}; refresh and try again.",
            command.expected_version, current_version
        )));
    }
    if task.status == "completed" {
        return Err(AppError::Conflict(
            "Completed tasks retain their final owner.".into(),
        ));
    }

    let current_owner = task_owner_from_db(
        task.owner_principal_id,
        task.owner_principal_kind.as_deref(),
        "locked background task",
    )?;
    let actor = principal_facts(
        &mut tx,
        command.company_id,
        task.channel_id,
        command.actor.principal_id,
    )
    .await?;
    authorize(&command, current_owner, &actor)?;

    let target = match command.new_owner.principal_id() {
        Some(id) if id == command.actor.principal_id => actor.clone(),
        Some(id) => principal_facts(&mut tx, command.company_id, task.channel_id, id).await?,
        None => actor.clone(),
    };
    assert_target_eligible(command.new_owner, &target)?;

    let previous_label = if current_owner.principal_id() == Some(command.actor.principal_id) {
        Some(actor.display_label.clone())
    } else if let Some(id) = current_owner.principal_id() {
        Some(
            sqlx::query_scalar::<_, String>(
                "SELECT display_label FROM principals WHERE company_id = $1 AND id = $2",
            )
            .bind(command.company_id)
            .bind(id.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .map_err(AppError::from)?,
        )
    } else {
        None
    };
    let new_label = command
        .new_owner
        .principal_id()
        .map(|_| target.display_label.clone());
    let new_principal_kind = match command.new_owner {
        TaskOwner::Human(_) => Some("person"),
        TaskOwner::Agent(_) => Some("agent"),
        TaskOwner::Unassigned => None,
    };
    let new_version = current_version
        .checked_add(1)
        .ok_or_else(|| AppError::Conflict("Ownership version exhausted.".into()))?;

    if let (Some(lease), Some(reference)) = (command.execution, command.invocation) {
        super::record_harness_result_on(&mut tx, command.company_id, lease, reference, serde_json::json!({
            "task_id":command.task_id, "operation":command.operation.as_str(), "ownership_version":new_version,
            "owner_kind":command.new_owner.as_str(), "run_ended":true,
        })).await?;
    }
    super::supersede_harness_runs_on(
        &mut tx,
        command.company_id,
        command.task_id,
        current_version,
    )
    .await?;

    sqlx::query(
        r#"UPDATE background_tasks
           SET owner_principal_id = $3, owner_principal_kind = $4,
               ownership_version = $5,
               status = CASE WHEN status = 'processing' THEN 'pending' ELSE status END,
               worker_id = NULL, execution_generation = NULL, locked_at = NULL,
               lock_expires_at = NULL,
               run_at = CASE WHEN status = 'processing' THEN CURRENT_TIMESTAMP ELSE run_at END,
               transition_reason = CASE WHEN status = 'processing'
                   THEN 'ownership_transferred' ELSE transition_reason END,
               transition_actor_kind = CASE WHEN status = 'processing'
                   THEN 'system' ELSE transition_actor_kind END,
               transition_actor_id = CASE WHEN status = 'processing'
                   THEN NULL ELSE transition_actor_id END,
               transition_approval_id = CASE WHEN status = 'processing'
                   THEN NULL ELSE transition_approval_id END,
               transition_outreach_id = CASE WHEN status = 'processing'
                   THEN NULL ELSE transition_outreach_id END,
               updated_at = CURRENT_TIMESTAMP
           WHERE company_id = $1 AND id = $2 AND ownership_version = $6"#,
    )
    .bind(command.company_id)
    .bind(command.task_id)
    .bind(command.new_owner.principal_id().map(PrincipalId::as_uuid))
    .bind(new_principal_kind)
    .bind(
        i64::try_from(new_version)
            .map_err(|_| AppError::Conflict("Ownership version exhausted.".into()))?,
    )
    .bind(task.ownership_version)
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;

    if task.status == "processing" {
        sqlx::query(
            r#"UPDATE task_attempts
               SET status = 'failed', stop_reason = 'ownership_transferred',
                   error = 'Task ownership changed during execution',
                   finished_at = CURRENT_TIMESTAMP
               WHERE task_id = $1 AND execution_generation = $2 AND status = 'processing'"#,
        )
        .bind(command.task_id)
        .bind(task.execution_generation)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
    }

    let actor_kind = if actor.kind == "agent" {
        "agent"
    } else {
        "human"
    };
    let insert = format!(
        r#"INSERT INTO task_ownership_events (
               task_id, company_id, sequence, from_version, to_version, command_id,
               command_fingerprint, operation, actor_principal_id, actor_kind,
               previous_owner_principal_id, previous_owner_kind, previous_owner_label,
               new_owner_principal_id, new_owner_kind, new_owner_label, reason, reason_detail,
               handoff_instruction
           ) VALUES ($1, $2, $3, $4, $3, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                     $15, $16, $17, $18)
           RETURNING {EVENT_COLUMNS}"#,
    );
    let event = sqlx::query_as::<_, TaskOwnershipEventDb>(&insert)
        .bind(command.task_id)
        .bind(command.company_id)
        .bind(
            i64::try_from(new_version)
                .map_err(|_| AppError::Conflict("Ownership version exhausted.".into()))?,
        )
        .bind(task.ownership_version)
        .bind(command.command_id)
        .bind(fingerprint)
        .bind(command.operation.as_str())
        .bind(command.actor.principal_id.as_uuid())
        .bind(actor_kind)
        .bind(current_owner.principal_id().map(PrincipalId::as_uuid))
        .bind(current_owner.as_str())
        .bind(previous_label)
        .bind(command.new_owner.principal_id().map(PrincipalId::as_uuid))
        .bind(command.new_owner.as_str())
        .bind(new_label)
        .bind(command.reason.as_str())
        .bind(command.reason_detail.as_deref())
        .bind(command.handoff_instruction.as_deref().map(str::trim))
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;

    tx.commit().await.map_err(AppError::from)?;
    event.try_into()
}

pub(crate) async fn list_task_ownership_events_on(
    pool: &sqlx::PgPool,
    company_id: Uuid,
    task_id: Uuid,
) -> AppResult<Vec<TaskOwnershipEvent>> {
    let query = format!(
        "SELECT {EVENT_COLUMNS} FROM task_ownership_events \
         WHERE company_id = $1 AND task_id = $2 ORDER BY sequence"
    );
    let rows = sqlx::query_as::<_, TaskOwnershipEventDb>(&query)
        .bind(company_id)
        .bind(task_id)
        .fetch_all(pool)
        .await
        .map_err(AppError::from)?;
    rows.into_iter().map(TryInto::try_into).collect()
}

pub(crate) async fn list_chain_ownership_events_on(
    pool: &sqlx::PgPool,
    company_id: Uuid,
    task_ids: &[Uuid],
    limit: i64,
) -> AppResult<Vec<TaskOwnershipEvent>> {
    if task_ids.is_empty() {
        return Ok(Vec::new());
    }
    let query = format!(
        "SELECT {EVENT_COLUMNS} FROM task_ownership_events \
         WHERE company_id = $1 AND task_id = ANY($2) \
         ORDER BY occurred_at, task_id, sequence LIMIT $3"
    );
    let rows = sqlx::query_as::<_, TaskOwnershipEventDb>(&query)
        .bind(company_id)
        .bind(task_ids)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(AppError::from)?;
    rows.into_iter().map(TryInto::try_into).collect()
}
