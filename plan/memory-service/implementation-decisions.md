# Memory service implementation decisions

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Expansion record for phases [01](01-service-boundaries-and-contracts.md)–[04](04-observation-generation.md),
the seam assumptions [05](05-openbot-integration.md) relies on, and its OpenBot decisions
(E1–E12). The phase files remain the acceptance criteria; this record fixes the architecture they
leave to implementation. Where this record and a phase file disagree, the phase file wins and this
record is corrected.

Checked against base `a6bed48a`: OpenBot [access levels](../../src/domain/entities/access_level.rs)
(`RunKnowledge` ceiling/content), [run admission](../../src/adapters/persistence/workflow/knowledge.rs)
(`raise_content_level_on` is a monotone `GREATEST`), the [embedding adapter](../../src/adapters/embedding/mod.rs)
and [artifact validation](../../src/adapters/embedding/artifacts.rs), [knowledge search](../../src/adapters/persistence/knowledge/search.rs)
(`'simple'` text search), the [history budget](../../src/application/workflow/agent_context.rs)
(`2 × JSON bytes` token bound), the `memory.load`/`memory.save` [contracts](../../src/application/workflow/registry/families.rs)
(`MAX_ITEMS` 256, `MAX_TEXT` 16,384), [CI](../../.github/workflows/ci.yml), [check.sh](../../scripts/check.sh),
[network isolation](../../scripts/test-network-isolation.sh), the [Dockerfile](../../Dockerfile) and
`NO_BACKWARD_COMPATIBILITY_NOTES` in [src/main.rs](../../src/main.rs).
Claude-Mem reference revision: `tmp/claude-mem` at `3ab6e1a215c7bbcf716b9d8d763aa78bacba52a0`,
Apache-2.0, NOTICE "Copyright 2026 Alex Newman". Adapted ideas (RRF k = 60, filtered HNSW iterative
scan with a short-leg retry, observation/summary intent) are credited in `services/memory/NOTICE`.

## D1 Package layout and independence

- One Cargo package `memory-service` (lib + bin) at `services/memory/` with an empty `[workspace]`
  table, so it is its own workspace root wherever it is copied. Root `Cargo.toml` has no workspace,
  and the root reachability test, transport-boundary and sample-boundary scripts scan only root
  `src/`/`tests/`, so nothing in the root gates picks the service up or needs an exclusion.
- Own `Cargo.lock`, `.sqlx/`, `migrations/`, `rust-version = "1.95"` and `rust-toolchain.toml`
  pinned to `1.95.0`, `.gitignore` (`target/`, `assets/model/` — the root ignores only
  `/target`), `Dockerfile` whose build context is the service directory, `NOTICE`, `openapi.json`,
  `contract/examples/`, `assets/model-manifest.json` (OpenBot's pins: model files, per-platform
  ONNX Runtime, model licence/notice), and `scripts/`: `verify.sh` (CI entrypoint), `focused.sh`,
  `boundary-check.sh`, `extracted-build.sh`, `prepare-model.py`, `provision-db.sh`,
  `test-network-isolation.sh`, `check-image.py`. Scripts are adapted copies; nothing sources a
  root script. 02A adds `services/` to the root `.dockerignore`, so the root image context never
  carries the service or its ~490 MB of model assets.
- Configuration isolation: `services/memory/.cargo/config.toml` sets `[env]` `RUST_MIN_STACK` and
  `SQLX_OFFLINE = "true"` (not forced, so `prepare` can override it). Cargo reads config from the
  current directory upward, so every script `cd`s into the service directory first. The service
  config must set every key the root `[env]` sets (today only `RUST_MIN_STACK`). The boundary
  check (run by `focused.sh` and `verify.sh`) reads every parent-directory
  `.cargo/config.toml`/`config`. A parent array-valued flag (`build.rustflags`,
  `build.rustdocflags`, `target.*.rustflags`, `target.*.rustdocflags`) always fails, even when the
  service sets the same key, because Cargo concatenates arrays. Any other table or key fails
  (`[env]`, `[build]`, `[target]`, `[net]`, `[alias]`, …) the service config does not set itself.
  Cargo concatenates parent `rustflags` and cannot be stopped from inside, so detection is the
  control; `verify.sh` runs the check before any build. `$CARGO_HOME` config is developer-machine
  state and is out of scope. Stack:
  the runtime thread stack (`MEMORY_RUNTIME_THREAD_STACK_BYTES`) and `RUST_MIN_STACK` are both the
  stock 2 MiB and move together. Raising either adds a 2 MiB stack-budget re-run to `verify.sh`,
  as AGENTS requires. The binary and scripts read only `MEMORY_*` keys, never `DATABASE_URL`.
  Every `cargo sqlx` call passes `--no-dotenv` and an explicit `--database-url`; only the `prepare`
  invocation sets `DATABASE_URL`, taken from the verify database (D10).
- Modules (`src/`):
  - `domain/` — pure: ids, `AccessLevel`/label computation, grants and operations, observation and
    generation state machines, canonical idempotency form, bounds, packing, chunking over a token
    counter, redaction. No I/O crates.
  - `application/` — use cases (provisioning, save, correct, delete/purge, sources, sessions,
    search/timeline/fetch, context pack/validate, generation accept/status/cancel) and supervised
    workers (indexer, generation, reaper, sweeper). Ports exist only at real I/O boundaries:
    `MemoryStore` (PostgreSQL), `Embedder` (local model; deterministic fake in tests),
    `GenerationProvider` (Anthropic; counted fake in tests), `Clock`.
  - `adapters/` — `http/` (axum routes, auth extractor, error mapping, OpenAPI assembly), `mcp/`
    (rmcp handler), `postgres/` (the `MemoryStore` implementation, claims, sweeps), `embedding/`
    (fastembed + manifest validation), `anthropic/` (reqwest client).
  - `infra/` — CLI (`serve`, `migrate`, `admin …`, `openapi`), config, telemetry, startup, shutdown.
  `boundary-check.sh` fails if `domain/` or `application/` name `sqlx`, `axum`, `reqwest`, `rmcp`,
  `fastembed`, `ort` or `crate::adapters`.
- Dependencies, `=`-pinned where OpenBot `=`-pins them (OpenBot uses caret `"0.8"` for sqlx and
  `"0.10"` for secrecy; the service pins `sqlx =0.8.6` to match sqlx-cli 0.8.6), all resolved
  once into the committed lockfile:
  `axum 0.8`, `tokio 1`, `tokio-util 0.7`, `tower 0.5`, `tower-http 0.6` (trace, limit, timeout,
  request-id), `sqlx =0.8.6` (postgres, runtime-tokio-rustls, macros, migrate, uuid, chrono, json),
  `pgvector` (sqlx feature, `=` pin chosen at 02A), `serde`, `serde_json`, `uuid` (v4, v7), `chrono`,
  `thiserror`, `anyhow` (bin only), `tracing`, `tracing-subscriber` (json, env-filter),
  `secrecy 0.10`, `sha2 0.10`, `subtle 2`, `getrandom`, `base64 0.22`, `fastembed =7.1.1`,
  `ort =2.0.0-rc.13`, `tokenizers =0.23.2`, `reqwest =0.13.4`, `rmcp =1.8.0` (server, macros,
  schemars, transport-streamable-http-server), `schemars 1` (rmcp's), `jsonschema =0.33.0`.
  No git dependencies (the boundary check rejects any) and no `dotenvy`: `sqlx::test` and dotenv
  walk parent directories for `.env`, which would read OpenBot's configuration.
- Independence evidence: `boundary-check.sh` rejects path dependencies outside the directory (via
  `cargo metadata`), `open_bots`, `include_*!`/`#[path]`/`CARGO_MANIFEST_DIR` escaping `..`, and
  root paths in scripts and the Dockerfile. The poisoned-parent probe copies the service under a
  temporary parent that holds a `.env` (`DATABASE_URL` aimed at a closed port, `SQLX_OFFLINE=false`)
  and a `.cargo/config.toml` (`RUST_MIN_STACK = "1"`, `build.rustflags = ["--cfg",
  "memory_poison"]`). It asserts that `boundary-check.sh` fails naming the uncovered `build.rustflags`.
  With that key removed, it runs an offline build plus `memory_boundary_environment_probe`, which
  asserts the service's stack value, the absence of the `.env` values and the absence of
  `cfg(memory_poison)`. A leak fails the probe. The probe also plants a `src/domain/` file naming
  `sqlx::` and `crate::adapters` and asserts the layering rule reports both. `extracted-build.sh`
  copies only the tracked files (`git ls-files --cached services/memory`) plus the prepared
  `assets/model/` to a fresh temporary directory and runs `verify.sh` there, so untracked local
  files cannot help the build. `MEMORY_EXTRACT_UNTRACKED=1` adds untracked, non-ignored files,
  only for a local run before the service is first committed; CI never sets it. CI always runs
  from that extracted copy (D10). The service's own `.gitattributes` keeps the bundled model
  licence's CRLF bytes (`-text`), so the manifest digest matches every checkout.

## D2 Schema, migrations and test databases

One init migration `migrations/0001_init.sql`, edited in place under the policy above; a checksum
mismatch means resetting the service database. The extension is not created by the migration:
pgvector is not a trusted extension (`vector.control` has no `trusted`), so `provision-db.sh`
creates the role, database and `CREATE EXTENSION vector` as a superuser, and the migration starts
with a guard that raises unless `vector` ≥ 0.8.0 (iterative scans) is installed. `serve` migrates
at startup under sqlx's advisory lock, as OpenBot does.

Domain `access_level smallint CHECK (VALUE BETWEEN 0 AND 9)`. Ids are UUIDv7 (time-ordered).
Every tenant-owned table leads its key with `tenant_id`; every FK is composite and includes it.

| Table | Key columns and invariants |
| --- | --- |
| `service_settings` | Singleton: `embedding_model_id`, `vector_dims = 384`. Startup refuses a different configured model (reset policy). |
| `tenants` | `id`, unique opaque `external_id`, `state` ∈ active/disabled/deleted, `generation_daily_token_cap ≥ 0` (default 0 = off, operator-only), `managed_by` (the admin credential allowed to manage it), `disabled_by` (credential and scope). Never deleted: a deleted row is the tenant barrier. |
| `tenant_generation_schedule` | PK `tenant_id`, `last_claimed_at`: the fairness cursor, kept off the tenant row so claims never conflict with the writes' `FOR SHARE`. Inserted in the tenant-creation transaction. |
| `namespaces` | PK `(tenant_id, id)`, unique `(tenant_id, external_id)` (an opaque token, never an encoded client identity), `state` ∈ active/deleted. Never deleted: the tombstone is the namespace barrier; create refuses a tombstoned external id. |
| `admin_credentials` | `id`, `verifier bytea(32)`, `scope` ∈ operator/provisioner, `expires_at`, `revoked_at`. |
| `credentials` | PK `(tenant_id, id)`, unique `id`, `verifier`, `expires_at`, `revoked_at`. |
| `credential_grants` | PK `(tenant_id, credential_id, namespace_id)`; `read_max` access_level NULL = no read, `write_max` NULL = no write; `operations text[]` constrained `<@` the operation set. |
| `sessions` | PK `(tenant_id, id)`; FK `(tenant_id, namespace_id)`; unique `(tenant_id, namespace_id, external_id)`; open/closed. |
| `sources` | Immutable evidence revisions: unique `(tenant_id, namespace_id, external_ref, revision)`, `access_level`, `content_sha256`, `content` (only nullable by retention; the trigger freezes every other column, including `session_id`, `redaction_version` and `expires_at`), optional session FK `(tenant_id, namespace_id, session_id)`. |
| `observations` | PK `(tenant_id, id)`; FK namespace and same-namespace session; `kind`, `title`, `body`, `revision`, `access_level`, `producing_ceiling`, `watermark_kind` ∈ exact/ceiling_fallback, `watermark_level`, `source_max_level`, `origin` ∈ explicit/generated, unique `(tenant_id, generation_operation_id, output_ordinal)`, `occurred_at`, `expires_at`, generated `search_tsv`, indexing state (D7). CHECKs: `access_level >= watermark_level`, `access_level >= source_max_level`, `watermark_level <= producing_ceiling`, `watermark_kind = 'exact' OR watermark_level = producing_ceiling`. |
| `observation_links` | Source links: FK `(tenant_id, observation_id)`, exactly one of a source FK or a parent observation FK, `source_level`; cascade on delete. The source FK is `(tenant_id, source_id, source_level)` → `sources (tenant_id, id, access_level)`, so a link carries its source's real, immutable level; a parent-observation link records the parent's level at derivation. |
| `observation_parts` | PK `(tenant_id, observation_id, ordinal)`; byte range into the body, `namespace_id`, `access_level`, `revision`, `model_id`, `embedding vector(384)`. FK `(tenant_id, observation_id, namespace_id, access_level, revision)` → observations (unique key there), `ON DELETE CASCADE`: a part cannot carry a different label or a stale revision. |
| `generation_operations` | D9: `state`, `fence`, `lease_owner`, `lease_until`, `cancel_requested_at`, `spend_day`, `reserved_tokens`, the immutable request. Partial unique index `(tenant_id) WHERE state IN ('claimed','dispatched')`. |
| `tenant_spend_days` | PK `(tenant_id, day)`, `reserved ≥ 0`, `used ≥ 0`. |
| `idempotency_records` | PK `(tenant_id, kind, key)`, `request_sha256`, `result jsonb`, `status`, `request_expires_at`, `created_at`. |
| `audit_events` | Append-only (trigger rejects UPDATE): actor credential, action, target ids, outcome, `request_id` (the HTTP request id; NULL for CLI actions), bounded non-content details. |

Indexes: GIN on `observations.search_tsv`; btree `(tenant_id, namespace_id, access_level)` on
observations and parts; `(tenant_id, namespace_id, occurred_at, id)` for timelines; HNSW
`embedding vector_cosine_ops` (m 16, ef_construction 64); partial due-claim indexes on indexing
`next_index_at` and generation `next_attempt_at`; retention indexes on each expiry column.

Test databases: an in-crate harness (not `#[sqlx::test]`, see D1) reads only
`MEMORY_TEST_DATABASE_URL`, a maintenance-database URL for a superuser (locally and in CI). It
follows the production role path. Once per test binary it creates a LOGIN owner role
`memtest_<host6>_<pid>_owner` with a random password. Each DB test creates
`memtest_<host6>_<pid>_<uuid8>` owned by that role, creates the extension as the superuser, then
runs `sqlx::migrate!()` and the test itself as the owner, so a migration step that needs a
superuser fails in tests. The guard's `Drop` runs `DROP DATABASE … WITH (FORCE)` on a dedicated
thread with its own runtime and joins it, including during unwinding. At harness start, databases
and roles carrying this host's tag and a dead pid are dropped, which covers SIGKILLed runs.
An unset variable or unreachable server panics with setup instructions; there is no skip switch.
The role-denial test also needs CREATEROLE: it creates two roles and databases with
`REVOKE CONNECT … FROM PUBLIC` and asserts cross-connection and cross-table reads fail.
Never point it at a development or customer server.

## D3 Credentials and authorization

- Bearer tokens `msk_<kind>_<id>_<secret>`: `kind` ∈ `adm`/`cli`, `id` is the row UUID, `secret`
  is 32 random bytes in base64url. Stored verifier = SHA-256(secret), compared with `subtle`. A
  256-bit random secret needs no slow hash, and the lookup stays cheap at 20 req/s. Unknown id and
  wrong secret both return 401 `unauthenticated`; `credential_expired`/`credential_revoked` are
  returned only after the secret matched. The secret is shown once at issue time and never logged.
- Admin authority is separate: admin tokens reach only `/admin/v1` and the CLI, never `/v1` or
  `/mcp`. `operator` manages everything, including spend caps. `provisioner` (user decision
  2026-11-08: OpenBot's adapter holds one) may create, disable, enable and delete tenants;
  create and delete namespaces; and issue and revoke client credentials, only for tenants whose
  `managed_by` is itself. It cannot manage operator-created tenants, change spend caps or create
  admin credentials, and every action is audited. Stated honestly: it cannot read memory
  directly, but because it can mint a client credential with any grant for its tenants, it holds
  transitive full data authority over those tenants. Bootstrap: `memory-service admin bootstrap`
  writes the first operator credential directly to the database; it is refused only while a
  usable operator (neither revoked nor expired) exists, so it also recovers a service whose
  operators all expired. Every admin write re-checks the acting admin credential inside its
  transaction (`FOR SHARE`, D13), so a revocation or expiry that commits after authentication is
  never outrun. Tenant and namespace creation are idempotent by external id: a retry by an admin
  that may manage the live tenant returns its current view (audited as `*.create.noop`); tenant
  ids are one global namespace, so an id that is tombstoned or managed by another admin gets one
  uniform 409 `conflict` that does not say which (an existence signal admins cannot avoid). Credential
  and admin-credential issue are exempt from idempotency: the secret is shown once and only its
  verifier is stored, so no replay could return it; a retried issue mints a second credential,
  which the admin lists and revokes.
- Client tokens belong to one tenant. Grants are per namespace: operations plus `read_max` and
  `write_max`. Issue refuses unusable grants: read operations (search, timeline, fetch, context,
  generation_status, correct, delete) need `read_max`; write operations (save, correct, delete,
  sources, sessions, generate, purge) need `write_max`. Delete reads (an id above the ceiling is
  `not_found`) and writes; purge writes every level of the namespace, so granting `purge`
  grants that authority. `authorize` refuses a write on a grant without `write_max`.
  `write_max` may exceed `read_max` (a write-up grant, such as a capture client): a stored source
  revision above the caller's `read_max` is reused only by the credential that recorded it, with
  identical content (`sources.recorded_by`); for any other credential it is a uniform 409
  `conflict` whatever content is offered (P6b r2). `GET /v1/credential` returns the caller's own tenant id, grants and expiry, so a
  client can compute ceilings without out-of-band knowledge. Namespace delete removes that
  namespace's grants in the same transaction, so the endpoint lists only active namespaces. Requests carry no tenant field
  (schemas reject it). They name namespaces by external id and must carry `read_ceiling` on every
  read. An ungranted, nonexistent or deleted namespace gives the same 403 `forbidden`: the grant is
  checked first, and tombstones are visible only to admin create (409 `namespace_deleted`). A
  ceiling above `read_max` for any requested namespace is also 403. Narrowing means choosing a
  subset of namespaces or a lower ceiling; asking for more is refused rather than clamped.
- State changes take effect atomically (D13). Disable, delete and revoke commit together with the
  cancellation of every `accepted`/`claimed` operation they cover (tenant, namespace or authorizing
  credential): `cancelled(tenant_disabled|tenant_deleted|namespace_deleted|credential_revoked)`,
  with reservations released. `dispatched` operations are marked `cancel_requested_at` (D9).
  Enable only flips the state: it never revives an operation, and new inference needs a new
  authorized request. `tenants.disabled_by` records the disabling admin credential and its scope;
  re-disabling an already-disabled tenant is a no-op that keeps the recorded disabler, so an
  operator disable stays sticky through any provisioner disable-then-enable sequence.
  A provisioner may not enable a tenant an operator disabled (403, tested in
  `memory_authority_`). Reads check state in their own snapshot, so a read that started before a
  revoke committed may finish. Delete tombstones the tenant or namespace and the sweeper removes
  its content in bounded batches; tombstoned ids can never be recreated.

## D4 Access-level contract

- `read_ceiling` = the request value, which must be ≤ `read_max` for every namespace it names.
  Every read path filters `tenant_id`, `namespace_id = ANY(…)`, `access_level <= ceiling`, state
  and `expires_at` before ordering or limiting, including snippets, timeline neighbours, link
  metadata, counts and omission lists. Each read response returns `exposed_level`, the maximum
  level of what it returned (0 when empty).
- A save carries `context.producing_ceiling` (required; missing → 422 `classification_refused` with reason `missing_trusted_ceiling`)
  and `context.consumed` = `{"kind":"exact","level":c}` or `{"kind":"unknown"}`. It may also carry
  service source references (`source_id`, `revision`, or an observation id for copies) and
  client-attested `external_sources[{ref, level}]`. Service sources always name an explicit,
  immutable revision; there is no "latest". Computation:
  `watermark = exact ? c : producing_ceiling`;
  `save_level = max(watermark, all service source levels, all external levels)`.
  Refusals (422 `classification_refused` with a reason): `c > producing_ceiling`
  (`consumed_exceeds_ceiling`); any known source above `producing_ceiling` (`source_above_ceiling`);
  an optional `expected_access_level` that differs from the computed value
  (`inconsistent_classification`); each is 422 `classification_refused` with `details.reason`.
  `save_level > write_max` is 403. Service sources: recorded sources must be in the target
  namespace; parent observations (copies) may be in any namespace the credential holds with the
  `fetch` operation and a `read_max` at or above the parent's current level. A reference that
  fails this (foreign, missing, expired, deleted namespace, no read authority) is `not_found`,
  revealing nothing. A readable reference above `producing_ceiling` is `source_above_ceiling`, and a
  readable parent at another revision is `revision_conflict`. Exact 0 with no sources saves at 0.
- Persist `producing_ceiling`, `watermark_kind`, `watermark_level`, `source_max_level`,
  `carried_level` (a correction's retained prior level, when its own evidence derives less) and
  `access_level`; a CHECK requires `access_level = greatest(watermark, source_max, carried)`. The CHECKs in D2 make a fallback that is not the ceiling, or a label below its
  inputs, unrepresentable.
- A correction recomputes from its own context and stores `max(existing, computed)`. It requires
  `read_ceiling ≥ existing level`, `write_max ≥` the result and `expected_revision`. Copies and
  derived summaries take their parents as service sources. Deduplication never merges across
  levels or namespaces.
- The canonical idempotency request contains every input the label is derived from: producing
  ceiling, consumed evidence, explicit source revisions, whose levels are immutable, external
  levels and `expected_access_level`. The label is therefore a pure function of hashed fields, and
  the stored result carries it. A retry that would label differently is a conflict.
- Observation delete and correct, and any id above the caller's ceiling, return 404 `not_found`.
  Both are writes at the record's level: a visible record above the credential's `write_max` is
  403 (delete included, user decision for P3). Fetch returns provenance links only when their
  target passes the same filter (named namespace, active, current level within the ceiling, not
  expired); others are omitted without a marker. Timeline, context and search reuse it.

## D5 Idempotency

- Every write carries `idempotency_key` (1–128 bytes `[A-Za-z0-9._:-]`) and `expires_at`, at most
  7 days ahead. Scope: `(tenant_id, kind, key)`, where `kind` is the use case. The namespace is in
  the canonical request, so the same key in another namespace is a conflict.
- Canonical form: a dedicated versioned struct per use case, holding the validated, normalized
  request (defaults applied, namespace external ids) plus the host-supplied authority facts. It
  excludes the key, credential id and request id but includes `expires_at`: a retry must resend the
  original deadline, so a changed deadline is a conflict. It contains nothing resolved from
  mutable server state. It has no maps with unordered keys and is serialized with `serde_json` and
  hashed with SHA-256. Text is hashed byte for byte, without Unicode normalization. Identity is
  computed after privacy (P6a, review `p6a-code-r1` F1): wherever D9 redacts or drops input
  before capture (recorded source content; for generation acceptance, excluded events and
  `<private>` spans and secrets in event text and the description), the canonical request holds
  the filtered form, so the stored unsalted hash cannot confirm guesses about removed text.
  Requests that differ only inside removed text are the same request and replay.
- Order: authenticate, authorize the current credential (operation and namespace grants), take
  the D13 parent locks, then `INSERT … ON CONFLICT DO NOTHING RETURNING` the record. A concurrent
  identical save blocks on the uncommitted unique entry. If the winner commits, the loser's insert
  returns nothing; it re-reads under READ COMMITTED and returns the stored result
  (`replayed: true`), or 409 `idempotency_conflict` when the hash differs. If the winner rolls
  back, the loser proceeds. Only a new execution then checks `expires_at` (410 `request_expired`),
  derives labels and writes. The record, the observations and the links commit together. A replay
  after `expires_at` still returns the original result (the state at execution) and never
  recreates rows. A replay by a credential other than the executing one (stored in the record)
  first re-runs that credential's authority in a fresh execution's order: every cited parent must
  still exist and be readable by it (else `not_found`, exactly as a fresh request citing it, so a
  replay is no existence oracle), then every level in the result must be within its `write_max`
  (else 403). Request timestamps are truncated to microseconds, the stored precision, before
  they are validated, hashed or stored.
- Records are kept for 7 days and until `request_expires_at` has passed. Because the deadline is
  hashed, every attempt that can still execute carries the stored deadline: while it is in the
  future the record exists and replays; after it the sweeper may remove the record, and any later
  attempt carrying it is past its deadline and gets 410. So no accepted key can execute twice. Generation acceptance uses the same table: one key gives one operation.

## D6 HTTP, MCP, errors and bounds

- Client API `/v1` (POST JSON unless noted): `observations:save` (batch), `observations:search`,
  `observations:timeline`, `observations:fetch`, `observations/{id}:correct`,
  `observations/{id}:delete`, `context:pack`, `context:validate`, `sources:record`,
  `sessions:open`, `sessions/{id}:close`, `generations` (request), `GET generations/{id}`,
  `generations/{id}:cancel`, `namespaces/{ns}:purge`, `GET credential`. Admin `/admin/v1`: tenants
  (create/disable/enable/delete/settings), namespaces (create/delete), credentials
  (issue/list/revoke). Ops: `/healthz`, `/readyz` (02A); metrics are 06A.
- MCP: one operation registry generates both the HTTP routes and the MCP tools, so every client use
  case except admin is both a route and a tool (`memory_save`, `memory_search`, … `memory_purge_namespace`),
  with identical grant checks. Transport: rmcp 1.8.0 streamable HTTP server mounted at `/mcp` in the
  same axum app, `stateful_mode: false`, `json_response: true`, behind the same bearer-auth layer;
  the handler reads the principal from the `http::request::Parts` rmcp injects. rmcp's
  `allowed_hosts` defaults to loopback only, so `MEMORY_MCP_ALLOWED_HOSTS` and
  `MEMORY_MCP_ALLOWED_ORIGINS` (default empty: no browser origins) are explicit configuration
  keys. Startup refuses a non-loopback bind address with an empty host list. OpenBot already
  speaks this transport as an rmcp client. rmcp collects bodies without a limit, so the `/mcp`
  gate reads the body itself under 4 MiB + 64 KiB of JSON-RPC framing (413 envelope) and runs
  authentication and the exchange under the request deadline (504 envelope). Each tool call is
  then held to its operation's body limit and, because rmcp runs it on a task of its own, to the
  request deadline where it runs (`payload_too_large`/`timeout` tool errors).
- OpenAPI: request/response types derive `schemars` 1.x (already required by rmcp tool schemas).
  `adapters/http/openapi.rs` assembles OpenAPI 3.1 from them. `memory-service openapi` writes it,
  and the committed `openapi.json` is checked by `memory_contract_openapi_is_current`. Examples in
  `contract/examples/` are validated with `jsonschema` against both the OpenAPI components and the
  MCP `inputSchema`. No utoipa: a second schema generator would let the HTTP and MCP schemas drift.
  The schemas publish the bounds below (`domain/bounds.rs` through schemars `length`/`range`/
  `regex`); `maxLength` counts characters, and byte limits are stated in field descriptions.
- Errors: `{"error":{"code","message","retryable","details"},"request_id"}`. Empty is 200 with
  `items: []`. 400 `invalid_request`; 413 `payload_too_large`; 401 as in D3; 403 `forbidden`
  (also any ungranted, unknown or deleted namespace), `tenant_disabled`, `generation_disabled`;
  404 `not_found` (foreign, nonexistent and above-ceiling are identical); batch fetch/validate
  report such ids as `unavailable` entries; 409 `idempotency_conflict`, `revision_conflict`,
  admin-only `namespace_deleted`; 410 `request_expired`; 422 `classification_refused`; 429
  `capacity_exceeded` with Retry-After, `spend_exhausted`; 503 `unavailable` (database, model,
  not ready, deadlock retries exhausted); 504 `timeout`. MCP returns the same body as a tool
  error result.
- Bounds (code maxima; config may only lower): body 256 KiB, 4 MiB on save/sources/generations;
  title ≤ 256 chars/1 KiB; body ≤ 16,384 chars/64 KiB; metadata ≤ 4 KiB; save ≤ 256 items,
  ≤ 64 sources each; namespaces per request ≤ 16; query ≤ 1,024 chars; search limit default 20,
  max 100; snippet ≤ 320 bytes; timeline ≤ 20 each side (default 5); fetch ≤ 64 ids and ≤ 2 MiB
  (overflow → `omitted_for_budget`); context `max_tokens` 1–65,536 (default 8,192), `max_bytes`
  ≤ 256 KiB (default 64 KiB), ≤ 200 candidates; source content ≤ 256 KiB; 64 in-flight HTTP
  requests (then 429); DB pool 16; embedding: 2 blocking threads, one of them reserved for query
  embeddings (a query waits at most 1 s for a permit, then 429); statement timeout 2 s for reads,
  5 s for writes; request deadline 10 s. Generation bounds are in D9, retention in D9/06A.

## D7 Retrieval

- Keyword leg: `observations.search_tsv = to_tsvector('simple', title || ' ' || body)`, the same
  multilingual, unstemmed choice OpenBot's knowledge search makes; it keeps exact identifiers.
  The query takes at most 32 lexemes of `to_tsvector('simple', q)` (in query order). It matches
  observations holding all of them, or any identifier-like raw token of the query (split on
  whitespace, trimmed of punctuation, holding a digit, one of `_ . @ / : -` inside, or a capital
  right after a lower-case letter; at most 16) as `phraseto_tsquery('simple',
  token)`, so `PgStore`, `idempotency_key`, `foo.bar.baz`, `ops@example.com`, `ERR_CONN_RESET`
  and `v1.2.3` match inside ordinary sentences. All-word matches rank first, then `ts_rank_cd`
  over any lexeme. P4 decision change (review `p4-code-r1` F1): `'simple'` keeps function words,
  so the OR of every lexeme matched almost every row, drowned the semantic leg in RRF and made the
  leg scan the tenant; a function-word-only query now matches only rows holding all its words.
  Bare capitalised words are not identifiers (review `p4-code-r2` F1a: `IT` or a Caps-Lock query
  became one-word phrases matching 74–94% of 100,001 rows in 254–302 ms), so both clauses stay
  GIN-selective. Keyword recall covers pending or failed-index records.
- Semantic leg: e5 `query: `/`passage: ` prefixes, mean pooling, L2 normalization (self-check norm
  within 0.001), cosine `<=>`. Chunking counts with the model's own tokenizer, with no padding or
  truncation, as in OpenBot: split paragraphs, then sentences, then token offsets; target ≤ 320
  tokens, hard limit 500 including prefix and specials (model maximum 512); no overlap; at most
  128 parts per observation. A part over the limit is a bug and fails indexing; text is never
  silently truncated.
- Both legs: `leg_candidates = min(4 × limit, 200)` observations, with the D4 predicate in each
  leg's WHERE. The vector leg runs in its own transaction with one `SET LOCAL` block:
  `hnsw.iterative_scan = strict_order`, `hnsw.ef_search = clamp(nearest_parts, 40, 1000)`,
  `hnsw.max_scan_tuples = 20000`, `hnsw.scan_mem_multiplier = 2`, with `nearest_parts =
  2 × leg_candidates`. Parts are grouped by observation (best distance). The leg is short when
  it yields fewer than `leg_candidates` observations, whatever the cause. A short leg whose scan
  returned exactly `nearest_parts` parts is retried with ×4 parts, up to `leg_candidates × 128`
  (Claude-Mem's approach). Any other short result, including one where the scan stopped at
  `max_scan_tuples` with fewer parts than its LIMIT, is inconclusive and goes to the exact scan.
  The exact scan first takes a bounded count of eligible parts (`LIMIT 100001`). At ≤ 100,000 it
  orders a `MATERIALIZED` CTE over the same predicate by distance, which is the true answer.
  Above that, the response says `semantic: "partial"` and a metric increments. It is never
  silently short.
- Fusion: RRF `Σ 1/(60 + rank)`; ties broken by `occurred_at` desc, then id. Snippet: the best part's
  byte range, else `ts_headline` (1 fragment, ≤ 40 words) on the final ≤ 100 rows only.
- Indexing: a save commits `index_state = pending`. The indexer claims up to 8 due rows in one
  `FOR UPDATE SKIP LOCKED` statement, with a 60 s lease and an `index_fence` increment. It renews
  the lease every 20 s under the fence and stops work at a local deadline (D13, the same rule as
  generation). It embeds through the bounded embedding pool (D6) and commits parts only if fence
  and revision still match (a correction deletes the parts and resets the state in its own
  transaction). Failure: `failed` with backoff 30 s × 2ⁿ up to 1 h; after 8 attempts it stays
  terminal-failed and visible (`index_state` in fetch/search) until an admin reindex. A query
  embedding failure is 503, not a keyword-only answer. Missing model or extension fails startup.

## D8 Context packing

- Candidates come from the D7 search (or recency when there is no query or it is blank), up to
  200. Order: fused score desc, `occurred_at` desc, id asc (recency: `occurred_at` desc, id asc).
  Greedy whole-item inclusion: an item that does not fit the remaining token or byte budget is
  skipped and the scan continues. Omission is reported as counts only (`omitted: {budget: n}`),
  with no ids or levels, so it exposes nothing; only visible candidates are ever counted.
  `detail` ∈ full/compact (default compact) picks body or snippet for all items. No partial bodies.
- Token count follows OpenBot's provider-independent upper bound: `2 × UTF-8 bytes of the item's
  serialized JSON`, plus 64 for the pack (an empty pack counts 0). `byte_count` is the exact sum
  of the items' compact JSON serializations as served; it excludes the `items` array's n + 1
  bracket and comma bytes and the envelope fields, and the token bound holds for the service's
  encoding (non-ASCII unescaped), not for a client's re-encoding. It is deterministic and composes with OpenBot's
  history budget. Under this rule tokens bind before bytes unless `max_bytes < max_tokens / 2`.
  The defaults (8,192 tokens, about 4 KiB of JSON; 64 KiB) make the token budget the working
  limit, and the byte cap is the backstop for callers that raise it.
- Items: id, namespace, kind, title, body/snippet (`text`), occurred_at, access_level, revision,
  content_sha256 (as fetch defines it, whatever `detail` is), visible source refs and
  `sources_sha256` (SHA-256 of their compact JSON as served). The pack
  also returns `exposed_level` (maximum level of the included items and their provenance, never
  the requested ceiling or a skipped candidate), `token_count`, `byte_count`, `omitted`,
  `candidates_truncated` (the 200-candidate window was full: a leg filled its window, more visible
  candidates were fused than kept, or a 201st recent row exists), `pack_digest` (SHA-256 over the
  service's compact serialization of the item list; clients compare it, they do not recompute
  it), `valid_until` (minimum expiry of the included items and of their visible provenance
  targets), `generated_at` and `semantic`. A `full` pack is selected on exact served sizes
  first (the body's JSON-string size is `octet_length(to_json(body)::text)`, which escapes as
  `serde_json` does), then only the selected bodies are loaded at the selected revisions; a body
  changed in between is left out and selection reruns on what was loaded. Text is
  returned as JSON data. The service builds no prompt text or roles.
- `context:validate` takes `[{id, revision, sources_sha256}]` (1–200, answered in request order)
  and a ceiling
  and returns valid/changed/expired/unavailable per item, without content, plus `exposed_level`
  (the maximum current level among items reported valid, changed or expired) and `checked_at`.
  `changed` means a new revision or a different visible provenance (a link's target expired, was
  deleted, or left the ceiling or the named namespaces).
  Deleted, foreign, unnamed-namespace and above-ceiling items are all `unavailable`; `expired`
  is reported only for an item otherwise visible, until the sweeper removes it. A revoked
  credential gets 401 and a narrowed grant 403, as on every read.

## D9 Generation

- Envelope (`sources:record` plus `generations`): ordered events `{event_id, source_ref, revision,
  occurred_at, access_level, role ∈ user/assistant/tool/note, text}`. At most 512 events and
  512 KiB per window, in one namespace and one optional session. Request fields: `window`
  (complete; events or recorded source revisions), `namespace`, `producing_ceiling`, `consumed`
  (exact/unknown, as in D4), `allowance_tokens`, `max_output_tokens` ≤ 8,192, `expires_at`
  ≤ 7 days, `idempotency_key`, optional `summary: true`. Input level =
  `max(watermark, all event/source levels)`. Checked synchronously at acceptance: no event above
  `producing_ceiling`; input level ≤ `write_max`; cap > 0 (otherwise `generation_disabled`);
  reservation estimate ≤ `allowance_tokens`.
- Provider destination (user decision 2026-11-08): every level a request is authorized for may go
  to Anthropic, with no extra per-tenant level gate. There is no zero-data-retention or pinned
  inference-region requirement for now, so Anthropic's standard data handling applies; the service
  documentation states this limitation. The operator's daily token cap (default 0) remains the
  switch that keeps generation off.
- Privacy before durable capture: drop events marked `exclude`; strip `<private>…</private>`
  spans; replace secrets matched by a versioned pattern set (API-key prefixes, PEM private keys,
  credentials after `Authorization:` and credential-shaped `Bearer`/`Basic` tokens — at least 16
  characters with a digit or `+ / = . _ -`, so prose like "basic configuration" stays —
  and `password=`-style pairs unless the whole value already is a marker) with
  `[REDACTED:<kind>]`, recording the redaction version. The same pure function runs again on
  loaded text before dispatch. Identifiers (event ids, source refs, revisions) are stored or sent
  as given, so one the function would change is refused (400 `secret_in_identifier`). Idempotency
  hashes the filtered request (D5). The untrusted `description` carries no level of its own: it
  is covered by the request's `consumed` attestation. Logs carry ids, counts and codes only.
- States: `accepted → claimed → dispatched → completed(saved|skipped) | failed(code) | cancelled |
  expired | unknown`. Claim (lock order in D13): read due `accepted` candidates without locks,
  ordered by `tenant_generation_schedule.last_claimed_at NULLS FIRST`, then `created_at`
  (round-robin). Per candidate: tenant/namespace/credential `FOR SHARE SKIP LOCKED` (a tenant
  being disabled is skipped), then the schedule row and the operation `FOR UPDATE SKIP LOCKED`,
  re-checking `state = 'accepted'`. Then set `claimed`, `lease_owner`, `lease_until = now() + 90 s`
  and `fence = fence + 1`, and update the schedule row. The partial unique index is the backstop:
  a second in-flight claim for a tenant fails, and the loser rolls back a savepoint and moves to
  the next tenant. This gives 4 workers with at most 1 in flight per tenant.
- Spend in the claim transaction: `reserve = 2 × prompt bytes + 4,096 + max_output_tokens`. Insert
  the UTC day's `tenant_spend_days` row `ON CONFLICT DO NOTHING`, then
  `UPDATE … SET reserved = reserved + r WHERE reserved + used + r <= cap`. Zero rows fails the
  operation `spend_cap_reached`, terminally: a new day never revives it. The operation records
  `spend_day`, and settlement always targets that day. Settlement replaces the reservation with
  `usage.input_tokens + cache_creation_input_tokens + cache_read_input_tokens + output_tokens`.
  Usage above the reservation is still recorded (the cap can be overshot by at most that excess)
  and increments `generation_usage_exceeded_reservation_total`. That metric is the early-failing
  signal that the reserve formula no longer bounds the structured-output overhead, and a
  `memory_generation_` test covers it.
- Dispatch: compare-and-swap `claimed → dispatched` under fence and lease, committed before the HTTP
  send, after re-checking tenant, credential, namespace and expiry. Liveness:
  - A heartbeat every 30 s extends the lease under the fence. Its CAS also requires
    `cancel_requested_at IS NULL`.
  - The worker keeps a local deadline: the monotonic instant the last successful renewal was sent,
    plus 90 s − 15 s. The deadline always falls before the database's `lease_until`, with no clock
    skew involved.
  - The provider future races the deadline and any failed heartbeat CAS. When either fires, the
    worker drops the future, which aborts the connection, and settles the operation (`unknown`,
    or `cancelled` when a cancel was requested). The provider timeout of 120 s is longer than the
    lease, so this abort is what bounds a call, not the timeout.
  - The reaper frees a `dispatched` slot only after `lease_until + 30 s`. By then any live worker
    has aborted, so at most one provider call per tenant is in flight.
- Retry rules: a post-send failure counts as a definite, safe rejection only when Anthropic's error
  envelope (`{"type":"error","error":{"type": rate_limit_error | overloaded_error | api_error}}`)
  arrives with a `request-id`. Such an operation goes back to `accepted`, its reservation is
  released, and it backs off 30 s × 2ⁿ, at most 3 attempts. Anthropic `invalid_request_error`,
  `authentication_error`, `permission_error`, `not_found_error`, `request_too_large` and
  billing/quota errors are terminal failures. Anything else after the send (no response, a timeout,
  a reset, a gateway 5xx without the envelope, a crash) is `unknown`: the reservation is charged
  and the request is never re-sent. The reaper also turns expired `claimed` leases back into
  `accepted` (nothing was sent, nothing is spent) and past-expiry `accepted` rows into `expired`.
- Cancel: `generations/{id}:cancel`, disable, delete and revoke move `accepted`/`claimed` straight
  to `cancelled`. Any later dispatch CAS then fails, so nothing is sent. On `dispatched` they set
  `cancel_requested_at`. The worker aborts at its next heartbeat, or the reaper acts after
  `lease_until + 30 s`. The state becomes `cancelled` with the reservation charged and any output
  discarded, and the slot is freed only at that point.
- Commit: tenant/namespace/credential `FOR SHARE`, then the operation `FOR UPDATE`, comparing fence
  and state and requiring no `cancel_requested_at`. Then validate the output and insert
  observations keyed by `(generation_operation_id, output_ordinal)`, their links, the spend
  settlement and completion. If a re-check fails, the output is discarded and the state is
  `cancelled` (spend stays charged). Every output item gets the operation's input level: the whole
  window was exposed.
- Provider: raw HTTP (no official Rust SDK) `POST {base_url}/v1/messages`, `x-api-key`,
  `anthropic-version: 2026-10-21`, model `claude-sonnet-5-5`.
  - No temperature, top_p or top_k (400 on that version). Effort `low` (configurable among
    `low`/`medium`/`high`). Thinking off: `thinking: {"type": "between_tools"}`, checked against
    the claude-api reference (P6a): on `claude-sonnet-5-5` omitting `thinking` runs adaptive
    thinking inside `max_tokens`, `{type: "disabled"}` is a 400, and `between_tools` takes no
    other field and is refused above effort `high`, so the adapter refuses such an effort at
    construction. `max_output_tokens` is then wholly available to the answer.
  - No `fallbacks`, so content never reaches a model other than the configured one (the
    reference recommends `fallbacks: "default"`; it would route windows to another model, so a
    `refusal` stays the terminal `provider_refused`).
  - Structured output via `output_config.format` (JSON schema), not forced tool use:
    `tool_choice` `tool`/`any` returns 400 on `claude-sonnet-5-5`. The schema sent is
    provider-safe: `{outcome: save|nothing_to_save, observations: [{kind ∈
    fact/decision/discovery/fix/outcome, title, body, event_ids}], summary}`, with
    `additionalProperties: false` and every property required on every object (`summary` nullable),
    no length, count or numeric constraints, and no level or namespace fields.
  - A stricter local `jsonschema` then enforces ≤ 16 observations, the D6 title/body limits and
    `event_ids` ⊆ the window. `max_tokens` → `output_truncated`; `refusal` → `provider_refused`;
    an invalid response → `malformed_output`; a valid `nothing_to_save` → `completed(skipped)`.
  - The Messages API has no idempotency key or status lookup. Message Batches has status lookup,
    but creating a batch is itself ambiguous on timeout and results can take up to 24 h, so it is
    not used. Ambiguity therefore stays `unknown`.
- Tests: a `#[cfg(test)]` counted `GenerationProvider` (scripted outcomes, delays, recorded
  requests, reported usage) and, for black-box runs, a local fake Anthropic HTTP server (configured
  `MEMORY_ANTHROPIC_BASE_URL`) that counts requests. The production binary contains no test
  provider; CI makes no paid calls.
- Retention sweeper: one task, batches of 500 per category per tick (locking in D13).
  A full batch is followed by another after a 100 ms yield; an error backs off 1 s → 5 min with
  jitter. Raw evidence, source content and generation requests are nulled 30 days after the
  referencing operation is terminal; evidence no operation references is nulled 30 days after it
  was recorded, or at its own earlier expiry. A null-out never touches a source referenced by a
  non-terminal operation: it checks under its row lock. Acceptance locks the window's recorded
  sources `FOR SHARE` and refuses one already nulled. If the load before dispatch still finds
  window content missing, the operation fails `source_unavailable` and nothing is sent; an
  incomplete window is never sent. Identity, revision, level and digest stay for
  provenance. Idempotency: D5. Audit: 400 days. Tombstones (opaque ids only): kept forever.
  Observations: until deleted or corrected (no revision history is kept), or their own
  `expires_at`.

## D10 Verification map

Tests live in the crate (`src/**/*_tests.rs` and `#[cfg(test)]` modules, like OpenBot), named with
the batch prefix. `scripts/focused.sh <prefix>` runs `cargo test --locked --offline <prefix> --
--list`, counts `: test` lines, fails on zero, then runs the selection and prints both counts;
results are recorded in the phase file. DB-backed selections run under the service's
`test-network-isolation.sh`. Every concurrency case uses competing tasks on separate connections.

| Prefix | Location | Deliverables proved |
| --- | --- | --- |
| `memory_contract_` | `adapters/http/contract_tests.rs`, `adapters/mcp/contract_tests.rs` | D6 schemas/examples, OpenAPI freshness, error taxonomy, bounds, two tenants, non-OpenBot client, `GET /v1/credential` |
| `memory_authority_` | `application/authority_tests.rs` (DB) | D3 narrowing, uniform namespace 403, foreign ids; disable/enable, delete and revoke racing competing claimers, writers and commits (no revival, no deadlock leak) |
| `memory_access_level_` | `domain/access_level_tests.rs` + `application/access_level_tests.rs` (DB, HTTP+MCP) | D4 table rows, all ten levels, forged/replayed labels, missing ceiling |
| `memory_startup_` | `infra/startup_tests.rs` | bad config, missing model/extension, migration failure, MCP host refusal, readiness, shutdown |
| `memory_boundary_` | `infra/boundary_tests.rs` + `scripts/boundary-check.sh` + `extracted-build.sh` | D1 independence, poisoned-parent environment probe, root `[env]` coverage, role-denial test |
| `memory_storage_` | `adapters/postgres/storage_tests.rs` (DB) | D2 FKs/CHECKs, owner-role migration, correction and delete races, vector cleanup, competing indexers, indexer heartbeat loss |
| `memory_idempotency_` | `adapters/postgres/idempotency_tests.rs` (DB) | D5 concurrent identical saves, conflicts, label conflicts, replay after source retention |
| `memory_search_` | `domain/retrieval_tests.rs`, `application/embedding.rs` (pool), `adapters/postgres/search_tests.rs` (DB, fake embedder) + one real-model acceptance test | D7 corpus cases; > 20,000 foreign and above-ceiling parts nearer the query than a small eligible set still return the eligible rows; partial flag above the exact cap |
| `memory_recall_` / `memory_context_` / `memory_mcp_` | `adapters/postgres/recall_tests.rs` (DB; HTTP and MCP; the independent Python client `contract/client.py`), `domain/packing_tests.rs`, `adapters/http/contract_tests.rs` | D6/D8, count-only omission |
| `memory_observation_` | `domain/generation_tests.rs`, `application/observation_tests.rs`, `adapters/anthropic/tests.rs` (local fake Messages API on loopback) | D9 envelope, redaction, labels, provider-safe schema plus local bounds, the counted provider's recorded requests, the adapter's request and error mapping |
| `memory_generation_` | `adapters/postgres/generation_tests.rs` (DB, counted provider) | D9/D13 competing claimants, fairness, leases and fences; a stalled heartbeat against a slow provider never yields two in-flight calls per tenant; dispatched cancel; reaper vs heartbeat; delete vs claim/commit; envelope-only retries; usage above reservation; competing sweepers; source retention racing acceptance/dispatch (`source_unavailable`, nothing sent) |

`verify.sh` is the service's full gate, run from the service directory (in CI, the extracted copy).
- Database: it requires `MEMORY_TEST_DATABASE_URL` (a superuser maintenance URL) and refuses to run
  when that is unset, even if `DATABASE_URL` is set. It creates a disposable owner role and
  `memverify_<pid>_<rand>` database, creates the extension as the superuser, and runs
  `cargo sqlx migrate run` and `prepare --check -- --all-targets` as the owner. Both pass
  `--no-dotenv --database-url <owner url>` with sqlx-cli 0.8.6 invoked as `cargo sqlx` (its
  `prepare` refuses any other invocation; CI puts the installed binaries on `PATH`; the script
  fails if the version differs), and a trap drops the database and role on exit. The test-suite
  step runs `test-network-isolation.sh`, which first runs `network-isolation.py`: loopback must
  work, external IPv4/IPv6 must be refused, and the `MEMORY_TEST_DATABASE_URL` server must be
  reachable. On Linux it admits a loopback-published container by conntrack original destination
  (127.0.0.1:5432), as OpenBot's script does. It never touches OpenBot's or any
  development database.
- Order: fmt check; `git diff --check` when inside a git tree; boundary check; `cargo fetch
  --locked`; offline check and clippy `-D warnings`; the two sqlx steps; `prepare-model.py
  --verify`; `test --no-run`; the full suite under network isolation.
- CI: new `.github/workflows/memory-service.yml` extracts with `extracted-build.sh`, pins the
  toolchain to 1.95.0, and runs a PostgreSQL 18 service image carrying pgvector 0.8.7, the version
  used in development, pinned by tag and digest at 02A. It caches model assets by manifest hash and
  runs `verify.sh`. It then builds the service image and runs `check-image.py`, which starts the
  image as non-root with `--network none --read-only` and runs `memory-service model-check`
  (manifest validation plus embedding self-check, no database).
- OpenBot's `ci.yml` is unchanged until 05. The image check runs in CI; locally it is recorded as
  unavailable (no Docker).

## D11 Phase 05 seam assumptions (service side only)

- Tenant `external_id` = OpenBot company UUID. Namespaces must be opaque tokens, for example a
  random UUID OpenBot stores per company/agent/user scope; never readable scope names. The
  service never parses them, and its barriers keep only these opaque ids.
- OpenBot supplies `producing_ceiling = RunKnowledge::ceiling()` and `consumed exact =
  content()` when its accounting is complete, otherwise `unknown`. It sends one `read_ceiling`
  = min(run ceiling, audience, grant), with grants read from `GET /v1/credential`, and raises
  `content_level` by each response's `exposed_level` before exposure.
- Contract reconciliation for 05A.3:
  - `memory.save` (256 facts × 16,384 chars, i.e. 4–16 MiB) exceeds one 4 MiB save. The adapter
    splits the frozen fact list deterministically: greedy in fact order, closing a batch before it
    would exceed 3.5 MiB serialized or 256 items. Batch keys are `<base>.<i>-<n>`, using only the
    key charset; the base key is ≤ 112 bytes, so the suffix (≤ 8 bytes) keeps it within 128.
    Every batch carries the identical frozen `producing_ceiling`, `consumed` and source set from the
    delivery intent, so all batches get the same label and a retry rebuilds identical requests.
    Single-request atomicity is lost and must be surfaced: a partial failure reports the saved ids,
    and a retry completes the remaining batches.
  - `memory.load` allows `limit` up to 256 against the search maximum of 100, and a `query` of
    16,384 chars against the 1,024-char limit. 05 must shrink the contract or refuse; it may not
    silently truncate. Fetch details in batches of 64.
- Delivery intents reuse one idempotency key and must finish within the request's `expires_at`
  (≤ 7 days); after that the key is refused, not re-executed.
- Pack token counts use OpenBot's own `2 × JSON bytes` bound; `context:validate` and
  `pack_digest` support frozen-preparation replay checks.
- OpenBot's adapter holds one `provisioner` admin credential (D3). Through it, it has transitive
  data authority over the tenants it created. It keeps that credential away from agents, as 01B.5
  requires for client credentials. Company suspend/delete maps to tenant disable/delete; scope
  removal maps to namespace purge.

## D12 Decisions, open requirements and routine choices

User decisions 2026-11-08 (recorded, applied above): provider levels are unrestricted, so any level
the request is authorized to read may go to Anthropic (D9); no zero-data-retention or pinned
inference region for now, recorded as a documented limitation; OpenBot's adapter holds one
provisioner credential, with the authority stated in D3. Earlier decisions (provider and model,
embedding pin, ops defaults) are as given in the P1 assignment and applied in D2, D7 and D9.

Still open: the named budget for 06's paid-provider smoke check, deferred to 06 as planned.
Resolved 2026-11-10 (user, U9 in COORDINATION I003 r5): 50,000 tokens, enforced through the smoke
tenant's daily generation cap. Model `claude-sonnet-5-5`, the service default. The key is a test
key from `MEMORY_SMOKE_ANTHROPIC_API_KEY`, read only by `scripts/memory-paid-smoke.sh`.

Routine choices resolved here, noted for reviewers: forced tool use replaced by structured outputs
(the model rejects forced `tool_choice`); search refuses a ceiling above any requested namespace's
grant instead of computing per-namespace minima; `expires_at` is required on writes, so tombstones
plus the 7-day idempotency window stop late deliveries; corrections overwrite without keeping text
history; all client use cases are also MCP tools; no per-tenant provider-level setting is added.

Environment notes (2026-11-08, this host): rustc/cargo 1.96.0 (`rust-toolchain.toml` will select
1.95.0, which needs a one-time download); `cargo sqlx` is 0.9.0, so `verify.sh` needs 0.8.6
installed separately (P2 round 1: installed under the untracked `tmp/memory-service-run/tools`
and put first on `PATH`); Homebrew PostgreSQL 18 with pgvector 0.8.7, `psql` not on PATH
(`/opt/homebrew/opt/postgresql@18/bin`, so scripts take `MEMORY_PSQL`), password required, no
`MEMORY_TEST_DATABASE_URL` yet; Docker not installed, so image checks are unavailable locally.

## D13 Lock order and contention

One global order for every transaction: acting admin credential row (admin writes, `FOR SHARE`;
an admin-credential revoke then updates its target row, so two operators revoking each other can
deadlock and are retried) → tenant row → namespace row → credential row (and its grants) → `tenant_generation_schedule` row → operation rows (id order) → idempotency record →
content rows (observations → parts → links → sources; id order within each) →
`tenant_spend_days` row. Audit rows are insert-only. A transaction may skip levels but never
acquires upward.
- Client memory writes (02B: save, copy, correct, delete): the exact order is in the P3
  implementation notes; cited parents' namespaces follow the target namespace, shared only.
- Writers, commits (generation and indexer), the dispatch CAS and the claim take the parent rows
  `FOR SHARE` before any child row.
- Tenant disable and delete: tenant `FOR NO KEY UPDATE`, then operations in id order. Every
  cancellation (disable, delete, namespace delete, revoke) first locks the covered
  accepted/claimed/dispatched rows `ORDER BY id FOR NO KEY UPDATE`, then cancels or flags them,
  zeroing `reserved_tokens` on cancelled rows, and releases spend rows last in day order.
  Admin namespace delete: tenant `FOR SHARE`, then namespace `FOR NO KEY UPDATE`, its grants and
  its operations. Client purge: tenant `FOR SHARE`, namespace `FOR NO KEY UPDATE`, calling
  credential `FOR SHARE`, then operations. Credential issue and grant changes: tenant `FOR SHARE`,
  then the granted namespaces `FOR SHARE` in id order, re-checking they are active before the
  grants are inserted. A racing namespace delete either commits first (the issue is refused) or
  waits and then removes the grant. Revoke: tenant `FOR SHARE`, credential
  `FOR NO KEY UPDATE`, then operations.
- The heartbeat and the indexer claim are single-statement and single-level: they hold nothing
  else and cannot form a cycle. The reaper locks an operation and then that operation's spend row,
  which is downward and in order. The sweeper selects `ctid … FOR UPDATE SKIP LOCKED LIMIT 500`
  (`FOR NO KEY UPDATE` for content null-outs) and then deletes or nulls only those rows. Cascades
  descend from observations to parts and links. Competing sweepers therefore skip each other
  instead of deadlocking.
- A deadlock (`40P01`) or serialization failure (`40001`) retries the whole transaction up to 3
  times with 10–100 ms jitter, then returns 503 `unavailable` (workers back off instead).
  `memory_authority_` and `memory_generation_` race delete against claim and commit to prove it.
- Workers repeat the tenant, credential, namespace and expiry checks at claim, at dispatch and at
  commit, so a state change committed earlier is always seen.

## P2 implementation notes (01A, 01B, 02A)

Deviations from the record above, with reasons:
- HTTP paths use segments instead of `resource:verb` (`/v1/observations/save`), and ids travel in
  the body (`observation_id`, `operation_id`). The router does not take a `{param}:verb` segment,
  and body-only operations make each MCP tool argument object identical to its HTTP body.
- Added `501 not_implemented` to the D6 taxonomy. Declared-but-unimplemented operations validate
  their body, then return it (`x-memory-implemented: false` in `openapi.json`); never a fake success.
- `GET /v1/credential` is also the MCP tool `memory_credential`.
- The CI image check is inline in `memory-service.yml` (non-root user, `--network none
  --read-only`, `version`, explicit refusal without a database). `check-image.py` and
  `memory-service model-check` arrive with the model in 03A.
- Transport contract tests use a `#[cfg(test)]` store double (`application/test_store.rs`) holding
  one credential. Every storage and authority behaviour is tested against PostgreSQL only.
- The bundled model licence/notice files are tracked under `assets/bundled/`, with digests checked
  against the manifest. Model files are not downloaded yet (`prepare-model.py --download`, 03A).

P2 correction round 1 (review `p2-code-r1`), resolution by finding:
- F1 fixed: `services/memory/.gitattributes` marks `assets/bundled/MODEL-LICENSE.txt`
  `-text whitespace=cr-at-eol`. Staged into a scratch index, the blob hashes to the manifest's
  `904dc4…`, and `git diff --cached --check` is clean.
- F2 fixed as described in D6; `memory_mcp_` covers the oversized body (413), the per-tool limit
  and both deadlines.
- F3 fixed in code (conntrack rule and `network-isolation.py`, D10). The Linux path is proved
  only by the first CI run, which cannot run here.
- m1 fixed (D3, D13). m2 fixed (D3). m3 fixed (D13). m4 fixed (D2): source trigger, label FK and
  `audit_events.request_id`, which every admin write now fills from the HTTP request id.
- m5 fixed (D3); the classification is pinned by `memory_authority_operation_classification_*`.
- m6 fixed (D6). Declared-but-unimplemented operations still only decode their body (serde), so
  a request over a published bound gets 501, not 400, until its use case lands.
- m7 and m12 fixed (D3): idempotent creates by external id, uniform `conflict`, and the issue
  exemption. `admin bootstrap --label` without a value or with a bad label now exits with the
  usage or the label rule before touching the database.
- m8 fixed: tracked-only extraction with the explicit opt-in (D1); the layering rule is
  enforced and probed (D1); `verify.sh` and `provision-db.sh` split the URL so the database path
  is optional. R3 residual nit: any array-valued parent key now fails.
- m9 decided: compile-time-checked queries. Every PgStore statement is a `sqlx::query*!` macro
  and `.sqlx/` holds their metadata (34 queries), so `prepare --check` is no longer vacuous: it
  fails when a statement or the schema changes without regenerating. Domain-typed
  (`access_level`) columns are cast to `smallint` in and out because the macros check only the
  base type. Test fixtures (ad hoc claimers, raw schema probes) stay runtime `sqlx::query`; the
  DB-backed suite is what proves them. Regenerate after any SQL change with
  `SQLX_OFFLINE=false DATABASE_URL=<migrated owner URL> cargo sqlx prepare --no-dotenv
  --database-url <same> -- --all-targets`. The macros read a `.env` (current or parent
  directory) only for keys the environment lacks; `.cargo/config.toml` always sets
  `SQLX_OFFLINE=true`, so a parent `DATABASE_URL` is never used, and the poisoned-parent probe
  builds the macros offline under such a `.env`.
- m10 fixed: `tower-http` removed; `tower` is a dev-dependency (`util`, for `oneshot`).
- m11 fixed: the 501 loop asserts that every client operation has a contract example (17 now,
  one per client operation plus admin issue), and calls each unimplemented one over HTTP and MCP.
  New success-path cases cover `set_tenant_cap`, `list_credentials`, `revoke_admin_credential`
  and repeated namespace delete. `memory_authority_disable_races_competing_claimers_without_revival`
  races ad hoc claimer SQL that follows the D9 claim order, not production claim code (04B). It
  proves that disable leaves no live operation, not the D10 claimer protocol; `memory_generation_`
  carries that proof.

Handoff state (P2 round 1): the DB-backed suite ran against PostgreSQL 18.6 with pgvector 0.8.7
and passed, and `verify.sh` and `extracted-build.sh` passed locally. Results are in phases 01/02.
CI (the verify job on Linux, F3, and the image job) has not run.

## P3 implementation notes (02B)

Implemented: `sources.record`, `sessions.open`/`close`, `observations.save` (explicit saves and
classification-preserving copies), `observations.fetch`, `observations.correct` and
`observations.delete`, on HTTP and MCP. The application layer (`application/memory.rs`)
validates the D6 bounds, authorizes the current grants and hashes the canonical request. The
store (`adapters/postgres/memory.rs`, `MemoryStore` port) re-checks state under the D13 parent
locks, claims the idempotency record, resolves sources, derives the label (`derive_label`),
writes the rows, and commits the result with a client audit event. Search, timeline, context,
generation and purge stay `501` until their points (03–06).

Decisions and deviations, with reasons:
- Client writes lock tenant, namespace and the calling credential `FOR SHARE`, then re-check
  them: the tenant is active (disabled gives `tenant_disabled`, deleted `credential_revoked`),
  the namespace is active (otherwise 403, since its delete removed the grant), and the
  credential is neither revoked nor expired. They then claim the idempotency record and lock
  content rows. Actual order (D13): save/copy — tenant, target namespace, credential `FOR SHARE`;
  idempotency record; the other namespaces holding cited parents `FOR SHARE` in id order
  (re-checked active); parent observations `FOR SHARE` in id order; target-namespace sources `FOR
  SHARE` in id order; inserts. Correct — the same parents; idempotency record; target
  observation `FOR NO KEY UPDATE`; cited namespaces, parents and sources as above; then parts and
  links deleted, the row updated, links inserted. Delete — the same parents; idempotency record;
  target observation `FOR UPDATE` (it is removed, so no later lock upgrade); delete with cascades.
  Cited namespaces come after the target namespace rather than in one id order; all namespace
  locks taken by writes are shared and no transaction locks two namespaces exclusively, so this
  cannot close a cycle. Two corrections citing each other's targets can deadlock and are retried.
- Canonical request (D5): the typed, validated request without `idempotency_key`, wrapped as
  `{v: 1, kind, request}` and hashed with SHA-256 (P3 round 1: `expires_at` is included, review
  `p3-code-r1` F2). A replay returns the stored result with `replayed: true`: the result at
  execution, as 01A.6 requires. A replay after the deadline still replays; a new execution past
  it gives 410 and leaves no record.
- Cross-namespace citation (review `p3-code-r1` F1/F3/m2): recorded sources are evidence of the
  target namespace only, so source links never cross namespaces and a namespace purge deletes its
  observations (cascading their links) before its sources. Parent observations may be cited
  across namespaces only with `fetch` authority there; parent links are `ON DELETE CASCADE`, so
  deleting or sweeping a parent never blocks on its copies and leaves no dangling id, and a copy
  keeps its own label. A deleted namespace hides its parents from citation and provenance at once.
- Label refusals are 422 `classification_refused` with `details.reason` (`missing_trusted_ceiling`,
  `consumed_exceeds_ceiling`, `source_above_ceiling`, `inconsistent_classification`). A label
  above `write_max` is 403.
- Correction stores `max(existing, recomputed)`, recording a retained prior level in
  `carried_level`, and refuses a target or a result above `write_max`. It cannot cite itself (400). It replaces the
  observation's links with those of its own context. It deletes its searchable parts and
  requeues indexing (`index_state = pending`, fence + 1). Stale context references are detected
  by the revision bump (`context:validate`, 03B).
- Delete removes the row. Parts (vectors) and its links cascade, and so do other observations'
  links naming it as a parent; a copy keeps its own label. A new delete of a gone id is
  `not_found`.
- Schema: `observation_links.parent_revision` (non-null exactly for parent links) records the
  revision a copy read, so fetch can return `SourceRef::Observation` exactly. A partial index on
  parent links supports the cascade.
- `content_sha256` in details is lowercase hex SHA-256 over title, NUL, body. Stored text cannot
  contain NUL, so text fields with NUL are refused with 400.
- Sessions are idempotent by external id: a second open returns the same session. Closing is
  idempotent. A closed session still accepts saves and sources (summaries after close).
  Re-recording a source revision identically returns the same id; other content, level, session,
  `occurred_at` or `expires_at` is a 409 `conflict`. `idempotency_records` keeps `credential_id`
  instead of the unused `status` column.
- Sources were stored as given until P6a; since then `sources.record` stores the D9 redacted
  content (`redaction_version` 1). The idempotency identity stays the request as sent.
- Fetch reads in one `REPEATABLE READ, READ ONLY` transaction, in requested-id order (duplicate
  ids are collapsed). It stops adding details past 2 MiB (`omitted_for_budget`).
- Searchable parts: 02B never creates them. It only deletes them (correction, cascade) and sets
  the indexing state; 03A's indexer fills them under the fence.

## P4 implementation notes (P3 minors, 03A)

P3 recheck minors (review `p3-code-r2`):
- n1: every write request's `expires_at` description says the value is part of the request
  identity, a retry must resend it unchanged and a changed value is `idempotency_conflict`
  (`memory_contract_write_deadlines_publish_their_retry_identity` reads both schemas).
- n2: request timestamps are truncated to microseconds before validation, hashing and storage
  (D5), so a sub-microsecond re-record names the same source and the same key replays.
- n3: a replay by another credential answers `not_found` when a cited parent is missing,
  expired, in an inactive namespace or unreadable, as a fresh citation does; then 403 for a level
  above `write_max` (D5).

03A as implemented, with deviations and reasons:
- Layout: `domain/retrieval.rs` (constants, chunker, vector validation, the vector-leg plan, RRF),
  `application/embedding.rs` (the pool), `application/indexer.rs` (the worker),
  `application/search.rs` (the use case), `adapters/embedding/` (manifest validation, the local
  model, `model-check`), `adapters/postgres/search.rs` (both legs, the indexing state machine).
  The DB-backed tests live beside the other DB tests in `adapters/postgres/`, because the D1
  layering rule forbids `sqlx` anywhere under `application/`.
- No `pgvector` crate: vectors are bound as `real[]` and cast to `vector` in SQL (written parts as
  their text form), so no new driver dependency is needed. Model crates as OpenBot pins them:
  `fastembed =7.1.1` (`ort-load-dynamic`), `ort =2.0.0-rc.13`, `tokenizers =0.23.2`; `libc` for
  the peak-memory report. Dev builds optimize `sha2`, `tokenizers`, `fastembed`, `ndarray` and
  `serde_json` (validation hashes ~490 MB); release is unchanged.
- Keyword leg: see D7 (all lexemes, or identifier-like raw tokens as phrases). The real-model
  acceptance check failed with the OR form: function words ranked an unrelated long note first.
- P4 correction round 1 (review `p4-code-r1`): F1 as in D7, probed on the keyword leg with the
  reviewer's six sentences and a function-word guard. F2: kept the 1,024-character bound and the
  500-token refusal (400 `invalid_request`, never truncated); the query description says so and
  `memory_contract_search_refuses_a_query_over_the_model_token_limit` shows 1,024 CJK
  characters refused on HTTP and MCP. Lowering the bound would cut every Latin-script query to
  ~500 characters for a CJK edge case. F3: a pass observes shutdown (the embedding in flight is
  cancelled, nothing committed), `run` joins the worker within what remains of the one
  `MEMORY_SHUTDOWN_GRACE_MS` after the server drain (review `p4-code-r2`) and aborts it after, and `supervise` restarts a panicked or stopped worker with backoff (1 s
  doubling to 30 s). Notes taken: model files are read once and verified in memory (what was
  hashed is what loads; the runtime library is still loaded from its checked path), and the
  claim locks `FOR NO KEY UPDATE`.
- Parts: the embedded text is `passage: {title}\n\n{part}` for every part, counted with the
  model's own tokenizer (no padding or truncation). Part packing targets 320 tokens, raised to
  `title + 192` (at most 500) for long titles so a full body fits in 128 parts. A blank body has
  one title-only part `0..0` (the CHECK allows exactly that). Over 500 tokens, more than 128
  parts or an invalid vector fail indexing with a bounded `index_error` code.
- Snippet: the best part's text at the part's revision, as its keyword fragment when the query's
  words occur in it, else `ts_headline` over the body; at most 40 words and 320 bytes.
- Provenance: search items carry `sources` through fetch's D4 filter (`provenance` is shared),
  and `exposed_level` includes the current level of every returned link target. Fetch now counts
  link targets too (more conservative than P3).
- Indexing state machine: claim is one statement (`FOR UPDATE OF o SKIP LOCKED`, active tenant and
  namespace, unexpired, due, attempts < 8), fence + 1 and attempt + 1; the same statement turns a
  final attempt whose lease expired into `failed(lease_expired)`. Commit locks tenant and namespace
  `FOR SHARE`, then the observation `FOR NO KEY UPDATE`, and writes only when fence and revision
  match (`Stale` otherwise); a disabled tenant, deleted namespace or expired row defers the job
  and refunds the attempt. Failure: backoff 30 s × 2ⁿ⁻¹ up to 1 h; the 8th failure is terminal
  until `memory-service admin reindex-failed` (audited as `index.reindex_failed`).
- Embedding pool: two model sessions, not one session with two threads: an ONNX Runtime session
  runs one inference at a time (`run(&mut self)`), so a reserved query thread needs its own
  session. Each session has one intra-op thread. Budget: peak RSS 2.34 GB (2,228 MiB) in
  `model-check` on macOS arm64 after the read-once load (dev build, 2026-11-08; 2.58 GB before);
  the CI image job fails above 2,560 MiB (recalibrated down from 3,072; the first Linux run is
  calibration). A single shared session would halve memory at the cost of queries waiting behind
  an indexing batch; that trade-off is open for the operator.
- `exact_scan_cap` is a store setting (code maximum 100,000; tests lower it); `max_scan_tuples` is
  the fixed 20,000. The partial counter is in-process (`semantic_partial_total`) until 06A exports
  metrics. Search statements run with a 2 s statement timeout.
- Startup: connect, migrate, load and self-check the model (`MEMORY_MODEL_DIR`, default
  `assets/model`), require pgvector >= 0.8.0 and a `vector(384)` column, adopt the model id in
  `service_settings` or refuse another one, then serve with the supervised indexer.
  `memory-service model-check` needs no database (CI image job, `--network none --read-only`).
- Open: a long note with many parts can outrank a short, exactly relevant one in the semantic
  leg (best-of-parts); the acceptance corpus avoids generic filler. Quality tuning is 06's load
  and relevance work.

## P5 implementation notes (03B)

Implemented: `observations.timeline`, `context.pack` and `context.validate` on HTTP and MCP from
the one registry (`application/recall.rs`, `adapters/postgres/recall.rs`, `domain/packing.rs`).
Search and fetch were already in place; the 03B.1 index was checked as is.

Decisions and deviations, with reasons:
- Timeline: the anchor must pass the D4 predicate, otherwise 404 `not_found` (identical for
  foreign, missing, deleted, expired, unnamed-namespace and above-ceiling anchors). Neighbours come
  from the named namespaces (a client names one to stay inside it), at most `before`/`after` ≤ 20
  each side by (`occurred_at`, id), with the predicate inside each side's WHERE ahead of its
  LIMIT, so restricted rows take no slot; `session_only` keeps the anchor's session (an anchor
  without one has no neighbours). The response is the anchor plus neighbours oldest first and
  `exposed_level`, with no counts, "more" flags or gap markers. Snippets are the body's first
  320 bytes. One REPEATABLE READ, READ ONLY snapshot with the 2 s statement timeout.
- Shared filters: search, timeline and pack rows go through one recall row shape
  (`RecallRow`), and provenance through the existing `provenance` filter (D4), so a link target
  above the ceiling, in an unnamed or deleted namespace or expired is omitted and a visible one
  raises `exposed_level` to its current level. The search pipeline was refactored into
  `recall_once` (search with limit ≤ 100; packs with 200), which also returns the pack's body or
  snippet and a content digest computed in SQL with fetch's definition (tested equal).
- Headlines: a compact query pack computes `ts_headline` on at most 200 final rows, not D7's 100
  (the unused `MAX_HEADLINE_ROWS`); it stays bounded by the 200-candidate cap and the 2 s
  statement timeout.
- `valid_until` and `context.validate` cover provenance since P6a (review `p5-code-r1` m1, see
  D8).
- Audit: reads (search, timeline, fetch, pack, validate) write no audit events on either
  transport, as before for search and fetch; writes audit identically on both. Write tools need
  `write_max` (D3), so a read-only credential is refused `memory_save` with 403 on both.
- The second client is `contract/client.py`, standard-library Python that reads operation paths
  from `openapi.json` and tool names from `tools/list`, checks each request against the published
  request schema's fields, and runs over real TCP against a served router. The DB test that runs
  it (`memory_recall_independent_python_client_…`) fails, never skips, without `python3`.
- Recall DB tests live in `adapters/postgres/recall_tests.rs`, because the D1 layering rule
  forbids `sqlx` under `application/`.

## P6a implementation notes (P5 minors, 04A)

P5 review minors (`p5-code-r1`):
- m1: pack items carry `sources_sha256` (SHA-256 of their visible `sources` as served), and
  `context.validate` takes it back and answers `changed` when the current visible provenance
  differs: a source expired, a parent was deleted, raised above the ceiling or left the named or
  active namespaces. `valid_until` is the earliest expiry of the items and their visible
  provenance targets (the shared provenance filter now returns each target's expiry).
- m2: a `full` pack selects on exact served sizes before loading any body. The body's
  JSON-string size comes from `octet_length(to_json(body)::text)`, which escapes as
  `serde_json` does (tested with quotes, backslashes, control characters, U+2028, emoji and
  accents), then only the selected bodies are loaded at the selected revisions (at most the
  byte budget). A body corrected in between is left out and selection reruns on what loaded, so
  both budgets always hold.
- m3: D8 and the field descriptions say `byte_count` excludes the array's n + 1 bracket and
  comma bytes and the envelope, and that the token bound holds for the service's encoding.

04A as implemented, with deviations and reasons:
- Layout: `domain/generation.rs` (privacy, the provider-safe schema, bounded output validation,
  the reservation), `application/generation.rs` (`prepare` and `generate`), the
  `GenerationProvider` port in `application/ports.rs`, `application/test_provider.rs` (the
  counted, scripted test provider; test builds only) and `adapters/anthropic/` (raw HTTP with
  `reqwest =0.13.4`, rustls, no proxy; `regex` for the redaction patterns).
- Scope split with the coordinator: P6a stops before durable acceptance. `generations.request`,
  `generations.status` and `generations.cancel` stay `501`: accepting a request now would
  promise execution that only 04B's workers provide. Tests run `prepare` and `generate`
  synchronously through the use case. Acceptance storage, recording the window as source
  revisions, recorded-source windows, claims, leases, fences, spend and the commit of generated
  observations are P6b.
- Configuration: the adapter takes `AnthropicSettings` (base URL, key, model
  `claude-sonnet-5-5`, effort `low`, 120 s timeout); nothing reads `MEMORY_ANTHROPIC_*` yet,
  so no key is documented. P6b wires `MEMORY_ANTHROPIC_API_KEY` and `MEMORY_ANTHROPIC_BASE_URL`
  into startup with the workers, and documents them only then.
- Window rules (`prepare`): 1–512 events, at most 512 KiB of text, unique event ids,
  non-decreasing `occurred_at` (400 `events_out_of_order`), one text and level per
  `(source_ref, revision)` in the window (409 `conflict`), an optional untrusted
  `description` of at most 1 KiB, `max_output_tokens` 1–8,192, and a reservation within the
  allowance (400 `allowance_below_reservation`, with `reserve_tokens`). A past deadline is 410.
  A summary needs its `session_id` (400 `summary_without_session`); it is generated only from
  this window, cites the whole window and carries the window's floor.
- Privacy: excluded events and events left empty by `<private>` stripping are dropped (never
  stored or sent) and do not raise the label; redacted events still count at their level.
  Every event, excluded or not, above the producing ceiling refuses the request
  (`source_above_ceiling`). The redaction function is idempotent and runs again in
  `provider_request`, on whatever is about to be sent, including the description.
- Labels: `derive_label` over the producing ceiling, the consumed evidence (exact or the
  ceiling fallback) and every sent event's level; all outputs share that level. The model sees
  no levels, namespaces, session ids, source refs or keys, and output naming any field outside
  the schema (for example `access_level`) is `malformed_output`.
- Outcomes: valid `nothing_to_save` is a skip; empty text is `empty_output`, anything else
  invalid `malformed_output`, `max_tokens` is `output_truncated`, `refusal` is
  `provider_refused`. Nothing left after privacy is a skip without a call. A destination whose
  `max_level` is below the input level is refused before any call (Anthropic: level 9, user
  decision). Provider errors: no connection → retryable `not_sent`; Anthropic's envelope with a
  `request-id` → retryable for `rate_limit_error`/`overloaded_error`/`api_error`, terminal
  otherwise; anything else after the send → `unknown`, never re-sent.

P6a correction round 1 (review `p6a-code-r1`), resolution by finding:
- F1 fixed: `sources.record` refuses secret-shaped identifiers and redacts content before the
  request is hashed, so `request_sha256` is over the redacted canonical request (D5, D9). Test
  `memory_observation_source_identity_is_hashed_after_privacy`: two requests differing only in a
  `<private>` span and a `password=` value replay, the stored hash equals the redacted request's
  canonical hash, other visible text is `idempotency_conflict`. Checked for other raw-derived
  persistence: client audit details are counts, revisions and levels; logs carry codes and
  provider error types; the database error log is the driver's message, not row values.
- m1 fixed: `thinking: {"type": "between_tools"}` sent, effort limited to low/medium/high
  (D9); the exact-body test and a construction refusal for `xhigh`/`max` cover it.
- m2 fixed: `Authorization:` anchored and credential-shaped `Bearer`/`Basic` patterns; prose
  negatives ("the basic configuration", "the bearer of bad news", "Bearer tokens expire
  hourly", …) are tested unchanged.
- m3 fixed: the `secret` pattern skips only a value that is exactly a marker;
  `token=abc[REDACTED:x]def` is redacted.
- m4 fixed: secret-shaped event ids, source refs and revisions are refused in generation
  windows and source records; the description's level rule is stated in D9 and its field doc.

P6b handoff (batch 04B; what exists, where, and what P6b must do):
- Exists: `domain/generation.rs` (redaction, `holds_secret`, provider schema, output
  validation, `reservation`), `application/generation.rs` (`prepare` → `PreparedGeneration`
  with the label, filtered events, `reserve_tokens`; `provider_request`; `generate` →
  `Generated { outcome, usage }`), the `GenerationProvider` port and `ProviderError` retry
  classes in `application/ports.rs`, `application/test_provider.rs` (counted, scripted),
  `adapters/anthropic/` (request, mapping, `AnthropicSettings`), and the
  `generation_operations`/`tenant_spend_days`/`tenant_generation_schedule` tables in
  `migrations/0001_init.sql`. `generations.request/status/cancel` are still `501` in
  `application/operations.rs`.
- Must do: accept in one transaction (D5 order; canonical identity over the prepared, filtered
  request per D5; the window recorded as source revisions in the target namespace with
  conflicts as `sources.record` defines them; recorded-source windows checked against stored
  sources, sessions and retention; summaries limited to their session's sources); claim,
  lease, fence, heartbeat, local deadline, dispatch CAS and reaper per D9/D13; spend reserve and
  settlement per D9 with `generation_usage_exceeded_reservation_total`; re-check tenant,
  credential, namespace and expiry at claim, dispatch and commit; commit generated observations
  (`origin = generated`, `(operation, output_ordinal)`, source links from `GeneratedItem.sources`,
  label = the frozen `input_level`) with completion in one transaction; cancel and expiry;
  wire `MEMORY_ANTHROPIC_API_KEY`/`MEMORY_ANTHROPIC_BASE_URL` (and effort) into config and
  startup, documenting only what startup reads; flip the three operations to implemented with
  examples; the `memory_generation_` checks listed in 04B and D10. Lint/boundary note: the
  lockfile now carries `aws-lc-sys` (rustls), first proved by the CI image job.

## P6b implementation notes (P6a recheck minors, 04B)

P6a recheck (review `p6a-code-r2`): m1 the `secret` pattern takes a `[A-Za-z0-9]+[_-]` key
prefix (`DB_PASSWORD=`, `client_secret:`, `OPENAI_API_KEY=`, `AWS_SECRET_ACCESS_KEY=`, the whole
key kept) and a URL-userinfo pattern redacts `scheme://user:password@`; n1 a token never ends in
`.`/`-` and Bearer/Basic (≥ 16 bytes) and `Authorization:` (≥ 8) values need a digit or `+ / =`;
n2 the identifier field docs say keyword-shaped ids are refused; n3 a quoted value takes what
follows it up to the next separator, so a second pass changes nothing; n4 `AnthropicProvider`
refuses any model but `claude-sonnet-5-5` (the model is not configurable); n5 secret-shaped
revisions are tested in both windows and `sources.record`.

04B as implemented, with deviations and reasons:
- Layout: `adapters/postgres/generation.rs` (acceptance, status, cancel, claim, dispatch,
  renew, settle, reap, sweep), `application/generation.rs` (the three use cases, the canonical
  acceptance identity, `restore`), `application/generation_worker.rs` (workers, maintenance),
  the `GenerationStore` port, `adapters/postgres/generation_tests.rs` (`memory_generation_`).
- Window storage: the window's text lives once, in `sources`. Each kept event's `(source_ref,
  revision)` is recorded in the target namespace (session = the request's, content = the
  redacted text, `occurred_at` = its first event's) or matched with an identical stored
  revision: same redacted digest, level and session (`occurred_at`/`expires_at` are not
  compared: an event's time is not its revision's). Another digest, level or session is 409
  `conflict`; a nulled or expired stored revision is 409 with `details.reason =
  source_unavailable`. Revisions are processed in `(ref, revision)` order so concurrent
  acceptances lock alike. Deviation from "events or recorded source revisions": there are no
  text-less events; a window names a recorded revision by carrying its identical text. The
  summary-session check is the session comparison: a summary's window can only hold its own
  session's revisions. `generation_operation_sources` links an operation to its revisions
  (dispatch load, retention); `request` holds `{v, description, events[{event_id, source_id,
  role, occurred_at}]}`.
- Schema (edited in place): operation `session_id`, `summary`, `source_max_level` (CHECK
  `input_level = greatest(watermark, source_max)`), `reservation` (the D9 estimate, fixed at
  acceptance), `inputs_nulled_at`, `dispatched_at`; CHECKs tie the lease to claimed/dispatched,
  `finished_at` to terminal states and a null request to retention. A trigger freezes identity,
  label, reservation and request (it may only become NULL) and refuses any change to a
  finished operation, so a classification change during a retry and a revived operation are
  unrepresentable. Generated observations reference `(tenant, namespace, operation)`.
- Acceptance (D5 order): writer locks, idempotency claim, cap check (0 → 403
  `generation_disabled` reason `daily_cap_zero`; a reservation above the whole cap → 429
  `spend_exhausted`), session, window revisions, operation, links, result and audit. No
  configured provider → `generation_disabled` reason `provider_not_configured`. The identity
  hashes the prepared request (kept events and redacted text and description; D5); a past
  deadline still replays and only a new execution gets 410. A privacy-only window is stored as
  `completed(nothing_to_process)` and never dispatched. A replay returns the acceptance result.
- Status takes `read_ceiling` (D3: every read names one); an operation above it is
  `not_found`. Cancel needs `generate` and a `write_max` at the operation's level (else
  `not_found`), locks the operation before its idempotency record (D13), and reuses
  `cancel_operations` (scope `Operation`). Every flagged dispatched row now keeps its reason in
  `outcome_code` until settled; status shows an outcome only for terminal states.
- Claim: candidates as D9; per candidate one transaction with tenant/namespace/credential
  `FOR SHARE SKIP LOCKED`, the grant, the schedule row and the operation `FOR UPDATE SKIP
  LOCKED`. A credential that expired or lost `generate`/`write_max` is cancelled
  (`credential_expired`, `authority_changed`), past-deadline work `expired`. The claim update
  runs before the spend reservation; a partial-unique-index violation gives up the candidate.
  The same re-checks run, blocking, at dispatch and at commit.
- Worker: claim → dispatch CAS (commits `dispatched`, `attempts + 1`, before the send) →
  heartbeat task (aborted with the pass, so an aborted worker never renews) → the call races the
  local deadline, a failed renewal and shutdown. Settlement: saved/skipped/failed charge the
  reported usage; `unknown` (`provider_unknown`, `lease_lost`, `deadline`, `shutdown`) charges
  the reservation; a definite rejection or `not_sent` releases it and backs off 30 s × 2ⁿ⁻¹ up
  to 3 dispatches, then `failed(<type>)`; a cancel requested meanwhile ends `cancelled` with what
  ran charged. A settlement write that fails is retried from memory while the lease holds; after
  that the reaper records `unknown(lease_expired)` and the output is lost (surfaced, never
  re-sent). Shutdown before dispatch gives the claim back (`accepted`, nothing spent).
- Maintenance: one supervised task runs the reaper (expired claims → `accepted`; dispatched past
  `lease_until + 30 s` → `unknown`/`cancelled`, charged; pending past deadline → `expired`) and the
  sweeper, 500 rows per category per tick, 100 ms after a full category, else every 5 s; errors
  back off 1 s doubling to 5 min with up to 25 % jitter. "Full" counts rows actually changed.
- Retention (06A.1 for generation inputs and raw evidence; idempotency, audit and tombstones
  stay with 06A): a request is nulled 30 days after its operation finished; source content when
  no unfinished operation uses it and (it expired, or it was recorded 30+ days ago and no
  operation using it finished in the last 30 days). The sweep locks candidates `FOR NO KEY
  UPDATE SKIP LOCKED`, then nulls them in a second statement whose fresh snapshot re-checks the
  live reference, which closes the race with an acceptance that commits between the candidate
  scan and the lock.
- Configuration: `MEMORY_ANTHROPIC_API_KEY` (optional; absent → generation disabled and no
  worker), `MEMORY_ANTHROPIC_BASE_URL` (https, or http to loopback), `MEMORY_ANTHROPIC_EFFORT`,
  `MEMORY_GENERATION_WORKERS` (1–4). Startup validates their shape and never contacts the
  provider; the README says so. `supervise` now names its worker.
- Not done here (06A): exported metrics (`generation_usage_exceeded_reservation_total` is an
  in-process counter like `semantic_partial_total`), idempotency/audit retention, purge.

P6b correction round 1 (review `p6b-code-r1`), resolution by finding:
- M1 fixed with real competing tests on separate connections:
  `memory_generation_stalled_heartbeat_with_a_slow_provider_never_doubles_calls` (a provider
  slower than the lease, renewals blocked by another connection's row lock and failing on a
  100 ms `lock_timeout`, a second worker with a second same-tenant operation and the reaper at
  grace 0 racing throughout; evidence: lock waits sampled in `pg_stat_activity`, the call
  dropped by the local deadline, the slot not reaped while locked, `max_in_flight = 1`, and the
  second call starting only after the first was dropped), `…_reaper_racing_live_heartbeats_…`
  (two reapers ticking every 10 ms against 150 ms renewals for 2.5 leases: nothing reaped,
  nothing dropped, saved once) and `…_competing_sweepers_split_the_work_exactly` (four sweepers,
  1,200 candidates, counts sum exactly, no error).
- m1 fixed: the source sweep's candidate scan uses the null-out's whole predicate, so evidence
  kept by a recently finished operation never fills the batch (test with 600 such rows ahead of
  an expired one).
- n1 fixed: a claim candidate that errors is logged and skipped; the error is returned only
  when no candidate was claimed. n4 fixed: claim-time stops write the `generation.settle`
  system audit row. n5 fixed: `save_output` inserts observations, then links (whose foreign
  keys key-share the cited sources), without a separate source lock, in D13 content order.
- n3 fixed within the single grace: a call already dispatched at shutdown may finish within
  80 % of `MEMORY_SHUTDOWN_GRACE_MS` (no new claim starts); after that it is dropped and
  settled `unknown(shutdown)` with the remaining 20 % before the join aborts the worker.
- n6 fixed: D3 allows `write_max` above `read_max`. A stored revision above the caller's
  `read_max` for the namespace is a uniform 409 `conflict` on `sources.record` and on
  acceptance, whatever the content offered, so a write-up grant cannot confirm guesses. Bound
  that remains: a 409 still says that some revision exists at that `(ref, revision)`, which
  unique identifiers cannot avoid. Round 2 (review `p6b-code-r2` m2) narrows it: `sources`
  records `recorded_by` (frozen by the trigger), and the credential that recorded a revision
  reuses it with identical content (record then generate, overlapping windows, re-recording
  under a new key); every other caller above its read grant keeps the uniform conflict.

P6b correction round 2 (review `p6b-code-r2`):
- M1: the stalled-heartbeat test now fails renewals by a test-only store fault
  (`PgStore::renew_fault`, `cfg(test)`), touching no row, so the reaper (grace 0) and a second
  worker run free. It asserts that the drop is observed before the database's `lease_until`
  (`clock_timestamp()`), `unknown(deadline)`, `max_in_flight = 1` and `drops_at_start = [0, 1]`.
  A new reaper-boundary test asserts that a tick freeing a dispatched slot ends after
  `lease_until + grace` by the database clock. The reviewer's mutations, re-run on scratch
  copies with a scratch database (both removed afterwards): A (local deadline after the lease)
  fails the stalled-heartbeat test; D (reaper 0.5 s early) fails the boundary test; A + D fails
  both.
- m2 fixed as above, with tests for the recording credential and a probing one.

P6b correction round 3 (review `p6b-code-r3`): F1, a flaky gate, fixed in the stalled-heartbeat
test: the faulty store gets its own `renew_fault` flag (a clone shared the `Arc` and made the
competitors' renewals fail too), and the competitor and reaper start only after the first
dispatch. Carried to 06A's credential-rotation documentation: a rotated write-up credential
loses reuse of revisions its predecessor recorded above its read grant; it fails safe with the
uniform 409, and the client records new revisions instead.

P6b correction round 4 (review `p6b-code-r4` F2): the reaper-boundary and reaper-vs-heartbeat
tests measure by the database clock instead of counting ticks, so machine load cannot fail them
while a reaper acting early still does (mutation D re-checked in a separate target).
- n2 left for 06A purge: a `Saved` settlement whose parent rows are missing saves today; that
  is unreachable while tenants and namespaces are tombstoned, and 06A's purge must treat it
  as a stop.
- Carried: the black-box `serve` run against the fake Messages API (06B.1); the
  `UNIQUE (tenant_id, idempotency_key)` decision once idempotency records are swept (06A); an
  authentication circuit breaker (06A provider-outage handling).

## Verification record

2026-11-08, record creation (this file plus its startup-warning path in `src/main.rs`):
`cargo fmt --all -- --check` passed; `git diff --check` passed; inline audit passed (15 local
links resolve, policy note present, path registered once, clean whitespace and final newline).
No Rust, database, model or container suite ran: documentation-only change, no service code yet.

2026-11-08, correction round 1 (review `p1-arch-r1`: M1–M6 fixed, m1–m14 resolved, user decisions
applied): `cargo fmt --all -- --check` passed; `git diff --check` passed, and the same check on
the untracked record via `--no-index` was clean; inline audit passed (15 local links resolve,
policy note present, path registered once, D13 present, clean whitespace and final newline).
Documentation only; no Rust, database, model or container suite ran.

2026-11-08, correction round 2 (review `p1-arch-r2` n1–n5 fixed): `git diff --check` passed; the
`--no-index` whitespace check on the record was clean; the inline audit passed.

## Phase 05 decisions (OpenBot integration)

Expansion for [05](05-openbot-integration.md) (P7a, preparation; no OpenBot source changed), traced
at base `a6bed48a` with the service tree unchanged; `path:line` anchors refer to that tree. OpenBot
uses only `services/memory/openapi.json` over HTTP: no service crate, no shared pool, no corpus.
Correction round 1 (review `p7a-arch-r1`) and the user decisions in E12 are applied below.

### E1 Ports, adapter, configuration and credentials

- Ports in the consuming layer (`src/application/memory/ports.rs`, new): `MemoryClient`
  (credential, search, timeline, fetch, context pack/validate, save, generations request) and
  `MemoryProvisioner` (tenant create/disable/enable/delete, namespace create/delete, credential
  issue/list/revoke), typed after those OpenAPI components only. One error enum (`Unauthorized`,
  `Forbidden(code)`, `NotFound`, `Conflict(code)`, `Expired`, `Refused(reason)`,
  `Capacity{retry_after}`, `Unavailable`, `Ambiguous`: a write that may have run, retried only
  with its key); no default methods. Workflow orchestration and the pure scope/ceiling/label
  decisions live in `src/application/workflow/memory/`.
- Adapter `src/adapters/memory_service/`: one shared `reqwest` client built like JevHttp
  (`src/adapters/workflow_jev/mod.rs:23-30`: no redirects, connect 5 s, idle 30 s), bodies read
  under a 4 MiB + 64 KiB cap, tokens never in logs, spans or `Debug`. Capacity (src/AGENTS.md
  fairness): an interactive pool (recall, tools, `memory.load`) and a background pool (delivery,
  lifecycle) with separate permits, plus at most 2 interactive and 1 background operations in flight
  per company. One recall, with its up to three concurrent `context.pack` calls, is one interactive
  operation; a tool call or `memory.load` is one. A permit is waited for only until the caller's
  deadline.
- Startup reads (`src/infra/config.rs`, Resend pattern `:208-254`): `MEMORY_SERVICE_URL` (unset:
  memory unavailable, memory kinds and builtins refused, no memory workers, and a warning naming
  every pending `memory_lifecycles` row; https, or http only to loopback; no userinfo or query);
  `MEMORY_SERVICE_PROVISIONER_TOKEN` (required with the URL, `SecretString`, `msk_adm_<uuid>_<secret>`
  shape checked, its credential id parsed); `MEMORY_SERVICE_TIMEOUT_MS` (default 12,000,
  1,000–30,000: above the service's 10 s deadline); `MEMORY_SERVICE_RECALL_DEADLINE_MS` (default
  2,000, 250–5,000: all recall scopes together, run concurrently); `MEMORY_SERVICE_INTERACTIVE_IN_FLIGHT`
  (default 8, 1–32) and `MEMORY_SERVICE_BACKGROUND_IN_FLIGHT` (default 4, 1–16). The service reads no
  `MEMORY_SERVICE_*` key. Startup never contacts the service and the docs say so; readiness is
  unchanged (memory optional).
- The provisioner token is process configuration, never stored or given to companies or agents
  (D3, D11). Each namespace's client token is stored `enc:v2` under a new
  `CredentialContext::memory_client_credential(company_id, namespace_id)` (beside
  `typesafe_credential`, `src/adapters/persistence/credentials/envelope.rs:101-110`; seal/open
  `credentials/mod.rs:199-217`) with its key version, read by one narrow method, and listed as
  `CredentialTable::MemoryClientCredentials` (`src/adapters/persistence/credential_rotation.rs:64-80`).
- The service cannot reassign a tenant's `managed_by` (D3, the admin credential id), so the
  provisioner credential cannot be rotated yet. 06A must add reassignment before the first
  production deploy. Until then, on compromise:
  1. An operator revokes the provisioner credential. Client credentials keep serving.
  2. The lifecycle worker's refusals (credential-id mismatch, `tenant_unmanageable`) show in
     settings.
  3. The operator, whose scope manages every tenant, completes pending deletions by hand.

### E2 Tenants, namespaces and provisioning

- Tenant `external_id` = company UUID (D11); each `memory_namespaces` row holds a random UUID as its
  namespace external id. Scopes:
  - company: one per company;
  - agent: one per (company, agent); tools, recall and capture use the run's selected agent;
  - user: one per (company, user), reachable only when the reader set is that user: the root run
    is bound to a `private_session` (`workflow_agent_sessions`) whose `creator_user_id` is the run
    actor (the invariant `guard_workflow_agent_session` enforces) and the session has no
    channel/thread attachment. Children inherit the root's eligibility. A message-triggered run
    (actor = binding run-as), a schedule, a manual run outside a session and every child of those
    are never eligible; a session attached to a channel or thread is not eligible in the first
    release (its context can include others' thread content). Steps have no user scope.
  - A missing actor, agent or session omits or refuses the scope, never a wider one. Channel,
    thread, run and source ids are provenance only, never grants.
- One client credential per namespace: {that namespace, `read_max` 9, `write_max` 9, ops search,
  timeline, fetch, context, save, sources, generate}. Grants are fixed at issue, at most 64, and
  issue is not idempotent (D3), so 1:1 keeps lifecycle simple at the cost of at most three reads
  per run, merged by OpenBot. Recalled observations from other namespaces are cited as
  `external_sources` (provenance only), never as service parents.
- Opt-in (U1): an owner or admin enables memory in company settings, which creates the tenant row
  and lifecycle; namespaces provision lazily, and a run needing one meanwhile sees `not_ready`.
  Disable stops new recall, saves and capture, fails pending intents (their waits wake with a
  terminal failure) and sets the service tenant disabled; data is retained; enable revives nothing.
  Enforcement:
  - Disable takes `memory_tenants … FOR UPDATE`, bumps its `disable_epoch`, then fails the pending
    intents (E9). Only disable changes `disable_epoch`. Settings edits change `revision` only, so
    they never refuse a valid pending intent. Cap-cache refreshes change neither, so a settings
    form rendered by enable stays current (review p7b r1 n1).
  - Every intent-creating transaction (step save, tool save, capture enqueue) takes `memory_tenants
    … FOR SHARE` after the run row and before the namespace. It requires `enabled` and, for capture,
    the agent's matching switch; otherwise the save is terminal `memory.not_enabled` and capture is
    skipped. A racing save or capture therefore either commits first and disable fails it, or waits
    and sees the tenant disabled.
  - Each intent stores the tenant `disable_epoch`. As a backstop, a claim that finds a stale epoch
    turns the intent into `failed(disabled)`, which wakes its wait; it is never silently skipped.
    A poison-batch case fills a claim batch with stale-epoch intents.
- Tables, added to `migrations/20260817000000_init_schema.sql` in place (reset policy):
  - `memory_tenants`: PK `company_id`, cascade; state enabled/disabled; `revision`;
    `disable_epoch`;
    `provisioner_credential_id`; `generation_cap`, cached from `TenantView.generation_daily_token_cap`
    at the last create. A stale cache only yields a terminal `generation_disabled`; settings show
    "generation off (operator cap 0)".
  - `memory_namespaces`: `(company_id, id)`, scope, agent/user id, state
    provisioning/ready/removing, credential id, envelope, key version; unique per scope; cascade.
  - `memory_agent_capture`: per agent, the private-session and thread switches, allowance, max
    output, expiry, and the typed actor, reason and time of each switch change (E8).
  - `memory_lifecycles`: no FK to companies, so it outlives the owner (persistence AGENTS "remote
    resources", precedent `integrations/lifecycle.rs:31-62`). Holds resource, owner kind/id,
    `provisioner_credential_id` (kept here too, because the tenant row cascades away on the delete
    path), desired present/disabled/absent, observed, generation, fence, lease, attempts, outcome
    and CHECKs.
- Owner locks (M7), in the existing order users → company → membership/principal → agent → run
  (`src/adapters/persistence/workflow/action_authority.rs:42`, `prelock_accounts_on`):
  - Company delete locks `companies … FOR UPDATE` first. Today it checks `EXISTS` unlocked
    (`src/adapters/persistence/company.rs:104-142`).
  - Agent delete locks its company `FOR KEY SHARE` and then the agent `FOR UPDATE`
    (`lock_agent_delete_on`). Before review p7b r1 M4 it locked only the agent, and that
    deadlocked with company delete, which holds the memory rows and cascades into agents.
  - Member removal locks users `FOR SHARE` → company `FOR NO KEY UPDATE` → membership `FOR UPDATE`.
  - Account deletion (`lock_account_on`) locks the user `FOR UPDATE`, then every company it touches
    in one id-ordered pass, and only then its memberships `FOR UPDATE`. Owned companies are locked
    `FOR UPDATE` (they are deleted), member companies `FOR KEY SHARE`. Before M4 it locked only the
    owned companies, and the memory hook deadlocked with a member company's deletion.
  - Enable, and lazy namespace provisioning, run in a short transaction of their own, never inside
    the fenced run transaction (the run continues with `not_ready`). Order: users → company →
    member or agent `FOR SHARE` → namespace and lifecycle upsert, re-checking that the owner is live
    and memory is enabled.
  - Every `absent` write is an upsert (`INSERT … ON CONFLICT DO UPDATE SET desired = 'absent',
    generation = generation + 1`).
- Supervised `MemoryLifecycleWorker`: claim one due row (`UPDATE … FROM (SELECT … ORDER BY
  available_at, id LIMIT 1 FOR UPDATE SKIP LOCKED)`, fence + 1, 60 s lease; HTTP timeout and local
  deadline below it: one request per remote resource in flight); completion requires fence and
  generation. It refuses all work, logging, while the configured provisioner credential id differs
  from the row's `provisioner_credential_id`, and turns `present` into `absent` when the owner row
  is gone. Present tenant: create (idempotent; 409 → terminal `tenant_conflict`), recording the
  credential id. Namespace, once the tenant is present: create; list credentials and revoke those
  labelled `ob:<namespace>` but not stored; issue; store the envelope under the fence (an ambiguous
  issue leaves a stray the next pass revokes). Disabled: tenant disable. Absent tenant: create, then
  always delete, whatever create returned; only a 200 from delete completes it (the service answers
  200 for its own tombstone); a 404 is terminal `tenant_unmanageable`, surfaced to operators, never
  done. Creating first makes an earlier ambiguous create definite before the delete. Absent
  namespace: create, then delete, as for tenants (review p7b1 M2: the service answers 404 for a
  namespace whose create never landed and leaves no tombstone, so a delayed create could still
  land). A create answering 409 `namespace_deleted` (our tombstone) or a tenant conflict (the
  tenant tombstone covers it) is done; otherwise only a delete 200 completes, except that a 404
  after a `tenant_disabled` create is done (a disabled tenant admits no create). Then revoke every
  `ob:<namespace>` credential. Namespace delete removes its grants, replacing D11's purge (501,
  needs a client credential).
- Retry budgets (review p7b r1 M2, M3):
  - Outages are retried without limit: `service_unavailable`, `outcome_unknown`,
    `service_capacity`, `provisioner_unauthorized`, `interrupted`, `deadline_exceeded` and
    `store_unavailable`. They count `transient_retries`, back off 2ⁿ s to 5 min, and never fail.
  - Every other retry spends one of 8 attempts; the eighth is `failed`, shown in settings and at
    startup, and documented with a manual re-drive in `docs/deploy.md`.
  - A namespace waiting for its tenant (not created, or disabled) is postponed 30 s, not charged.
  - A new generation (any upsert, or a `Superseded` re-pend) resets both counters, so a superseding
    desired state never inherits an exhausted budget.
  - Enable, and lazy provisioning of a still-provisioning scope, re-drive failed `present`
    lifecycles with a new generation. Failed `absent` rows are re-driven only by an operator.
  `credentials/list` is unpaginated, so revoked strays accumulate per re-issue within the 4 MiB
  body cap; 06A's retention must bound them (review p7b1 n3).

### E3 Read ceiling, consumed watermark and exactness

- `read_ceiling = min(run.knowledge_ceiling, destination audience, grant read_max)`, computed in the
  transaction that authorizes the request. The destination is one helper extracted from `audience_on`
  (`src/adapters/persistence/workflow/agent_history/frozen.rs:78-106`): `Thread` →
  `thread_audience_on` (`knowledge.rs:46-71`: lowest human reader, agents/system excluded,
  externals public, malformed or more than 64 facts fails closed, channel without thread public);
  otherwise `actor_level_on` (`:72-92`) of the run actor. Readers 6/8/9 give 6. The admitted ceiling
  is immutable (`access_level.rs:168-193`). (Audience level only; E2 governs user-scope privacy.)
- Destination below `content_level` is the existing `AppError::KnowledgeRelease` refusal before any
  request, model entry, pack reuse or disclosure; one between content and ceiling narrows reads.
- Watermark = the run's monotone `content_level` (`knowledge.rs:139-158`), never `knowledge_ceiling`
  or one response's level; producing ceiling = `knowledge_ceiling`. New `workflow_runs.content_exact
  boolean NOT NULL` with no DEFAULT (every writer states it; the same holds for the E3 history
  columns), only ever lowered (trigger). Child admission (`children.rs:229-285`, workflow-tool
  children included) copies it with the parent's levels; a child's result lowers the parent's when
  inexact. Saves and captures send `consumed: exact(content_level)` while true, else `unknown` (the
  service's ceiling fallback, D4), frozen into the intent.
- Fail closed (M3): two generic hooks lower `content_exact` unless the source is allowlisted as
  level-accounted.
  1. Inside `batch_commit::complete` (`src/adapters/persistence/workflow/batch_commit.rs:11`), the
     single writer of step outputs for every caller (completion, wait resume, decisions, channel
     reply resume, recovery, batch), keyed by the execution's step kind.
  2. At `checkpoint_updates::update_on` for `WorkflowCheckpointUpdate::InvocationResult`
     (`checkpoint_updates.rs:4`, `:70`), the funnel for direct calls, approved or parked
     continuations and refusals, keyed by `AgentToolTarget`.

  Allowlist: pure kinds (`data.map`,
  `decision.rule`, `channel.no_reply`, `wait.timer`); raising reads (`context.history`,
  `memory.load`); model steps (`agent.run`, `ai.classify`, `decision.agent`: outputs derive from
  accounted inputs); receipts without external content (`message.send`, `message.reply`,
  `memory.save`); the `decision.human` choice (an authored enum, exact); human free text (its
  feedback, `message.ask` replies) goes through `human_input_exactness` below, which makes it
  inexact; child
  control (`workflow.call`, `flow.repeat`, exactness propagated); targets Knowledge, Memory, Message,
  Provisioning (company configuration, level 0 per U2), Workflow. Everything else lowers it:
  `http.request`, `mcp.call`, `tool.call`, `wait.event`, `jev.evaluate`, Http and Mcp targets, and
  any future kind or target until allowlisted. Test `memory_access_level_every_step_kind_and_tool_target_is_classified`
  enumerates `registry::TYPES`, every `AgentBuiltinTool` and every `AgentToolTarget` variant and
  asserts allowlisted-with-raise or lowering, in the style of
  `production_handler_covers_every_io_kind_or_refuses_it`. It also asserts that both hooks are
  wired at those two funnels: a scripted output through each `batch_commit::complete` caller, and a
  result through each `InvocationResult` path.
- Human input is classified by one pure function, `human_input_exactness(class)`, over two classes,
  so a change to U2 is one match arm plus its tests:
  - `AuthoredConfiguration`: agent instructions, skills, workflow literals and schedule params.
    Schedule params belong here because an owner or admin authors them in the binding and the
    schedule sends them into every audience it serves. The function takes the save's target scope
    as well, `human_input_exactness(class, scope)`: agent and user scope give exact 0; company
    scope gives inexact, so a company-scope save falls back to the producing run's ceiling (U2
    addendum). Every run consumes authored configuration (instructions, literals or params), so
    this is a rule, not tracked state: a company-scope save always sends `consumed: unknown`
    (the ceiling fallback), and agent- and user-scope saves follow `content_exact` (review
    p7b1 m6).
  - `ConversationalInput`: human messages (`messages_human_public`), manual and interactive run
    input, `decision.human` free-text feedback (its declared choice is an authored enum and stays
    exact) and `message.ask` replies.

  U2 (E12): `AuthoredConfiguration` is an exact 0. `ConversationalInput` is inexact, so a save from
  a run that consumed it falls back to the run's trusted ceiling: the conversation's lowest-reader
  level for message-driven runs, the actor's clearance for manual runs (run-as is the schedule
  case). With a level-0 reader that is still 0. The level stays 0 as admission sets it (`knowledge.rs:104-111`); only
  `content_exact` drops. Message stamps (`messages_human_public`) and knowledge release are
  unchanged. The classification is applied at admission (a human trigger message,
  manual input), by hook (1) (`decision.human`, `message.ask`) and by the history paths below.
- Missing trusted authority refuses. Known content above the ceiling refuses
  (`workflow_runs_knowledge_bounds`; E6 checks every `exposed_level` against the ceiling used).
- Declassification risk, documented in the record and the settings text: a cleared person who types
  classified facts into authored configuration is not protected, and memory makes such text persist
  at level 0. Conversational input no longer has this risk in memory, because it falls back to the
  ceiling.
- Exactness across runs (N-m4): option (a), carrying an exact bit. Agent-authored `messages` and
  `agent_history_exchanges` get `content_exact`, stamped in the same statement that stamps their
  `access_level` from the run. `messages.content_exact` is nullable with a CHECK: NULL for human
  rows, NOT NULL for agent and system rows, and system writers default to false. Human and external
  messages therefore carry no stored bit; their exactness is derived from the role at read time
  through `human_input_exactness`, so no stamp can drift from the rule.
  `agent_history_exchanges.content_exact` is NOT NULL with no DEFAULT. P7c budgets for the churn
  this causes in raw-SQL fixtures.
  `context.history` (`history_reader.rs:81-136`) and the history freeze (`selection.rs`, the
  `selected_level` aggregate) lower the reading run's flag when any selected item is inexact. This
  is proportionate: the change is one column per table, written where the level already is, and one
  `bool_and` beside the existing `max`.

### E4 `memory.load` and `memory.save`

- Scope (`src/application/workflow/registry/schemas.rs:18-23`) becomes `{kind:"company"}` |
  `{kind:"agent", agent:<AgentKey>}`, with no tenant, company or user id (no user scope on steps,
  E2), a literal field (no references) resolved against the bundle at publication.
- Gating (m8): publication refuses memory steps and tools unless memory is configured and the
  company has it enabled (`memory.not_enabled` diagnostic); a run reaching one after disable fails
  terminal `memory.not_enabled`. `UNHANDLED_STEP_KINDS` (`src/infra/workflow.rs:56-58`) stays the
  unconfigured refusal set, also until 05A's last sub-point lands; the tests at `:562-577` and
  `:659-689` split into configured and unconfigured cases.
- `memory.load` (`families.rs:14-21`): `query` 1–1,024 characters (over the service's 500-token
  limit is `memory.invalid_query`, never truncated), `limit` 1–20; search, then fetch the hits.
  Output `{items:[{id, scope, kind, title, content, occurred_at, access_level, revision}],
  exposed_level, omitted}`; items past the step-output budget are omitted whole and counted, and
  `exposed_level` (from fetch) still covers them, which is conservative. Pattern
  `history_reader.rs:81-136`: authorize under the fence (rolled back), call outside any transaction,
  then complete, raise and record the consumption in one transaction.
- `memory.save` (`families.rs:22-28`): `facts` 1–64 × 1–16,384 characters; title = first line cut at
  a character boundary to 256, body = the whole fact, kind `fact`. A frozen request above 3.5 MiB is
  terminal `memory.save_too_large` before any intent. One atomic request and key, superseding D11's
  batch split. Output `{saved:[{id, revision}], access_level, replayed}`; no classification input.
  Key `ob.s.<execution uuid, simple>`; `expires_at` = intent creation + 6 days, frozen; retries
  resend key and body unchanged.
- `memory.save` joins `marker_owned_effect_kind` (`actions/failure.rs:26-38`): its only effect path
  is the committed intent (E8). `memory.load` stays effect-free. Update `registry/help.rs:59-61` and
  `examples.rs:86-87`.
- Release (M5): `knowledge_release::evaluate_on` maps unknown Local targets to PUBLIC
  (`knowledge_release.rs:204`), which would refuse every classified save. Add an explicit `memory`
  arm, reachable only from the memory target: a memory save is a labelled store whose frozen label
  is at least the content level by construction, so release is allowed. Tests: a consumed-6 save
  passes; no other resource kind reaches the arm. Read queries sent to the service (recall and tool
  queries, load) are not release-checked egress: the service is the deployment's labelled store and
  persists no read text (reads write no audit events; logs carry ids).

### E5 Agent tools

- Builtins `AgentBuiltinTool::{SearchMemory, MemoryTimeline, MemoryDetails, SaveMemory}`
  (`src/domain/entities/agent_capability.rs:77-103`), offered as Configured by `builtins()`
  (`src/adapters/persistence/agent_authoring/catalog.rs:243-259`) only while memory is configured and
  enabled; snapshots beside `src/adapters/persistence/agent_publication/tools.rs:164-183` with a
  trusted-snapshot check; target kind `memory` (`src/application/workflow/agent_contract.rs:48-83`);
  `AgentToolTarget::Memory{tool}` (`agent.rs:38-60`) mapped in `targets()`
  (`src/adapters/persistence/workflow/agent_preparation.rs:64-102`) and `prepared_tool`
  (`agent.rs:322-371`); dispatch via `AgentActionTools::with_memory` like `with_knowledge`
  (`src/adapters/workflow_agent/mod.rs:61-177`). Frozen at publication; revocation applies at
  runtime (06-D5).
- Inputs: search `{query ≤ 1,024, scopes? ⊆ eligible, limit ≤ 10}` (snippets); timeline `{scope, id,
  before ≤ 5, after ≤ 5}`; details `{items:[{scope, id}] ≤ 4}` (whole bodies, omitted over budget);
  save `{facts:[≤ 4,096 characters] ≤ 8, scope?: "agent" | "user"}` (default agent; user only under
  E2). Save key `ob.t.<invocation uuid, simple>`; `external_sources` = the distinct recalled
  observations this execution consumed, at most 64, ordered by level desc then id (provenance only:
  the watermark already covers every level). No namespace, tenant, user id or level in any input.
  Results `{scope, id, revision, kind, title, text, occurred_at, access_level}` within
  `MAX_TOOL_RESULT_BYTES`.
- No company holds a client credential, which is the boundary. As defense in depth, company MCP/HTTP
  connections whose origin equals `MEMORY_SERVICE_URL` are refused (it misses alias forms such as an
  IP for a name or a default port).

### E6 Atomic accounting, the save freeze and the concurrency protocol

- Reads (tools, `memory.load`) follow `knowledge_checkpoint.rs`. `memory_context` (as `context_on`,
  `:85-99`): current agent authority, owner lock, live, the saved invocation (as
  `require_knowledge_invocation`, `src/application/workflow/knowledge/ports.rs:97-169`); a completed
  call replays. The service call runs outside any transaction under the claim deadline.
  `observe_memory` (as `observe_on`, `:101-147`), one transaction before any text reaches the model:
  invocation result, `raise_content_level_on(exposed_level)` after checking it against the ceiling
  used, a `workflow_memory_consumptions` row (knowledge shape plus item refs `{id, revision,
  sources_sha256}` for E7), live. An ambiguous commit grants no continuation.
- Save (step or tool), one transaction, in the existing lock order: authority/company prelock →
  `lock_fence` → `lock_scope`, which takes `workflow_runs … FOR UPDATE` before the execution and job
  rows (`src/adapters/persistence/workflow/lease.rs:285-294`; `checkpoints.rs:293-320` for tools) →
  re-read `knowledge_ceiling, content_level, content_exact` from that locked row (never claim-time
  `RunKnowledge` or a value read before the lock) → scope and write authorization →
  `memory_tenants … FOR SHARE`, requiring `enabled` (E2) → namespace `FOR SHARE`, requiring `ready`
  → `memory_deliveries` intent with the frozen body and the tenant `disable_epoch` → effect wait →
  live →
  commit. No extra run lock is added.
- Protocol: the run row lock already held by every fenced write serializes accounting. A read's
  raise commits under it before exposure, so a freeze either waits for an in-flight observation and
  freezes its level, or commits first while that text has reached no model; save content is fixed
  before any later exposure. A retry replays its intent; each logical save gets its own intent and
  the then-current level; nothing deduplicates by content.
- Competing test `memory_access_level_competing_recall_and_save_freeze_preserve_the_watermark`
  (`own_database`, two connections, both orders, exact and fallback): an observation raising 4 → 6
  is held before commit, the freeze waits (lock wait seen in `pg_stat_activity`) and freezes 6; in
  the other order the freeze commits 4, the observation commits 6 and the next save freezes 6. It is
  re-run under two mutations, (A) freezing from the claim-time `RunKnowledge` and (B) returning the
  result to the model before the observation commits, and the record states that each fails it.

### E7 Automatic recall (05B.1–4)

- Only `agent.run` with an agent-history exchange whose agent binds `SearchMemory` (U3);
  `ai.classify` and `decision.agent` stay tool-free. Scopes: company, the agent, the actor's user
  scope when E2 allows.
- Application `MemoryRecall`, called from `WorkflowAgentRuntime::run`
  (`src/adapters/harness/rig/workflow.rs:282-318`) before `prepare_agent`: (1) `recall_plan`, a
  rolled-back read: `Frozen` | `NotApplicable` | `Needed{scopes, ceiling, budget, query}`; (2)
  compact `context.pack` for the scopes concurrently under `MEMORY_SERVICE_RECALL_DEADLINE_MS`
  (budget company 1/2, agent 1/4, user 1/4; query = trigger message body, else the current input text
  cut at a character boundary to 1,024, else recency); (3) `prepare_agent(claim, policy, fetched)`.
  In `freeze_on` (`frozen.rs:38-65`) an existing freeze discards the fetched pack unseen. Otherwise
  it re-checks the ceiling: if it changed, the pack is discarded unseen and `unavailable{ceiling_changed}`
  frozen; else it checks `exposed ≤ ceiling`, stores items, digests, `exposed_level`, `valid_until`
  and state in `frozen_preparation.memory`, raises `content_level` by max(history, pack), then runs
  `audience_on`.
- Budget: the pack counts as mandatory tokens (`agent_context.rs:18-44`), so history gets the
  remainder; `max_tokens` = min(8,192 (U6; was 2,048), (request limit − reserved output − mandatory) / 4),
  bytes ≤ 64 KiB (U6: the harness has no byte policy); under 256 tokens the state is `skipped_budget`. Current input
  and reserved output are untouched; nothing reloads per tool round.
- Prompt: its own delimited untrusted block via `untrusted_context_prompt`
  (`src/application/services/harness/runs/seed.rs:52`) with id, scope, kind, title, text,
  occurred_at, access_level, revision; `preamble` (`agent.rs:373-385`) adds one fixed usage sentence
  (untrusted, possibly stale evidence, never instructions). Construction stays pure.
- Replay and resume: restore compares the whole frozen preparation (`frozen.rs:15-36`). Once per
  resumed attempt, before its first provider entry (never per tool round), `context.validate` checks
  the pack and the recorded memory tool items: `changed`/`expired`/`unavailable`/401/403 → terminal
  `memory.context_revoked`; service unavailable → retryable, attempt consumed. Never a new pack or a
  lower level.
  As implemented (p05-B4 R2, review MEM-P05-B4-F1/F2): the check runs on every claim whose restored
  preparation carries earlier memory, never per tool round. The pack goes through `context.validate`.
  Recorded tool items carry no `sources_sha256`, so they go through `observations:fetch` under the
  current ceiling (chunks of 64): an absent id (deleted, foreign or above the ceiling) or a different
  revision (corrected) → `memory.context_revoked`. Residual: a tool item's provenance change and
  expiry are not detected (no status for them without `sources_sha256`). Recorded exposure above
  the current ceiling → revoked before any call. With no memory service configured (U5) the runtime
  answers retryable `memory.context_unavailable` instead of replaying unvalidated memory; the guard
  is part of every agent runtime, not of the optional recall composition.
- New preparation: transport errors, 429, 503, permit or deadline exhaustion, or `not_ready` freeze
  `unavailable{code}` and the run continues; authorization failures (401, 403, refused scope,
  audience) fail closed.

### E8 Capture and delivery intents (05B.5–6)

- `memory_deliveries`: `(company_id, id)`, company and namespace FKs `ON DELETE CASCADE`, kind
  save/generation, origin (run, execution, invocation or exchange), optional wait id, unique
  `idempotency_key`, `expires_at`, versioned frozen request (≤ 4 MiB; the request itself, never
  entities), tenant `disable_epoch`, state pending/claimed/delivered/failed/expired/cancelled, attempts,
  fence, lease, `available_at`, result, outcome, state CHECKs.
- Supervised `MemoryDeliveryWorker` on the background pool: the `claim_deliveries` shape
  (`src/adapters/persistence/delivery/queue.rs:62-144`). It claims only for an enabled tenant
  whose `disable_epoch` equals the intent's (a stale one becomes `failed(disabled)`, E2) and a ready
  namespace. 60 s lease, 20 s heartbeat under the fence, local deadline below the lease, HTTP
  timeout below the deadline. The fenced completion writes the intent's receipt and state only; the effect wait wakes by
  its own poll through `effect_resume_on` (`wait_commit.rs:90-118`), which gains a memory receipt,
  as message delivery does. Lease loss or crash consumes an attempt; 8 → `failed`; past
  `expires_at` → `expired`, nothing sent. Retry = same key and body (replay). 409 terminal; 410
  expired; 401, 403, `tenant_disabled` → `failed(authority)`; 429, 503, ambiguous → backoff.
  Poison-batch test.
- Capture policy (U3), per agent in `memory_agent_capture`, off by default, set by an owner or
  admin: `private_sessions` (into the creator's user scope, E2 eligibility) and `threads` (into the
  agent scope), each with allowance tokens per window, `max_output_tokens` ≤ 8,192 and expiry
  1–168 h; the operator's tenant cap (cached, E2) must also be above 0. Each switch change records
  its typed actor, reason and time. The cross-channel text is shown when `threads` is turned on and
  on the settings page: captured thread memories are labelled at the thread's lowest-reader level;
  any reader cleared to that level, in any channel the agent serves, can recall them; and a thread
  with a public or external reader is captured at level 0. `child_branch` conversations and private sessions attached to a channel or thread are
  never captured.
- The transaction accepting a succeeded `agent.run` exchange
  (`src/adapters/persistence/workflow/agent_history/checkpoint.rs:224`) takes `memory_tenants … FOR
  SHARE` after the run row (E2) and enqueues a generation intent only when the tenant is enabled and
  the matching switch is on: the logical input (user) and accepted output (assistant) at the
  exchange level, ceiling and consumed frozen per E3, no `session_id` in the first release (no
  summaries). A thread window contains `ConversationalInput`, so its consumed evidence is `unknown`
  and the captured memory is labelled at the thread's lowest-reader level. That narrows U3's
  cross-channel exposure: the memory reaches only audiences cleared to that level. Excluded: failed or cancelled runs, samples, tool arguments and results, credentials,
  internal notes, ineligible scopes. Key `ob.g.<exchange uuid, simple>`; `expires_at` = the policy
  expiry and allowance, both frozen; `generation_disabled` is terminal; a retry is a replay, never a
  new generation request.
- Deadlines: save intents 6 days, generation intents the policy expiry (≤ 168 h), both inside the
  service's 7-day maximum (D5): the record exists while any attempt can execute; afterwards the
  service answers 410 and OpenBot never re-keys.

- As implemented (p05-B5, routine choices within E8): one `memory_agent_capture` row per
  (agent, kind) with its own allowance (1–1,000,000), output limit (≤ 8,192) and expiry (1–168 h),
  revision-checked, recording `changed_by`, a 1–500 character reason and the time; an agent's final
  delete cascades its rows. The freeze locks run → tenant → namespace → policy row, so the delete's
  cascade (after removal's namespace lock) cannot deadlock with a capture. A window is captured only
  into a ready namespace (an unready one would fail delivery), so preparation requests the
  switched-on target scope (`capture_targets`, own transaction) and the first window after turning
  capture on can be missed. The request carries two events, the logical input and the accepted
  output as canonical JSON text, at the run's content level, ids `ob.e.<exchange>.input|output`.
  Switching capture off stops new windows, not queued intents; run cancellation cancels the run's
  open intents of both kinds. A cap raised by the operator is picked up at the next tenant
  lifecycle settle (create, enable or disable).

### E9 Lifecycle ordering (05B.7)

- Company delete: the transaction locks the company `FOR UPDATE` (E2), upserts the tenant lifecycle
  to absent, then `DELETE FROM companies` cascades namespaces, intents and consumptions, so nothing
  can claim afterwards; an in-flight delivery's fenced completion finds nothing, and what it landed
  goes with the tenant delete.
- OpenBot has no company suspension; memory disable (E2) is its analogue. Order: the users →
  company prelocks, then `memory_tenants … FOR UPDATE` with `disable_epoch` + 1, then pending intents →
  `failed(disabled)`, then the lifecycle's desired state → disabled (the service cancels accepted
  generation, D3). Saves and capture serialize behind the tenant row (E2), and the epoch backstop
  fails anything older. Test `memory_integration_disable_races_save_and_capture_then_reenable`
  (competing connections, both orders, then re-enable) asserts that nothing is delivered and that
  every wait resolves.
- Agent delete (retire is the same final delete, `agent.rs:398-435`), member removal
  (`src/adapters/persistence/company_invite.rs:549-619`) and account deletion
  (`src/adapters/persistence/user_lifecycle.rs:11-61`): owner row → namespace `FOR UPDATE` →
  `removing`, pending intents → `cancelled`, lifecycle upserted absent, in their transaction. Run
  cancellation cancels the run's pending intents after its run lock (run → intent, the save order).
  Neither touches runs or waits: waits wake through their own poll and read the terminal intent
  state, so no transaction takes namespace → run. A returning user or agent gets a new namespace row
  and id, so a delayed provision, delivery or capture meets the tombstone's terminal 403/409.

### E10 Samples and legacy tests

- Samples keep scripted fixtures (`memory.load` FixtureRead, `memory.save` InterceptedWrite,
  `sample/dispatch.rs:127`; memory tools as scripted results; no recall); the sample-capture refusal
  (`src/infra/workflow/services.rs:112-123`) follows E4's gating, and
  `scripts/workflow-sample-boundary.py` keeps rejecting `application::memory`/`adapters::memory_service`.
- `the_memory_provider_schema_is_gone` (`src/adapters/persistence/mod.rs:236-280`) becomes an exact
  allowlist of the new tables and columns plus an absence check on legacy names
  (`memory_provider_connections`, `memory_remote_resource_lifecycles`, `memory_cleanup_jobs`,
  `hydradb%`). The refusal tests in `src/application/workflow/authoring/tests.rs:143-177` and
  `src/adapters/http/routes/workflows/ui/authoring/tests.rs:423-449` split by gating.

### E11 Verification map

| Prefix | Proves |
| --- | --- |
| `memory_scope_` | E2 mapping, missing identities, user scope refused for a thread run's child, a schedule run as another user and an attached session, and allowed (U4, documented) for a foreign-authored manual workflow and a child of an eligible root; cross-company keys and ids; reconciler fences, stray credentials, create-then-delete, foreign-managed tenant never completing deletion, changed provisioner id refused; enable vs company delete, lazy provision vs agent delete and vs member removal (competing) |
| `memory_integration_` | contract-fake round trips, ungranted tools, tampered arguments, revoked credentials, lost ownership, sample isolation, duplicate saves, `memory.not_enabled`, disable failing pending intents, release `memory` arm |
| `memory_access_level_` | readers 6/8/9 read 0–6; a message run with readers 6/8/9 saves at 6, even after only public recall; a schedule run (and its children) consuming level-4 knowledge saves at 4; a schedule run consuming only configuration and level-4 knowledge saves at 4 into agent scope but at its ceiling into company scope, and an agent-scope recall of that memory stays within that agent; a captured thread window is labelled at the thread's lowest-reader level; the `decision.human` choice stays exact while its free-text feedback and `message.ask` replies lower it; a mixed-audience conversation (a level-0 reader) saves at 0; consumed 6 saves 6 after a public latest recall; inexact saves at the ceiling; inexact parent's child saves at the fallback; a run reading an inexact run's message through `context.history` or the history freeze saves at the fallback; forged labels; missing ceiling; drop to 3 refused; kind/target coverage; the E6 race and its mutations |
| `memory_preparation_` | actual model context: bounded pack, no instruction promotion, exact retry snapshot, discarded late pack, `ceiling_changed`, one validation per resumed attempt, optional unavailable, recall deadline, two concurrent recalls in one company both succeeding |
| `memory_capture_` | per-agent switches, eligible windows only, no secrets, no duplicate capture, promised delivery, competing claimants, delete vs delivery |

- Regressions: publication (`production_*`, authoring), tool dispatch (`autonomous_agent_`),
  knowledge level (`knowledge_run_level_`, `knowledge_search_`, `knowledge_provisioning_`), samples
  (`workflow_sample_` and the script), preparation and history (`agent_history_`), checkpoint, budget
  (`workflow_budget_`), cancellation (`workflow_action_cancellation`), lifecycle
  (`workflow_lifecycle_`, company delete, credential rotation).
- DB: `TEST_DATABASE_URL=postgres://postgres@127.0.0.1:55432/open_bots_mem_test`, created once in
  root's private PostgreSQL 18.6 cluster, so `test_pool`'s `DATABASE_URL` → `_test` redirect never
  engages; `DATABASE_URL` is unset or points at that cluster; `own_database` for unscoped claims and
  whole-table checks; never `localhost:5432` or a `.env`. `cargo sqlx prepare --check` uses a
  migrated `postgres://postgres@127.0.0.1:55432/open_bots_mem_prepare` instead of src/AGENTS.md's
  `localhost:5432`. List each filter; require a nonzero count.
- Service: port doubles for application logic; a loopback axum fake validating every request and
  response against `services/memory/openapi.json` with `jsonschema =0.33.0` and counting calls; and
  `memory_integration_live_` `#[ignore]` tests run by a new `scripts/memory-integration.sh` (build the
  service in its own directory, `provision-db.sh` on that cluster, `admin bootstrap`, a provisioner,
  `serve` on loopback with the fake Messages API, then `-- --ignored`), recorded explicitly, with a
  CI step like `ci.yml:124`; 06B extends it to `memory_e2e_`.
- Per sub-point: fmt check, `git diff --check`, offline `cargo check --all-targets`, its prefixes and
  regressions, `sqlx prepare --check` after SQL. 05 gate: all five prefixes, the live tests,
  `scripts/stack-budget.sh` (new seams `Box::pin`ned), boundary scripts, credential rotation; the full
  suite stays 06C.

### E12 User decisions, routine choices and split

User decisions (2026-11-08, fixed):
- U1 Memory is opt-in per company: an owner or admin enables it in company settings; disable as in
  E2.
- U2 (refined, applied in E3):
  - `AuthoredConfiguration` (agent instructions, skills, workflow literals, schedule params) stays
    an exact 0, because the agent already sends it into every audience it serves.
  - `ConversationalInput` (human messages, manual and interactive input, `decision.human` feedback,
    `message.ask` replies) is inexact, so saves fall back to the run's trusted ceiling.
  - The user's reasoning: `messages_human_public` stores human messages at 0 even in conversations
    whose readers are all cleared to 6 or above, and memory is the first mechanism that carries such
    content across conversations; thread capture makes that automatic.
  - Consequence: "consumed 4 saves at 4" holds for schedule runs and their children, while a
    message-driven run saves at its conversation's lowest-reader level.
- U2 addendum (2026-11-08, fixed): for company-scope saves `AuthoredConfiguration` is also
  inexact; agent- and user-scope saves keep it at exact 0. Reason: other agents recall
  company-scope memories in audiences that never received the producing agent's configuration, so
  "already sent to every audience it serves" does not hold there. Code lands in P7c.
- U3 Automatic recall is enabled by the agent's `SearchMemory` binding, with E1/E7 bounds. Capture:
  private sessions into the actor's user scope (E2 eligibility), never tool results; thread
  conversations are also captured, into the agent scope (chosen over the reviewer's deferral), only
  behind the per-agent capture policy, off by default and enabled by an owner or admin, with the
  cross-channel consequence stated in the record and the settings text (E8).

- U4 (user, 2026-11-09; review MEM-P05-B3-F1): the user scope's eligibility rests on run facts
  only, never on authorship. A run may use its actor's private (user) memory when its root is that
  actor's own manual start in an unattached session they created, and so may every child of that
  root run as the same actor. Every admission binds a session, so a schedule's or a message's root
  is told apart by its admission source, and a manual run in a channel or thread has an attached
  session: neither is eligible. Who published the workflow version, configured the binding or wrote
  the agent is deliberately not checked: an owner or admin who manually runs a colleague's workflow
  lets that workflow's steps and agent read their personal memory, and the workflow can send what
  it read wherever its author chose. Kept and documented (docs/workflow-operations.md "Agent memory
  tools"): only owners and admins can start manual runs, and an authorized admin can already
  inspect another person's private run. This replaces M2's "a foreign-authored manual workflow"
  refusal (E11) with an allowed, documented case.
- U5 (user, 2026-11-09; review MEM-P05-B3-F4): a deployment that removes `MEMORY_SERVICE_URL` while
  companies still have memory enabled is a documented operator edge, not gated. Enabling is refused
  while unconfigured, but in that state the agent editor still lists the four memory tools, an agent
  version using them still publishes, and every call fails closed (`ServiceUnavailable`: nothing read
  or saved; the step retries, then fails). Remedy: restore the URL, or disable memory in those
  companies, which removes the tools from the editor and refuses new publication
  (docs/deploy.md "Memory service"). This narrows E1's "builtins refused" and E5's "offered only
  while configured" to the enabled check for this case.

- U6 (user, 2026-11-09; review MEM-P05-B4-F4): the automatic recall cap is 8,192 tokens, not E7's
  2,048. The service counts a pack as 64 + 2 tokens per byte of its items, and one compact item is
  about 400–700 bytes, so 2,048 split company 1/2, agent 1/4, user 1/4 left the agent and user
  quarters room for no item and the company half for about one. 8,192 is the service's own default,
  fits a few items per scope (about 12 at most, under the consumption row's 64-item check) and is
  still at most a quarter of what the request has left. E7's byte term `max_context_bytes / 4` has no
  counterpart in the harness policy (it has no byte limit), so the byte bound is the fixed 65,536.
- U7 (user, 2026-11-10; review MEM-P05-B5-F2, extends U5): with `MEMORY_SERVICE_URL` unset nothing is
  captured. A runtime with the memory service configured arms capture in the preparation it freezes
  (`capture: true`, kept by every restore); acceptance captures only an armed exchange. Without it
  an unsent intent holding up to 512 KiB of conversation text would be written for every completed
  exchange of a capture-enabled agent. Residual: a run prepared while the service was configured
  and accepted after the URL was removed can still queue one capture (bounded by the runs in
  flight); it waits unsent until the URL returns or memory is disabled.

Routine, resolved above: per-namespace credentials; namespace delete instead of purge; no save
split; `memory.save` marker-owned with a release `memory` arm; the existing run row lock as the
accounting lock; fail-closed exactness; fail-closed validation once per resumed attempt;
deadlines; disable as the suspend analogue; no user scope on steps and no capture `session_id` in
the first release; no generation-status client until something uses it.

Split, each with its focused gate: P7b (05A.1–2) config, ports, adapter and pools, credential
storage and rotation, the configuration tables, owner locks, the reconciler, settings and lifecycle
hooks (`memory_scope_`, adapter `memory_integration_`); P7c (05A.3, 05A.6) `content_exact` with the
generic hooks, child copy and coverage test, the ceiling helper, both steps and gating, the release
arm, intents, delivery worker and effect waits, samples, legacy tests (`memory_access_level_`,
`memory_integration_`); P7d (05A.4–5) the four tools, consumption records, the E6 race and its
mutations; P7e (05B.1–4) recall (`memory_preparation_`); P7f (05B.5–7) per-agent capture policy,
generation intents, lifecycle races and the 05 gate (`memory_capture_`, live tests).

### P7b implementation notes and handoff (05A.1–2, first substep)

Done (uncommitted, on base `a6bed48a`):
- Configuration: `src/infra/memory_config.rs` (+ `_tests.rs`) reads `MEMORY_SERVICE_URL`,
  `MEMORY_SERVICE_PROVISIONER_TOKEN`, `MEMORY_SERVICE_TIMEOUT_MS`,
  `MEMORY_SERVICE_RECALL_DEADLINE_MS`, `MEMORY_SERVICE_INTERACTIVE_IN_FLIGHT` and
  `MEMORY_SERVICE_BACKGROUND_IN_FLIGHT`. Per-company shares are fixed constants (2 interactive, 1
  background). Stray keys without the URL are refused, and the recall deadline must be below the
  timeout. The token keeps out of `AppConfig` and `Debug`; its credential id is parsed. Startup
  (`src/infra/setup.rs`, `memory_service_startup`) logs the configuration or, when the URL is unset,
  warns naming up to 20 unfinished `memory_lifecycles` rows. It never contacts the service.
- Port: `src/application/memory/ports.rs` defines `MemoryProvisioner` (tenant create/disable/
  enable/delete, namespace create/delete, credential issue/list/revoke, no defaults), the error
  taxonomy `MemoryServiceError` and the `ob:<namespace>` label. `MemoryClient` is deferred to P7c,
  where its first consumer lands.
- Adapter: `src/adapters/memory_service/` holds the reqwest client (no redirects, retries or
  proxy; a sensitive bearer header; 4 MiB + 64 KiB body cap), the status taxonomy (connect →
  `Unavailable`, after send → `Ambiguous`, 504 → `Ambiguous`, 400/other → `Protocol`) and `MemoryPools`
  (global interactive/background semaphores plus per-company semaphores in a self-pruning map).
  `contract_fake.rs` is a loopback axum fake that validates every request and 200 response
  against `services/memory/openapi.json` with `jsonschema` and models D3 tenant semantics (own
  tombstone delete 200, foreign 404, uniform 409).
- Credentials: `CredentialContext::memory_client_credential`; `CredentialTable::MemoryClientCredentials`
  walks `memory_namespaces.credential_envelope` through `src/adapters/persistence/memory/inventory.rs`
  (keyset page, compare-and-swap rewrap).
- Schema, edited in place at the end of `migrations/20260817000000_init_schema.sql`:
  `memory_tenants` (with `revision`, `disable_epoch`, `provisioner_credential_id`,
  `generation_cap`, typed `changed_by`/`changed_at`); `memory_namespaces` (with scope-shape and
  credential CHECKs and a unique live scope index); `memory_lifecycles` (no company FK, with
  `provisioner_credential_id`, desired/observed, generation, fence, lease-shape CHECK, due index).
  `the_memory_provider_schema_is_gone` is now the exact allowlist plus the legacy-name absence
  checks. Only runtime `sqlx::query` was added, so `.sqlx/` is unchanged.

The remainder listed here at the first substep's handoff (persistence for the tables, the
reconciler, the settings page, the competing tests) is done; see the next section.

### P7b implementation notes, second substep (05A.1–2 remainder; author generation 7)

Uncommitted, on base `a6bed48a`, over the frozen first substep (tree `c2e6f609`).

Review `p7b1-code-r1` resolutions (all applied, each with a test):
- M1: only a connect failure is `Unavailable`; a builder error is `Protocol`; any other transport
  failure after `send()` is `Ambiguous`. A 503 is `Unavailable` only with the service's
  `unavailable` envelope; 500, 502, a bare 503, 504 and any status outside the taxonomy are
  `Ambiguous`; 400 and 413 stay `Protocol`. The fake gains `drop_after_write` (it applies the
  operation, then unwinds the connection task without the panic hook), and a test asserts
  `Ambiguous` with the write visible in the fake.
- M2: the fake records violations (answered 500 `contract_violation`) and fails the test in
  `Drop` or `assert_clean()`, on the test thread. Fidelity now matches the service:
  - delete of a never-created namespace: 404, no tombstone;
  - credential issue for a missing or deleted namespace: 400;
  - namespace and credential work on a deleted tenant: 409 (disabled: 403 `tenant_disabled`);
  - tenant delete leaves credentials unrevoked.
  E2's absent-namespace step is now create-then-delete (E2 text updated).
- m1: the lease CHECK is split (`claimed` ⇔ `lease_owner`, and `lease_owner` ⇔ `lease_until`),
  with a rollback test. n1: `owner_kind = 'company'` requires `owner_id = company_id` for every
  row.
- m2: a rotation-arm DB test: keyset paging across a page boundary, rotate and reopen under the
  namespace-bound context, a swapped context refused, and the CAS refusal. The rotation tests'
  session-local shadows now include `memory_namespaces`. Without that, every rotation test swept
  the shared database's real memory credentials.
- m3: an exact `table.column` allowlist for every `memory%` table, `hydradb%` tables refused, the
  tautological legacy loop removed, and other tables' memory/provider columns still refused.
- m4: the six keys documented in `.env.example` and `docs/deploy.md` ("Memory service"),
  including the stray-key refusal and that startup never contacts the service.
- m5: 429 `spend_exhausted` is `Refused { reason: "spend_exhausted" }`.
- m6: E3 states the company-scope rule (always `consumed: unknown`) instead of tracked state.
- n2: adapter-private wire enums. n3: noted in E2 for 06A. n4: the token is trimmed. n5: the
  long line was wrapped.
- `scripts/transport-boundary-check.sh` was re-run on a `git archive a6bed48a` export: the same
  single failure (`channel_reply_continuation_views_support.rs:59`), byte-identical output. It
  predates this work and is not fixed here.

Done:
- One lock order for every memory path: users → company → membership/principal → agent →
  `memory_tenants` → `memory_namespaces` → `memory_lifecycles`. Fenced completions lock the
  tenant or namespace row before the lifecycle row. Company delete locks its tenant and
  namespaces before the lifecycles and before the cascade. A claim touches only the lifecycle row.
- Opt-in (`src/adapters/persistence/memory/tenants.rs`, port `MemorySettingsStore`, use case
  `MemorySettings`):
  - Enable and disable run in one transaction: the actor's `users` row `FOR SHARE`, then
    `authorize_company` (company `FOR NO KEY UPDATE`, owner/admin member and principal
    `FOR SHARE`, anyone else a concealed `NotFound`), then `memory_tenants … FOR UPDATE`, checked
    against the form's revision (0 before the first enable).
  - Disable bumps `revision` and `disable_epoch`; enable bumps `revision` only.
  - Each records `changed_by` and `changed_at`. Each upserts the tenant lifecycle to
    `present`/`disabled`, and never over `absent` (refused as resurrection).
  - Enable is refused while the service is unconfigured, and refused under another provisioner
    credential than the one recorded.
  - Failing pending intents on disable lands with `memory_deliveries` (P7c).
- Lazy provisioning (`namespaces.rs`, port `MemoryNamespaces::ensure_namespace`) runs in its own
  transaction:
  - Locks, in order: the user `FOR SHARE` (user scope), the company `FOR SHARE`, then the member
    or the agent (in that company) `FOR SHARE`, then the tenant `FOR SHARE`, which must be
    enabled.
  - Answers: an existing ready namespace is `Ready(id)`; otherwise it inserts a `provisioning`
    row plus a `present` lifecycle and answers `NotReady`.
  - A concurrent provisioner of the same scope waits on the unique live index and then inserts
    nothing.
  - A missing, foreign or deleted owner is `OwnerUnavailable`; a missing or disabled tenant is
    `NotEnabled`.
- Owner hooks, each in the owner's own transaction after its owner-row lock:
  - Company delete (`company.rs`) now locks `companies … FOR UPDATE` first, replacing the
    unlocked `EXISTS`. `retire_company_on` upserts the tenant lifecycle to `absent` and settles
    the namespace lifecycles as `tenant_removed`, moving their fence and generation.
  - Agent delete, member removal and account deletion (memberships) call
    `retire_namespaces_on`. It sets the namespaces to `removing`, drops their credential columns
    (the credential is revoked remotely) and upserts each lifecycle to `absent`, superseding an
    in-flight claim. Account deletion's owned companies go through `retire_company_on`.
- The reconciler (`src/application/memory/lifecycle.rs`, store in `persistence/memory/lifecycle.rs`):
  - `MemoryLifecycleWorker::pass` claims one due row: pending and due, or claimed with an expired
    lease, which consumes an attempt; an expired lease on the last attempt fails `lease_expired`.
    The claim takes a 60 s lease and moves the fence, and it turns `present` into `absent` when
    the owner is gone.
  - A row recorded under another provisioner credential id is postponed 300 s with outcome
    `provisioner_mismatch` and a warning, without contacting the service or consuming an attempt.
  - Each pass runs under a 45 s local deadline, and shutdown interrupts it (`interrupted`, one
    attempt).
  - Steps:
    - Tenant present: create (409 → terminal `tenant_conflict`), then enable if it is disabled;
      the cap cache refresh changes no revision (r1 n1).
    - Tenant disabled: create, then disable.
    - Tenant absent: create (any answer), then delete; only a 200 settles, and a 404 is terminal
      `tenant_unmanageable`.
    - Namespace present: requires the tenant settled `present`. Create (409 → terminal
      `namespace_conflict`), revoke the `ob:<namespace>` strays (up to 16 per pass), then issue
      and store, sealed under the fence and generation.
    - Namespace absent: per E2, then revoke every labelled credential and delete the row.
  - Completion: `Applied`, `Superseded` (re-pended without an attempt) or `Lost`.
  - Retries back off 2ⁿ s to 5 min, or `Retry-After`; budgets as corrected in round 1 below.
- Supervision: `WorkflowEngine::with_memory` (composed in `setup/mail.rs` only when
  `MEMORY_SERVICE_URL` is set) adds the worker to `spawn`'s handles, which `main` joins at
  shutdown. `RESERVED_DATABASE_CONNECTIONS` 12 → 13. The guard that still fails early is
  `worker_slots`' startup refusal; its test and both docs moved with it.
- Settings: company Settings loads a Memory section (`pages/company_memory.rs`, route
  `/ui/companies/{id}/memory`, GET and POST `enable|disable` with the expected revision). It shows:
  - the state, revision, change time, the tenant lifecycle and namespace counts;
  - up to 20 namespace lifecycle problems;
  - the cap ("generation off (operator cap 0)") and a provisioner-mismatch notice;
  - the U1 and E3 explanation.
  Owner/admin only; members, strangers and foreign company ids see one denial.

Deviations and choices (review p7b r1: 1, 4, 5 and 6 accepted, 2 accepted with M2 fixed, 3 not
accepted and replaced):
- Disable remains allowed while the service is unconfigured: it only stops work locally and
  records the remote intent.
- `retire_company_on` settles namespace lifecycles instead of reconciling each namespace: the
  tenant delete removes them, and that deletion now survives outages (M2).
- (Replaced) A namespace whose tenant is not ready is postponed 30 s without an attempt
  (`tenant_not_ready`, `tenant_disabled`), and a failed provisioning is re-driven (M1).
- Absent-namespace NotFound or Conflict on create (tenant gone or not ours) settles: nothing of
  ours can serve there. The tenant's own lifecycle carries `tenant_unmanageable`.
- Memory steps, tools and kinds stay refused (`UNHANDLED_STEP_KINDS` unchanged): P7c. The P7c gate
  must include disable failing pending intents and the owner hooks cancelling them, with
  `memory_integration_disable_races_save_and_capture_then_reenable`.

Correction round 1 (review `p7b-code-r1` and the findings recheck), applied, each with a test:
- R-n1: `ContractFake::assert_clean` treats a poisoned state lock as a failure.
- M1: re-drive and tenant waits (above), tested end to end to `Ready`.
- M2: outage budget; failed rows in the startup warning (with or without the URL); the
  `docs/deploy.md` query and re-drive; the deletion claim corrected. Tested with twelve
  consecutive 503s on delete.
- M3: counter resets; both the crash path (attempt 7, then absent, then lease expiry: reclaimed at
  attempt 1) and the completion path are tested.
- M4: the lock orders above. Competing tests run each deletion against company delete in both
  orders, through the production lock functions. A mutation probe (agent delete without the
  company lock) is recorded in 05.
- m1: two concurrent claimants on four rows (distinct claims). The expiry sweep now uses
  `SKIP LOCKED`, so it never waits on a row a completion holds (tested). A completion blocked
  behind `retire_company_on` ends `Lost` (tested).
- m2: mismatch refusal tested on the tenant-absent and namespace-absent paths: no call, no
  attempt.
- m3: shutdown during an in-flight request: `interrupted`, lease released, prompt join.
- m4: the memory POST route is in the cross-origin write enumeration.
- m5: a listing of 1,000 credentials (the service's unpaginated limit) never counts as "all
  revoked": `credential_list_truncated`, bounded. Revocation batches are progress, postponed
  0 s without an attempt.
- n1: the cap refresh does not bump `revision` (tested). n3: a returning member gets a new
  namespace id (tested).
- 06A conditions:
  - When tenant `managed_by` reassignment lands, the recorded provisioner id and the remote
    manager can diverge. Then `namespace_absent`'s 404/409 settles could falsely complete
    deletions, and the 404 accepted after a `tenant_disabled` create needs the quiescence
    deadline that persistence AGENTS requires for remote cleanup.
  - Settled lifecycle rows are retained forever, and the expiry sweep and the OR'd claim
    predicate are not served by the partial `pending` index. 06A sets retention with a
    threshold metric.
  - The 1,000-row credential bound (revoked credentials are never removed by the service)
    needs service-side retention or pagination.
- P7c obligation: never call `ensure_namespace` while holding a run transaction (it self-blocks on
  the company lock and takes a third connection per slot).

Correction round 2 (review `p7b-code-r2`), applied, each with a test:
- r2-m1: lazy provisioning re-drives a failed namespace only when a retry can fix it, and only
  10 min after the failure. Definite outcomes are never re-driven automatically:
  `namespace_conflict`, `credential_list_truncated`, `protocol_error`, `invalid_lifecycle`,
  `tenant_conflict` and `tenant_unmanageable`. A `namespace_conflict` scope is retired and
  provisioned under a new namespace id. Enable and the owner action re-drive everything.
- r2-m2: a row with 12 or more outage retries is stuck. The worker logs it once at `error` when
  it crosses the threshold; startup lists stuck rows beside failed ones, and the docs query
  includes them.
- r2-m3: an owner/admin "Retry failed provisioning" command (POST `action=retry` with the form
  revision, the same route and CSRF enumeration). It is shown only when a non-absent lifecycle
  failed, and it changes neither `revision` nor `disable_epoch`. The docs now describe it instead
  of a disable-and-enable.
- r2-m4: `Retry-After` is floored at the exponential backoff, and everything is capped at 5 min.
  The 1,000-credential ceiling is recorded in `docs/deploy.md`. It joins the 06A carry, needing a
  service change: a label-filtered or paginated `credentials/list`.
- r2-n1: the agent and account lock and finish functions are `pub(in crate::adapters::persistence)`.
  Agent delete's finish takes an `AgentDeleteLocks` token with private fields, and
  `user_lifecycle` is private again.
- r2-n2: a stray batch revokes at most 16 credentials. It is free and immediate only when the
  batch revoked at least one; a batch of 404s is `stray_revocation_stuck`, a bounded attempt.

Correction round 3 (review `p7b-code-r3`, r3-m1), applied with tests:
- On namespace create, only a 409 `namespace_deleted` (or a `deleted` view) is our namespace's
  tombstone (`namespace_conflict`). Lazy provisioning replaces it with a new namespace id, and
  only after the 10 min cool-down.
- Any other 409 is the tenant's (deleted service-side, or not ours): terminal `tenant_conflict`
  on the namespace. It is never re-driven or replaced automatically (it is in
  `DEFINITE_FAILURES`), and the settings page shows it as a tenant-level refusal for the
  operator.
- The test confirms that a service-side tenant deletion mints no new namespace ids or lifecycle
  rows across repeated `ensure` calls.

Next (P7c, per E12): `content_exact` and its hooks, both steps and gating, the release arm,
`memory_deliveries` (and then disable's intent failing and the owner hooks' intent cancelling),
the delivery worker, samples and legacy tests.

### P7c implementation notes (p05-B2, author generation 8)

Uncommitted, on base `a6bed48a` over the accepted P7b tree `113801e4`. The batch decisions, the
criterion → test → assertion mapping and the contract-impact list are kept in
`plan/memory-service/phases/p05/docs/TASK.md` ("p05-B2 decisions, verification mapping and evidence");
this block records what changed against E1–E12.

- Exactness (E3) as designed, with these refinements: the step-output hook takes the frozen graph
  (`batch_commit::CompletedStep`), so every caller must pass it, and folds the lowering into the run
  state update; a reviewer's channel-reply edit is conversational input and lowers the run at the
  approved continuation (a path E3 did not list); a result for no prepared tool lowers;
  `decision.human` is exact only with empty feedback and null/empty data.
- Delivery (E8): no run FK on `memory_deliveries` (E8 names company and namespace FKs only); no
  heartbeat (deadline 45 s < lease 60 s, HTTP ≤ 30 s); outages back off on `transient_retries` without
  spending attempts, bounded by the 6-day `expires_at`; separate `Delivery` permits; run cancellation
  cancels open intents by trigger; owner removal cancels after its namespace update.
- Steps (E4): agent scopes name an agent the bundle snapshots (`memory.unknown_agent` otherwise);
  `memory_parked` stop reason with its own guard branch and suspension proof; no `memory` arm in
  `knowledge_release::evaluate_on` (unreachable: memory never routes through ActionService release, and
  a save's label is at least the run's content by construction) — open for review.
- `SaveScope` has no user variant: user-scope saves arrive with the save tool (B2d), which adds it.
- Correction round R2 (review MEM-P05-B2-R1):
  - E4: `memory.save` is marker-owned (`marker_owned_effect_kind`). Its only effect path is the intent,
    committed in the transaction that parks the step, so an attempt that died without one sent nothing and
    its lease expiry retries (`safe`) instead of reconciling.
  - E8 concurrency and fairness: the worker runs `background_in_flight` lanes (the size of the client's
    Delivery pool, whose global and per-company permits bound the sends), and the claim skips a company
    with a live claimed intent (its per-company share of 1). One company's backlog therefore holds at most
    one lane. With `MEMORY_SERVICE_BACKGROUND_IN_FLIGHT=1` delivery stays FIFO by availability, so a burst
    delays other companies by burst × request latency; that is the documented bound at that setting. Two
    lanes can still claim one company's intents together (neither claim sees the other uncommitted); the
    per-company permit then serialises the sends, and the loser retries without spending an attempt.
  - E8 poison and retention: an envelope this process cannot open settles its intent
    `failed/credential_unavailable` at the claim instead of failing the pass. Terminal intents drop
    their frozen body (`request` NULL; CHECK open ⇔ body present) and keep key, receipt and outcome.
    Partial indexes serve the claim's expired-lease scan, the per-company in-flight check and the run-cancel
    lookup. Row retention of settled intents is a 06A obligation (sweep after the 7-day service window).
  - E9 ordering: every multi-intent settlement (disable, owner removal, the run-cancel trigger) locks the
    rows in id order before updating them, so overlapping settlements cannot deadlock.
  - E6 consumption refs: `memory.load` raises the run to max(`exposed_level`, item levels) (defence in
    depth against an under-reported `exposed_level`). Item refs carry the service's `content_sha256`
    under that name; `fetch` returns no `sources_sha256`, so B3/E7 takes it from the `context.pack`
    PackItem when the pack lands.
  - E10: the editor catalogue lists the memory kinds where the memory service is configured, and their
    notes name `memory.not_enabled`.

## Phase 06 implementation notes (06A, as implemented)
The full phase 06 record (gates, implementation, checks and reviews) is
`phases/p06/docs/TASK.md` and `phases/p06/docs/REVIEW.md`. This section records only what amends
an earlier decision here.

- D9 retention, amended by review MEM-P06-A1-R1 F5 (recorded as asked in A1-R2 F8 (c)). D9 says
  "Identity, revision, level and digest stay for provenance". As implemented, they stay while
  anything references the source:
  - A source whose content was nulled more than 30 days ago, and that no link and no operation
    names, is deleted (`remove_unreferenced_sources`,
    `services/memory/src/adapters/postgres/retention.rs:541`). The candidates are locked
    first, so a save citing one waits, and the delete re-checks the references in a fresh
    snapshot.
  - A finished operation that no observation names is deleted with its op-source rows more
    than 30 days after it finished (`remove_unreferenced_operations`, same file, line 518).
    Its delete relies on the row lock taken with the candidates (`FOR UPDATE SKIP LOCKED`)
    rather than a re-check; an observation names its operation only from that operation's
    own commit, before it finished.
  - Both run from `sweep_aged_once` (line 405), each in its own transaction.
  - Consequences:
    - re-recording a removed revision creates a fresh source row, because revision immutability
      holds only while something references the revision;
    - `GET generations/{id}` for a removed operation answers `not_found`;
    - OpenBot's capture refs are per exchange and never re-recorded, so OpenBot never meets
      either case.
