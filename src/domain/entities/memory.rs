use std::{collections::HashSet, fmt};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::entities::transport::PrincipalId;

pub const MAX_MEMORY_CONTEXT_CHARS: usize = 16_000;
pub const MAX_MEMORY_RECALL_QUERY_CHARS: usize = 16_000;
pub const MAX_MEMORY_PERSIST_USER_CONTEXT_CHARS: usize = 32_000;
pub const MAX_MEMORY_PERSIST_ASSISTANT_ANSWER_CHARS: usize = 32_000;
pub const MAX_MEMORY_ADDITIONAL_CONTEXT_CHARS: usize = 512;
pub const MAX_MEMORY_UPSTREAM_CONTEXT_CHARS: usize = 24_000;
pub const MAX_MEMORY_TARGET_COLLECTIONS: usize = 3;
pub const MAX_MEMORY_PROVIDER_REQUEST_BYTES: usize = 384 * 1024;
pub const MAX_MEMORY_PROVIDER_REQUEST_OVERHEAD_BYTES: usize = 8 * 1024;
pub const MAX_MEMORY_PROVIDER_REQUEST_BODY_BYTES: usize =
    MAX_MEMORY_PROVIDER_REQUEST_BYTES - MAX_MEMORY_PROVIDER_REQUEST_OVERHEAD_BYTES;
pub const MAX_MEMORY_PROVIDER_RESPONSE_BYTES: usize = 512 * 1024;
pub const MAX_MEMORY_RETURNED_ROWS: usize = 20;
pub const MAX_MEMORY_CHUNK_CHARS: usize = 16_000;
pub const MAX_MEMORY_PROVIDER_IDENTIFIER_BYTES: usize = 256;
pub const MAX_MEMORY_PROVIDER_BASE_URL_BYTES: usize = 2_048;
pub const MAX_MEMORY_PROVIDER_CREDENTIAL_BYTES: usize = 4_096;
pub const MAX_MEMORY_PROVIDER_CONNECT_SECONDS: u64 = 10;
pub const MAX_MEMORY_PROVIDER_REQUEST_SECONDS: u64 = 110;
pub const MAX_MEMORY_PROVIDER_OPERATION_SECONDS: u64 = 120;
pub const MEMORY_DELETION_QUIESCENCE_SECONDS: i64 = 180;
pub const MEMORY_READINESS_WINDOW_SECONDS: i64 = 15 * 60;
pub const MEMORY_READINESS_TIMEOUT_ERROR: &str = "memory provider readiness deadline was exceeded";
pub const MEMORY_TRUNCATION_MARKER: &str = "\n...[truncated]";

/// Hindsight publishes no `bank_id` limit. Our worst case is the User scope at 123 bytes
/// (`mail-agents-company-{32 hex}--user_{64 hex}`), so bound it conservatively here rather than
/// discovering the real limit in production.
pub const MAX_HINDSIGHT_BANK_ID_BYTES: usize = 128;
/// Hindsight's recall is budgeted in tokens rather than rows; this is the per-scope budget.
pub const HINDSIGHT_RECALL_MAX_TOKENS: u32 = 4_096;

/// Truncate free text at a Unicode scalar boundary while keeping the marker inside the limit.
pub fn truncate_memory_text(value: &str, max_chars: usize) -> (String, bool) {
    let mut chars = value.chars();
    if chars.by_ref().take(max_chars.saturating_add(1)).count() <= max_chars {
        return (value.to_owned(), false);
    }

    let marker_chars = MEMORY_TRUNCATION_MARKER.chars().count();
    let content_chars = max_chars.saturating_sub(marker_chars);
    let mut truncated: String = value.chars().take(content_chars).collect();
    truncated.push_str(
        &MEMORY_TRUNCATION_MARKER
            .chars()
            .take(max_chars)
            .collect::<String>(),
    );
    (truncated, true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryProviderKind {
    Hydradb,
    Hindsight,
}

impl MemoryProviderKind {
    /// Every provider the application knows about, in the order the settings UI offers them.
    /// Iterating this is what keeps the wire strings, the `<select>` options and the stored-value
    /// parsing from drifting apart as providers are added.
    pub const ALL: [Self; 2] = [Self::Hydradb, Self::Hindsight];

    /// The wire and database value. Must stay in sync with the `provider` CHECK constraints in
    /// `migrations/20260817000000_init_schema.sql`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hydradb => "hydradb",
            Self::Hindsight => "hindsight",
        }
    }

    /// The human-facing name, for settings copy and operator-visible errors.
    pub fn label(self) -> &'static str {
        match self {
            Self::Hydradb => "HydraDB",
            Self::Hindsight => "Hindsight",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
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
    #[error("memory provider request exceeded its size bound")]
    RequestTooLarge,
    #[error("memory provider response exceeded its size bound")]
    ResponseTooLarge,
    #[error("memory provider returned more rows than requested")]
    TooManyResults,
    #[error("memory provider operation exceeded its target collection bound")]
    TooManyTargets,
    #[error("memory provider identifier is not well formed")]
    InvalidIdentifier,
}

impl MemoryProviderError {
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited | Self::Timeout | Self::NotReady | Self::Unavailable
        )
    }

    pub fn bound_label(&self) -> Option<&'static str> {
        match self {
            Self::RequestTooLarge => Some("request_bytes"),
            Self::ResponseTooLarge => Some("response_bytes"),
            Self::TooManyResults => Some("result_rows"),
            Self::TooManyTargets => Some("target_collections"),
            _ => None,
        }
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

/// Where one principal's memories live.
///
/// Keyed on the principal id, not on a handle. A provider identity string is a *name* a person
/// arrived under, and two of them -- a mailbox and a Slack account -- can belong to one person;
/// hashing the string would give that person two disjoint memories and would give whoever next
/// holds a recycled address the previous holder's. The principal is the actor, so it is the scope.
///
/// Still hashed, so a collection name carries no identifier out to the provider.
fn user_collection(principal: PrincipalId) -> String {
    let mut hash = Sha256::new();
    hash.update(b"principal:");
    hash.update(principal.as_uuid().as_bytes());
    format!("user_{:x}", hash.finalize())
}

pub fn resolve_scopes(
    company: bool,
    agent: bool,
    user: bool,
    agent_id: Option<Uuid>,
    subject_principal: Option<PrincipalId>,
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
        match subject_principal {
            Some(principal) => result.resolved.push(ResolvedMemoryScope {
                scope: MemoryScope::User,
                collection: user_collection(principal),
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
        let (content, truncated) = truncate_memory_text(&chunk.content, MAX_MEMORY_CHUNK_CHARS);
        chunk.content = content;
        chunk.truncated |= truncated;
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
        let (content, truncated) = truncate_memory_text(&chunk.content, remaining);
        chunk.content = content;
        chunk.truncated |= truncated;
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
    pub truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_provider_kind_round_trips_through_its_wire_value() {
        for kind in MemoryProviderKind::ALL {
            assert_eq!(MemoryProviderKind::parse(kind.as_str()), Some(kind));
            assert!(!kind.label().is_empty());
        }
        assert_eq!(MemoryProviderKind::parse("none"), None);
        assert_eq!(MemoryProviderKind::parse(""), None);
    }

    #[test]
    fn identifier_errors_are_not_retryable_and_carry_no_bound_label() {
        assert!(!MemoryProviderError::InvalidIdentifier.retryable());
        assert_eq!(MemoryProviderError::InvalidIdentifier.bound_label(), None);
    }

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
        let scopes = resolve_scopes(true, true, true, Some(agent), Some(PrincipalId::random()));
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
                Some(PrincipalId::random()),
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

    /// The scope key is the actor, so two sightings of one principal share a memory and two
    /// principals never do -- and the key itself carries no identifier out to the provider.
    #[test]
    fn user_collection_follows_the_principal_and_leaks_nothing() {
        let principal = PrincipalId::random();
        let first = resolve_scopes(false, false, true, None, Some(principal));
        let second = resolve_scopes(false, false, true, None, Some(principal));
        assert_eq!(first.resolved[0].collection, second.resolved[0].collection);

        let other = resolve_scopes(false, false, true, None, Some(PrincipalId::random()));
        assert_ne!(first.resolved[0].collection, other.resolved[0].collection);

        let collection = &first.resolved[0].collection;
        assert!(!collection.contains('@'));
        assert!(!collection.contains(&principal.to_string()));
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
                truncated: false,
            },
            MemoryChunk {
                source_chunk_id: None,
                content: "same content".into(),
                source_scope: MemoryScope::User,
                truncated: false,
            },
            MemoryChunk {
                source_chunk_id: Some("source-2".into()),
                content: "different".into(),
                source_scope: MemoryScope::Agent(Uuid::nil()),
                truncated: false,
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
            truncated: false,
        }]);
        assert_eq!(chunks[0].content.chars().count(), MAX_MEMORY_CONTEXT_CHARS);
        assert!(chunks[0].content.ends_with(MEMORY_TRUNCATION_MARKER));
        assert!(chunks[0].truncated);
    }

    #[test]
    fn truncation_boundary_and_unicode_marker_are_exact() {
        let exact = "é".repeat(MAX_MEMORY_RECALL_QUERY_CHARS);
        assert_eq!(
            truncate_memory_text(&exact, MAX_MEMORY_RECALL_QUERY_CHARS),
            (exact, false)
        );

        let over = "é".repeat(MAX_MEMORY_RECALL_QUERY_CHARS + 1);
        let (bounded, truncated) = truncate_memory_text(&over, MAX_MEMORY_RECALL_QUERY_CHARS);
        assert!(truncated);
        assert_eq!(bounded.chars().count(), MAX_MEMORY_RECALL_QUERY_CHARS);
        assert!(bounded.ends_with(MEMORY_TRUNCATION_MARKER));
    }
}
