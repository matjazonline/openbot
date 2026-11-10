# Memory service

Standalone Rust service: tenants, namespaces, ordered 0–9 access levels and scoped credentials,
with an HTTP API (`/v1`, `/admin/v1`) and MCP (`/mcp`) generated from one operation registry.
Design: `plan/.archive/memory-service/implementation-decisions.md` in the OpenBot repository. This
directory builds and tests on its own (`scripts/extracted-build.sh`).

Status: tenants, namespaces, admin/provisioner and client credentials, grants, the
access-level contract, explicit memory storage (source records, sessions, save including
classification-preserving copies, fetch, correct and delete, with logical idempotency), hybrid
search, timeline, context packing and validation, observation generation, namespace purge,
retention and tenant reassignment are implemented.
Recorded source content and generation windows are redacted before they are stored:
`<private>…</private>` spans are removed and recognised secrets (API-key prefixes, PEM private
keys, credential-shaped bearer/basic/`Authorization:` values, `password=`-style pairs including
prefixed keys such as `DB_PASSWORD=` or `client_secret:`, and URL userinfo passwords) become
`[REDACTED:<kind>]` (redaction version 1). Identifiers are stored as given, so secret-shaped ones,
including keyword-shaped ids like `token:7`, are refused with `secret_in_identifier`. The
patterns are not exhaustive; clients should still not record secrets.

Recall is progressive. Search returns a compact index (ids, titles, snippets of at most 320
bytes, timestamps, levels, revisions, visible provenance, indexing state; no bodies). Timeline
returns the anchor and up to 20 of its nearest visible neighbours on each side (default 5),
oldest first, from the named namespaces (optionally only the anchor's session). Fetch returns
whole bodies for at most 64 selected ids. Every read authorizes the credential's current
grants and applies the tenant, namespace, ceiling and expiry filter itself, whatever an earlier
search returned: records the reader may not see take no slot and leave no count, gap marker or
provenance link, and each response's `exposed_level` is the highest level of what it returned,
provenance included.

A context pack (`context.pack`) takes up to 200 candidates (the search results for a query,
else the most recent observations), then includes whole items in that order while they fit both
budgets: `max_tokens` (default 8,192; a pack counts 64 + 2 × the UTF-8 bytes of its items'
compact JSON, 0 when empty) and `max_bytes` (default 64 KiB, the same bytes counted exactly).
An item that does not fit is skipped and the scan continues; `omitted.budget` counts skipped
visible candidates only. `detail` is `compact` (snippets, the default) or `full` (bodies),
never partial bodies. Each item carries its id, namespace, kind, title, text, time, level,
revision, `content_sha256` and visible provenance. The pack's `exposed_level` is the highest
level actually included, never the requested ceiling or a skipped candidate; a client combines it
with its own consumed watermark. The pack is structured data: the service builds no prompt,
message roles or instructions, and stored text, including text that looks like instructions or
markup, is returned as ordinary JSON strings for the host to treat as untrusted. A client that
freezes a pack keeps `pack_digest`, `valid_until` (the earliest expiry of the items and their
visible provenance) and each item's id, revision and `sources_sha256`, and later calls
`context.validate`, which answers `valid`, `changed` (new revision or different visible
provenance), `expired` or `unavailable` per item without content. `byte_count` and the token
count cover the items' compact JSON as served (not the array's brackets and commas, the envelope,
or a client's re-encoding); a `full` pack is selected on exact sizes before any body is loaded. Validation cannot retract text already sent to a model:
the client owns its retained packs and model requests. `contract/client.py` is an independent
standard-library Python client of the published HTTP and MCP contract.

Search fuses a PostgreSQL full-text leg (`'simple'` configuration: observations holding every
one of the query's first 32 words, or any identifier-like raw token, matched as a phrase: a token
with a digit, `_ . @ / : -` inside, or an internal capital; never a bare capitalised word) and a pgvector leg over
searchable parts with reciprocal rank fusion (k = 60). Both legs filter by tenant, named
namespaces, ceiling, active namespace and expiry before any limit; a filtered HNSW iterative scan
that stops short falls back to an exact scan of at most 100,000 eligible parts, and beyond that the
response says `semantic: "partial"`. A saved observation is `pending` until the supervised indexer
has split it into at most 128 parts of at most 500 model tokens (never truncated) and embedded
them; keyword recall works meanwhile and while indexing is `failed`. Failed indexing retries with
backoff (30 s doubling to 1 h); after 8 attempts it stays `failed` until `admin reindex-failed`.

The embedding model is `intfloat/multilingual-e5-small` at a pinned revision (fp32, mean pooling,
384 dimensions, e5 `query: `/`passage: ` prefixes), with a pinned ONNX Runtime, both recorded in
`assets/model-manifest.json`. `scripts/prepare-model.py --download` fetches and verifies them into
the gitignored `assets/model/` (build network only); serving validates sizes and SHA-256 and never
downloads. Startup fails when the model, pgvector >= 0.8.0 or the `vector(384)` column is missing,
or when the database was indexed with another model (reset it: vectors of different models are
never compared). There is no keyword-only mode.

Resource budget: two model sessions (the reserved query slot and the shared indexing slot), each
one intra-op thread, so embedding uses at most 2 CPU threads; a query waits at most 1 s for a slot,
then gets `429 capacity_exceeded`. Model files are read once and verified in memory, then both
sessions load those bytes. Peak RSS of `model-check` with both sessions was 2.34 GB (2,228 MiB) on
macOS arm64 (2026-11-08); the CI image job fails above 2,560 MiB (the first Linux run calibrates
it; lower it to the measured value plus headroom rather than raising it silently). The indexer is
supervised (restarted after a panic); so are the generation workers and the maintenance task
(reaper and retention sweep). Server drain and worker stop share one `MEMORY_SHUTDOWN_GRACE_MS`.

Request bodies are bounded per operation (256 KiB, or 4 MiB for saves, source records and
generation requests) on `/v1` and for each MCP tool call; `/mcp` refuses bodies above 4 MiB plus
64 KiB of JSON-RPC framing with `413 payload_too_large`. The schemas publish the remaining D6
bounds (item counts, text lengths, id patterns, timeline depth, pack budgets). Reads (search, timeline, fetch, context pack and validate) write no audit events;
writes audit identically on both transports. Tenant and namespace
creation are idempotent by external id; issuing a credential is not (the secret is shown once and
only its verifier is stored), so a retried issue mints a second credential to list and revoke.

Generation (`generations.request`, `status`, `cancel`) turns one complete, bounded window of
activity (at most 512 events and 512 KiB) into typed observations through Anthropic's Messages
API (`claude-sonnet-5-5`, structured output, thinking off). It is off unless
`MEMORY_ANTHROPIC_API_KEY` is set *and* an operator gives the tenant a daily token cap above 0
(`generation_disabled` otherwise). Acceptance is durable and does not call the provider: after
privacy (excluded events dropped, `<private>` spans and recognised secrets removed), the
window's text is recorded once as source revisions in the namespace (an identical revision
already recorded is reused; one with other content, level or session is `409 conflict`; one
whose content retention removed is `409` with `source_unavailable`; a revision above the
caller's read grant is reused only by the credential that recorded it, and is a uniform `409`
for any other, so a write-up grant cannot probe what it cannot read), and the operation stores
its frozen label (the producing ceiling, the consumed watermark or its ceiling fallback, and
every sent event's level). Every level a request is authorized for may be sent to Anthropic,
whose standard data handling applies: there is no zero-data-retention or pinned inference
region. Up to 4 supervised workers dispatch accepted operations, at most one per tenant at a
time, tenants in round-robin order. Each dispatch reserves `2 × prompt bytes + 4,096 +
max_output_tokens` tokens against the tenant's UTC-day cap (a day that cannot hold it fails the
operation `spend_cap_reached`), commits `dispatched` before it sends, and holds a 90 s lease
renewed every 30 s; the call is dropped at a local deadline 15 s before the lease ends, when a
renewal fails (a cancel, revoke, disable or delete), or when a shutdown has used 80 % of
`MEMORY_SHUTDOWN_GRACE_MS` (a call that answers before then is saved; no new claim starts). A definite provider
rejection (rate limit, overload, transient API error) is retried after 30 s × 2ⁿ, at most three
dispatches; a rejection of the service's own key (`authentication_error`, `permission_error`,
`billing_error`) pauses generation instead of failing operations (see Operations); anything that may have run (a timeout, a lost lease, a crash after the send) becomes
`unknown`, is charged and is never re-sent, and output received but not yet saved by then is
lost. Results, their source links, the spend settlement and completion commit in one fenced
transaction; generated observations then index like any other. Nothing revives a finished
operation: a login, quota reset, restart or tenant enable never resends anything, and recovery
is a new request. Statuses: `accepted`, `claimed`/`dispatched` (running), `completed` (`saved`,
`nothing_to_save`, `nothing_to_process`), `failed`, `cancelled`, `expired`, `unknown`, each with a
bounded outcome code and never provider text. Retention: a request's stored window list and the
recorded source content are cleared 30 days after the last operation using them finished (or
30 days after recording when none did, or at a source's own expiry), never while an unfinished
operation uses them; identity, revision, level and digest stay for provenance while anything
references the source (see Retention).

## Deletion, purge and forgetting

- **Correct** overwrites an observation in place (title, body, `revision + 1`); the old text, its
  vectors and its links go at once and no text history is kept. **Delete** removes the
  observation with its vectors and links at once.
- **Namespace delete** (admin) and **purge** (`namespaces.purge`, a client with the `purge`
  grant) tombstone the namespace in one transaction: every credential's grant on it is removed,
  accepted and claimed generation operations are cancelled, and a dispatched call is flagged so
  it cannot renew its lease; its late answer is discarded. Reads and writes stop at that commit.
  The namespace's content (observations, vectors, links, sources, sessions, operations) is then
  removed by the maintenance task in bounded batches, but only once none of its operations is
  unfinished, so no stale worker can write into it afterwards. The tombstone is kept forever and
  its external id can never be recreated (`409 namespace_deleted`). A purge retried by the
  credential that executed it replays (`replayed: true`); any other caller gets `403`.
- **Tenant delete** also tombstones every namespace of the tenant and removes all its grants.
  After their content is gone, the tenant's idempotency records, spend days and schedule go too,
  and every credential label is replaced by `removed` (labels are free text). Credential rows
  are kept (a verifier hash and timestamps), so an old token still answers
  `credential_revoked`.
- **Limits of deletion.** The service cannot reach copies outside itself: text already returned
  to a client, placed in a prompt, transcript or frozen preparation, or sent to Anthropic (no
  zero-data-retention, see above). A copy saved in another namespace (a parent cited across
  namespaces) belongs to that namespace and keeps its text and label when the original is
  deleted or purged; only its parent link goes. Restoring a backup taken before a deletion
  brings the deleted content back, and the restored audit log does not record that deletion;
  deletions (and every client's observation deletes and corrections) made after the backup must
  be re-applied before clients are served (see "Backup and restore" under Operations).

## Retention

Maintenance (one supervised task with the reaper) removes at most 500 rows per category per
pass, runs again after 100 ms while a category is full, else every 5 s, and backs off on errors.

| Data | Kept |
| --- | --- |
| Observations, vectors, links | until corrected, deleted or their own `expires_at`; then removed |
| Raw evidence (source content), generation requests | 30 days after the last operation using them finished (or 30 days after recording, or the source's expiry), never while an unfinished operation uses them |
| Source metadata (id, revision, level, digest) | while a link or operation references it; then 30 days after its content was cleared. A removed revision recorded again is a new source |
| Finished generation operations | while an observation references them; otherwise 30 days after they finished (`GET generations/{id}` is then `not_found`) |
| Idempotency records (results hold ids and levels, never text) | 7 days and until the request's own deadline has passed; a later attempt carrying that deadline is `410 request_expired`. A generation key reused with a new deadline is `409 idempotency_conflict` while its operation exists |
| Audit events (ids and counts, no content) | 400 days; the schema refuses deleting a younger event |
| Daily spend rows | 400 days |
| Deleted namespaces and tenants | tombstones forever (opaque ids); their content as above, after inference settles |
| Credentials | for the tenant's life, revoked and expired ones included (list them with `active_only`); labels replaced by `removed` when the tenant is removed; rows kept with the tombstone |

## Observability

- **Request ids.** Every answer carries the service-minted `x-request-id` (also in error bodies
  and audit rows). A client may send `x-correlation-id` (1–128 bytes of `[A-Za-z0-9._:-]`,
  opaque and non-secret; a malformed one is `400 invalid_request`). On `/mcp` it applies to the
  call its HTTP request carries. It is logged with each request, stored on the audit row and,
  for a generation, on the operation, so the worker's dispatch, lease-loss and settle lines and
  the settle audit row carry it too. It is never authority: it is not part of a request's
  identity and is never compared for replay.
- **Logs** carry ids, operation names, outcome codes and counts only: never a prompt, query,
  memory text, event text, token or credential.
- **`GET /admin/v1/metrics`** (operator credential only; provisioners get 403, clients 401)
  returns Prometheus text with these series:
  - per-process counters (sum them across instances): `memory_requests_total{operation,code}`
    (capacity refusals as `operation="unrouted",code="capacity_exceeded"`),
    `memory_generation_settled_total{state,outcome}`, `memory_generation_dispatched_total`,
    `memory_generation_retries_total`, `memory_generation_lease_lost_total{reason}`
    (`renewal_cancel_requested`, `renewal_lost`, `settle_stale`),
    `memory_generation_reaped_total{kind}`, `memory_provider_tokens_total{kind}`,
    `memory_generation_usage_exceeded_reservation_total`, `memory_index_commits_total{result}`,
    `memory_search_semantic_partial_total`, `memory_context_packs_total`,
    `memory_context_omitted_total`, `memory_retention_rows_total{category}`,
    `memory_maintenance_errors_total` and `memory_provider_auth_failures_total{kind}`;
  - database-wide gauges (read them from any one instance), from one read-only query with a 2 s
    statement timeout: operations by state, the oldest accepted age, `unknown` outcomes in the
    last day, tenants waiting, the largest per-tenant queue, pending and failed indexing, and the
    oldest pending age. If the query cannot finish, the export still answers, with
    `memory_metrics_query_ok 0` and no stale gauges;
  - also `memory_ready`, `memory_maintenance_seconds_since_pass` and
    `memory_provider_auth_breaker_open` (per process: 1 while its generation is paused).
  Labels come only from closed sets, never from tenant or namespace ids.

## Provisioner rotation and listing

`admin.tenants.reassign {from, to, limit?}` (operator only) moves up to 500 tenants per call,
in tenant id order and in any state (tombstones included), from admin credential `from` (which
may be revoked) to `to`, an active, unexpired provisioner credential other than `from` (else
`409 conflict` with `details.reason = "invalid_target"`). It returns `{moved, remaining}`; repeat
until `remaining` is false. A request the old manager already holds a tenant lock for finishes
first; its next request is `not_found`. Each move is audited (`tenant.reassign`).
`admin.credentials.list {tenant, label?, active_only?, after?, limit?}` returns one page (1–1,000,
default 1,000) in credential id order with `next_after` set when the page is full; it answers
for a tenant in any state its caller manages.

## Operations

### Rotating credentials

- **Operator and provisioner credentials:** issue the new one (`admin.admin_credentials.issue`),
  switch the caller to it, and for a provisioner move its tenants with `admin.tenants.reassign`
  until `remaining` is false (see "Provisioner rotation and listing" above). Then revoke the old
  one (`admin.admin_credentials.revoke`). A revoked credential answers `credential_revoked` at
  once.
- **Client credentials:** issue a new one with the grants the client needs
  (`admin.credentials.issue`), switch the client, then revoke the old one
  (`admin.credentials.revoke`). A source revision recorded above the old credential's read grant
  (a write-up grant) is reused only by the credential that recorded it. After a rotation the new
  credential gets the uniform `409` for it, so the client records the source as a new revision.

### Provider outage, quota and credentials

- An outage, rate limit or overload is a definite rejection and is retried (above). A call that
  may have run becomes `unknown`, is charged and is never re-sent; watch
  `memory_generation_settled_total{state="unknown"}` and the gauge of `unknown` outcomes in the
  last day.
- A tenant's daily cap fails operations that cannot fit `spend_cap_reached`. Raise it with
  `admin.tenants.settings`; nothing already failed is retried, so the client sends a new
  request.
- A rejection of the service's own key, its permissions or the account's billing
  (`authentication_error`, `permission_error`, `billing_error`) opens a per-process breaker. The
  operation goes back to `accepted` with that dispatch not counted, and no worker of the process
  claims for 5 minutes. After the window one worker calls once: another such rejection doubles
  the window (up to 1 hour), and any other answer closes it. Watch
  `memory_provider_auth_breaker_open` and `memory_provider_auth_failures_total{kind}`; an error
  log line marks each opening. Fix the key or the account: generation resumes at the next probe.
  After a restart with a new key, new operations run at once, and each paused one when its wait
  (at most the window it was paused for) ends. Requests whose `expires_at` passes meanwhile
  expire (no inference ran).

### Explicit recovery

- `admin reindex-failed` requeues observations whose indexing failed terminally.
- OpenBot re-drives its own lifecycle rows (its deploy guide).
- Nothing revives a finished operation: recovery from `failed`, `cancelled`, `expired` or
  `unknown` is a new request.

### Backup and restore

The database is the only state. Back it up with `pg_dump -Fc` (or the platform's snapshots). To
restore:

1. Keep clients away: stop OpenBot's memory traffic (unset `MEMORY_SERVICE_URL` there) or block
   the service's port.
2. Restore into a fresh database (`pg_restore`, as a role that may create the `vector`
   extension, or with it already created).
3. Run `memory-service migrate`, then `memory-service admin after-restore`. It finishes every
   unfinished operation as `cancelled(restored)` and releases its reservation: whether a
   dispatched call ran after the backup cannot be known, and its output is not in the backup.
   A restore or restart therefore never replays inference. Running it again changes nothing.
4. Re-apply what the backup predates and the service cannot know:
   - revoke again every admin credential revoked after the backup;
   - re-apply tenant settings and caps changed after it (`admin.tenants.settings`);
   - for OpenBot, run its restore steps (its deploy guide, "Restoring the memory service").
   They delete again what it deleted, and replace client credentials the backup lacks.
5. Reopen traffic.

What a restore loses or undoes:
- Memories saved, generated or corrected after the backup are lost. OpenBot does not resend
  settled saves: that would be a new execution under a lost idempotency record.
- Other clients' deletes and corrections made after the backup come back, and the restored
  audit log does not record them. Those clients must re-apply them.
- Credentials issued after the backup do not exist, so their tokens answer `401`.

`scripts/backup-restore-check.sh` exercises the dump, restore and after-restore on disposable
databases. CI runs it.

### Stack and memory budgets

The process runs Tokio with `MEMORY_RUNTIME_THREAD_STACK_BYTES` (2 MiB, startup refuses any other
value), and its tests run on libtest threads of the same 2 MiB (`.cargo/config.toml`
`RUST_MIN_STACK`). The two move together; raising either needs a stack-budget gate. The model
process's peak RSS is checked against its budget by `model-check` in CI.

## Configuration (environment only; no `.env` file is read)

| Key | Default | Checked at startup |
| --- | --- | --- |
| `MEMORY_DATABASE_URL` | required | must be `postgres://`; connection within `MEMORY_DB_CONNECT_TIMEOUT_MS`, then migrations (pgvector ≥ 0.8.0 required) |
| `MEMORY_BIND_ADDR` | `127.0.0.1:8085` | parses as host:port |
| `MEMORY_MCP_ALLOWED_HOSTS` | `localhost,127.0.0.1,::1` | non-empty; required when the bind address is not loopback |
| `MEMORY_MCP_ALLOWED_ORIGINS` | empty (no browser origins) | — |
| `MEMORY_DB_MAX_CONNECTIONS` | 16 | 1–64 |
| `MEMORY_DB_ACQUIRE_TIMEOUT_MS` | 3000 | 100–30000 |
| `MEMORY_DB_STATEMENT_TIMEOUT_MS` | 5000 | 100–60000 |
| `MEMORY_DB_CONNECT_TIMEOUT_MS` | 10000 | 100–120000 |
| `MEMORY_MAX_IN_FLIGHT` | 64 | 1–64; excess requests get `429 capacity_exceeded` |
| `MEMORY_REQUEST_DEADLINE_MS` | 10000 | 100–60000; exceeded `/v1`, `/admin/v1` and `/mcp` requests get `504 timeout`; an MCP tool call is also bounded where it runs and answers the `timeout` envelope as a tool error |
| `MEMORY_SHUTDOWN_GRACE_MS` | 10000 | 0–120000 |
| `MEMORY_RUNTIME_THREAD_STACK_BYTES` | 2097152 | must equal the default; raising it needs a stack-budget gate |
| `MEMORY_LOG` / `MEMORY_LOG_FORMAT` | `info` / `json` | invalid values fall back and are logged |
| `MEMORY_MODEL_DIR` | `assets/model` | not blank; at startup every manifest file must match, then both sessions load and self-check |
| `MEMORY_ANTHROPIC_API_KEY` | unset (generation off) | secret, never logged; 1–512 visible ASCII characters. Startup does not contact the provider, so a wrong key shows up at the first call: generation pauses and `memory_provider_auth_breaker_open` is 1 |
| `MEMORY_ANTHROPIC_BASE_URL` | `https://api.anthropic.com` | needs the key; `https://`, or `http://` only to a loopback host; no credentials or query |
| `MEMORY_ANTHROPIC_EFFORT` | `low` | needs the key; `low`, `medium` or `high` |
| `MEMORY_GENERATION_WORKERS` | 4 | 1–4: the global number of concurrent generation calls |

## Commands

`memory-service serve` (default), `migrate`, `admin bootstrap [--label L]` (prints the first
operator token once), `admin reindex-failed` (requeues terminally failed indexing),
`admin after-restore` (after restoring a backup, before `serve`: see Operations),
`model-check` (no database: manifest, both sessions, self-check and probes, as JSON), `openapi`,
`version`. Provision a database with `scripts/provision-db.sh`
(pgvector needs a superuser once). Checks: `scripts/focused.sh <prefix>`, `scripts/verify.sh`,
`scripts/backup-restore-check.sh` (needs `MEMORY_PG_BIN`, the directory of a `pg_dump` and
`pg_restore` matching the server's major version), and `scripts/standalone-journey.sh`: a generic
client walks the whole API against a copy of the service built outside any checkout, using a
Messages API stand-in (`MEMORY_JOURNEY_PROVIDER=anthropic` makes it one paid call, capped by
`MEMORY_JOURNEY_CAP`).
