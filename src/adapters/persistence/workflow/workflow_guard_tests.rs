use super::*;
use crate::adapters::monitoring::in_memory_monitor::InMemoryMonitor;
use crate::application::app_error::WorkflowGuardViolation;
use crate::application::workflow::guard::*;
use crate::domain::monitoring::MonitoringService;

fn violation() -> WorkflowGuardViolation {
    WorkflowGuardViolation::from_constraint("workflow_guard:test_forced_guard", "forced guard")
        .unwrap()
}

async fn inject(pool: &sqlx::PgPool, table: &str, condition: &str, code: &str) {
    let constraint = if code == "23514" {
        ", CONSTRAINT='workflow_guard:test_forced_guard'"
    } else {
        ""
    };
    sqlx::raw_sql(&format!("CREATE FUNCTION p03_forced_guard() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF {condition} THEN RAISE EXCEPTION 'forced guard' USING ERRCODE='{code}'{constraint}; END IF; RETURN NEW; END $$; CREATE CONSTRAINT TRIGGER p03_forced_guard AFTER UPDATE ON {table} DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION p03_forced_guard()"))
        .execute(pool).await.unwrap();
}

async fn no_work(p: &PostgresPersistence, run: RunId) {
    for _ in 0..2 {
        assert!(
            !p.poll_work(None, 128)
                .await
                .unwrap()
                .candidates
                .iter()
                .any(|c| c.scope.run == run)
        );
    }
}

async fn assert_failed(f: &AdmissionFixture, scope: ActivationRequest, expected_job: &str) {
    let state: (String, Option<Uuid>) =
        sqlx::query_as("SELECT state,terminal_execution_id FROM workflow_runs WHERE id=$1")
            .bind(scope.run.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(state, ("failed".into(), Some(scope.execution.as_uuid())));
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_run_events WHERE run_id=$1 AND event_kind='guard_violation:test_forced_guard'")
        .bind(scope.run.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(events, 1);
    let job: String = sqlx::query_scalar("SELECT status FROM background_tasks WHERE id=$1")
        .bind(scope.job.0)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(job, expected_job);
    no_work(f.persistence(), scope.run).await;
}

fn metric(monitor: &InMemoryMonitor, site: &str) {
    let stats = monitor.get_stats_json();
    assert_eq!(
        stats["custom_counters"]["workflow_guard_violations_total"],
        1
    );
    assert_eq!(
        stats["custom_counters"][format!(
            "workflow_guard_violations_total{{guard=test_forced_guard,site={site},outcome=failed}}"
        )],
        1
    );
}

#[tokio::test]
async fn workflow_guard_error_pure_step_fails_once_and_stops_polling() {
    let source = serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    let (f, scope) = fixture_source(source).await;
    inject(
        f.persistence().pool(),
        "background_tasks",
        "NEW.status='completed'",
        "23514",
    )
    .await;
    let handler = scripted();
    let monitor = InMemoryMonitor::new();
    let mut worker = polling(f.persistence(), &handler);
    worker.monitoring = Some(&monitor);
    let candidate = f.persistence().poll_work(None, 1).await.unwrap().candidates[0];
    worker
        .process(candidate, &CancellationToken::new())
        .await
        .unwrap();
    assert_failed(&f, scope, "failed").await;
    assert!(matches!(
        worker.process(candidate, &CancellationToken::new()).await,
        Err(AppError::Conflict(_))
    ));
    assert_failed(&f, scope, "failed").await;
    assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
    metric(&monitor, "step");
}

#[tokio::test]
async fn workflow_guard_error_io_completion_never_reexecutes_the_handler() {
    let (f, scope) = fixture().await;
    inject(
        f.persistence().pool(),
        "background_tasks",
        "NEW.status='completed'",
        "23514",
    )
    .await;
    let handler = scripted();
    let monitor = InMemoryMonitor::new();
    let mut worker = polling(f.persistence(), &handler);
    worker.monitoring = Some(&monitor);
    let candidate = f.persistence().poll_work(None, 1).await.unwrap().candidates[0];
    worker
        .process(candidate, &CancellationToken::new())
        .await
        .unwrap();
    assert_failed(&f, scope, "failed").await;
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    let attempts: Vec<(String,Option<String>,Option<String>)> = sqlx::query_as("SELECT status,workflow_retirement,workflow_failure_code FROM task_attempts WHERE task_id=$1")
        .bind(scope.job.0).fetch_all(f.persistence().pool()).await.unwrap();
    assert_eq!(
        attempts,
        [(
            "failed".into(),
            Some("guard".into()),
            Some("workflow.guard_violation".into())
        )]
    );
    metric(&monitor, "step");
}

#[tokio::test]
async fn workflow_guard_error_run_expiry_handles_processing_expired_lease() {
    let (f, scope) = fixture().await;
    f.persistence()
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    expire(&f, scope).await;
    sqlx::query("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '2 minutes',deadline=clock_timestamp()-interval '1 minute' WHERE id=$1")
        .bind(scope.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    inject(
        f.persistence().pool(),
        "task_attempts",
        "NEW.workflow_retirement='deadline'",
        "23514",
    )
    .await;
    let handler = scripted();
    let monitor = InMemoryMonitor::new();
    let mut worker = polling(f.persistence(), &handler);
    worker.monitoring = Some(&monitor);
    let candidate = f.persistence().poll_work(None, 1).await.unwrap().candidates[0];
    assert_eq!(candidate.work, PollWork::ExpiredRun);
    worker
        .process(candidate, &CancellationToken::new())
        .await
        .unwrap();
    assert_failed(&f, scope, "failed").await;
    metric(&monitor, "run_expiry");
}

#[tokio::test]
async fn workflow_guard_error_decision_timeout_fails_instead_of_deferring_to_itself() {
    use super::super::super::decision_tests::DecisionFixture;
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    f.park().await;
    sqlx::query("UPDATE workflow_waits SET created_at=clock_timestamp()-interval '2 seconds',deadline=clock_timestamp()-interval '1 second' WHERE run_id=$1 AND state='waiting'")
        .bind(f.scope.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    inject(
        f.persistence().pool(),
        "workflow_waits",
        "NEW.state='expired'",
        "23514",
    )
    .await;
    let handler = scripted();
    let worker = polling(f.persistence(), &handler);
    let candidate = f.persistence().poll_work(None, 1).await.unwrap().candidates[0];
    assert_eq!(candidate.work, PollWork::Wait);
    worker
        .process(candidate, &CancellationToken::new())
        .await
        .unwrap();
    assert_failed(&f.admission, f.scope, "completed").await;
}

#[tokio::test]
async fn workflow_guard_error_ordinary_connection_error_is_retried() {
    let source = serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    let (f, scope) = fixture_source(source).await;
    inject(
        f.persistence().pool(),
        "background_tasks",
        "NEW.status='completed'",
        "08006",
    )
    .await;
    let handler = scripted();
    let worker = polling(f.persistence(), &handler);
    let candidate = f.persistence().poll_work(None, 1).await.unwrap().candidates[0];
    assert!(matches!(
        worker.process(candidate, &CancellationToken::new()).await,
        Err(AppError::Database(_))
    ));
    assert_eq!(run_state(f.persistence(), scope).await, "queued");
    sqlx::query("DROP TRIGGER p03_forced_guard ON background_tasks")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    worker
        .process(candidate, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(run_state(f.persistence(), scope).await, "succeeded");
    no_work(f.persistence(), scope.run).await;
}

#[tokio::test]
async fn workflow_guard_error_retirement_first_keeps_the_competing_owners_result() {
    let (f, scope) = fixture().await;
    let claim = f
        .persistence()
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    expire(&f, scope).await;
    f.persistence()
        .settled()
        .retire_expired_io(scope, policy())
        .await
        .unwrap();
    let before = snapshot(&f).await;
    let result = f
        .persistence()
        .fail_run_on_guard(GuardFailure {
            scope,
            violation: violation(),
            site: GuardSite::Step,
            fence: Some(claim.fence),
        })
        .await
        .unwrap();
    assert_eq!(result, GuardFailureOutcome::Deferred);
    assert_eq!(snapshot(&f).await, before);
}

#[tokio::test]
async fn workflow_guard_error_cancel_and_guard_failure_compete_for_one_terminal_run() {
    let (f, scope) = fixture().await;
    let command = super::super::control_tests::command(&f, scope, "cancel-guard-race").await;
    let barrier = tokio::sync::Barrier::new(3);
    let fail = async {
        barrier.wait().await;
        f.persistence()
            .fail_run_on_guard(GuardFailure {
                scope,
                violation: violation(),
                site: GuardSite::Step,
                fence: None,
            })
            .await
            .unwrap()
    };
    let cancel = async {
        barrier.wait().await;
        f.persistence().cancel(command).await.unwrap()
    };
    let (failure, _, ()) = tokio::join!(fail, cancel, async {
        barrier.wait().await;
    });
    let state = run_state(f.persistence(), scope).await;
    assert!(matches!(
        (failure, state.as_str()),
        (GuardFailureOutcome::Failed, "failed") | (GuardFailureOutcome::Inactive, "cancelled")
    ));
    let snapshot = snapshot(&f).await;
    assert_eq!(snapshot["jobs"][0]["status"], "failed");
    no_work(f.persistence(), scope.run).await;
}

#[tokio::test]
async fn workflow_guard_error_pending_job_is_never_control_retry_safe() {
    let (f, scope) = fixture().await;
    let claim = f
        .persistence()
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let failure = crate::application::workflow::lease::WorkflowFailure {
        failure: crate::domain::workflow::StepFailure::new(
            crate::domain::workflow::FailureClass::Retryable,
            crate::domain::workflow::FailureCode::parse("provider.unavailable").unwrap(),
            None,
        )
        .unwrap(),
        safety: crate::domain::workflow::RetrySafety::SafeToRetry,
    };
    f.persistence()
        .settled()
        .release_io(
            claim.fence,
            policy(),
            LeaseReleaseCause::Classified(failure),
        )
        .await
        .unwrap();
    assert_eq!(
        f.persistence()
            .fail_run_on_guard(GuardFailure {
                scope,
                violation: violation(),
                site: GuardSite::Step,
                fence: None
            })
            .await
            .unwrap(),
        GuardFailureOutcome::Failed
    );
    let safe: bool = sqlx::query_scalar("SELECT workflow_control_retry_safe($1,$2,$3)")
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .bind(scope.job.0)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert!(!safe);
    assert_failed(&f, scope, "failed").await;
}

#[tokio::test]
async fn workflow_guard_error_parent_failure_cancels_its_live_child_subtree() {
    let source = serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    let (f, scopes) = AdmissionFixture::chain(source, 2).await;
    let scope = scopes[0];
    assert_eq!(
        f.persistence()
            .fail_run_on_guard(GuardFailure {
                scope,
                violation: violation(),
                site: GuardSite::Step,
                fence: None
            })
            .await
            .unwrap(),
        GuardFailureOutcome::Failed
    );
    assert_failed(&f, scope, "completed").await;
    for child in &scopes[1..] {
        assert_eq!(run_state(f.persistence(), *child).await, "cancelled");
    }
}

#[tokio::test]
async fn workflow_guard_error_transient_completion_confirmation_keeps_one_handler_execution() {
    let (f, scope) = fixture().await;
    sqlx::query("CREATE SEQUENCE p03_confirmation_attempt")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    inject(
        f.persistence().pool(),
        "background_tasks",
        "NEW.status='completed' AND nextval('p03_confirmation_attempt')=1",
        "23514",
    )
    .await;
    let handler = scripted();
    let monitor = InMemoryMonitor::new();
    let mut worker = polling(f.persistence(), &handler);
    worker.monitoring = Some(&monitor);
    let candidate = f.persistence().poll_work(None, 1).await.unwrap().candidates[0];
    worker
        .process(candidate, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(run_state(f.persistence(), scope).await, "succeeded");
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM task_attempts WHERE task_id=$1")
        .bind(scope.job.0)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(attempts, 1);
    assert!(
        monitor.get_stats_json()["custom_counters"]
            .get("workflow_guard_violations_total")
            .is_none()
    );
    let guards:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_run_events WHERE run_id=$1 AND event_kind LIKE 'guard_violation:%'").bind(scope.run.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(guards, 0);
    no_work(f.persistence(), scope.run).await;
}

#[path = "workflow_guard_compensation_tests.rs"]
mod compensation_tests;
