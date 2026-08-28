# Add Shodh as a second long-term memory provider

## Context

Long-term memory today has exactly one backend. `MemoryProviderKind` is a single-variant enum
(`Hydradb`), five SQL `CHECK` constraints hardcode the literal `'hydradb'`, and the wiring that
decides "is memory available on this deployment" is a bare `bool` named `hydradb_configured`
threaded through the use cases, routes, and pages.

The seams for a second provider are already there and were clearly built for this: the outbound
port `MemoryProvider` (`src/application/services/memory_provider.rs:103`), a
`MemoryProviderRegistry` keyed by `MemoryProviderKind`, and a provisioning/cleanup schema keyed by
`(company_id, provider)` throughout. What is missing is the second variant, the adapter, a
migration relaxing the CHECKs, and the removal of the hydradb-specific shortcuts that were taken
because there was only ever one row.

We want [shodh-memory](https://github.com/varun29ankuS/shodh-memory) as the alternative — a
self-hosted single Rust binary that does semantic recall with **zero LLM calls** (local ONNX
embeddings + a Hebbian ranking graph). We talk to it over its **HTTP API**; we do not link the
`shodh-memory` crate (it pulls rocksdb, onnxruntime and tantivy into our build).

**Decisions already taken:**
- Provider selection stays **per company** (the existing `<select>` on company settings). Channel
  config keeps its per-scope toggles and inherits the company's provider — no `channels.memory_provider`.
- `persist` on Shodh **stores the conversation verbatim**. Shodh has no inference; HydraDB's
  `infer: true` + per-scope `custom_instructions` extraction has no equivalent and we are not
  adding an LLM pass on the persist path. `custom_instructions` and `memory_persistence_mode`
  become no-ops on Shodh, surfaced in the UI.

## The Shodh API (verified against the repo, not just the docs site)

Base URL + `X-API-Key` header (it also accepts `Authorization: Bearer`). Config is deployment-wide
(`SHODH_API_KEYS`, `SHODH_HOST`, `SHODH_PORT`), so one Shodh instance serves every company.

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

Shodh has **no database/collection concept** — `user_id` is the only partition key. So:

```
shodh user_id  =  "{database_id}--{collection}"
```

e.g. `mail-agents-company-0193…abcd--user_3f2a…` (64 hex). This preserves the scope isolation
`resolve_scopes()` guarantees, and the `--` prefix makes a company's namespaces enumerable for
`delete`. Longest case is the User scope at **123 bytes against Shodh's 128 limit** — tight, so the
adapter checks the length and returns `MemoryProviderError::RequestTooLarge`, with a unit test
pinning the worst case. (If you want more headroom, the fallback is to hash the collection segment
to 32 hex; it costs nothing but readability, since scope attribution comes from *which call*
returned the row, not from parsing the id back.)

| Port method | Shodh implementation |
|---|---|
| `provision` | `GET /health` — there is no tenant to create. Success completes the existing provisioning job on its first poll; the whole durable lifecycle keeps working unchanged. |
| `is_ready` | `GET /health`, `status == "ok"`. |
| `recall` | One `POST /api/recall` **per scope** (≤ `MAX_MEMORY_TARGET_COLLECTIONS` = 3) via `join_all`, `mode: "hybrid"`, `limit: max_results`. Merge, sort by `score * scope.weight` descending, truncate to `max_results`. `MemoryRecallMode` selects the timeout only (Fast → `fast_timeout`, Thinking → `thinking_timeout`). `additional_context` has no field on `/api/recall` — append it to the query as a bounded suffix. (`/api/proactive_context` takes a `context` field but defaults to `auto_ingest: true`, i.e. it *writes* on a read path; not worth it here.) |
| `persist` | One `POST /api/upsert` per target via `join_all`, `external_id = conversation.id` (already `stable_memory_id(task_id, channel_id, agent_id)`, so retries are idempotent), `content` = the user/assistant pair, `memory_type: "Conversation"`, `tags: [scope label]`. `target.custom_instructions` is ignored. |
| `delete` | `GET /api/users`, filter by the `{database_id}--` prefix, `DELETE /api/users/{id}` for each. |

**Bound that needs stating:** our `MemoryConversation` allows 32k chars each for user and assistant
— up to 64k, over Shodh's 50k byte content cap. The adapter composes the pair and truncates with
the existing `truncate_memory_text` to a new `MAX_SHODH_CONTENT_BYTES`, reporting truncation the
same way the rest of the memory path does.

## Work items

### 1. Schema — edit `migrations/20260817000000_init_schema.sql` in place

The database is reset rather than migrated, so there is no backward compatibility to preserve and
no additive migration to write. Widen five CHECK constraints in the squashed init file so they read
`IN ('hydradb', 'shodh')` from the start:

- `companies.companies_memory_provider_check`
- `memory_provider_connections.provider` (`CHECK (provider IN ('hydradb'))`)
- `memory_remote_resource_lifecycles.provider` (`CHECK (provider = 'hydradb')`)
- `memory_provisioning_jobs.provider` (`CHECK (provider = 'hydradb')`)
- `memory_cleanup_jobs.provider` (`CHECK (provider IN ('hydradb'))`)

Note the three written as `= 'hydradb'` become `IN (...)`, so the constraint shape is uniform.
Nothing else in the schema is provider-specific — every PK/UNIQUE is already `(…, provider)`.

Recreate the databases after editing, since the init file's checksum changes:

```sh
dropdb --if-exists mail_agents && createdb mail_agents
dropdb --if-exists mail_agents_test && createdb mail_agents_test
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo sqlx migrate run
```

*No `cargo sqlx prepare` needed*: the memory persistence layer uses runtime `sqlx::query(...)` /
`query_as::<_, XDb>(...)`, not the macros — no `.sqlx/` entry mentions memory. (If you end up
touching a macro query, the `src/AGENTS.md` rule applies:
`DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo sqlx prepare -- --all-targets`.)

### 2. Domain — `src/domain/entities/memory.rs`

- `MemoryProviderKind::Shodh` + its `as_str() == "shodh"` arm. This is the compile-error driver for
  the rest of the change; follow the errors.
- New bounds beside the existing `MAX_MEMORY_PROVIDER_*` consts:
  `MAX_SHODH_NAMESPACE_BYTES = 128`, `MAX_SHODH_CONTENT_BYTES = 50_000`,
  `MIN_SHODH_CONTENT_BYTES = 10`.

### 3. Config — `src/infra/config.rs`

`HydraDbConfig { api_key, base_url, fast_timeout, thinking_timeout }` and the Shodh config are the
same four fields with the same validation. Per `src/AGENTS.md` §"One decision, one place", extract
the shape and its `from_values` validation once (e.g. `MemoryProviderHttpConfig`) and have
`AppConfig` carry `hydradb: Option<…>` and `shodh: Option<…>` of that type, keeping the existing
all-or-nothing env parsing and the startup assertions (absolute http(s) URL, credential length,
`thinking >= fast`, both `<= MAX_MEMORY_PROVIDER_REQUEST_SECONDS`).

New env vars, documented in `.env.example` and `docs/deploy.md` next to the `HYDRA_DB_*` block:
`SHODH_BASE_URL`, `SHODH_API_KEY`, `SHODH_FAST_TIMEOUT_SECS`, `SHODH_THINKING_TIMEOUT_SECS`.

### 4. Shared HTTP plumbing — new `src/adapters/memory/http.rs`

`hydradb.rs` holds ~140 lines of provider-agnostic, carefully-bounded machinery that the Shodh
adapter needs verbatim. Lift it before writing the second adapter rather than copying it:

- `json_response` (content-length precheck **and** streaming byte cap → `ResponseTooLarge`)
- `bounded_json` (→ `RequestTooLarge`)
- `validate_identifier`
- `classify_status` / `classify_transport`
- `measured(...)` + the activity handle

The multipart builder stays in `hydradb.rs` — Shodh is JSON-only.

### 5. Adapter — new `src/adapters/memory/shodh.rs`

`ShodhProvider` implementing `MemoryProvider`, modelled directly on `HydraDbProvider`:
constructor validates and returns `Err(MemoryProviderError::Unavailable)` rather than panicking;
one `reqwest::Client` with `connect_timeout`; per-request `.timeout()` by recall mode;
`SecretString` key sent as `X-API-Key`; **no in-adapter retry** (retry is durable, in
`memory_worker.rs` + the `memory_*_jobs` tables); error messages carry no bodies or credentials.

Register in `src/adapters/memory/mod.rs`.

Keep the file under the `src/AGENTS.md` ~1,000-line trigger — with the tests below it will be
close, so plan on `#[cfg(test)] #[path = "shodh_tests.rs"] mod tests;` from the start.

### 6. Persistence — `src/adapters/persistence/`

Two real bugs that only exist once a second provider does:

- **`memory.rs:57` `memory_connection()` hardcodes `WHERE connection.provider = 'hydradb'`.** It
  must resolve the company's *selected* provider — join `companies` on
  `connection.provider = company.memory_provider`, the way `active_binding` (line 28) already
  correctly does.
- **`select_provider()` never retires the previous provider.** Switching HydraDB → Shodh today
  would leave the HydraDB `memory_provider_connections` row and its lifecycle at
  `desired_state = 'present'` forever — the remote database leaks and is never torn down. Fix
  inside the existing transaction with
  `DELETE FROM memory_provider_connections WHERE company_id = $1 AND provider <> $2`: the existing
  `memory_connection_lifecycle_compatibility_delete` BEFORE DELETE trigger already flips the
  lifecycle to `'absent'`, sets `quiesce_until = now + 180s` and enqueues a `memory_cleanup_jobs`
  row keyed `md5(provider || ':' || remote_database_id)`. Also drop the old provider's non-leased
  `memory_provisioning_jobs` row (leased ones are fenced by `operation_generation`).
- `memory_rows.rs:61` `fn provider()` — add the `"shodh"` arm.
- `retry_provisioning`'s "HydraDB has not been selected." message becomes provider-neutral.

### 7. Generalize "is a provider configured" — `src/application/`

`MemoryUseCases.hydradb_configured: bool` becomes a set. A small type beside the port
(`ConfiguredMemoryProviders(HashSet<MemoryProviderKind>)` with `contains(kind)` / `is_empty()`)
keeps this a value rather than another bool parameter, per §"No flag parameters".

- `use_cases/memory.rs`: `hydradb_configured()` → `configured()`; `select_hydradb(user, company)`
  → `select_provider(user, company, kind)`; `retry()` checks `company.memory_provider.is_some()`
  and that the kind is configured, not `== Hydradb`; `status()` gates on the *selected* provider
  being configured.
- `use_cases/channel.rs`: both `memory_ready()` (line ~313) and the write-path validation
  (line ~285) check `self.config.hydradb.is_none()` — replace with the configured-set check. The
  four `AppError::BadRequest` messages there are already provider-neutral and stay as they are.
- `infra/setup.rs:52-83`: build the registry from both configs; pass the configured set into
  `MemoryUseCases::new`.

### 8. Routes and UI

- `src/adapters/http/routes/company.rs:111` — `validate_memory_provider(value, hydradb_configured)`
  becomes a match over `MemoryProviderKind::from_str`-style parsing against the configured set,
  with a per-provider "not configured for this deployment" message. `apply_memory_provider` calls
  the generalized `select_provider`.
- `src/adapters/http/pages/company_settings.rs` — add `<option value="shodh">Shodh</option>` beside
  HydraDB in the `memory_provider` select (lines 508-515), each `disabled` when its own config is
  absent; `memory_status()` (line 550) currently hardcodes the string `"HydraDB"` in its badge and
  its "not configured" copy — take the provider from `MemoryConnection.provider`. Update the helper
  text to note that Shodh stores conversations verbatim (no extraction pass), so
  `memory_persistence_mode` does not apply to it.
- `src/adapters/http/pages/channel_settings.rs` `memory_fields()` (line ~902) and the classic
  `channels.rs:970` `classic_memory_fields()` — no field changes; only the persistence-mode select
  gets a note that it applies to HydraDB only.

### 9. Runtime metrics

`HydraDbActivity` / `HydraDbInterval` and the `runtime_metric_samples.hydradb_{calls,failures,
duration_ms}` columns are a machine-level aggregate. Share the same handle with the Shodh adapter
and rename the Rust types to `MemoryProviderActivity` / `MemoryProviderInterval` (mechanical, ~6
files); **keep the DB column names** to avoid a migration on a table with coupled CHECK
constraints, with a schema comment recording why. Relabel the dashboard panel in
`pages/dashboard_runtime.rs` ("HydraDB memory calls" → "Memory provider calls"). A per-provider
split of these counters is deliberately out of scope.

### 10. Tests

Mirror the existing `hydradb.rs` test module (it already has a `mock_server` / `raw_response_server`
harness worth reusing from `shodh_tests.rs`):

- namespace composition, charset, and the **worst-case 123-byte** length assertion
- `recall` fans out one call per scope, weights and truncates to `max_results`, and rejects a
  response with more rows than requested (`TooManyResults`)
- `recall` attributes each row to the scope whose call returned it
- oversized `Content-Length` rejected without reading the body; a chunked body crossing the cap
  stops with `ResponseTooLarge`; a never-ending body obeys the request timeout
- `persist` composes and truncates over `MAX_SHODH_CONTENT_BYTES`, sends `external_id`, and maps a
  non-2xx per item to `RejectedItem` without echoing the response
- content shorter than `MIN_SHODH_CONTENT_BYTES` → `RejectedItem`
- `delete` filters `GET /api/users` by prefix and never deletes another company's namespace
- `config.rs`: Shodh config is all-or-nothing and validated (mirror
  `hydradb_config_is_all_or_nothing_and_validated`)
- `persistence/memory.rs` (DB-backed): switching provider retires the previous connection, flips its
  lifecycle to `'absent'` and enqueues exactly one cleanup job; `active_binding` follows the newly
  selected provider

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
   reply uses the recalled context.
4. `curl -s -H 'X-API-Key: dev-key' localhost:3030/api/users` — expect
   `mail-agents-company-<uuid>--company` and `…--user_<sha256>`, and **no** cross-company ids.
5. `curl -s -H 'X-API-Key: dev-key' -X POST localhost:3030/api/recall -d '{"user_id":"…--company","query":"…","limit":5}'`
   to see what was actually stored.
6. Switch the company back to HydraDB and confirm a `memory_cleanup_jobs` row appears for `shodh`
   and that the Shodh namespaces are gone ~180s later (the quiesce window).

## Risks / explicitly out of scope

- **123 of 128 bytes** of Shodh's `user_id` budget in the User scope. Guarded and tested, but it is
  an upstream constant we do not control.
- `delete` depends on `GET /api/users` returning every tenant on the instance in one unpaginated
  response. On a large shared instance that can exceed `MAX_MEMORY_PROVIDER_RESPONSE_BYTES` and
  fail as `ResponseTooLarge`; the durable cleanup job will retry forever without progressing.
  Acceptable for a self-hosted per-deployment instance; worth revisiting if Shodh is ever shared.
- Recall quality will differ from HydraDB in kind, not just degree — verbatim conversation storage
  plus lexical/embedding ranking rather than LLM-extracted facts. Worth an A/B on a real channel
  before recommending Shodh as a default.
- `plan/hydradb/06-make-memory-ingestion-durable.md` (memory-ingestion outbox) stays unimplemented;
  `persist` remains synchronous best-effort for both providers.
