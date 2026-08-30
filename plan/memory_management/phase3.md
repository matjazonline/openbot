# Phase 3 — The provider port: ingest and forget

Read [`general_plan_instructions.md`](general_plan_instructions.md) first — in particular the
verified provider API tables and the `type: "memory"` decision.

**Goal:** `MemoryProvider` can put one document chunk into a collection and take a set of chunks back
out, on both adapters. Nothing calls it yet; phase 4 does.

**Landable on its own:** yes — new methods, new tests, no behaviour change to existing paths.

---

## 0. Groundwork already done

The HydraDB adapter was off the v2 contract and has been fixed ahead of this phase — see
[`general_plan_instructions.md`](general_plan_instructions.md) for the six changes and the verified
route table. Hindsight was checked against its 0.9.2 OpenAPI spec and needed nothing.

So the shapes below are not guesses; they come from the machine-readable specs. What no spec can
tell you is whether a deployment behaves like its documentation, and the pre-v2 bug survived a green
suite for exactly that reason: **every test asserted against a mock returning the shape the adapter
already expected.** Run the `#[ignore]`d live smoke tests against a real instance before calling
this phase done.

---

## 1. The port — `src/application/services/memory_provider.rs`

One bounded value object and two methods. **No default bodies**: `src/application/AGENTS.md` says
durable writes and state transitions have no default implementation, every production adapter and
mock states its behaviour.

```rust
/// One chunk of an uploaded document, bounded the way every other thing we send a provider is.
#[derive(Debug, Clone)]
pub struct MemoryDocumentChunk {
    /// `document_chunk_id(document_id, index)` — stable across retries, so an interrupted ingest
    /// upserts rather than duplicating.
    pub id: String,
    pub index: usize,
    pub total: usize,
    title: BoundedMemoryText,   // MAX_MEMORY_DOCUMENT_TITLE_CHARS
    text: BoundedMemoryText,    // MAX_MEMORY_DOCUMENT_CHUNK_CHARS
}

impl MemoryDocumentChunk {
    pub fn new(id: String, index: usize, total: usize, title: &str, text: &str) -> Self;
    pub fn title(&self) -> &str;
    pub fn text(&self) -> &str;
    pub fn was_truncated(&self) -> bool;
    /// "Reference document: {title} (part {index+1} of {total})" — the one place this framing
    /// sentence is written, so both adapters say the same thing.
    pub fn provenance(&self) -> String;
}

#[async_trait]
pub trait MemoryProvider: Send + Sync {
    // …existing five…

    async fn ingest_document_chunk(
        &self,
        database_id: &str,
        target: &MemoryPersistenceTarget,
        chunk: &MemoryDocumentChunk,
    ) -> Result<(), MemoryProviderError>;

    /// Remove specific items. The slice is deliberate: HydraDB takes an id array in one request,
    /// Hindsight needs one call per item, and the port should not force the cheaper provider to
    /// pay the expensive one's cost.
    async fn forget_items(
        &self,
        database_id: &str,
        collection: &str,
        item_ids: &[String],
    ) -> Result<(), MemoryProviderError>;
}
```

`BoundedMemoryText` already exists in this file (`:16`) — reuse it, do not hand-roll truncation.

**Five impls to update.** Find them before starting so the size is not a surprise:

```sh
grep -rn "impl MemoryProvider for" src/
```

`hydradb.rs:164`, `hindsight.rs:328`, and three test doubles: `memory_coordinator.rs:514`
(`CountingProvider`), `memory_worker.rs:609` (`CountingReadinessProvider`), `memory_worker.rs:661`
(`PausingProvider`). The doubles get real bodies — a counter and a recorded call list — not
`unimplemented!()`, because phase 4's tests drive them.

---

## 2. Hindsight — `src/adapters/memory/hindsight.rs`

Paths are relative to `HINDSIGHT_BASE_URL`, which already carries the `/v1/{namespace}` prefix
(`.env.example:87`, `docs/deploy.md:171`). Do not add it in code.

### `ingest_document_chunk`

Reuse `persist_scope`'s shape (`:200`); only the item fields differ:

```rust
let bank_id = Self::bank_id(database_id, &target.collection)?;
let item = json!({
    "document_id": chunk.id,          // upsert key
    "content":     chunk.text(),      // required
    "context":     chunk.provenance(),
    "tags":        [target.scope.label(), "document"],
    // Default is `replace`, but say it: a retry of a partly-applied chunk must reprocess from
    // scratch rather than concatenate onto what is already there.
    "update_mode": "replace",
});
let body = MemoryHttpClient::bounded_json(&json!({
    "items": [item], "async": true, "operation_id": chunk.id,
}))?;
```

Keep the **create-bank-on-404-and-retry-exactly-once** behaviour (`:230-236`): the company and agent
banks are only nameable once something is written to them, and a company's first upload may be the
first write of all. Never a loop.

Accept on `success == true && items_count == 1`, as `persist_scope` does.

### `forget_items`

`DELETE /banks/{bank_id}/documents/{document_id}` -> `{memory_units_deleted}`, deleting the document
*and every memory extracted from it*. One call per id, `join_all` with **at most
`MEMORY_DOCUMENT_CHUNKS_PER_LEASE` in flight** so 128 deletes do not open 128 connections. `404` is
success: the goal state is absent, and a partly-completed previous attempt is the normal way to
reach one. Other non-2xx go through `classify_status`; the first hard error aborts and the durable
job retries, by which time the already-deleted ids answer `404`.

### Worth knowing, not required

The spec says *"items sharing a `document_id` are grouped into the same document"*. One
`document_id` for a whole uploaded document would make forget a **single** call instead of one per
chunk — but then each chunk send would have to be `append` rather than `replace`, which loses
per-chunk idempotence. Recall also accepts `tags` + `tags_match`, so `"document"`-tagged chunks
could be filtered separately from conversation memory. Both are refinements for later; v1 keeps one
Hindsight document per chunk.

---

## 3. HydraDB — `src/adapters/memory/hydradb.rs`

Both new methods go through the `v2_data()` envelope helper and the existing `multipart_body`
helper, which already enforces `MAX_MEMORY_PROVIDER_REQUEST_BODY_BYTES` **before** allocating and
rejects a value containing the boundary.

### `ingest_document_chunk`

```rust
let memory = json!({
    "id":          chunk.id,      // upsert key; `upsert` defaults true. No comma — HydraDB reserves it.
    "title":       chunk.title(),
    "text":        format!("{}\n\n{}", chunk.provenance(), chunk.text()),
    "is_markdown": true,
    // Verbatim. A price list or a policy is the text, not an LLM's summary of it — and this is
    // what keeps recall's `type: "memory"` able to see it. See the note below.
    "infer":       false,
});
let memories = serde_json::to_string(&[memory])?;
let fields = vec![
    ("database",   database_id),
    ("collection", target.collection.as_str()),
    ("type",       "memory"),
    ("memories",   memories.as_str()),
];
```

`target.custom_instructions` is **not** sent — it is conversation-extraction guidance and `infer` is
off. Say so in a comment so nobody adds it back.

**Record the `memory`-vs-`knowledge` decision in a comment.** HydraDB calls a reference document
*knowledge* (`type=knowledge`, ingested as `app_knowledge` items whose primary text sits in
`fields.body`), and that is semantically where these belong. But `POST /query` defaults to
knowledge-only and our recall pins `type: "memory"`, so chunks ingested as knowledge would be
**invisible to the recall we have**. Moving them is a one-line recall change to `type: "all"`, which
merges and re-ranks both — worth doing once someone can A/B recall quality live.

Response is `202`; read `data.results[0]` and treat `queued` and `processing` as success. Only
`failed`, a non-null `error`, or `data.failed_count > 0` is a rejection.

### `forget_items`

One request for the whole slice — the reason the port takes a slice:

```rust
let body = MemoryHttpClient::bounded_json(&json!({
    "database":   database_id,
    "collection": collection,
    "type":       "memory",
    "ids":        item_ids,
}))?;
self.request(Method::DELETE, "/context").body(body)
```

Split `item_ids` if the body would exceed the request bound, and count a partial success as
progress. Read `data.results[]` (`{id, deleted, error}`) and `data.deleted_count`. Do **not** send
`X-HydraDB-Delete-Status: strict` — the default returns `200` with `deleted:false` for an id that is
already gone, and that is exactly the idempotence a retried job needs.

---

## 4. Rules both adapters follow

- `validate_identifier` on `database_id` **and** `collection` before building any body — the guard
  that keeps a crafted collection name from reaching another company's namespace.
- Error messages carry no bodies, no credentials, no ids. `MemoryProviderError` variants only.
- **No in-adapter retry.** Retry is durable, in the phase-4 worker plus the job table. An adapter
  that retries silently makes the attempt ledger lie.
- Every call goes through `self.http.measured(async { … })` so it lands in the runtime metric
  sample, and takes its timeout from `self.http.fast_timeout()`.
- Keep each file under the ~1,000-line trigger; tests already live in `*_tests.rs` siblings.

### Async completion — what "done" means here

HydraDB ingest returns `202` with per-item `queued|processing|completed|failed`, and Hindsight's
retain is `async: true`. **v1 treats *accepted* as done and does not poll** `/context/status` or
Hindsight's operations endpoint. That is why phase 5 words the terminal status **"In memory"** rather
than implying the index is warm.

Polling for true completion is a natural follow-up — `memory_provisioning_jobs` already has the
`next_poll_at` machinery — but it is per-provider work with a per-provider status endpoint, and it is
out of scope. Record that in a comment beside each accept check.

---

## 5. Tests — `hydradb_tests.rs`, `hindsight_tests.rs`

Use the shared harness in `src/adapters/memory/test_support.rs` (`mock_server`, `scripted_server`,
`uniform_server`, `raw_response_server`, `request_line`, `request_body`). Port the closest existing
persist tests rather than inventing a new style.

Per adapter:

- `ingesting_a_chunk_sends_its_stable_id_so_a_retry_upserts` — assert the exact id in the body, and
  that two calls with the same chunk send the same id.
- `the_ingest_body_names_the_right_collection_and_type` — HydraDB: `type=memory`, `infer:false`,
  `memories`; Hindsight: `document_id`, `tags`, `context`.
- `a_chunk_over_the_content_bound_is_truncated_and_reported`.
- `a_rejected_item_is_reported_without_echoing_the_response` — assert the error is
  `RejectedItem` and that the provider's message text appears nowhere in it.
- `a_non_2xx_classifies_the_way_the_shared_client_says` — 401 → `Authentication`, 429 →
  `RateLimited`, 5xx → `Unavailable`.
- `forget_never_touches_another_companys_namespace` — the important one. Give the mock two
  companies' ids and assert the request line and body name only ours.
- `forget_treats_a_missing_item_as_success` — HydraDB `deleted:false`; Hindsight `404`.
- `forget_of_many_ids_is_one_request_on_hydradb_and_one_per_id_on_hindsight` — pins the
  slice-shaped port actually paying off.
- Hindsight only: `ingest_creates_the_bank_on_404_and_retries_exactly_once` — and **not twice**.
- HydraDB only: `an_id_list_over_the_request_bound_is_split_rather_than_refused`.

Also extend `every_provider_kind_round_trips_through_its_wire_value`-style coverage if any new enum
arm appears, and make sure the three test doubles record enough for phase 4 to assert against.

---

## Done when

- `grep -rn "impl MemoryProvider for" src/` returns five, all with real bodies.
- `DATABASE_URL=… cargo test memory` green.
- `cargo fmt --check`, `cargo clippy` clean.
- A comment in `hydradb.rs` records the `memory`-vs-`knowledge` decision and the `type: "all"`
  follow-up; a comment in both adapters records that acceptance is not indexing.
- The `#[ignore]`d live smoke tests have actually been run against a real HydraDB and a real
  Hindsight, not just the mocks. A mock proves the parser matches the fixture; only a live call
  proves the fixture matches the provider, and that gap is what hid the pre-v2 bug.
- Every new assertion names the canonical v2 field (`database`, `collection`, `memories`) and
  asserts the absence of the pre-v2 spelling, the way
  `persist_sends_scope_instructions_and_accepts_empty_extraction` now does.
