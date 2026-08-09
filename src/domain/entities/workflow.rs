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
