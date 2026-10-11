//! Receipts are accounting facts; jobs and attempts retain exclusive ownership.
use super::persistence_bound::{Attempt, End, Evidence, WriteBound};
use super::*;
use crate::application::workflow::{budget::*, lease::*};
use tokio::time::Instant;

#[async_trait]
impl WorkflowBudgets for PostgresPersistence {
    async fn reserve_budget(
        &self,
        request: BudgetReservation,
        policy: LeasePolicy,
    ) -> AppResult<BudgetReservationResult> {
        let bound = WriteBound::start("budget.reserve", policy);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = reserve_on(&mut tx, &request, bound.server_timeout()).await;
                bound.end(tx, result.map(End::Commit)).await
            }))
            .await;
        match attempt {
            Attempt::Done(result) => Ok(result),
            Attempt::Failed(error) => Err(error),
            // The scoped receipt is the evidence. Replayed carries no reauthorization, so
            // it is the conservative answer whether or not this call wrote it.
            Attempt::Ambiguous => {
                bound
                    .resolve(Box::pin(async {
                        let mut conn = bound.acquire(&self.pool).await?;
                        let mut tx = bound.begin(&mut conn).await?;
                        let saved = async {
                            lock_run(&mut tx, &request).await?;
                            saved(&mut tx, &request).await
                        }
                        .await;
                        let saved = bound.roll_back(tx, saved).await?;
                        Ok(saved.map_or(Evidence::NotApplied, |disposition| {
                            Evidence::Applied(BudgetReservationResult::Replayed(disposition))
                        }))
                    }))
                    .await
            }
        }
    }
}

/// `server` is the lock and statement bound the fence check sets: what remains of the
/// caller's own client bound.
pub(super) async fn reserve_on(
    db: &mut PgConnection,
    request: &BudgetReservation,
    server: std::time::Duration,
) -> AppResult<BudgetReservationResult> {
    // Scope verification precedes replay, even for terminal owners. Lock only
    // this run; the receipt trigger then locks the shared usage, never its root.
    let scoped = lock_run(db, request).await?;
    super::persistence_bound::locks_held();
    if scoped.is_none() {
        return Ok(BudgetReservationResult::NotOwned);
    }
    if let Some(saved) = saved(db, request).await? {
        return Ok(BudgetReservationResult::Replayed(saved));
    }
    if !lease::lock_fence(db, request.fence, server).await?
        || lease::live_window(db, request.fence, Instant::now())
            .await?
            .is_none()
    {
        return Ok(BudgetReservationResult::NotOwned);
    }
    reserve_fenced(db, request).await
}

async fn lock_run(db: &mut PgConnection, request: &BudgetReservation) -> AppResult<Option<Uuid>> {
    let scope = request.fence.scope;
    let scoped: Option<Uuid> = sqlx::query_scalar("SELECT run.id FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$2 AND EXISTS (SELECT 1 FROM workflow_executions AS execution JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id WHERE execution.company_id=run.company_id AND execution.run_id=run.id AND execution.id=$3 AND job.id=$4 AND job.queue_kind='workflow') FOR UPDATE")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0).fetch_optional(&mut *db).await?;
    Ok(scoped)
}

async fn reserve_fenced(
    db: &mut PgConnection,
    request: &BudgetReservation,
) -> AppResult<BudgetReservationResult> {
    let scope = request.fence.scope;
    let result: String = sqlx::query_scalar("INSERT INTO workflow_budget_receipts(company_id,run_id,execution_id,root_run_id,resource,reservation_key,quantity,disposition) SELECT company_id,run_id,$3,root_run_id,$4,$5,$6,'granted' FROM workflow_run_budgets WHERE company_id=$1 AND run_id=$2 RETURNING disposition")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(resource(request.charge().resource())).bind(request.key.as_str()).bind(i64::from(request.charge().quantity())).fetch_one(&mut *db).await?;
    // Waiting for a sibling's usage lock can consume the lease. Roll back the
    // whole transaction if it did: neither debit nor receipt may survive.
    if lease::live_window(db, request.fence, Instant::now())
        .await?
        .is_none()
    {
        return Err(super::persistence_bound::refused("budget reservation"));
    }
    let disposition = disposition(&result)?;
    if disposition == BudgetDisposition::Exhausted {
        Box::pin(recovery::retire_on(
            db,
            request.fence,
            lease::Retirement::BudgetExhausted,
        ))
        .await?;
        waits::audit(db, scope, "workflow.root_budget_exhausted").await?;
    }
    Ok(BudgetReservationResult::Recorded(disposition))
}

#[derive(sqlx::FromRow)]
struct Saved {
    quantity: i32,
    disposition: String,
}

async fn saved(
    db: &mut PgConnection,
    request: &BudgetReservation,
) -> AppResult<Option<BudgetDisposition>> {
    let scope = request.fence.scope;
    let saved = sqlx::query_as::<_, Saved>("SELECT quantity,disposition FROM workflow_budget_receipts WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND resource=$4 AND reservation_key=$5")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(resource(request.charge().resource())).bind(request.key.as_str()).fetch_optional(db).await?;
    saved
        .map(|saved| {
            if i64::from(saved.quantity) != i64::from(request.charge().quantity()) {
                return Err(AppError::Conflict(
                    "Workflow budget reservation payload conflicts".into(),
                ));
            }
            disposition(&saved.disposition)
        })
        .transpose()
}

fn disposition(value: &str) -> AppResult<BudgetDisposition> {
    match value {
        "granted" => Ok(BudgetDisposition::Granted),
        "exhausted" => Ok(BudgetDisposition::Exhausted),
        _ => Err(invalid()),
    }
}

fn resource(value: BudgetResource) -> &'static str {
    match value {
        BudgetResource::Activation => "activation",
        BudgetResource::ModelCall => "model_call",
        BudgetResource::Repetition => "repetition",
    }
}

/// A first activation has no attempt and cannot have dispatched an effect. The
/// caller holds its run and execution; SQL serializes only the shared usage row.
pub(super) async fn activate(
    db: &mut PgConnection,
    scope: crate::application::workflow::activation::ActivationRequest,
) -> AppResult<bool> {
    let pending: bool = sqlx::query_scalar("SELECT status='pending' AND worker_id IS NULL AND execution_generation IS NULL AND lock_expires_at IS NULL FROM background_tasks WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow'")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).fetch_one(&mut *db).await?;
    if !pending {
        return Err(AppError::Conflict(
            "Workflow budget activation refused: job is no longer pending".into(),
        ));
    }
    let result: String = sqlx::query_scalar("INSERT INTO workflow_budget_receipts(company_id,run_id,execution_id,root_run_id,resource,reservation_key,quantity,disposition) SELECT company_id,run_id,$3,root_run_id,'activation','activation',1,'granted' FROM workflow_run_budgets WHERE company_id=$1 AND run_id=$2 ON CONFLICT(company_id,run_id,execution_id,resource,reservation_key) DO UPDATE SET quantity=EXCLUDED.quantity RETURNING disposition")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(&mut *db).await?;
    // The shared usage lock can wait past the deadline. Propagation rolls back
    // both the receipt and debit, including an exhausted disposition.
    let live: bool = sqlx::query_scalar("SELECT state IN ('queued','running') AND deadline > clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut *db).await?;
    if !live {
        return Err(super::persistence_bound::refused("budget activation"));
    }
    if disposition(&result)? == BudgetDisposition::Exhausted {
        // Retirement also supports error routes that read saved activations;
        // box this seam to bound the async cycle and its debug-stack footprint.
        Box::pin(pending_recovery::settle(
            db,
            scope,
            pending_recovery::PendingFailure::RootBudgetExhausted,
        ))
        .await?;
        return Ok(false);
    }
    Ok(true)
}
