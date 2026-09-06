//! Immutable response versions and the exact canonical publication they may produce.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::transport::{ExternalDestination, PrincipalId, uuid_id};

uuid_id!(ResponseDraftId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseDraftStatus {
    Active,
    Superseded,
    Published,
}

impl ResponseDraftStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Superseded => "superseded",
            Self::Published => "published",
        }
    }
}

/// Recipients frozen with a draft version rather than re-derived when it is published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum DraftRecipientSnapshot {
    #[serde(rename = "1")]
    V1 {
        to: Vec<ExternalDestination>,
        cc: Vec<ExternalDestination>,
    },
}

impl DraftRecipientSnapshot {
    pub fn email(
        to: crate::entities::value_objects::EmailAddress,
        cc: Vec<crate::entities::value_objects::EmailAddress>,
    ) -> Self {
        Self::V1 {
            to: vec![ExternalDestination::Email(to)],
            cc: cc.into_iter().map(ExternalDestination::Email).collect(),
        }
    }
}

/// One immutable draft version. Workflow status may advance, and retention may clear its optional
/// task link, but authored content, scope, author, and recipients never change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseDraft {
    pub id: ResponseDraftId,
    pub version: u32,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub task_id: Option<Uuid>,
    pub author_principal_id: PrincipalId,
    pub subject: String,
    pub body: String,
    pub recipients: DraftRecipientSnapshot,
    pub status: ResponseDraftStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
