use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{creation::CreationProvenance, value_objects::AvatarUrl};

pub const MAX_AGENT_SKILLS: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Agent {
    pub id: Uuid,
    /// `None` identifies an operator-managed definition in the global agent library.
    #[serde(default)]
    pub company_id: Option<Uuid>,
    pub name: String,
    pub slug: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    /// A short statement of what this agent is for, shown to other agents in the same company by
    /// the agent directory tool. Not part of the prompt.
    pub description: Option<String>,
    /// The picture shown next to the agent's name; `None` renders as a letter bubble.
    pub avatar_url: Option<AvatarUrl>,
    pub created_by: CreationProvenance,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Agent {
    pub fn is_library(&self) -> bool {
        self.company_id.is_none()
    }
}
