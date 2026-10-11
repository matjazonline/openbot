//! Public claim deadline waits and commit failures leave no partial public facts.
use super::*;
use std::pin::Pin;

#[path = "action_reconciliation_claim_abort_probes.rs"]
mod probes;
use probes::{ProbeKind, install_probe, remove_probe};

#[path = "action_reconciliation_claim_cancellation_tests.rs"]
mod cancellation_tests;

use super::super::super::super::control_serialization_tests::contention::{
    try_wait_for_lock, wait_for_lock,
};
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::gated_policy;

/// Only a hang guard for joins and drains; outcomes and server lock state are the signals.
const HANG_GUARD: Duration = Duration::from_secs(10);

const USAGE_WAIT: &str = "SELECT usage.root_run_id FROM workflow_root_budget_usage";
const RUN_WAIT: &str = "SELECT binding_id, max_context_bytes FROM workflow_runs";

async fn live_deadline(observer: &mut sqlx::PgConnection, s: &Scheduled) -> bool {
    sqlx::query_scalar(
        "SELECT deadline>clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(s.request.scope().company.as_uuid())
    .bind(s.request.scope().run.as_uuid())
    .fetch_one(observer)
    .await
    .unwrap()
}

struct UsageGate<'a> {
    tx: Transaction<'a, Postgres>,
    pid: i32,
}

async fn usage_gate(s: &Scheduled) -> UsageGate<'_> {
    let mut tx = s.fixture.persistence().pool().begin().await.unwrap();
    let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT root_run_id FROM workflow_root_budget_usage WHERE company_id=$1 AND root_run_id=$2 FOR UPDATE")
        .bind(s.request.scope().company.as_uuid()).bind(s.request.scope().run.as_uuid())
        .execute(&mut *tx).await.unwrap();
    UsageGate { tx, pid }
}

/// `true` when the crossing was proven; `false` when the attempt is void: a claimant reached
/// its first statement only after the A/2 deadline had already passed (under load), so it
/// was correctly refused without waiting. The void is asserted (refusals only, every public
/// fact unchanged) and logged; the caller rebuilds the fixture.
async fn deadline_crossing(s: &Scheduled, attempt: usize) -> bool {
    // Derived from the claimants' own policy (the 6 s gate policy: A = 2 s). The run
    // deadline falls at A/2, set right before the snapshot and the claimants (the gate and
    // observer are acquired first), so both claimants queue before it and it passes well
    // before the release cutoff at A·3/4, below their server lock bound A.
    let crossing = gated_policy();
    let gate = usage_gate(s).await;
    let mut observer = s.fixture.persistence().pool().acquire().await.unwrap();
    sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp()+make_interval(secs => $3) WHERE company_id=$1 AND id=$2")
        .bind(s.request.scope().company.as_uuid()).bind(s.request.scope().run.as_uuid())
        .bind((crossing.attempt_bound() / 2).as_secs_f64())
        .execute(s.fixture.persistence().pool()).await.unwrap();
    let before = all_tables(&s.fixture).await;
    let first = s
        .fixture
        .persistence()
        .claim_io(s.request.fence.scope, worker(), crossing);
    let second = s
        .fixture
        .persistence()
        .claim_io(s.request.fence.scope, worker(), crossing);
    tokio::pin!(first, second);
    let started = Instant::now();
    let first_pid =
        match try_wait_for_lock(&mut observer, gate.pid, USAGE_WAIT, first.as_mut()).await {
            Ok(pid) => pid,
            Err(early) => {
                void_crossing(
                    s,
                    attempt,
                    "first",
                    early,
                    &mut observer,
                    gate,
                    second,
                    &before,
                )
                .await;
                return false;
            }
        };
    assert!(
        live_deadline(&mut observer, s).await,
        "usage wait reached before DB deadline"
    );
    let second_pid = tokio::select! {
        result = first.as_mut() => panic!("first claimant escaped usage wait: {result:?}"),
        pid = try_wait_for_lock(&mut observer, first_pid, RUN_WAIT, second.as_mut()) => pid,
    };
    let second_pid = match second_pid {
        Ok(pid) => pid,
        Err(early) => {
            void_crossing(
                s,
                attempt,
                "second",
                early,
                &mut observer,
                gate,
                first,
                &before,
            )
            .await;
            return false;
        }
    };
    assert_ne!(first_pid, second_pid);
    assert!(
        live_deadline(&mut observer, s).await,
        "both genuine callers queued before expiry"
    );
    eprintln!(
        "live DB deadline: usage waiter {first_pid} blocked by {}, run waiter {second_pid} blocked by {first_pid}",
        gate.pid
    );
    // Three quarters of the claimants' attempt bound: their server lock timeout is what
    // remains of it, so the gate must be released with room for the refusal to run.
    let cutoff = crossing.attempt_bound() * 3 / 4;
    let expired = async {
        while live_deadline(&mut observer, s).await {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    tokio::select! {
        result = first.as_mut() => panic!("first completed before observed DB expiry: {result:?}"),
        result = second.as_mut() => panic!("second completed before observed DB expiry: {result:?}"),
        result = tokio::time::timeout_at(started + cutoff, expired) => result.expect("DB expiry must precede persistence timeout"),
    }
    assert!(
        started.elapsed() < cutoff,
        "release before persistence cutoff"
    );
    eprintln!(
        "observed DB expiry and released usage gate at {:?}",
        started.elapsed()
    );
    gate.tx.rollback().await.unwrap();
    let (first, second) = tokio::time::timeout(HANG_GUARD, async { tokio::join!(first, second) })
        .await
        .expect("deadline rejection completes within the hang guard");
    eprintln!("deadline rejection settled at {:?}", started.elapsed());
    assert!(
        matches!(first, Err(AppError::Conflict(ref message)) if message=="Workflow lease claim refused: run or lease window closed"),
        "usage-after-wait rejects expired active snapshot: {first:?}"
    );
    assert!(
        second.unwrap().is_none(),
        "run-lock waiter sees expired deadline"
    );
    assert_quiescent(&mut observer, &[first_pid, second_pid]).await;
    drop(observer);
    assert_eq!(
        before,
        all_tables(&s.fixture).await,
        "expired install/refusal and every public fact roll back"
    );
    true
}

/// The void branch of `deadline_crossing`: the early claimant was refused (Ok(None)) and the
/// DB deadline had passed, the other claimant is released and must be refused too, and no
/// public fact changed.
#[allow(clippy::too_many_arguments)]
async fn void_crossing(
    s: &Scheduled,
    attempt: usize,
    label: &str,
    early: AppResult<Option<ClaimedWorkflow>>,
    observer: &mut sqlx::PgConnection,
    gate: UsageGate<'_>,
    other: Pin<&mut impl Future<Output = AppResult<Option<ClaimedWorkflow>>>>,
    before: &Value,
) {
    assert!(
        matches!(early, Ok(None)) && !live_deadline(observer, s).await,
        "{label} claimant completed before its observed wait while the deadline was live: {early:?}"
    );
    gate.tx.rollback().await.unwrap();
    let other = tokio::time::timeout(HANG_GUARD, other)
        .await
        .expect("the other claimant settles within the hang guard");
    assert!(
        matches!(&other, Ok(None))
            || matches!(&other, Err(AppError::Conflict(message)) if message=="Workflow lease claim refused: run or lease window closed"),
        "the other claimant is refused after the deadline: {other:?}"
    );
    assert_eq!(
        *before,
        all_tables(&s.fixture).await,
        "a void crossing writes nothing"
    );
    void_attempt(
        "claim_abort_deadline_crossing",
        attempt,
        &format!("early={label} outcome=Ok(None) deadline_passed=true other={other:?}"),
    );
}

async fn assert_quiescent(observer: &mut sqlx::PgConnection, pids: &[i32]) {
    tokio::time::timeout(HANG_GUARD, async {
        loop {
            sqlx::query("SELECT pg_stat_clear_snapshot()")
                .execute(&mut *observer).await.unwrap();
            let clean: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_stat_activity AS activity WHERE activity.pid=ANY($1) AND (activity.state='active' OR activity.xact_start IS NOT NULL)) AND NOT EXISTS(SELECT 1 FROM pg_locks AS held WHERE held.pid=ANY($1) AND held.locktype<>'virtualxid')")
                .bind(pids).fetch_one(&mut *observer).await.unwrap();
            if clean { break; }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("claim transactions and locks must drain");
}

async fn persistence_cutoff(s: &Scheduled) {
    let before = all_tables(&s.fixture).await;
    let gate = usage_gate(s).await;
    let mut observer = s.fixture.persistence().pool().acquire().await.unwrap();
    assert!(live_deadline(&mut observer, s).await);
    let claim = s
        .fixture
        .persistence()
        .claim_io(s.request.fence.scope, worker(), policy());
    tokio::pin!(claim);
    let started = Instant::now();
    let pid = wait_for_lock(&mut observer, gate.pid, USAGE_WAIT, claim.as_mut()).await;
    let result = tokio::time::timeout_at(started + policy().supervision_guard(), claim)
        .await
        .expect("the claim's own persistence and SQL bounds settle before the supervision guard");
    // The outer Tokio timer and PostgreSQL lock/statement timers legitimately
    // race. Both supported errors abort; neither an empty result nor a claim does.
    assert!(
        matches!(result, Err(AppError::DatabaseTimeout(ref message)) if matches!(message.as_str(),
            "error returned from database: canceling statement due to lock timeout"
                | "error returned from database: canceling statement due to statement timeout"))
            || matches!(result, Err(AppError::DatabaseTimeout(ref message)) if message=="Workflow lease.claim not applied: persistence bound elapsed before commit"),
        "unsupported timeout outcome: {result:?}"
    );
    eprintln!(
        "usage waiter {pid} blocked by {}, bounded timer returned {result:?} after {:?}",
        gate.pid,
        started.elapsed()
    );
    assert!(
        live_deadline(&mut observer, s).await,
        "timeout is independent of DB deadline"
    );
    gate.tx.rollback().await.unwrap();
    assert_quiescent(&mut observer, &[pid]).await;
    drop(observer);
    assert_eq!(before, all_tables(&s.fixture).await);
    successful_claim(s, &before).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_abort_usage_deadline_and_timeout() {
    // Box real provider/action fixture seams to retain stock 2 MiB test stacks.
    // Each variant must be proven on a fresh fixture within three attempts; void attempts
    // (asserted refusals that wrote nothing) are logged and rebuilt.
    for exhausted in [false, true] {
        let mut proven = false;
        for attempt in 1..=3 {
            let s = Box::pin(scheduled()).await;
            if exhausted {
                Box::pin(exhaust(&s)).await;
            }
            if deadline_crossing(&s, attempt).await {
                proven = true;
                break;
            }
        }
        assert!(
            proven,
            "deadline crossing (exhausted={exhausted}) never proven in 3 attempts"
        );
    }
    let untimed = Box::pin(scheduled()).await;
    persistence_cutoff(&untimed).await;
}

fn expected_fault(error: AppError, kind: ProbeKind) {
    let AppError::Database(message) = error else {
        panic!("deferred commit must return Database, got {error:?}");
    };
    assert_eq!(
        message,
        format!("error returned from database: {}", kind.diagnostic()),
        "only injection after all prerequisites is acceptable"
    );
    eprintln!("public commit fault reached after verified prerequisites: {message}");
}

async fn successful_claim(s: &Scheduled, before: &Value) {
    let claim = s
        .fixture
        .persistence()
        .settled()
        .claim_io(s.request.fence.scope, worker(), policy())
        .await
        .unwrap()
        .expect("unchanged below-limit fixture must claim");
    assert_eq!(claim.fence.attempt.0, s.request.fence.attempt.0 + 1);
    assert!(
        s.fixture
            .persistence()
            .settled()
            .validate_io(claim.fence, policy())
            .await
            .unwrap()
    );
    let after = all_tables(&s.fixture).await;
    assert_eq!(
        after["task_attempts"].as_array().unwrap().len(),
        before["task_attempts"].as_array().unwrap().len() + 1
    );
    for table in [
        "workflow_root_budget_usage",
        "workflow_budget_receipts",
        "workflow_action_claim_episodes",
        "workflow_action_claim_budget_refusals",
        "workflow_action_evidence",
        "workflow_action_evidence_commands",
        "workflow_action_receipts",
        "workflow_action_dispatches",
        "workflow_action_remote_entries",
        "workflow_action_evidence_consumptions",
    ] {
        assert_eq!(before[table], after[table], "claim preserves {table}");
    }
    for attempt in before["task_attempts"].as_array().unwrap() {
        assert!(
            after["task_attempts"].as_array().unwrap().contains(attempt),
            "all retired attempts remain exact"
        );
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_abort_deferred_candidate() {
    // Box real action fixture seams to retain stock 2 MiB test stacks.
    let s = Box::pin(scheduled()).await;
    let before = all_tables(&s.fixture).await;
    let claimant = worker();
    let probe = install_probe(&s, &before, claimant, ProbeKind::Candidate).await;
    let error = s
        .fixture
        .persistence()
        .claim_io(s.request.fence.scope, claimant, policy())
        .await
        .expect_err("candidate must not escape failed commit");
    expected_fault(error, ProbeKind::Candidate);
    assert_eq!(
        before,
        all_tables(&s.fixture).await,
        "installed lease/attempt, fairness, revision and every public table roll back"
    );
    remove_probe(&s, probe).await;
    successful_claim(&s, &before).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_abort_deferred_refusal() {
    // Box real action and descendant budget seams to retain stock 2 MiB stacks.
    let s = Box::pin(scheduled()).await;
    Box::pin(exhaust(&s)).await;
    let before = all_tables(&s.fixture).await;
    let claimant = worker();
    let probe = install_probe(&s, &before, claimant, ProbeKind::Refusal).await;
    let error = s
        .fixture
        .persistence()
        .claim_io(s.request.fence.scope, claimant, policy())
        .await
        .expect_err("refusal None must not escape failed commit");
    expected_fault(error, ProbeKind::Refusal);
    assert_eq!(
        before,
        all_tables(&s.fixture).await,
        "retirement/refusal/audit/revision and every public table roll back"
    );
    remove_probe(&s, probe).await;
    assert!(
        s.fixture
            .persistence()
            .settled()
            .claim_io(s.request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    let after = all_tables(&s.fixture).await;
    assert_refusal(&before, &after, &s.request, &s.command);
    for table in [
        "task_attempts",
        "workflow_root_budget_usage",
        "workflow_action_evidence_consumptions",
        "workflow_action_remote_entries",
    ] {
        assert_eq!(before[table], after[table], "refusal preserves {table}");
    }
}
