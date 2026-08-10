use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workflow {
    pub id: Uuid,
    pub company_id: Uuid,
    pub name: String,
    pub slug: String,
    pub participant_emails: Option<Vec<String>>,
    pub workflow_config: Option<serde_json::Value>,
    pub created_at: chrono::NaiveDateTime,
}

impl Workflow {
    pub fn default_config() -> serde_json::Value {
        serde_json::json!({
            "trigger": "email_received",
            "action": "ai_agent_reply",
            "mode": "auto"
        })
    }
}
