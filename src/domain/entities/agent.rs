use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Agent {
    pub id: Uuid,
    pub company_id: Uuid,
    pub name: String,
    pub slug: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
    pub config_json: Option<serde_json::Value>,
    pub created_at: chrono::NaiveDateTime,
}

impl Agent {
    pub fn default_config() -> serde_json::Value {
        serde_json::json!({
            "system_prompt": "You are a helpful email agent."
        })
    }
}
