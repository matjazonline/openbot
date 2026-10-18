//! Effective retirement state of one claimed action step, after the handler's failure has
//! been released through the real lease/retirement writer (the shared predicate applies).
use super::*;
use crate::application::workflow::lease::{
    ClaimedWorkflow, LeasePolicy, LeaseReleaseCause, WorkflowFailure, WorkflowLeases,
};

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
pub(super) struct Retired {
    pub(super) run_state: String,
    pub(super) waiting_reason: Option<String>,
    pub(super) code: Option<String>,
    pub(super) class: Option<String>,
    pub(super) safety: Option<String>,
    pub(super) reconciliations: i64,
}
impl Retired {
    pub(super) fn new(
        run_state: &str,
        waiting_reason: Option<&str>,
        code: &str,
        class: &str,
        safety: &str,
        reconciliations: i64,
    ) -> Self {
        Self {
            run_state: run_state.into(),
            waiting_reason: waiting_reason.map(Into::into),
            code: Some(code.into()),
            class: Some(class.into()),
            safety: Some(safety.into()),
            reconciliations,
        }
    }
}

pub(super) async fn release(
    persistence: &PostgresPersistence,
    claim: &ClaimedWorkflow,
    lease: LeasePolicy,
    cause: LeaseReleaseCause,
) {
    assert!(
        persistence
            .release_io(claim.fence, lease, cause)
            .await
            .unwrap()
    );
}

pub(super) async fn retire(
    persistence: &PostgresPersistence,
    claim: &ClaimedWorkflow,
    lease: LeasePolicy,
    failure: WorkflowFailure,
) -> Retired {
    release(
        persistence,
        claim,
        lease,
        LeaseReleaseCause::Classified(failure),
    )
    .await;
    retired(persistence, claim).await
}

pub(super) async fn retired(persistence: &PostgresPersistence, claim: &ClaimedWorkflow) -> Retired {
    let scope = claim.fence.scope;
    sqlx::query_as(
        "SELECT run.state AS run_state, run.waiting_reason, attempt.workflow_failure_code AS code,
                attempt.workflow_failure_class AS class, attempt.workflow_retry_safety AS safety,
                (SELECT count(*) FROM workflow_action_reconciliations AS fact
                  WHERE fact.company_id=run.company_id AND fact.run_id=run.id) AS reconciliations
         FROM workflow_runs AS run
         JOIN task_attempts AS attempt ON attempt.task_id=$3 AND attempt.attempt_number=$4
         WHERE run.company_id=$1 AND run.id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.job.0)
    .bind(claim.fence.attempt.0)
    .fetch_one(persistence.pool())
    .await
    .unwrap()
}

/// Existing completion tests must fail if a handler unexpectedly parks.
pub(super) trait CompletedOutcome {
    fn completed_result(self) -> crate::application::workflow::lease::WorkflowHandlerResult;
}

impl CompletedOutcome for crate::application::workflow::lease::WorkflowHandlerOutcome {
    fn completed_result(self) -> crate::application::workflow::lease::WorkflowHandlerResult {
        use crate::application::workflow::lease::WorkflowHandlerOutcome;
        match self {
            WorkflowHandlerOutcome::Completed(result) => result,
            WorkflowHandlerOutcome::Suspended(wait) => {
                panic!("unexpected suspension: {:?}", wait.wait)
            }
        }
    }
}
