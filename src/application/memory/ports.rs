use async_trait::async_trait;
use secrecy::SecretString;
use uuid::Uuid;

/// An opaque service identifier (`^[A-Za-z0-9._:-]{1,128}$`): tenant external ids are company
/// UUIDs and namespace external ids are random UUIDs, so a UUID's hyphenated form always fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TenantExternalId(pub Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NamespaceExternalId(pub Uuid);

/// Every failure the service can answer, classified once at the adapter boundary (E1).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MemoryServiceError {
    #[error("memory service refused the credential")]
    Unauthorized,
    #[error("memory service forbade the request ({code})")]
    Forbidden { code: String },
    #[error("memory service resource not found")]
    NotFound,
    #[error("memory service conflict ({code})")]
    Conflict { code: String },
    #[error("memory service request expired")]
    Expired,
    #[error("memory service refused the classification ({reason})")]
    Refused { reason: String },
    #[error("memory service is at capacity")]
    Capacity { retry_after_secs: Option<u64> },
    /// Nothing was acted on: a connect failure, the service's own `unavailable` 503, or no permit.
    #[error("memory service unavailable")]
    Unavailable,
    /// The request may have been acted on (sent, then no definite answer). A write is retried only
    /// with the same idempotency key; a non-idempotent issue leaves a stray to reconcile.
    #[error("memory service outcome unknown")]
    Ambiguous,
    /// A response that does not match the published contract.
    #[error("memory service answered outside its contract")]
    Protocol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteTenantState {
    Active,
    Disabled,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTenant {
    pub state: RemoteTenantState,
    pub generation_daily_token_cap: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteNamespaceState {
    Active,
    Deleted,
}

/// The operations a per-namespace client credential is granted (E2).
pub const CLIENT_OPERATIONS: &[&str] = &[
    "save", "search", "timeline", "fetch", "context", "sources", "generate",
];

/// A freshly issued client credential. The token is shown once; only its envelope is stored.
pub struct IssuedClientCredential {
    pub credential_id: Uuid,
    pub token: SecretString,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCredential {
    pub credential_id: Uuid,
    pub label: String,
    pub revoked: bool,
}

/// One `credentials/list` page request (p06 A1-6, A3-6): an exact label, only active credentials,
/// and an id cursor.
#[derive(Debug, Clone, Copy)]
pub struct CredentialQuery<'a> {
    pub label: Option<&'a str>,
    pub active_only: bool,
    pub after: Option<Uuid>,
    pub limit: u32,
}

/// One page in credential id order; `next_after` is set when the page was full.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialPage {
    pub credentials: Vec<RemoteCredential>,
    pub next_after: Option<Uuid>,
}

/// Admin operations through OpenBot's one provisioner credential (D3, E2). Every method is a
/// required, explicit decision: no defaults, because each one changes remote authority.
#[async_trait]
pub trait MemoryProvisioner: Send + Sync {
    /// Idempotent by external id; a tombstoned or foreign-managed id is `Conflict`.
    async fn create_tenant(
        &self,
        tenant: TenantExternalId,
    ) -> Result<RemoteTenant, MemoryServiceError>;
    async fn disable_tenant(
        &self,
        tenant: TenantExternalId,
    ) -> Result<RemoteTenant, MemoryServiceError>;
    async fn enable_tenant(
        &self,
        tenant: TenantExternalId,
    ) -> Result<RemoteTenant, MemoryServiceError>;
    /// 200 also for this provisioner's own tombstone; 404 when the tenant is not ours.
    async fn delete_tenant(
        &self,
        tenant: TenantExternalId,
    ) -> Result<RemoteTenant, MemoryServiceError>;
    async fn create_namespace(
        &self,
        tenant: TenantExternalId,
        namespace: NamespaceExternalId,
    ) -> Result<RemoteNamespaceState, MemoryServiceError>;
    async fn delete_namespace(
        &self,
        tenant: TenantExternalId,
        namespace: NamespaceExternalId,
    ) -> Result<RemoteNamespaceState, MemoryServiceError>;
    /// Not idempotent: an ambiguous issue leaves a stray credential labelled `label`.
    async fn issue_client_credential(
        &self,
        tenant: TenantExternalId,
        namespace: NamespaceExternalId,
        label: &str,
    ) -> Result<IssuedClientCredential, MemoryServiceError>;
    /// One page of the tenant's client credentials. Answers for a tenant in any state the
    /// provisioner manages, so it also proves management of a tombstone (p06 A3 F5).
    async fn list_client_credentials(
        &self,
        tenant: TenantExternalId,
        query: CredentialQuery<'_>,
    ) -> Result<CredentialPage, MemoryServiceError>;
    async fn revoke_client_credential(
        &self,
        tenant: TenantExternalId,
        credential: Uuid,
    ) -> Result<(), MemoryServiceError>;
}

/// The label OpenBot gives a namespace's client credential, so strays can be found (E2).
pub fn client_credential_label(namespace: NamespaceExternalId) -> String {
    format!("ob:{}", namespace.0.hyphenated())
}

/// Whether a company has memory enabled now (E4 gating): publication refuses memory steps and
/// tools otherwise. A current read, never a cached one.
#[async_trait]
pub trait MemoryPublicationGate: Send + Sync {
    async fn memory_enabled(
        &self,
        company: crate::domain::workflow::CompanyId,
    ) -> crate::app_error::AppResult<bool>;
}

/// A namespace's client credential, opened for one request and never logged (E1, E2).
pub struct ClientCredential {
    pub tenant: TenantExternalId,
    /// The service's id for it: a `401` re-verifies the credential by id (p06 A4-9).
    pub id: Uuid,
    pub token: SecretString,
}

/// `Consumed` (D4): precise accounting, or the producing context's ceiling as the watermark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConsumedEvidence {
    Exact { level: u8 },
    Unknown,
}

/// Provenance only: a recalled observation from another namespace (E2).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalSourceRef {
    pub reference: String,
    pub access_level: u8,
}

/// `ProducingContext`: trusted host facts, never model-selectable (E3, E4).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProducingContext {
    pub consumed: ConsumedEvidence,
    pub producing_ceiling: u8,
    pub external_sources: Vec<ExternalSourceRef>,
}

/// `SaveItem` with kind `fact` (E4): title is the first line cut to 256 characters.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemorySaveItem {
    pub kind: String,
    pub title: String,
    pub body: String,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

/// `SaveRequest`, frozen into the delivery intent and resent byte-for-byte on every retry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemorySaveRequest {
    pub idempotency_key: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub namespace: String,
    pub context: ProducingContext,
    pub items: Vec<MemorySaveItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SavedMemory {
    pub observation_id: Uuid,
    pub revision: i64,
    pub access_level: u8,
}

/// The service's answer to a save; `replayed` when the key had already been executed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MemorySaveReceipt {
    pub observations: Vec<SavedMemory>,
    pub replayed: bool,
}

/// `EventRole`: capture sends only the exchange's logical input and accepted output (E8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEventRole {
    User,
    Assistant,
}

/// `ActivityEvent`: one turn of a captured window, at its trusted classification.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryActivityEvent {
    pub event_id: String,
    pub source_ref: String,
    pub revision: String,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub access_level: u8,
    pub role: MemoryEventRole,
    pub text: String,
}

/// `GenerationRequest`, frozen into the capture intent with its policy's allowance and expiry
/// and resent byte-for-byte on every retry: a retry is a replay, never a new request (E8). No
/// session id and no summary in the first release.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryGenerationRequest {
    pub idempotency_key: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub namespace: String,
    pub producing_ceiling: u8,
    pub consumed: ConsumedEvidence,
    pub events: Vec<MemoryActivityEvent>,
    pub allowance_tokens: u64,
    pub max_output_tokens: u32,
}

/// The service's answer to a generation request: the accepted operation (`GenerationStatus`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MemoryGenerationReceipt {
    pub operation_id: Uuid,
    pub state: String,
    pub replayed: bool,
}

/// `SearchRequest` for one namespace: the read ceiling is host-derived, never model-chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySearch {
    pub namespace: NamespaceExternalId,
    pub read_ceiling: u8,
    pub query: String,
    pub limit: u32,
}

/// `ContextPackRequest` (compact detail) within one namespace, for automatic recall (E7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryPackRequest {
    pub namespace: NamespaceExternalId,
    pub read_ceiling: u8,
    /// Candidates are search results for it; absent, the most recent observations.
    pub query: Option<String>,
    pub max_tokens: u32,
    pub max_bytes: u32,
}

/// One pack of `MemoryClient::context_pack`, through its namespace's own credential.
pub struct ScopedPack<'a> {
    pub credential: &'a ClientCredential,
    pub pack: MemoryPackRequest,
}

/// `PackItem`: one memory as untrusted data, with what `context.validate` checks later.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryPackItem {
    pub observation_id: Uuid,
    pub kind: String,
    pub title: String,
    pub text: String,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub access_level: u8,
    pub revision: i64,
    pub content_sha256: String,
    pub sources_sha256: String,
}

/// `ContextPack`, within the ceiling by construction of the service (checked by the adapter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryPack {
    pub items: Vec<MemoryPackItem>,
    pub exposed_level: u8,
    pub pack_digest: String,
    pub token_count: u32,
    pub valid_until: Option<chrono::DateTime<chrono::Utc>>,
    /// Candidates the service left out for the budget (a count only, p06 A2-4).
    pub omitted: u32,
}

/// `PackedRevision`: a frozen item sent back to `context.validate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedRevision {
    pub observation_id: Uuid,
    pub revision: i64,
    pub sources_sha256: String,
}

/// `PackItemStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackItemStatus {
    Valid,
    Changed,
    Expired,
    Unavailable,
}

/// One search of `MemoryClient::search_scopes`, through its namespace's own credential.
pub struct ScopedSearch<'a> {
    pub credential: &'a ClientCredential,
    pub search: MemorySearch,
}

/// `IndexItem`: a compact hit, within the ceiling by construction of the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryHit {
    pub observation_id: Uuid,
    pub kind: String,
    pub title: String,
    pub snippet: String,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub access_level: u8,
    pub revision: i64,
}

/// `TimelineRequest` within one namespace: the anchor and its nearest visible neighbours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryTimeline {
    pub namespace: NamespaceExternalId,
    pub read_ceiling: u8,
    pub anchor: Uuid,
    pub before: u32,
    pub after: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySearchResult {
    pub items: Vec<MemoryHit>,
    pub exposed_level: u8,
}

/// `ObservationDetail`: a whole body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryDetail {
    pub observation_id: Uuid,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub access_level: u8,
    pub revision: i64,
    pub content_sha256: String,
}

/// `FetchResponse`; `exposed_level` covers everything returned, provenance included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryFetchResult {
    pub items: Vec<MemoryDetail>,
    pub exposed_level: u8,
}

/// Client operations through a namespace's own credential. Every method is required: each one
/// either discloses classified content or makes a durable write.
#[async_trait]
pub trait MemoryClient: Send + Sync {
    /// One atomic save on the background delivery pool; a retry resends `request` unchanged.
    async fn save(
        &self,
        credential: &ClientCredential,
        request: &MemorySaveRequest,
    ) -> Result<MemorySaveReceipt, MemoryServiceError>;
    /// One generation request on the background delivery pool (E8); a retry resends `request`
    /// unchanged under the same key.
    async fn request_generation(
        &self,
        credential: &ClientCredential,
        request: &MemoryGenerationRequest,
    ) -> Result<MemoryGenerationReceipt, MemoryServiceError>;
    /// An interactive search under `search.read_ceiling`.
    async fn search(
        &self,
        credential: &ClientCredential,
        search: &MemorySearch,
    ) -> Result<MemorySearchResult, MemoryServiceError>;
    /// Searches of one company's namespaces as one interactive operation (E1): one permit for
    /// all of them, sent concurrently, answers in request order. Any failure fails the whole
    /// operation; searches of several companies are a contract fault.
    async fn search_scopes(
        &self,
        searches: &[ScopedSearch<'_>],
    ) -> Result<Vec<MemorySearchResult>, MemoryServiceError>;
    /// Compact packs of one company's namespaces for automatic recall as one interactive
    /// operation (E1, E7): one permit, concurrent requests, all within the recall deadline,
    /// answers in request order. Any failure fails the whole recall.
    async fn context_pack(
        &self,
        packs: &[ScopedPack<'_>],
    ) -> Result<Vec<MemoryPack>, MemoryServiceError>;
    /// The current status of frozen pack items in one namespace, in request order.
    async fn context_validate(
        &self,
        credential: &ClientCredential,
        namespace: NamespaceExternalId,
        read_ceiling: u8,
        items: &[PackedRevision],
    ) -> Result<Vec<PackItemStatus>, MemoryServiceError>;
    /// The anchor and its nearest visible neighbours, oldest first. An anchor the reader may
    /// not see is `NotFound`, the same as a missing one.
    async fn timeline(
        &self,
        credential: &ClientCredential,
        timeline: &MemoryTimeline,
    ) -> Result<MemorySearchResult, MemoryServiceError>;
    /// Whole bodies of at most 64 ids under the same ceiling; unknown or hidden ids are absent.
    async fn fetch(
        &self,
        credential: &ClientCredential,
        namespace: NamespaceExternalId,
        read_ceiling: u8,
        ids: &[Uuid],
    ) -> Result<MemoryFetchResult, MemoryServiceError>;
}

tokio::task_local! {
    /// The `x-correlation-id` of the memory-service requests made in this scope (p06 A2-2).
    static CORRELATION: String;
}

/// Run `call` with `id` as the correlation of every memory-service request it makes (on this
/// task: concurrent requests joined within it see it too). Deliveries use their idempotency key;
/// recall and the memory tools use [`read_correlation`]. Opaque and non-secret, never authority.
pub async fn with_correlation<F: std::future::Future>(id: String, call: F) -> F::Output {
    CORRELATION.scope(id, call).await
}

/// The correlation of the current scope, if any.
pub fn current_correlation() -> Option<String> {
    CORRELATION.try_with(Clone::clone).ok()
}

/// `ob.r.<run>.<execution>`: the reads one execution makes (recall, memory tools).
pub fn read_correlation(run: Uuid, execution: Uuid) -> String {
    format!("ob.r.{}.{}", run.simple(), execution.simple())
}
