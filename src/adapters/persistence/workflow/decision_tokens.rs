//! Token decisions authorize one recorded total and never reserve a provider call.
use super::*;
use crate::application::workflow::{checkpoints::WorkflowModelRequest, lease::WorkflowFence};
use crate::services::harness::runs::{ModelRequestId, Mutation, RunCheckpoint};

pub(in crate::adapters::persistence::workflow) enum TokenSubject<'a> {
    Request(&'a WorkflowModelRequest),
    Settlement(ModelRequestId),
}
pub(in crate::adapters::persistence::workflow) struct TokenPause<'a> {
    pub fence: WorkflowFence,
    pub checkpoint: &'a RunCheckpoint,
    pub root: &'a token_budget::RootTokens,
    pub subject: TokenSubject<'a>,
}
pub(in crate::adapters::persistence::workflow) struct TokenParked {
    pub checkpoint: RunCheckpoint,
    pub wait: WaitId,
}

#[derive(sqlx::FromRow)]
struct Head {
    step_id: String,
    deadline: DateTime<Utc>,
    remaining: bool,
    reviewers: Vec<Uuid>,
}

pub(in crate::adapters::persistence::workflow) async fn park(
    tx: &mut Transaction<'_, Postgres>,
    pause: TokenPause<'_>,
    notices: &DecisionNotices,
) -> AppResult<TokenParked> {
    let scope = pause.fence.scope;
    let head: Head = sqlx::query_as("SELECT execution.step_id,run.deadline,(job.continuation_count<63) AS remaining,
        ARRAY(SELECT (reviewer->>'id')::uuid FROM jsonb_array_elements(budget.token_policy->'reviewers') AS reviewer) AS reviewers
        FROM workflow_executions AS execution JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
        JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id
        JOIN workflow_run_budgets AS link ON link.company_id=run.company_id AND link.run_id=run.id
        JOIN workflow_root_budgets AS budget ON budget.company_id=link.company_id AND budget.root_run_id=link.root_run_id
        WHERE execution.company_id=$1 AND execution.id=$2 AND job.id=$3")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0).fetch_one(&mut **tx).await?;
    if !head.remaining || head.deadline <= waits::now(tx).await? || pause.root.refusal.is_some() {
        return Err(closed());
    }
    let reviewers = approvals::approvers(tx, scope, &head.reviewers)
        .await?
        .ok_or_else(missing)?;
    let context: NoticeContext = sqlx::query_as("SELECT version.title,company.name AS company_label,run.correlation_id
        FROM workflow_runs AS run JOIN workflow_versions AS version ON version.company_id=run.company_id AND version.workflow_id=run.workflow_id AND version.id=run.version_id
        JOIN companies AS company ON company.id=run.company_id WHERE run.company_id=$1 AND run.id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut **tx).await?;
    let decision = Uuid::new_v4();
    let wait = Uuid::new_v4();
    let step = StepId::parse(head.step_id).map_err(|_| invalid())?;
    let composed = compose_notices(
        notices,
        &reviewers,
        &context,
        &step,
        decision,
        head.deadline,
    )?;
    let proposal = pause.root.proposal(match &pause.subject {
        TokenSubject::Request(request) => Some(*request),
        TokenSubject::Settlement(_) => None,
    })?;
    let (stage, request) = match pause.subject {
        TokenSubject::Request(request) => ("reservation", request.reservation.request_id),
        TokenSubject::Settlement(request) => ("settlement", request),
    };
    let (checkpoint, _) = pause.checkpoint.apply(Mutation::WorkflowTokenPause)?;
    checkpoints::persist_on(
        tx,
        pause.fence,
        &checkpoint,
        checkpoints::CheckpointOwnership::TokenPause(decision),
    )
    .await?;
    sqlx::query("INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,notification_intent) VALUES($1,$2,$3,$4,'decision',$5,$6)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait).bind(head.deadline)
        .bind(json!({"version":1,"kind":"wait_registered","wait_id":wait,"run_id":scope.run.as_uuid(),"execution_id":scope.execution.as_uuid()})).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO workflow_decisions(company_id,run_id,execution_id,id,wait_id,subject_kind,proposal_digest,job_id,parked_attempt_number,
        checkpoint_id,checkpoint_revision,token_request_id,token_stage,token_proposal) VALUES($1,$2,$3,$4,$5,'token_allowance',$6,$7,$8,$9,$10,$11,$12,$13)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(decision).bind(wait).bind(hex_digest(&proposal)?)
        .bind(scope.job.0).bind(pause.fence.attempt.0).bind(checkpoint.run_id.0).bind(i64::try_from(checkpoint.revision.0).map_err(|_| invalid())?)
        .bind(request.0).bind(stage).bind(proposal).execute(&mut **tx).await?;
    insert_reviewers(tx, scope.company, decision, &reviewers, &composed).await?;
    approvals::close_attempt(tx, pause.fence).await?;
    approvals::audit(tx, scope, DecisionId(decision), "token_allowance_opened").await?;
    Ok(TokenParked {
        checkpoint,
        wait: WaitId::new(wait),
    })
}

pub(in crate::adapters::persistence::workflow) async fn approved_request(
    db: &mut PgConnection,
    scope: ActivationRequest,
    decision: Uuid,
    request: &WorkflowModelRequest,
) -> AppResult<()> {
    let saved: Value=sqlx::query_scalar("SELECT token_proposal FROM workflow_decisions AS decision
        JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id
        WHERE decision.company_id=$1 AND decision.run_id=$2 AND decision.execution_id=$3 AND decision.id=$4
        AND decision.subject_kind='token_allowance' AND decision.choice='approve' AND wait.state='completed'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(decision).fetch_optional(db).await?.ok_or_else(closed)?;
    if !saved["request"].is_null() && saved["request"] != request.saved() {
        return Err(AppError::Conflict("Approved model request changed".into()));
    }
    Ok(())
}

pub(super) async fn submit(
    db: &mut PgConnection,
    scope: ActivationRequest,
    principal: Uuid,
    command: &SubmitDecision,
    fingerprint: &str,
) -> AppResult<DecisionSubmission> {
    let root = token_budget::lock_scope(db, scope).await?;
    if root.refusal.is_some() {
        return Err(closed());
    }
    let listed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_run_budgets AS link
        JOIN workflow_root_budgets AS budget USING(company_id,root_run_id)
        CROSS JOIN LATERAL jsonb_array_elements(budget.token_policy->'reviewers') AS reviewer
        WHERE link.company_id=$1 AND link.run_id=$2 AND reviewer->>'id'=$3)",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(command.actor.user_id().to_string())
    .fetch_one(&mut *db)
    .await?;
    if !listed {
        return Err(missing());
    }
    approvals::record_choice(db, scope, principal, command, fingerprint).await?;
    if command.choice == "approve" {
        approvals::reopen_job(db, scope).await?;
        approvals::audit(db, scope, command.decision, "token_allowance_approved").await?;
    } else {
        fail(db, scope, command.decision, "token_allowance_rejected").await?;
    }
    Ok(DecisionSubmission {
        disposition: DecisionDisposition::Applied,
        revision: command.expected_revision + 1,
        successor: None,
    })
}

pub(in crate::adapters::persistence::workflow) async fn fail(
    db: &mut PgConnection,
    scope: ActivationRequest,
    decision: DecisionId,
    event: &str,
) -> AppResult<()> {
    // No error route and no automatic successor can bypass a root token refusal.
    sqlx::query("UPDATE workflow_runs SET state='failed',waiting_reason=NULL,terminal_execution_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).execute(&mut *db).await?;
    let changed=sqlx::query("UPDATE background_tasks SET status='failed',updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow' AND status='completed'")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).execute(&mut *db).await?.rows_affected();
    if changed != 1 {
        return Err(closed());
    }
    approvals::audit(db, scope, decision, event).await?;
    Ok(())
}
