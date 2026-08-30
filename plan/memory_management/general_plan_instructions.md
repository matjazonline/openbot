# Company- and agent-scoped documents in long-term memory — shared instructions

Read this before any phase file. Each phase lives in `plan/memory_management/phase{n}.md` and
assumes everything here.

| Phase | File | Goal |
|---|---|---|
| 1 | [`phase1.md`](phase1.md) | Domain types, bounds, chunking, text extraction — pure, no DB, no network |
| 2 | [`phase2.md`](phase2.md) | Schema, persistence adapter, use cases — upload stores locally |
| 3 | [`phase3.md`](phase3.md) | `MemoryProvider` gains ingest + forget; both adapters |
| 4 | [`phase4.md`](phase4.md) | Durable worker loop and the three lifecycle hooks |
| 5 | [`phase5.md`](phase5.md) | Company and agent settings UI |

Each phase compiles, tests green, and is landable on its own. Phases 1–2 are useful without 3–5
(documents are stored and listed, status stays `pending`).

---

## Context

Long-term memory only ever learns from conversations. `MemoryCoordinator::persist`
(`src/application/services/memory_coordinator.rs:206`) writes one user/assistant pair per completed
run into the provider; `recall` (:81) reads it back. There is no way to give an agent a policy PDF,
a price list, or an onboarding doc — the only route is pasting it into a system prompt, which costs
prompt budget on every message and cannot be scoped or managed.

Everything needed already exists and is unused for this purpose:

- `MemoryScope::{Company, Agent(Uuid), User}` with per-scope collections `"company"` /
  `"agent_{uuid}"` — `src/domain/entities/memory.rs:263`, `resolve_scopes` at `:338`.
- A per-company provider binding with a durable provisioning/cleanup lifecycle
  (`memory_provider_connections`, `memory_provisioning_jobs`, `memory_cleanup_jobs`) driven by
  `MemoryWorker` — `src/application/services/memory_worker.rs`.
- Multipart upload, magic-byte validation and private object storage —
  `src/adapters/http/routes/ui_uploads.rs`, `src/domain/entities/upload.rs`,
  `FileStorage::store_object(BucketKind::Private, ..)` in `src/adapters/storage/mod.rs:47`.

**The property the whole design rests on:** document chunks are written into the *same* `"company"`
/ `"agent_{uuid}"` collections `resolve_scopes` already targets, so **the retrieval path needs no
change at all**. A channel with `retrieve_company_memory` on picks up company documents;
`retrieve_agent_memory` picks up that agent's documents; `deduplicate_chunks` and
`format_memory_context` (`memory_coordinator.rs:426`) already label, order and bound them; the
untrusted-content fence already wraps them.

Outcome: an owner uploads a file on the company or agent settings pane, sees it reach *In memory*,
and the agents that channel config allows start recalling it. Deleting it removes it from the
provider.

### Decisions already taken — do not relitigate

1. **Destination: provider ingest.** Postgres + the private bucket are the record of truth (they
   power the manage UI and re-ingest); the text is chunked and pushed into the company's selected
   memory provider. Documents therefore require a *ready* memory binding — the same interlock
   channels already enforce.
2. **Accepted formats: text, PDF, DOCX.** `.txt`, `.md`, `.csv`, `.json`, `.html` (via `htmd`,
   already a dependency), plus PDF and DOCX behind new crates.
3. **Delete is a real delete.** `MemoryProvider` gains a per-item forget method. Both providers
   support one — see below.

---

## The memory subsystem as it stands

Facts worth having in hand; every one is load-bearing for some phase.

- **There is no local memory store.** No `memories` table, no embeddings, no pgvector. Postgres
  holds only the binding and the provisioning lifecycle; content lives in the provider.
- **Port** `MemoryProvider` — `src/application/services/memory_provider.rs:106`, five methods
  (`provision`, `is_ready`, `recall`, `persist`, `delete`). Registry at `:271`, deployment-level
  `ConfiguredMemoryProviders` at `:188`.
- **Adapters** — `src/adapters/memory/{http,hydradb,hindsight}.rs` with `*_tests.rs` siblings and a
  shared `test_support.rs` (`mock_server`, `scripted_server`, `uniform_server`, `request_body`, …).
  `http.rs` holds `MemoryHttpClient`, `validate_identifier` (charset `[A-Za-z0-9._~-]`),
  `classify_status`, `classify_transport`, `bounded_json`, `merge_scope_results`.
- **Scopes** — `resolve_scopes(company, agent, user, agent_id, sender_email)` returns
  `ResolvedMemoryScope { scope, collection, weight }`: Company → `"company"` w 1.0, Agent →
  `"agent_{uuid}"` w 2.0, User → `"user_{sha256(email)}"` w 3.0. A requested-but-unresolvable scope
  is **skipped, never widened to company** (`plan/hydradb/01-preserve-memory-scope-isolation.md`).
- **Direction is per channel** — six booleans on `channels`; **mode and limits are per agent**
  (`memory_recall_mode`, `memory_max_results` 1–20, `memory_persistence_mode`); **provider is per
  company** (`companies.memory_provider`).
- **Remote namespace** — `remote_memory_database_id(company_id)` = `mail-agents-company-{32 hex}`;
  Hindsight further composes `{database_id}--{collection}` as its bank id.
- **Worker** — `MemoryWorker::run` (`memory_worker.rs:65`) runs a provisioning loop and a cleanup
  loop, `POLL_INTERVAL` 1s, `LEASE_SECONDS` 180, `MAX_PROVIDER_FAILURE_ATTEMPTS` 8. Lease renewal
  during a provider call is `supervise_memory_job_lease` (`memory_job_lease.rs`); backoff is
  `retry_at` / `next_poll_at` (`memory_job_schedule.rs`).
- **Job table shape to copy** — `memory_cleanup_jobs`, `migrations/20260817000000_init_schema.sql:1140`.
- **Ready-binding interlock** — `ChannelUseCases::check_memory_interlock`,
  `src/application/use_cases/channel.rs:283`.

### What deliberately does not change

`MemoryCoordinator::recall`, `resolve_scopes`, `deduplicate_chunks` and `format_memory_context` are
untouched. Two consequences to put in the UI copy rather than leave for someone to discover:

- `MemoryRecallAudience::External` drops the Company scope (`memory_coordinator.rs:90`), so
  **external senders never see company documents**. A good default, but a default.
- Documents compete with conversation memory for the agent's `memory_max_results` slots (default 5)
  and the shared `MAX_MEMORY_CONTEXT_CHARS = 16_000` budget. Raising `memory_max_results` on the
  agent is the lever; mention it beside the upload control.

---

## Provider APIs — verified against both OpenAPI specs

Not from the prose docs: from the machine-readable specs, which are downloadable and worth
re-pulling whenever this is revisited.

| | HydraDB | Hindsight |
|---|---|---|
| spec | `https://docs.hydradb.com/api-reference/v2/openapi.json` — *HydraDB Application API 0.1.0*, server `https://api.hydradb.com` | `https://api.hindsight.vectorize.io/openapi.json` (Cloud) and `https://hindsight.vectorize.io/openapi.json` (self-hosted) — both *0.9.2*, identical paths |
| auth | Bearer (`Authorization: Bearer prefix.secret`) + `API-Version: 2` header | Bearer |
| envelope | **every route** answers `{success, data, error:{code,message}, meta}`; the payload is under `data` | plain objects, no envelope |
| tenant field | **`database`**. `tenant_id` is a deprecated-but-accepted alias; **`database_id` is not in the spec at all** | bank id in the path |
| collection field | `collection`; `sub_tenant_id` deprecated | `{database_id}--{collection}` composed into the bank id |

### HydraDB — the routes we use

| Need | Route | Notes |
|---|---|---|
| provision | `POST /databases` | body `{database}`. `409` is idempotent success |
| readiness | `GET /databases/status?database=…` | **`data.infra.ready_for_ingestion` is a boolean**, not a top-level status string |
| recall | `POST /query` | `{database, type, query, query_by, collections, mode, max_results, additional_context}`. `collections` is the preferred scope selector and takes a weighted object. Rows are **`data.chunks[]`** with `chunk_content`, `chunk_uuid`, `id`, `collection`, `relevancy_score` |
| persist / ingest | `POST /context/ingest` (multipart) | `database` REQUIRED, `collection`, `type` ∈ `knowledge｜memory` (default `knowledge`), `upsert` default `true`, payload in **`memories`** (JSON string) or `app_knowledge` or `documents` (files). `202`; `data.results[]` carry `status` ∈ `queued｜processing｜completed｜failed` |
| forget | `DELETE /context` (JSON) | `{database, collection, type, ids:[…]}` — **an id array, so one request per document**. A missing id is `200` with `deleted:false` unless `X-HydraDB-Delete-Status: strict` |
| teardown | `DELETE /databases?database=…` | query parameter, **not** a JSON body |

A memory item inside `memories` is `{id, title, text, user_assistant_pairs, is_markdown, infer,
custom_instructions, user_name, expiry_time, metadata}` — either `text` or `user_assistant_pairs` is
required, and `custom_instructions` is **per item**, not per request. (Item fields come from the
prose reference; the spec types `memories` only as a JSON string.)

### Hindsight — verified conformant, no changes needed

Paths are `/v1/{namespace}/banks/…`, and `HINDSIGHT_BASE_URL` already carries that prefix —
`.env.example:87` and `docs/deploy.md:171` both spell it out. Relative to that base, every path the
adapter uses exists: `PUT|DELETE /banks/{id}`, `GET /banks`, `GET /banks/{id}/config`,
`POST /banks/{id}/memories`, `POST /banks/{id}/memories/recall`, and — for phase 3 —
`DELETE /banks/{id}/documents/{document_id}`.

The payloads check out too, field for field:

- **retain** `items[]` takes `content` (required), `document_id`, `context`, `tags`, `metadata`,
  `timestamp`, and **`update_mode` ∈ `replace｜append`** (default `replace`: the same `document_id`
  deletes the old data and reprocesses). Top level takes `async` and `operation_id`. Response is
  `{success, bank_id, items_count, async, operation_id, usage}` — exactly what `persist_scope`
  sends and checks.
- **recall** takes `query`, `budget` ∈ `low｜mid｜high`, `max_tokens`, `prefer_observations`,
  `types`, and returns `results[]` with `id`, `text`, `chunk_id`, `document_id`, `tags`, and
  `scores` — where **`RecallScores.final` is a required field**, so the `scores.final` the adapter
  reads is always present.
- `document_id` groups items: *"items sharing a document_id are grouped into the same document"*,
  and deleting the document removes every memory extracted from it.

Two things this opens up for phase 3, both optional: recall accepts `tags` + `tags_match`, so
document chunks tagged `"document"` could be filtered; and one `document_id` per uploaded document
would make forget a single call instead of one per chunk.

### HydraDB was off-spec and has been fixed

`src/adapters/memory/hydradb.rs` was written against a pre-v2 contract. Six changes landed ahead of
this plan, with tests pinning each:

| Route | Was | Now |
|---|---|---|
| all | read the payload at the top level | `v2_data()` unwraps `{success, data, error, meta}`, and logs `meta.deprecation` |
| `POST /databases` | `{database_id, name}` | `{database}` |
| `GET /databases/status` | `?database_id=`, read `status == "ready_for_ingestion"` | `?database=`, read `data.infra.ready_for_ingestion` |
| `POST /query` | `database_id`, rows from `results[].content` / `chunk_id` | `database`, rows from `data.chunks[].chunk_content` / `chunk_uuid` |
| `POST /context/ingest` | `database_id`, `items`, item `type`, request-level `custom_instructions` | `database`, `memories`, form-level `type=memory`, per-item `custom_instructions`, item carries `user_assistant_pairs` |
| `DELETE /databases` | JSON body `{database_id}` | `?database=` query parameter |

`database_id` was never an alias — it is absent from the v2 schema — so these calls were reaching
the server with no tenant. **None of it was caught by the suite**, because every test asserted
against a mock returning the shape the adapter expected. That is the lesson worth carrying into
phase 3: a mock proves the parser matches the fixture, not that the fixture matches the provider.
The `#[ignore]`d live smoke test is the only thing that can, so run it.

---

## Shared bounds

New consts live beside the existing `MAX_MEMORY_*` block at the top of
`src/domain/entities/memory.rs`. `src/AGENTS.md` — "Bound work at every external boundary" — and
advertising a limit without rejecting input that exceeds it is not enforcement.

| Const | Value | Enforced at |
|---|---|---|
| `MAX_MEMORY_DOCUMENT_BYTES` | `10 * 1024 * 1024` | `DefaultBodyLimit` + `MemoryDocumentUpload::parse` |
| `MAX_MEMORY_DOCUMENT_TEXT_CHARS` | `400_000` | after extraction |
| `MAX_MEMORY_DOCUMENT_CHUNK_CHARS` | `4_000` | `chunk_document_text` |
| `MEMORY_DOCUMENT_CHUNK_OVERLAP_CHARS` | `200` | `chunk_document_text` |
| `MAX_MEMORY_DOCUMENT_CHUNKS` | `128` | `chunk_document_text` (400k / 3.8k ≈ 106, plus headroom) |
| `MAX_MEMORY_DOCUMENTS_PER_SCOPE` | `50` | use case, before insert |
| `MAX_MEMORY_DOCUMENT_TITLE_CHARS` | `200` | `MemoryDocumentWrite::normalize` |
| `MEMORY_DOCUMENT_CHUNKS_PER_LEASE` | `16` | worker, then reschedule (fairness) |
| `MAX_DOCX_DECOMPRESSED_BYTES` | `64 * 1024 * 1024` | docx extractor (zip bomb) |
| `MAX_PDF_PAGES` | `500` | pdf extractor |

Reuse `truncate_memory_text` (`memory.rs:41`) for every text bound rather than hand-rolling one.

---

## Migration style

**Reset and edit in place.** New tables go into `migrations/20260817000000_init_schema.sql` itself;
both databases are recreated because the file's checksum changes.

```sh
dropdb --if-exists mail_agents      && createdb mail_agents
dropdb --if-exists mail_agents_test && createdb mail_agents_test
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" sqlx migrate run
```

`src/adapters/persistence/AGENTS.md` §1 describes additive migrations instead; the reset workflow in
`plan/shodh_memory_provider.md` §1 is the one in force.

Both new tables carry a `provider` CHECK listing `('hydradb', 'hindsight')` — **add them to the list
in `plan/shodh_memory_provider.md` §1** so a third provider still lands as one edit.

---

## House rules that bite on this change

Full rules in `AGENTS.md`, `src/AGENTS.md`, `src/application/AGENTS.md`,
`src/adapters/persistence/AGENTS.md`, `src/adapters/http/pages/AGENTS.md`. The ones this feature
walks straight into:

- **Newtypes over bare `String`** when two same-typed values travel together. `collection`,
  `remote_database_id` and `title` are all `String` and all adjacent in the job row.
- **Split along phases; keep `async fn` chains shallow.** A non-`async` helper costs no stack frame,
  which is why extraction is a sync port called through `spawn_blocking`. `Box::pin` at the seam
  descending into a provider, with a comment saying why. Measure with `scripts/stack-frames.sh`.
- **No flag parameters; name your tuples.** A persistence `create` with six-plus arguments takes one
  params struct instead — `ChannelWrite` (`use_cases/channel.rs`) is the model.
- **Don't collapse errors into defaults on authorization paths.** `.ok().flatten()` and
  `.unwrap_or(false)` are banned wherever the result feeds an authz or dedup decision.
- **Ports have no defaulted correctness methods.** Every impl and test double states its behaviour —
  that is 5 `MemoryProvider` impls to update in phase 3 (`hydradb.rs:164`, `hindsight.rs:328`,
  `memory_coordinator.rs:514`, `memory_worker.rs:609`, `memory_worker.rs:661`).
- **Preserve dependency direction.** Application code must not import `pdf_extract`, `zip`,
  `quick-xml`, `axum` or `sqlx`.
- **Tenant identifiers participate in foreign keys.** `(company_id, agent_id) REFERENCES
  agents(company_id, id)` — `agents_company_id_id_key` already exists (`init_schema.sql:217`).
- **Treat persisted JSON as untrusted**; never `expect` a JSONB value to deserialize.
- **Escape for the output context** — `escape_html_text` for text nodes, an attribute encoder for
  attributes and `hx-confirm`.
- **Every user action needs visible progress feedback** — see phase 5.
- **`#[cfg(test)] mod tests` inline**, moved to a `#[path = "…_tests.rs"]` sibling past ~500 lines;
  split any file past ~1,000 lines into a directory module.

### DB-backed tests share one database

From `src/adapters/persistence/AGENTS.md`, and these have all cost real debugging time:

- Suffix every database-wide unique value (company slugs, usernames, emails) with
  `Uuid::new_v4().simple()`.
- Never assert on a global query's totals — worker claims are unscoped on purpose. Assert *your* row
  by id.
- Do not assert a row is still `pending` or still unclaimed.
- **Leave no claimable residue.** A pending job row another test's worker can claim will hang that
  test rather than fail it. Complete it, delete it, or push it past the claim horizon.
- Establish a green baseline with `git stash` over three or four runs before calling a failure
  pre-existing.

```sh
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo test --lib   # lands on _test
```

---

## Verification, once every phase has landed

```sh
cargo fmt --check

dropdb --if-exists mail_agents      && createdb mail_agents
dropdb --if-exists mail_agents_test && createdb mail_agents_test
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" sqlx migrate run
psql "postgres://mac03@localhost:5432/mail_agents" -c '\d memory_documents' -c '\d memory_document_jobs'

# agent.rs `delete` becomes a transaction and is a compile-time macro query, so the cache moves.
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo sqlx prepare -- --all-targets
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo test
SQLX_OFFLINE=true cargo build          # the offline path CI and Fly.io use
./scripts/stack-budget.sh              # the worker gains a third async chain
```

End to end against a real provider, server on `:3001`:

1. Company settings → Long-term memory → pick a provider, wait for `ready`.
2. Company settings → **Documents** → upload a small `.md`, a PDF and a `.docx`. Each row goes
   `pending → ingesting → in memory` without a manual refresh.
3. Confirm the chunks landed in the right namespace and nowhere else — Hindsight: `GET /banks` shows
   `mail-agents-company-<uuid>--company` and **no** cross-company bank; HydraDB: `POST /context/list`
   for that `database` + `collection` lists the `doc_…` ids.
   **Then prove the `type: "memory"` choice was right**: `POST /query` with the adapter's own body
   (`type: "memory"`) must return the document chunks. If it does not, the chunks went to the
   knowledge space — fix that before going further, because everything downstream depends on it.
4. Channel settings → enable *retrieve company memory*. Send a message answerable only from the
   document; the reply should use it. Repeat for an agent document with *retrieve agent memory*.
5. Send the same question from a non-team address — the company document must **not** appear
   (the `External` audience gate).
6. Delete a document; confirm the row goes, a `forget` job runs, and a follow-up no longer recalls it.
7. Switch the company to the other provider; every document returns to `pending` and reaches
   *in memory* again against the new one.
8. Delete an agent that owns documents; confirm forget jobs are enqueued and drain.
9. Refuse paths: a 20 MB file, a `.exe` renamed `.pdf`, a truncated PDF, a zip-bomb `.docx`, and an
   upload while memory is still provisioning — each a worded refusal in the fragment, nothing in the
   bucket, nothing queued.
