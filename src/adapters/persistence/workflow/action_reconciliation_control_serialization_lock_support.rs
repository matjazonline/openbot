//! Observe real owners without bypassing company authorization or run ownership.
use super::*;
use sqlx::Connection;
use std::{cell::Cell, future::Future, pin::Pin, time::Instant};

#[derive(Debug, PartialEq, Eq)]
pub(in super::super) enum RunProbe {
    Free,
    Held,
}
struct OwnerPids {
    first: i32,
    second: i32,
}
struct ControlOrder {
    owner: Terminal,
    first: First,
}
struct ProbeBackends<'a> {
    gate: &'a mut sqlx::PgConnection,
    gate_pid: i32,
    observer_pid: i32,
}

#[derive(sqlx::FromRow)]
pub(in super::super) struct RunObservation {
    pub(in super::super) visible_id: Uuid,
    pub(in super::super) lockable_id: Option<Uuid>,
}

pub(in super::super) fn classify_run(
    rows: Vec<RunObservation>,
    run: Uuid,
) -> Result<RunProbe, sqlx::Error> {
    let [row] = rows.as_slice() else {
        return if rows.is_empty() {
            Err(sqlx::Error::RowNotFound)
        } else {
            Err(sqlx::Error::Protocol(
                "exact run probe returned extra rows".into(),
            ))
        };
    };
    if row.visible_id != run {
        return Err(sqlx::Error::Protocol(
            "exact run probe returned wrong visible ID".into(),
        ));
    }
    match row.lockable_id {
        None => Ok(RunProbe::Held),
        Some(id) if id == run => Ok(RunProbe::Free),
        Some(_) => Err(sqlx::Error::Protocol(
            "exact run probe returned wrong lockable ID".into(),
        )),
    }
}

pub(in super::super) fn require_probe(
    result: Result<RunProbe, sqlx::Error>,
    expected: RunProbe,
) -> Result<(), sqlx::Error> {
    let actual = result?;
    if actual != expected {
        return Err(sqlx::Error::Protocol(format!(
            "exact run probe expected {expected:?}, observed {actual:?}"
        )));
    }
    Ok(())
}

async fn probe_run(
    observer: &mut sqlx::PgConnection,
    command: &ReconcileActionCommand,
    phase: &Cell<&'static str>,
) -> Result<RunProbe, sqlx::Error> {
    let started = Instant::now();
    phase.set("query_and_drain");
    eprintln!(
        "probe run={} company={} phase=query_and_drain elapsed_us=0",
        command.scope.run.as_uuid(),
        command.scope.company.as_uuid()
    );
    // fetch_all consumes through PostgreSQL ReadyForQuery, releasing successful
    // autocommit row locks before either genuine business owner can advance.
    let rows = sqlx::query_as::<_, RunObservation>(
        "SELECT visible.id AS visible_id, \
         (SELECT locked.id FROM workflow_runs AS locked \
          WHERE locked.company_id=visible.company_id AND locked.id=visible.id \
          FOR UPDATE OF locked SKIP LOCKED) AS lockable_id \
         FROM workflow_runs AS visible WHERE visible.company_id=$1 AND visible.id=$2",
    )
    .bind(command.scope.company.as_uuid())
    .bind(command.scope.run.as_uuid())
    .fetch_all(&mut *observer)
    .await;
    phase.set("done");
    eprintln!(
        "probe run={} company={} phase=done elapsed_us={} sqlstate={:?}",
        command.scope.run.as_uuid(),
        command.scope.company.as_uuid(),
        started.elapsed().as_micros(),
        rows.as_ref()
            .err()
            .and_then(|error| error.as_database_error())
            .and_then(|error| error.code())
    );
    classify_run(rows?, command.scope.run.as_uuid())
}

async fn held_run<T: std::fmt::Debug>(
    observer: &mut sqlx::PgConnection,
    backends: &mut ProbeBackends<'_>,
    first_pid: i32,
    command: &ReconcileActionCommand,
    mut first: Pin<&mut impl Future<Output = T>>,
) {
    let phase = Cell::new("not_polled");
    let started = Instant::now();
    eprintln!(
        "held probe run={} company={} first_pid={first_pid} gate_pid={} observer_pid={}",
        command.scope.run.as_uuid(),
        command.scope.company.as_uuid(),
        backends.gate_pid,
        backends.observer_pid
    );
    let observed = tokio::select! {
        result = first.as_mut() => panic!("first owner escaped execution gate during run probe: {result:?}"),
        result = tokio::time::timeout(Duration::from_millis(300), probe_run(observer, command, &phase)) => result,
    };
    if observed.is_err() {
        // Use the retained execution gate connection to capture actual backend
        // state before panic force-drops this isolated fixture. It never supplies
        // authorization or a business transition, and has the same300ms bound.
        let state = tokio::time::timeout(Duration::from_millis(300), sqlx::query_scalar::<_, Value>("SELECT COALESCE(jsonb_agg(jsonb_build_object('pid',activity.pid,'state',activity.state,'query',activity.query,'wait_type',activity.wait_event_type,'wait',activity.wait_event,'blockers',pg_blocking_pids(activity.pid))),'[]'::jsonb) FROM pg_stat_activity AS activity WHERE activity.pid=ANY($1)")
            .bind([first_pid,backends.gate_pid,backends.observer_pid].as_slice()).fetch_one(&mut *backends.gate)).await;
        panic!(
            "bounded exact run probe phase={} elapsed_us={} first_pid={first_pid} gate_pid={} observer_pid={} backend_state={state:?}",
            phase.get(),
            started.elapsed().as_micros(),
            backends.gate_pid,
            backends.observer_pid
        );
    }
    require_probe(observed.unwrap(), RunProbe::Held).unwrap();
}

pub(in super::super) async fn wait_for_lock<T: std::fmt::Debug>(
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
        result = contender.as_mut() => panic!("owner completed before observed lock competition: {result:?}"),
        result = tokio::time::timeout(Duration::from_millis(300), observe) => result.expect("exact competing PostgreSQL lock wait"),
    }
}

async fn observe_owners<R: std::fmt::Debug, T: std::fmt::Debug>(
    observer: &mut sqlx::PgConnection,
    backends: &mut ProbeBackends<'_>,
    command: &ReconcileActionCommand,
    order: ControlOrder,
    mut reconcile: Pin<&mut impl Future<Output = R>>,
    mut terminal: Pin<&mut impl Future<Output = T>>,
    release: &Notify,
) -> OwnerPids {
    let execution_wait = "SELECT id FROM workflow_executions";
    let company_wait = "SELECT user_id FROM companies WHERE id = $1 FOR NO KEY UPDATE";
    let run_wait = "SELECT run.company_id,run.id,run.workflow_id";
    let pids = match order.first {
        First::Command => {
            release.notify_one();
            let pid = wait_for_lock(
                observer,
                backends.gate_pid,
                execution_wait,
                reconcile.as_mut(),
            )
            .await;
            held_run(observer, backends, pid, command, reconcile.as_mut()).await;
            let statement = match order.owner {
                Terminal::Cancel => company_wait,
                Terminal::Expire => "SELECT id FROM workflow_runs",
            };
            let second = tokio::select! {
                result = reconcile.as_mut() => panic!("settlement escaped execution gate: {result:?}"),
                pid = wait_for_lock(observer, pid, statement, terminal.as_mut()) => pid,
            };
            OwnerPids { first: pid, second }
        }
        First::Terminal => {
            let pid = wait_for_lock(
                observer,
                backends.gate_pid,
                execution_wait,
                terminal.as_mut(),
            )
            .await;
            held_run(observer, backends, pid, command, terminal.as_mut()).await;
            release.notify_one();
            let statement = match order.owner {
                Terminal::Cancel => company_wait,
                Terminal::Expire => run_wait,
            };
            let second = tokio::select! {
                result = terminal.as_mut() => panic!("terminal owner escaped execution gate: {result:?}"),
                pid = wait_for_lock(observer, pid, statement, reconcile.as_mut()) => pid,
            };
            OwnerPids { first: pid, second }
        }
    };
    assert_ne!(pids.first, pids.second);
    assert_ne!(pids.first, backends.gate_pid);
    assert_ne!(pids.second, backends.gate_pid);
    pids
}

async fn inspect_finished_owners(
    observer: &mut sqlx::PgConnection,
    backends: ProbeBackends<'_>,
    command: &ReconcileActionCommand,
    pids: OwnerPids,
) {
    let phase = Cell::new("not_polled");
    require_probe(
        tokio::time::timeout(
            Duration::from_millis(300),
            probe_run(observer, command, &phase),
        )
        .await
        .expect("both owners release the exact run"),
        RunProbe::Free,
    )
    .unwrap();
    // The retained gate backend is distinct from both owners and the observer.
    // A fresh pool acquire could reuse an owner PID and observe its own query.
    sqlx::query("SELECT pg_stat_clear_snapshot()")
        .execute(&mut *backends.gate)
        .await
        .unwrap();
    let clean: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_stat_activity AS activity WHERE activity.pid=ANY($1) AND (activity.state='active' OR activity.xact_start IS NOT NULL))")
        .bind([pids.first, pids.second, backends.observer_pid].as_slice())
        .fetch_one(&mut *backends.gate).await.unwrap();
    assert!(
        clean,
        "both owners and observer drained without active transactions or queries"
    );
    eprintln!(
        "probe run={} company={} phase=quiescent first_pid={} second_pid={} observer_pid={} inspector_pid={} clean={clean}",
        command.scope.run.as_uuid(),
        command.scope.company.as_uuid(),
        pids.first,
        pids.second,
        backends.observer_pid,
        backends.gate_pid
    );
}

pub(super) async fn race(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    command: &ReconcileActionCommand,
    verifier: Arc<ObservedVerifier>,
    owner: Terminal,
    first: First,
) -> (ReconciliationResult, TerminalResult) {
    let (started, release) = verifier.pause.as_ref().unwrap();
    let reconcile = Box::pin(execute(f, command, verifier.clone()));
    tokio::pin!(reconcile);
    tokio::select! {
        _ = started.notified() => {},
        result = reconcile.as_mut() => panic!("verification escaped pause: {result:?}"),
    }
    // Each fixture owns its database. The execution-only gate is the sole
    // writer before either owner starts; the positive probe excludes a run lock
    // from that gate. Thus the first owner alone explains the later visible-but-unlockable result.
    let mut gate_connection = f.persistence().pool().acquire().await.unwrap();
    let mut gate = gate_connection.begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM workflow_executions WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(request.scope().company.as_uuid())
        .bind(request.scope().execution.as_uuid())
        .execute(&mut *gate)
        .await
        .unwrap();
    let mut observer = f.persistence().pool().acquire().await.unwrap();
    let observer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *observer)
        .await
        .unwrap();
    let free_phase = Cell::new("not_polled");
    require_probe(
        tokio::time::timeout(
            Duration::from_millis(300),
            probe_run(&mut observer, command, &free_phase),
        )
        .await
        .expect("execution gate does not own run"),
        RunProbe::Free,
    )
    .unwrap();
    let terminal = terminal(f, request, command, owner);
    tokio::pin!(terminal);
    let pids = observe_owners(
        &mut observer,
        &mut ProbeBackends {
            gate: &mut gate,
            gate_pid: blocker,
            observer_pid,
        },
        command,
        ControlOrder { owner, first },
        reconcile.as_mut(),
        terminal.as_mut(),
        release,
    )
    .await;
    gate.rollback().await.unwrap();
    let (result, terminal) = tokio::time::timeout(Duration::from_millis(900), async {
        tokio::join!(reconcile, terminal)
    })
    .await
    .expect("both real owners finish after execution gate release");
    // Box the extracted observation phase to keep the stock-stack race frame shallow.
    Box::pin(inspect_finished_owners(
        &mut observer,
        ProbeBackends {
            gate: &mut gate_connection,
            gate_pid: blocker,
            observer_pid,
        },
        command,
        pids,
    ))
    .await;
    (result.unwrap(), terminal)
}

#[tokio::test]
async fn workflow_action_reconciliation_control_serialization_probe_negative_controls() {
    let History {
        fixture: f,
        request,
        ledger,
    } = Box::pin(history(Truth::Unknown)).await;
    let verifier = Arc::new(ObservedVerifier {
        ledger,
        calls: AtomicUsize::new(0),
        pause: None,
    });
    let command = verified_command(&f, &request, &verifier, "probe-controls").await;
    let mut gate_connection = f.persistence().pool().acquire().await.unwrap();
    let mut gate = gate_connection.begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_executions WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(request.scope().company.as_uuid())
        .bind(request.scope().execution.as_uuid())
        .execute(&mut *gate)
        .await
        .unwrap();
    let mut observer = f.persistence().pool().acquire().await.unwrap();
    let observer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *observer)
        .await
        .unwrap();
    let phase = Cell::new("not_polled");
    let unlocked = tokio::time::timeout(
        Duration::from_millis(300),
        probe_run(&mut observer, &command, &phase),
    )
    .await
    .expect("bounded unlocked control");
    assert!(
        matches!(
            require_probe(unlocked, RunProbe::Held),
            Err(sqlx::Error::Protocol(_))
        ),
        "execution gate alone cannot be classified as a held run"
    );
    let mut missing = command.clone();
    missing.scope.run = crate::domain::workflow::RunId::new(Uuid::new_v4());
    let absent = tokio::time::timeout(
        Duration::from_millis(300),
        probe_run(&mut observer, &missing, &phase),
    )
    .await
    .expect("bounded missing control");
    assert!(
        matches!(
            require_probe(absent, RunProbe::Held),
            Err(sqlx::Error::RowNotFound)
        ),
        "a missing scoped run fails observation rather than proving contention"
    );
    gate.rollback().await.unwrap();
    sqlx::query("SELECT pg_stat_clear_snapshot()")
        .execute(&mut *gate_connection)
        .await
        .unwrap();
    let clean: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_stat_activity AS activity WHERE activity.pid=$1 AND (activity.state='active' OR activity.xact_start IS NOT NULL))")
        .bind(observer_pid).fetch_one(&mut *gate_connection).await.unwrap();
    assert!(clean, "negative-control observer fully drained");
    eprintln!(
        "probe controls unlocked_rejected=true missing_rejected=true observer_pid={observer_pid} clean={clean}"
    );
}
