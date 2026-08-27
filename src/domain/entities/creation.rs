use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CreatorType {
    User,
    Agent,
    System,
}

/// Immutable attribution for a durable resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreationProvenance {
    pub actor_type: CreatorType,
    pub actor_id: Option<Uuid>,
    pub actor_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_channel_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_task_id: Option<Uuid>,
}

impl CreationProvenance {
    pub fn label(&self) -> String {
        match self.actor_type {
            CreatorType::User => format!("Created by {}", self.actor_name),
            CreatorType::Agent => format!("Created by agent {}", self.actor_name),
            CreatorType::System => "Created by System".into(),
        }
    }

    pub fn user(id: Uuid) -> Self {
        Self {
            actor_type: CreatorType::User,
            actor_id: Some(id),
            actor_name: "Company owner".into(),
            source_channel_id: None,
            source_task_id: None,
        }
    }

    pub fn agent(id: Uuid, name: String, channel_id: Uuid, task_id: Uuid) -> Self {
        Self {
            actor_type: CreatorType::Agent,
            actor_id: Some(id),
            actor_name: name,
            source_channel_id: Some(channel_id),
            source_task_id: Some(task_id),
        }
    }

    pub fn system() -> Self {
        Self {
            actor_type: CreatorType::System,
            actor_id: None,
            actor_name: "System".into(),
            source_channel_id: None,
            source_task_id: None,
        }
    }
}
