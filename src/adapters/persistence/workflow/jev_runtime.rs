//! Exact Jev request admission and response accounting commit under the workflow fence.
use super::{
    persistence_bound::{Attempt, End, WriteBound},
    *,
};
use crate::application::workflow::{actions::ActionScope, budget::*, jev::*, lease::*};
use crate::domain::workflow::{BudgetCharge, BudgetResource};
use serde_json::Value;
use std::time::Duration;

#[derive(sqlx::FromRow)]
struct Saved {
    prepared: Vec<u8>,
    digest: Vec<u8>,
    output: Option<Value>,
}
#[async_trait]
impl JevExecutions for PostgresPersistence {
    async fn replay_jev(
        &self,
        claim: &ClaimedWorkflow,
        request: &JevRequest,
        policy: LeasePolicy,
    ) -> AppResult<Option<WorkflowHandlerResult>> {
        let bound = WriteBound::start("jev.replay", policy);
        bound.read(Box::pin(async {
            let mut conn = bound.acquire(&self.pool).await?;
            let mut tx = bound.begin(&mut conn).await?;
            lock_owner(&mut tx, claim, &bound).await?;
            let saved: Option<ReplaySaved> = sqlx::query_as("SELECT prepared,digest,output,failure_code,retryable FROM workflow_jev_invocations WHERE company_id=$1 AND execution_id=$2 AND (output IS NOT NULL OR retryable=false) ORDER BY created_at LIMIT 1")
                .bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.execution.as_uuid()).fetch_optional(&mut *tx).await?;
            let output = saved.map(|saved| replay(request, saved)).transpose()?;
            live(&mut tx, claim, &bound).await?;
            bound.roll_back(tx, Ok(output)).await
        })).await
    }
    async fn admit_jev(
        &self,
        claim: &ClaimedWorkflow,
        request: &JevRequest,
        policy: LeasePolicy,
    ) -> AppResult<JevAdmissionResult> {
        let bound = WriteBound::start("jev.admit", policy);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = admit_on(self, &mut tx, claim, request, &bound).await;
                bound.end(tx, result).await
            }))
            .await;
        finish(attempt, &bound)
    }
    async fn settle_jev(
        &self,
        claim: &ClaimedWorkflow,
        admission: &JevAdmission,
        request: &JevRequest,
        response: &JevResponse,
        latency: Duration,
        policy: LeasePolicy,
    ) -> AppResult<()> {
        let bound = WriteBound::start("jev.settle", policy);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = settle_on(
                    &mut tx, claim, admission, request, response, latency, &bound,
                )
                .await
                .map(End::Commit);
                bound.end(tx, result).await
            }))
            .await;
        finish(attempt, &bound)
    }
    async fn fail_jev(
        &self,
        claim: &ClaimedWorkflow,
        admission: &JevAdmission,
        failure: JevError,
        latency: Duration,
        policy: LeasePolicy,
    ) -> AppResult<()> {
        let bound = WriteBound::start("jev.failure", policy);
        let attempt = bound.attempt(Box::pin(async {
            let mut conn = bound.acquire(&self.pool).await?;
            let mut tx = bound.begin(&mut conn).await?;
            let result = async {
                lock_owner(&mut tx, claim, &bound).await?;
                let changed = sqlx::query("UPDATE workflow_jev_invocations SET failure_code=$6,retryable=$7,latency_ms=$8,completed_at=clock_timestamp(),retry_not_before=CASE WHEN $9::bigint IS NULL THEN NULL ELSE clock_timestamp()+$9*interval '1 second' END WHERE company_id=$1 AND request_id=$2 AND generation=$3 AND worker_id=$4 AND attempt=$5 AND completed_at IS NULL AND execution_id=$10")
                    .bind(claim.fence.scope.company.as_uuid()).bind(admission.request_id()).bind(claim.fence.generation.0).bind(claim.fence.worker.0).bind(claim.fence.attempt.0)
                    .bind(failure.code()).bind(failure.retryable()).bind(latency_ms(latency)).bind(retry_delay(failure)).bind(claim.fence.scope.execution.as_uuid()).execute(&mut *tx).await?.rows_affected();
                if changed != 1 { return Err(invalid()); }
                live(&mut tx, claim, &bound).await?;
                Ok(End::Commit(()))
            }.await;
            bound.end(tx, result).await
        })).await;
        finish(attempt, &bound)
    }
}
fn finish<T>(attempt: Attempt<T>, bound: &WriteBound) -> AppResult<T> {
    match attempt {
        Attempt::Done(value) => Ok(value),
        Attempt::Failed(error) => Err(error),
        Attempt::Ambiguous => Err(bound.unresolved()),
    }
}
fn latency_ms(latency: Duration) -> i64 {
    latency.as_millis().min(3_600_000) as i64
}
fn matching(request: &JevRequest, saved: Saved) -> AppResult<Option<Value>> {
    if saved.prepared != request.identity() || saved.digest != request.digest() {
        return Err(AppError::BadRequest("Jev preparation changed".into()));
    }
    saved
        .output
        .map(|value| {
            request
                .validate_response(value)
                .map(|response| response.value)
                .map_err(|_| invalid())
        })
        .transpose()
}
async fn live(tx: &mut PgConnection, claim: &ClaimedWorkflow, bound: &WriteBound) -> AppResult<()> {
    if lease::live_window(tx, claim.fence, bound.started())
        .await?
        .is_none()
    {
        return Err(AppError::Execution(
            crate::app_error::ExecutionFailure::OwnershipLost,
        ));
    }
    Ok(())
}
async fn lock_owner(
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    bound: &WriteBound,
) -> AppResult<()> {
    let scope = claim.fence.scope;
    let actor = action_authority::prelock_on(
        tx,
        ActionScope {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
        },
    )
    .await?
    .ok_or_else(invalid)?;
    if !lease::lock_fence(tx, claim.fence, bound.server_timeout()).await? {
        return Err(AppError::Execution(
            crate::app_error::ExecutionFailure::OwnershipLost,
        ));
    }
    let current: Uuid =
        sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(&mut **tx)
            .await?;
    if current != actor.user_id() {
        return Err(jev::revoked());
    }
    live(tx, claim, bound).await
}
async fn admit_on(
    p: &PostgresPersistence,
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    request: &JevRequest,
    bound: &WriteBound,
) -> AppResult<End<JevAdmissionResult>> {
    // Reuse current authority+selected connection checks before local fence and root locks.
    let credential = jev::runtime::authorize_on(p, tx, claim).await?;
    if credential.model != *request.model()
        || !request.ready()
        || JevRequest::prepare(&claim.activation.inputs)?.identity() != request.identity()
    {
        return Err(jev::revoked());
    }
    let scope = claim.fence.scope;
    let saved: Option<Saved> = sqlx::query_as("SELECT prepared,digest,output FROM workflow_jev_invocations WHERE company_id=$1 AND execution_id=$2 ORDER BY created_at LIMIT 1")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).fetch_optional(&mut **tx).await?;
    if let Some(saved) = saved {
        matching(request, saved)?;
    }
    let admitted: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_jev_invocations WHERE company_id=$1 AND execution_id=$2 AND (generation=$3 OR output IS NOT NULL OR retryable=false))")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).bind(claim.fence.generation.0).fetch_one(&mut **tx).await?;
    if admitted {
        return Ok(End::Rollback(JevAdmissionResult::AlreadyAdmitted));
    }
    let delay: i64 = sqlx::query_scalar("SELECT COALESCE(GREATEST(ceil(extract(epoch FROM max(retry_not_before)-clock_timestamp())),0),0)::bigint FROM workflow_jev_invocations WHERE company_id=$1 AND execution_id=$2")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(&mut **tx).await?;
    if delay > 0 {
        return Ok(End::Rollback(JevAdmissionResult::Deferred(
            Duration::from_secs(delay.min(60) as u64),
        )));
    }
    let root = token_budget::lock_on(tx, claim.fence).await?;
    if root.refusal.is_some()
        || root.requires_approval((request.reserved_input() + request.reserved_output()) as u64)?
    {
        return Ok(End::Rollback(JevAdmissionResult::TokenLimit));
    }
    let request_id = Uuid::new_v4();
    let receipt = BudgetReservation::new(
        claim.fence,
        BudgetReservationKey::parse(format!("jev.{request_id}"))?,
        BudgetCharge::new(BudgetResource::ModelCall, 1).map_err(|_| invalid())?,
    )?;
    match budget::reserve_on(tx, &receipt, bound.server_timeout()).await? {
        BudgetReservationResult::Recorded(BudgetDisposition::Granted) => {}
        BudgetReservationResult::Recorded(BudgetDisposition::Exhausted) => {
            return Ok(End::Commit(JevAdmissionResult::BudgetExhausted));
        }
        _ => return Err(invalid()),
    }
    reserve_usage(tx, claim, request_id, request).await?;
    sqlx::query("INSERT INTO workflow_jev_invocations(company_id,run_id,execution_id,request_id,generation,worker_id,attempt,prepared,digest) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(request_id).bind(claim.fence.generation.0).bind(claim.fence.worker.0).bind(claim.fence.attempt.0).bind(request.identity()).bind(request.digest()).execute(&mut **tx).await?;
    live(tx, claim, bound).await?;
    Ok(End::Commit(JevAdmissionResult::New(JevAdmission {
        request_id,
    })))
}
async fn reserve_usage(
    tx: &mut PgConnection,
    claim: &ClaimedWorkflow,
    id: Uuid,
    request: &JevRequest,
) -> AppResult<()> {
    let scope = claim.fence.scope;
    let changed = sqlx::query("INSERT INTO model_usage_calls(company_id,request_id,task_id,agent_id,provider,model,reserved_input,reserved_output,workflow_id,version_id,run_id,execution_id,root_run_id)
        SELECT run.company_id,$2,$3,NULL,'typesafe',$4,$5,$6,run.workflow_id,run.version_id,run.id,$7,budget.root_run_id FROM workflow_runs AS run JOIN workflow_run_budgets AS budget ON budget.company_id=run.company_id AND budget.run_id=run.id WHERE run.company_id=$1 AND run.id=$8")
        .bind(scope.company.as_uuid()).bind(id).bind(scope.job.0).bind(request.model().as_str()).bind(request.reserved_input()).bind(request.reserved_output()).bind(scope.execution.as_uuid()).bind(scope.run.as_uuid()).execute(tx).await?.rows_affected();
    if changed != 1 {
        return Err(invalid());
    }
    Ok(())
}
async fn settle_on(
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    admission: &JevAdmission,
    request: &JevRequest,
    response: &JevResponse,
    latency: Duration,
    bound: &WriteBound,
) -> AppResult<()> {
    lock_owner(tx, claim, bound).await?;
    let saved: Saved = sqlx::query_as("SELECT prepared,digest,output FROM workflow_jev_invocations WHERE company_id=$1 AND request_id=$2 AND execution_id=$3 AND generation=$4 AND worker_id=$5 AND attempt=$6 FOR UPDATE")
        .bind(claim.fence.scope.company.as_uuid()).bind(admission.request_id()).bind(claim.fence.scope.execution.as_uuid()).bind(claim.fence.generation.0).bind(claim.fence.worker.0).bind(claim.fence.attempt.0).fetch_optional(&mut **tx).await?.ok_or_else(invalid)?;
    if !request.ready()
        || JevRequest::prepare(&claim.activation.inputs)?.identity() != request.identity()
    {
        return Err(invalid());
    }
    let prior = matching(request, saved)?;
    let validated = request
        .validate_response(response.value.clone())
        .map_err(|_| invalid())?;
    if validated.input_tokens != response.input_tokens
        || validated.output_tokens != response.output_tokens
    {
        return Err(invalid());
    }
    if let Some(prior) = prior {
        return if prior == response.value {
            Ok(())
        } else {
            Err(invalid())
        };
    }
    token_budget::lock_on(tx, claim.fence).await?;
    let changed = sqlx::query("UPDATE workflow_jev_invocations SET output=$3,latency_ms=$4,completed_at=clock_timestamp() WHERE company_id=$1 AND request_id=$2 AND completed_at IS NULL")
        .bind(claim.fence.scope.company.as_uuid()).bind(admission.request_id()).bind(&response.value).bind(latency_ms(latency)).execute(&mut **tx).await?.rows_affected();
    if changed != 1 {
        return Err(invalid());
    }
    let changed = sqlx::query("UPDATE model_usage_calls SET input_tokens=$3,output_tokens=$4,usage_source='reported',settled_at=clock_timestamp() WHERE company_id=$1 AND request_id=$2 AND settled_at IS NULL")
        .bind(claim.fence.scope.company.as_uuid()).bind(admission.request_id()).bind(response.input_tokens).bind(response.output_tokens).execute(&mut **tx).await?.rows_affected();
    if changed != 1 {
        return Err(invalid());
    }
    live(tx, claim, bound).await
}

#[derive(sqlx::FromRow)]
struct ReplaySaved {
    prepared: Vec<u8>,
    digest: Vec<u8>,
    output: Option<Value>,
    failure_code: Option<String>,
    retryable: Option<bool>,
}
fn replay(request: &JevRequest, saved: ReplaySaved) -> AppResult<WorkflowHandlerResult> {
    let output = matching(
        request,
        Saved {
            prepared: saved.prepared,
            digest: saved.digest,
            output: saved.output,
        },
    )?;
    if let Some(value) = output {
        return Ok(Ok(value));
    }
    if saved.retryable != Some(false) {
        return Err(invalid());
    }
    use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};
    Ok(Err(WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Terminal,
            FailureCode::parse(saved.failure_code.ok_or_else(invalid)?).map_err(|_| invalid())?,
            None,
        )
        .map_err(|_| invalid())?,
        safety: RetrySafety::SafeToRetry,
    }))
}

fn retry_delay(failure: JevError) -> Option<i64> {
    match failure {
        JevError::Http(_, Some(delay)) if failure.retryable() => {
            Some(delay.as_secs().min(60) as i64)
        }
        _ => None,
    }
}
