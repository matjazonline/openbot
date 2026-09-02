use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    creation::CreationProvenance,
    memory::{MemoryPersistenceMode, MemoryRecallMode, default_memory_max_results},
    value_objects::AvatarUrl,
};

pub const MIN_AGENT_RUN_TIMEOUT_SECS: u32 = 1;
pub const MAX_AGENT_RUN_TIMEOUT_SECS: u32 = 3_600;

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
    /// Per-run wall-clock limit. `None` inherits the deployment-wide default.
    #[serde(default)]
    pub run_timeout_secs: Option<u32>,
    pub system_prompt: Option<String>,
    /// A short statement of what this agent is for, shown to other agents in the same company by
    /// the agent directory tool. Not part of the prompt.
    pub description: Option<String>,
    pub config_json: Option<serde_json::Value>,
    /// Master policy switch for every memory scope used by this agent.
    #[serde(default)]
    pub memory_enabled: bool,
    #[serde(default)]
    pub memory_persistence_mode: MemoryPersistenceMode,
    #[serde(default)]
    pub memory_recall_mode: MemoryRecallMode,
    #[serde(default = "default_memory_max_results")]
    pub memory_max_results: u8,
    /// The picture shown next to the agent's name; `None` renders as a letter bubble.
    pub avatar_url: Option<AvatarUrl>,
    pub created_by: CreationProvenance,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Agent {
    pub fn is_library(&self) -> bool {
        self.company_id.is_none()
    }

    pub fn default_config() -> serde_json::Value {
        serde_json::json!({
            "system_prompt": "You are a helpful email agent."
        })
    }

    pub fn run_timeout(&self, global: std::time::Duration) -> std::time::Duration {
        self.run_timeout_secs
            .map(|seconds| std::time::Duration::from_secs(u64::from(seconds)))
            .unwrap_or(global)
    }
}
