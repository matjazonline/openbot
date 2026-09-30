//! Public claim deadline waits and commit failures leave no partial public facts.
use super::*;
use std::{future::Future, pin::Pin};

#[path = "action_reconciliation_claim_abort_probes.rs"]
mod probes;
use probes::{ProbeKind, install_probe, remove_probe};

#[path = "action_reconciliation_claim_cancellation_tests.rs"]
mod cancellation_tests;

const USAGE_WAIT: &str = "SELECT usage.root_run_id FROM workflow_root_budget_usage";
const RUN_WAIT: &str = "SELECT binding_id, max_context_bytes FROM workflow_runs";

async fn wait_for_lock<T: std::fmt::Debug>(
    observer: &mut sqlx::PgConnection,
    blocker: i32,
    statement: &str,
    mut contender: Pin<&mut impl Future<Output = T>>,
) -> i32 {
    let observe = async {
        loop {
            sqlx::query("SELECT pg_stat_clear_snapshot()")
                .execute(&mut *observer)
                .await
                .unwrap();
            let pid = sqlx::query_scalar("SELECT activity.pid FROM pg_stat_activity AS activity WHERE activity.datname=current_database() AND activity.state='active' AND activity.wait_event_type='Lock' AND position($1 IN activity.query)>0 AND $2=ANY(pg_blocking_pids(activity.pid))")
                .bind(statement).bind(blocker).fetch_optional(&mut *observer).await.unwrap();
            if let Some(pid) = pid {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    tokio::select! {
        result = contender.as_mut() => panic!("claim completed before exact blocker {blocker}, statement {statement}: {result:?}"),
        result = tokio::time::timeout(Duration::from_millis(300), observe) => result.expect("exact PostgreSQL lock wait must be observed promptly"),
    }
}

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

async fn deadline_crossing(s: &Scheduled) {
    // A fixture deadline, before the snapshot, leaves a strict margin below the
    // unchanged one-second persistence timeout; missing that margin fails.
    sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp()+interval '450 milliseconds' WHERE company_id=$1 AND id=$2")
        .bind(s.request.scope().company.as_uuid()).bind(s.request.scope().run.as_uuid())
        .execute(s.fixture.persistence().pool()).await.unwrap();
    let before = all_tables(&s.fixture).await;
    let gate = usage_gate(s).await;
    let mut observer = s.fixture.persistence().pool().acquire().await.unwrap();
    let first = s
        .fixture
        .persistence()
        .claim_io(s.request.fence.scope, worker(), policy());
    let second = s
        .fixture
        .persistence()
        .claim_io(s.request.fence.scope, worker(), policy());
    tokio::pin!(first, second);
    let started = Instant::now();
    let first_pid = wait_for_lock(&mut observer, gate.pid, USAGE_WAIT, first.as_mut()).await;
    assert!(
        live_deadline(&mut observer, s).await,
        "usage wait reached before DB deadline"
    );
    let second_pid = tokio::select! {
        result = first.as_mut() => panic!("first claimant escaped usage wait: {result:?}"),
        pid = wait_for_lock(&mut observer, first_pid, RUN_WAIT, second.as_mut()) => pid,
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
    let expired = async {
        while live_deadline(&mut observer, s).await {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    tokio::select! {
        result = first.as_mut() => panic!("first completed before observed DB expiry: {result:?}"),
        result = second.as_mut() => panic!("second completed before observed DB expiry: {result:?}"),
        result = tokio::time::timeout_at(started+Duration::from_millis(750), expired) => result.expect("DB expiry must precede persistence timeout"),
    }
    assert!(
        started.elapsed() < Duration::from_millis(750),
        "release before persistence cutoff"
    );
    eprintln!(
        "observed DB expiry and released usage gate at {:?}",
        started.elapsed()
    );
    gate.tx.rollback().await.unwrap();
    let (first, second) = tokio::time::timeout_at(started + Duration::from_millis(900), async {
        tokio::join!(first, second)
    })
    .await
    .expect("deadline rejection must complete before outer timeout");
    assert!(
        matches!(first, Err(AppError::Conflict(ref message)) if message=="Workflow lease transaction timed out"),
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
}

async fn assert_quiescent(observer: &mut sqlx::PgConnection, pids: &[i32]) {
    tokio::time::timeout(Duration::from_millis(500), async {
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
    let result = tokio::time::timeout_at(started + Duration::from_millis(1500), claim)
        .await
        .expect("existing one-second persistence/SQL timers remain bounded");
    // The outer Tokio timer and PostgreSQL lock/statement timers legitimately
    // race. Both supported errors abort; neither an empty result nor a claim does.
    assert!(
        matches!(result, Err(AppError::DatabaseTimeout(ref message)) if matches!(message.as_str(),
            "error returned from database: canceling statement due to lock timeout"
                | "error returned from database: canceling statement due to statement timeout"))
            || matches!(result, Err(AppError::Conflict(ref message)) if message=="Workflow lease transaction timed out"),
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
    let eligible = Box::pin(scheduled()).await;
    deadline_crossing(&eligible).await;
    let exhausted = Box::pin(scheduled()).await;
    Box::pin(exhaust(&exhausted)).await;
    deadline_crossing(&exhausted).await;
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
        .claim_io(s.request.fence.scope, worker(), policy())
        .await
        .unwrap()
        .expect("unchanged below-limit fixture must claim");
    assert_eq!(claim.fence.attempt.0, s.request.fence.attempt.0 + 1);
    assert!(
        s.fixture
            .persistence()
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
