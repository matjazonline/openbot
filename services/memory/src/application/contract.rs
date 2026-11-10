//! Wire types for every operation. The same types produce the OpenAPI components and the MCP
//! tool input schemas, so the two transports cannot drift (decisions D6).
use crate::domain::{
    access_level::{AccessLevel, Consumed},
    bounds::*,
    grant::Operation,
};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObservationKind {
    Fact,
    Decision,
    Discovery,
    Fix,
    Outcome,
    Note,
    Summary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceRef {
    /// An immutable recorded source revision.
    /// A source in the target namespace.
    Source {
        source_id: Uuid,
        /// At most 128 bytes.
        #[schemars(length(min = 1, max = MAX_REVISION_BYTES))]
        revision: String,
    },
    /// A parent observation at an exact revision (copies, derived summaries).
    Observation { observation_id: Uuid, revision: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExternalSource {
    /// At most 512 bytes.
    #[schemars(length(min = 1, max = MAX_REFERENCE_BYTES))]
    pub reference: String,
    pub access_level: AccessLevel,
}

/// Trusted host facts about the context that produced the memory (plan 01 access contract).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProducingContext {
    /// The producing context's trusted authorized ceiling. Missing → `classification_refused`.
    pub producing_ceiling: Option<AccessLevel>,
    pub consumed: Consumed,
    #[serde(default)]
    #[schemars(length(max = MAX_CONTEXT_SOURCES))]
    pub sources: Vec<SourceRef>,
    #[serde(default)]
    #[schemars(length(max = MAX_CONTEXT_SOURCES))]
    pub external_sources: Vec<ExternalSource>,
    pub expected_access_level: Option<AccessLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveItem {
    pub kind: ObservationKind,
    /// At most 256 characters and 1 KiB.
    #[schemars(length(min = 1, max = MAX_TITLE_CHARS))]
    pub title: String,
    /// At most 16,384 characters and 64 KiB.
    #[schemars(length(max = MAX_BODY_CHARS))]
    pub body: String,
    /// Truncated to microseconds, the stored precision.
    pub occurred_at: DateTime<Utc>,
    /// Truncated to microseconds, the stored precision.
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveRequest {
    /// 1–128 bytes of `[A-Za-z0-9._:-]`, stable across retries.
    #[schemars(regex(pattern = ID_PATTERN))]
    pub idempotency_key: String,
    /// At most 7 days ahead; a new execution after it is refused. Part of the request's
    /// identity: a retry must resend the original value unchanged, and a changed value is
    /// `idempotency_conflict`. Truncated to microseconds before it is hashed or stored.
    pub expires_at: DateTime<Utc>,
    #[schemars(regex(pattern = ID_PATTERN))]
    pub namespace: String,
    pub session_id: Option<Uuid>,
    pub context: ProducingContext,
    #[schemars(length(min = 1, max = MAX_SAVE_ITEMS))]
    pub items: Vec<SaveItem>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IndexState {
    Pending,
    Indexed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SavedObservation {
    pub observation_id: Uuid,
    pub revision: i64,
    pub access_level: AccessLevel,
    pub index_state: IndexState,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SaveResponse {
    pub observations: Vec<SavedObservation>,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    #[schemars(length(min = 1, max = MAX_NAMESPACES_PER_REQUEST), inner(regex(pattern = ID_PATTERN)))]
    pub namespaces: Vec<String>,
    pub read_ceiling: AccessLevel,
    /// At most 1,024 characters and at most 500 embedding-model tokens: a longer query (for
    /// example 1,024 CJK characters, about one token each) is refused with 400
    /// `invalid_request`, never truncated, so split it client-side. The keyword leg matches
    /// observations holding all of its first 32 words, or any of its identifier-like tokens
    /// (with a digit, `_ . @ / : -` inside, or an internal capital) as a phrase; a blank query
    /// matches nothing.
    #[schemars(length(max = MAX_QUERY_CHARS))]
    pub query: String,
    /// Default 20, maximum 100.
    #[schemars(range(min = 1, max = MAX_SEARCH_LIMIT))]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IndexItem {
    pub observation_id: Uuid,
    pub namespace: String,
    pub kind: ObservationKind,
    /// At most 256 characters and 1 KiB.
    pub title: String,
    /// At most 320 bytes. Search: the best-matching searchable part, else a keyword headline;
    /// timeline: the body's opening.
    pub snippet: String,
    pub occurred_at: DateTime<Utc>,
    pub access_level: AccessLevel,
    pub revision: i64,
    /// Provenance the reader may see (named, active namespace, within the ceiling, unexpired);
    /// anything else is omitted without a marker.
    pub sources: Vec<SourceRef>,
    /// `pending`/`failed` items are found by keyword only until semantic indexing succeeds.
    pub index_state: IndexState,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SemanticState {
    Complete,
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchResponse {
    /// Fused order: reciprocal rank over the keyword and semantic legs, then `occurred_at`
    /// descending, then id.
    pub items: Vec<IndexItem>,
    /// The highest level of everything returned, provenance included; 0 when empty.
    pub exposed_level: AccessLevel,
    /// `partial` when the exact semantic scan was needed but more than 100,000 eligible parts
    /// exist; the semantic leg is then approximate. Never silently short.
    pub semantic: SemanticState,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimelineRequest {
    /// The neighbourhood is drawn from these namespaces; name one to stay inside it.
    #[schemars(length(min = 1, max = MAX_NAMESPACES_PER_REQUEST), inner(regex(pattern = ID_PATTERN)))]
    pub namespaces: Vec<String>,
    pub read_ceiling: AccessLevel,
    /// An observation the reader may see; anything else (foreign, missing, deleted, expired or
    /// above the ceiling) is the same 404 `not_found`.
    pub anchor: Uuid,
    /// Neighbours before the anchor: 0–20, default 5.
    #[schemars(range(max = MAX_TIMELINE_SIDE))]
    pub before: Option<u32>,
    /// Neighbours after the anchor: 0–20, default 5.
    #[schemars(range(max = MAX_TIMELINE_SIDE))]
    pub after: Option<u32>,
    /// Only neighbours in the anchor's session; an anchor without a session then has none.
    #[serde(default)]
    pub session_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TimelineResponse {
    /// The anchor and its nearest visible neighbours, oldest first (`occurred_at`, then id).
    /// Records the reader may not see take no slot and leave no gap marker or count.
    pub items: Vec<IndexItem>,
    /// The highest level of everything returned, provenance included; at least the anchor's.
    pub exposed_level: AccessLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FetchRequest {
    #[schemars(length(min = 1, max = MAX_NAMESPACES_PER_REQUEST), inner(regex(pattern = ID_PATTERN)))]
    pub namespaces: Vec<String>,
    pub read_ceiling: AccessLevel,
    /// At most 64 ids.
    #[schemars(length(min = 1, max = MAX_FETCH_IDS))]
    pub ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ObservationDetail {
    pub observation_id: Uuid,
    pub namespace: String,
    pub kind: ObservationKind,
    pub title: String,
    pub body: String,
    pub occurred_at: DateTime<Utc>,
    pub access_level: AccessLevel,
    pub revision: i64,
    pub content_sha256: String,
    pub sources: Vec<SourceRef>,
    pub index_state: IndexState,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FetchResponse {
    pub items: Vec<ObservationDetail>,
    /// Ids that are foreign, nonexistent, deleted or above the ceiling: indistinguishable.
    pub unavailable: Vec<Uuid>,
    pub omitted_for_budget: Vec<Uuid>,
    pub exposed_level: AccessLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CorrectRequest {
    /// 1–128 bytes of `[A-Za-z0-9._:-]`, stable across retries.
    #[schemars(regex(pattern = ID_PATTERN))]
    pub idempotency_key: String,
    /// At most 7 days ahead; a new execution after it is refused. Part of the request's
    /// identity: a retry must resend the original value unchanged, and a changed value is
    /// `idempotency_conflict`. Truncated to microseconds before it is hashed or stored.
    pub expires_at: DateTime<Utc>,
    #[schemars(regex(pattern = ID_PATTERN))]
    pub namespace: String,
    pub read_ceiling: AccessLevel,
    pub observation_id: Uuid,
    #[schemars(range(min = 1))]
    pub expected_revision: i64,
    /// At most 256 characters and 1 KiB.
    #[schemars(length(min = 1, max = MAX_TITLE_CHARS))]
    pub title: String,
    /// At most 16,384 characters and 64 KiB.
    #[schemars(length(max = MAX_BODY_CHARS))]
    pub body: String,
    pub context: ProducingContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteRequest {
    /// 1–128 bytes of `[A-Za-z0-9._:-]`, stable across retries.
    #[schemars(regex(pattern = ID_PATTERN))]
    pub idempotency_key: String,
    /// At most 7 days ahead; a new execution after it is refused. Part of the request's
    /// identity: a retry must resend the original value unchanged, and a changed value is
    /// `idempotency_conflict`. Truncated to microseconds before it is hashed or stored.
    pub expires_at: DateTime<Utc>,
    #[schemars(regex(pattern = ID_PATTERN))]
    pub namespace: String,
    pub read_ceiling: AccessLevel,
    pub observation_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeleteResponse {
    pub observation_id: Uuid,
    pub deleted: bool,
    pub replayed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PackDetail {
    /// Each item's whole body.
    Full,
    /// Each item's snippet (at most 320 bytes).
    Compact,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextPackRequest {
    #[schemars(length(min = 1, max = MAX_NAMESPACES_PER_REQUEST), inner(regex(pattern = ID_PATTERN)))]
    pub namespaces: Vec<String>,
    pub read_ceiling: AccessLevel,
    /// Candidates are the hybrid search results for this query (the same bounds and refusals
    /// as search); absent or blank, the most recent observations. At most 200 either way.
    #[schemars(length(max = MAX_QUERY_CHARS))]
    pub query: Option<String>,
    /// 1–65,536; default 8,192. A pack counts 64 + 2 × the UTF-8 bytes of its items' compact
    /// JSON serializations as served (0 when empty). The bound holds for that encoding, with
    /// non-ASCII text unescaped; a client that re-encodes the items (for example escaping
    /// non-ASCII) can exceed it.
    #[schemars(range(min = 1, max = MAX_CONTEXT_TOKENS))]
    pub max_tokens: Option<u32>,
    /// 1–262,144; default 65,536. The exact UTF-8 bytes of the items' compact JSON
    /// serializations as served, summed: the `items` array's brackets and commas (n + 1 bytes)
    /// and the pack's other fields are not counted.
    #[schemars(range(min = 1, max = MAX_CONTEXT_BYTES))]
    pub max_bytes: Option<u32>,
    /// `compact` (default) packs snippets, `full` whole bodies; never partial bodies.
    pub detail: Option<PackDetail>,
}

/// One memory as untrusted data: its text is stored content, never an instruction.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PackItem {
    pub observation_id: Uuid,
    pub namespace: String,
    pub kind: ObservationKind,
    pub title: String,
    /// The body (`full`) or the snippet (`compact`).
    pub text: String,
    pub occurred_at: DateTime<Utc>,
    pub access_level: AccessLevel,
    pub revision: i64,
    /// SHA-256 over the title, NUL and the whole body, as in fetch, whatever `detail` is.
    pub content_sha256: String,
    /// Provenance the reader may see; anything else is omitted without a marker.
    pub sources: Vec<SourceRef>,
    /// Lowercase hex SHA-256 of the compact JSON of `sources` as served; send it back to
    /// `context.validate`, which reports `changed` when the visible provenance differs.
    pub sources_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Omitted {
    /// Visible candidates skipped because they did not fit the remaining budget. A count only:
    /// no ids or levels, and records the reader may not see are never candidates.
    pub budget: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextPack {
    /// Included items in candidate order: fused score (or recency), `occurred_at` descending,
    /// then id. Each is whole; one that does not fit is skipped and the scan continues.
    pub items: Vec<PackItem>,
    /// The highest level of the included items and their provenance; 0 when empty. Never the
    /// requested ceiling, and never raised by skipped candidates. Combine it with the
    /// consumer's prior watermark before exposing the pack.
    pub exposed_level: AccessLevel,
    /// `64 + 2 × byte_count`, or 0 for an empty pack.
    pub token_count: u32,
    /// The sum of the items' compact JSON sizes as served; excludes the array's n + 1
    /// bracket/comma bytes and the envelope fields.
    pub byte_count: u32,
    pub omitted: Omitted,
    /// The 200-candidate window was full: more visible candidates were ranked or exist.
    pub candidates_truncated: bool,
    /// Lowercase hex SHA-256 of the compact JSON serialization of `items`.
    pub pack_digest: String,
    /// The earliest expiry of the included items and of their visible provenance targets.
    pub valid_until: Option<DateTime<Utc>>,
    pub generated_at: DateTime<Utc>,
    /// The search's semantic state for a query pack; `complete` for a recency pack.
    pub semantic: SemanticState,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackedRevision {
    pub observation_id: Uuid,
    #[schemars(range(min = 1))]
    pub revision: i64,
    /// The frozen item's `sources_sha256`.
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub sources_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextValidateRequest {
    #[schemars(length(min = 1, max = MAX_NAMESPACES_PER_REQUEST), inner(regex(pattern = ID_PATTERN)))]
    pub namespaces: Vec<String>,
    pub read_ceiling: AccessLevel,
    /// At most 200 frozen pack items, each answered in request order.
    #[schemars(length(min = 1, max = MAX_CONTEXT_ITEMS))]
    pub items: Vec<PackedRevision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PackItemStatus {
    /// Still visible under this request's grants and ceiling, at the same revision and with
    /// the same visible provenance.
    Valid,
    /// Visible, but corrected since (the frozen text is stale) or its visible provenance
    /// differs (a link's target expired, was deleted, or left the ceiling or namespaces).
    Changed,
    /// Visible apart from its own expiry, until the retention sweeper removes it.
    Expired,
    /// Deleted, purged, foreign, in an unnamed or deleted namespace, or above the ceiling:
    /// indistinguishable.
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ValidatedItem {
    pub observation_id: Uuid,
    pub status: PackItemStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextValidateResponse {
    /// One status per requested item, in request order; no content.
    pub items: Vec<ValidatedItem>,
    /// The highest current level among the items reported `valid`, `changed` or `expired`.
    pub exposed_level: AccessLevel,
    /// The snapshot the statuses describe. A status cannot retract text already sent to a
    /// model: clients own their retained packs and model requests.
    pub checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    /// At most 512 bytes; stored as given, so a secret-shaped reference (`?token=…`,
    /// `user:password@` URLs, keyword-shaped ids such as `secret:42`) is refused with 400
    /// `secret_in_identifier`.
    #[schemars(length(min = 1, max = MAX_REFERENCE_BYTES))]
    pub external_ref: String,
    /// At most 128 bytes; secret-shaped revisions are refused like `external_ref`.
    #[schemars(length(min = 1, max = MAX_REVISION_BYTES))]
    pub revision: String,
    pub access_level: AccessLevel,
    /// Truncated to microseconds, the stored precision; a re-record compares the stored value.
    pub occurred_at: DateTime<Utc>,
    /// At most 256 KiB.
    #[schemars(length(max = MAX_SOURCE_CONTENT_CHARS))]
    pub content: String,
    /// Truncated to microseconds, the stored precision; a re-record compares the stored value.
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourcesRecordRequest {
    /// 1–128 bytes of `[A-Za-z0-9._:-]`, stable across retries.
    #[schemars(regex(pattern = ID_PATTERN))]
    pub idempotency_key: String,
    /// At most 7 days ahead; a new execution after it is refused. Part of the request's
    /// identity: a retry must resend the original value unchanged, and a changed value is
    /// `idempotency_conflict`. Truncated to microseconds before it is hashed or stored.
    pub expires_at: DateTime<Utc>,
    #[schemars(regex(pattern = ID_PATTERN))]
    pub namespace: String,
    pub session_id: Option<Uuid>,
    #[schemars(length(min = 1, max = MAX_WINDOW_EVENTS))]
    pub sources: Vec<SourceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecordedSource {
    pub source_id: Uuid,
    pub external_ref: String,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourcesRecordResponse {
    pub sources: Vec<RecordedSource>,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionOpenRequest {
    /// 1–128 bytes of `[A-Za-z0-9._:-]`, stable across retries.
    #[schemars(regex(pattern = ID_PATTERN))]
    pub idempotency_key: String,
    /// At most 7 days ahead; a new execution after it is refused. Part of the request's
    /// identity: a retry must resend the original value unchanged, and a changed value is
    /// `idempotency_conflict`. Truncated to microseconds before it is hashed or stored.
    pub expires_at: DateTime<Utc>,
    #[schemars(regex(pattern = ID_PATTERN))]
    pub namespace: String,
    #[schemars(regex(pattern = ID_PATTERN))]
    pub external_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionCloseRequest {
    /// 1–128 bytes of `[A-Za-z0-9._:-]`, stable across retries.
    #[schemars(regex(pattern = ID_PATTERN))]
    pub idempotency_key: String,
    /// At most 7 days ahead; a new execution after it is refused. Part of the request's
    /// identity: a retry must resend the original value unchanged, and a changed value is
    /// `idempotency_conflict`. Truncated to microseconds before it is hashed or stored.
    pub expires_at: DateTime<Utc>,
    #[schemars(regex(pattern = ID_PATTERN))]
    pub namespace: String,
    pub session_id: Uuid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Open,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionView {
    pub session_id: Uuid,
    pub external_id: String,
    pub state: SessionState,
    pub replayed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EventRole {
    User,
    Assistant,
    Tool,
    Note,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActivityEvent {
    /// At most 128 bytes; sent to the provider as given, so one that holds a secret or a
    /// `<private>` span is refused with 400 `secret_in_identifier` (as are such `source_ref`s
    /// and revisions). Keyword-shaped ids such as `secret:42`, `token:7` or `api-key:prod`
    /// look like `key: value` secrets and are refused too.
    #[schemars(length(min = 1, max = MAX_REVISION_BYTES))]
    pub event_id: String,
    /// At most 512 bytes; stored as given, so a secret-shaped reference (`?token=…`,
    /// `user:password@` URLs, keyword-shaped ids) is refused with 400 `secret_in_identifier`.
    #[schemars(length(min = 1, max = MAX_REFERENCE_BYTES))]
    pub source_ref: String,
    /// At most 128 bytes; secret-shaped revisions are refused like `source_ref`.
    #[schemars(length(min = 1, max = MAX_REVISION_BYTES))]
    pub revision: String,
    /// Events are in non-decreasing `occurred_at` order.
    pub occurred_at: DateTime<Utc>,
    /// The event's trusted classification; at most the producing ceiling.
    pub access_level: AccessLevel,
    pub role: EventRole,
    /// The window's texts total at most 512 KiB. `<private>…</private>` spans are removed and
    /// recognised secrets replaced with `[REDACTED:<kind>]` before anything is stored or sent.
    pub text: String,
    /// Excluded events are dropped before capture and never sent.
    #[serde(default)]
    pub exclude: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationRequest {
    /// 1–128 bytes of `[A-Za-z0-9._:-]`, stable across retries.
    #[schemars(regex(pattern = ID_PATTERN))]
    pub idempotency_key: String,
    /// At most 7 days ahead; a new execution after it is refused. Part of the request's
    /// identity: a retry must resend the original value unchanged, and a changed value is
    /// `idempotency_conflict`. Truncated to microseconds before it is hashed or stored.
    pub expires_at: DateTime<Utc>,
    #[schemars(regex(pattern = ID_PATTERN))]
    pub namespace: String,
    /// Required for a session summary, which may only draw on this session's window.
    pub session_id: Option<Uuid>,
    /// The producing context's trusted authorized read ceiling. Missing →
    /// `classification_refused`; an event above it refuses the request.
    pub producing_ceiling: Option<AccessLevel>,
    /// The prior consumed watermark: exact, or unknown (the ceiling is then used).
    pub consumed: Consumed,
    /// Untrusted descriptive context for the window (at most 1 KiB), redacted and sent as
    /// data. It carries no level of its own: it is covered by the `consumed` attestation, so
    /// it must not hold anything above that watermark.
    #[schemars(length(max = 1024))]
    pub description: Option<String>,
    /// The complete bounded window: at most 512 events and 512 KiB.
    #[schemars(length(min = 1, max = MAX_WINDOW_EVENTS))]
    pub events: Vec<ActivityEvent>,
    #[schemars(range(min = 1))]
    pub allowance_tokens: u64,
    /// At most 8,192: the provider's `max_tokens`. Thinking is turned off for this call, so
    /// the whole bound is available to the answer.
    #[schemars(range(min = 1, max = MAX_OUTPUT_TOKENS))]
    pub max_output_tokens: u32,
    #[serde(default)]
    pub summary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationRef {
    #[schemars(regex(pattern = ID_PATTERN))]
    pub namespace: String,
    pub operation_id: Uuid,
    /// At most the grant's `read_max`; an operation whose input level is above it is
    /// `not_found`, like a foreign or missing one.
    pub read_ceiling: AccessLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationCancelRequest {
    /// 1–128 bytes of `[A-Za-z0-9._:-]`, stable across retries.
    #[schemars(regex(pattern = ID_PATTERN))]
    pub idempotency_key: String,
    /// At most 7 days ahead; a new execution after it is refused. Part of the request's
    /// identity: a retry must resend the original value unchanged, and a changed value is
    /// `idempotency_conflict`. Truncated to microseconds before it is hashed or stored.
    pub expires_at: DateTime<Utc>,
    #[schemars(regex(pattern = ID_PATTERN))]
    pub namespace: String,
    pub operation_id: Uuid,
}

/// `accepted` (queued), `claimed`/`dispatched` (running; `dispatched` means the provider call
/// was committed to before it was sent), then exactly one terminal state. Nothing ever leaves a
/// terminal state: recovery is a new request with new authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GenerationState {
    Accepted,
    Claimed,
    Dispatched,
    /// `outcome` is `saved`, or a skip: `nothing_to_save` (the provider found nothing worth
    /// keeping) or `nothing_to_process` (privacy left nothing to send; no call was made).
    Completed,
    /// `outcome` is a bounded failure code (for example `malformed_output`,
    /// `output_truncated`, `provider_refused`, `spend_cap_reached`, `source_unavailable` or a
    /// provider error type such as `invalid_request_error`), never provider text. A rejection
    /// of the service's own provider credentials does not fail an operation: it waits in
    /// `accepted` until the operator fixes them or its `expires_at` passes.
    Failed,
    /// `outcome` names who cancelled (`client_cancelled`, `tenant_disabled`,
    /// `credential_revoked`, …), or `restored` when an operator restored the service's database
    /// from a backup and the operation was unfinished.
    Cancelled,
    /// The request's `expires_at` passed before it could run or be saved.
    Expired,
    /// The provider call may have run (timeout, lost lease, crash after the send): its cost is
    /// charged, any output was not saved, and it is never re-sent.
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GenerationStatus {
    pub operation_id: Uuid,
    pub state: GenerationState,
    pub outcome: Option<String>,
    /// The generated observations still visible under the request's ceiling, in output order.
    pub observations: Vec<Uuid>,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PurgeRequest {
    /// 1–128 bytes of `[A-Za-z0-9._:-]`, stable across retries.
    #[schemars(regex(pattern = ID_PATTERN))]
    pub idempotency_key: String,
    /// At most 7 days ahead; a new execution after it is refused. Part of the request's
    /// identity: a retry must resend the original value unchanged, and a changed value is
    /// `idempotency_conflict`. Truncated to microseconds before it is hashed or stored.
    pub expires_at: DateTime<Utc>,
    #[schemars(regex(pattern = ID_PATTERN))]
    pub namespace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PurgeResponse {
    pub namespace: String,
    pub purged: bool,
    pub replayed: bool,
}

/// No request fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrantSpec {
    #[schemars(regex(pattern = ID_PATTERN))]
    pub namespace: String,
    /// Required when `operations` holds a read operation.
    pub read_max: Option<AccessLevel>,
    /// Required when `operations` holds a write operation (including delete and purge).
    pub write_max: Option<AccessLevel>,
    #[schemars(length(min = 1, max = 12))]
    pub operations: Vec<Operation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CredentialView {
    pub tenant: String,
    pub credential_id: Uuid,
    pub label: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    /// Only active namespaces are listed.
    pub grants: Vec<GrantSpec>,
}

// ---- Administration (admin credentials only; HTTP only) ----

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TenantCreateRequest {
    #[schemars(regex(pattern = ID_PATTERN))]
    pub external_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TenantRef {
    #[schemars(regex(pattern = ID_PATTERN))]
    pub tenant: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TenantSettingsRequest {
    #[schemars(regex(pattern = ID_PATTERN))]
    pub tenant: String,
    /// Tokens per UTC day; 0 disables generation.
    pub generation_daily_token_cap: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TenantState {
    Active,
    Disabled,
    Deleted,
}

pub use crate::domain::admin_policy::AdminScope;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TenantView {
    pub tenant: String,
    pub state: TenantState,
    pub generation_daily_token_cap: u64,
    pub disabled_by_scope: Option<AdminScope>,
    /// Operations cancelled by this state change.
    pub cancelled_operations: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NamespaceRequest {
    #[schemars(regex(pattern = ID_PATTERN))]
    pub tenant: String,
    #[schemars(regex(pattern = ID_PATTERN))]
    pub namespace: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NamespaceState {
    Active,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NamespaceView {
    pub tenant: String,
    pub namespace: String,
    pub state: NamespaceState,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CredentialIssueRequest {
    #[schemars(regex(pattern = ID_PATTERN))]
    pub tenant: String,
    #[schemars(length(min = 1, max = MAX_LABEL_CHARS))]
    pub label: String,
    pub expires_at: Option<DateTime<Utc>>,
    #[schemars(length(min = 1, max = MAX_GRANTS_PER_CREDENTIAL))]
    pub grants: Vec<GrantSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IssuedCredential {
    pub credential_id: Uuid,
    /// Shown once; the service stores only its verifier.
    pub token: String,
    pub expires_at: Option<DateTime<Utc>>,
}

/// A page of a tenant's client credentials in id order (06A A1-6).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CredentialListRequest {
    #[schemars(regex(pattern = ID_PATTERN))]
    pub tenant: String,
    /// Only credentials with exactly this label.
    #[serde(default)]
    pub label: Option<String>,
    /// Only credentials that are neither revoked nor expired.
    #[serde(default)]
    pub active_only: bool,
    /// Continue after this credential id: the previous page's `next_after`.
    #[serde(default)]
    pub after: Option<Uuid>,
    /// Page size, 1–1000 (default 1000).
    #[serde(default)]
    #[schemars(range(min = 1, max = 1000))]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CredentialList {
    pub credentials: Vec<CredentialView>,
    /// The last credential id of a full page, to pass as `after`; absent when the page was not
    /// full, so nothing follows it.
    pub next_after: Option<Uuid>,
}

/// Move tenants from one admin credential's management to a provisioner's (06A A1-5), for
/// provisioner rotation. Operator only; repeat until `remaining` is false.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TenantReassignRequest {
    /// The admin credential that manages the tenants now (it may be revoked).
    pub from: Uuid,
    /// An active, unexpired provisioner credential, different from `from`.
    pub to: Uuid,
    /// Tenants to move in this call, 1–500 (default 500), in tenant id order.
    #[serde(default)]
    #[schemars(range(min = 1, max = 500))]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TenantReassignResponse {
    pub moved: u32,
    /// Whether `from` still manages tenants after this call.
    pub remaining: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CredentialRevokeRequest {
    #[schemars(regex(pattern = ID_PATTERN))]
    pub tenant: String,
    pub credential_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AdminCredentialIssueRequest {
    pub scope: AdminScope,
    #[schemars(length(min = 1, max = MAX_LABEL_CHARS))]
    pub label: String,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AdminCredentialRevokeRequest {
    pub credential_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AdminCredentialView {
    pub credential_id: Uuid,
    pub scope: AdminScope,
    pub revoked: bool,
}
