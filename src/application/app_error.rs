use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Execution stopped: {0}")]
    Execution(ExecutionFailure),
    #[error("Database error: {0}")]
    Database(String),

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
