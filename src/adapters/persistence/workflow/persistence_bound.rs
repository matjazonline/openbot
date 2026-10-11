//! One client bound per fenced workflow persistence operation.
//!
//! The attempt covers a bounded pool acquire, the statements and COMMIT. Only a COMMIT that
//! may have been sent leaves the outcome unknown (sqlx buffers it on first poll and the
//! released connection flushes it). Then a read-only resolution takes the original's own
//! leading locks, which it holds until it commits or aborts, reads that operation's
//! immutable evidence and rolls back. Resolution never writes, so it cannot create a second
//! ambiguous outcome. Caller-side cancellation drops the future and resolves nothing; lease
//! expiry retirement is the backstop for that, and the event records it as `cancelled`.
use super::*;
use crate::application::workflow::lease::LeasePolicy;
use sqlx::{Acquire, PgPool, Postgres, Transaction, pool::PoolConnection};
use std::sync::{
    Mutex,
    atomic::{AtomicU8, Ordering},
};
use std::time::Duration;
use tokio::time::Instant;

const ACQUIRING: u8 = 0;
const RUNNING: u8 = 1;
/// COMMIT may have been sent: only resolution can tell.
const COMMITTING: u8 = 2;
/// COMMIT was answered with a server error: definitely not applied.
const REFUSED: u8 = 3;

pub(super) enum Attempt<T> {
    Done(T),
    Failed(AppError),
    Ambiguous,
}

/// What the original transaction left behind, read under its own locks.
pub(super) enum Evidence<T> {
    Applied(T),
    NotApplied,
}

/// How a bounded body ends its transaction; `WriteBound::end` sends it.
pub(super) enum End<T> {
    /// The only ending that can leave the outcome unknown.
    Commit(T),
    /// A refusal or a read: nothing written is kept.
    Rollback(T),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Cancelled,
    Completed,
    Refused,
    NotApplied,
    ResolvedApplied,
    /// A reservation proven committed under the original's locks and handed back as ours.
    ResolvedAdopted,
    ResolvedNotApplied,
    Unknown,
}
impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::Completed => "completed",
            Self::Refused => "refused",
            Self::NotApplied => "not_applied",
            Self::ResolvedApplied => "resolved_applied",
            Self::ResolvedAdopted => "resolved_adopted",
            Self::ResolvedNotApplied => "resolved_not_applied",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Default)]
struct Marks {
    acquired: Option<Instant>,
    /// Test-only: when the operation's leading locks were last reported held.
    #[cfg(test)]
    locked: Option<Instant>,
    committing: Option<Instant>,
    attempted: Option<Instant>,
    outcome: Option<Outcome>,
    /// Why an `Unknown` stayed unknown: `unresolved` (the resolution elapsed or failed) or
    /// `lapsed` (our commit is proven, but its fence no longer holds).
    cause: Option<&'static str>,
}

pub(super) struct WriteBound {
    op: &'static str,
    policy: LeasePolicy,
    /// A for writes; the whole deadline for read-only work, which nothing can leave ambiguous.
    attempt_until: Instant,
    /// The operation's overall client bound T: attempt plus resolution.
    deadline: Instant,
    started: Instant,
    phase: AtomicU8,
    marks: Mutex<Marks>,
}

impl WriteBound {
    pub(super) fn start(op: &'static str, policy: LeasePolicy) -> Self {
        let started = Instant::now();
        Self::with_bounds(
            op,
            policy,
            started,
            started + policy.attempt_bound(),
            started + policy.persistence_deadline(),
        )
    }
    /// A further attempt inside an earlier operation's deadline (release's single retry):
    /// it gets at most A and never outlives that operation's T, so one call stays inside the
    /// supervision guard G > T that callers wrap it in.
    pub(super) fn within(op: &'static str, policy: LeasePolicy, deadline: Instant) -> Self {
        let started = Instant::now();
        Self::with_bounds(
            op,
            policy,
            started,
            (started + policy.attempt_bound()).min(deadline),
            deadline,
        )
    }
    /// Read-only work ends in ROLLBACK: no synchronous commit flush and no ambiguous outcome.
    pub(super) fn read_only(op: &'static str, policy: LeasePolicy) -> Self {
        let started = Instant::now();
        let deadline = started + policy.persistence_deadline();
        Self::with_bounds(op, policy, started, deadline, deadline)
    }
    fn with_bounds(
        op: &'static str,
        policy: LeasePolicy,
        started: Instant,
        attempt_until: Instant,
        deadline: Instant,
    ) -> Self {
        Self {
            op,
            policy,
            attempt_until,
            deadline,
            started,
            phase: AtomicU8::new(ACQUIRING),
            marks: Mutex::new(Marks::default()),
        }
    }
    pub(super) fn started(&self) -> Instant {
        self.started
    }
    pub(super) fn deadline(&self) -> Instant {
        self.deadline
    }
    fn resolving(&self) -> bool {
        self.phase.load(Ordering::SeqCst) == COMMITTING
    }
    fn until(&self) -> Instant {
        if self.resolving() {
            self.deadline
        } else {
            self.attempt_until
        }
    }
    /// What remains of the current phase: the attempt, or during resolution the whole
    /// deadline. A statement starting late can then never outlive the client bound.
    pub(super) fn server_timeout(&self) -> Duration {
        self.until().saturating_duration_since(Instant::now())
    }
    fn mark(&self, update: impl FnOnce(&mut Marks)) {
        if let Ok(mut marks) = self.marks.lock() {
            update(&mut marks);
        }
    }

    /// Pre-effect work fails fast: a connection within the acquire bound a, so a sick pool is
    /// a definite, retry-safe NotApplied long before the attempt bound.
    pub(super) async fn acquire(&self, pool: &PgPool) -> AppResult<PoolConnection<Postgres>> {
        self.acquire_within(pool, self.policy.acquire_bound().min(self.server_timeout()))
            .await
    }

    /// Post-effect truth (a remote receipt after the provider applied) waits for a connection
    /// for what remains of the attempt instead: failing fast there would turn a pool hiccup
    /// into marker reconciliation rather than save any retry.
    pub(super) async fn acquire_for_attempt(
        &self,
        pool: &PgPool,
    ) -> AppResult<PoolConnection<Postgres>> {
        self.acquire_within(pool, self.server_timeout()).await
    }

    async fn acquire_within(
        &self,
        pool: &PgPool,
        bound: Duration,
    ) -> AppResult<PoolConnection<Postgres>> {
        let conn = tokio::time::timeout(bound, pool.acquire())
            .await
            .map_err(|_| self.not_applied())??;
        self.mark(|marks| {
            marks.acquired.get_or_insert_with(Instant::now);
        });
        Ok(conn)
    }

    pub(super) async fn begin<'c>(
        &self,
        conn: &'c mut PoolConnection<Postgres>,
    ) -> AppResult<Transaction<'c, Postgres>> {
        let _ = self
            .phase
            .compare_exchange(ACQUIRING, RUNNING, Ordering::SeqCst, Ordering::SeqCst);
        let mut tx = conn.begin().await?;
        set_server_timeouts(&mut tx, self.server_timeout()).await?;
        Ok(tx)
    }

    pub(super) async fn commit(&self, tx: Transaction<'_, Postgres>) -> AppResult<()> {
        self.phase.store(COMMITTING, Ordering::SeqCst);
        self.mark(|marks| marks.committing = Some(Instant::now()));
        tx.commit().await.map_err(|error| {
            if error.as_database_error().is_some() {
                self.phase.store(REFUSED, Ordering::SeqCst);
            }
            AppError::from(error)
        })
    }

    /// Ends the transaction on every path before the caller sees the result. A dropped
    /// transaction only queues its ROLLBACK, which sqlx flushes later from the pooled
    /// connection's return task, and until then the backend keeps every row lock it took.
    /// An error keeps its own value: its ROLLBACK is best effort, because one that fails
    /// means the connection is gone, which releases the locks as well.
    pub(super) async fn end<T>(
        &self,
        tx: Transaction<'_, Postgres>,
        body: AppResult<End<T>>,
    ) -> AppResult<T> {
        match body {
            Ok(End::Commit(value)) => {
                self.commit(tx).await?;
                Ok(value)
            }
            Ok(End::Rollback(value)) => {
                tx.rollback().await?;
                Ok(value)
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }

    /// Read-only work and resolutions never commit: ROLLBACK on every path.
    pub(super) async fn roll_back<T>(
        &self,
        tx: Transaction<'_, Postgres>,
        body: AppResult<T>,
    ) -> AppResult<T> {
        self.end(tx, body.map(End::Rollback)).await
    }

    pub(super) async fn attempt<T>(&self, body: impl Future<Output = AppResult<T>>) -> Attempt<T> {
        #[cfg(test)]
        let locked = std::sync::Arc::new(std::sync::Mutex::new(None));
        #[cfg(test)]
        let body = recorder::LOCKED.scope(locked.clone(), body);
        let result = tokio::time::timeout_at(self.attempt_until, body).await;
        self.mark(|marks| marks.attempted = Some(Instant::now()));
        #[cfg(test)]
        if let Some(at) = locked.lock().ok().and_then(|at| *at) {
            self.mark(|marks| marks.locked = Some(at));
        }
        if self.resolving() && !matches!(result, Ok(Ok(_))) {
            return Attempt::Ambiguous;
        }
        match result {
            Ok(Ok(value)) => {
                self.settle(Outcome::Completed);
                Attempt::Done(value)
            }
            Ok(Err(error)) => {
                self.settle(if matches!(error, AppError::DatabaseTimeout(_)) {
                    Outcome::NotApplied
                } else {
                    Outcome::Refused
                });
                Attempt::Failed(error)
            }
            Err(_) => {
                self.settle(Outcome::NotApplied);
                Attempt::Failed(self.not_applied())
            }
        }
    }

    /// Read-only work never commits, so every failure is definitely not applied.
    pub(super) async fn read<T>(&self, body: impl Future<Output = AppResult<T>>) -> AppResult<T> {
        match self.attempt(body).await {
            Attempt::Done(value) => Ok(value),
            Attempt::Failed(error) => Err(error),
            Attempt::Ambiguous => Err(self.not_applied()),
        }
    }

    pub(super) async fn resolve<T>(
        &self,
        evidence: impl Future<Output = AppResult<Evidence<T>>>,
    ) -> AppResult<T> {
        self.resolve_retryable(evidence)
            .await?
            .ok_or_else(|| self.not_applied())
    }

    /// As `resolve`, but a proven NotApplied is `Ok(None)`, for a caller that retries it.
    pub(super) async fn resolve_retryable<T>(
        &self,
        evidence: impl Future<Output = AppResult<Evidence<T>>>,
    ) -> AppResult<Option<T>> {
        match tokio::time::timeout_at(self.deadline, evidence).await {
            Ok(Ok(Evidence::Applied(value))) => {
                self.settle(Outcome::ResolvedApplied);
                Ok(Some(value))
            }
            Ok(Ok(Evidence::NotApplied)) => {
                self.settle(Outcome::ResolvedNotApplied);
                Ok(None)
            }
            _ => Err(self.unresolved()),
        }
    }

    /// An ambiguous COMMIT this operation does not, or could not, resolve.
    pub(super) fn unresolved(&self) -> AppError {
        self.unknown("unresolved")
    }

    /// A COMMIT proven applied whose result the caller still cannot use, because the fence
    /// it was made under no longer holds. To the caller it stays unknown.
    pub(super) fn lapsed(&self) -> AppError {
        self.unknown("lapsed")
    }

    /// A COMMIT proven applied under the original's locks whose result is handed back.
    pub(super) fn adopted(&self) {
        self.settle(Outcome::ResolvedAdopted);
    }

    fn unknown(&self, cause: &'static str) -> AppError {
        self.mark(|marks| {
            marks.outcome = Some(Outcome::Unknown);
            marks.cause = Some(cause);
        });
        AppError::Conflict(format!("Workflow {} commit outcome unknown", self.op))
    }

    fn settle(&self, outcome: Outcome) {
        self.mark(|marks| marks.outcome = Some(outcome));
    }
    /// The definite NotApplied error of this operation.
    pub(super) fn not_applied(&self) -> AppError {
        AppError::DatabaseTimeout(format!(
            "Workflow {} not applied: persistence bound elapsed before commit",
            self.op
        ))
    }
}

impl Drop for WriteBound {
    fn drop(&mut self) {
        let now = Instant::now();
        let marks = self.marks.get_mut().map(std::mem::take).unwrap_or_default();
        let outcome = marks.outcome.unwrap_or(Outcome::Cancelled);
        let since = |from: Option<Instant>, to: Option<Instant>| {
            from.map(|from| {
                to.unwrap_or(now)
                    .saturating_duration_since(from)
                    .as_millis()
            })
        };
        let acquire_ms = since(Some(self.started), marks.acquired);
        let body_ms = since(marks.acquired, marks.committing.or(marks.attempted));
        let commit_ms = since(marks.committing, marks.attempted);
        // Test-only body split at the leading locks: waits on competing transactions, then
        // the statements this operation owns.
        #[cfg(test)]
        let lock_ms = marks
            .locked
            .and_then(|locked| since(marks.acquired, Some(locked)));
        #[cfg(test)]
        let work_ms = since(
            marks.locked.or(marks.acquired),
            marks.committing.or(marks.attempted),
        );
        let resolution_ms = matches!(
            outcome,
            Outcome::ResolvedApplied
                | Outcome::ResolvedAdopted
                | Outcome::ResolvedNotApplied
                | Outcome::Unknown
        )
        .then(|| since(marks.attempted, None))
        .flatten();
        let total_ms = now.saturating_duration_since(self.started).as_millis();
        let attempt_bound_ms = self
            .attempt_until
            .saturating_duration_since(self.started)
            .as_millis();
        if matches!(outcome, Outcome::Unknown | Outcome::NotApplied) {
            tracing::warn!(
                op = self.op,
                outcome = outcome.as_str(),
                cause = marks.cause,
                ?acquire_ms,
                ?body_ms,
                ?commit_ms,
                ?resolution_ms,
                total_ms,
                attempt_bound_ms,
                "Workflow persistence bound"
            );
        } else {
            tracing::debug!(
                op = self.op,
                outcome = outcome.as_str(),
                cause = marks.cause,
                ?acquire_ms,
                ?body_ms,
                ?commit_ms,
                ?resolution_ms,
                total_ms,
                attempt_bound_ms,
                "Workflow persistence bound"
            );
        }
        #[cfg(test)]
        recorder::observe(recorder::Observed {
            op: self.op,
            outcome: outcome.as_str(),
            cause: marks.cause,
        });
        #[cfg(test)]
        recorder::record(serde_json::json!({
            "op": self.op, "outcome": outcome.as_str(), "cause": marks.cause, "acquire_ms": acquire_ms,
            "body_ms": body_ms, "lock_ms": lock_ms, "work_ms": work_ms, "commit_ms": commit_ms, "resolution_ms": resolution_ms,
            "total_ms": total_ms, "attempt_bound_ms": attempt_bound_ms,
            "injected": recorder::injected(),
        }));
    }
}

pub(super) async fn set_server_timeouts(db: &mut PgConnection, timeout: Duration) -> AppResult<()> {
    // Zero would disable both bounds; never ask the server for less than a millisecond.
    let value = format!("{}ms", timeout.as_millis().max(1));
    sqlx::query(
        "SELECT set_config('lock_timeout',$1,true), set_config('statement_timeout',$1,true)",
    )
    .bind(value)
    .execute(db)
    .await?;
    Ok(())
}

/// Reports that the calling operation now holds its leading locks (run, execution, job, and
/// for renewal the capacity row). Test-only measurement: it splits the body phase into lock
/// waits on competing transactions and the code-owned statements after them.
pub(super) fn locks_held() {
    #[cfg(test)]
    recorder::locks_held();
}

/// An in-transaction refusal (rolled back): the run or the lease window closed meanwhile.
pub(super) fn refused(op: &str) -> AppError {
    AppError::Conflict(format!("Workflow {op} refused: run or lease window closed"))
}

/// Test-only phase recorder read by `scripts/lease-budget.sh`. Each event is appended as one
/// JSON line to `$WORKFLOW_PERSISTENCE_RECORD` when that is set. Tests that deliberately
/// stall an operation set `INJECTED` on their thread, so the budget gate ignores them.
#[cfg(test)]
pub(crate) mod recorder {
    use std::{
        cell::{Cell, RefCell},
        io::Write,
        sync::Mutex,
    };

    thread_local! {
        static INJECTED: Cell<bool> = const { Cell::new(false) };
        static OBSERVED: RefCell<Vec<Observed>> = const { RefCell::new(Vec::new()) };
    }

    /// One finished operation, as an injection test sees it.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct Observed {
        pub(crate) op: &'static str,
        pub(crate) outcome: &'static str,
        pub(crate) cause: Option<&'static str>,
    }
    static FILE: Mutex<()> = Mutex::new(());

    pub(super) fn injected() -> bool {
        INJECTED.get()
    }

    tokio::task_local! {
        /// Scoped to one attempt's body, so concurrent operations never share a mark.
        pub(super) static LOCKED: std::sync::Arc<Mutex<Option<tokio::time::Instant>>>;
    }
    pub(super) fn locks_held() {
        let _ = LOCKED.try_with(|locked| {
            if let Ok(mut locked) = locked.lock() {
                *locked = Some(tokio::time::Instant::now());
            }
        });
    }

    /// Marks every operation this thread finishes while it lives as deliberately stalled.
    /// Injection tests run on a current-thread runtime, so their operations drop here.
    pub(crate) struct Injected(());
    impl Injected {
        pub(crate) fn mark() -> Self {
            OBSERVED.with_borrow_mut(Vec::clear);
            INJECTED.set(true);
            Self(())
        }
        /// Every operation this thread finished since `mark`, in order.
        pub(crate) fn outcomes(&self) -> Vec<Observed> {
            OBSERVED.with_borrow(Clone::clone)
        }
    }
    impl Drop for Injected {
        fn drop(&mut self) {
            INJECTED.set(false);
        }
    }
    /// Kept for injection tests whatever `$WORKFLOW_PERSISTENCE_RECORD` says.
    pub(super) fn observe(observed: Observed) {
        if INJECTED.get() {
            OBSERVED.with_borrow_mut(|list| list.push(observed));
        }
    }
    pub(super) fn record(event: serde_json::Value) {
        let Ok(path) = std::env::var("WORKFLOW_PERSISTENCE_RECORD") else {
            return;
        };
        let _guard = FILE.lock();
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{event}");
        }
    }
}
