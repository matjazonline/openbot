//! Task-owned pauses at existing writes; neither gate owns the business run.
use super::super::super::super::control_serialization_tests::contention::{
    RunObservation, RunProbe, classify_run, require_probe, wait_for_lock,
};
use super::queue::{self, QueuePids};
use super::*;
use sqlx::Connection;
use std::{cell::Cell, future::Future, pin::Pin};

const PAUSE_KEY: i64 = 470_203;
const RUN_RECEIPT: &str = "SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE";
const RUN_COMPLETION: &str = "SELECT binding_id,max_steps FROM workflow_runs";
const OBSERVATION: &str = "INSERT INTO workflow_action_actual_receipt_observations";
const EXECUTION: &str = "SELECT completed_at,committed_output,committed_route";

#[derive(Clone, Copy)]
pub(super) struct Owners {
    pub(super) first: i32,
    pub(super) second: i32,
}

/// A real lock wait fails here, on the server, so a stalled disk cannot pass for one.
const PROBE_LOCK_TIMEOUT: &str = "100ms";
/// Only a hang guard: the server-side lock timeout above is the correctness signal.
const PROBE_HANG_GUARD: Duration = Duration::from_secs(10);

async fn probe(
    observer: &mut sqlx::PgConnection,
    request: &ActionDispatchRequest,
) -> Result<RunProbe, sqlx::Error> {
    // Rolled back, so a Free probe's row lock never waits on a synchronous commit flush.
    let mut tx = observer.begin().await?;
    sqlx::query("SELECT set_config('lock_timeout',$1,true)")
        .bind(PROBE_LOCK_TIMEOUT)
        .execute(&mut *tx)
        .await?;
    let rows = sqlx::query_as::<_, RunObservation>(
        "SELECT visible.id AS visible_id, \
         (SELECT locked.id FROM workflow_runs AS locked \
          WHERE locked.company_id=visible.company_id AND locked.id=visible.id \
          FOR UPDATE OF locked SKIP LOCKED) AS lockable_id \
         FROM workflow_runs AS visible WHERE visible.company_id=$1 AND visible.id=$2",
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.scope().run.as_uuid())
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    classify_run(rows, request.scope().run.as_uuid())
}

/// Each probe names itself, so a failure says which observation broke and how long it took.
async fn bounded_probe(
    observer: &mut sqlx::PgConnection,
    request: &ActionDispatchRequest,
    label: &str,
) -> Result<RunProbe, sqlx::Error> {
    let started = std::time::Instant::now();
    let result = tokio::time::timeout(PROBE_HANG_GUARD, probe(observer, request))
        .await
        .unwrap_or_else(|_| panic!("run probe {label} hung past {PROBE_HANG_GUARD:?}"));
    eprintln!(
        "completion/actual-receipt run probe {label} elapsed_ms={} result={result:?}",
        started.elapsed().as_millis()
    );
    result
}

async fn checked_probe(observer: &mut sqlx::PgConnection, h: &HeldTruth, expected: RunProbe) {
    let label = format!("expecting {expected:?}");
    let result = bounded_probe(observer, &h.request, &label).await;
    require_probe(result, expected)
        .unwrap_or_else(|error| panic!("exact run observation {label}: {error:?}"));
}

/// Every backend of this test's own database still in a transaction or holding a lock on
/// workflow_runs, with its transaction age: the run's possible holders.
async fn run_holders(observer: &mut sqlx::PgConnection) -> Value {
    sqlx::query("SELECT pg_stat_clear_snapshot()")
        .execute(&mut *observer)
        .await
        .unwrap();
    sqlx::query_scalar("SELECT COALESCE(jsonb_agg(jsonb_build_object('pid',activity.pid,'state',activity.state,'query',activity.query,'xact_age_ms',(extract(epoch FROM clock_timestamp()-activity.xact_start)*1000)::bigint,'state_age_ms',(extract(epoch FROM clock_timestamp()-activity.state_change)*1000)::bigint,'run_locks',(SELECT jsonb_agg(held.mode) FROM pg_locks AS held WHERE held.pid=activity.pid AND held.relation='workflow_runs'::regclass))),'[]'::jsonb) FROM pg_stat_activity AS activity WHERE activity.datname=current_database() AND activity.pid<>pg_backend_pid() AND (activity.xact_start IS NOT NULL OR EXISTS(SELECT 1 FROM pg_locks AS held WHERE held.pid=activity.pid AND held.relation='workflow_runs'::regclass))")
        .fetch_one(&mut *observer)
        .await
        .unwrap()
}

/// Both owners have returned, but an owner whose transaction ended by drop (an early
/// `return` inside it) is rolled back by sqlx only when its connection's return-to-pool
/// task runs, so under load the run can stay locked briefly after the owner's future
/// resolved. Poll until the exact run is lockable; a holder that persists past the hang
/// guard is a leaked transaction and fails with every possible holder named.
async fn released_run(observer: &mut sqlx::PgConnection, h: &HeldTruth) {
    let started = std::time::Instant::now();
    let mut first_held = None;
    loop {
        match bounded_probe(observer, &h.request, "expecting Free after owners").await {
            Ok(RunProbe::Free) => break,
            Ok(RunProbe::Held) => {}
            Err(error) => panic!("exact run observation expecting Free after owners: {error:?}"),
        }
        let holders = run_holders(observer).await;
        if first_held.is_none() {
            eprintln!(
                "completion/actual-receipt run still held after owners returned holders={holders}"
            );
            first_held = Some(holders.clone());
        }
        assert!(
            started.elapsed() < PROBE_HANG_GUARD,
            "exact run still held {PROBE_HANG_GUARD:?} after both owners returned: holders={holders} first_held={first_held:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    eprintln!(
        "completion/actual-receipt run released after owners elapsed_ms={} was_held={}",
        started.elapsed().as_millis(),
        first_held.is_some()
    );
}

pub(super) async fn controls(h: &HeldTruth) {
    let mut observer = h.fixture.persistence().pool().acquire().await.unwrap();
    let unlocked = bounded_probe(&mut observer, &h.request, "unlocked control").await;
    assert!(matches!(
        require_probe(unlocked, RunProbe::Held),
        Err(sqlx::Error::Protocol(_))
    ));
    let mut missing = h.request.clone();
    missing.fence.scope.run = crate::domain::workflow::RunId::new(Uuid::new_v4());
    let absent = bounded_probe(&mut observer, &missing, "missing run control").await;
    assert!(matches!(
        require_probe(absent, RunProbe::Held),
        Err(sqlx::Error::RowNotFound)
    ));
    eprintln!(
        "completion/actual-receipt probe controls unlocked_rejected=true missing_rejected=true"
    );
}

pub(super) async fn install_pause(h: &HeldTruth, gate: &mut sqlx::PgConnection) {
    // The isolated database belongs to this test. Match only its authentic OLD
    // entry: the NEW result must still commit while this advisory gate is held.
    sqlx::query(&format!(
        "CREATE FUNCTION completion_receipt_pause() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN PERFORM pg_advisory_xact_lock({PAUSE_KEY}); RETURN NEW; END $$"
    ))
    .execute(&mut *gate)
    .await
    .unwrap();
    sqlx::query(&format!(
        "CREATE TRIGGER completion_receipt_pause BEFORE INSERT \
         ON workflow_action_actual_receipt_observations FOR EACH ROW \
         WHEN (NEW.remote_entry_id='{}'::uuid) EXECUTE FUNCTION completion_receipt_pause()",
        h.entry_id
    ))
    .execute(&mut *gate)
    .await
    .unwrap();
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(PAUSE_KEY)
        .execute(gate)
        .await
        .unwrap();
}

pub(super) async fn remove_pause(gate: &mut sqlx::PgConnection) {
    sqlx::query(
        "DROP TRIGGER completion_receipt_pause ON workflow_action_actual_receipt_observations",
    )
    .execute(&mut *gate)
    .await
    .unwrap();
    sqlx::query("DROP FUNCTION completion_receipt_pause()")
        .execute(&mut *gate)
        .await
        .unwrap();
    let clean: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgname='completion_receipt_pause') AND NOT EXISTS(SELECT 1 FROM pg_proc WHERE proname='completion_receipt_pause') AND NOT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND database=(SELECT oid FROM pg_database WHERE datname=current_database()))")
        .fetch_one(gate).await.unwrap();
    assert!(
        clean,
        "all task-owned trigger/function/advisory gates removed"
    );
}

async fn unlock(gate: &mut sqlx::PgConnection) {
    let released: bool = sqlx::query_scalar("SELECT pg_advisory_unlock($1)")
        .bind(PAUSE_KEY)
        .fetch_one(gate)
        .await
        .unwrap();
    assert!(released);
}

async fn observe_owners<C: std::fmt::Debug, R: std::fmt::Debug>(
    observer: &mut sqlx::PgConnection,
    h: &HeldTruth,
    gate_pid: i32,
    first: Winner,
    mut completion: Pin<&mut impl Future<Output = C>>,
    mut receipt: Pin<&mut impl Future<Output = R>>,
) -> Owners {
    checked_probe(observer, h, RunProbe::Free).await;
    let first_pid = match first {
        Winner::Completion => {
            wait_for_lock(observer, gate_pid, EXECUTION, completion.as_mut()).await
        }
        Winner::Receipt => wait_for_lock(observer, gate_pid, OBSERVATION, receipt.as_mut()).await,
    };
    // The gate locks only execution/advisory, and the drained unlocked control
    // above excludes it. Thus a held exact run now identifies the real first owner.
    checked_probe(observer, h, RunProbe::Held).await;
    let second = match first {
        Winner::Completion => tokio::select! {
            result = completion.as_mut() => panic!("completion escaped execution gate: {result:?}"),
            pid = wait_for_lock(observer, first_pid, RUN_RECEIPT, receipt.as_mut()) => pid,
        },
        Winner::Receipt => tokio::select! {
            result = receipt.as_mut() => panic!("receipt escaped actual-write pause: {result:?}"),
            pid = wait_for_lock(observer, first_pid, RUN_COMPLETION, completion.as_mut()) => pid,
        },
    };
    assert_ne!(first_pid, second);
    assert_ne!(first_pid, gate_pid);
    assert_ne!(second, gate_pid);
    eprintln!(
        "completion/actual-receipt owner_order={first:?} gate_pid={gate_pid} first_pid={first_pid} second_pid={second} first_blocked_by_gate=true second_blocked_by_first=true exact_run_held=true"
    );
    Owners {
        first: first_pid,
        second,
    }
}

pub(super) struct RaceResult {
    pub(super) completion: Option<crate::application::workflow::completion::CommittedWorkflowStep>,
    pub(super) completed_before_receipt: Option<Value>,
}

/// How one race on a fresh fixture ended.
pub(super) enum Raced {
    Proven(RaceResult),
    /// Receipt first only: the completion's attempt bound elapsed while it queued behind the
    /// receipt's COMMIT, as the snapshots recorded. The order is unproven; the caller asserts
    /// the backstop and races a fresh fixture. Completion first is never void.
    Void(queue::CommitQueue),
}

fn completion_policy() -> crate::application::workflow::lease::LeasePolicy {
    crate::application::workflow::lease::LeasePolicy::new(std::time::Duration::from_secs(3))
        .unwrap()
}

pub(super) async fn race(h: &mut HeldTruth, first: Winner, gate: &mut sqlx::PgConnection) -> Raced {
    let old = h.entry.take().unwrap();
    let writer = adapter(&h.fixture, 0);
    let receipt = Box::pin(async {
        assert!(matches!(
            writer.finish_remote(old, h.result.clone()).await.unwrap(),
            RemoteDispatchObservation::Interrupted
        ));
    });
    let completion = Box::pin(h.fixture.persistence().complete_io(FencedWorkflowResult {
        policy: completion_policy(),
        fence: h.request.fence,
        output: h.result.clone(),
    }));
    tokio::pin!(receipt, completion);
    let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    let mut execution_gate = gate.begin().await.unwrap();
    if matches!(first, Winner::Completion) {
        sqlx::query("SELECT id FROM workflow_executions WHERE company_id=$1 AND id=$2 FOR UPDATE")
            .bind(h.request.scope().company.as_uuid())
            .bind(h.request.scope().execution.as_uuid())
            .execute(&mut *execution_gate)
            .await
            .unwrap();
    }
    let mut observer = h.fixture.persistence().pool().acquire().await.unwrap();
    let observer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *observer)
        .await
        .unwrap();
    let owners = Box::pin(observe_owners(
        &mut observer,
        h,
        gate_pid,
        first,
        completion.as_mut(),
        receipt.as_mut(),
    ))
    .await;
    execution_gate.rollback().await.unwrap();
    let result = Box::pin(settle(
        FinishContext {
            h,
            gate,
            observer: &mut observer,
            gate_pid,
            owners,
            first,
        },
        completion.as_mut(),
        receipt.as_mut(),
    ))
    .await;
    released_run(&mut observer, h).await;
    quiescent(gate, observer_pid, owners).await;
    result
}

struct FinishContext<'a> {
    h: &'a HeldTruth,
    gate: &'a mut sqlx::PgConnection,
    observer: &'a mut sqlx::PgConnection,
    gate_pid: i32,
    owners: Owners,
    first: Winner,
}

async fn settle(
    context: FinishContext<'_>,
    mut completion: Pin<
        &mut impl Future<
            Output = AppResult<
                Option<crate::application::workflow::completion::CommittedWorkflowStep>,
            >,
        >,
    >,
    mut receipt: Pin<&mut impl Future<Output = ()>>,
) -> Raced {
    let FinishContext {
        h,
        gate,
        observer,
        gate_pid,
        owners,
        first,
    } = context;
    require_guard_above(h.request.lease);
    let (result, completed_before_receipt) = match first {
        Winner::Completion => {
            // The still-held advisory gate keeps the receipt paused and select! fails if
            // it escapes, so the order is structural. A completion that waited on the
            // paused receipt fails on complete_on's server-side lock_timeout instead.
            let result = guarded(
                observer,
                "completion finishes before actual receipt pause releases",
                async {
                    tokio::select! {
                        value = completion.as_mut() => value.unwrap_or_else(|error| panic!("completion failed while actual receipt paused ({}): {error:?}", error_class(&error))),
                        value = receipt.as_mut() => panic!("old receipt escaped unreleased actual-write gate: {value:?}"),
                    }
                },
            )
            .await;
            assert!(result.is_some());
            wait_for_lock(observer, gate_pid, OBSERVATION, receipt.as_mut()).await;
            let completed = all_tables(&h.fixture).await;
            unlock(gate).await;
            (result, Some(completed))
        }
        Winner::Receipt => {
            let pids = QueuePids {
                receipt: owners.first,
                completion: owners.second,
            };
            return Box::pin(receipt_first(gate, observer, pids, completion, receipt)).await;
        }
    };
    guarded(observer, "late actual owner drains", receipt).await;
    Raced::Proven(RaceResult {
        completion: result,
        completed_before_receipt,
    })
}

/// Releases the receipt and lets both owners finish, sampling the completion's run-lock
/// queue meanwhile. A completion `DatabaseTimeout` is void only with the queue evidence;
/// without it, it fails as before.
async fn receipt_first(
    gate: &mut sqlx::PgConnection,
    observer: &mut sqlx::PgConnection,
    pids: QueuePids,
    completion: Pin<
        &mut impl Future<
            Output = AppResult<
                Option<crate::application::workflow::completion::CommittedWorkflowStep>,
            >,
        >,
    >,
    receipt: Pin<&mut impl Future<Output = ()>>,
) -> Raced {
    unlock(gate).await;
    let unlocked = std::time::Instant::now();
    let returned = Cell::new(None);
    let completion = async {
        let result = completion.await;
        returned.set(Some(std::time::Instant::now()));
        result
    };
    let (result, (), samples) = guarded(
        observer,
        "both real owners complete after receipt-write gate releases",
        async {
            tokio::join!(
                completion,
                receipt,
                Box::pin(queue::samples_until(gate, pids, unlocked, &returned))
            )
        },
    )
    .await;
    match result {
        Err(AppError::DatabaseTimeout(error)) => {
            let returned_ms = returned
                .get()
                .expect("the completion returned")
                .duration_since(unlocked)
                .as_millis();
            let attempt_bound = completion_policy().attempt_bound();
            Raced::Void(
                queue::commit_queue(&samples, pids, returned_ms, attempt_bound, error)
                    .unwrap_or_else(|missing| {
                        panic!(
                            "completion DatabaseTimeout without evidence that its bound elapsed queued behind the receipt's COMMIT: {missing}"
                        )
                    }),
            )
        }
        result => Raced::Proven(RaceResult {
            completion: result.unwrap(),
            completed_before_receipt: None,
        }),
    }
}

/// Only a hang guard around real owner commits. A completion that waits on the paused
/// receipt fails on the server at its bounded lock_timeout (the chain ends at an idle
/// advisory gate, so there is no deadlock cycle); so does the receipt side, whose
/// transaction carries the same server bound from BEGIN.
/// A commit that stalls on a WAL flush under parallel load is measured, not failed.
const OWNER_HANG_GUARD: Duration = Duration::from_secs(10);

/// The guard must stay above every production persistence bound the owners spend, or
/// a later bound raise would surface here as a hang instead of the owner's own error.
fn require_guard_above(lease: crate::application::workflow::lease::LeasePolicy) {
    let bound = lease.supervision_guard();
    assert!(
        OWNER_HANG_GUARD > bound,
        "owner hang guard {OWNER_HANG_GUARD:?} must exceed the production persistence bound {bound:?}"
    );
}

fn error_class(error: &AppError) -> &'static str {
    match error {
        AppError::DatabaseTimeout(_) => "server lock/statement timeout",
        AppError::Conflict(_) => "conflict or client persistence bound",
        AppError::Database(_) => "database error",
        _ => "other error",
    }
}

/// Names its bound and logs the measured duration; on a hang it reports backend state.
async fn guarded<T>(
    observer: &mut sqlx::PgConnection,
    label: &str,
    owners: impl Future<Output = T>,
) -> T {
    let started = std::time::Instant::now();
    let result = tokio::time::timeout(OWNER_HANG_GUARD, owners).await;
    let elapsed_ms = started.elapsed().as_millis();
    eprintln!(
        "completion/actual-receipt owners {label} elapsed_ms={elapsed_ms} hang_guard={OWNER_HANG_GUARD:?} hung={}",
        result.is_err()
    );
    let Ok(value) = result else {
        let state = tokio::time::timeout(OWNER_HANG_GUARD, sqlx::query_scalar::<_, Value>("SELECT COALESCE(jsonb_agg(jsonb_build_object('pid',activity.pid,'state',activity.state,'query',activity.query,'wait_type',activity.wait_event_type,'wait',activity.wait_event,'blockers',pg_blocking_pids(activity.pid))),'[]'::jsonb) FROM pg_stat_activity AS activity WHERE activity.datname=current_database() AND activity.pid<>pg_backend_pid()")
            .fetch_one(&mut *observer)).await;
        panic!(
            "owners {label}: hung past hang guard {OWNER_HANG_GUARD:?} elapsed_ms={elapsed_ms} backend_state={state:?}"
        );
    };
    value
}

/// Polled with a fresh statistics snapshot each time: under load a returned connection can
/// still be flushing its queued ROLLBACK or ping, so `xact_start` is briefly non-null. A
/// genuinely leaked transaction still fails at the hang guard.
async fn quiescent(gate: &mut sqlx::PgConnection, observer: i32, owners: Owners) {
    let started = std::time::Instant::now();
    loop {
        sqlx::query("SELECT pg_stat_clear_snapshot()")
            .execute(&mut *gate)
            .await
            .unwrap();
        let clean: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_stat_activity AS activity WHERE activity.pid=ANY($1) AND (activity.state='active' OR activity.xact_start IS NOT NULL))")
            .bind([owners.first, owners.second, observer].as_slice()).fetch_one(&mut *gate).await.unwrap();
        if clean {
            return;
        }
        assert!(
            started.elapsed() < OWNER_HANG_GUARD,
            "both production owners and drained observer quiescent within {OWNER_HANG_GUARD:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}
