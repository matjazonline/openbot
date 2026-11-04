use super::*;
use crate::domain::workflow::{RunState, WaitingReason};

const STATES: [RunState; 6] = [
    RunState::Queued,
    RunState::Running,
    RunState::Waiting(WaitingReason::Timer),
    RunState::Succeeded,
    RunState::Failed,
    RunState::Cancelled,
];

async fn set_state(
    db: &mut PgConnection,
    scope: ActivationRequest,
    state: RunState,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE workflow_runs SET state=$3,waiting_reason=CASE WHEN $3='waiting' THEN 'timer' END,terminal_execution_id=CASE WHEN $3 IN ('succeeded','failed') THEN $4 END WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(state.stored_name()).bind(scope.execution.as_uuid()).execute(db).await?;
    Ok(())
}

#[tokio::test]
async fn run_transition_matrix_matches_domain() {
    for from in STATES {
        let (f, scope) = fixture().await;
        if from == RunState::Failed {
            failed(&f, scope, RetrySafety::SafeToRetry).await;
        } else {
            let mut tx = f.persistence().pool().begin().await.unwrap();
            set_state(&mut tx, scope, from).await.unwrap();
            tx.commit().await.unwrap();
        }
        for to in STATES {
            let mut tx = f.persistence().pool().begin().await.unwrap();
            if from == RunState::Failed && to == RunState::Running {
                sqlx::query("UPDATE background_tasks SET status='pending' WHERE id=$1")
                    .bind(scope.job.0)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
            }
            let result = set_state(&mut tx, scope, to).await;
            assert_eq!(
                result.is_ok(),
                from.can_transition_to(to),
                "{} -> {}: {result:?}",
                from.stored_name(),
                to.stored_name()
            );
            if let Err(error) = result {
                assert_eq!(
                    error.as_database_error().unwrap().constraint(),
                    Some("workflow_guard:guard_workflow_run_transition"),
                    "{} -> {}",
                    from.stored_name(),
                    to.stored_name()
                );
            }
            tx.rollback().await.unwrap();
        }
    }
}

#[tokio::test]
async fn run_transition_eligible_control_retry_commits_and_clears_terminal_id() {
    let (f, scope) = fixture().await;
    failed(&f, scope, RetrySafety::SafeToRetry).await;
    assert!(matches!(
        f.persistence()
            .retry(retry(command(&f, scope, "matrix-retry").await))
            .await
            .unwrap(),
        RetryResult::Applied { .. }
    ));
    let saved: (String, Option<Uuid>) =
        sqlx::query_as("SELECT state,terminal_execution_id FROM workflow_runs WHERE id=$1")
            .bind(scope.run.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(saved, ("running".into(), None));
}

#[tokio::test]
async fn run_transition_failed_to_running_without_retry_evidence_is_rejected() {
    let (f, scope) = fixture().await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    set_state(&mut tx, scope, RunState::Failed).await.unwrap();
    let error = set_state(&mut tx, scope, RunState::Running)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_guard:guard_workflow_run_transition")
    );
    let typed = AppError::from(error);
    assert!(
        matches!(typed,AppError::WorkflowGuard(ref v) if v.guard()=="guard_workflow_run_transition")
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn run_transition_cancelled_to_running_is_rejected() {
    let (f, scope) = fixture().await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    set_state(&mut tx, scope, RunState::Cancelled)
        .await
        .unwrap();
    let error = set_state(&mut tx, scope, RunState::Running)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_guard:guard_workflow_run_transition")
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn run_transition_terminal_execution_shape_is_required_exactly_for_success_and_failure() {
    let (f, scope) = fixture().await;
    for state in STATES {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let error=sqlx::query("UPDATE workflow_runs SET state=$2,waiting_reason=CASE WHEN $2='waiting' THEN 'timer' END,terminal_execution_id=CASE WHEN $2 NOT IN ('succeeded','failed') THEN $3 END WHERE id=$1")
            .bind(scope.run.as_uuid()).bind(state.stored_name()).bind(scope.execution.as_uuid()).execute(&mut *tx).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().constraint(),
            Some("workflow_run_terminal_execution_shape"),
            "{}",
            state.stored_name()
        );
        tx.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn run_transition_guard_fires_before_finished_and_state_witness_triggers() {
    let (f, _) = fixture().await;
    let names:Vec<String>=sqlx::query_scalar("SELECT tgname::text FROM pg_trigger WHERE tgrelid='workflow_runs'::regclass AND NOT tgisinternal AND (tgtype & 2)=2 AND (tgtype & 16)=16 ORDER BY tgname")
        .fetch_all(f.persistence().pool()).await.unwrap();
    assert_eq!(
        names.first().map(String::as_str),
        Some("guard_workflow_run_transition")
    );
    assert!(names.iter().any(|name| name == "workflow_run_finished"));
    assert!(
        names
            .iter()
            .any(|name| name == "workflow_action_capture_state_witness")
    );
}

#[tokio::test]
async fn workflow_lease_renew_update_itself_refuses_each_stale_fence_component() {
    let (f, scope) = fixture().await;
    let claim = f
        .persistence()
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let mut stale = Vec::new();
    let mut fence = claim.fence;
    fence.generation.0 = Uuid::new_v4();
    stale.push(fence);
    let mut fence = claim.fence;
    fence.worker = worker();
    stale.push(fence);
    let mut fence = claim.fence;
    fence.attempt.0 += 1;
    stale.push(fence);
    let mut fence = claim.fence;
    fence.scope.company = CompanyId::new(Uuid::new_v4());
    stale.push(fence);
    let mut fence = claim.fence;
    fence.scope.execution = ExecutionId::new(Uuid::new_v4());
    stale.push(fence);
    let mut fence = claim.fence;
    fence.scope.run = RunId::new(Uuid::new_v4());
    stale.push(fence);
    for fence in stale {
        let before = snapshot(&f).await;
        let mut tx = f.persistence().pool().begin().await.unwrap();
        assert!(
            !crate::adapters::persistence::workflow::lease::renew_owned(&mut tx, fence, policy())
                .await
                .unwrap()
        );
        tx.rollback().await.unwrap();
        assert_eq!(snapshot(&f).await, before);
    }
    let mut tx = f.persistence().pool().begin().await.unwrap();
    assert!(
        crate::adapters::persistence::workflow::lease::renew_owned(&mut tx, claim.fence, policy())
            .await
            .unwrap()
    );
    tx.rollback().await.unwrap();
}
