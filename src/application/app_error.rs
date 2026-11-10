use thiserror::Error;

/// `Clone` so a cache can hand one failed reading to every caller it serves, the way it hands
/// out a successful one. See `services::dashboard_snapshot`.
#[derive(Error, Debug, Clone)]
pub enum AppError {
    #[error("The triggering message exceeds this audience's access level")]
    KnowledgeTrigger {
        content: crate::entities::access_level::AccessLevel,
        ceiling: crate::entities::access_level::AccessLevel,
    },
    #[error("The destination cannot receive this run's content")]
    KnowledgeRelease {
        content: crate::entities::access_level::AccessLevel,
        destination: crate::entities::access_level::AccessLevel,
    },
    #[error("Execution stopped: {0}")]
    Execution(ExecutionFailure),
    #[error("Database error: {0}")]
    Database(String),
    /// PostgreSQL cancelled a statement at its `lock_timeout` or `statement_timeout`. Transient:
    /// the same work can succeed once the contention clears. Kept apart from [`Self::Timeout`],
    /// which is an execution deadline, so a busy database is never mistaken for work that ran out of
    /// time. A caller that bounded its own transaction turns this into a
    /// `Timeout` that says what did not happen. A workflow persistence bound that elapses
    /// before COMMIT was sent is this class too: definitely not applied, safe to retry.
    #[error("Database timed out: {0}")]
    DatabaseTimeout(String),
    /// A workflow database guard refused a write: Rust and SQL disagree about what may happen.
    /// Retrying the same write trips the same guard, so the workflow worker treats a confirmed
    /// violation as terminal for its run instead of retrying it until the deadline.
    #[error("Workflow guard {0}")]
    WorkflowGuard(WorkflowGuardViolation),

    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("MCP service rejected the configured credentials")]
    McpAuthentication,

    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Not found: {0}")]
    NotFound(String),

    /// The caller can see the resource -- they belong to its company -- but may not do this to
    /// it. Something they cannot see at all is [`Self::NotFound`], so an id probe learns nothing.
    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    /// The state this work was planned against changed before it could apply, so nothing was
    /// applied. Transient, unlike [`Self::Conflict`]: planning again against the current state can
    /// succeed, and a caller that can replan -- an inbound transport's retry -- should.
    #[error("Superseded: {0}")]
    Superseded(String),

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
    #[error("MCP tool failed under the workflow completion policy")]
    McpToolFailed,
    #[error("execution budget exhausted")]
    Budget,
    #[error("model repeated tool calls whose results no longer change")]
    RepeatedToolCall,
    #[error("invalid provider or checkpoint protocol")]
    Protocol,
    #[error("indeterminate effect requires reconciliation")]
    IndeterminateEffect,
    #[error("human checkpoint rejected or expired")]
    CheckpointDenied,
    #[error("execution ownership lost")]
    OwnershipLost,
}

/// Prefix of the constraint name every workflow guard `RAISE` in the init schema reports
/// (`USING CONSTRAINT = 'workflow_guard:<name>'`). Table constraints never carry it.
pub const WORKFLOW_GUARD_PREFIX: &str = "workflow_guard:";

/// A validated PostgreSQL workflow-guard identifier, distinct from its free-text diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowGuardName(String);

/// Which guard refused, and the database diagnostic. A validated name can label a metric and
/// audit event without confusing the identifier with its free-text message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowGuardViolation {
    guard: WorkflowGuardName,
    message: String,
}

impl WorkflowGuardViolation {
    /// `None` unless `constraint` is a prefixed guard name of 1-63 `[a-z0-9_]` bytes.
    pub fn from_constraint(constraint: &str, message: impl Into<String>) -> Option<Self> {
        let guard = constraint.strip_prefix(WORKFLOW_GUARD_PREFIX)?;
        let valid = (1..=63).contains(&guard.len())
            && guard
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
        valid.then(|| Self {
            guard: WorkflowGuardName(guard.to_owned()),
            message: message.into(),
        })
    }

    pub fn guard(&self) -> &str {
        &self.guard.0
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for WorkflowGuardViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.guard(), self.message)
    }
}

pub const KNOWLEDGE_TRIGGER_ABOVE_CEILING: &str = "knowledge.trigger_above_ceiling";
pub use crate::domain::workflow::KNOWLEDGE_AUDIENCE_BELOW_CONTENT;

impl AppError {
    pub fn knowledge_code(&self) -> Option<&'static str> {
        match self {
            Self::KnowledgeTrigger { .. } => Some(KNOWLEDGE_TRIGGER_ABOVE_CEILING),
            Self::KnowledgeRelease { .. } => Some(KNOWLEDGE_AUDIENCE_BELOW_CONTENT),
            _ => None,
        }
    }
}
