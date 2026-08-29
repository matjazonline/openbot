# Add Shodh as a third long-term memory provider

> **Revised after the Hindsight integration landed.** Most of what the original version of this
> plan described is now done: the shared HTTP transport, the configured-provider set, the
> provider-neutral routes and settings UI, the runtime-metric rename, and the widened `CHECK`
> constraints all exist. Two "bugs" the original §6 listed had already been fixed before it was
> written. What is left is the parts that are genuinely Shodh-specific — an enum variant, four env
> vars, one adapter, and its tests. See `plan/hindsight_remaining_work.md` for what is outstanding
> on Hindsight itself; none of it blocks this.

## Context

Long-term memory now has two backends, HydraDB and Hindsight, behind the outbound port
`MemoryProvider` (`src/application/services/memory_provider.rs`) and a `MemoryProviderRegistry`
keyed by `MemoryProviderKind`. Adding a third is deliberately small: the provider list is rendered
from `MemoryProviderKind::ALL`, wire strings are parsed through `MemoryProviderKind::parse`, and
"is this provider available on this deployment" is a `ConfiguredMemoryProviders` set rather than a
per-provider bool. No route, page, use-case or persistence file needs to change.

We want [shodh-memory](https://github.com/varun29ankuS/shodh-memory) as a third option — a
self-hosted single Rust binary that does semantic recall with **zero LLM calls** (local ONNX
embeddings + a Hebbian ranking graph). We talk to it over its **HTTP API**; we do not link the
`shodh-memory` crate (it pulls rocksdb, onnxruntime and tantivy into our build).

**Decisions already taken:**
- Provider selection stays **per company** (the existing `<select>` on company settings). Channel
  config keeps its per-scope toggles and inherits the company's provider — no
  `channels.memory_provider`.
- `persist` on Shodh **stores the conversation verbatim**. Shodh has no inference; HydraDB's
  `infer: true` extraction and Hindsight's fact extraction have no equivalent, and we are not
  adding an LLM pass on the persist path. `custom_instructions` and `memory_persistence_mode` are
  no-ops on Shodh, surfaced in the UI.

## The Shodh API (verified against the repo, not just the docs site)

Base URL + `X-API-Key` header — **it also accepts `Authorization: Bearer`**, which is what the
shared `MemoryHttpClient::request` sends, so no custom auth header is needed. Config is
deployment-wide (`SHODH_API_KEYS`, `SHODH_HOST`, `SHODH_PORT`), so one Shodh instance serves every
company.

| Need | Endpoint | Notes |
|---|---|---|
| health | `GET /health` | `HealthResponse { status, version, uptime_seconds, memory_mb, active_users }` |
| store | `POST /api/upsert` | `UpsertRequest { user_id, external_id, content, memory_type, tags, importance? }` → `UpsertResponse { id, external_id, created, updated, revision }`. Idempotent on `external_id` — preferable to `/api/remember` for a retried persist. |
| search | `POST /api/recall` | `RecallRequest { user_id, query, limit, mode, offset, debug }` → `RecallResponse { memories: [{ id, experience: { content, memory_type, tags }, importance, created_at, score, tier }], count }` |
| list tenants | `GET /api/users` | returns `Vec<String>` of every `user_id` on the box |
| delete tenant | `DELETE /api/users/{user_id}` | `{ success, user_id, message }` (GDPR path) |

Server-side limits we must respect (`src/validation.rs` in that repo):
`MAX_USER_ID_LENGTH = 128`, charset **alphanumeric + `-` `_` `@` `.` only (no `:`)**,
`MAX_CONTENT_LENGTH = 50_000` bytes, `MIN_MEANINGFUL_CONTENT_LENGTH = 10`, `MAX_TAGS = 50`.
`memory_type` is a closed set (`Observation | Decision | Learning | Error | Discovery | Pattern |
Context | Task | Conversation | …`).

### Mapping our port onto it

Shodh has **no database/collection concept** — `user_id` is the only partition key, exactly like
Hindsight's bank. So the namespace scheme is the one `hindsight.rs` already uses:

```
shodh user_id  =  "{database_id}--{collection}"
```

e.g. `mail-agents-company-0193…abcd--user_3f2a…` (64 hex). This preserves the scope isolation
`resolve_scopes()` guarantees, and the `--` infix makes a company's namespaces enumerable for
`delete`. Longest case is the User scope at **123 bytes against Shodh's 128 limit** — tight, so the
adapter checks the length and returns `MemoryProviderError::RequestTooLarge`, with a unit test
pinning the worst case (copy `the_widest_bank_id_we_generate_stays_inside_the_bound` from
`hindsight_tests.rs`). If you want more headroom, the fallback is hashing the collection segment to
32 hex; it costs nothing but readability, since scope attribution comes from *which call* returned
a row, not from parsing the id back.

Shodh's charset also permits `@` and `.`, but the shared `validate_identifier` already restricts
ids to `[A-Za-z0-9._~-]`, which is a subset — every id we generate passes both.

| Port method | Shodh implementation |
|---|---|
| `provision` | `GET /health` — there is no tenant to create. Success completes the existing provisioning job on its first poll; the whole durable lifecycle keeps working unchanged. |
| `is_ready` | `GET /health`, `status == "ok"`. |
| `recall` | One `POST /api/recall` **per scope** (≤ `MAX_MEMORY_TARGET_COLLECTIONS` = 3) via `join_all`, `mode: "hybrid"`, `limit: max_results`, then hand the rows to the shared `merge_scope_results` with each scope's `weight` — the same shape `hindsight.rs::recall` uses. `MemoryRecallMode` selects the timeout only, via `MemoryHttpClient::timeout_for`. `additional_context` has no field on `/api/recall`; append it to the query as a bounded suffix the way `hindsight.rs` does. (`/api/proactive_context` takes a `context` field but defaults to `auto_ingest: true`, i.e. it *writes* on a read path; not worth it here.) |
| `persist` | One `POST /api/upsert` per target via `join_all`, `external_id = conversation.id` (already `stable_memory_id(task_id, channel_id, agent_id)`, so retries are idempotent), `content` = the user/assistant pair, `memory_type: "Conversation"`, `tags: [scope.label()]`. `target.custom_instructions` is ignored. |
| `delete` | `GET /api/users`, filter by the `{database_id}--` prefix, `DELETE /api/users/{id}` for each. |

**Bound that needs stating:** our `MemoryConversation` allows 32k chars each for user and assistant
— up to 64k, over Shodh's 50k byte content cap. The adapter composes the pair and truncates with
the existing `truncate_memory_text` to a new `MAX_SHODH_CONTENT_BYTES`, reporting truncation the
same way the rest of the memory path does. (Hindsight needed no such constant; the shared
`bounded_json` envelope was enough there. Shodh's cap is tighter than the envelope, so it does.)

---

## Already done — do not redo

These were work items in the original plan and are now in `main`. Listed so the diff stays small
and nobody re-derives them.

| Original item | State |
|---|---|
| §4 Shared HTTP plumbing | **Done.** `src/adapters/memory/http.rs` holds `MemoryHttpClient` (constructor validation, `measured`, `request`, `json_response`, `bounded_json`), `validate_identifier`, `classify_status`, `classify_transport`, `validate_recall_bounds`, and `merge_scope_results` with `ScopeRecallResults` / `ScoredMemoryChunk`. |
| §6 Persistence bugs | **Already fixed before that plan was written.** `memory_connection()` joins `connection.provider = company.memory_provider`, and `select_provider()` deletes the abandoned connection and its non-leased provisioning job inside the transaction. Covered by `switching_providers_retires_the_previous_connection_and_enqueues_its_cleanup`. |
| §6 `memory_rows.rs` / `company.rs` provider arms | **Done.** Both call `MemoryProviderKind::parse`; a new variant needs no edit. |
| §7 Configured-provider set | **Done.** `ConfiguredMemoryProviders` (`services/memory_provider.rs`) with `contains` / `is_empty` / `select`; `MemoryUseCases::{configured, select_provider}`; `AppConfig::configured_memory_providers()`; `use_cases/channel.rs` gates on the *selected* provider being configured. |
| §8 Routes and UI | **Done.** `routes/company.rs` and `routes/ui_companies.rs` go through `ConfiguredMemoryProviders::select`; the three provider `<select>`s render from `MemoryProviderKind::ALL` via `pages::company_settings::memory_provider_options`; `memory_status()` takes its label from `MemoryConnection.provider`. |
| §9 Runtime metrics | **Done.** `MemoryProviderActivity` / `MemoryProviderInterval`, one handle shared across providers, dashboard panel relabelled. DB column names `hydradb_*` kept deliberately, with a schema comment. |
| §10 Test harness | **Done.** `src/adapters/memory/test_support.rs` holds `mock_server`, `scripted_server`, `uniform_server`, `raw_response_server`, `concurrent_server`, `request_line`, `request_body`. Adapter tests live in sibling files. |

---

## Work items

### 1. Schema — one word in `migrations/20260817000000_init_schema.sql`

Five `CHECK` constraints already read `IN ('hydradb', 'hindsight')`; add `'shodh'` to each:

- `companies.companies_memory_provider_check` (line 152)
- `memory_provider_connections.provider` (line 999)
- `memory_remote_resource_lifecycles.provider` (line 1017)
- `memory_provisioning_jobs.provider` (line 1045)
- `memory_cleanup_jobs.provider` (line 1105)

Add the value only once the adapter exists — a constraint accepting a provider with nothing behind
it lets a bad write in.

The database is reset rather than migrated, so edit in place and recreate both databases (the
file's checksum changes):

```sh
dropdb --if-exists mail_agents && createdb mail_agents
dropdb --if-exists mail_agents_test && createdb mail_agents_test
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo sqlx migrate run
```

*No `cargo sqlx prepare` needed*: the memory persistence layer uses runtime `sqlx::query(...)` /
`query_as::<_, XDb>(...)`, not the macros. (If you end up touching a macro query, the
`src/AGENTS.md` rule applies: `DATABASE_URL=… cargo sqlx prepare -- --all-targets`.)

### 2. Domain — `src/domain/entities/memory.rs`

- `MemoryProviderKind::Shodh`, its `as_str() == "shodh"` and `label() == "Shodh"` arms, and bump
  `ALL` to `[Self; 3]`. The `match` arms are exhaustive, so the compiler drives the rest; the
  round-trip test `every_provider_kind_round_trips_through_its_wire_value` covers the new variant
  automatically.
- New bounds beside the existing `MAX_HINDSIGHT_*` consts: `MAX_SHODH_NAMESPACE_BYTES = 128`,
  `MAX_SHODH_CONTENT_BYTES = 50_000`, `MIN_SHODH_CONTENT_BYTES = 10`.

### 3. Config — `src/infra/config.rs`

The shape and its validation already exist as `MemoryProviderHttpConfig::from_env(prefix)`. Add:

- `pub shodh: Option<MemoryProviderHttpConfig>` on `AppConfig`
- `MemoryProviderHttpConfig::from_env("SHODH")` in `AppConfig::from_env`
- a `MemoryProviderKind::Shodh => self.shodh.is_some()` arm in `configured_memory_providers()`
- `shodh: None` in the test-config constructor

New env vars, documented in `.env.example` and `docs/deploy.md` beside the `HYDRA_DB_*` and
`HINDSIGHT_*` blocks: `SHODH_BASE_URL`, `SHODH_API_KEY`, `SHODH_FAST_TIMEOUT_SECS`,
`SHODH_THINKING_TIMEOUT_SECS`. Validation — credential and URL length, absolute `http(s)` URL,
timeouts ≤ `MAX_MEMORY_PROVIDER_REQUEST_SECONDS`, `thinking >= fast` — comes for free from the
shared type and reports against the `SHODH_` prefix.

### 4. Adapter — new `src/adapters/memory/shodh.rs`

Model it on **`hindsight.rs`, not `hydradb.rs`**: Shodh is the same shape — a namespace-per-scope
provider that fans out one call per scope and merges client-side. `hydradb.rs` is the odd one out
(server-side weighted ranking, multipart ingest).

What to copy directly from `hindsight.rs`: the `MemoryHttpClient` field and constructor, the
`{database_id}--{collection}` namespace composition with its length and charset check, the
`join_all` recall fan-out feeding `merge_scope_results`, and the prefix-filtered `delete`.

What differs:
- `provision` / `is_ready` are both `GET /health` — there is no per-tenant resource, so `provision`
  is purely a reachability and credential check.
- `persist` truncates the composed pair to `MAX_SHODH_CONTENT_BYTES` and rejects anything under
  `MIN_SHODH_CONTENT_BYTES` as `RejectedItem`.
- Recall rows are `memories[].experience.content` with a top-level `score`, not `results[].text`
  with `scores.final`.
- `GET /api/users` returns a bare `Vec<String>`, not a paginated object — see the risk below.

No in-adapter retry (retry is durable, in `memory_worker.rs` plus the `memory_*_jobs` tables), and
error messages carry no bodies or credentials. Register in `src/adapters/memory/mod.rs` and add the
`if let Some(shodh) = config.shodh.as_ref()` block to `src/infra/setup.rs`, sharing the one
`MemoryProviderActivity` handle.

Keep the file under the `src/AGENTS.md` ~1,000-line trigger: use
`#[cfg(test)] #[path = "shodh_tests.rs"] mod tests;` from the start, as both other adapters do.

### 5. Tests — new `src/adapters/memory/shodh_tests.rs`

Use the shared `test_support` harness. `hindsight_tests.rs` is the closest model and most cases
port directly:

- namespace composition, charset, and the **worst-case 123-byte** length assertion
- `provision` and `is_ready` map `status == "ok"` and a non-ok health response correctly
- `recall` fans out one call per scope, weights by `score * scope.weight`, truncates to
  `max_results`, and attributes each row to the scope whose call returned it
- `recall` appends `additional_context` to the query and rejects a response with more rows than
  requested (`TooManyResults`) and more than three scopes (`TooManyTargets`)
- `persist` composes and truncates over `MAX_SHODH_CONTENT_BYTES`, sends `external_id`, and maps a
  non-2xx per item to `RejectedItem` without echoing the response
- content shorter than `MIN_SHODH_CONTENT_BYTES` → `RejectedItem`
- `delete` filters `GET /api/users` by prefix and **never deletes another company's namespace**
- `config.rs`: Shodh config is all-or-nothing and validated (mirror
  `hindsight_config_is_all_or_nothing_and_validated`)
- an `#[ignore]`d live smoke test, as `hydradb_tests.rs:435` has and
  `plan/hindsight_remaining_work.md` §1 specifies for Hindsight — Shodh is trivial to run locally,
  so this one is cheap to actually exercise

**Do not** add a DB-backed test that selects a provider without deleting the provisioning job it
queues. These tests share one database with `memory_worker`'s, whose workers claim *unscoped*; a
left-behind pending job is claimed by whichever worker test runs next, whose registry carries only
the provider it is exercising, so the claim resolves to no provider and that test's own job never
runs — it hangs rather than fails. See the tail of
`switching_providers_retires_the_previous_connection_and_enqueues_its_cleanup` for the cleanup and
the reasoning. Cleanup jobs need no such care; the 180-second quiescence window keeps them
unclaimable past the end of a run.

## Verification

```sh
cargo fmt --check
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo sqlx migrate run
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo test
SQLX_OFFLINE=true cargo build            # the offline path CI and Fly.io use
```

End-to-end against a real Shodh:

```sh
docker run -d -p 3030:3030 -v shodh-data:/data \
  -e SHODH_DEV_API_KEY=dev-key varunshodh/shodh-memory
curl -s localhost:3030/health
```

then in `.env`: `SHODH_BASE_URL=http://localhost:3030`, `SHODH_API_KEY=dev-key`,
`SHODH_FAST_TIMEOUT_SECS=15`, `SHODH_THINKING_TIMEOUT_SECS=60`; run the server (`:3001`) and:

1. Company settings → Long-term memory → **Shodh**. The badge should reach `ready` on the first
   worker poll (~1-2s), since `provision` is a health check.
2. Channel settings → enable retrieve + persist for Company and User scope.
3. Send a message through the channel, then a follow-up that depends on the first. Confirm the
   reply uses the recalled context. Unlike Hindsight there is no extraction lag — Shodh stores
   verbatim, so the follow-up can be immediate.
4. `curl -s -H 'X-API-Key: dev-key' localhost:3030/api/users` — expect
   `mail-agents-company-<uuid>--company` and `…--user_<sha256>`, and **no** cross-company ids.
5. `curl -s -H 'X-API-Key: dev-key' -X POST localhost:3030/api/recall -d '{"user_id":"…--company","query":"…","limit":5}'`
   to see what was actually stored.
6. Switch the company to another provider and confirm a `memory_cleanup_jobs` row appears for
   `shodh` and that the Shodh namespaces are gone ~180s later (the quiesce window).

## Risks / explicitly out of scope

- **123 of 128 bytes** of Shodh's `user_id` budget in the User scope. Guarded and tested, but it is
  an upstream constant we do not control.
- `delete` depends on `GET /api/users` returning every tenant on the instance in one **unpaginated**
  response. On a large shared instance that can exceed `MAX_MEMORY_PROVIDER_RESPONSE_BYTES` and fail
  as `ResponseTooLarge`; the durable cleanup job will then retry forever without progressing. This
  is the one place Shodh is genuinely worse than Hindsight, whose `GET /banks` paginates and takes a
  server-side filter. Acceptable for a self-hosted per-deployment instance; worth revisiting if
  Shodh is ever shared.
- Recall quality will differ from both existing providers in kind, not just degree — verbatim
  conversation storage with lexical/embedding ranking, rather than LLM-extracted facts. Worth an
  A/B on a real channel before recommending Shodh as a default.
- `plan/hydradb/06-make-memory-ingestion-durable.md` (memory-ingestion outbox) stays unimplemented;
  `persist` remains synchronous best-effort for every provider.
