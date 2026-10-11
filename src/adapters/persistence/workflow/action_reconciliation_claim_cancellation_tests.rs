//! Domain cancellation orders and asynchronous claim cancellation are distinct.
use super::*;
use crate::application::workflow::{CancelCommand, CancelResult};

const EXECUTION_WAIT: &str = "SELECT id FROM workflow_executions";
const CANCEL_RUN_WAIT: &str = "SELECT run.company_id,run.id,run.workflow_id";

async fn cancel_command(s: &Scheduled, key: &str) -> CancelCommand {
    let scope = s.request.fence.scope;
    CancelCommand {
        company_id: scope.company,
        run_id: scope.run,
        actor: s.command.actor,
        command_key: IdempotencyKey::parse(key).unwrap(),
        expected_revision: s
            .fixture
            .persistence()
            .head(scope.company, scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    }
}

fn preserved(before: &Value, after: &Value, changed: &[&str]) {
    for (table, rows) in before.as_object().unwrap() {
        if !changed.contains(&table.as_str()) {
            assert_eq!(rows, &after[table], "preserve complete {table} rows");
        }
    }
}

const CANCEL_MUTATIONS: &[&str] = &[
    "workflow_runs",
    "background_tasks",
    "task_attempts",
    "workflow_control_commands",
    "workflow_run_events",
    "workflow_action_state_witnesses",
];

fn cancelled(s: &Scheduled, before: &Value, after: &Value) {
    preserved(before, after, CANCEL_MUTATIONS);
    let run = &after["workflow_runs"][0];
    assert_eq!(run["id"], json!(s.request.scope().run.as_uuid()));
    assert_eq!(run["state"], "cancelled");
    assert_eq!(run["waiting_reason"], Value::Null);
    assert!(
        run["revision"].as_i64().unwrap()
            > before["workflow_runs"][0]["revision"].as_i64().unwrap()
    );
    let job = &after["background_tasks"][0];
    assert_eq!(job["id"], json!(s.request.fence.scope.job.0));
    assert_eq!(job["status"], "failed");
    for field in [
        "worker_id",
        "execution_generation",
        "locked_at",
        "lock_expires_at",
    ] {
        assert_eq!(job[field], Value::Null, "cancel clears {field}");
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_cancellation_cancel_first() {
    for attempt in 1..=3 {
        if Box::pin(cancel_first(attempt)).await {
            return;
        }
    }
    panic!("every cancel_first attempt was void");
}

/// The claim's own attempt bound runs from its first poll. If it elapses before the claim's
/// wait is observed, or while the claim queues behind a cancel COMMIT that stalls on the
/// shared disk, the claim reports a definite NotApplied and the order under test is
/// unproven. That attempt is void, with its evidence and backstop asserted in
/// `void_claim`. The release guard is the production supervision guard G, derived from the
/// policy, rather than a fixed 900 ms.
fn release_guard() -> Duration {
    policy().supervision_guard()
}

fn not_applied<T>(result: &AppResult<T>) -> bool {
    matches!(result, Err(AppError::DatabaseTimeout(_)))
}

/// Backstop for a void attempt: the NotApplied claimant wrote no attempt, and when the
/// cancel ran, it applied: nothing of the claim's reached the run.
async fn void_claim<T: std::fmt::Debug>(
    s: &Scheduled,
    scenario: &str,
    attempt: usize,
    claimant: WorkflowWorkerId,
    claim: &AppResult<T>,
    cancel: Option<AppResult<CancelResult>>,
) {
    let attempts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM task_attempts WHERE task_id=$1 AND worker_id=$2")
            .bind(s.request.fence.scope.job.0)
            .bind(claimant.0)
            .fetch_one(s.fixture.persistence().pool())
            .await
            .unwrap();
    assert_eq!(attempts, 0, "the NotApplied claimant wrote no attempt");
    if let Some(cancel) = cancel {
        assert!(
            matches!(cancel, Ok(CancelResult::Applied { .. })),
            "a cancel beside a NotApplied claim applies: {cancel:?}"
        );
    }
    uncaptured(&format!(
        "VOID attempt scenario={scenario} attempt={attempt} evidence=claim_not_applied claim={claim:?}"
    ));
}

async fn cancel_first(attempt: usize) -> bool {
    // Box real fixture/action seams to retain the stock 2 MiB test stack.
    let s = Box::pin(scheduled()).await;
    let command = cancel_command(&s, "episode-cancel-first").await;
    let before = all_tables(&s.fixture).await;
    let mut gate = s.fixture.persistence().pool().begin().await.unwrap();
    let blocker = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM workflow_executions WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(s.request.scope().company.as_uuid())
        .bind(s.request.scope().execution.as_uuid())
        .execute(&mut *gate)
        .await
        .unwrap();
    let mut observer = s.fixture.persistence().pool().acquire().await.unwrap();
    let cancel = s.fixture.persistence().cancel(command);
    let claimant = worker();
    let claim = s
        .fixture
        .persistence()
        .claim_io(s.request.fence.scope, claimant, policy());
    tokio::pin!(cancel, claim);
    let cancel_pid = wait_for_lock(&mut observer, blocker, EXECUTION_WAIT, cancel.as_mut()).await;
    let mut early = None;
    let claim_pid = tokio::select! {
        result = cancel.as_mut() => panic!("cancel escaped execution gate: {result:?}"),
        pid = try_wait_for_lock(&mut observer, cancel_pid, RUN_WAIT, claim.as_mut()) => match pid {
            Ok(pid) => Some(pid),
            Err(output) if not_applied(&output) => {
                early = Some(output);
                None
            }
            Err(output) => panic!("claim completed before its observed lock wait: {output:?}"),
        },
    };
    assert_ne!(Some(cancel_pid), claim_pid);
    gate.rollback().await.unwrap();
    let (cancel, claim) = tokio::time::timeout(release_guard(), async {
        tokio::join!(cancel, async {
            match early {
                Some(output) => output,
                None => claim.as_mut().await,
            }
        })
    })
    .await
    .expect("real cancel and claim complete after gate release");
    if not_applied(&claim) {
        void_claim(&s, "cancel_first", attempt, claimant, &claim, Some(cancel)).await;
        return false;
    }
    let claim_pid = claim_pid.unwrap();
    assert!(matches!(cancel.unwrap(), CancelResult::Applied { .. }));
    assert!(claim.unwrap().is_none());
    assert_quiescent(&mut observer, &[cancel_pid, claim_pid]).await;
    drop(observer);
    let after = all_tables(&s.fixture).await;
    cancelled(&s, &before, &after);
    assert_eq!(after["task_attempts"], before["task_attempts"]);
    assert_eq!(
        after["background_tasks"][0]["retry_count"],
        before["background_tasks"][0]["retry_count"]
    );
    assert_eq!(after["workflow_control_commands"][0]["result"], "applied");
    assert!(
        s.fixture
            .persistence()
            .settled()
            .claim_io(s.request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        after,
        all_tables(&s.fixture).await,
        "terminal polling is inert"
    );
    true
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_cancellation_claim_first() {
    for attempt in 1..=3 {
        if Box::pin(claim_first(attempt)).await {
            return;
        }
    }
    panic!("every claim_first attempt was void");
}

async fn claim_first(attempt: usize) -> bool {
    let s = Box::pin(scheduled()).await;
    let command = cancel_command(&s, "episode-stale-cancel").await;
    let before = all_tables(&s.fixture).await;
    let gate = usage_gate(&s).await;
    let mut observer = s.fixture.persistence().pool().acquire().await.unwrap();
    let claimant = worker();
    let claim = s
        .fixture
        .persistence()
        .claim_io(s.request.fence.scope, claimant, policy());
    let cancel = s.fixture.persistence().cancel(command);
    tokio::pin!(claim, cancel);
    let claim_pid =
        match try_wait_for_lock(&mut observer, gate.pid, USAGE_WAIT, claim.as_mut()).await {
            Ok(pid) => pid,
            Err(output) if not_applied(&output) => {
                // The cancel was never polled, so nothing at all ran besides the claim.
                gate.tx.rollback().await.unwrap();
                void_claim(&s, "claim_first", attempt, claimant, &output, None).await;
                assert_eq!(before, all_tables(&s.fixture).await);
                return false;
            }
            Err(output) => panic!("claim completed before its observed lock wait: {output:?}"),
        };
    let mut early = None;
    let cancel_pid = tokio::select! {
        result = claim.as_mut() => {
            assert!(not_applied(&result), "claim escaped usage gate: {result:?}");
            early = Some(result);
            None
        }
        pid = wait_for_lock(&mut observer, claim_pid, CANCEL_RUN_WAIT, cancel.as_mut()) => Some(pid),
    };
    assert_ne!(Some(claim_pid), cancel_pid);
    gate.tx.rollback().await.unwrap();
    let (claim, cancel) = tokio::time::timeout(release_guard(), async {
        tokio::join!(
            async {
                match early {
                    Some(output) => output,
                    None => claim.as_mut().await,
                }
            },
            cancel
        )
    })
    .await
    .expect("real claim and cancel complete after gate release");
    if not_applied(&claim) {
        void_claim(&s, "claim_first", attempt, claimant, &claim, Some(cancel)).await;
        return false;
    }
    let cancel_pid = cancel_pid.unwrap();
    let claim = claim.unwrap().expect("claim wins the requesting run lock");
    assert!(matches!(
        cancel.unwrap(),
        CancelResult::RevisionConflict { .. }
    ));
    assert_quiescent(&mut observer, &[claim_pid, cancel_pid]).await;
    drop(observer);
    let claimed = all_tables(&s.fixture).await;
    claimed_once(&s, &before, &claimed, claim.fence);
    assert!(
        s.fixture
            .persistence()
            .settled()
            .validate_io(claim.fence, policy())
            .await
            .unwrap()
    );
    let fresh = cancel_command(&s, "episode-fresh-cancel").await;
    assert!(matches!(
        s.fixture.persistence().cancel(fresh).await.unwrap(),
        CancelResult::Applied { .. }
    ));
    let after = all_tables(&s.fixture).await;
    cancelled(&s, &claimed, &after);
    retired_exact(&before, &claimed, &after, claim.fence);
    assert!(
        !s.fixture
            .persistence()
            .settled()
            .validate_io(claim.fence, policy())
            .await
            .unwrap()
    );
    assert!(
        s.fixture
            .persistence()
            .settled()
            .claim_io(s.request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(after, all_tables(&s.fixture).await);
    true
}

fn claimed_once(s: &Scheduled, before: &Value, claimed: &Value, fence: WorkflowFence) {
    preserved(
        before,
        claimed,
        &[
            "workflow_runs",
            "background_tasks",
            "task_attempts",
            "workflow_control_commands",
        ],
    );
    assert_eq!(
        claimed["workflow_control_commands"][0]["result"],
        "conflict"
    );
    assert_eq!(
        claimed["task_attempts"].as_array().unwrap().len(),
        before["task_attempts"].as_array().unwrap().len() + 1
    );
    assert_eq!(fence.attempt.0, s.request.fence.attempt.0 + 1);
}

fn retired_exact(before: &Value, claimed: &Value, after: &Value, fence: WorkflowFence) {
    assert_eq!(
        after["background_tasks"][0]["retry_count"],
        claimed["background_tasks"][0]["retry_count"]
            .as_i64()
            .unwrap()
            + 1
    );
    assert_eq!(
        after["task_attempts"].as_array().unwrap().len(),
        claimed["task_attempts"].as_array().unwrap().len()
    );
    for old in before["task_attempts"].as_array().unwrap() {
        assert!(after["task_attempts"].as_array().unwrap().contains(old));
    }
    let attempt = after["task_attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["attempt_number"] == json!(fence.attempt.0))
        .unwrap();
    assert_eq!(attempt["worker_id"], json!(fence.worker.0));
    assert_eq!(attempt["execution_generation"], json!(fence.generation.0));
    assert_eq!(attempt["workflow_retirement"], "cancel");
}

async fn recover_unchanged(s: &Scheduled, before: &Value) {
    if before["workflow_root_budget_usage"][0]["model_calls"] == json!(0) {
        successful_claim(s, before).await;
    } else {
        assert!(
            s.fixture
                .persistence()
                .settled()
                .claim_io(s.request.fence.scope, worker(), policy())
                .await
                .unwrap()
                .is_none()
        );
        assert_refusal(
            before,
            &all_tables(&s.fixture).await,
            &s.request,
            &s.command,
        );
    }
}

/// The drop or abort must end the claim inside its own attempt bound, or the bound, not the
/// cancellation, ended it. When the bound elapsed first (the claim completed with a definite
/// NotApplied before its wait was observed, or observation itself took the whole bound),
/// the attempt is void: the gate is released and every public table must still equal
/// `before` (the backstop), and the caller retries on a fresh fixture.
async fn void_waiting(
    s: &Scheduled,
    scenario: &str,
    attempt: usize,
    before: &Value,
    evidence: &str,
) {
    assert_eq!(
        before,
        &all_tables(&s.fixture).await,
        "a void {scenario} wrote nothing"
    );
    uncaptured(&format!(
        "VOID attempt scenario={scenario} attempt={attempt} evidence={evidence}"
    ));
}

async fn waiting_proven(
    scenario: &str,
    exhausted: bool,
    run: impl AsyncFn(&Scheduled, usize) -> bool,
) {
    for attempt in 1..=3 {
        let s = Box::pin(scheduled()).await;
        if exhausted {
            Box::pin(exhaust(&s)).await;
        }
        if run(&s, attempt).await {
            return;
        }
    }
    panic!("every {scenario} attempt was void");
}

async fn drop_waiting(s: &Scheduled, attempt: usize) -> bool {
    let before = all_tables(&s.fixture).await;
    let gate = usage_gate(s).await;
    let mut observer = s.fixture.persistence().pool().acquire().await.unwrap();
    let mut claim = Box::pin(s.fixture.persistence().claim_io(
        s.request.fence.scope,
        worker(),
        policy(),
    ));
    let started = Instant::now();
    let observed = try_wait_for_lock(&mut observer, gate.pid, USAGE_WAIT, claim.as_mut()).await;
    let elapsed = started.elapsed();
    let pid = match observed {
        Ok(pid) if elapsed < policy().attempt_bound() => pid,
        observed => {
            let evidence = match observed {
                Ok(_) => format!("observed_after_bound elapsed_ms={}", elapsed.as_millis()),
                Err(output) => {
                    assert!(
                        not_applied(&output),
                        "claim completed before its observed lock wait: {output:?}"
                    );
                    format!("claim_not_applied {output:?}")
                }
            };
            drop(claim);
            gate.tx.rollback().await.unwrap();
            drop(observer);
            void_waiting(s, "dropped_usage_wait", attempt, &before, &evidence).await;
            return false;
        }
    };
    assert!(live_deadline(&mut observer, s).await);
    // Dropping the pinned future itself cancels the real claim transaction.
    drop(claim);
    gate.tx.rollback().await.unwrap();
    assert_quiescent(&mut observer, &[pid]).await;
    drop(observer);
    assert_eq!(
        before,
        all_tables(&s.fixture).await,
        "dropped claim leaves every public table exact"
    );
    recover_unchanged(s, &before).await;
    true
}

async fn abort_waiting(s: &Scheduled, attempt: usize) -> bool {
    let before = all_tables(&s.fixture).await;
    let gate = usage_gate(s).await;
    let mut observer = s.fixture.persistence().pool().acquire().await.unwrap();
    let persistence = crate::adapters::persistence::PostgresPersistence::new(
        s.fixture.persistence().pool().clone(),
    );
    let scope = s.request.fence.scope;
    let mut task =
        tokio::spawn(async move { persistence.claim_io(scope, worker(), policy()).await });
    let started = Instant::now();
    let observed =
        try_wait_for_lock(&mut observer, gate.pid, USAGE_WAIT, Pin::new(&mut task)).await;
    let elapsed = started.elapsed();
    let pid = match observed {
        Ok(pid) if elapsed < policy().attempt_bound() => pid,
        observed => {
            let evidence = match observed {
                Ok(_) => {
                    task.abort();
                    let _ = (&mut task).await;
                    format!("observed_after_bound elapsed_ms={}", elapsed.as_millis())
                }
                Err(output) => {
                    let output = output.expect("the claim task was not aborted yet");
                    assert!(
                        not_applied(&output),
                        "claim completed before its observed lock wait: {output:?}"
                    );
                    format!("claim_not_applied {output:?}")
                }
            };
            gate.tx.rollback().await.unwrap();
            drop(observer);
            void_waiting(s, "aborted_task_usage_wait", attempt, &before, &evidence).await;
            return false;
        }
    };
    assert!(live_deadline(&mut observer, s).await);
    task.abort();
    assert!(
        task.await.unwrap_err().is_cancelled(),
        "explicit task cancellation is awaited"
    );
    gate.tx.rollback().await.unwrap();
    assert_quiescent(&mut observer, &[pid]).await;
    drop(observer);
    assert_eq!(
        before,
        all_tables(&s.fixture).await,
        "aborted claim leaves every public table exact"
    );
    recover_unchanged(s, &before).await;
    true
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_cancellation_dropped_usage_wait() {
    waiting_proven("dropped_usage_wait", false, drop_waiting).await;
    waiting_proven("dropped_usage_wait", true, drop_waiting).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_cancellation_aborted_task_usage_wait() {
    waiting_proven("aborted_task_usage_wait", false, abort_waiting).await;
    waiting_proven("aborted_task_usage_wait", true, abort_waiting).await;
}
