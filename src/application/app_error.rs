use thiserror::Error;

/// `Clone` so a cache can hand one failed reading to every caller it serves, the way it hands
/// out a successful one. See `services::dashboard_snapshot`.
#[derive(Error, Debug, Clone)]
pub enum AppError {
    #[error("Execution stopped: {0}")]
    Execution(ExecutionFailure),
    #[error("Database error: {0}")]
    Database(String),
    /// PostgreSQL cancelled a statement at its `lock_timeout` or `statement_timeout`. Transient:
    /// the same work can succeed once the contention clears. Kept apart from [`Self::Timeout`],
    /// which is an execution deadline the task worker treats as final, so a busy database never
    /// ends a task for good. A caller that bounded its own transaction turns this into a
    /// `Timeout` that says what did not happen.
    #[error("Database timed out: {0}")]
    DatabaseTimeout(String),

    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Execution timed out: {0}")]
    Timeout(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type AppResult<T> = Result<T, AppError>;

/// A provider redelivering a key with different content is a conflict, not an internal fault:
/// the request is well-formed and the stored state is what refuses it.
impl From<crate::entities::message::ExternalMessageCollision> for AppError {
    fn from(error: crate::entities::message::ExternalMessageCollision) -> Self {
        AppError::Conflict(error.to_string())
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::Internal(err.to_string())
    }
}

/// Terminal execution categories never become an agent reply or an automatic provider retry.
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionFailure {
    #[error("structured final response is invalid")]
    InvalidOutput,
    #[error("execution budget exhausted")]
    Budget,
    #[error("invalid provider or checkpoint protocol")]
    Protocol,
    #[error("indeterminate effect requires reconciliation")]
    IndeterminateEffect,
    #[error("human checkpoint rejected or expired")]
    CheckpointDenied,
    #[error("execution ownership lost")]
    OwnershipLost,
}
