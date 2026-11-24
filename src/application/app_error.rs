use thiserror::Error;

/// `Clone` so a cache can hand one failed reading to every caller it serves, the way it hands
/// out a successful one. See `services::dashboard_snapshot`.
#[derive(Error, Debug, Clone)]
pub enum AppError {
    #[error("{0}")]
    WorkflowRole(crate::entities::department::RoleAdmissionRefusal),
    #[error("The triggering message exceeds this audience's access level")]
    KnowledgeTrigger {
        content: crate::entities::access_level::AccessLevel,
        ceiling: crate::entities::access_level::AccessLevel,
    },
    /// A skill or connection outside the reach of the agent, definition, desk or run that would
    /// use it (Plan07). A definite refusal: retrying the same command refuses again.
    #[error("{0}")]
    ResourceOutsideUnit(ResourceOutsideUnit),
    /// A unit on the run's chain has spent its cap for the period (Plan07 02B.2). A definite
    /// refusal for this admission: the next period, a raised cap or another desk may admit it.
    #[error("{0}")]
    BudgetExhausted(BudgetExhausted),
    /// The unit, or the unit of the desk, is archived (Plan08 02B): it admits no work and accepts
    /// no configuration. Definite: restoring the unit is the only way the same write succeeds.
    #[error("This department is archived; restore it to change it or start work on its desks")]
    UnitArchived,
    /// Archiving refused because runs on the subtree's desks are not finished (Plan08 02B.1).
    /// `cancellable` is false when cancel-and-archive cannot end them in one write.
    #[error(
        "{count} unfinished run(s) on this department's desks; cancel them or let them finish first"
    )]
    UnitArchiveOpenWork { count: i64, cancellable: bool },
    /// `message.ask` in a run that answers a channel call: the requester is an agent and cannot
    /// answer a question (agent-workflow-call AWC-P04-D1 r2 D8). Definite and terminal.
    #[error("A run answering a channel call cannot ask its requester a question")]
    InteractiveUnsupported,
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

    /// Only emitted after authenticated native invocation recovery found no call
    /// and compilation refused before any admission write or commit.
    #[error("Native MCP arguments failed pre-admission validation")]
    NativeMcpNotAdmitted,

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

/// Where a unit-owned resource was found out of reach (Plan07 G1 D8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitCheckStage {
    /// Publishing an agent or workflow that selects it.
    Publication,
    /// Saving a binding that maps a slot to it or resolves it.
    Binding,
    /// Admitting a run, including native sessions and native MCP calls.
    Admission,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceOutsideUnit {
    pub stage: UnitCheckStage,
    pub kind: crate::application::ports::unit_resources::UnitResourceKind,
    pub resource: uuid::Uuid,
    /// A name a person can act on: the tool alias, skill slug or connection, and where it was
    /// required (a step, for workflow publication).
    pub name: String,
}

impl ResourceOutsideUnit {
    pub fn code(&self) -> &'static str {
        match self.stage {
            UnitCheckStage::Publication => "publication.resource_outside_unit",
            UnitCheckStage::Binding => "binding.resource_outside_unit",
            UnitCheckStage::Admission => "admission.resource_outside_unit",
        }
    }
}

impl std::fmt::Display for ResourceOutsideUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::application::ports::unit_resources::UnitResourceKind;
        let kind = match self.kind {
            UnitResourceKind::Skill => "Skill",
            UnitResourceKind::McpConnection => "MCP connection",
            UnitResourceKind::HttpConnection => "HTTP connection",
            UnitResourceKind::ModelConnection => "Model connection",
        };
        write!(
            f,
            "{kind} {} belongs to a department outside the reach of what uses it; move it up or \
             choose another",
            self.name
        )
    }
}

/// The first unit from a run's write unit upward whose spend reached its cap (Plan07 02B.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetExhausted {
    pub unit: uuid::Uuid,
    /// The department's name, for the refusal message.
    pub name: String,
    /// `day` or `month` (UTC, decision i).
    pub period: &'static str,
}

impl BudgetExhausted {
    pub const CODE: &'static str = "budget.unit_exhausted";

    pub fn code(&self) -> &'static str {
        Self::CODE
    }
}

impl AppError {
    pub const UNIT_ARCHIVED: &'static str = "unit.archived";
    pub const UNIT_ARCHIVE_OPEN_WORK: &'static str = "unit.archive_open_work";
}

impl std::fmt::Display for BudgetExhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let period = if self.period == "day" {
            "today's"
        } else {
            "this month's"
        };
        write!(
            f,
            "The department {} has spent {period} budget; new runs start again in the next period \
             or once its cap is raised",
            self.name
        )
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
