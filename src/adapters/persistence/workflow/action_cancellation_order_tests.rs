//! Exact PostgreSQL blockers distinguish lock ordering from an accidental timeout.
use super::*;
use std::{future::Future, pin::Pin};

#[derive(Clone, Copy, Debug)]
enum Boundary {
    Local,
    Reserve,
    Enter,
    Return,
    Authority,
}
#[derive(Clone, Copy, Debug)]
enum First {
    Action,
    Cancel,
}

async fn command(f: &AdmissionFixture, request: &ActionDispatchRequest) -> CancelCommand {
    CancelCommand {
        company_id: request.scope().company,
        run_id: request.scope().run,
        actor: f.binding.target.actor,
        command_key: IdempotencyKey::parse("action-cancel-order").unwrap(),
        expected_revision: f
            .persistence()
            .head(request.scope().company, request.scope().run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    }
}

async fn reservation(writer: &Dispatch, request: &ActionDispatchRequest) -> RemoteReservation {
    let RemoteReservationResult::Reserved(reservation) =
        writer.reserve_remote(request, None).await.unwrap()
    else {
        panic!("genuine first reservation required")
    };
    *reservation
}

struct Prepared {
    boundary: Boundary,
    reservation: Option<RemoteReservation>,
    entry: Option<RemoteEntry>,
}
impl Prepared {
    async fn new(boundary: Boundary, writer: &Dispatch, request: &ActionDispatchRequest) -> Self {
        let reservation = match boundary {
            Boundary::Enter | Boundary::Return => Some(reservation(writer, request).await),
            _ => None,
        };
        let (reservation, entry) = if matches!(boundary, Boundary::Return) {
            let entry = writer.enter_remote(reservation.unwrap()).await.unwrap();
            (None, Some(entry))
        } else {
            (reservation, None)
        };
        Self {
            boundary,
            reservation,
            entry,
        }
    }
    async fn run(
        self,
        f: &AdmissionFixture,
        writer: &Dispatch,
        request: &ActionDispatchRequest,
    ) -> AppResult<bool> {
        match self.boundary {
            Boundary::Local => writer
                .local(request)
                .await
                .map(|result| matches!(result, LocalDispatchResult::Committed(_))),
            Boundary::Reserve => writer
                .reserve_remote(request, None)
                .await
                .map(|result| matches!(result, RemoteReservationResult::Reserved(_))),
            Boundary::Enter => writer
                .enter_remote(self.reservation.unwrap())
                .await
                .map(|_| true),
            Boundary::Return => writer.return_authorized(self.entry.as_ref().unwrap()).await,
            Boundary::Authority => f
                .persistence()
                .action_authority(request.scope(), &request.subject)
                .await
                .map(|_| true),
        }
    }
}

async fn blocked<T: std::fmt::Debug>(
    observer: &mut sqlx::PgConnection,
    blocker: i32,
    statement: &str,
    mut work: Pin<&mut impl Future<Output = T>>,
) -> i32 {
    let observe = async {
        loop {
            sqlx::query("SELECT pg_stat_clear_snapshot()")
                .execute(&mut *observer)
                .await
                .unwrap();
            let pid: Option<i32> = sqlx::query_scalar("SELECT activity.pid FROM pg_stat_activity AS activity WHERE activity.datname=current_database() AND activity.state='active' AND activity.wait_event_type='Lock' AND position($1 IN activity.query)>0 AND $2=ANY(pg_blocking_pids(activity.pid))")
                .bind(statement).bind(blocker).fetch_optional(&mut *observer).await.unwrap();
            if let Some(pid) = pid {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    tokio::select! {
        result = work.as_mut() => panic!("contender completed before exact wait on {statement}: {result:?}"),
        result = tokio::time::timeout(Duration::from_secs(10), observe) => {
            let pid = result.expect("exact backend blocker must be observed");
            eprintln!("action cancellation blocker={blocker} waiter={pid} statement={statement}");
            pid
        }
    }
}

#[derive(Debug)]
enum RunLock {
    Free,
    Held,
}

async fn probe_run(
    observer: &mut sqlx::PgConnection,
    request: &ActionDispatchRequest,
    expected: RunLock,
) {
    let probe = async {
        let mut tx = sqlx::Connection::begin(observer).await.unwrap();
        let rows = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE NOWAIT",
        )
        .bind(request.scope().company.as_uuid())
        .bind(request.scope().run.as_uuid())
        .fetch_all(&mut *tx)
        .await;
        // Drain ReadyForQuery on both successful and failed lock attempts.
        tx.rollback().await.unwrap();
        match expected {
            RunLock::Free => assert_eq!(rows.unwrap(), vec![request.scope().run.as_uuid()]),
            RunLock::Held => assert_eq!(
                rows.unwrap_err()
                    .as_database_error()
                    .unwrap()
                    .code()
                    .as_deref(),
                Some("55P03")
            ),
        }
        eprintln!(
            "exact run probe company={} run={} expected={expected:?} drained=true",
            request.scope().company.as_uuid(),
            request.scope().run.as_uuid()
        );
    };
    tokio::time::timeout(Duration::from_secs(10), probe)
        .await
        .expect("bounded exact-run probe and rollback");
}

async fn race(boundary: Boundary, first: First) {
    let (f, request) = Box::pin(setup()).await;
    let writer = adapter(&f, 0);
    let prepared = Prepared::new(boundary, &writer, &request).await;
    let command = command(&f, &request).await;
    let before = facts(&f).await;
    let mut gate = f.persistence().pool().begin().await.unwrap();
    let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    let gate_sql = match first {
        First::Action => "SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE",
        First::Cancel => {
            "SELECT id FROM workflow_executions WHERE company_id=$1 AND id=$2 FOR UPDATE"
        }
    };
    let gate_id = match first {
        First::Action => request.scope().run.as_uuid(),
        First::Cancel => request.scope().execution.as_uuid(),
    };
    sqlx::query(gate_sql)
        .bind(request.scope().company.as_uuid())
        .bind(gate_id)
        .execute(&mut *gate)
        .await
        .unwrap();
    let mut observer = f.persistence().pool().acquire().await.unwrap();
    if matches!(first, First::Cancel) {
        probe_run(&mut observer, &request, RunLock::Free).await;
    }
    let action = prepared.run(&f, &writer, &request);
    let cancel = f.persistence().cancel(command);
    tokio::pin!(action, cancel);
    let company_wait = "SELECT user_id FROM companies";
    let (first_pid, second_pid) = match first {
        First::Action => {
            let pid = blocked(
                &mut observer,
                gate_pid,
                "FROM workflow_runs",
                action.as_mut(),
            )
            .await;
            let second = tokio::select! {
                result = action.as_mut() => panic!("action escaped run gate: {result:?}"),
                pid = blocked(&mut observer, pid, company_wait, cancel.as_mut()) => pid,
            };
            (pid, second)
        }
        First::Cancel => {
            let pid = blocked(
                &mut observer,
                gate_pid,
                "SELECT id FROM workflow_executions",
                cancel.as_mut(),
            )
            .await;
            probe_run(&mut observer, &request, RunLock::Held).await;
            let second = tokio::select! {
                result = cancel.as_mut() => panic!("cancel escaped execution gate: {result:?}"),
                pid = blocked(&mut observer, pid, company_wait, action.as_mut()) => pid,
            };
            (pid, second)
        }
    };
    assert_ne!(first_pid, second_pid);
    gate.rollback().await.unwrap();
    let (action, cancel) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(action, cancel)
    })
    .await
    .expect("both real operations drain");
    let expected = expected_facts(boundary, first, action, cancel, before);
    assert_eq!(facts(&f).await, expected);
    assert_cancelled(&f, &request).await;
    probe_run(&mut observer, &request, RunLock::Free).await;
    assert_drained(&mut observer, first_pid, second_pid).await;
}

async fn assert_drained(observer: &mut sqlx::PgConnection, first_pid: i32, second_pid: i32) {
    sqlx::query("SELECT pg_stat_clear_snapshot()")
        .execute(&mut *observer)
        .await
        .unwrap();
    let clean: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_stat_activity AS activity WHERE activity.pid=ANY($1) AND (activity.state='active' OR activity.xact_start IS NOT NULL))")
        .bind(vec![first_pid, second_pid]).fetch_one(&mut *observer).await.unwrap();
    assert!(clean, "completed contenders released their transactions");
}

fn expected_facts(
    boundary: Boundary,
    first: First,
    action: AppResult<bool>,
    cancel: AppResult<CancelResult>,
    before: (i64, i64, i64),
) -> (i64, i64, i64) {
    eprintln!("boundary={boundary:?} first={first:?} action={action:?} cancel={cancel:?}");
    assert!(matches!(cancel.unwrap(), CancelResult::Applied { .. }));
    match first {
        First::Action => assert!(action.unwrap()),
        First::Cancel if matches!(boundary, Boundary::Return) => assert!(!action.unwrap()),
        First::Cancel => assert!(matches!(
            action,
            Err(AppError::Conflict(_)) | Err(AppError::NotFound(_))
        )),
    }
    match (first, boundary) {
        (First::Action, Boundary::Local) => (1, 1, 1),
        (First::Action, Boundary::Reserve) => (1, 0, 0),
        _ => before,
    }
}

async fn assert_cancelled(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(request.scope().run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(state, "cancelled");
    let clean: bool = sqlx::query_scalar("SELECT completed_at IS NULL AND committed_output IS NULL AND successor_execution_id IS NULL FROM workflow_executions WHERE id=$1")
        .bind(request.scope().execution.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(clean);
    let attempts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM task_attempts WHERE status='processing'")
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(attempts, 0);
    let claimable: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM background_tasks WHERE status IN ('pending','processing')",
    )
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(claimable, 0);
    let intents: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_intents")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(intents, 1);
}

#[tokio::test]
async fn workflow_action_cancellation_company_before_run_all_callers() {
    for boundary in [
        Boundary::Local,
        Boundary::Reserve,
        Boundary::Enter,
        Boundary::Return,
        Boundary::Authority,
    ] {
        for first in [First::Action, First::Cancel] {
            Box::pin(race(boundary, first)).await;
        }
    }
}

#[tokio::test]
async fn workflow_action_cancellation_return_lost_owner_precedes_denied_authority() {
    let (f, request) = Box::pin(setup()).await;
    let writer = adapter(&f, 0);
    let entry = writer
        .enter_remote(reservation(&writer, &request).await)
        .await
        .unwrap();
    assert!(matches!(
        f.persistence()
            .cancel(command(&f, &request).await)
            .await
            .unwrap(),
        CancelResult::Applied { .. }
    ));
    sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=$2")
        .bind(request.scope().company.as_uuid())
        .bind(f.binding.target.actor.user_id())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert!(!writer.return_authorized(&entry).await.unwrap());
    assert_eq!(facts(&f).await, (1, 0, 0));
    assert_cancelled(&f, &request).await;
}
