use std::{collections::HashSet, fmt};

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPersistenceMode {
    #[default]
    AudienceOnly,
    ScopeSpecificFacts,
}

impl MemoryPersistenceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AudienceOnly => "audience_only",
            Self::ScopeSpecificFacts => "scope_specific_facts",
        }
    }
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

impl MemoryScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Company => "Company",
            Self::Agent(_) => "Agent",
            Self::User => "User",
        }
    }

    fn precedence(self) -> u8 {
        match self {
            Self::User => 3,
            Self::Agent(_) => 2,
            Self::Company => 1,
        }
    }

    pub fn extraction_instructions(self) -> &'static str {
        match self {
            Self::User => USER_MEMORY_EXTRACTION_INSTRUCTIONS,
            Self::Agent(_) => AGENT_MEMORY_EXTRACTION_INSTRUCTIONS,
            Self::Company => COMPANY_MEMORY_EXTRACTION_INSTRUCTIONS,
        }
    }
}

pub const USER_MEMORY_EXTRACTION_INSTRUCTIONS: &str = "Extract only durable preferences, facts, constraints, and corrections attributable to this user. Exclude credentials, secrets, unrelated personal facts, and transient request details. If there are no qualifying facts, extract nothing.";
pub const AGENT_MEMORY_EXTRACTION_INSTRUCTIONS: &str = "Extract only reusable lessons about agent behavior, tools, workflows, failures, and successful strategies. Exclude credentials, secrets, unrelated personal facts, and transient request details. If there are no qualifying facts, extract nothing.";
pub const COMPANY_MEMORY_EXTRACTION_INSTRUCTIONS: &str = "Extract only durable organization-wide policies, terminology, decisions, and processes. Exclude credentials, secrets, unrelated personal facts, and transient request details. If there are no qualifying facts, extract nothing.";

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedMemoryScope {
    pub scope: MemoryScope,
    pub collection: String,
    pub weight: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableMemoryScope {
    Agent,
    User,
}

impl UnavailableMemoryScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::User => "user",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedMemoryScopes {
    pub resolved: Vec<ResolvedMemoryScope>,
    pub unavailable: Vec<UnavailableMemoryScope>,
}

pub fn normalize_sender_email(email: &str) -> String {
    email.trim().to_lowercase()
}

fn user_collection(email: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(normalize_sender_email(email).as_bytes());
    format!("user_{:x}", hash.finalize())
}

pub fn resolve_scopes(
    company: bool,
    agent: bool,
    user: bool,
    agent_id: Option<Uuid>,
    sender_email: Option<&str>,
) -> ResolvedMemoryScopes {
    let mut result = ResolvedMemoryScopes::default();
    if company {
        result.resolved.push(ResolvedMemoryScope {
            scope: MemoryScope::Company,
            collection: "company".to_string(),
            weight: 1.0,
        });
    }
    if agent {
        match agent_id {
            Some(id) => result.resolved.push(ResolvedMemoryScope {
                scope: MemoryScope::Agent(id),
                collection: format!("agent_{id}"),
                weight: 2.0,
            }),
            None => result.unavailable.push(UnavailableMemoryScope::Agent),
        }
    }
    if user {
        match sender_email
            .map(normalize_sender_email)
            .filter(|email| !email.is_empty())
        {
            Some(email) => result.resolved.push(ResolvedMemoryScope {
                scope: MemoryScope::User,
                collection: user_collection(&email),
                weight: 3.0,
            }),
            None => result.unavailable.push(UnavailableMemoryScope::User),
        }
    }
    result
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
    let mut chunks: Vec<_> = chunks.into_iter().collect();
    chunks.sort_by_key(|chunk| std::cmp::Reverse(chunk.source_scope.precedence()));
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
    pub source_scope: MemoryScope,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_scopes_are_skipped_without_company_fallback() {
        let scopes = resolve_scopes(false, true, true, None, None);
        assert!(scopes.resolved.is_empty());
        assert_eq!(
            scopes.unavailable,
            [UnavailableMemoryScope::Agent, UnavailableMemoryScope::User]
        );
    }
    #[test]
    fn all_available_scopes_are_additive() {
        let agent = Uuid::nil();
        let scopes = resolve_scopes(true, true, true, Some(agent), Some(" USER@Example.COM "));
        assert_eq!(scopes.resolved.len(), 3);
        assert!(scopes.unavailable.is_empty());
        assert!(
            scopes
                .resolved
                .iter()
                .any(|s| s.scope == MemoryScope::User && s.weight == 3.0)
        );
        assert!(
            scopes
                .resolved
                .iter()
                .all(|scope| !scope.collection.contains('@'))
        );
    }

    #[test]
    fn each_retrieval_flag_selects_only_its_own_scope() {
        let agent = Uuid::new_v4();
        let cases = [
            ((true, false, false), MemoryScope::Company),
            ((false, true, false), MemoryScope::Agent(agent)),
            ((false, false, true), MemoryScope::User),
        ];
        for ((company, agent_enabled, user), expected) in cases {
            let scopes = resolve_scopes(
                company,
                agent_enabled,
                user,
                Some(agent),
                Some("user@example.com"),
            );
            assert_eq!(scopes.resolved.len(), 1);
            assert_eq!(scopes.resolved[0].scope, expected);
            assert!(scopes.unavailable.is_empty());
        }
    }

    #[test]
    fn persistence_mode_uses_the_channel_wire_values() {
        assert_eq!(
            serde_json::to_string(&MemoryPersistenceMode::AudienceOnly).unwrap(),
            "\"audience_only\""
        );
        assert_eq!(
            serde_json::from_str::<MemoryPersistenceMode>("\"scope_specific_facts\"").unwrap(),
            MemoryPersistenceMode::ScopeSpecificFacts
        );
    }

    #[test]
    fn user_collection_is_normalized_stable_and_pii_free() {
        let first = resolve_scopes(false, false, true, None, Some(" User@Example.COM "));
        let second = resolve_scopes(false, false, true, None, Some("user@example.com"));
        assert_eq!(first.resolved[0].collection, second.resolved[0].collection);
        assert!(!first.resolved[0].collection.contains('@'));
        assert!(!first.resolved[0].collection.contains("example"));
    }

    #[test]
    fn missing_user_does_not_suppress_available_company_and_agent_scopes() {
        let agent = Uuid::new_v4();
        let scopes = resolve_scopes(true, true, true, Some(agent), None);
        assert_eq!(
            scopes
                .resolved
                .iter()
                .map(|scope| scope.scope)
                .collect::<Vec<_>>(),
            [MemoryScope::Company, MemoryScope::Agent(agent)]
        );
        assert_eq!(scopes.unavailable, [UnavailableMemoryScope::User]);
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
                source_scope: MemoryScope::Company,
            },
            MemoryChunk {
                source_chunk_id: None,
                content: "same content".into(),
                source_scope: MemoryScope::User,
            },
            MemoryChunk {
                source_chunk_id: Some("source-2".into()),
                content: "different".into(),
                source_scope: MemoryScope::Agent(Uuid::nil()),
            },
        ]);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].source_scope, MemoryScope::User);
    }

    #[test]
    fn unicode_cap_uses_character_boundaries() {
        let content = "é".repeat(MAX_MEMORY_CONTEXT_CHARS + 5);
        let chunks = deduplicate_chunks([MemoryChunk {
            source_chunk_id: None,
            content,
            source_scope: MemoryScope::Company,
        }]);
        assert_eq!(chunks[0].content.chars().count(), MAX_MEMORY_CONTEXT_CHARS);
    }
}
