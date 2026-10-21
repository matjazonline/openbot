//! Protected-action subjects use the decision ledger, but never complete a business step.
use super::*;
use crate::application::workflow::actions::{
    ActionDispatchRequest, ActionRequest, ActionTarget, ApprovalSubject, FrozenAction,
};
use crate::application::workflow::lease::WorkflowFence;

pub(in crate::adapters::persistence::workflow) async fn approved(
    db: &mut PgConnection,
    scope: ActivationRequest,
    subject: &ApprovalSubject,
    operation_digest: &str,
) -> AppResult<Option<Uuid>> {
    Ok(sqlx::query_scalar("SELECT decision.id FROM workflow_decisions AS decision JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id WHERE decision.company_id=$1 AND decision.run_id=$2 AND decision.execution_id=$3 AND decision.subject_kind='action_approval' AND decision.action_invocation_id=$4 AND decision.argument_digest=$5 AND decision.operation_digest=$6 AND decision.choice='approve' AND wait.state='completed'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(subject.invocation.as_uuid()).bind(subject.argument_digest.as_str()).bind(operation_digest).fetch_optional(db).await?)
}

pub(in crate::adapters::persistence::workflow) async fn approved_request(
    db: &mut PgConnection,
    request: &ActionRequest,
) -> AppResult<bool> {
    let frozen = FrozenAction::restore(serde_json::to_value(request).map_err(|_| invalid())?)?;
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_decisions AS decision JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id JOIN workflow_action_intents AS intent ON intent.company_id=decision.company_id AND intent.id=decision.action_invocation_id WHERE decision.company_id=$1 AND decision.run_id=$2 AND decision.execution_id=$3 AND decision.subject_kind='action_approval' AND decision.argument_digest=$4 AND decision.operation_digest=$5 AND decision.choice='approve' AND wait.state='completed' AND intent.operation=$6)")
        .bind(request.scope.company.as_uuid()).bind(request.scope.run.as_uuid()).bind(request.scope.execution.as_uuid()).bind(frozen.argument_digest().as_str()).bind(frozen.operation_digest().as_str()).bind(frozen.saved_operation()).fetch_one(db).await?)
}

#[derive(sqlx::FromRow)]
struct ParkHead {
    inputs: Value,
    step_id: String,
    deadline: DateTime<Utc>,
    remaining: bool,
}

/// A settled approval permits receipt reads, but never another attempt's remote entry.
pub(in crate::adapters::persistence::workflow) async fn consumed(
    db: &mut PgConnection,
    request: &ActionDispatchRequest,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_action_dispatches WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND invocation_id=$4 AND current_policy->'approval_required'='true'::jsonb AND attempt_number<$5)")
        .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
        .bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid())
        .bind(request.fence.attempt.0).fetch_one(db).await?)
}

pub(in crate::adapters::persistence::workflow) async fn park(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionDispatchRequest,
    action: &FrozenAction,
    notices: Option<&DecisionNotices>,
) -> AppResult<Option<WaitId>> {
    let Some(notices) = notices else {
        return Ok(None);
    };
    let scope = request.fence.scope;
    let head: ParkHead = sqlx::query_as("SELECT execution.frozen_inputs AS inputs,execution.step_id,run.deadline,(job.continuation_count<63) AS remaining FROM workflow_executions AS execution JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id WHERE execution.company_id=$1 AND execution.id=$2 AND job.id=$3")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0).fetch_one(&mut **tx).await?;
    let Some(config) = head.inputs.get("approval") else {
        return Ok(None);
    };
    let Ok((users, deadline)) = reviewer_deadline(config) else {
        return Ok(None);
    };
    let deadline = deadline.min(head.deadline);
    if !head.remaining || deadline <= waits::now(tx).await? {
        return Ok(None);
    }
    // One approval per immutable invocation; unrelated completed actions retain their history.
    let used: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_decisions WHERE company_id=$1 AND execution_id=$2 AND action_invocation_id=$3) OR EXISTS(SELECT 1 FROM workflow_action_dispatches WHERE company_id=$1 AND execution_id=$2 AND invocation_id=$3)")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).bind(request.subject.invocation.as_uuid()).fetch_one(&mut **tx).await?;
    if used {
        return Ok(None);
    }
    let Some(reviewers) = approvers(tx, scope, &users).await? else {
        return Ok(None);
    };
    let context: NoticeContext = sqlx::query_as("SELECT version.title,company.name AS company_label,run.correlation_id FROM workflow_runs AS run JOIN workflow_versions AS version ON version.company_id=run.company_id AND version.workflow_id=run.workflow_id AND version.id=run.version_id JOIN companies AS company ON company.id=run.company_id WHERE run.company_id=$1 AND run.id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut **tx).await?;
    let (decision, wait) = (Uuid::new_v4(), Uuid::new_v4());
    let step = StepId::parse(head.step_id).map_err(|_| invalid())?;
    let composed = match compose_notices(notices, &reviewers, &context, &step, decision, deadline) {
        Ok(composed) => composed,
        Err(AppError::BadRequest(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    let digest = hex_digest(&proposal(request.subject.invocation.as_uuid(), action))?;
    sqlx::query("INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,notification_intent) VALUES($1,$2,$3,$4,'decision',$5,$6)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait).bind(deadline).bind(json!({"version":1,"kind":"wait_registered","wait_id":wait,"run_id":scope.run.as_uuid(),"execution_id":scope.execution.as_uuid()})).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO workflow_decisions(company_id,run_id,execution_id,id,wait_id,subject_kind,proposal_digest,action_invocation_id,argument_digest,operation_digest,job_id,parked_attempt_number) VALUES($1,$2,$3,$4,$5,'action_approval',$6,$7,$8,$9,$10,$11)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(decision).bind(wait).bind(digest).bind(request.subject.invocation.as_uuid()).bind(action.argument_digest().as_str()).bind(action.operation_digest().as_str()).bind(scope.job.0).bind(request.fence.attempt.0).execute(&mut **tx).await?;
    insert_reviewers(tx, scope.company, decision, &reviewers, &composed).await?;
    close_attempt(tx, request.fence).await?;
    audit(tx, scope, DecisionId(decision), "action_approval_opened").await?;
    Ok(Some(WaitId::new(wait)))
}

pub(super) async fn approvers(
    db: &mut PgConnection,
    scope: ActivationRequest,
    users: &[Uuid],
) -> AppResult<Option<Vec<Reviewer>>> {
    let mut reviewers = Vec::with_capacity(users.len());
    for user in users {
        let Some(reviewer) = eligible_reviewer(db, scope, *user).await? else {
            return Ok(None);
        };
        let privileged: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM company_members AS member JOIN companies AS company ON company.id=member.company_id WHERE member.company_id=$1 AND member.user_id=$2 AND (member.role IN ('admin','owner') OR company.user_id=$2))")
            .bind(scope.company.as_uuid()).bind(user).fetch_one(&mut *db).await?;
        if !privileged {
            return Ok(None);
        }
        reviewers.push(reviewer);
    }
    Ok(Some(reviewers))
}

fn proposal(invocation: Uuid, action: &FrozenAction) -> Value {
    json!({"invocation":invocation,"operation":action.operation_key(),"target":action.request().target,
        "arguments":action.request().arguments,"argument_digest":action.argument_digest().as_str(),"operation_digest":action.operation_digest().as_str()})
}

pub(super) async fn close_attempt(db: &mut PgConnection, fence: WorkflowFence) -> AppResult<()> {
    let scope = fence.scope;
    let closed = sqlx::query("UPDATE task_attempts SET status='completed',stop_reason='approval_parked',finished_at=clock_timestamp() WHERE task_id=$1 AND attempt_number=$2 AND worker_id=$3 AND execution_generation=$4 AND status='processing'")
        .bind(scope.job.0).bind(fence.attempt.0).bind(fence.worker.0).bind(fence.generation.0).execute(&mut *db).await?;
    let job = sqlx::query("UPDATE background_tasks SET status='completed',updated_at=clock_timestamp(),worker_id=NULL,execution_generation=NULL,locked_at=NULL,lock_expires_at=NULL WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow' AND status='processing' AND worker_id=$4 AND execution_generation=$5 AND attempt_ordinal=$6 AND continuation_count<63 AND lock_expires_at>clock_timestamp()")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).bind(fence.worker.0).bind(fence.generation.0).bind(fence.attempt.0).execute(&mut *db).await?;
    if closed.rows_affected() != 1 || job.rows_affected() != 1 {
        return Err(closed_error());
    }
    sqlx::query("UPDATE workflow_runs SET state='waiting',waiting_reason='decision' WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(db).await?;
    Ok(())
}
fn closed_error() -> AppError {
    AppError::Conflict("Approval park lost ownership".into())
}

/// Exact attempt history proves a park even if a fast reviewer already settled it.
pub(in crate::adapters::persistence::workflow) async fn parked(
    db: &mut PgConnection,
    request: &ActionDispatchRequest,
) -> AppResult<Option<WaitId>> {
    let fence = request.fence;
    let wait: Option<Uuid> = sqlx::query_scalar("SELECT decision.wait_id FROM workflow_decisions AS decision JOIN task_attempts AS attempt ON attempt.task_id=decision.job_id AND attempt.attempt_number=decision.parked_attempt_number WHERE decision.job_id=$4 AND decision.parked_attempt_number=$5 AND decision.company_id=$1 AND decision.run_id=$2 AND decision.execution_id=$3 AND decision.subject_kind='action_approval' AND decision.action_invocation_id=$8 AND decision.argument_digest=$9 AND attempt.worker_id=$6 AND attempt.execution_generation=$7 AND attempt.status='completed' AND attempt.stop_reason='approval_parked'")
        .bind(fence.scope.company.as_uuid()).bind(fence.scope.run.as_uuid()).bind(fence.scope.execution.as_uuid()).bind(fence.scope.job.0).bind(fence.attempt.0).bind(fence.worker.0).bind(fence.generation.0).bind(request.subject.invocation.as_uuid()).bind(request.subject.argument_digest.as_str()).fetch_optional(db).await?;
    Ok(wait.map(WaitId::new))
}

pub(super) async fn view_proposal(
    db: &mut PgConnection,
    company: CompanyId,
    decision: DecisionId,
) -> AppResult<Value> {
    let (invocation, operation): (Uuid, Value) = sqlx::query_as("SELECT intent.id,intent.operation FROM workflow_decisions AS decision JOIN workflow_action_intents AS intent ON intent.company_id=decision.company_id AND intent.id=decision.action_invocation_id WHERE decision.company_id=$1 AND decision.id=$2")
        .bind(company.as_uuid()).bind(decision.0).fetch_one(db).await?;
    Ok(proposal(invocation, &FrozenAction::restore(operation)?))
}

pub(super) async fn submit(
    db: &mut PgConnection,
    scope: ActivationRequest,
    principal: Uuid,
    command: &SubmitDecision,
    fingerprint: &str,
) -> AppResult<DecisionSubmission> {
    record_choice(db, scope, principal, command, fingerprint).await?;
    let successor = if command.choice == "approve" {
        reopen_job(db, scope).await?;
        audit(db, scope, command.decision, "action_approved").await?;
        None
    } else {
        fail(db, scope, command.decision, "approval_rejected").await?;
        successor_of(db, scope).await?
    };
    Ok(DecisionSubmission {
        disposition: DecisionDisposition::Applied,
        revision: command.expected_revision + 1,
        successor,
    })
}

pub(in crate::adapters::persistence::workflow) async fn fail(
    db: &mut PgConnection,
    scope: ActivationRequest,
    decision: DecisionId,
    reason: &str,
) -> AppResult<()> {
    let operation: Value = sqlx::query_scalar("SELECT intent.operation FROM workflow_decisions AS decision JOIN workflow_action_intents AS intent ON intent.company_id=decision.company_id AND intent.id=decision.action_invocation_id WHERE decision.company_id=$1 AND decision.id=$2 AND decision.subject_kind='action_approval'")
        .bind(scope.company.as_uuid()).bind(decision.0).fetch_one(&mut *db).await?;
    let action = FrozenAction::restore(operation)?;
    let (ActionTarget::Connection { resource_kind, .. }
    | ActionTarget::Local { resource_kind, .. }) = &action.request().target;
    let family = match resource_kind.as_str() {
        "http" => "http",
        "mcp" => "mcp",
        "messaging" => "message",
        _ => return Err(invalid()),
    };
    let code = format!("{family}.{reason}");
    let failure = crate::domain::workflow::StepFailure::new(
        crate::domain::workflow::FailureClass::Terminal,
        crate::domain::workflow::FailureCode::parse(&code).map_err(|_| invalid())?,
        None,
    )
    .map_err(|_| invalid())?;
    audit(db, scope, decision, &code).await?;
    recovery::route_parked_failure(db, scope, &failure).await?;
    Ok(())
}

/// Confirms a suspension the handler reported: an action approval or a token-allowance pause
/// (06-D4), each parked in its own exact attempt with the same `approval_parked` witness.
pub(in crate::adapters::persistence::workflow) async fn confirm(
    persistence: &PostgresPersistence,
    fence: WorkflowFence,
    wait: WaitId,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_decisions AS decision JOIN task_attempts AS attempt ON attempt.task_id=decision.job_id AND attempt.attempt_number=decision.parked_attempt_number WHERE decision.job_id=$4 AND decision.parked_attempt_number=$5 AND decision.company_id=$1 AND decision.run_id=$2 AND decision.execution_id=$3 AND decision.wait_id=$8 AND decision.subject_kind IN ('action_approval','token_allowance') AND attempt.worker_id=$6 AND attempt.execution_generation=$7 AND attempt.status='completed' AND attempt.stop_reason='approval_parked')")
        .bind(fence.scope.company.as_uuid()).bind(fence.scope.run.as_uuid()).bind(fence.scope.execution.as_uuid()).bind(fence.scope.job.0).bind(fence.attempt.0).bind(fence.worker.0).bind(fence.generation.0).bind(wait.as_uuid()).fetch_one(persistence.pool()).await?)
}

pub(super) async fn audit(
    db: &mut PgConnection,
    scope: ActivationRequest,
    decision: DecisionId,
    kind: &str,
) -> AppResult<()> {
    sqlx::query("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,execution_id,decision_id) SELECT run.company_id,run.id,(SELECT COALESCE(MAX(event.sequence),0)+1 FROM workflow_run_events AS event WHERE event.company_id=$1 AND event.run_id=$2),$4,run.actor_id,$3,$5 FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(kind).bind(decision.0).execute(db).await?;
    Ok(())
}

// Shared choice commit; subject-specific approval and refusal remain with their owner.
pub(super) async fn record_choice(
    db: &mut PgConnection,
    scope: ActivationRequest,
    principal: Uuid,
    command: &SubmitDecision,
    fingerprint: &str,
) -> AppResult<()> {
    if !matches!(command.choice.as_str(), "approve" | "reject")
        || command.data != json!({})
        || command.feedback.chars().count() > crate::application::workflow::registry::MAX_TEXT
    {
        return Err(AppError::BadRequest("Invalid approval submission".into()));
    }
    sqlx::query("UPDATE workflow_decisions SET submitted_at=clock_timestamp(),submitted_by=$3,command_key=$4,command_fingerprint=$5,choice=$6,feedback=$7,data=$8,revision=revision+1 WHERE company_id=$1 AND id=$2 AND submitted_at IS NULL")
        .bind(scope.company.as_uuid()).bind(command.decision.0).bind(principal).bind(command.command_key.as_str()).bind(fingerprint).bind(&command.choice).bind(&command.feedback).bind(&command.data).execute(&mut *db).await?;
    sqlx::query("UPDATE workflow_waits SET state='completed',settled_at=clock_timestamp() WHERE company_id=$1 AND id=(SELECT wait_id FROM workflow_decisions WHERE company_id=$1 AND id=$2) AND reason='decision' AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(command.decision.0).execute(&mut *db).await?;
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    Ok(())
}

pub(in crate::adapters::persistence::workflow) async fn reopen_job(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<()> {
    let reopened = sqlx::query("UPDATE background_tasks SET status='pending',continuation_count=continuation_count+1,run_at=clock_timestamp(),updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND status='completed' AND continuation_count<63")
            .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).execute(&mut *db).await?;
    if reopened.rows_affected() != 1 {
        return Err(closed_error());
    }
    Ok(())
}
