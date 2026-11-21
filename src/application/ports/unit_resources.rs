//! Skills and connections belong to a unit (Plan07). Moving one changes only its unit: published
//! agent revisions keep what they captured, and the current uses are reported so their managers
//! can republish or move desks.
use crate::entities::department::DepartmentId;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitResourceKind {
    Skill,
    McpConnection,
    HttpConnection,
    /// A company model connection (Plan07 02A), one per provider.
    ModelConnection,
}

impl UnitResourceKind {
    /// The wire name refusals carry (Plan07 G1 D8).
    pub fn code(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::McpConnection => "mcp",
            Self::HttpConnection => "http",
            Self::ModelConnection => "model",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceUseKind {
    /// An agent whose active revision uses the resource.
    Agent,
    /// An active workflow binding whose current configuration maps a slot to the connection.
    Binding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceUse {
    pub kind: ResourceUseKind,
    pub id: Uuid,
    pub name: String,
}

/// At most this many uses are named, and only those the mover manages; `more_uses` counts the
/// rest.
pub const MAX_LISTED_RESOURCE_USES: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedResource {
    pub from: DepartmentId,
    pub to: DepartmentId,
    pub uses: Vec<ResourceUse>,
    pub more_uses: u64,
}

/// One period's spend cap of a unit and its counted spend for the current UTC period (Plan07 02B,
/// 02C), as the department's managers see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitBudgetState {
    /// `day` or `month`.
    pub period: &'static str,
    pub cap_micros: Option<i64>,
    pub spent_micros: i64,
    pub unpriced_tokens: i64,
}
