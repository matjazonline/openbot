//! Human-operational work projected from authoritative tasks, handoffs, reviews and failures.
//!
//! None of the states in this module is persisted as another workflow. Resolving an item always
//! means changing its source; reading this projection is side-effect free.

use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{correlation::CorrelationId, transport::PrincipalId};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessPriority {
    #[default]
    Normal,
    High,
    Urgent,
}

impl BusinessPriority {
    pub const ALL: [Self; 3] = [Self::Normal, Self::High, Self::Urgent];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::High => "high",
            Self::Urgent => "urgent",
        }
    }

    pub const fn sort_rank(self) -> u8 {
        match self {
            Self::Urgent => 0,
            Self::High => 1,
            Self::Normal => 2,
        }
    }
}

impl FromStr for BusinessPriority {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "normal" => Ok(Self::Normal),
            "high" => Ok(Self::High),
            "urgent" => Ok(Self::Urgent),
            _ => Err(format!("invalid business priority '{value}'")),
        }
    }
}

impl fmt::Display for BusinessPriority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionSourceKind {
    Task,
    Handoff,
    ResponseReview,
    DelegationDecision,
    DeliveryFailure,
}

impl AttentionSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Handoff => "handoff",
            Self::ResponseReview => "response_review",
            Self::DelegationDecision => "delegation_decision",
            Self::DeliveryFailure => "delivery_failure",
        }
    }
}

impl FromStr for AttentionSourceKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "task" => Ok(Self::Task),
            "handoff" => Ok(Self::Handoff),
            "response_review" => Ok(Self::ResponseReview),
            "delegation_decision" => Ok(Self::DelegationDecision),
            "delivery_failure" => Ok(Self::DeliveryFailure),
            _ => Err(format!("invalid attention source kind '{value}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionView {
    MyWork,
    Unassigned,
    TeamWork,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "principal_id", rename_all = "snake_case")]
pub enum AttentionResponsibility {
    Principal(PrincipalId),
    ChannelTeam,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionItem {
    pub source_kind: AttentionSourceKind,
    pub source_id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub correlation_id: Option<CorrelationId>,
    /// Source-owned business status, not a copied lifecycle.
    pub state: String,
    pub responsibility: AttentionResponsibility,
    pub responsibility_label: String,
    pub title: String,
    pub next_action: String,
    pub priority: BusinessPriority,
    pub due_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Filled only after the application has proved channel visibility.
    pub href: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionCursor {
    pub as_of: DateTime<Utc>,
    pub due_rank: u8,
    pub priority_rank: u8,
    pub created_at: DateTime<Utc>,
    pub source_kind: AttentionSourceKind,
    pub source_id: Uuid,
}

impl AttentionCursor {
    pub fn for_item(item: &AttentionItem, as_of: DateTime<Utc>) -> Self {
        let due_rank = match item.due_at {
            Some(due) if due < as_of => 0,
            Some(due) if due <= as_of + chrono::Duration::hours(24) => 1,
            _ => 2,
        };
        Self {
            as_of,
            due_rank,
            priority_rank: item.priority.sort_rank(),
            created_at: item.created_at,
            source_kind: item.source_kind,
            source_id: item.source_id,
        }
    }
}

impl fmt::Display for AttentionCursor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}~{}~{}~{}~{}~{}",
            self.as_of.timestamp_micros(),
            self.due_rank,
            self.priority_rank,
            self.created_at.timestamp_micros(),
            self.source_kind.as_str(),
            self.source_id
        )
    }
}

impl FromStr for AttentionCursor {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || format!("invalid attention cursor '{value}'");
        let mut parts = value.split('~');
        let as_of = parse_micros(parts.next().ok_or_else(invalid)?)?;
        let due_rank = parts
            .next()
            .ok_or_else(invalid)?
            .parse::<u8>()
            .map_err(|_| invalid())?;
        let priority_rank = parts
            .next()
            .ok_or_else(invalid)?
            .parse::<u8>()
            .map_err(|_| invalid())?;
        let created_at = parse_micros(parts.next().ok_or_else(invalid)?)?;
        let source_kind = parts.next().ok_or_else(invalid)?.parse()?;
        let source_id =
            Uuid::parse_str(parts.next().ok_or_else(invalid)?).map_err(|_| invalid())?;
        if parts.next().is_some() || due_rank > 2 || priority_rank > 2 {
            return Err(invalid());
        }
        Ok(Self {
            as_of,
            due_rank,
            priority_rank,
            created_at,
            source_kind,
            source_id,
        })
    }
}

fn parse_micros(value: &str) -> Result<DateTime<Utc>, String> {
    let micros = value
        .parse::<i64>()
        .map_err(|_| "invalid attention cursor timestamp".to_string())?;
    DateTime::from_timestamp_micros(micros)
        .ok_or_else(|| "invalid attention cursor timestamp".to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionPage {
    pub items: Vec<AttentionItem>,
    pub next_cursor: Option<String>,
    pub as_of: DateTime<Utc>,
    /// Eligible rows examined after the cursor, capped at `MAX_WORKING_SET + 1`.
    pub working_set_size: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct AttentionQuery<'a> {
    pub company_id: Uuid,
    pub principal_id: PrincipalId,
    pub visible_channel_ids: &'a [Uuid],
    pub view: AttentionView,
    pub cursor: Option<&'a AttentionCursor>,
    pub limit: usize,
}

impl AttentionQuery<'_> {
    pub const DEFAULT_LIMIT: usize = 50;
    pub const MAX_LIMIT: usize = 100;
    pub const MAX_WORKING_SET: usize = 1_000;

    pub fn clamped_limit(self) -> usize {
        self.limit.clamp(1, Self::MAX_LIMIT)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewManualHandoff {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Option<Uuid>,
    pub correlation_id: Option<CorrelationId>,
    pub title: String,
    pub next_action: String,
    pub responsible_principal_id: Option<PrincipalId>,
    pub priority: BusinessPriority,
    pub due_at: Option<DateTime<Utc>>,
    pub command_id: Uuid,
    pub actor_principal_id: PrincipalId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionSourceCommand {
    pub company_id: Uuid,
    pub source_kind: AttentionSourceKind,
    pub source_id: Uuid,
    pub command_id: Uuid,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    /// Authorization context, not command semantics; omitted from the idempotency fingerprint.
    #[serde(skip)]
    pub visible_channel_ids: Vec<Uuid>,
    pub priority: BusinessPriority,
    pub due_at: Option<DateTime<Utc>>,
    /// Only handoffs may change responsibility through this command.
    pub responsible_principal_id: Option<PrincipalId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffResolution {
    Resolved,
    Withdrawn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveHandoffCommand {
    pub company_id: Uuid,
    pub handoff_id: Uuid,
    pub command_id: Uuid,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    /// Authorization context, not command semantics; omitted from the idempotency fingerprint.
    #[serde(skip)]
    pub visible_channel_ids: Vec<Uuid>,
    pub resolution: HandoffResolution,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationalSummary {
    pub as_of: DateTime<Utc>,
    pub unassigned_count: i64,
    pub oldest_actionable_age_seconds: Option<f64>,
    pub average_time_to_claim_seconds: Option<f64>,
    pub average_delegation_wait_seconds: Option<f64>,
    pub average_review_turnaround_seconds: Option<f64>,
    pub timeout_cancel_reassign_rate: Option<f64>,
    pub average_time_to_logical_external_response_seconds: Option<f64>,
    pub permanent_delivery_failure_count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trips_every_sort_component() {
        let cursor = AttentionCursor {
            as_of: DateTime::parse_from_rfc3339("2026-09-07T10:00:00.123456Z")
                .unwrap()
                .with_timezone(&Utc),
            due_rank: 1,
            priority_rank: 0,
            created_at: DateTime::parse_from_rfc3339("2026-09-01T09:30:00Z")
                .unwrap()
                .with_timezone(&Utc),
            source_kind: AttentionSourceKind::ResponseReview,
            source_id: Uuid::new_v4(),
        };
        assert_eq!(
            cursor.to_string().parse::<AttentionCursor>().unwrap(),
            cursor
        );
    }

    #[test]
    fn cursor_rejects_unknown_ranks_and_trailing_data() {
        let id = Uuid::new_v4();
        assert!(
            format!("1~3~0~1~task~{id}")
                .parse::<AttentionCursor>()
                .is_err()
        );
        assert!(
            format!("1~0~0~1~task~{id}~extra")
                .parse::<AttentionCursor>()
                .is_err()
        );
    }
}
