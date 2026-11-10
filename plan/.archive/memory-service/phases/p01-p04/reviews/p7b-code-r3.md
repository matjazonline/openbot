# P7b code review r3 (limited recheck: r2-m1..m4, r2-n1/n2) — reviewer B generation 5

Nonce `run=e471aa3f role=B gen=5`, agent `a217255500990fff2`. Date 2026-11-08.

## Artifact identity

- `tmp/memory-service-run/frozen/p7b-code-r3.diff` sha256
  `b2b2699347382950f1a9392eb5ff071e71fc6d8d1c754824d4756da672d3abd9` (verified).
- `p7b-r3-incremental-from-r2.diff` sha256
  `99f49006a0cf3d00c92b33b959d97f6dc2e2f239c6e797eaf3c44df3182f5969` (verified).
- The live tree recomputes to `dc758b67474684926823a77b89793337d96b2c04`, equal to the frozen tree.

## Coverage

The whole r2→r3 incremental:
- deploy.md, `pages/company_memory.rs`, `ui/memory.rs` and its route test, `agent.rs`;
- `memory/{lifecycle,mod,namespaces,tenants}.rs` and both test files, `persistence/mod.rs`,
  `user_lifecycle.rs`;
- `application/memory/{lifecycle,settings}.rs`, `setup.rs`.

Also re-read: the adapter's 409 mapping (`memory_service/mod.rs:158`, carrying the service code)
and the service's generic `conflict()` (`services/memory/src/adapters/postgres/mod.rs:184-186`,
code `conflict`, used for a deleted tenant, versus `namespace_deleted` for our tombstone).

## Rechecked findings

| Finding | Status | Evidence |
| --- | --- | --- |
| r2-m1 lazy re-drive churn | Resolved, with r3-m1 below | `redrive_failed_on(mode)` (`tenants.rs:59-104`): `Automatic` skips `DEFINITE_FAILURES` and rows updated within the last 600 s; `Owner` re-drives every failed `present`/`disabled` row; `absent` rows are never touched. Test `lazy_redrive_skips_definite_failures_and_replaces_a_conflict`. |
| r2-m2 stuck visibility | Resolved | `STUCK_TRANSIENT_RETRIES = 12` (about 24 min of capped backoff): one `error` log at the crossing (`application/memory/lifecycle.rs:291-298`). `pending_lifecycles` always also lists pending or claimed rows at or past it (`memory/mod.rs:35-40`), so the configured-startup warning names them. The docs query includes them. Test asserts a stuck pending row is listed. |
| r2-m3 UI re-drive | Resolved | `action=retry` → `MemorySettings::retry`: refused unconfigured, then `retry_memory_provisioning` (`tenants.rs:437-458`). It takes `lock_authority` (owner/admin; anyone else a concealed NotFound and the denial page), the tenant `FOR UPDATE`, then checks the revision and refuses a disabled tenant. It re-drives in `Owner` mode and changes neither `revision` nor `disable_epoch`. The button shows only when the tenant is enabled, configured and has a failed non-`absent` row. Route test: member denied, stale refused, count shown, epoch unchanged. CSRF: same route, already in the enumeration. |
| r2-m4 Retry-After floor and 1,000 ceiling | Resolved | `backoff` is `max(2ⁿ, Retry-After)` capped at 5 min (`persistence/memory/lifecycle.rs:164-186`), with a unit test. The ceiling is in deploy.md and the 06A carry (decisions `:1852, :1871`). |
| r2-n1 visibility | Resolved | `pub(in crate::adapters::persistence)`. `AgentDeleteLocks` is a private-field token, so the finish step cannot run without the lock step. `user_lifecycle` is private again. `source_modules` passes. |
| r2-n2 404-only batch | Resolved | `revoke` answers whether it revoked. A full batch that revoked nothing is `Retry("stray_revocation_stuck")`, a bounded attempt (test). |

## Root's two questions

**Can retiring a `namespace_conflict` scope and re-provisioning orphan data or resurrect a deleted
owner's scope?**
- **Resurrection: no.** The retire and re-insert happen inside `ensure_on` only after
  `lock_owner` has the owner `FOR SHARE` (user, company, member or agent, which must exist in this
  company) and the tenant `FOR SHARE` (which must be enabled). A deleted owner returns
  `OwnerUnavailable` first. An owner deletion racing it serializes on the owner row, the same
  protocol the competing tests cover.
- **Orphaned data: no.** Only a provisioning (never-ready) row reaches this path. Its credential
  was never stored, and the remote namespace is a tombstone. The old row goes `removing` with an
  `absent` lifecycle (`retire_namespaces_on`); that runs create, then 409, then revokes labelled
  strays, then settles and deletes the row. Lock order: owner → tenant → namespace → lifecycle.
- **Two concurrent `ensure` calls:** the second's update finds the row already `removing`, so its
  lifecycle upsert is a no-op. Its insert waits on the unique live index and inserts nothing.
- **The remaining issue is the trigger, not the retirement:** r3-m1.

**Can the retry action be used by non-owners or bypass disable?**
- Non-owners: no. Route and store both require `lock_authority` (owner/admin), and members,
  strangers and foreign ids get the same denial. The route test covers a member; the stranger and
  cross-company cases share the same function, already tested for enable/disable.
- Disable: no. It refuses a `disabled` tenant under the tenant `FOR UPDATE` lock, so it
  serializes with disable. It never re-drives `absent` rows and changes no epoch.
- Provisioner mismatch: still refused by the worker before any remote call. A re-driven
  mismatched row just waits postponed.

## New finding

**r3-m1 (Minor). Any 409 on namespace create is treated as our tombstone, so a remotely deleted
tenant makes lazy provisioning mint a new namespace id at run cadence, without bound.**
- `namespace_present` maps `Ok(Deleted) | Err(Conflict { .. })` to `Fail("namespace_conflict")`
  (`application/memory/lifecycle.rs`, `namespace_present`), whatever the code. The service answers
  409 `conflict` for a deleted tenant and 409 `namespace_deleted` for our namespace tombstone.
- The loop when an operator deletes the tenant in the service while OpenBot still has it enabled
  and settled `present` (`tenant_ready` checks only local state):
  1. Every new namespace's create gets 409 `conflict` and fails `namespace_conflict` immediately.
  2. The next run's `ensure` retires that row and inserts a fresh id, with no cool-down.
  3. The worker fails the fresh id at once.
- Each cycle adds a namespace lifecycle row that is never deleted, plus about three service calls,
  for as long as runs continue.

Required:
- Retire-and-replace only on `namespace_deleted` (or `Ok(Deleted)`).
- Treat any other 409 as a definite failure with no automatic replacement. Preferably also mark
  the tenant lifecycle failed (for example `tenant_conflict`), so settings show it and the owner or
  operator acts.
- Optionally put the replacement under the same cool-down.
- Test the deleted-tenant case.

## Checks run

Private cluster, own scratch DB `b5_p7b_r3_test`, dropped afterwards.
`SQLX_OFFLINE=true CARGO_INCREMENTAL=0`, `DATABASE_URL` unset. Unmodified tree, shared target, no
mutated build.
- Selections listed first:
  - `memory_scope_` 29 selected, 29 passed;
  - `memory_integration_` 21 selected, 21 passed (`logs/b5-p7b-r3-memory_*.log`).
- Regressions (`logs/b5-p7b-r3-regressions.log`), all passed:

  | Filter | Passed |
  | --- | --- |
  | `every_workspace_write_refuses` | 1 |
  | `agent::retire_tests` | 3 |
  | `account_deletion` | 2 |
  | `company_invite::` | 33 |
  | `the_memory_provider_schema_is_gone` | 1 |
  | `--test source_modules` | 8 |

- Not re-run by me: clippy, fmt and `prepare --check` (author evidence), the browser check, the
  full suite, the live tests.

## Verdict

**r2-m1..m4 and r2-n1/n2: resolved, with evidence above.** One new Minor, r3-m1 (any 409 on
namespace create, including a remotely deleted tenant, triggers unbounded namespace replacement).
No Major remains. The fix is small: discriminate on `namespace_deleted`. I recommend fixing it
before P7b acceptance.
