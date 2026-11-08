//! Bounded inspection of native agent conversations through an actual run/step anchor.
use super::{agent::AgentToolDeclaration, agent_history::*};
use crate::app_error::{AppError, AppResult};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

pub const MAX_HISTORY_PAGE: u8 = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct HistoryCursor(String);
impl HistoryCursor {
    pub fn parse(value: String) -> AppResult<Self> {
        let cursor = Self(value);
        cursor.parts()?;
        Ok(cursor)
    }
    pub fn after(conversation: AgentConversationId, order: i64) -> AppResult<Self> {
        Self::parse(format!("1:{}:{order}", conversation.as_uuid()))
    }
    pub fn parts(&self) -> AppResult<(AgentConversationId, AgentExchangeOrder)> {
        if self.0.len() > 80 {
            return Err(invalid_cursor());
        }
        let fields: Vec<_> = self.0.split(':').collect();
        if fields.len() != 3 || fields[0] != "1" {
            return Err(invalid_cursor());
        }
        let id = Uuid::parse_str(fields[1]).map_err(|_| invalid_cursor())?;
        let order = fields[2]
            .parse::<u64>()
            .ok()
            .and_then(|order| AgentExchangeOrder::try_from(order).ok())
            .ok_or_else(invalid_cursor)?;
        if format!("1:{id}:{}", order.get()) != self.0 {
            return Err(invalid_cursor());
        }
        Ok((AgentConversationId::new(id), order))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
fn invalid_cursor() -> AppError {
    AppError::BadRequest("Invalid agent history cursor".into())
}

#[derive(Debug, Clone)]
pub struct HistoryPageRequest {
    pub after: Option<HistoryCursor>,
    pub limit: u8,
}
impl HistoryPageRequest {
    pub fn new(after: Option<String>, limit: Option<u8>) -> AppResult<Self> {
        let limit = limit.unwrap_or(MAX_HISTORY_PAGE);
        if !(1..=MAX_HISTORY_PAGE).contains(&limit) {
            return Err(AppError::BadRequest(
                "Agent history page size must be 1..16".into(),
            ));
        }
        Ok(Self {
            after: after.map(HistoryCursor::parse).transpose()?,
            limit,
        })
    }
}

pub use super::agent_history::AgentExchangeState as HistoryState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryPayloadState {
    Retained,
    Expired,
    Restricted,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryTiming {
    pub created_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistorySource {
    pub run: crate::domain::workflow::RunId,
    pub execution: Uuid,
    pub checkpoint: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryProvenance {
    pub agent: crate::entities::agent_capability::AgentId,
    pub revision: crate::entities::agent_capability::AgentRevisionId,
    pub manifest: crate::entities::agent_capability::DependencyManifestId,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryEntry {
    pub order: i64,
    pub state: HistoryState,
    pub timing: HistoryTiming,
    pub payload: HistoryPayloadState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exchange: Option<AgentExchangeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<HistorySource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<HistoryActivity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<HistoryProvenance>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryActivity {
    Queued,
    WaitingForTurn,
    Active,
    WaitingForHuman,
    Produced,
    FailedBeforeOutput,
    CancelledBeforeOutput,
}
impl HistoryActivity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::WaitingForTurn => "Waiting for agent turn",
            Self::Active => "Active",
            Self::WaitingForHuman => "Waiting for human input",
            Self::Produced => "Produced",
            Self::FailedBeforeOutput => "Failed before output",
            Self::CancelledBeforeOutput => "Cancelled before output",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryPage {
    pub entries: Vec<HistoryEntry>,
    pub next: Option<HistoryCursor>,
    pub expired_count: i64,
    pub expired_through_order: i64,
}

#[derive(Serialize)]
pub struct FrozenHistoryInspection {
    pub history: FrozenAgentHistory,
    pub prompt: String,
    pub current_prompt: String,
    pub output_schema: Value,
    pub tools: Vec<AgentToolDeclaration>,
}

#[derive(Serialize)]
#[serde(tag = "availability", rename_all = "snake_case")]
pub enum HistorySelectedSource {
    Available {
        exchange: AgentExchangeId,
        source: HistorySource,
    },
    Expired {
        exchange: AgentExchangeId,
    },
    Unavailable {
        exchange: AgentExchangeId,
    },
}

#[derive(Serialize)]
pub struct HistoryOutputStatus {
    pub workflow: String,
    pub waiting_reason: Option<String>,
    pub channel_response: Option<String>,
    pub delivery: Option<String>,
}

#[derive(Serialize)]
pub struct HistoryPayload {
    pub input: Value,
    /// Some(JSON null) is accepted output; state supplies the Produced distinction.
    pub output: Option<Value>,
    pub frozen: Option<FrozenHistoryInspection>,
    pub selected_sources: Vec<HistorySelectedSource>,
}

#[derive(Serialize)]
pub struct HistoryDetail {
    pub entry: HistoryEntry,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<HistoryPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<HistoryOutputStatus>,
}
