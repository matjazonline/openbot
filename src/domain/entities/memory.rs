use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const MAX_MEMORY_CONTEXT_CHARS: usize = 16_000;
pub const MAX_MEMORY_PROVIDER_OPERATION_SECONDS: u64 = 120;
pub const MEMORY_DELETION_QUIESCENCE_SECONDS: i64 = 180;
pub const MEMORY_READINESS_WINDOW_SECONDS: i64 = 15 * 60;
pub const MEMORY_READINESS_TIMEOUT_ERROR: &str = "memory provider readiness deadline was exceeded";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryProviderKind {
    Hydradb,
}

impl MemoryProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hydradb => "hydradb",
        }
    }
}

impl fmt::Display for MemoryProviderKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryConnectionReadiness {
    #[default]
    Pending,
    Provisioning,
    Ready,
    Failed,
}

impl MemoryConnectionReadiness {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Provisioning => "provisioning",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryConnection {
    pub company_id: Uuid,
    pub provider: MemoryProviderKind,
    pub remote_database_id: String,
    pub readiness: MemoryConnectionReadiness,
    pub last_error: Option<String>,
    pub provisioning_phase: Option<MemoryProvisioningPhase>,
    pub failure_attempts: i32,
    pub readiness_deadline: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryProvisioningPhase {
    CreatePending,
    WaitingReady,
    Ready,
    Failed,
}

impl MemoryProvisioningPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CreatePending => "create_pending",
            Self::WaitingReady => "waiting_ready",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeasedProvisioningJob {
    pub id: Uuid,
    pub company_id: Uuid,
    pub provider: MemoryProviderKind,
    pub remote_database_id: String,
    pub phase: MemoryProvisioningPhase,
    pub failure_attempts: i32,
    pub readiness_deadline: Option<DateTime<Utc>>,
    pub lease_token: Uuid,
    pub operation_generation: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeasedCleanupJob {
    pub id: Uuid,
    pub provider: MemoryProviderKind,
    pub remote_database_id: String,
    pub failure_attempts: i32,
    pub lease_token: Uuid,
    pub operation_generation: i64,
}

/// A provider-safe error. Messages deliberately contain no request bodies or credentials.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MemoryProviderError {
    #[error("memory provider authentication failed")]
    Authentication,
    #[error("memory provider rate limit was reached")]
    RateLimited,
    #[error("memory provider request timed out")]
    Timeout,
    #[error("memory provider returned a malformed response")]
    MalformedResponse,
    #[error("memory provider workspace is not ready")]
    NotReady,
    #[error("memory provider is unavailable")]
    Unavailable,
    #[error("memory provider rejected an item")]
    RejectedItem,
    #[error("memory provider workspace does not exist")]
    NotFound,
}

impl MemoryProviderError {
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited | Self::Timeout | Self::NotReady | Self::Unavailable
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryRecallMode {
    #[default]
    Fast,
    Thinking,
}

impl MemoryRecallMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Thinking => "thinking",
        }
    }
}

pub fn default_memory_max_results() -> u8 {
    5
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryScope {
    Company,
    Agent(Uuid),
    User,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedMemoryScope {
    pub scope: MemoryScope,
    pub collection: String,
    pub weight: f32,
}

pub fn normalize_sender_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub fn resolve_scopes(
    company: bool,
    agent: bool,
    user: bool,
    agent_id: Option<Uuid>,
    sender_email: Option<&str>,
) -> (Vec<ResolvedMemoryScope>, Vec<&'static str>) {
    let mut by_collection: HashMap<String, ResolvedMemoryScope> = HashMap::new();
    let mut warnings = Vec::new();
    let requested = [
        (company, MemoryScope::Company, 1.0),
        (
            agent,
            agent_id
                .map(MemoryScope::Agent)
                .unwrap_or(MemoryScope::Company),
            2.0,
        ),
        (user, MemoryScope::User, 3.0),
    ];
    for (enabled, scope, weight) in requested {
        if !enabled {
            continue;
        }
        let collection = match scope {
            MemoryScope::Company => "company".to_string(),
            MemoryScope::Agent(id) => format!("agent_{id}"),
            MemoryScope::User => match sender_email
                .map(normalize_sender_email)
                .filter(|e| !e.is_empty())
            {
                Some(email) => format!("user_{email}"),
                None => {
                    warnings.push("user");
                    "company".to_string()
                }
            },
        };
        if agent && agent_id.is_none() && weight == 2.0 {
            warnings.push("agent");
        }
        by_collection
            .entry(collection.clone())
            .and_modify(|existing| existing.weight = existing.weight.max(weight))
            .or_insert(ResolvedMemoryScope {
                scope,
                collection,
                weight,
            });
    }
    let mut scopes: Vec<_> = by_collection.into_values().collect();
    scopes.sort_by(|a, b| a.collection.cmp(&b.collection));
    warnings.sort_unstable();
    warnings.dedup();
    (scopes, warnings)
}

pub fn stable_memory_id(task_id: Uuid, channel_id: Uuid, agent_id: Option<Uuid>) -> String {
    let mut hash = Sha256::new();
    hash.update(task_id.as_bytes());
    hash.update(channel_id.as_bytes());
    if let Some(id) = agent_id {
        hash.update(id.as_bytes());
    }
    format!("memory_{:x}", hash.finalize())
}

pub fn remote_memory_database_id(company_id: Uuid) -> String {
    format!("mail-agents-company-{}", company_id.simple())
}

pub fn deduplicate_chunks(chunks: impl IntoIterator<Item = MemoryChunk>) -> Vec<MemoryChunk> {
    let mut ids = HashSet::new();
    let mut contents = HashSet::new();
    let mut total = 0usize;
    let mut result = Vec::new();
    for mut chunk in chunks {
        let normalized = chunk
            .content
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        let duplicate_content = !contents.insert(normalized);
        let duplicate_id = chunk
            .source_chunk_id
            .as_ref()
            .is_some_and(|id| !ids.insert(id.clone()));
        if duplicate_id || duplicate_content || total >= MAX_MEMORY_CONTEXT_CHARS {
            continue;
        }
        let remaining = MAX_MEMORY_CONTEXT_CHARS - total;
        let char_count = chunk.content.chars().count();
        if char_count > remaining {
            chunk.content = chunk.content.chars().take(remaining).collect();
        }
        total += chunk.content.chars().count();
        result.push(chunk);
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryChunk {
    pub source_chunk_id: Option<String>,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fallback_deduplicates_and_keeps_highest_weight() {
        let (scopes, warnings) = resolve_scopes(true, true, true, None, None);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].collection, "company");
        assert_eq!(scopes[0].weight, 3.0);
        assert_eq!(warnings, ["agent", "user"]);
    }
    #[test]
    fn all_available_scopes_are_additive() {
        let agent = Uuid::nil();
        let (scopes, warnings) =
            resolve_scopes(true, true, true, Some(agent), Some(" USER@Example.COM "));
        assert_eq!(scopes.len(), 3);
        assert!(warnings.is_empty());
        assert!(
            scopes
                .iter()
                .any(|s| s.collection == "user_user@example.com" && s.weight == 3.0)
        );
    }
    #[test]
    fn memory_id_is_stable_and_agent_sensitive() {
        let task = Uuid::new_v4();
        let channel = Uuid::new_v4();
        assert_eq!(
            stable_memory_id(task, channel, None),
            stable_memory_id(task, channel, None)
        );
        assert_ne!(
            stable_memory_id(task, channel, None),
            stable_memory_id(task, channel, Some(Uuid::nil()))
        );
    }

    #[test]
    fn remote_database_id_is_deterministic_and_company_scoped() {
        let company = Uuid::new_v4();
        assert_eq!(
            remote_memory_database_id(company),
            remote_memory_database_id(company)
        );
        assert_ne!(
            remote_memory_database_id(company),
            remote_memory_database_id(Uuid::new_v4())
        );
    }

    #[test]
    fn deduplication_crosses_id_and_content_forms() {
        let chunks = deduplicate_chunks([
            MemoryChunk {
                source_chunk_id: Some("source-1".into()),
                content: "Same   CONTENT".into(),
            },
            MemoryChunk {
                source_chunk_id: None,
                content: "same content".into(),
            },
            MemoryChunk {
                source_chunk_id: Some("source-2".into()),
                content: "different".into(),
            },
        ]);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn unicode_cap_uses_character_boundaries() {
        let content = "é".repeat(MAX_MEMORY_CONTEXT_CHARS + 5);
        let chunks = deduplicate_chunks([MemoryChunk {
            source_chunk_id: None,
            content,
        }]);
        assert_eq!(chunks[0].content.chars().count(), MAX_MEMORY_CONTEXT_CHARS);
    }
}
