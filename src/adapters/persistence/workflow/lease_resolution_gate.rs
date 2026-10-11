//! Slow-commit injection for the persistence-bound resolution tests.
//!
//! A BEFORE row trigger clears `statement_timeout` and `lock_timeout` for the rest of the
//! matched transaction, so its COMMIT starts uncancellable like a WAL flush stall. A
//! deferred constraint trigger (named to fire first) then blocks inside COMMIT on an
//! advisory lock this module holds, with every row lock of the stalled transaction still
//! held. Releasing the gate lets that COMMIT finish, or abort when an abort row is present.
//! Nothing here sleeps: every step waits on an observed backend state, under a hang guard.
use super::*;
use sqlx::{ConnectOptions, PgConnection};
use std::time::Duration;
use tokio::time::Instant;

const GATE_KEY: i64 = 0x1ea5_e9a7;
/// Only a hang guard: observations are the signal, a loaded host is measured, not failed.
pub(super) const HANG_GUARD: Duration = Duration::from_secs(10);
/// Polling granularity for backend observation, not a timing assumption.
const POLL: Duration = Duration::from_millis(5);

/// A 6 s lease: A = 2 s, R = 1 s, T = 3 s, a = 1 s, G = 3.5 s.
pub(super) fn gated_policy() -> LeasePolicy {
    LeasePolicy::new(Duration::from_secs(6)).unwrap()
}

pub(super) async fn connect(f: &AdmissionFixture) -> (PgConnection, i32) {
    let mut conn = f
        .persistence()
        .pool()
        .connect_options()
        .connect()
        .await
        .unwrap();
    let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    (conn, pid)
}

/// Where the stall happens: in the deferred COMMIT, or in a row trigger of a later statement
/// (whose own statement_timeout the arming trigger already cleared).
pub(super) enum Stall<'a> {
    Commit,
    Statement {
        table: &'a str,
        events: &'a str,
        when: &'a str,
    },
}

pub(super) enum Abort {
    Nothing,
    /// Every transaction passing the gate, retries included.
    Every,
    /// Only this backend's transaction: a competitor's later COMMIT still applies.
    Backend(i32),
}

pub(super) struct Gate {
    conn: PgConnection,
    pub(super) pid: i32,
}

impl Gate {
    /// Installs the gate on `table` for rows matching `when`, then holds it.
    pub(super) async fn arm(
        f: &AdmissionFixture,
        table: &str,
        events: &str,
        when: &str,
        stall: Stall<'_>,
    ) -> Self {
        let blocking = match stall {
            Stall::Commit => format!(
                "CREATE CONSTRAINT TRIGGER aa_lease_gate AFTER {events} ON {table} DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN ({when}) EXECUTE FUNCTION lease_gate();"
            ),
            Stall::Statement {
                table,
                events,
                when,
            } => format!(
                "CREATE TRIGGER aa_lease_stall AFTER {events} ON {table} FOR EACH ROW WHEN ({when}) EXECUTE FUNCTION lease_gate();"
            ),
        };
        sqlx::raw_sql(&format!(
            "CREATE TABLE lease_gate_abort(id int PRIMARY KEY); \
             CREATE FUNCTION lease_gate_uncancellable() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM set_config('statement_timeout','0',true), set_config('lock_timeout','0',true); RETURN NEW; END $$; \
             CREATE FUNCTION lease_gate() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock({GATE_KEY}); IF EXISTS (SELECT 1 FROM lease_gate_abort WHERE id IN (0, pg_backend_pid())) THEN RAISE EXCEPTION 'injected commit abort'; END IF; RETURN NULL; END $$; \
             CREATE TRIGGER lease_gate_arm BEFORE {events} ON {table} FOR EACH ROW WHEN ({when}) EXECUTE FUNCTION lease_gate_uncancellable(); \
             {blocking}"
        ))
        .execute(f.persistence().pool())
        .await
        .unwrap();
        let (mut conn, pid) = connect(f).await;
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(GATE_KEY)
            .execute(&mut conn)
            .await
            .unwrap();
        Self { conn, pid }
    }

    /// Which stalled transactions abort instead of applying once released.
    pub(super) async fn abort_on_release(&mut self, abort: Abort) {
        sqlx::query("DELETE FROM lease_gate_abort")
            .execute(&mut self.conn)
            .await
            .unwrap();
        let id = match abort {
            Abort::Nothing => return,
            Abort::Every => 0,
            Abort::Backend(pid) => pid,
        };
        sqlx::query("INSERT INTO lease_gate_abort VALUES ($1)")
            .bind(id)
            .execute(&mut self.conn)
            .await
            .unwrap();
    }

    pub(super) async fn release(&mut self) {
        let released: bool = sqlx::query_scalar("SELECT pg_advisory_unlock($1)")
            .bind(GATE_KEY)
            .fetch_one(&mut self.conn)
            .await
            .unwrap();
        assert!(released, "the test held the gate");
    }

    /// Takes the gate back. Lock waiters are granted in order, so this returns only after
    /// every transaction already stalled on the gate has committed or aborted.
    pub(super) async fn retake(&mut self) {
        tokio::time::timeout(
            HANG_GUARD,
            sqlx::query("SELECT pg_advisory_lock($1)")
                .bind(GATE_KEY)
                .execute(&mut self.conn),
        )
        .await
        .expect("stalled transactions end once the gate is released")
        .unwrap();
    }

    /// Releases the gate and waits until everything stalled on it has finished.
    pub(super) async fn drain(&mut self) {
        self.release().await;
        self.retake().await;
        self.release().await;
    }

    /// The backend stalled on this gate (its COMMIT or statement).
    pub(super) async fn stalled(
        &self,
        observer: &mut PgConnection,
        running: &[&dyn Fn() -> bool],
    ) -> i32 {
        lock_waiters(
            observer,
            self.pid,
            true,
            None,
            1,
            "gated transaction",
            running,
        )
        .await[0]
    }
}

/// At least `n` backends in a heavyweight lock wait behind `blocker`, directly or queued
/// behind another such waiter, in the order their statements started.
pub(super) async fn waiters(
    observer: &mut PgConnection,
    blocker: i32,
    n: usize,
    what: &str,
    running: &[&dyn Fn() -> bool],
) -> Vec<i32> {
    lock_waiters(observer, blocker, false, None, n, what, running).await
}

/// The resolution of the operation stalled in `stalled`: a lock waiter behind it whose
/// transaction began at least A/2 after the stalled one, i.e. after the attempt elapsed.
/// A competitor that began with the stalled operation (and whose pooled connection the
/// resolution may reuse once it gave up) cannot match.
pub(super) async fn resolution_behind(
    observer: &mut PgConnection,
    stalled: i32,
    policy: LeasePolicy,
    running: &[&dyn Fn() -> bool],
) -> i32 {
    let since = Some((stalled, policy.attempt_bound() / 2));
    lock_waiters(observer, stalled, false, since, 1, "resolution", running).await[0]
}

/// Panics when an operation in `running` ends first: it must not complete before the lock
/// competition it is meant to lose or wait out.
async fn lock_waiters(
    observer: &mut PgConnection,
    blocker: i32,
    advisory: bool,
    since: Option<(i32, Duration)>,
    n: usize,
    what: &str,
    running: &[&dyn Fn() -> bool],
) -> Vec<i32> {
    let started = Instant::now();
    loop {
        sqlx::query("SELECT pg_stat_clear_snapshot()")
            .execute(&mut *observer)
            .await
            .unwrap();
        // Transitive: a later row-lock waiter queues on the tuple lock the first waiter
        // holds, so PostgreSQL names that waiter, not the stalled transaction, as blocker.
        let pids: Vec<i32> = sqlx::query_scalar("WITH RECURSIVE behind(pid) AS (SELECT $1::int UNION SELECT waiter.pid FROM pg_stat_activity AS waiter, behind WHERE waiter.datname=current_database() AND waiter.wait_event_type='Lock' AND behind.pid=ANY(pg_blocking_pids(waiter.pid))) SELECT activity.pid FROM pg_stat_activity AS activity WHERE activity.pid IN (SELECT pid FROM behind) AND activity.pid<>$1 AND (activity.wait_event='advisory')=$2 AND ($3::int IS NULL OR activity.xact_start > (SELECT stalled.xact_start FROM pg_stat_activity AS stalled WHERE stalled.pid=$3) + make_interval(secs => $4)) ORDER BY activity.query_start")
            .bind(blocker).bind(advisory).bind(since.map(|(pid, _)| pid)).bind(since.map_or(0.0, |(_, after)| after.as_secs_f64()))
            .fetch_all(&mut *observer).await.unwrap();
        if pids.len() >= n {
            eprintln!(
                "observed {what} pids={pids:?} blocker={blocker} elapsed_ms={}",
                started.elapsed().as_millis()
            );
            return pids;
        }
        assert!(
            running.iter().all(|running| running()),
            "an operation ended before {what} was observed behind {blocker}"
        );
        assert!(
            started.elapsed() < HANG_GUARD,
            "{what} behind {blocker} not observed within hang guard {HANG_GUARD:?}"
        );
        tokio::time::sleep(POLL).await;
    }
}

/// Waits until the transaction of `stalled` is older than A by the server clock, then lets
/// this runtime poll once more. The operation's client bound started before its transaction
/// did, so its attempt has elapsed and (on this current-thread runtime) it has moved on to
/// its resolution.
pub(super) async fn attempt_elapsed(
    observer: &mut PgConnection,
    stalled: i32,
    policy: LeasePolicy,
    running: &[&dyn Fn() -> bool],
) {
    let started = Instant::now();
    loop {
        sqlx::query("SELECT pg_stat_clear_snapshot()")
            .execute(&mut *observer)
            .await
            .unwrap();
        let elapsed: bool = sqlx::query_scalar("SELECT clock_timestamp()-xact_start > make_interval(secs => $2) FROM pg_stat_activity WHERE pid=$1")
            .bind(stalled).bind(policy.attempt_bound().as_secs_f64()).fetch_one(&mut *observer).await.unwrap();
        if elapsed {
            tokio::time::sleep(POLL).await;
            return;
        }
        assert!(
            running.iter().all(|running| running()),
            "an operation ended before its attempt bound elapsed"
        );
        assert!(started.elapsed() < HANG_GUARD);
        tokio::time::sleep(POLL).await;
    }
}

/// Waits until nothing waits behind `blocker` any more (a server-side bound ended the wait).
pub(super) async fn no_waiter_behind(observer: &mut PgConnection, blocker: i32, within: Duration) {
    let started = Instant::now();
    loop {
        sqlx::query("SELECT pg_stat_clear_snapshot()")
            .execute(&mut *observer)
            .await
            .unwrap();
        let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)))")
            .bind(blocker).fetch_one(&mut *observer).await.unwrap();
        if !waiting {
            return;
        }
        assert!(
            started.elapsed() < within,
            "a wait behind {blocker} outlived its server bound by more than {within:?}"
        );
        tokio::time::sleep(POLL).await;
    }
}

#[derive(Debug, sqlx::FromRow)]
pub(super) struct AttemptRow {
    pub(super) attempt_number: i32,
    pub(super) status: String,
    pub(super) execution_generation: Uuid,
    pub(super) worker_id: Uuid,
    pub(super) stop_reason: Option<String>,
    pub(super) workflow_retirement: Option<String>,
}

pub(super) async fn attempts(f: &AdmissionFixture, scope: ActivationRequest) -> Vec<AttemptRow> {
    sqlx::query_as("SELECT attempt_number,status,execution_generation,worker_id,stop_reason,workflow_retirement FROM task_attempts WHERE task_id=$1 ORDER BY attempt_number")
        .bind(scope.job.0).fetch_all(f.persistence().pool()).await.unwrap()
}

#[derive(Debug, sqlx::FromRow)]
pub(super) struct JobRow {
    pub(super) status: String,
    pub(super) retry_count: i32,
    pub(super) worker_id: Option<Uuid>,
    pub(super) execution_generation: Option<Uuid>,
}

pub(super) async fn job(f: &AdmissionFixture, scope: ActivationRequest) -> JobRow {
    sqlx::query_as(
        "SELECT status,retry_count,worker_id,execution_generation FROM background_tasks WHERE id=$1",
    )
    .bind(scope.job.0)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap()
}

pub(super) fn not_applied(result: &AppError, op: &str) -> bool {
    matches!(result, AppError::DatabaseTimeout(message)
        if message == &format!("Workflow {op} not applied: persistence bound elapsed before commit"))
}

pub(super) fn unknown(result: &AppError, op: &str) -> bool {
    matches!(result, AppError::Conflict(message)
        if message == &format!("Workflow {op} commit outcome unknown"))
}
