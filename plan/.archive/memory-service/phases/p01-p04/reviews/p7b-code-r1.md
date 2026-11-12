# P7b code review r1 (remainder scope) — reviewer B generation 5

Nonce `run=e471aa3f role=B gen=5`, agent `a217255500990fff2`. Date 2026-11-08.

## Artifact identity

- Frozen full diff `tmp/memory-service-run/frozen/p7b-code-r1.diff`, sha256
  `37e1ac74f6c7a39c065935ee6dd61d3e4eea26fa05268efbc53b6e149e23ae12` (verified).
- Incremental vs gated record tree 6238f2bc: `p7b-incremental-from-p7a.diff`, sha256
  `fbe4f511d5d5437eccbd81bb341a9dfade0eea23e80b30e4514ac1ef1870cb59` (verified).
- Live worktree recomputed with `freeze.sh`'s method (base a6bed48a, tmp/ excluded): tree
  `0df92996b0077291026172b7eeb8d696959a6ff9` = the frozen tree. Line numbers below refer to it.

## Coverage

Read in full: plan README, 01 (access-level contract, 01B.6), 05 (all, incl. 05A.1–2, 05B.7 and the
verification record), 06B.2; AGENTS.md, src/, application/, infra/, persistence/, http/, pages/
AGENTS; decisions E1, E2, E9–E12 and both P7b implementation-note sections.
Code: `application/memory/{ports,settings,lifecycle}.rs`, `persistence/memory/{mod,tenants,
namespaces,lifecycle}.rs` and both test files, owner hooks in `company.rs`, `agent.rs`,
`company_invite.rs`, `user_lifecycle.rs`, `infra/workflow.rs`, `workflow/services.rs`,
`setup.rs`, `setup/mail.rs`, `main.rs`, the schema hunk, `pages/company_memory.rs`,
`routes/workflows/ui/memory.rs`, `company_settings.rs` hunks, `memory_tests.rs`, the docs/.env
hunks, the adapter's `admin()` path, and the service's tenant/namespace/credential admin code
(`services/memory/src/adapters/postgres/mod.rs:302-348, 880-1220`). Out of scope (other reviewer):
the substep-1 findings (classifier, fake fidelity, lease CHECK, rotation test, legacy allowlist,
key docs, 429 mapping).

## Findings

### Major

**M1. A failed namespace provisioning lifecycle is permanent; nothing re-drives it, so the scope
is dead for good.**
`persistence/memory/namespaces.rs:127-131` answers `NotReady` for any existing non-`removing`
row and never looks at its lifecycle; `tenants.rs:289` returns early when the tenant is already
enabled, and `enable_on` re-pends only the tenant lifecycle (`want_tenant`). Nothing else touches
a namespace lifecycle except the owner hooks. The live-scope unique index
(`memory_namespaces_live_scope`) stops a replacement row. A namespace `present` row goes
terminal through ordinary paths:
- the declared deviation: `application/memory/lifecycle.rs:272-277` retries `tenant_not_ready`
  and consumes attempts while the tenant is not settled `present`;
- disabling while a namespace is provisioning (`tenant_ready` needs `state = 'enabled'`);
- a service outage of about 4.5 minutes right after enable (see M2);
- `namespace_conflict`.

After that, `ensure_namespace` answers `NotReady` for that agent, user or company scope forever.
Disable and re-enable do not help either. Only deleting the owner clears it, and the company scope
has no owner to delete short of deleting the company.
Required: lazy provisioning (or enable) must re-pend a `failed` `present` namespace lifecycle,
with fresh attempts and a new generation, under the same locks. A namespace must not burn its
attempt budget while its tenant is not ready or is disabled: postpone without an attempt, as
`provisioner_mismatch` does. Test: disable with a namespace provisioning, exhaust it, re-enable,
then `ensure` must lead to `Ready`.

**M2. Every lifecycle, including deletions, goes terminal after about 4.5 minutes of service
trouble, with no re-drive and no durable operator surface once the owner is gone.**
Backoff is 2ⁿ s and attempt 8 fails (`persistence/memory/lifecycle.rs:161-165, 404-435`;
`MAX_LIFECYCLE_ATTEMPTS`). That totals 2+4+…+128 = 254 s of waiting, and 429 `Retry-After` is
capped at 300 s. A company, agent, member or account deletion that lands during a short outage,
or while the provisioner is revoked (`provisioner_unauthorized` also counts), ends `failed`. The
remote data is then never deleted.
- Visibility: for a deleted company there is no settings page. `pending_lifecycles`
  (`memory/mod.rs:32`) excludes `failed` rows from the startup warning, so only one `error` log
  line records it.
- Company delete has already marked every namespace lifecycle `settled` (`tenants.rs:133-143`),
  on the assumption that the tenant delete covers them.
- `docs/deploy.md:745` promises that deleting a company, agent, member or account deletes the
  remote tenant or namespace.

05B.7 requires "reliable lifecycle requests", and 01B.6 requires deletion to hold. Persistence
AGENTS wants a terminal state for poison, but a transient outage is not poison.
Required:
- Give `absent` lifecycles a retry budget sized for outages (or no attempt cap for
  transport/unavailable/capacity outcomes, with a capped backoff), keeping terminal failure for
  definite refusals (`tenant_unmanageable`, a protocol error).
- Surface failed and stuck lifecycles durably to operators: the startup warning and a
  documented query or metric.
- Document the manual re-drive.
- Correct the docs claim.

**M3. A superseding desired state inherits the old generation's exhausted attempts. Under lease
expiry the deletion fails without a single remote attempt (reproduced).**
The upserts keep `attempts` while the row is `claimed` (`tenants.rs:79-81, 122-124`,
`namespaces.rs:210-212`). `fence_on`'s `Superseded` re-pend leaves `attempts` unchanged
(`lifecycle.rs:125-135`), and the claim's expiry sweep (`lifecycle.rs:229-238`) fails any claimed
row with `attempts + 1 >= 8`, whatever its generation.

Probe (`logs/b5-p7b-probe-attempts.log`, the statements verbatim on a freshly migrated database):
1. A provisioning row claimed at attempt 7, whose worker then dies.
2. The owner is deleted: `retire_namespaces_on`'s upsert gives `absent` at generation 2, still
   `claimed`, with attempts 7.
3. The lease expires and the next claim marks it `failed lease_expired`, still desired `absent`.

The deletion is never attempted. On the non-crash path, the `Superseded` re-pend leaves one
attempt for the deletion.
Required: a generation change starts a fresh attempt budget. At most the in-flight execution's
own attempt may be charged, and the expiry sweep must not terminally fail a generation that never
ran. Test both paths: crash, and superseded-then-retry.

**M4. Two new owner-hook deadlocks against company delete (both reproduced). The stated lock
order does not hold for agent delete or for account deletion.**
`retire_company_on` locks the tenant and then every namespace `FOR UPDATE` (`tenants.rs:104-113`)
before `DELETE FROM companies` cascades to agents and members. Two hooks hold those rows first and
then update a namespace, without the company lock that E2's order (users → company → … → agent)
puts first:
- Agent delete: `agents … FOR UPDATE` → `retire_namespaces_on` (`agent.rs:404-424`).
- Account deletion: memberships in companies the user does not own, `FOR UPDATE`, with no lock on
  those companies → `retire_namespaces_on` (`user_lifecycle.rs:25-41`).

Probes ran the production statements in order in two sessions on a migrated database
(`logs/b5-p7b-deadlock-{agent,agent-company,account,company}.log`). Both give `ERROR: deadlock
detected` in the agent-delete or account-deletion transaction. The same account-deletion
interleaving without the memory statements (`b5-p7b-deadlock-base-*.log`) commits cleanly, so the
memory hooks introduce this cycle. Member removal is safe: it takes `companies … FOR NO KEY
UPDATE` first (`company_invite.rs:559-562`). Lazy provisioning is safe: company `FOR SHARE` first.
Effect: one deletion aborts with a 500 (it can be retried). E2's claim that account deletion runs
"after the existing users → company prelocks" is false, and no competing test covers either pair.
Required:
- Agent delete takes its company (`FOR KEY SHARE` is enough to order against company delete's
  `FOR UPDATE` without blocking `FOR NO KEY UPDATE` users) before the agent row.
- Account deletion locks all of its companies in one ordered pass (owned `FOR UPDATE`, member
  companies `FOR KEY SHARE`) before memberships.
- Add competing tests for agent delete vs company delete and account deletion vs company delete,
  in both orders.

### Minor

**m1. No competing-claimant test for the new lifecycle queue (root AGENTS.md).** All concurrency
tests race the owner hooks against each other. None runs two `claim_lifecycle` callers at once, or
a fenced completion against `retire_*` or `want_tenant` in real concurrent transactions:
`fences_stolen_and_superseded_claims` is sequential. Add two concurrent workers on several due
rows (distinct claims, the expiry sweep under `lock_timeout 2s`). Also add a completion blocked
behind `retire_company_on`'s namespace lock that ends `Lost`.

**m2. The provisioner-mismatch refusal (root's added item).** Structurally confirmed: every remote
call is in `MemoryLifecycleWorker` (`application/memory/lifecycle.rs:290-461`), and `pass` refuses
a mismatched `claim.provisioner_credential_id` (`:215-226`) before `step`. Every (resource,
desired) path goes through `step`, and nothing else holds the provisioner (settings use only the
id). The recorded ids are consistent: `want_tenant`, `retire_*` and `ensure_on` all copy
`memory_tenants.provisioner_credential_id`, and enable refuses a different configured id.
The test covers only tenant `present` (`lifecycle_tests.rs:286-308`, asserting no fake calls).
Required:
- Add mismatch cases for namespace `absent`, the path where a foreign 404 would falsely settle,
  and for tenant `absent`, each asserting zero calls and an unchanged attempt count.
- Record as a 06A condition: when tenant `managed_by` reassignment lands, the recorded id and the
  remote manager can diverge, and then the 404/409 settles in `namespace_absent` (`:388-404`)
  would falsely complete deletions. The 404 accepted after a `tenant_disabled` create also lacks
  the quiescence deadline that persistence AGENTS requires for remote cleanup.

**m3. Shutdown during active work is untested (infra AGENTS).**
`memory_integration_reconciler_stops_on_shutdown` (`lifecycle_tests.rs:580-607`) cancels only
after the row settled. Add a cancel during a blocked step (the fake delaying a call), asserting
`interrupted`, one attempt, lease cleared and a prompt join.

**m4. The memory write route is not in the CSRF enumeration.**
`every_workspace_write_refuses_a_cross_origin_cookie_post` (`authoring/access_tests.rs:273-318`)
chains `typesafe::WRITE_ROUTES`, but `/ui/companies/{company_id}/memory` (`ui/memory.rs:9`) is not
listed. The central `browser_security` middleware is presumably applied; follow the convention by
exporting the route and adding it.

**m5. Stray revocation is silently incomplete for large tenants.**
- The service lists at most 1,000 credentials per tenant, ordered by id
  (`services/memory/src/adapters/postgres/mod.rs:1145`). Revoked credentials are never removed,
  and there is one live credential per namespace. A tenant past 1,000 credentials hides newer
  strays from `revoke_labelled`, both for present strays and for `absent` revocation.
- Each 16-revocation batch is a `Retry` with `Duration::ZERO` that consumes an attempt
  (`application/memory/lifecycle.rs:443-450`).

Required: page or bound explicitly, so the reconciler can tell the list was truncated and treat
that as not done, and make batch progress free of attempts. This tightens the n3 carry (the cap
that applies is 1,000 rows, not 4 MiB).

### Nits

- n1: A cap-cache settle bumps `revision` within seconds of enable (`lifecycle.rs:328-338`), so a
  form rendered by the enable response is almost always stale. The first "Disable" click gets
  "reload and retry". This follows the E2 design; consider re-rendering, or not counting the cap
  refresh against the form revision.
- n2: Settled `absent` lifecycle rows are retained forever. The claim's expiry sweep
  (`lifecycle.rs:229-238`) and the OR'd candidate predicate are not served by the partial `pending`
  index. Carry the retention to 06A with the threshold metric persistence AGENTS asks for.
- n3: There is no test that a re-added member or agent gets a new namespace id (only
  `OwnerUnavailable` after removal is asserted).

## Deviations (as declared)

1. Disable allowed while unconfigured: **acceptable**. It only stops local work and records a
   remote intent under the recorded provisioner.
2. Company delete settles namespace lifecycles as covered by the tenant delete: **acceptable only
   with M2 fixed**. If the tenant delete goes terminal, those namespaces are marked done without
   anything having deleted them, and `observed` is left stale.
3. A namespace whose tenant is not ready retries and consumes attempts: **not acceptable**. It
   hides M1.
4. Absent-namespace NotFound/Conflict on create settles: **acceptable today**. It is protected by
   the mismatch refusal (m2) and by no `managed_by` reassignment existing. It is a 06A condition.
5. Failing pending intents on disable, and cancelling them in owner hooks, deferred to P7c:
   **acceptable**, because `memory_deliveries` does not exist. It must be in P7c's gate, with the
   E9 test `memory_integration_disable_races_save_and_capture_then_reenable`.
6. Memory step kinds stay refused: **acceptable**.

## Other review questions (no finding)

- **Lazy provisioning outside the run transaction.** It holds by construction: `ensure_namespace`
  opens its own pool transaction (`namespaces.rs:171-180`) and has no caller yet. P7c obligation:
  never call it while holding a run transaction. If that transaction holds the company
  `FOR NO KEY UPDATE`, the call self-blocks until the 5 s `lock_timeout`. It also takes a third
  connection per slot, which `RESERVED_DATABASE_CONNECTIONS` does not cover.
- **Lock order (other pairs).**
  - Enable/disable vs company delete, account deletion or member removal: both sides go
    users → company, so they serialize.
  - Provisioning vs agent/member/company delete: company `FOR SHARE` first.
  - Fenced completion: resource → lifecycle; company delete: tenant → namespaces → lifecycles;
    the claim locks only the lifecycle. No other cycle found.
- **Resurrection.**
  - `want_tenant` never overwrites `absent` (tested).
  - The claim turns `present` into `absent` for a vanished owner (tested for an agent).
  - Owner hooks move the fence and generation, so a delayed `store_credential` is `Superseded`
    (tested).
  - A returning owner gets a new row because the unique index excludes `removing`.
- **Reconciler.**
  - The 60 s lease is above the 45 s local deadline, and completions are fenced (store and
    settle also check the generation).
  - The credential is sealed under `memory_client_credential(company, namespace)` and stored
    only under the fence and generation, while `provisioning`.
  - Create-then-delete with only a 200 settling, and a delete 404 being terminal
    `tenant_unmanageable`, are both tested.
  - Stray revocation works within m5's limits.
  - An expired lease consumes an attempt, and a poison batch leaves the due set (tested).
  - Real-service semantics checked: create of a disabled tenant returns its view; namespace
    delete, list and revoke work on a disabled tenant; a foreign or unmanaged tenant gives 404.
- **Settings authorization.**
  - Every GET and POST passes `lock_authority`: the users row, then `authorize_company` (owner or
    admin; others get a concealed `NotFound`), and the route renders one denial (tested for a
    member, a stranger and a cross-company id).
  - The revision conflict is tested; the body limit is 4 KiB; values are escaped.
  - Progress feedback uses `hx-disabled-elt` and a busy label.
  - CSRF relies on the central middleware (m4).
  - U1 as far as implemented: disable bumps `disable_epoch`, refuses new provisioning
    (`NotEnabled`) and records desired `disabled`; enable revives no epoch.
- **`RESERVED_DATABASE_CONNECTIONS` 12 → 13.** This is not a bound raised to clear a failure: it
  tightens the startup refusal. The early-failing check still exists:
  - `worker_slots` refuses at startup;
  - `worker_slots_respect_the_pool` was updated, ran here (1/1), and runs in CI through
    `cargo test --all-targets`;
  - deploy.md and workflow-operations.md now say 13.

  Pre-existing caveat: nothing ties the constant to the number of spawned DB users.
- **Supervision.** `with_memory` is composed only with the URL set, the handle is pushed to
  `spawn`'s list and joined by `main`, and cancellation reaches the step (`select!`, then
  `interrupted`).
- **Docs and keys.** Only the six keys read by `memory_config.rs` are named. The deletion claim
  in deploy.md:745 is the M2 inaccuracy.
- **Competing tests.** They use real transactions and `pg_blocking_pids` waits. Each test's second
  order emulates the deleting side with its statements, except member removal, which runs the
  real removal behind a held company lock.
  - A mutation removing `lock_owner`'s agent `FOR SHARE` makes agent delete not block, so
    `blocked_by` times out: caught by reasoning.
  - Removing the fence predicate breaks `fences_stolen…`: caught by reasoning.
  - A built mutation probe was not needed: the SQL probes above demonstrate M3 and M4 directly.

## Assumptions challenged

- "One lock order for every memory path": false for agent delete and account deletion (M4).
- "Retry consumes attempts" is harmless for a namespace waiting on its tenant: false, see M1.
- "Settled as covered by the tenant delete": holds only when the tenant delete itself is
  reliable (M2).
- "The upsert supersedes an in-flight claim": it does, but it carries the old attempt budget
  across (M3).

## Checks run (private cluster 127.0.0.1:55432, own scratch databases, both dropped afterwards)

- Hashes and live tree recomputed (above).
- `SQLX_OFFLINE=true CARGO_INCREMENTAL=0 TEST_DATABASE_URL=…/b5_p7b_test`, `DATABASE_URL` unset,
  shared root target (unmodified tree; nothing was rebuilt). Listed first:
  - `memory_scope_` 19 selected, 19 passed;
  - `memory_integration_` 18 selected, 18 passed (`logs/b5-p7b-{list,scope,integration}.log`).
- Regressions (`logs/b5-p7b-regressions.log`), all passed:

  | Filter | Passed |
  | --- | --- |
  | `worker_slots_respect_the_pool` | 1/1 |
  | `credential_rotation` | 14/14 |
  | `company_invite::` | 33/33 |
  | `agent::retire_tests` | 3/3 |
  | `account_deletion` | 1/1 |
  | `pages::tests::company` | 6/6 |
  | `the_memory_provider_schema_is_gone` | 1/1 |
  | `every_workspace_write_refuses` | 1/1 |

- SQL probes on a freshly migrated `b5_p7b_probe`:
  - attempt carry-over (M3);
  - agent-delete and account-deletion deadlocks, plus a no-memory baseline (M4).
- No project file edited; no mutated build.

## Remaining checks (not run)

- Browser check of the settings section in both themes (pages AGENTS).
- Full suite and stack budget (05/06C gate).
- Live-service `memory_integration_live_` tests (not written yet).
- P7c: the caller of `ensure_namespace` stays outside the run transaction, with connection
  accounting; intent failing on disable; owner hooks cancelling intents; contention between the
  delivery pool and lifecycle permits (a lifecycle gets `Unavailable` for no permit and consumes
  an attempt).

## Verdict

**Not accepted.** Four Major findings:
- M1: failed provisioning is permanent.
- M2: deletions go terminal after about 4.5 minutes, invisibly.
- M3: attempt carry-over can fail a deletion without any attempt.
- M4: two reproduced deadlocks with company delete.

Plus minors m1–m5. A focused recheck is needed after the fixes, with the new competing and
mismatch tests.
