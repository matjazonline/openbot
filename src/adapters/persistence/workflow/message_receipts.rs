//! Receipt consumption, effect-wait resumption, the finality-breach park, and the workflow
//! side of delivery admission (04.11-D2 §4, §6; D3 §D, §E, §I; D4 §M).
use super::super::persistence_bound::{Attempt, End, Evidence, WriteBound};
use super::*;
use crate::application::workflow::lease::{LeasePolicy, WorkflowFence};
use crate::entities::transport::DeliveryCancellationReason;
use std::time::Duration;

/// At most this many actions of one execution are consumed per poll visit, one per transaction.
pub(crate) const MESSAGE_RECEIPT_PAGE: i64 = 8;
const MAX_CONSUME_FAILURES: i16 = 5;
/// Discovery and poison bookkeeping are single short statements outside any fence.
const SHORT: Duration = Duration::from_secs(2);

#[derive(sqlx::FromRow)]
struct OpenAction {
    invocation_id: Uuid,
    run_id: Uuid,
    execution_id: Uuid,
    argument_digest: String,
    dispatch_id: Uuid,
    consume_failures: i16,
}

/// Consume up to `MESSAGE_RECEIPT_PAGE` ready actions of the execution, ordered
/// `(consume_after, invocation_id)`, one action per transaction (D3 §E).
pub(in crate::adapters::persistence::workflow) async fn consume_page(
    persistence: &PostgresPersistence,
    scope: ActivationRequest,
    policy: LeasePolicy,
) -> AppResult<u16> {
    let ready: Vec<OpenAction> = tokio::time::timeout(
        SHORT,
        sqlx::query_as(
            "SELECT action.invocation_id, action.run_id, action.execution_id, action.argument_digest, action.dispatch_id, action.consume_failures \
             FROM workflow_message_actions AS action \
             WHERE action.company_id=$1 AND action.execution_id=$2 AND action.state IN ('pending','uncertain') \
               AND action.consume_after<=clock_timestamp() \
               AND EXISTS(SELECT 1 FROM workflow_message_delivery_events AS event WHERE event.company_id=action.company_id \
                 AND event.invocation_id=action.invocation_id AND event.seq>action.consumed_through) \
             ORDER BY action.consume_after, action.invocation_id LIMIT $3",
        )
        .bind(scope.company.as_uuid())
        .bind(scope.execution.as_uuid())
        .bind(MESSAGE_RECEIPT_PAGE)
        .fetch_all(persistence.pool()),
    )
    .await
    .map_err(|_| super::super::lease::timed_out())??;
    let mut consumed = 0;
    for action in ready {
        match Box::pin(consume_one(persistence, scope.company, &action, policy)).await {
            Ok(()) => consumed += 1,
            Err(error) => {
                tracing::warn!(company_id = %scope.company.as_uuid(), invocation_id = %action.invocation_id,
                    error = %error, "Workflow message receipt consumption failed");
                record_consume_failure(persistence, scope.company, &action).await?;
            }
        }
    }
    Ok(consumed)
}

/// One action per transaction under `message.consume`. Leading locks: run `FOR UPDATE`,
/// action `FOR NO KEY UPDATE`, then every parent `FOR SHARE` in id order (D3 §D, §E).
async fn consume_one(
    persistence: &PostgresPersistence,
    company: CompanyId,
    action: &OpenAction,
    policy: LeasePolicy,
) -> AppResult<()> {
    let bound = WriteBound::start("message.consume", policy);
    let attempt = bound
        .attempt(Box::pin(async {
            let mut conn = bound.acquire(persistence.pool()).await?;
            let mut tx = bound.begin(&mut conn).await?;
            let consumed = consume_on(&mut tx, company, action).await;
            bound.end(tx, consumed).await
        }))
        .await;
    match attempt {
        Attempt::Done(()) => Ok(()),
        Attempt::Failed(error) => Err(error),
        // Idempotent: the cursor is monotone and the aggregate is recomputed from authority,
        // so a resolution only reports whether the cursor already reached the newest event;
        // either way rediscovery finishes the work and nothing is retried here.
        Attempt::Ambiguous => {
            let _ = bound
                .resolve_retryable(Box::pin(resolve_consume(
                    persistence,
                    company,
                    action,
                    &bound,
                )))
                .await;
            Ok(())
        }
    }
}

async fn lead_on(
    db: &mut PgConnection,
    company: CompanyId,
    action: &OpenAction,
) -> AppResult<Option<(String, i64)>> {
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(company.as_uuid())
        .bind(action.run_id)
        .execute(&mut *db)
        .await?;
    let open: Option<(String, i64)> = sqlx::query_as(
        "SELECT state, consumed_through FROM workflow_message_actions WHERE company_id=$1 AND invocation_id=$2 FOR NO KEY UPDATE",
    )
    .bind(company.as_uuid())
    .bind(action.invocation_id)
    .fetch_optional(&mut *db)
    .await?;
    if open.is_some() {
        sqlx::query(
            "SELECT delivery.id FROM message_deliveries AS delivery \
             JOIN workflow_message_destinations AS target ON target.company_id=delivery.company_id AND target.delivery_id=delivery.id \
             WHERE target.company_id=$1 AND target.invocation_id=$2 ORDER BY delivery.id FOR SHARE OF delivery",
        )
        .bind(company.as_uuid())
        .bind(action.invocation_id)
        .execute(&mut *db)
        .await?;
    }
    super::super::persistence_bound::locks_held();
    Ok(open)
}

async fn newest_on(db: &mut PgConnection, company: CompanyId, invocation: Uuid) -> AppResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT COALESCE(max(seq),0) FROM workflow_message_delivery_events WHERE company_id=$1 AND invocation_id=$2",
    )
    .bind(company.as_uuid())
    .bind(invocation)
    .fetch_one(&mut *db)
    .await?)
}

async fn consume_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    action: &OpenAction,
) -> AppResult<End<()>> {
    let Some((previous, _)) = lead_on(tx, company, action).await? else {
        return Ok(End::Rollback(()));
    };
    if !matches!(previous.as_str(), "pending" | "uncertain") {
        return Ok(End::Rollback(()));
    }
    // A new statement after the leading locks: every event committed before them is visible.
    let seq = newest_on(tx, company, action.invocation_id).await?;
    let (state, result): (String, Value) =
        sqlx::query_as("SELECT state, result FROM workflow_message_aggregate($1,$2)")
            .bind(company.as_uuid())
            .bind(action.invocation_id)
            .fetch_one(&mut **tx)
            .await?;
    if matches!(state.as_str(), "accepted" | "failed") {
        // The receipt guard recomputes and compares this value; the Rust copy is a convenience.
        sqlx::query("INSERT INTO workflow_action_receipts (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,result) VALUES ($1,$2,$3,$4,$5,$6,'message',$7)")
            .bind(company.as_uuid()).bind(action.run_id).bind(action.execution_id).bind(action.invocation_id)
            .bind(&action.argument_digest).bind(action.dispatch_id).bind(&result).execute(&mut **tx).await?;
    }
    sqlx::query(
        "UPDATE workflow_message_actions SET state=$3, consumed_through=GREATEST(consumed_through,$4), updated_at=clock_timestamp() \
         WHERE company_id=$1 AND invocation_id=$2",
    )
    .bind(company.as_uuid())
    .bind(action.invocation_id)
    .bind(&state)
    .bind(seq)
    .execute(&mut **tx)
    .await?;
    if state == "uncertain" && previous != "uncertain" {
        // First-uncertainty observation; the delivery engine owns the effect, so any run state.
        sqlx::query("INSERT INTO workflow_action_reconciliations (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,failure_code) VALUES ($1,$2,$3,$4,$5,$6,'message','message.outcome_unknown') ON CONFLICT (company_id,invocation_id) DO NOTHING")
            .bind(company.as_uuid()).bind(action.run_id).bind(action.execution_id).bind(action.invocation_id)
            .bind(&action.argument_digest).bind(action.dispatch_id).execute(&mut **tx).await?;
    }
    Ok(End::Commit(()))
}

/// Read-only, under the original's leading locks: applied iff the cursor reached the newest event.
async fn resolve_consume(
    persistence: &PostgresPersistence,
    company: CompanyId,
    action: &OpenAction,
    bound: &WriteBound,
) -> AppResult<Evidence<()>> {
    let mut conn = bound.acquire(persistence.pool()).await?;
    let mut tx = bound.begin(&mut conn).await?;
    let read = async {
        let Some((_, through)) = lead_on(&mut tx, company, action).await? else {
            return Ok(Evidence::NotApplied);
        };
        Ok(
            if through >= newest_on(&mut tx, company, action.invocation_id).await? {
                Evidence::Applied(())
            } else {
                Evidence::NotApplied
            },
        )
    }
    .await;
    bound.roll_back(tx, read).await
}

/// Poison handling in its own short transaction: backoff `min(2^n s, 300 s)`, `stalled` at five.
async fn record_consume_failure(
    persistence: &PostgresPersistence,
    company: CompanyId,
    action: &OpenAction,
) -> AppResult<()> {
    let failures = action
        .consume_failures
        .saturating_add(1)
        .min(MAX_CONSUME_FAILURES);
    let backoff = 2_f64.powi(i32::from(failures)).min(300.0);
    tokio::time::timeout(
        SHORT,
        sqlx::query(
            "UPDATE workflow_message_actions SET consume_failures=$3, consume_after=clock_timestamp()+make_interval(secs=>$4), \
               state=CASE WHEN $3>=5 THEN 'stalled' ELSE state END, updated_at=clock_timestamp() \
             WHERE company_id=$1 AND invocation_id=$2 AND state IN ('pending','uncertain') AND consume_failures<$3",
        )
        .bind(company.as_uuid())
        .bind(action.invocation_id)
        .bind(failures)
        .bind(backoff)
        .execute(persistence.pool()),
    )
    .await
    .map_err(|_| super::super::lease::timed_out())??;
    Ok(())
}

/// What the effect branch of `resume_on` found, under the run and wait locks it holds.
pub(in crate::adapters::persistence::workflow) enum EffectResume {
    Pending,
    Accepted(Value),
    Failed,
    /// Already parked by `conflicted_effect_park_on` in this transaction.
    Conflicted,
}

pub(in crate::adapters::persistence::workflow) async fn effect_resume_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    wait: Uuid,
) -> AppResult<EffectResume> {
    // The accepted 04.7 veto: a finality breach stops automatic continuation even when a usable
    // receipt exists (D4 §M). Settlement inserts conflicts under the same run lock.
    let action_scope = ActionScope {
        company: scope.company,
        run: scope.run,
        execution: scope.execution,
    };
    if super::super::action_uncertainty::conflicted_on(db, action_scope).await? {
        conflicted_effect_park_on(db, scope).await?;
        return Ok(EffectResume::Conflicted);
    }
    let result: Option<Value> = sqlx::query_scalar(
        "SELECT receipt.result FROM workflow_message_actions AS action \
         JOIN workflow_action_receipts AS receipt ON receipt.company_id=action.company_id AND receipt.invocation_id=action.invocation_id \
         WHERE action.company_id=$1 AND action.execution_id=$2 AND action.wait_id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(wait)
    .fetch_optional(&mut *db)
    .await?;
    Ok(match result {
        None => EffectResume::Pending,
        Some(result) if result.get("status").and_then(Value::as_str) == Some("accepted") => {
            EffectResume::Accepted(result)
        }
        Some(_) => EffectResume::Failed,
    })
}

/// Whether this effect wait belongs to a saved model tool call (an agent continuation)
/// rather than a message step.
pub(in crate::adapters::persistence::workflow) async fn model_tool_wait_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    wait: Uuid,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_message_actions WHERE company_id=$1 AND execution_id=$2 AND wait_id=$3 AND origin='model_tool')",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(wait)
    .fetch_one(&mut *db)
    .await?)
}

/// The Reconcile outcome `retire_on` gives `complete_io`'s conflict failure, for a parked
/// message step (D4 §M): no route, successor or output; unsent siblings superseded; any
/// receipt stays recorded truth.
pub(in crate::adapters::persistence::workflow) async fn conflicted_effect_park_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<()> {
    sqlx::query("UPDATE workflow_waits SET state='failed',failure_code='evidence_conflict',settled_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND reason='effect' AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).execute(&mut *db).await?;
    let closed = sqlx::query("UPDATE background_tasks SET status='failed',updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow' AND status='completed'")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).execute(&mut *db).await?;
    if closed.rows_affected() != 1 {
        return Err(invalid());
    }
    super::super::recovery::settle_run(
        db,
        scope,
        crate::domain::workflow::RecoveryDecision::Reconcile,
    )
    .await?;
    let code = FailureCode::parse("action.evidence_conflict").map_err(|_| invalid())?;
    super::super::action_uncertainty::record(
        db,
        scope.company,
        scope.run,
        Some(scope.execution),
        &code,
    )
    .await?;
    let invocations: Vec<Uuid> = sqlx::query_scalar(
        "SELECT invocation_id FROM workflow_message_actions WHERE company_id=$1 AND execution_id=$2 ORDER BY invocation_id",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_all(&mut *db)
    .await?;
    for invocation in invocations {
        cancel_message_deliveries_on(
            db,
            scope.company,
            scope.run,
            Some(invocation),
            DeliveryCancellationReason::EvidenceConflict,
        )
        .await?;
    }
    super::super::waits::audit(db, scope, "action_evidence_conflict").await?;
    Ok(())
}

/// Workflow controls, expiry and the conflict park supersede unsent deliveries; in-flight ones
/// keep their lease and get only the intent (D2 §6). Callers hold the run lock.
pub(in crate::adapters::persistence::workflow) async fn cancel_message_deliveries_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    invocation: Option<Uuid>,
    reason: DeliveryCancellationReason,
) -> AppResult<()> {
    crate::adapters::persistence::delivery::cancellation::cancel_workflow_deliveries_on(
        db,
        company.as_uuid(),
        run.as_uuid(),
        invocation,
        reason,
    )
    .await?;
    Ok(())
}

/// Supervisor proof that this exact fence was closed by the effect park and owns `wait`
/// (D3 §B: the same close cause the deferred dispatch guard requires).
pub(in crate::adapters::persistence::workflow) async fn confirm_suspension(
    persistence: &PostgresPersistence,
    fence: WorkflowFence,
    wait: Uuid,
) -> AppResult<bool> {
    tokio::time::timeout(
        SHORT,
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM task_attempts AS attempt \
               JOIN workflow_message_actions AS action ON action.company_id=$1 AND action.execution_id=$2 AND action.wait_id=$3 \
               JOIN workflow_action_dispatches AS marker ON marker.company_id=action.company_id AND marker.invocation_id=action.invocation_id \
              WHERE attempt.task_id=$4 AND attempt.attempt_number=$5 AND attempt.worker_id=$6 AND attempt.execution_generation=$7 \
                AND attempt.status='completed' AND attempt.stop_reason='effect_parked' \
                AND marker.job_id=attempt.task_id AND marker.attempt_number=attempt.attempt_number)",
        )
        .bind(fence.scope.company.as_uuid())
        .bind(fence.scope.execution.as_uuid())
        .bind(wait)
        .bind(fence.scope.job.0)
        .bind(fence.attempt.0)
        .bind(fence.worker.0)
        .bind(fence.generation.0)
        .fetch_one(persistence.pool()),
    )
    .await
    .map_err(|_| super::super::lease::timed_out())?
    .map_err(Into::into)
}

/// What a workflow-linked `begin_part` may do (D2 §6, R005 F014.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkflowDeliveryAdmission {
    NotWorkflow,
    Proceed,
    Refuse(DeliveryCancellationReason),
}

#[derive(sqlx::FromRow)]
struct DeliveryLink {
    company_id: Uuid,
    run_id: Uuid,
    execution_id: Uuid,
    kind: String,
    destination_id: Uuid,
    principal_id: Option<Uuid>,
    recipient_address: Option<String>,
}

/// The workflow side of `begin_part`, before the parent lock: the immutable link read without a
/// lock, then company/actor (as `authorize_company`), then run `FOR SHARE`. Cancellation, the
/// consumer and settlement all take run before parent, so there is no inversion.
pub(crate) async fn workflow_delivery_admission_on(
    db: &mut PgConnection,
    delivery: Uuid,
) -> AppResult<WorkflowDeliveryAdmission> {
    let link: Option<DeliveryLink> = sqlx::query_as(
        "SELECT target.company_id, target.run_id, action.execution_id, target.kind, target.destination_id, target.principal_id, target.recipient_address \
         FROM message_deliveries AS delivery \
         JOIN workflow_message_destinations AS target ON target.company_id=delivery.company_id AND target.delivery_id=delivery.id \
         JOIN workflow_message_actions AS action ON action.company_id=target.company_id AND action.invocation_id=target.invocation_id \
         WHERE delivery.id=$1",
    )
    .bind(delivery)
    .fetch_optional(&mut *db)
    .await?;
    let Some(link) = link else {
        return Ok(WorkflowDeliveryAdmission::NotWorkflow);
    };
    let company = CompanyId::new(link.company_id);
    let actor: Uuid =
        sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(link.company_id)
            .bind(link.run_id)
            .fetch_one(&mut *db)
            .await?;
    let actor = WorkflowActor::authenticated(actor)?;
    match super::super::authority::authorize_company(db, company, actor).await {
        Ok(()) => {}
        Err(AppError::NotFound(_)) => {
            return Ok(WorkflowDeliveryAdmission::Refuse(
                DeliveryCancellationReason::WorkflowRevoked,
            ));
        }
        Err(error) => return Err(error),
    }
    let (state, live): (String, bool) = sqlx::query_as(
        "SELECT state, deadline>clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR SHARE",
    )
    .bind(link.company_id)
    .bind(link.run_id)
    .fetch_one(&mut *db)
    .await?;
    if state == "cancelled" {
        return Ok(WorkflowDeliveryAdmission::Refuse(
            DeliveryCancellationReason::WorkflowCancelled,
        ));
    }
    if !live || !matches!(state.as_str(), "queued" | "running" | "waiting") {
        return Ok(WorkflowDeliveryAdmission::Refuse(
            DeliveryCancellationReason::WorkflowExpired,
        ));
    }
    // Remote dispatch refuses new provider entry in a conflicted execution; so does messaging.
    let action_scope = ActionScope {
        company,
        run: RunId::new(link.run_id),
        execution: ExecutionId::new(link.execution_id),
    };
    if super::super::action_uncertainty::conflicted_on(db, action_scope).await? {
        return Ok(WorkflowDeliveryAdmission::Refuse(
            DeliveryCancellationReason::EvidenceConflict,
        ));
    }
    if link.kind == "user" {
        let current = super::member_on(db, company, link.destination_id, true).await?;
        let unchanged = current.is_some_and(|member| {
            Some(member.principal) == link.principal_id
                && Some(member.email.as_str()) == link.recipient_address.as_deref()
        });
        if !unchanged {
            return Ok(WorkflowDeliveryAdmission::Refuse(
                DeliveryCancellationReason::RecipientRevoked,
            ));
        }
    }
    Ok(WorkflowDeliveryAdmission::Proceed)
}

/// Under the parent lock: a reopened part that has started no request since its reopen needs the
/// consuming proof to be unexpired (D3 §I).
pub(crate) async fn reopen_proof_expired_on(
    db: &mut PgConnection,
    delivery: Uuid,
    part: Uuid,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM message_deliveries AS delivery \
           JOIN workflow_message_delivery_reopens AS reopen ON reopen.company_id=delivery.company_id AND reopen.id=delivery.reopen_id \
           JOIN message_delivery_parts AS part ON part.delivery_id=delivery.id AND part.id=$2 \
          WHERE delivery.id=$1 AND (part.request_started_at IS NULL OR part.request_started_at<reopen.created_at) \
            AND reopen.valid_until<=clock_timestamp())",
    )
    .bind(delivery)
    .bind(part)
    .fetch_one(&mut *db)
    .await?)
}
