use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CreatorType {
    User,
    System,
}

/// Immutable attribution for a durable resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreationProvenance {
    pub actor_type: CreatorType,
    pub actor_id: Option<Uuid>,
    pub actor_name: String,
}

impl CreationProvenance {
    pub fn label(&self) -> String {
        match self.actor_type {
            CreatorType::User => format!("Created by {}", self.actor_name),
            CreatorType::System => "Created by System".into(),
        }
    }

    pub fn user(id: Uuid) -> Self {
        Self {
            actor_type: CreatorType::User,
            actor_id: Some(id),
            actor_name: "Company owner".into(),
        }
    }

    pub fn system() -> Self {
        Self {
            actor_type: CreatorType::System,
            actor_id: None,
            actor_name: "System".into(),
        }
    }
}
