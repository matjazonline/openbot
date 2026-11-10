# Review p1-arch-r1 — architecture gate P1 (reviewer B gen 1)

## Artifact identity

- Reviewer: role B, gen 1, nonce `run=e127dfad role=B gen=1`, agent `a636617799682ea85`, root session `6fedfb2e-40aa-486c-bd0b-051b5a3add7b`.
- Base `a6bed48a89d986204d7df13d4b4c5a58458176cf`. Frozen diff `tmp/memory-service-run/frozen/p1-arch-r1.diff`,
  sha256 `f136fd6a6a5b77b559489bc9cde0db5016d90e18e3f6a2ae78aa7ba95fb98272` (recomputed, matches). Working-tree
  `plan/memory-service/implementation-decisions.md` is byte-identical to the diff's added file; `src/main.rs` change is the
  single `NO_BACKWARD_COMPATIBILITY_NOTES` entry (registered once, policy note present at record lines 3–5).

## Coverage

Read fully: README, phases 01–06, the decisions record (414 lines), AGENTS.md. Code checked:
`src/domain/entities/access_level.rs` (`RunKnowledge`), `src/adapters/persistence/workflow/knowledge.rs`
(`admission_levels_on`, `raise_content_level_on`), `src/adapters/embedding/mod.rs` (no truncation, norm 0.001, e5 prefixes),
`src/adapters/persistence/knowledge/search.rs` (`'simple'`), `src/application/workflow/agent_context.rs` (2 × JSON bytes),
`registry/mod.rs`/`families.rs` (`MAX_ITEMS` 256, `MAX_TEXT` 16,384, `memory.load`/`memory.save` schemas), root `Cargo.toml`/
`Cargo.lock`, `.cargo/config.toml`, `.env.example`, `.gitignore`, `.dockerignore`, `Dockerfile`, `.github/workflows/ci.yml`,
`scripts/check.sh`, `stack-budget.sh`, `test-network-isolation.sh`, `prepare-knowledge-model.py`, `assets/knowledge/model-manifest.json`,
`tests/source_modules.rs` and boundary scripts (scan scope), `tmp/claude-mem/src/storage/postgres/observations.ts` (RRF, iterative-scan loop).
Crate sources: sqlx-macros-core/sqlx-postgres 0.8.6 (dotenv behaviour), rmcp 1.8.0 (config, features, `Parts` injection), schemars
version used by rmcp. Local pgvector 0.8.7 control file and GUC strings. Anthropic facts via the bundled claude-api skill
(cached 2026-10-06; live docs not fetched).

## Findings

### Major

**M1 Vector leg can still silently drop eligible rows when the iterative scan stops at `hnsw.max_scan_tuples`.**
Location: record D7 lines 237–244; reference `tmp/claude-mem/src/storage/postgres/observations.ts:528–534`.
Problem: The record copies Claude-Mem's retry condition ("short and every part read was used"). pgvector 0.8 iterative scans
also stop at `hnsw.max_scan_tuples` (default 20,000; GUC confirmed in the installed 0.8.7 library), and in that case the scan
returns *fewer* parts than its LIMIT. That is exactly the "another tenant filled the candidate window" case 03A.4 forbids, and
under the copied condition it reads as "no more parts", so neither the ×4 retry nor the exact scan runs. The text "If it is still
short, run an exact scan" does not say whether the exact scan also runs when no retry was triggered. The record never names
`max_scan_tuples` or `scan_mem_multiplier`.
Required: define shortness as "fewer than `leg_candidates` observations", whatever caused it. Treat a scan that returns fewer
parts than its LIMIT as inconclusive unless a bounded count of eligible parts proves the set is exhausted. Fall through to the
bounded exact scan in every such case. Set `hnsw.max_scan_tuples`/`scan_mem_multiplier` explicitly in the same `SET LOCAL` block,
with documented values. D10's `memory_search_` case must put more than `max_scan_tuples` foreign or above-ceiling parts nearer
the query than a small eligible set, and assert the eligible rows are returned.

**M2 No global lock order; the worker, claim and delete paths deadlock against each other.**
Location: D3 lines 129–136, D9 lines 289–294 and 310–312.
Problem: Writes lock tenant → namespace → credential. Tenant/namespace delete locks the tenant row and then updates its
`generation_operations` rows ("cancels its operations"). The claim transaction locks an operation row (`FOR UPDATE SKIP LOCKED`)
and then updates the tenant's `last_claimed_at`. The commit transaction compares fence/state on the operation row and then takes
`FOR SHARE` on tenant/namespace/credential. Both worker paths take operation → tenant while delete takes tenant → operation: a
lock-order inversion. PostgreSQL will abort one side with a deadlock error, which the record does not handle. Also, keeping
`last_claimed_at` on the tenant row (it is not even listed among the `tenants` columns at D2 line 83) makes every claim conflict
with the `FOR SHARE` taken by every write for that tenant.
Required: state one lock order for every transaction (tenant → namespace → credential → operation/observation rows), including
claim, dispatch CAS, heartbeat, commit, reaper, indexer commit, disable/delete/purge and revoke. Workers take the parent `FOR SHARE`
locks before locking the operation row (or re-select under that order). Move the fairness cursor to a separate per-tenant
scheduling row that writes do not lock. Specify retry-on-deadlock (`40P01`) / serialization handling, and add a competing
delete-vs-commit/claim test to `memory_generation_`/`memory_authority_`.

**M3 A lease can expire while a provider request is still live, which breaks "≤ 1 in flight per tenant" and cancellation.**
Location: D9 lines 291–294 and 301–309.
Problem: "Lease loss drops the request future" only works when the worker notices the loss. A worker that cannot reach the
database cannot heartbeat and cannot see that the lease is gone. Its 120 s provider call outlives the 90 s lease. The reaper then
turns the row into `unknown`, which frees the partial-unique slot, and a second claim for the same tenant can dispatch while the
first HTTP request is still running. The same applies when delete/disable/cancel moves a `dispatched` row to `cancelled`. The
record also does not say what `generations/{id}:cancel` does to a `dispatched` operation.
Required: each worker enforces a local deadline taken from its last successful renewal (monotonic clock, with a margin below
`lease_until`). It aborts the provider future when that deadline passes or a heartbeat CAS fails, so the reaper can only reclaim
after local cancellation. Alternatively, the reaper waits a grace period at least as long as the provider timeout before freeing
the slot. Define cancel/delete of a `dispatched` row (state, spend charged, slot release only after the grace period). Add a test
with a stalled heartbeat against a slow counted provider, asserting there is never more than one in-flight provider call per tenant.

**M4 Disable → enable can revive queued inference depending on worker timing.**
Location: D3 lines 133–135.
Problem: Disable only blocks new admission. Queued work "fails `tenant_inactive` at its next check". If an operator re-enables
the tenant before a worker reaches that check, the old accepted operations run. The outcome depends on scheduling, and 04B.4
forbids a reset or unrelated operation reviving old inference.
Required: disable moves the tenant's `accepted`/`claimed` operations to a terminal state in the same transaction, as delete does,
and handles `dispatched` operations per M3. Enable never revives them; new work needs a new authorized request. Test the
disable/enable race with competing workers.

**M5 Parent-directory configuration still reaches in-tree service builds and tests.**
Location: D1 lines 27–30 and 60–66; code `.cargo/config.toml:1–6`, `.env.example` (`DATABASE_URL=`), sqlx-macros-core 0.8.6
`src/query/mod.rs:389–407` and `:120–126`.
Problem: Dropping `dotenvy` only covers `sqlx::test`. Three other sources still read parent configuration. (a) The sqlx query
macros load `.env` from `CARGO_MANIFEST_DIR` and otherwise from the current directory *and its parents*. A developer's root `.env`
with OpenBot's `DATABASE_URL`, and no `SQLX_OFFLINE`, makes the service compile online against OpenBot's database. (b) sqlx-cli
also loads parent `.env` files unless `--no-dotenv` is passed. (c) Cargo merges the root `.cargo/config.toml`, so in-tree service
tests silently inherit `RUST_MIN_STACK = 16 MiB` while the extracted copy does not. Results then differ between in-tree and
extracted runs, against 01A.1 ("cannot depend on … root configuration") and AGENTS' stack rules. The record sets no service stack
policy (06A.5). CI only exercises the extracted copy, so none of these leaks fail anywhere.
Required: ship `services/memory/.cargo/config.toml` that sets every key the root sets (its own `RUST_MIN_STACK`, kept equal to
the service's runtime thread stack, with a stack-budget re-run if raised) plus `SQLX_OFFLINE = "true"` (non-forced, so `prepare`
can override it). The service's runtime and scripts read only `MEMORY_*` keys, never `DATABASE_URL`. Every `cargo sqlx`
invocation passes `--no-dotenv` and an explicit `--database-url`. `boundary-check.sh` adds an in-tree probe with a poisoned
parent `.env` / `[env]` that must not change the build or test environment.

**M6 `verify.sh` never says which database migrations and `prepare --check` run against.**
Location: D10 lines 357–361 and D2 lines 103–110; compare `scripts/check.sh` header and 06C ("Never migrate or reset a
development/customer database").
Problem: `cargo sqlx migrate run` and `prepare --check` need a migrated database. The test harness only defines a maintenance URL.
Without an explicit target, sqlx-cli falls back to `DATABASE_URL` or a parent `.env` (M5), which can migrate the memory schema
into OpenBot's development database.
Required: `verify.sh` creates its own disposable `memverify_<pid>_<rand>` database from `MEMORY_TEST_DATABASE_URL` (superuser
step: create the extension; then migrate and prepare as a non-superuser owner role, mirroring `provision-db.sh`). It passes that
URL explicitly with `--no-dotenv`, refuses to run when only `DATABASE_URL` is set, and drops the database on exit.

### Minor

**m1 Some phase-05 seam facts are wrong or missing.** D11 lines 374–376: 256 facts × 16,384 chars is at least 4 MiB of ASCII
before JSON framing, so it does not fit the 4 MiB save body (D6 line 216); multibyte text is up to 16 MiB. `memory.load`'s
`query` is `text(MAX_TEXT)` (16,384 chars) against the service's 1,024-char query limit, and that mismatch is not listed. D11
line 372 assumes OpenBot knows each namespace's `read_max` ("min(…, grant)"), but no client endpoint exposes a credential's grants
when an operator provisions by hand. Required: state the split rule (deterministic per-batch idempotency keys, with loss of
single-request atomicity surfaced), list the query-length reconciliation for 05A.3, and add a grant-introspection read (or record
that grants are known from provisioning).

**m2 Several spend-ledger details are missing.** D9 lines 295–300: the conditional `UPDATE` matches nothing when the day's
`tenant_spend_days` row is missing, and that would be misreported as `spend_cap_reached`; it needs an upsert first. Settlement
must use the reservation's recorded UTC day, not the commit day. Actual usage above the reservation (the `2 × bytes + 4,096`
upper bound has not been verified against structured-output schema overhead) needs a metric and a test, as the early-failing
signal. Check `allowance_insufficient` and `write_max ≥ input level` synchronously at acceptance too, not only at claim.

**m3 The structured-output schema includes constraints the API does not support.** D9 lines 318–322: Anthropic structured outputs
reject or strip numeric, string-length and complex array constraints, and require `additionalProperties: false` on every object
(claude-api skill, JSON Schema limitations). The service uses raw HTTP, so no SDK strips them. Required: send a provider-safe
schema without `maxItems`/`maxLength`, with `additionalProperties: false` on every object, and enforce the record's bounds (≤ 16
observations, title/body limits, `event_ids` belonging to the window) with the local `jsonschema` validation.

**m4 The "definite response" retry rule is too broad.** D9 lines 304–306: a 5xx from an intermediary (a gateway 502/503/504
without Anthropic's error envelope or `request-id`) does not prove the request was not accepted. Required: treat only an
Anthropic-shaped error body (`overloaded_error`, `rate_limit_error`, `api_error`) as a safe retry. Any other post-send failure is
`unknown`.

**m5 The canonical idempotency form depends on mutable state.** D5 lines 172–176 and D4 lines 155–156: "defaults" for source
revisions (latest) and source readability are resolved at request time. A legitimate retry after a new source revision or a
source deletion therefore returns 409/404 instead of the original result. Required: saves name explicit immutable source revisions
(01A.4); the hash covers the normalized request plus host-supplied authority facts. Order the steps as: authenticate and authorize
the current credential, then look up idempotency, then check expiry and re-derive.

**m6 Error codes reveal namespace existence and object state.** D3 lines 125–127 and D6 lines 210–213: an ungranted namespace
returns 403 while an unknown one presumably returns 404, and `namespace_deleted` (409) can reveal tombstones. Required: give
ungranted, nonexistent and deleted-but-ungranted namespaces the same response. Check the grant before the tombstone. Specify that
delete/correct of an observation above the caller's read ceiling is `not_found`.

**m7 Context-pack omission and default budgets are underspecified.** D8 lines 257–266: if `omitted` lists item ids/levels, those
are returned metadata and must be counted in `exposed_level`; otherwise `omitted` should be counts only. With the 2 × JSON-bytes
rule, the 2,048-token default allows about 1 KiB, so the 32 KiB byte default never applies and a pack holds about two items.
Required: pick one semantics and coherent defaults.

**m8 Test databases do not exercise the production role path.** D2 lines 103–110: everything runs as a superuser. The production
path (superuser creates the extension, a non-superuser owner migrates and serves) is not exercised, so a migration step that needs
superuser would pass CI and fail in production. Async `Drop` needs a blocking path, and SIGKILLed runs leak `memtest_*` databases.
Required: migrate and run as a created owner role, and sweep stale `memtest_<dead pid>_*` databases at harness start.

**m9 Retention has gaps.** D9 lines 330–333: raw sources recorded via `sources:record` that no operation references never expire,
which contradicts the fixed 30-day raw-evidence decision. Barriers are kept forever with namespace external ids that D11 line 369
calls "company/agent/user names". The user decision requires opaque ids, and 01A.4 says a namespace is not an encoded OpenBot
identity. Required: unreferenced raw evidence expires 30 days after recording (or at its own earlier expiry), and namespace
external ids must be opaque identifiers (or barriers keep only a keyed digest).

**m10 The provisioner scope is mis-described and is ahead of the user's decision.** D3 lines 120–122 and D11 lines 381–383: a
provisioner can issue client credentials with any grant for the tenants it created, so "no memory read access" is false in
effect. Required: describe it as transitive data authority over those tenants. Because D12.2 is undecided and the originals require
only separate administration authority, defer the `provisioner` scope and `created_by_admin` until 05 (YAGNI) or mark them as
conditional on D12.2.

**m11 CI and extraction details are incomplete.** D10 lines 362–364: GitHub runners have Docker, so the non-root,
network-denied, read-only image startup check (as `scripts/check-knowledge-image.py` does for OpenBot) belongs in
`memory-service.yml`. Only the local run is unavailable. `extracted-build.sh` should copy tracked files (`git ls-files`) plus the
explicitly prepared model/runtime assets, so untracked local files cannot make the extracted build pass. Root `.dockerignore` does
not exclude `services/`, so the root image context would carry the service's ~490 MB model assets. Name the pgvector image's exact
version (not "≥ 0.8") and pin the service toolchain to its MSRV. Package the ONNX Runtime library and the model license/notice
files in the service image, as OpenBot's manifest does.

**m12 MCP host validation is not addressed.** D6 lines 199–202: rmcp 1.8.0 `StreamableHttpServerConfig::allowed_hosts` defaults to
loopback only (`tower.rs:116`), so `/mcp` rejects requests in the separate-host topology (06B.3). Required: add an implemented,
documented configuration key for allowed hosts and origins, and a startup check.

**m13 The indexer protocol has no lease renewal and no competing-claimant test.** D7 lines 247–251 and D10 lines 352–355: a 60 s
lease with no heartbeat on up to 8 × 128 parts of CPU embedding can expire mid-work and livelock on slow hosts. Embedding
concurrency (query embeddings at 20 req/s plus the indexer) has no bound. AGENTS requires competing claimants for every new
concurrency protocol, and D10 lists none for indexing, the reaper or the sweeper. Required: add a heartbeat or a measured lease
sized to the bounded work, a blocking-pool semaphore for embedding, and competing-indexer and reaper-vs-heartbeat tests.

**m14 The pin description is inaccurate.** D1 line 52: OpenBot pins `sqlx` as `"0.8"` and `secrecy` as `"0.10"` (caret; lockfile
sqlx 0.8.6), not with `=` pins. Exact-pinning sqlx in the service is good, since it matches sqlx-cli 0.8.6. Correct the rationale.

## Assumptions challenged (accepted unless listed above)

- SHA-256 verifier without a slow hash for 256-bit random secrets: accepted.
- Refusing (not clamping) a ceiling above `read_max`: accepted. It forces multi-namespace searches to the minimum grant (see m1
  for the seam cost).
- Omitting server-side `fallbacks`: accepted and justified. The skill recommends fallbacks by default, but they would route
  classified content to a model outside `provider_max_level`.
- No provider reconciliation: the Messages API has no idempotency key or status lookup. The Message Batches API has status lookup,
  but batch creation is itself ambiguous on timeout and adds latency of up to 24 h. The record should say in one line why batches
  were rejected (04B.3 says "if the provider offers usable … status reconciliation, use it").
- A single global HNSW index across tenants with predicate filtering: accepted only with M1 fixed.
- A database per DB test: accepted (stronger than 02B.5 requires), cost noted.
- An operation registry generating both HTTP routes and MCP tools: accepted as the equivalence mechanism, not over-engineering.

## Claims verified / unverifiable

Verified:
- pgvector is not a trusted extension (local `vector.control` has no `trusted`; contrast `btree_gin` `trusted = true`).
- GUCs `hnsw.iterative_scan` (`strict_order`/`relaxed_order`), `hnsw.ef_search`, `hnsw.max_scan_tuples` and
  `hnsw.scan_mem_multiplier` exist in 0.8.7.
- Claude-Mem revision `3ab6e1a2…`, NOTICE "Copyright 2026 Alex Newman", RRF_K = 60, iterative-scan ×4 loop and the ef_search
  1000 cap.
- rmcp 1.8.0 has features `server`/`macros`/`schemars`/`transport-streamable-http-server`, config fields `stateful_mode` and
  `json_response`, `http::request::Parts` injected into extensions, and depends on schemars 1.x.
- OpenBot is an rmcp client (`=1.8.0`, client features).
- `fastembed =7.1.1`, `ort =2.0.0-rc.13`, `tokenizers =0.23.2`, `reqwest =0.13.4`, `jsonschema =0.33.0` match root pins.
- sqlx lock 0.8.6. `sqlx::test` reads `DATABASE_URL` via `dotenvy` (parent walk). The macros' `.env` fallback is confirmed (M5).
- CI installs sqlx-cli 0.8.6. Host `cargo sqlx` 0.9.0 and rustc 1.96.0.
- Root has no `[workspace]`. Root gates scan only root `src/`/`tests/`.
- `RunKnowledge` ceiling/content, the monotone `GREATEST`, no-truncation tokenizer, norm 0.001, e5 prefixes, `'simple'`, the
  2 × JSON-bytes bound, and `MAX_ITEMS`/`MAX_TEXT`.
- Anthropic, via the bundled claude-api skill (cached 2026-10-06): `anthropic-version: 2026-10-21` is valid. On it,
  `temperature`/`top_p`/`top_k` return 400 on every model. `claude-sonnet-5-5` rejects forced `tool_choice` `any`/`tool` with a
  400. Structured outputs use `output_config.format`. Effort is `output_config.effort`, and Sonnet 5.5 defaults to `high`, so
  setting `low` explicitly is correct. `fallbacks`, top-level `inference_geo`, and the `refusal` stop reason with `stop_details`
  all exist.

Unverifiable here:
- Live Anthropic docs (not fetched; skill cache is one month old).
- Whether 5xx responses are never billed.
- Whether the reserve formula upper-bounds structured-output overhead.
- pgvector 0.8.0 as the first version with iterative scans (from memory; consistent with the GUCs present in 0.8.7).
- Exact `ef_search` maximum, beyond Claude-Mem's comment.

## Verdict

**Findings to resolve.** No blockers. Resolve the 6 majors (M1–M6) in the record before dependent implementation; resolve or
explicitly accept each minor (m1–m14). Re-review is needed only for the changed sections plus D9/D10 consistency.
