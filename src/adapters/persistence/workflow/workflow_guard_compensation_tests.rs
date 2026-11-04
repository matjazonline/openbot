//! A failure while compensating IO must not enter candidate confirmation again.
use super::*;

struct CountCandidates<'a> {
    handler: &'a dyn WorkflowHandler,
    entries: AtomicUsize,
}

#[async_trait::async_trait]
impl WorkflowHandler for CountCandidates<'_> {
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        self.entries.fetch_add(1, Ordering::SeqCst);
        self.handler.supports(kind)
    }

    async fn execute(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome {
        self.handler.execute(kind, claim).await
    }
}

#[derive(Clone, Copy)]
enum Cause {
    Guard,
    Connection,
}

impl Cause {
    fn refusal(self) -> &'static str {
        match self {
            Self::Guard => {
                "RAISE EXCEPTION 'compensation refused' USING ERRCODE='23514', CONSTRAINT='workflow_guard:test_compensation_guard';"
            }
            Self::Connection => "RAISE EXCEPTION 'compensation refused' USING ERRCODE='08006';",
        }
    }
}

async fn compensation_failure(cause: Cause) {
    let (f, scope) = fixture().await;
    inject(
        f.persistence().pool(),
        "background_tasks",
        "NEW.status='completed'",
        "23514",
    )
    .await;
    sqlx::raw_sql(&format!("CREATE FUNCTION p03_compensation_refusal() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.status='failed' THEN {} END IF; RETURN NEW; END $$; CREATE CONSTRAINT TRIGGER p03_compensation_refusal AFTER UPDATE ON background_tasks DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION p03_compensation_refusal()", cause.refusal()))
        .execute(f.persistence().pool()).await.unwrap();
    let scripted = scripted();
    let handler = CountCandidates {
        handler: &scripted,
        entries: AtomicUsize::new(0),
    };
    let monitor = InMemoryMonitor::new();
    let mut worker = polling(f.persistence(), &handler);
    worker.monitoring = Some(&monitor);
    let candidate = f.persistence().poll_work(None, 1).await.unwrap().candidates[0];
    let error = worker
        .process(candidate, &CancellationToken::new())
        .await
        .expect_err("the failed compensation must propagate without another candidate attempt");
    match (cause, error) {
        (Cause::Guard, AppError::WorkflowGuard(violation)) => {
            assert_eq!(violation.guard(), "test_compensation_guard")
        }
        (Cause::Connection, AppError::Database(message)) => {
            assert!(message.contains("compensation refused"))
        }
        (_, error) => panic!("the original compensation cause must survive: {error:?}"),
    }
    assert_eq!(
        handler.entries.load(Ordering::SeqCst),
        1,
        "one candidate capability check before claiming"
    );
    assert_eq!(scripted.calls.load(Ordering::SeqCst), 1);
    assert_eq!(run_state(f.persistence(), scope).await, "running");
    let attempts: Vec<String> =
        sqlx::query_scalar("SELECT status FROM task_attempts WHERE task_id=$1")
            .bind(scope.job.0)
            .fetch_all(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(attempts, ["processing"]);
    let guards: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_run_events WHERE run_id=$1 AND event_kind LIKE 'guard_violation:%'")
        .bind(scope.run.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(guards, 0, "the compensation transaction rolled back");
    assert_eq!(
        monitor.get_stats_json()["custom_counters"]["workflow_guard_violations_total{guard=test_forced_guard,site=step,outcome=error}"],
        1
    );
}

#[tokio::test]
async fn workflow_guard_error_io_compensation_guard_propagates_without_reentering_candidate() {
    compensation_failure(Cause::Guard).await;
}

#[tokio::test]
async fn workflow_guard_error_io_compensation_database_error_propagates_without_reentering_candidate()
 {
    compensation_failure(Cause::Connection).await;
}
