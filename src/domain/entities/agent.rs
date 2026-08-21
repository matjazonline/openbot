use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::value_objects::AvatarUrl;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Agent {
    pub id: Uuid,
    pub company_id: Uuid,
    pub name: String,
    pub slug: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    #[serde(skip_serializing)]
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
    /// A short statement of what this agent is for, shown to other agents in the same company by
    /// the agent directory tool. Not part of the prompt.
    pub description: Option<String>,
    pub config_json: Option<serde_json::Value>,
    /// The picture shown next to the agent's name; `None` renders as a letter bubble.
    pub avatar_url: Option<AvatarUrl>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Agent {
    pub fn default_config() -> serde_json::Value {
        serde_json::json!({
            "system_prompt": "You are a helpful email agent."
        })
    }
}
