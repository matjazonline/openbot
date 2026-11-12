# Recheck p7b-code-r1: review p7b1-code-r1 findings

Reviewer B, generation 4 (agent `a5916c4a63f198c6f`, nonce `run=8e7c1e85 role=B gen=4`).
Date 2026-11-08. Read-only: no project file was edited and nothing was committed.

**Verdict: PASS.** All of my p7b1-code-r1 findings (M1, M2, m1–m6, n1–n5) are resolved in the
frozen tree. There is one new nit and one hand-off note for the other reviewer.

## Artifact identity

| Artifact | sha256 (recomputed) | Extent |
| --- | --- | --- |
| `tmp/memory-service-run/frozen/p7b-code-r1.diff` | `37e1ac74f6c7a39c065935ee6dd61d3e4eea26fa05268efbc53b6e149e23ae12` | full diff |
| `tmp/memory-service-run/frozen/p7b-incremental-from-p7b1.diff` | `171746f7a3043466aec6ce46f70dcf3c23a95741c55e3249be5e0e2d6de36471` | 37 files, +4321/−148 |

Both hashes match. I reviewed tree `0df92996b0077291026172b7eeb8d696959a6ff9`, extracted with
`git archive`, not the moving working tree.

## Scope

Limited to my own findings and their cross-effects. The new P7b remainder belongs to the separate
reviewer: tenants, namespaces and lifecycle persistence, the reconciler, the owner hooks and the
settings pages. I read those files only where a finding touches them:

- `application/memory/lifecycle.rs`: the `retry` mapping and `namespace_absent`.
- The rotation shadow tables.

No cargo build or DB run was mine. The author's `tmp/memory-service-run/logs/a7-final-focused.log`
shows 119/119 passed. That run includes each test that proves one of my findings:

- `memory_integration_adapter_connection_dropped_after_the_write_is_ambiguous`
- `memory_integration_adapter_error_taxonomy`
- `memory_integration_contract_fake_matches_the_service_namespace_and_tenant_answers`
- `memory_integration_contract_fake_reports_violations_on_the_test_thread`
- `rotation_covers_memory_client_credentials_with_namespace_bound_envelopes`
- `the_memory_provider_schema_is_gone`

## Findings rechecked

| Finding | Status | Evidence |
| --- | --- | --- |
| M1 classifier | Resolved | See M1 below. |
| M2 fake | Resolved | See M2 below. |
| m1 lease CHECK | Resolved | `(state='claimed') = (lease_owner IS NOT NULL)` plus `(lease_owner IS NULL) = (lease_until IS NULL)`. The tenant CHECK is split, and the new `owner_kind <> 'company' OR owner_id = company_id` also closes n1 for namespace rows. |
| m2 rotation arm | Resolved | A DB test seals three envelopes plus one NULL envelope. It pages across the boundary in key order, rotates, reopens under the namespace context, refuses a swapped context, and shows the compare-and-swap rewrap refuses a stale value. A `memory_namespaces` temporary shadow table in `isolated_connection` keeps rotation tests off real rows. |
| m3 schema allowlist | Resolved | Exact table list (now also refusing `hydradb%` tables) and exact sorted `table.column` allowlist for every `memory%` table. The out-of-table column scan excludes `memory%` tables. The tautological loop is removed. |
| m4 documentation | Resolved | `.env.example` and `docs/deploy.md` "Memory service" name the six keys with their defaults and bounds, the stray-key refusal, and "startup never contacts the service". They also cover opt-in, disable and delete semantics and that the provisioner cannot be rotated yet. The doc tables match `memory_config.rs`. |
| m5 `spend_exhausted` | Resolved | 429 with that code → `Refused{spend_exhausted}`, with a test. |
| m6 company-scope rule | Resolved | Record E3 now states a rule, not tracked state: a company-scope save always sends `consumed: unknown`, and agent- and user-scope saves follow `content_exact`. |
| n1 | Resolved | Via m1. |
| n2 wire enums | Resolved | Adapter-private wire enums, mapped into the port. The port enums no longer derive serde. |
| n3 credential list | Resolved | Recorded in E2 for 06A retention. |
| n4 token trim | Resolved | The token is trimmed. |
| n5 long line | Resolved | The cited line was rewrapped. Other lines over 100 characters are mostly pre-existing context. |

**M1, classifier** (`memory_service/mod.rs`):

- `transport()`: connect → `Unavailable`, builder → `Protocol`, anything else → `Ambiguous`.
- `classify()`:
  - 503 is `Unavailable` only with the `unavailable` envelope.
  - 400 and 413 → `Protocol`.
  - 500, 502, a bare 503, 504 and anything else out of taxonomy → `Ambiguous`. A test covers 500,
    502, 503, 504 and 418 with an HTML body.
- `drop_after_write` applies the write and then drops the connection. The client sees `Ambiguous`,
  and the write is visible in fake state.
- Cross-effect: the reconciler's `retry()` maps `Ambiguous` and `Protocol` to bounded retries.
  Nothing terminal hangs on the new taxonomy.

**M2, contract fake:**

- Violations are recorded and answered with a 500 `contract_violation`. `Drop` calls
  `assert_clean()` on the test thread unless the test is already panicking, and a test proves a
  violation fails.
- Fidelity now matches the service code I checked in round 1:
  - deleting a never-created namespace → 404, with no tombstone;
  - issuing a credential naming a namespace that is not active → 400;
  - `require_active`: disabled 403, deleted 409, for namespace create and credential issue;
  - tenant delete leaves credentials unrevoked.
- E2's absent-namespace path is now create-then-delete. `namespace_absent` implements it: a 409 or
  404 on create, or a 200 on delete, completes; a delete 404 is accepted only after a
  `tenant_disabled` create.

## New nit

**R-n1.** In `ContractFake::assert_clean`, `lock().map(..).unwrap_or_default()` treats a poisoned
mutex as clean. Today nothing panics while holding the lock (the drop path releases it before
`resume_unwind`), but a future handler `unwrap` could hide violations. Treat a poisoned lock as a
failure.

## Note for the other reviewer

`namespace_absent` treats a 404 on namespace create as done, meaning "no tenant of ours". That is
safe only because the worker refuses every lifecycle whose `provisioner_credential_id` differs from
the configured provisioner. Under a rotated provisioner, a 404 would otherwise falsely complete
deletion, exactly as round-1 M1 did for tenants. Please confirm that the refusal runs before any
remote step and is tested. It is in the reconciler code, outside my scope.
