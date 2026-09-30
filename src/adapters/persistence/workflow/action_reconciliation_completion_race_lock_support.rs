//! Task-owned pauses at existing writes; neither gate owns the business run.
use super::super::super::super::control_serialization_tests::contention::{
    RunObservation, RunProbe, classify_run, require_probe, wait_for_lock,
};
use super::*;
use sqlx::Connection;
use std::{future::Future, pin::Pin};

const PAUSE_KEY: i64 = 470_203;
const RUN_RECEIPT: &str = "SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE";
const RUN_COMPLETION: &str = "SELECT binding_id,max_steps FROM workflow_runs";
const OBSERVATION: &str = "INSERT INTO workflow_action_actual_receipt_observations";
const EXECUTION: &str = "SELECT completed_at,committed_output,committed_route";

pub(super) struct Owners {
    pub(super) first: i32,
    pub(super) second: i32,
}

async fn probe(
    observer: &mut sqlx::PgConnection,
    request: &ActionDispatchRequest,
) -> Result<RunProbe, sqlx::Error> {
    let rows = sqlx::query_as::<_, RunObservation>(
        "SELECT visible.id AS visible_id, \
         (SELECT locked.id FROM workflow_runs AS locked \
          WHERE locked.company_id=visible.company_id AND locked.id=visible.id \
          FOR UPDATE OF locked SKIP LOCKED) AS lockable_id \
         FROM workflow_runs AS visible WHERE visible.company_id=$1 AND visible.id=$2",
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.scope().run.as_uuid())
    .fetch_all(observer)
    .await?;
    classify_run(rows, request.scope().run.as_uuid())
}

async fn checked_probe(observer: &mut sqlx::PgConnection, h: &HeldTruth, expected: RunProbe) {
    require_probe(
        tokio::time::timeout(Duration::from_millis(300), probe(observer, &h.request))
            .await
            .expect("bounded exact run observation"),
        expected,
    )
    .unwrap();
}

pub(super) async fn controls(h: &HeldTruth) {
    let mut observer = h.fixture.persistence().pool().acquire().await.unwrap();
    let unlocked =
        tokio::time::timeout(Duration::from_millis(300), probe(&mut observer, &h.request))
            .await
            .expect("bounded positive unlocked control");
    assert!(matches!(
        require_probe(unlocked, RunProbe::Held),
        Err(sqlx::Error::Protocol(_))
    ));
    let mut missing = h.request.clone();
    missing.fence.scope.run = crate::domain::workflow::RunId::new(Uuid::new_v4());
    let absent = tokio::time::timeout(Duration::from_millis(300), probe(&mut observer, &missing))
        .await
        .expect("bounded missing run control");
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

pub(super) async fn race(
    h: &mut HeldTruth,
    first: Winner,
    gate: &mut sqlx::PgConnection,
) -> RaceResult {
    let old = h.entry.take().unwrap();
    let writer = adapter(&h.fixture, 0);
    let receipt = Box::pin(async {
        assert!(matches!(
            writer.finish_remote(old, h.result.clone()).await.unwrap(),
            RemoteDispatchObservation::Interrupted
        ));
    });
    let completion = Box::pin(h.fixture.persistence().complete_io(FencedWorkflowResult {
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
            first,
        },
        completion.as_mut(),
        receipt.as_mut(),
    ))
    .await;
    checked_probe(&mut observer, h, RunProbe::Free).await;
    quiescent(gate, observer_pid, owners).await;
    result
}

struct FinishContext<'a> {
    h: &'a HeldTruth,
    gate: &'a mut sqlx::PgConnection,
    observer: &'a mut sqlx::PgConnection,
    gate_pid: i32,
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
) -> RaceResult {
    let FinishContext {
        h,
        gate,
        observer,
        gate_pid,
        first,
    } = context;
    let (result, completed_before_receipt) = match first {
        Winner::Completion => {
            let result = tokio::time::timeout(Duration::from_millis(300), async {
                tokio::select! {
                    value = completion.as_mut() => value.unwrap(),
                    value = receipt.as_mut() => panic!("old receipt escaped unreleased actual-write gate: {value:?}"),
                }
            }).await.expect("completion finishes before actual receipt pause releases");
            assert!(result.is_some());
            wait_for_lock(observer, gate_pid, OBSERVATION, receipt.as_mut()).await;
            let completed = all_tables(&h.fixture).await;
            unlock(gate).await;
            (result, Some(completed))
        }
        Winner::Receipt => {
            unlock(gate).await;
            let (result, ()) = tokio::time::timeout(Duration::from_millis(900), async {
                tokio::join!(completion.as_mut(), receipt.as_mut())
            })
            .await
            .expect("both real owners complete after receipt-write gate releases");
            return RaceResult {
                completion: result.unwrap(),
                completed_before_receipt: None,
            };
        }
    };
    tokio::time::timeout(Duration::from_millis(900), receipt)
        .await
        .expect("late actual owner drains");
    RaceResult {
        completion: result,
        completed_before_receipt,
    }
}

async fn quiescent(gate: &mut sqlx::PgConnection, observer: i32, owners: Owners) {
    sqlx::query("SELECT pg_stat_clear_snapshot()")
        .execute(&mut *gate)
        .await
        .unwrap();
    let clean: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_stat_activity AS activity WHERE activity.pid=ANY($1) AND (activity.state='active' OR activity.xact_start IS NOT NULL))")
        .bind([owners.first, owners.second, observer].as_slice()).fetch_one(gate).await.unwrap();
    assert!(
        clean,
        "both production owners and drained observer quiescent"
    );
}
