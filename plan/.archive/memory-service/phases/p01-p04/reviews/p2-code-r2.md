# Review p2-code-r2 — P2 recheck (reviewer B gen 1)

## Artifact identity

- Reviewer: role B gen 1, nonce `run=e127dfad role=B gen=1`, agent `a636617799682ea85`.
- Frozen artifact: `tmp/memory-service-run/frozen/p2-code-r2.diff`, sha256 `92a87fc18f0b13d83370c6afea78d809df1bec8cf94f6c0ed9617326c02045e9` (recomputed and matches), base `a6bed48a`, tree `9e95a871…`.
  I recomputed the worktree tree at review time and it is identical.
- Compared against r1 tree `60bf672…`: 41 files changed outside `.sqlx`, plus the committed `.sqlx` metadata.
- Database: root's private cluster `postgres://postgres@127.0.0.1:55432/postgres` (PostgreSQL 18.6 + pgvector 0.8.7). I did not touch localhost:5432 or any `.env`.

## Independent evidence (my runs)

All runs used `CARGO_INCREMENTAL=0` and the shared `services/memory/target`.

- `scripts/verify.sh` (`MEMORY_TEST_DATABASE_URL` = private cluster, sqlx-cli 0.8.6 from `tmp/memory-service-run/tools/bin` on PATH, Homebrew psql 18) **passed all gates**: fmt, boundary check with the poisoned-parent probe, `cargo fetch --locked`, offline check, clippy `-D warnings`, migrate as the owner role, `cargo sqlx prepare --check -- --all-targets`, model manifest, test compilation, and the full suite under macOS network isolation: **59 passed, 0 failed**. Log: `logs/b-p2r2-verify.log`.
- `cargo test --lib memory_authority_` was repeated 3× against the same cluster: **22/22 passed each time**, so the two race tests are stable. Afterwards no `memtest_*`/`memverify_*` databases remained, so cleanup works. Log: `logs/b-p2r2-authority-repeat.log`.
- The frozen-tree blob of `assets/bundled/MODEL-LICENSE.txt` hashes to `904dc4d8…`, which equals the manifest.

## Recheck of r1 findings

| r1 | Status | Evidence |
| --- | --- | --- |
| F1 licence CRLF | Fixed | `services/memory/.gitattributes` (`-text whitespace=cr-at-eol`) travels with the extracted copy. The frozen blob digest matches the manifest, and verify's "Model manifest" step passed. |
| F2 MCP bounds | Fixed | `mcp_gate` (`http/mod.rs`) authenticates, reads the body under `MCP_BODY_LIMIT` (4 MiB + 64 KiB) and runs the exchange under the request deadline. It hands rmcp a fully buffered `Body::from(bytes)`, so rmcp's unbounded `collect()` now only ever sees at most the limit. `McpHandler::call_tool` re-applies the per-operation `spec.body_limit` to the argument object and wraps `execute` in `bounded(deadline, …)`, because rmcp runs the handler on its own task. Both paths return the `payload_too_large` / `timeout` envelopes, and the new tests pass. |
| F3 Linux isolation | Fixed in code; Linux path unproven | The conntrack `--ctorigdst 127.0.0.1 --ctorigdstport 5432` rule, INT/TERM traps and a `network-isolation.py` self-test were ported. The self-test checks that loopback works, that IPv4/IPv6 egress to documentation addresses is refused, and that the test PostgreSQL is reachable. The Darwin path passed in my verify run. Linux needs the first CI run. |
| m1 admin credential re-check | Fixed | `lock_actor` (`postgres/mod.rs:197`) takes `FOR SHARE` on the acting admin credential and re-checks revocation and expiry. It is called in all 10 admin transactions: change_tenant, issue_credential, admin-credential issue and revoke, create_tenant, set_cap, create/delete namespace, list and revoke credential; bootstrap correctly has none. D13 now starts with the acting admin credential row and records the revoke-each-other deadlock as retried. Two operators revoking each other deadlock, one transaction retries, and the retry sees its own credential revoked and refuses, which is the safe outcome. |
| m2 bootstrap | Fixed | Bootstrap counts only operators that are neither revoked nor expired. |
| m3 cancellation | Fixed | `cancel_operations`: `SELECT … ORDER BY id FOR NO KEY UPDATE`, then per-id updates with `reserved_tokens = 0`, with spend release last. That matches the D13 order. Under READ COMMITTED a row being claimed concurrently is re-read after the wait, so the decision uses its current state. |
| m4 migration | Fixed | The source trigger now freezes `session_id`, `redaction_version` and `expires_at`. The links FK `(tenant_id, source_id, source_level)` points to the new unique key `sources (tenant_id, id, access_level)`; MATCH SIMPLE leaves parent-observation links alone, as documented. `audit_events.request_id` was added. |
| m5 grant semantics | Fixed | `Operation::writes()`; `validate_grants` requires `write_max` for any write operation; `reads()` now includes Delete. |
| m6 schema bounds | Fixed | `domain/bounds.rs` feeds the schemars attributes, `openapi.json` was regenerated, and the freshness test passes. |
| m7 / m12 admin idempotency and leaks | Fixed or justified | Tenant and namespace create are idempotent by external id. A replay is answered only to an admin allowed to manage the live row: `may_manage`, and namespaces are scoped by the managed tenant. A tombstoned or foreign id gets one uniform 409, so there is no replay across admins or tenants. Credential issue is exempt, with a recorded rationale (the secret is shown once). |
| m8 extraction, layering, URL | Fixed | Extraction is tracked-only, with an explicit `MEMORY_EXTRACT_UNTRACKED=1` local override that CI never sets, and a refusal when nothing is tracked. The D1 layering rule is now in `boundary_check.py`. URL handling is fixed. |
| m9 SQL verification | Fixed | Production statements use `query!`/`query_as!`/`query_scalar!`, and `.sqlx` is committed. `prepare --check` against the migrated owner-role database passed in my run, so the SQL is compile-time checked, and the DB suite executes it. |
| m10 unused dependency | Fixed | `tower-http` was removed. |
| m11 test strength | Fixed | The 501 loop now covers every client operation over HTTP and MCP (examples exist for all of them). Success-path tests were added. The disable race still uses an ad hoc claimer; this is accepted for P2, and the production claim protocol is due in 04B. |

## New findings

None at minor or above. Two observations, no action needed for P2:

- In `network-isolation.py`, `ENETUNREACH`/`EHOSTUNREACH` count as "refused". On a host with no IPv6 route, the IPv6 probe therefore passes without proving the guard works. The IPv4 probe (a timeout would fail it) still does. Revisit if CI shows IPv6 egress.
- The idempotent `tenants.create` replay lets an operator receive the view of a provisioner-managed tenant. That is consistent with operator authority.

## Remaining required checks (CI-only, not reproducible here)

1. The first green run of `.github/workflows/memory-service.yml`:
   - **verify job**: extracted tracked-only copy, Linux network isolation with the conntrack rule and self-test reaching the service container, and the DB suite.
   - **image job**: non-root `memory:memory`, `--network none --read-only` `version`, and the explicit refusal without a database.
2. Record the CI run id and results in phase 02. Until then the Linux isolation path and the Docker image remain unverified, not passed.

## Verdict

**Pass** for the P2 code (01A, 01B, 02A), with evidence above: all r1 findings are fixed, and verify passed 59/59 including every DB-backed test plus three stable authority repeats. Acceptance depends on the CI-only checks listed above.
