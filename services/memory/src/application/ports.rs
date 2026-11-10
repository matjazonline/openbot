//! Ports at real I/O boundaries only (D1). PostgreSQL implements `Store`; the local model
//! implements `Embedder`.
use super::contract::{
    AdminCredentialView, AdminScope, ContextValidateResponse, CorrectRequest, CredentialList,
    CredentialView, DeleteRequest, FetchResponse, GenerationState, GenerationStatus, GrantSpec,
    NamespaceView, PackDetail, PackItem, PackedRevision, PurgeRequest, SaveRequest, SearchResponse,
    SemanticState, SessionCloseRequest, SessionOpenRequest, SourcesRecordRequest,
    TenantReassignResponse, TenantState, TenantView, TimelineResponse,
};
use crate::domain::{
    access_level::AccessLevel, correlation::CorrelationId, error::ServiceError,
    grant::NamespaceGrant,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;

/// The admin principal performing an administrative change, and the request it acts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Actor {
    pub id: Uuid,
    pub scope: AdminScope,
    /// Recorded in `audit_events.request_id`; `None` for CLI actions.
    pub request_id: Option<Uuid>,
    /// The caller's `x-correlation-id`, recorded beside it (06A A2-2).
    pub correlation: Option<CorrelationId>,
}

#[derive(Debug, Clone)]
pub struct AdminCredentialRecord {
    pub id: Uuid,
    pub scope: AdminScope,
    pub verifier: Vec<u8>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct ClientCredentialRecord {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub tenant: String,
    pub tenant_state: TenantState,
    pub label: String,
    pub verifier: Vec<u8>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    /// Grants on active namespaces only.
    pub grants: Vec<NamespaceGrant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantAction {
    Disable,
    Enable,
    Delete,
}

#[derive(Debug, Clone)]
pub struct NewCredential {
    pub id: Uuid,
    pub label: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub verifier: [u8; 32],
    pub grants: Vec<GrantSpec>,
}

/// One authorized client write (02B): who writes where, under which bounds, and its logical
/// idempotency identity. The use case built it after authorizing the current grants; the
/// store re-checks tenant, namespace and credential state under the D13 parent locks.
#[derive(Debug, Clone)]
pub struct ClientWrite {
    pub tenant_id: Uuid,
    pub credential_id: Uuid,
    pub namespace_id: Uuid,
    /// The narrowest write grant of the target namespace.
    pub write_max: AccessLevel,
    /// Namespaces this credential may read and their `read_max`, for resolving service sources.
    pub readable: Vec<(Uuid, AccessLevel)>,
    /// The target namespace grant's `read_max`. A write may be granted above it (D3); a stored
    /// revision above it is answered uniformly, so a write cannot probe what it cannot read.
    pub read_max: Option<AccessLevel>,
    /// The registry operation id, the idempotency scope's `kind`.
    pub kind: &'static str,
    pub key: String,
    /// SHA-256 of the canonical request (D5).
    pub request_sha256: [u8; 32],
    pub expires_at: DateTime<Utc>,
    pub now: DateTime<Utc>,
    pub request_id: Uuid,
    /// The caller's `x-correlation-id` (06A A2-2): recorded in the audit row and, for a
    /// generation, on the operation. Never part of the identity.
    pub correlation: Option<CorrelationId>,
}

/// One authorized client read.
#[derive(Debug, Clone)]
pub struct ClientRead {
    pub tenant_id: Uuid,
    pub namespace_ids: Vec<Uuid>,
    pub read_ceiling: AccessLevel,
}

/// Client use cases with storage semantics (02B). Writes return the stored result, with
/// `replayed: true` when an identical request with the same key already executed.
#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn record_sources(
        &self,
        write: &ClientWrite,
        request: &SourcesRecordRequest,
    ) -> Result<Value, ServiceError>;
    async fn open_session(
        &self,
        write: &ClientWrite,
        request: &SessionOpenRequest,
    ) -> Result<Value, ServiceError>;
    async fn close_session(
        &self,
        write: &ClientWrite,
        request: &SessionCloseRequest,
    ) -> Result<Value, ServiceError>;
    async fn save_observations(
        &self,
        write: &ClientWrite,
        request: &SaveRequest,
    ) -> Result<Value, ServiceError>;
    async fn correct_observation(
        &self,
        write: &ClientWrite,
        request: &CorrectRequest,
    ) -> Result<Value, ServiceError>;
    async fn delete_observation(
        &self,
        write: &ClientWrite,
        request: &DeleteRequest,
    ) -> Result<Value, ServiceError>;
    /// Tombstone the namespace (06A A1-2): the admin delete's effect, by a client holding the
    /// `purge` grant. Its content goes through maintenance.
    async fn purge_namespace(
        &self,
        write: &ClientWrite,
        request: &PurgeRequest,
    ) -> Result<Value, ServiceError>;
    /// The stored purge result for this key when this credential executed this exact request
    /// (the purge removed the caller's own grant, so a retry cannot pass authorization).
    async fn purge_replay(
        &self,
        tenant: Uuid,
        credential: Uuid,
        key: &str,
        request_sha256: [u8; 32],
    ) -> Result<Option<Value>, ServiceError>;
    async fn fetch_observations(
        &self,
        read: &ClientRead,
        ids: &[Uuid],
    ) -> Result<FetchResponse, ServiceError>;
}

/// One authorized search after validation: the query text for the keyword leg, its normalized
/// query embedding for the semantic leg, and the model that produced it (D7).
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub text: String,
    /// Identifier-like raw tokens, matched as phrases by the keyword leg.
    pub identifiers: Vec<String>,
    pub vector: Vec<f32>,
    pub model_id: String,
    pub limit: usize,
}

/// One claimed indexing job: the already-saved text at one revision, under one fence (D7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexJob {
    pub tenant_id: Uuid,
    pub observation_id: Uuid,
    pub revision: i64,
    pub fence: i64,
    pub title: String,
    pub body: String,
}

/// One searchable part: a byte range of the body and its normalized vector.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexedPart {
    pub byte_start: usize,
    pub byte_end: usize,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexCommit {
    /// Parts committed and the observation is `indexed`.
    Indexed,
    /// The fence or revision moved (correction, delete, another claimant): nothing written.
    Stale,
    /// The tenant or namespace is not active, or the observation expired: the lease is released
    /// and the attempt refunded, nothing written.
    Deferred,
}

/// Why indexing failed; stored as a bounded code, never content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexFailure {
    EmbeddingUnavailable,
    InvalidVector,
    PartOverLimit,
    TooManyParts,
}

impl IndexFailure {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EmbeddingUnavailable => "embedding_unavailable",
            Self::InvalidVector => "invalid_vector",
            Self::PartOverLimit => "part_over_limit",
            Self::TooManyParts => "too_many_parts",
        }
    }
}

/// Retrieval storage (D7): hybrid search and the indexing state machine.
#[async_trait]
pub trait SearchStore: Send + Sync {
    async fn search_observations(
        &self,
        read: &ClientRead,
        query: &SearchQuery,
    ) -> Result<SearchResponse, ServiceError>;
    /// Claim up to `limit` due rows in one `FOR UPDATE SKIP LOCKED` statement: fence + 1,
    /// attempt + 1, lease until now + `lease`. Rows whose final attempt's lease expired become
    /// terminal `failed` in the same statement.
    async fn claim_index_jobs(
        &self,
        limit: usize,
        lease: Duration,
    ) -> Result<Vec<IndexJob>, ServiceError>;
    /// Extend the leases still held under their fences; returns the observations renewed.
    async fn renew_index_leases(
        &self,
        jobs: &[IndexJob],
        lease: Duration,
    ) -> Result<Vec<Uuid>, ServiceError>;
    async fn commit_index(
        &self,
        job: &IndexJob,
        model_id: &str,
        parts: &[IndexedPart],
    ) -> Result<IndexCommit, ServiceError>;
    /// Record a failed attempt under the fence (backoff 30 s × 2ⁿ up to 1 h; terminal after 8).
    /// Returns false when the fence moved.
    async fn fail_index(&self, job: &IndexJob, failure: IndexFailure)
    -> Result<bool, ServiceError>;
    /// Operator action: requeue terminal-failed rows. Returns how many.
    async fn reindex_failed(&self) -> Result<u64, ServiceError>;
    /// Adopt `model_id` on first start; refuse a database indexed with another model.
    async fn adopt_embedding_model(&self, model_id: &str) -> Result<(), ServiceError>;
    /// pgvector ≥ 0.8.0 and a `vector(384)` part column, or an explicit refusal.
    async fn vector_capability(&self) -> Result<(), ServiceError>;
}

/// One authorized timeline request after validation and defaults (D6 bounds).
#[derive(Debug, Clone, Copy)]
pub struct TimelineQuery {
    pub anchor: Uuid,
    pub before: u32,
    pub after: u32,
    pub session_only: bool,
}

/// One visible context candidate, ready to pack: the item as it would be returned, the highest
/// level it exposes (its own and its provenance targets') and when it stops being current.
#[derive(Debug, Clone)]
pub struct PackCandidate {
    /// A `full` candidate's text is empty until `pack_bodies` loads it.
    pub item: PackItem,
    pub exposed_level: AccessLevel,
    /// The earliest expiry of the item and of its visible provenance targets.
    pub valid_until: Option<DateTime<Utc>>,
    /// `full` only: the exact JSON-string size of the body (quotes included), so the item's
    /// serialized size is known before its body is loaded.
    pub body_json_bytes: Option<u64>,
}

/// At most `MAX_CONTEXT_ITEMS` visible candidates in rank order (D8).
#[derive(Debug, Clone)]
pub struct PackCandidates {
    pub candidates: Vec<PackCandidate>,
    /// The candidate window was full: more visible candidates were ranked or exist.
    pub truncated: bool,
    pub semantic: SemanticState,
}

/// Progressive recall beyond search (03B): timeline, context candidates and validation, each
/// under the same D4 predicate and provenance filter as search and fetch.
#[async_trait]
pub trait RecallStore: Send + Sync {
    async fn timeline(
        &self,
        read: &ClientRead,
        query: TimelineQuery,
    ) -> Result<TimelineResponse, ServiceError>;
    /// Candidates from the hybrid search for `query`, or the most recent observations.
    async fn pack_candidates(
        &self,
        read: &ClientRead,
        query: Option<&SearchQuery>,
        detail: PackDetail,
    ) -> Result<PackCandidates, ServiceError>;
    /// The bodies of selected `full` candidates at the selected revisions; rows changed or
    /// hidden since are absent.
    async fn pack_bodies(
        &self,
        read: &ClientRead,
        items: &[(Uuid, i64)],
    ) -> Result<std::collections::HashMap<Uuid, String>, ServiceError>;
    async fn validate_pack(
        &self,
        read: &ClientRead,
        items: &[PackedRevision],
    ) -> Result<ContextValidateResponse, ServiceError>;
}

/// One provider call, exactly as the provider adapter sends it: the service-owned system text,
/// the window as JSON data, the provider-safe output schema and the output bound (D9).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ProviderRequest {
    pub system: String,
    pub user: String,
    pub schema: Value,
    pub max_output_tokens: u32,
}

impl ProviderRequest {
    /// The bytes the reservation is computed over.
    pub fn prompt_bytes(&self) -> u64 {
        (self.system.len() + self.user.len() + self.schema.to_string().len()) as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProviderUsage {
    pub input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub output_tokens: u64,
}

impl ProviderUsage {
    /// What settlement charges (D9).
    pub fn total(&self) -> u64 {
        self.input_tokens
            + self.cache_creation_input_tokens
            + self.cache_read_input_tokens
            + self.output_tokens
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    Refusal,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderResponse {
    pub stop: StopReason,
    /// The concatenated text output; validated locally, never trusted.
    pub text: String,
    pub usage: ProviderUsage,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    /// Nothing reached the provider (no connection): safe to retry.
    #[error("the request was not sent")]
    NotSent,
    /// A definite rejection the provider identified (error envelope with a request id) that may
    /// be retried with backoff: rate limit, overload, transient API error.
    #[error("the provider rejected the request ({0})")]
    Rejected(String),
    /// A definite terminal rejection: invalid request, authentication, permission, not found,
    /// too large, billing or quota.
    #[error("the provider refused the request terminally ({0})")]
    Terminal(String),
    /// No definite answer after the send (timeout, reset, a reply without the provider's
    /// envelope): the call may have run and been charged, so it is never re-sent.
    #[error("the provider outcome is unknown")]
    Ambiguous,
}

/// The inference provider (D9). Implementations send exactly the request given and report
/// usage; they never choose levels, namespaces or retries.
#[async_trait]
pub trait GenerationProvider: Send + Sync {
    /// The highest access level this destination is authorized to receive.
    fn max_level(&self) -> AccessLevel;
    async fn generate(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError>;
}

/// The local embedding model behind one inference session (D7). Calls block: the embedding
/// pool runs them on its bounded blocking slots.
pub trait Embedder: Send + Sync {
    /// The pinned identity, e.g. `intfloat/multilingual-e5-small@<revision>/fp32/mean/384`.
    fn model_id(&self) -> &str;
    /// Tokens of one complete model input, special tokens included; never padded or truncated.
    fn count_tokens(&self, input: &str) -> Result<usize, EmbedError>;
    /// Raw (unvalidated) vectors for complete model inputs of at most `MAX_INPUT_TOKENS` each.
    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, EmbedError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EmbedError {
    #[error("the input exceeds the model token limit")]
    InputTooLong,
    #[error("the embedding model could not process the input")]
    Unavailable,
}

/// One claimed generation operation, held under `fence` by `owner` (D9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationJob {
    pub tenant_id: Uuid,
    pub operation_id: Uuid,
    pub owner: Uuid,
    pub fence: i64,
    /// The accepting request's correlation id, for the worker's log lines.
    pub correlation: Option<String>,
}

/// What the dispatch compare-and-swap decided.
#[derive(Debug, Clone)]
pub enum Dispatch {
    /// `dispatched` is committed: send exactly this window, once.
    Send(Box<super::generation::PreparedGeneration>),
    /// The fence, lease or state moved (another worker, the reaper, a cancel): nothing to do.
    Stale,
    /// A re-check failed and the operation is finished with this code; nothing was sent.
    Stopped(&'static str),
}

/// How a worker finishes the operation it holds.
#[derive(Debug, Clone)]
pub enum Settlement {
    /// Output to persist, with what the provider reported.
    Saved(Vec<super::generation::GeneratedItem>, ProviderUsage),
    /// `completed` without output (`nothing_to_save`).
    Skipped(&'static str, ProviderUsage),
    /// Terminal failure with a bounded code.
    Failed(String, ProviderUsage),
    /// Nothing ran or the provider definitely rejected it: back to `accepted` with backoff, or
    /// failed once the attempts are used up. The reservation is released.
    Retry(String),
    /// The call may have run: charged, never re-sent.
    Unknown(&'static str),
    /// Shutdown before dispatch: back to `accepted`, nothing sent, nothing spent.
    Release,
    /// The provider refused the service's credentials (one of the breaker's kinds): back to
    /// `accepted` with the dispatch not counted and the reservation released, claimable again
    /// after the delay (p06 A4-1). Nothing ran.
    Paused(String, std::time::Duration),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    /// The transition committed into this state.
    Done(GenerationState),
    /// The fence or state moved: nothing written (a stale worker).
    Stale,
}

/// One page of `admin.credentials.list` (06A A1-6).
#[derive(Debug, Clone, Copy)]
pub struct CredentialPage<'a> {
    pub label: Option<&'a str>,
    pub active_only: bool,
    pub after: Option<Uuid>,
    pub limit: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reaped {
    /// Expired `claimed` leases returned to `accepted` (nothing was sent).
    pub released: u64,
    /// `dispatched` past `lease_until + grace`: `unknown`, or `cancelled` when requested.
    pub abandoned: u64,
    /// `accepted` past their request expiry.
    pub expired: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Swept {
    /// Generation requests nulled.
    pub requests: u64,
    /// Source contents nulled.
    pub sources: u64,
}

/// Content removed from deleted namespaces and tenants in one pass (06A A1-1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Removed {
    pub observations: u64,
    pub operations: u64,
    pub sources: u64,
    pub sessions: u64,
    pub namespaces_marked: u64,
    /// Credential labels of deleted tenants blanked to `removed`.
    pub labels: u64,
    /// Grants, idempotency records, spend days and schedule rows of removed tenants.
    pub tenant_rows: u64,
    pub tenants_marked: u64,
}

impl Removed {
    pub fn counts(&self) -> [u64; 8] {
        [
            self.observations,
            self.operations,
            self.sources,
            self.sessions,
            self.namespaces_marked,
            self.labels,
            self.tenant_rows,
            self.tenants_marked,
        ]
    }
}

/// Rows removed by age in one pass (06A A1-4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Aged {
    pub observations: u64,
    pub idempotency_records: u64,
    pub audit_events: u64,
    pub spend_days: u64,
    pub operations: u64,
    pub sources: u64,
}

impl Aged {
    pub fn counts(&self) -> [u64; 6] {
        [
            self.observations,
            self.idempotency_records,
            self.audit_events,
            self.spend_days,
            self.operations,
            self.sources,
        ]
    }
}

/// Generation operations (04B, D9, D13): acceptance, status, cancel and the worker protocol.
#[async_trait]
pub trait GenerationStore: Send + Sync {
    /// Store the accepted operation, its immutable request and its window's source revisions
    /// atomically (or replay the stored acceptance).
    async fn accept_generation(
        &self,
        write: &ClientWrite,
        prepared: &super::generation::PreparedGeneration,
    ) -> Result<Value, ServiceError>;
    async fn generation_status(
        &self,
        read: &ClientRead,
        operation: Uuid,
    ) -> Result<GenerationStatus, ServiceError>;
    async fn cancel_generation(
        &self,
        write: &ClientWrite,
        operation: Uuid,
    ) -> Result<Value, ServiceError>;
    /// Claim the next due operation fairly (round-robin across tenants, at most one in flight
    /// per tenant), reserving its spend; `None` when nothing can be claimed.
    async fn claim_generation(
        &self,
        owner: Uuid,
        lease: Duration,
    ) -> Result<Option<GenerationJob>, ServiceError>;
    /// Re-check everything and commit `claimed → dispatched` before anything is sent.
    async fn dispatch_generation(
        &self,
        job: &GenerationJob,
        lease: Duration,
    ) -> Result<Dispatch, ServiceError>;
    /// Extend the lease under the fence; false when it is lost or a cancel was requested.
    async fn renew_generation(
        &self,
        job: &GenerationJob,
        lease: Duration,
    ) -> Result<bool, ServiceError>;
    async fn settle_generation(
        &self,
        job: &GenerationJob,
        settlement: &Settlement,
    ) -> Result<Settled, ServiceError>;
    /// Free expired leases and pending requests, at most `limit` rows per category.
    async fn reap_generations(&self, grace: Duration, limit: i64) -> Result<Reaped, ServiceError>;
    /// Null retained generation inputs and raw evidence past retention, at most `limit` rows
    /// per category.
    async fn sweep_retention(&self, limit: i64) -> Result<Swept, ServiceError>;
    /// Remove the content of deleted namespaces and tenants (06A A1-1), at most `limit` rows per
    /// step, each step its own transaction; mark what is empty.
    async fn remove_deleted(&self, limit: i64) -> Result<Removed, ServiceError>;
    /// Age-based retention (06A A1-4, F5): expired observations, idempotency records, audit
    /// events, spend days, and unreferenced finished operations and nulled sources.
    async fn sweep_aged(&self, limit: i64) -> Result<Aged, ServiceError>;
    /// Operator action on a restored database, before `serve` (p06 A4-2): finish every
    /// unfinished operation as `cancelled(restored)`, its reservation released. Whether a
    /// dispatched call ran after the backup is unknowable, and its output is not in the
    /// restored database, so a restore never replays inference. Returns how many; a second run
    /// finds none and writes nothing.
    async fn after_restore(&self) -> Result<u64, ServiceError>;
}

#[async_trait]
pub trait Store: MemoryStore + SearchStore + RecallStore + GenerationStore + Send + Sync {
    async fn ping(&self) -> Result<(), ServiceError>;
    /// The scrape's database-wide gauges: one read-only query under a 2 s statement timeout,
    /// taking no row locks (06A A2-1).
    async fn metrics_gauges(&self) -> Result<super::metrics::Gauges, ServiceError>;
    async fn admin_credential(
        &self,
        id: Uuid,
    ) -> Result<Option<AdminCredentialRecord>, ServiceError>;
    async fn client_credential(
        &self,
        id: Uuid,
    ) -> Result<Option<ClientCredentialRecord>, ServiceError>;
    /// Writes the first operator credential; refused when an operator credential exists.
    async fn bootstrap_operator(
        &self,
        id: Uuid,
        label: &str,
        verifier: [u8; 32],
    ) -> Result<(), ServiceError>;
    async fn issue_admin_credential(
        &self,
        actor: Actor,
        id: Uuid,
        scope: AdminScope,
        label: &str,
        expires_at: Option<DateTime<Utc>>,
        verifier: [u8; 32],
    ) -> Result<(), ServiceError>;
    async fn revoke_admin_credential(
        &self,
        actor: Actor,
        id: Uuid,
    ) -> Result<AdminCredentialView, ServiceError>;
    async fn create_tenant(&self, actor: Actor, tenant: &str) -> Result<TenantView, ServiceError>;
    async fn change_tenant(
        &self,
        actor: Actor,
        tenant: &str,
        action: TenantAction,
    ) -> Result<TenantView, ServiceError>;
    async fn set_tenant_cap(
        &self,
        actor: Actor,
        tenant: &str,
        cap: u64,
    ) -> Result<TenantView, ServiceError>;
    async fn create_namespace(
        &self,
        actor: Actor,
        tenant: &str,
        namespace: &str,
    ) -> Result<NamespaceView, ServiceError>;
    async fn delete_namespace(
        &self,
        actor: Actor,
        tenant: &str,
        namespace: &str,
    ) -> Result<NamespaceView, ServiceError>;
    async fn issue_credential(
        &self,
        actor: Actor,
        tenant: &str,
        credential: NewCredential,
    ) -> Result<(), ServiceError>;
    async fn list_credentials(
        &self,
        actor: Actor,
        tenant: &str,
        page: CredentialPage<'_>,
    ) -> Result<CredentialList, ServiceError>;
    /// Operator only (checked by the caller): move up to `limit` tenants managed by `from` to the
    /// provisioner `to`, in tenant id order, any state (06A A1-5).
    async fn reassign_tenants(
        &self,
        actor: Actor,
        from: Uuid,
        to: Uuid,
        limit: u32,
    ) -> Result<TenantReassignResponse, ServiceError>;
    async fn revoke_credential(
        &self,
        actor: Actor,
        tenant: &str,
        id: Uuid,
    ) -> Result<CredentialView, ServiceError>;
}
