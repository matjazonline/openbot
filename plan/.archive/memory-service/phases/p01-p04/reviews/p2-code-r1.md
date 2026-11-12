# Review p2-code-r1 — P2 code (01A, 01B, 02A) and closure of P1 R1–R3 (reviewer B gen 1)

## Artifact identity

- Reviewer: role B gen 1, nonce `run=e127dfad role=B gen=1`, agent `a636617799682ea85`.
- Frozen artifact: `tmp/memory-service-run/frozen/p2-code-r1.diff`, sha256 `04cea2c0266ff9892e5d1e7eeb57b775f685587d9f173e8ab20d9f5bc1215c28` (recomputed and matches), base `a6bed48a`, tree
  `60bf672ee93b3c58914ab1bad012be98e7776c84`. I recomputed the worktree tree with `freeze.sh`'s method at review time and it is identical, so I read the
  working copy as the frozen content.

## Coverage

Read in full:
- `migrations/0001_init.sql`.
- `adapters/postgres/{mod,test_support,authority_tests,migration_tests}.rs`.
- `application/{service,operations,test_store}.rs`.
- The `contract.rs` request types.
- `domain/{access_level,admin_policy,credential,error,grant}.rs`.
- `adapters/http/{mod,openapi,contract_tests}.rs` and `adapters/mcp/mod.rs`.
- `infra/{config,startup,startup_tests,telemetry}.rs`, `main.rs`, `boundary_tests.rs`.
- `scripts/*` (boundary_check.py, focused, verify, extracted-build, provision-db, network isolation; skimmed prepare-model).
- `Cargo.toml`, `.cargo/config.toml`, `rust-toolchain.toml`, `Dockerfile`, `.dockerignore`, the service README/NOTICE, `.github/workflows/memory-service.yml`, the root `.dockerignore` change, and the phase 01/02 and record changes (R1–R3 text and P2 notes).

Checked against crate sources:
- rmcp 1.8.0 collects the request body with no limit (`transport/common/server_side_http.rs:178`).
- `CallToolResult::structured_error` sets `is_error: true`.
- PostgreSQL 18 `kwlist.h`: `day`, `at`, `key`, `label` and `action` are all unreserved.

Commands I ran (non-DB only; logs in `tmp/memory-service-run/logs/b-p2-*.log`):
- `scripts/boundary-check.sh` → `boundary: ok`.
- `cargo test --locked --offline --lib` over the non-DB `memory_contract_|memory_mcp_|memory_access_level_|memory_startup_*|memory_authority_*(domain)|memory_boundary_` selection → 31 passed. One DB-backed test caught by the filter (`memory_boundary_separate_database_roles_cannot_cross`) failed explicitly on the unset `MEMORY_TEST_DATABASE_URL`, with the setup message. That confirms DB tests fail rather than skip.

Not run (no DB, no Docker, no sqlx-cli 0.8.6): every DB-backed test, `verify.sh`, `extracted-build.sh`, migrate/prepare, the image job. All SQL below was reviewed by reading only; none of it has ever been parsed by PostgreSQL.

## Findings

### Major

**F1 The bundled model licence's digest breaks on any checkout, so the CI gate fails at "Model manifest".**
Location: `services/memory/assets/bundled/MODEL-LICENSE.txt`, `assets/model-manifest.json` `bundled_files`, root `.gitattributes`.
Problem:
- The working file has CRLF line endings (sha256 `904dc4…`, which is the value the manifest records).
- With `core.autocrlf=input` and no `-text` attribute (root `.gitattributes` exempts only `assets/knowledge/MODEL-LICENSE.txt`), git stores LF. The frozen tree's blob hashes to `b5cb7ff7…`.
- So any clone, including CI's checkout, fails `prepare-model.py --check-manifest` (verify.sh step "Model manifest").
- `git diff --check` will also flag the CRs once the file is tracked.
Required: add `services/memory/.gitattributes` with `assets/bundled/MODEL-LICENSE.txt -text whitespace=cr-at-eol`. It must live in the service directory so it travels with the extracted copy. Re-add the file and confirm the committed blob's digest equals the manifest.

**F2 `/mcp` has no request-body bound and no request deadline.**
Location: `adapters/http/mod.rs:185–195` (only `limit` and `mcp_auth` wrap the rmcp service); rmcp `server_side_http.rs:178` (`body.collect()`, unbounded).
Problem:
- `/v1` routes enforce 256 KiB / 4 MiB through `read_json` and the 10 s `timeout` in `handle` (lines 68–104).
- MCP requests bypass both, so any authenticated client can stream an unbounded body that is buffered in memory, and tool calls have no deadline.
- This breaks 01A.5 bounds, 03B.2 "same … body/result bounds" equivalence, and the README promise "exceeded requests get `504 timeout`".
- `tower-http` (`limit`, `timeout`) is already a dependency but unused.
Required:
- Wrap the `/mcp` service in `RequestBodyLimitLayer` with the Large limit (4 MiB, so the largest tool body fits; or per-tool checks against `spec.body_limit`).
- Apply the same `request_deadline` to tool calls, returning the envelope (`payload_too_large` / `timeout`).
- Add `memory_mcp_` cases for an oversized body and a deadline.

**F3 (probable; unverified) The Linux network-isolation wrapper likely blocks the CI PostgreSQL service container.**
Location: `services/memory/scripts/test-network-isolation.sh:13–20` vs root `scripts/test-network-isolation.sh:16–39`.
Problem:
- The root script exists because "Docker's loopback-published PostgreSQL is allowed by its original destination (OUTPUT DNAT happens before the filter table)". It adds a `conntrack --ctorigdst 127.0.0.1 --ctorigdstport 5432 ACCEPT` rule, and OpenBot's CI needs it on the same runner type with a `ports: 5432:5432` service.
- The service copy dropped that rule and also the isolation self-test (`network-isolation.py`).
- If the runner DNATs loopback-published ports, every DB-backed test under `verify.sh` fails to connect in CI. The only evidence would be an unrelated red build.
- Even where it works, nothing proves egress is actually denied.
Required: port the conntrack rule and a self-test (an external connection must fail, loopback PostgreSQL must succeed) into the service script, then confirm the first CI run.

### Minor

- **m1 Admin writes never re-check the acting admin credential inside the transaction.**
  - Location: `adapters/postgres/mod.rs` (`change_tenant_once`, `issue_credential_once`, `create_*`, `revoke_*`); `service.rs:94–107`.
  - Problem: Admin authentication reads `admin_credentials` once, outside the write transaction. A provisioner or operator revoked concurrently can still commit actions it was authenticated for. D13's lock order has no place for `admin_credentials`, unlike the D3 rule that writes take the credential row `FOR SHARE`. Client writes in 02B will need the same in-transaction re-check.
  - Required: lock the admin credential `FOR SHARE` first and re-check revocation and expiry; add it to D13; test revoke racing an admin action.
- **m2 Bootstrap counts expired operators** (`mod.rs:618–621`). An expired-but-unrevoked operator blocks bootstrap permanently, so if every operator credential expires the service needs DB surgery. Count only operators that are neither revoked nor expired.
- **m3 Operation cancellation does not lock in id order and leaves `reserved_tokens` on cancelled rows** (`mod.rs:278–295`).
  - The `UPDATE … WHERE state IN (…)` locks in scan order, not the D13 "operations in id order". Namespace delete and revoke both hold only tenant `FOR SHARE`, so they can overlap and deadlock. That is retried, but contradicts the record.
  - Cancelled rows keep a non-zero `reserved_tokens` after the day row is released, which invites a double release by later reaper or settlement code.
  - Required: select ids `ORDER BY id FOR UPDATE` first, and zero `reserved_tokens` in the same update.
- **m4 Gaps in the migration's integrity rules.**
  - The `sources_are_immutable` trigger leaves `session_id`, `redaction_version` and `expires_at` mutable (`0001_init.sql:144–148`), so a source can be re-homed to another session.
  - `observation_links.source_level` is not tied to the source's real level. Use a composite FK `(tenant_id, source_id, source_level)` → `sources (tenant_id, id, access_level)`, as `observation_parts` does, so a link cannot under-label (02B "source-label tampering").
  - `audit_events` has no `request_id` column, although D2 lists one.
- **m5 Grant validation and the domain disagree about write and read operations.**
  - `validate_grants` (`service.rs:364–388`) requires `read_max` for read operations but accepts write operations (save, sources, sessions, generate, purge, delete) with `write_max = NULL`, which produces grants that can never be used.
  - `Operation::reads()` (`grant.rs:64–74`) omits `Delete`, yet `DeleteRequest` carries `read_ceiling` and D4 requires above-ceiling deletes to be `not_found`.
  - Required: write operations need `write_max`; decide the read/write classification of Delete and Purge before 02B uses `authorize`.
- **m6 Published schemas carry no bounds.** `openapi.json` and the MCP `inputSchema`s have no `maxItems`/`maxLength` (count 0). The D6 bounds (items ≤ 256, namespaces ≤ 16, title/body/query sizes) live only in the record. 01A.5 asks the contract to define them. Required: add schemars length and count attributes, or document the bounds in the schema descriptions, and test them.
- **m7 Admin creates are not idempotent.**
  - `admin.credentials.issue` has no idempotency key, so a retried issue mints a second live credential.
  - `tenants.create` and `namespaces.create` answer a retry with 409 instead of the original result (01A.6 "give all writes logical idempotency").
  - Required: an idempotency key for admin creates, or a documented exemption with the rationale (creates are keyed by external id; issue is interactive).
- **m8 The CI entrypoint and the layering rule are not as the record states.**
  - `extracted-build.sh:8` uses `git ls-files --cached --others --exclude-standard`, which includes untracked files. That is needed while nothing is committed, but the record says "only tracked".
  - `boundary_check.py` does not implement D1's rule that `domain/`/`application/` must not name `sqlx`/`axum`/`reqwest`/`rmcp`/`fastembed`/`ort`/`crate::adapters`. The code obeys it today (grep is clean), but nothing enforces it.
  - `verify.sh:37` builds the extension URL as `${base%/*}/${db}`, which breaks when `MEMORY_TEST_DATABASE_URL` has no database path.
- **m9 `.sqlx/` is empty and every statement uses runtime `sqlx::query`.** `cargo sqlx prepare --check` therefore passes vacuously. This matches OpenBot's style (2155 runtime vs 29 macro queries), but SQL correctness then rests entirely on the DB-backed suite. Required: state this in the record; the DB suite becomes a hard acceptance condition (below).
- **m10 Unused speculative dependencies.** `tower-http` (`limit`, `trace`, `timeout`) is unused, and `tower` (`limit`, `load-shed`) is used only through `ServiceExt` in tests (`Cargo.toml`). Use them for F2 or remove them.
- **m11 Test strength is overstated in places.**
  - The "every unimplemented op answers 501" loop silently skips operations without an example file (`contract_tests.rs:308–330`); only 7 examples exist for 16 client operations. Generate a minimal valid body per operation, or assert an example exists.
  - `memory_authority_disable_races_competing_claimers_without_revival` races an ad hoc claimer SQL, not production claim code (which arrives in 04B). The phase record should say so, rather than counting it as the D10 competing-claimer proof.
  - Untested success paths: `set_tenant_cap`, `list_credentials`, `revoke_admin_credential`, repeated namespace delete.
- **m12 Small leaks of existence or detail.**
  - `admin.tenants.create` returns 409 for a tenant owned by another admin, so a provisioner learns that another principal's external id exists (D3 says out-of-authority tenants are indistinguishable from missing). Low impact (admin-only, opaque ids); note it or return the same error as a fresh conflict.
  - `admin bootstrap --label` with no value hits the label CHECK and surfaces as a generic 503 `unavailable`.

## R1–R3 closure (P1 gate)

- R1 is closed in code and record. A re-disable of an already-disabled tenant is a no-op that keeps `disabled_by` (`mod.rs:331–334`, `admin_policy.rs:47–56`). A provisioner enable of an operator-disabled tenant gets 403 (`mod.rs:356–362`). The DB test `memory_authority_operator_disable_is_sticky…` covers the sequence but has not run.
- R2 is closed in code and record.
  - Issue locks tenant `FOR SHARE`, then the namespaces `ORDER BY id FOR SHARE`, re-checking `active` (`mod.rs:432–455`).
  - Delete takes the namespace `FOR NO KEY UPDATE` and removes its grants (`mod.rs:785–811`).
  - `grants_for` lists only active namespaces (`mod.rs:509–515`).
  - The race test exists but has not run.
- R3 is closed. `ARRAY_FLAGS` always fails a parent `build.*`/`target.*` `rustflags` or `rustdocflags` (`boundary_check.py:8, 29–31`). `focused.sh:8` runs the check, and I ran it (`ok`).
  - Residual nit: other array-valued parent keys are caught only because the service sets none. If the service ever sets one, `isinstance(value, list)` should always fail.

## Requirement coverage notes

- 01A: the contract surface is complete and declared (OpenAPI and MCP from one registry, with a freshness test). Opaque ids, an idempotency key on every client write, and the error taxonomy are in place. 501 can never look like success (HTTP 501; MCP `isError: true`; `x-memory-implemented: false`). Not yet demonstrable:
  - The 01A focused cases for conflicting retries, context exhaustion and sibling namespaces need 02B/03B use cases. The phase file should list them as deferred, not passed.
  - Bounds are not in the schemas (m6).
- 01B:
  - Credentials store verifiers only, compared in constant time.
  - Unknown id and wrong secret give the same 401, and lifecycle errors come only after the secret matches.
  - Kind separation holds both ways (admin tokens never reach `/v1`/`/mcp`, client tokens never reach `/admin`).
  - The provisioner bound (`may_manage`, operator-only caps and admin credentials) matches decision (c), with its transitive authority documented.
  - Narrowing never widens: `authorize` refuses rather than clamps (domain-tested, not yet wired into use cases).
  - Disable, delete and revoke cancel accepted/claimed operations atomically and flag dispatched ones.
- 02A:
  - Configuration reads only `MEMORY_*` keys, reports every problem, redacts the URL, and the README lists exactly the keys the code reads (apart from the MCP deadline claim, F2).
  - Tracing starts before config; readiness tracks ping and shutdown; graceful shutdown is bounded.
  - The non-root image uses digest-pinned bases, uid 10001 and port 8085.
  - There are no git dependencies, the stack is 2 MiB both ways, and extracted-copy CI exists.
  - Open items are F1, F3 and m8.

## Checks that remain required before P2 acceptance

1. The whole DB-backed suite against PostgreSQL 18 + pgvector 0.8.7, via `focused.sh` with nonzero counts recorded:
   - `memory_authority_` (8 DB cases), `memory_startup_` (2), `memory_boundary_` (1).
   - This is the first execution of `0001_init.sql` and of every PgStore statement, including the cancel CTE, both race tests, bootstrap and audit.
2. `scripts/verify.sh` (sqlx-cli 0.8.6: migrate as the owner role, `prepare --check`) and `scripts/extracted-build.sh`, after F1.
3. A green CI run of `memory-service.yml`: the verify job, showing that F3's isolation works with the service container, and the image job (non-root, `--network none --read-only`, explicit refusal without a database).
4. The new tests for F2 (oversized MCP body, MCP deadline) and the fixes for m1/m3/m5 if accepted.
5. `cargo fmt --check`, `clippy -D warnings` and `cargo check --locked --offline --all-targets` re-run after the fixes. The author's runs predate this review.

## Verdict

**Findings to resolve:** 3 major (F1, F2, F3) and 12 minor (m1–m12). R1–R3 are closed in code; their DB tests remain unexecuted. P2 cannot be accepted until the required checks above run green, because no SQL in this point has executed yet.
