use super::*;
use crate::application::workflow::{activation::*, lease::*};
use std::time::Duration;
use tokio::{sync::Barrier, time::Instant};

#[path = "capacity_tests.rs"]
mod capacity_tests;

#[path = "action_dispatch_tests.rs"]
mod action_dispatch_tests;

fn policy() -> LeasePolicy {
    LeasePolicy::new(Duration::from_secs(3)).unwrap()
}
fn worker() -> WorkflowWorkerId {
    WorkflowWorkerId(Uuid::new_v4())
}

/// Writes one line straight to stderr, past libtest's output capture, so a run's log lists
/// every asserted Unknown, void attempt and re-poll even when the test passes.
fn uncaptured(line: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{line}");
}

/// The one re-poll rule for bounded workflow persistence calls in these tests.
///
/// `AppError::DatabaseTimeout` is a bounded operation's definite NotApplied: its bound
/// elapsed before COMMIT was sent, or the server cancelled a statement, and the transaction
/// rolled back, so nothing was written (persistence_bound.rs records both as `not_applied`).
/// None of these operations has an external effect. Asking again is therefore the same
/// question, so a sequential setup step or probe re-polls until it gets a definite answer,
/// which the caller then asserts exactly as before. Every other outcome, including a
/// "commit outcome unknown" Conflict, is returned the first time and never retried.
///
/// Use `settled()` only where nothing in the test competes with the call. Identical
/// competing claimants use `competing`, which keeps their exactly-one-winner assertion.
/// Each re-poll is logged through `uncaptured`. The hang guard is only a guard.
trait Settle {
    fn settled(&self) -> SettledIo<'_>;
}
impl Settle for PostgresPersistence {
    #[track_caller]
    fn settled(&self) -> SettledIo<'_> {
        SettledIo {
            persistence: self,
            at: std::panic::Location::caller(),
        }
    }
}
const SETTLE_HANG_GUARD: Duration = Duration::from_secs(10);
struct SettledIo<'a> {
    persistence: &'a PostgresPersistence,
    at: &'static std::panic::Location<'static>,
}
async fn repoll<T, Fut>(
    at: &std::panic::Location<'_>,
    op: &str,
    mut call: impl FnMut() -> Fut,
) -> AppResult<T>
where
    Fut: std::future::Future<Output = AppResult<T>>,
{
    let started = std::time::Instant::now();
    let mut attempt = 1;
    loop {
        match call().await {
            Err(AppError::DatabaseTimeout(error)) => {
                uncaptured(&format!(
                    "SETTLE repoll op={op} at={at} attempt={attempt} not_applied={error}"
                ));
                assert!(
                    started.elapsed() < SETTLE_HANG_GUARD,
                    "{op} at {at} never settled within the hang guard: {error}"
                );
                attempt += 1;
            }
            settled => return settled,
        }
    }
}
impl<'a> SettledIo<'a> {
    async fn claim_io(
        &self,
        scope: ActivationRequest,
        worker: WorkflowWorkerId,
        policy: LeasePolicy,
    ) -> AppResult<Option<ClaimedWorkflow>> {
        repoll(self.at, "claim_io", || {
            self.persistence.claim_io(scope, worker, policy)
        })
        .await
    }
    async fn renew_io(
        &self,
        fence: WorkflowFence,
        policy: LeasePolicy,
    ) -> AppResult<Option<LeaseWindow>> {
        repoll(self.at, "renew_io", || {
            self.persistence.renew_io(fence, policy)
        })
        .await
    }
    async fn validate_io(&self, fence: WorkflowFence, policy: LeasePolicy) -> AppResult<bool> {
        repoll(self.at, "validate_io", || {
            self.persistence.validate_io(fence, policy)
        })
        .await
    }
    async fn release_io(
        &self,
        fence: WorkflowFence,
        policy: LeasePolicy,
        cause: LeaseReleaseCause,
    ) -> AppResult<bool> {
        repoll(self.at, "release_io", || {
            self.persistence.release_io(fence, policy, cause.clone())
        })
        .await
    }
    async fn retire_expired_io(
        &self,
        scope: ActivationRequest,
        policy: LeasePolicy,
    ) -> AppResult<bool> {
        repoll(self.at, "retire_expired_io", || {
            self.persistence.retire_expired_io(scope, policy)
        })
        .await
    }
    async fn retire_exhausted_work(
        &self,
        scope: ActivationRequest,
        policy: LeasePolicy,
    ) -> AppResult<bool> {
        repoll(self.at, "retire_exhausted_work", || {
            self.persistence.retire_exhausted_work(scope, policy)
        })
        .await
    }
    async fn complete_io(
        &self,
        result: FencedWorkflowResult,
    ) -> AppResult<Option<crate::application::workflow::completion::CommittedWorkflowStep>> {
        use crate::application::workflow::completion::WorkflowCompletion;
        repoll(self.at, "complete_io", || {
            self.persistence.complete_io(FencedWorkflowResult {
                fence: result.fence,
                output: result.output.clone(),
                policy: result.policy,
            })
        })
        .await
    }
    async fn reserve_budget(
        &self,
        request: crate::application::workflow::budget::BudgetReservation,
        policy: LeasePolicy,
    ) -> AppResult<crate::application::workflow::budget::BudgetReservationResult> {
        use crate::application::workflow::budget::WorkflowBudgets;
        repoll(self.at, "reserve_budget", || {
            self.persistence.reserve_budget(request.clone(), policy)
        })
        .await
    }
    /// Two identical claimants race (claimant index 0 and 1). The pair is re-polled only
    /// while there is no winner: at least one reported a definite NotApplied, and every
    /// other result is a non-winning `Ok`. A non-winning claim may itself have committed
    /// settles (expiry retirement, activation, a refusal row), so the re-poll is a further
    /// claimant against that settled state, as a worker's next poll would be. An `Err` of
    /// any other kind, Unknown included, is never re-polled. Beside a winner, a NotApplied
    /// claimant lost: it waited out its bound
    /// behind the winner and wrote nothing, so it is returned as the losing `T::default()`
    /// (`None` or `false`), which keeps the caller's exactly-one-winner assertion intact.
    async fn competing<T: Default, Fut>(
        &self,
        call: impl Fn(&'a PostgresPersistence, usize) -> Fut,
        won: impl Fn(&T) -> bool,
    ) -> [AppResult<T>; 2]
    where
        Fut: std::future::Future<Output = AppResult<T>>,
    {
        let at = self.at;
        let started = std::time::Instant::now();
        let mut attempt = 1;
        loop {
            let (first, second) =
                tokio::join!(call(self.persistence, 0), call(self.persistence, 1));
            let results = [first, second];
            let not_applied =
                |result: &AppResult<T>| matches!(result, Err(AppError::DatabaseTimeout(_)));
            let winner = results
                .iter()
                .any(|result| matches!(result, Ok(value) if won(value)));
            let quiet = results.iter().all(|result| match result {
                Ok(value) => !won(value),
                error => not_applied(error),
            });
            if quiet && results.iter().any(not_applied) {
                uncaptured(&format!(
                    "SETTLE repoll op=competing at={at} attempt={attempt} not_applied=pair, no winner"
                ));
                assert!(
                    started.elapsed() < SETTLE_HANG_GUARD,
                    "competing claimants at {at} never settled within the hang guard"
                );
                attempt += 1;
                continue;
            }
            return results.map(|result| match result {
                Err(AppError::DatabaseTimeout(error)) if winner => {
                    uncaptured(&format!(
                        "SETTLE loser op=competing at={at} not_applied={error}"
                    ));
                    Ok(T::default())
                }
                result => result,
            });
        }
    }
}

/// Proof of the re-poll rule on scripted outcomes (no database is reached: the pool is lazy
/// and the scripted calls never use it). A definite NotApplied is re-polled to the settled
/// answer; Unknown, any other error and a pair that may have written are returned as is.
mod settle_tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    fn not_applied() -> AppError {
        AppError::DatabaseTimeout(
            "Workflow lease.claim not applied: persistence bound elapsed before commit".into(),
        )
    }
    fn unknown() -> AppError {
        AppError::Conflict("Workflow lease.claim commit outcome unknown".into())
    }
    fn scripted<T>(outcomes: Vec<AppResult<T>>) -> Mutex<VecDeque<AppResult<T>>> {
        Mutex::new(outcomes.into())
    }
    fn persistence() -> PostgresPersistence {
        PostgresPersistence::new(
            sqlx::PgPool::connect_lazy("postgres://invalid:invalid@127.0.0.1:1/invalid").unwrap(),
        )
    }
    async fn single(outcomes: Vec<AppResult<u8>>) -> (AppResult<u8>, usize) {
        let script = scripted(outcomes);
        let calls = Mutex::new(0);
        let settled = repoll(std::panic::Location::caller(), "scripted", || {
            *calls.lock().unwrap() += 1;
            let next = script.lock().unwrap().pop_front().unwrap();
            async move { next }
        })
        .await;
        (settled, calls.into_inner().unwrap())
    }
    async fn pair(rounds: Vec<[AppResult<bool>; 2]>) -> ([AppResult<bool>; 2], usize) {
        let script = scripted(rounds.into_iter().flatten().map(Ok).collect());
        let calls = Mutex::new(0);
        let p = persistence();
        let settled = p
            .settled()
            .competing(
                |_, _| {
                    *calls.lock().unwrap() += 1;
                    let next = script.lock().unwrap().pop_front().unwrap().unwrap();
                    async move { next }
                },
                |retired: &bool| *retired,
            )
            .await;
        (settled, calls.into_inner().unwrap())
    }

    #[tokio::test]
    async fn workflow_io_settle_repolls_only_a_definite_not_applied() {
        // A setup step whose first attempt was not applied settles on the retry.
        let (settled, calls) = single(vec![Err(not_applied()), Err(not_applied()), Ok(7)]).await;
        assert_eq!((settled.unwrap(), calls), (7, 3));
        // Unknown may have committed: returned on first sight, so the caller's unwrap fails.
        let (settled, calls) = single(vec![Err(unknown()), Ok(7)]).await;
        assert!(
            matches!(&settled, Err(error) if error.to_string().contains("commit outcome unknown"))
        );
        assert_eq!(calls, 1);
        // Any other error is never retried either.
        let (settled, calls) = single(vec![Err(AppError::Database("x".into())), Ok(7)]).await;
        assert!(matches!(settled, Err(AppError::Database(_))));
        assert_eq!(calls, 1);
    }

    #[tokio::test]
    #[should_panic(expected = "commit outcome unknown")]
    async fn workflow_io_settle_unknown_setup_still_fails() {
        let (settled, _) = single(vec![Err(unknown()), Ok(7)]).await;
        settled.unwrap();
    }

    #[tokio::test]
    async fn workflow_io_settle_competing_pair_repolls_only_when_nothing_was_written() {
        // Nothing written (NotApplied beside a non-winner): the pair re-polls; on the second
        // round the NotApplied competitor beside the winner lost.
        let (settled, calls) = pair(vec![
            [Err(not_applied()), Ok(false)],
            [Ok(true), Err(not_applied())],
        ])
        .await;
        assert_eq!(calls, 4);
        let [first, second] = settled;
        assert_eq!((first.unwrap(), second.unwrap()), (true, false));
        // A competitor whose outcome is unknown may have written: never re-polled, and the
        // Unknown is returned unchanged beside a NotApplied or a winner.
        for round in [
            [Err(not_applied()), Err(unknown())],
            [Ok(true), Err(unknown())],
        ] {
            let (settled, calls) = pair(vec![round]).await;
            assert_eq!(calls, 2);
            assert!(
                matches!(&settled[1], Err(error) if error.to_string().contains("commit outcome unknown"))
            );
            if let Ok(won) = &settled[0] {
                assert!(*won);
            } else {
                assert!(matches!(settled[0], Err(AppError::DatabaseTimeout(_))));
            }
        }
    }
}

async fn fixture() -> (AdmissionFixture, ActivationRequest) {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["input_schema"] = json!({"type":"object","properties":{"value":{"type":"integer","minimum":1,"maximum":1000}},"required":["value"]});
    source["steps"]["start"]["with"]["max_tokens"] = json!({"ref":"/input/value"});
    fixture_source(source).await
}
async fn fixture_source(source: Value) -> (AdmissionFixture, ActivationRequest) {
    let f = AdmissionFixture::with_binding(BindingFixture::from_source(source).await).await;
    let command = f.prepare(f.request("lease", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    let scope = ActivationRequest {
        company: command.company_id(),
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    };
    (f, scope)
}
async fn snapshot(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('executions',(SELECT jsonb_agg(to_jsonb(execution)) FROM workflow_executions AS execution),'jobs',(SELECT jsonb_agg(to_jsonb(job)) FROM background_tasks AS job),'attempts',(SELECT jsonb_agg(to_jsonb(attempt)) FROM task_attempts AS attempt),'runs',(SELECT jsonb_agg(to_jsonb(run)) FROM workflow_runs AS run))").fetch_one(f.persistence().pool()).await.unwrap()
}
async fn expire(f: &AdmissionFixture, scope: ActivationRequest) {
    sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds',lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(scope.job.0).execute(f.persistence().pool()).await.unwrap();
}
async fn due(f: &AdmissionFixture, scope: ActivationRequest) {
    sqlx::query(
        "UPDATE background_tasks SET run_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(scope.job.0)
    .execute(f.persistence().pool())
    .await
    .unwrap();
}
#[tokio::test]
async fn workflow_io_real_competing_claims_lost_ack_crash_backoff_and_frozen_retry() {
    let (f, scope) = fixture().await;
    let barrier = Barrier::new(2);
    let barrier = &barrier;
    let [left, right] = f
        .persistence()
        .settled()
        .competing(
            |p, _| async move {
                barrier.wait().await;
                p.claim_io(scope, worker(), policy()).await
            },
            Option::is_some,
        )
        .await;
    let (left, right) = (left.unwrap(), right.unwrap());
    assert_eq!(
        usize::from(left.is_some()) + usize::from(right.is_some()),
        1
    );
    let first = left.or(right).unwrap();
    let before = snapshot(&f).await;
    // A lost acknowledgment never authorizes another invocation.
    assert!(
        f.persistence()
            .settled()
            .claim_io(scope, first.fence.worker, policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(snapshot(&f).await, before);
    expire(&f, scope).await;
    assert!(
        !f.persistence()
            .settled()
            .validate_io(first.fence, policy())
            .await
            .unwrap()
    );
    assert!(
        f.persistence()
            .settled()
            .renew_io(first.fence, policy())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !f.persistence()
            .settled()
            .release_io(first.fence, policy(), LeaseReleaseCause::LocalInterruption)
            .await
            .unwrap()
    );
    assert!(
        f.persistence()
            .settled()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    let after = snapshot(&f).await;
    assert_eq!(after["jobs"][0]["retry_count"], 1);
    assert_eq!(after["attempts"][0]["stop_reason"], "lease_lost");
    assert!(
        f.persistence()
            .settled()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(snapshot(&f).await, after);
    sqlx::query("UPDATE workflow_runs SET input = '{\"value\":999}' WHERE id=$1")
        .bind(scope.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    due(&f, scope).await;
    let second = f
        .persistence()
        .settled()
        .claim_io(scope, first.fence.worker, policy())
        .await
        .unwrap()
        .unwrap();
    assert_ne!(first.fence.generation, second.fence.generation);
    assert_eq!(second.fence.attempt, WorkflowAttempt(2));
    assert_eq!(first.activation, second.activation);
    assert_eq!(second.activation.inputs["max_tokens"], 1);
    assert!(
        !f.persistence()
            .settled()
            .release_io(first.fence, policy(), LeaseReleaseCause::LocalInterruption)
            .await
            .unwrap()
    );
    assert!(
        f.persistence()
            .settled()
            .renew_io(first.fence, policy())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !f.persistence()
            .settled()
            .validate_io(first.fence, policy())
            .await
            .unwrap()
    );
    assert!(
        f.persistence()
            .settled()
            .release_io(second.fence, policy(), LeaseReleaseCause::Deadline)
            .await
            .unwrap()
    );
    assert!(
        f.persistence()
            .settled()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    due(&f, scope).await;
    let third = f
        .persistence()
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(third.fence.attempt, WorkflowAttempt(3));
    assert!(
        f.persistence()
            .settled()
            .release_io(third.fence, policy(), LeaseReleaseCause::LocalInterruption)
            .await
            .unwrap()
    );
    due(&f, scope).await;
    assert!(
        f.persistence()
            .settled()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    let final_state = snapshot(&f).await;
    assert_eq!(final_state["attempts"].as_array().unwrap().len(), 3);
    assert_eq!(final_state["jobs"][0]["retry_count"], 3);
    let reason: Option<String> = sqlx::query_scalar(
        "SELECT stop_reason FROM task_attempts WHERE task_id=$1 AND attempt_number=2",
    )
    .bind(scope.job.0)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(reason.as_deref(), Some("timed_out"));
}
#[tokio::test]
async fn workflow_io_claim_rolls_back_at_each_write_boundary() {
    for (table, condition) in [
        ("workflow_executions", "NEW.activated_at IS NOT NULL"),
        ("background_tasks", "NEW.status='processing'"),
        ("task_attempts", "true"),
    ] {
        let (f, scope) = fixture().await;
        let before = snapshot(&f).await;
        sqlx::raw_sql(&format!("CREATE FUNCTION lease_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF {condition} THEN RAISE EXCEPTION 'injected lease failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER lease_fault AFTER INSERT OR UPDATE ON {table} FOR EACH ROW EXECUTE FUNCTION lease_fault();")).execute(f.persistence().pool()).await.unwrap();
        assert!(
            f.persistence()
                .claim_io(scope, worker(), policy())
                .await
                .is_err()
        );
        assert_eq!(snapshot(&f).await, before);
        sqlx::raw_sql(&format!(
            "DROP TRIGGER lease_fault ON {table}; DROP FUNCTION lease_fault();"
        ))
        .execute(f.persistence().pool())
        .await
        .unwrap();
        assert!(
            f.persistence()
                .settled()
                .claim_io(scope, worker(), policy())
                .await
                .unwrap()
                .is_some()
        );
    }
}
#[tokio::test]
async fn workflow_io_exact_scope_fence_and_run_state_refuse_without_writes() {
    let (f, scope) = fixture().await;
    for invalid in [
        ActivationRequest {
            job: WorkflowJobId(Uuid::new_v4()),
            ..scope
        },
        ActivationRequest {
            company: CompanyId::new(Uuid::new_v4()),
            ..scope
        },
        ActivationRequest {
            run: RunId::new(Uuid::new_v4()),
            ..scope
        },
        ActivationRequest {
            execution: ExecutionId::new(Uuid::new_v4()),
            ..scope
        },
    ] {
        let before = snapshot(&f).await;
        assert!(
            f.persistence()
                .settled()
                .claim_io(invalid, worker(), policy())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(snapshot(&f).await, before);
    }
    let claim = f
        .persistence()
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    for fence in [
        WorkflowFence {
            worker: worker(),
            ..claim.fence
        },
        WorkflowFence {
            generation: WorkflowGeneration(Uuid::new_v4()),
            ..claim.fence
        },
        WorkflowFence {
            attempt: WorkflowAttempt(2),
            ..claim.fence
        },
    ] {
        let before = snapshot(&f).await;
        assert!(
            f.persistence()
                .settled()
                .renew_io(fence, policy())
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            !f.persistence()
                .settled()
                .validate_io(fence, policy())
                .await
                .unwrap()
        );
        assert!(
            !f.persistence()
                .settled()
                .release_io(fence, policy(), LeaseReleaseCause::LocalInterruption)
                .await
                .unwrap()
        );
        assert_eq!(snapshot(&f).await, before);
    }
    sqlx::query("UPDATE workflow_runs SET state='cancelled' WHERE id=$1")
        .bind(scope.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let before = snapshot(&f).await;
    assert!(
        f.persistence()
            .settled()
            .renew_io(claim.fence, policy())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !f.persistence()
            .settled()
            .validate_io(claim.fence, policy())
            .await
            .unwrap()
    );
    assert!(
        !f.persistence()
            .settled()
            .release_io(claim.fence, policy(), LeaseReleaseCause::LocalInterruption)
            .await
            .unwrap()
    );
    assert_eq!(snapshot(&f).await, before);
}
#[tokio::test]
async fn workflow_io_external_handler_has_no_transaction_and_returns_fence() {
    use crate::application::workflow::supervise::*;
    let (f, scope) = fixture().await;
    let claim = f
        .persistence()
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let fence = claim.fence;
    let handler = async {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE NOWAIT")
            .bind(scope.run.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM workflow_executions WHERE id=$1 FOR UPDATE NOWAIT")
            .bind(scope.execution.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM background_tasks WHERE id=$1 FOR UPDATE NOWAIT")
            .bind(scope.job.0)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        Ok(Value::Null)
    };
    let result = supervise_io(
        f.persistence(),
        claim,
        policy(),
        Instant::now() + Duration::from_secs(10),
        handler,
        std::future::pending(),
    )
    .await;
    assert!(matches!(result,SupervisedResult::Ready(result) if result.fence==fence));
    let state = snapshot(&f).await;
    assert_eq!(state["jobs"][0]["status"], "processing");
    assert_eq!(state["executions"][0]["completed_at"], Value::Null);
}

#[path = "lease_adversarial_tests.rs"]
mod adversarial_tests;

#[path = "lease_resolution_gate.rs"]
pub(in crate::adapters::persistence::workflow) mod resolution_gate;

#[path = "lease_resolution_tests.rs"]
mod resolution_tests;

#[path = "completion_tests.rs"]
mod completion_tests;

#[path = "wait_tests.rs"]
mod wait_tests;

#[path = "polling_tests.rs"]
mod polling_tests;

#[path = "recovery_tests.rs"]
mod recovery_tests;

#[path = "pending_io_tests.rs"]
mod pending_io_tests;

#[path = "maintenance_tests.rs"]
mod maintenance_tests;

#[path = "control_tests.rs"]
mod control_tests;

#[path = "harness_owner_tests.rs"]
mod harness_owner_tests;
