# P7b code review r2 (recheck) — reviewer B generation 5

Nonce `run=e471aa3f role=B gen=5`, agent `a217255500990fff2`. Date 2026-11-08.

## Artifact identity

- `tmp/memory-service-run/frozen/p7b-code-r2.diff` sha256
  `4fb5b7eeaac119511ac7647c80288d0283ce72901269f58b194f171cdad7c698` (verified).
- `p7b-r2-incremental-from-r1.diff` sha256
  `879356cc689df57325dfd3e24e240b4a9d59ebd84ed039530265c6d801da71d5` (verified; 18 files).
- Live worktree tree recomputed with `freeze.sh`'s method: `7cfddf32950cb530aa128204c832740e2720e009`,
  equal to the frozen tree. Line numbers refer to it.

## Coverage

The whole r1→r2 incremental, product code and tests:
- docs/deploy.md, schema, `ui/memory.rs`, contract_fake;
- agent.rs, memory/{lifecycle,mod,namespaces,tenants}.rs, persistence/mod.rs, user_lifecycle.rs;
- application/memory/lifecycle.rs, setup.rs, both memory test files, access_tests.rs.

Also re-read: the adapter's `retry_after` (`memory_service/mod.rs:180-188`), the service's
`Retry-After` emission (`services/memory/src/adapters/http/mod.rs:127`) and the unchanged settings
page (`pages/company_memory.rs`).

## Prior findings

| Finding | Status | Evidence |
| --- | --- | --- |
| M1 permanent failed provisioning | Resolved | `tenant_not_ready` and `tenant_disabled` → `Step::Wait` (30 s postpone, no attempt; `application/memory/lifecycle.rs:315, 395-397`). `redrive_failed_on` (`tenants.rs:56-77`) is called by `enable_on` (`:311-314, 330-331`) and by `ensure_on` for a still-provisioning scope (`namespaces.rs:131-135`). It never touches `absent` rows. Test `failed_provisioning_is_redriven_not_permanent`. The UI gap is new finding r2-m3. |
| M2 deletions terminal after ~4.5 min | Resolved | `RetryBudget::Outage` (`lifecycle.rs:31-52`) counts `transient_retries`, never fails, and backs off to 5 min (`persistence/memory/lifecycle.rs:405-445`). Failed rows appear in the startup warning with or without the URL (`setup.rs:116-129, 130-135`). The docs give the query and the re-drive, and the deletion claim is corrected (`deploy.md:745-775`). Test `deletion_survives_an_outage` (12×503, then settles). Residual visibility gap: r2-m2. |
| M3 attempts carried across generations | Resolved | Every upsert (`want_tenant`, `retire_company_on`, `retire_namespaces_on`) and the `Superseded` re-pend (`lifecycle.rs:125-130`) reset both counters. My r1 probe sequence is now covered by `superseded_generation_starts_a_fresh_attempt_budget`: the reclaim charges only the crashed execution's attempt, so `attempts` is 1. |
| M4 deadlocks with company delete | Resolved | Agent delete: company `FOR KEY SHARE`, then agent `FOR UPDATE` (`agent.rs:411-436`). Account deletion: one id-ordered pass over owned companies (`FOR UPDATE`) and member companies (`FOR KEY SHARE`), before memberships (`user_lifecycle.rs:576-639`). Competing tests run both orders through the production lock functions. I re-ran my r1 two-session probes against the new statements, in all four interleavings: no error, both commit (`logs/b5-p7b-r2-*-{hook,company}.log`). r1 had produced `deadlock detected` for the old statements. |
| m1 competing claimants | Resolved | Two barrier-started claimants take 4 distinct rows. The expiry sweep is now `FOR UPDATE SKIP LOCKED` and skips a locked row in under 1 s. A completion behind `retire_company_on` ends `Lost`. |
| m2 mismatch on deletion paths | Resolved | `mismatch_refuses_tenant_and_namespace_deletion`: zero fake calls, attempts 0. The 06A conditions are recorded. |
| m3 shutdown during active work | Resolved | A cancel during a 30 s in-flight call ends `pending`/`interrupted` with the lease cleared; the loop joins in under 2 s. |
| m4 CSRF enumeration | Resolved | `memory::WRITE_ROUTES` is chained into the cross-origin test (passes). |
| m5 truncated listing / batch attempts | Resolved as specified | `>= 1000` gives `credential_list_truncated` (bounded); stray batches are `Wait(ZERO)`, with no attempt. Consequence: r2-m4. |
| Nits n1–n3, R-n1 | Resolved | The cap refresh no longer bumps `revision`. Retention: n2 was a carry and not code-changed; it is still open for 06A. A returning member gets a new namespace id (test). A poisoned fake mutex now fails `assert_clean`. |

## Root's cross-effect questions

1. **Can outage outcomes spin forever on a permanently revoked provisioner without durable
   visibility?** They retry forever by design. The behaviour is bounded: one row at a time,
   backing off to 5 min. Each retry logs a `warn` and leaves the row `pending` with
   `outcome_code = provisioner_unauthorized` and a growing `transient_retries`. Settings shows
   this for live companies. For deleted companies, though, it is invisible except in those
   recurring logs:
   - the startup warning with the URL set lists only `failed` rows (`setup.rs:117-119`);
   - the documented operator query selects only `state = 'failed'`.

   That is r2-m2.
2. **Does re-driving on any enable bypass a terminal state?**
   - Provisioner-mismatch rows are never `failed` (they are postponed as `pending`), and
     `enable_on` refuses a different configured id before any re-drive (`tenants.rs:300-305`).
     The worker's pre-step refusal still runs, so no bypass.
   - `tenant_unmanageable` and `tenant_conflict` on the deletion path are `absent` and are
     excluded. On a `present`/`disabled` tenant row, an owner enable re-drives one more attempt,
     bounded by a human: acceptable.
   - A bypass that is not human-bounded does exist: `ensure_on` re-drives definite namespace
     failures on every run (r2-m1).
3. **Does the new account-deletion lock pass fit every existing lock order?** Probes re-run
   (above). By analysis, `FOR KEY SHARE` conflicts only with `FOR UPDATE`, which only company
   delete and the owning account's deletion take, both company-first. It is compatible with the
   `FOR NO KEY UPDATE` of `authorize_company`, enable/disable and `remove_member`, and with the
   `FOR SHARE` of provisioning.
   - Remove-member, provisioning, enable and the workflow prelocks all take the user's row first.
     It conflicts with the deletion's `users … FOR UPDATE` there, before any company lock.
   - Two account deletions lock companies in id order.
   - New memberships cannot appear mid-deletion: the FK `KEY SHARE` on `users` waits.
   - No cycle found.
4. **Does `user_lifecycle` becoming `pub(crate)` widen a boundary?** Only slightly: see r2-n1.
   Its only new `pub(crate)` items are `AccountLocks` (private fields, so `finish_account_deletion_on`
   cannot run without `lock_account_on`'s value) and the two functions. They are used only by
   `memory/tests.rs`. `source_modules` passes (8/8). The agent pair
   (`lock_agent_delete_on`/`finish_agent_delete_on`) is `pub(crate)` in the already-public
   `persistence::agent`, and `finish_agent_delete_on` takes plain ids, so crate code could call it
   without the locks.

## New findings (all Minor)

**r2-m1. `ensure_on` re-drives definite namespace failures on every run, defeating their terminal
state.** `redrive_failed_on(db, company, Some(id))` (`namespaces.rs:131-135`) re-pends any failed
`present` row: `namespace_conflict` (our namespace id is tombstoned remotely, which can never
succeed), `credential_list_truncated`, `protocol_error`, `lease_expired`.
Once P7c calls `ensure` per run, a definitely refused scope cycles forever, paced by run frequency:
up to 8 bounded attempts per cycle against the service, and its outcome flips between `failed` and
`pending`.
Required: re-drive automatically only failures that a retry can fix (exhausted bounded retries),
or rate-limit it (for example only when `updated_at` is older than a cool-down). For
`namespace_conflict`, retire the row and provision a new namespace id rather than retrying the
dead one. Add a test.

**r2-m2. Indefinite outage retries are not surfaced as stuck work.**
- A revoked provisioner, or a service that stays down, leaves deletion rows `pending` forever.
- Both the configured-startup warning and the documented SQL list only `failed` rows.
- Nothing escalates past a `warn` per retry. Persistence and src AGENTS want stuck work to be
  distinguishable for alerts.

Required:
- include long-deferred rows (for example `transient_retries` above a threshold, or
  `outcome_code = 'provisioner_unauthorized'`) in the startup warning and in the docs query;
- log once at `error` when a row crosses that threshold.

**r2-m3. The documented owner re-drive does not exist in the UI.**
- `docs/deploy.md` says a failed provisioning "is re-driven by enabling memory again in company
  Settings", and `enable_on` re-drives on an already-enabled tenant (`tenants.rs:311-314`).
- The section renders only "Disable memory" while enabled (`pages/company_memory.rs:62-71`).
  The owner must disable and then enable, which bumps `disable_epoch`; in P7c that fails every
  pending intent.

Required: offer a re-drive (or the enable command) when the tenant or any namespace lifecycle is
`failed`, or correct the docs.

**r2-m4. Outage retries honour `Retry-After` without a floor, and the truncation is now a hard
ceiling.**
- `backoff` (`persistence/memory/lifecycle.rs:162-166`) takes `retry_after` verbatim, and
  `retry_after` accepts `0` (`memory_service/mod.rs:180-188`).
- The outage budget never ends, and `run` loops with zero delay after any pass. A 429 with
  `Retry-After: 0` from a proxy is therefore an unbounded hot loop. The service's own `1` gives a
  1 Hz loop per due row for the whole overload.
  - Floor the delay at the exponential backoff (`max`), or at least 1 s.
- Separately, revoked credentials are never removed by the service. A tenant that has accumulated
  1,000 credentials (namespace churn included) can no longer provision any namespace or finish a
  namespace deletion: `credential_list_truncated` is bounded, then fails.
  - This is correct as m5 asked, but it is a capacity ceiling. Record it in the docs and the 06A
    carry, together with the service change it needs (a label-filtered or paginated
    `credentials/list`).

## Nits

- r2-n1: Narrow the new lock/finish functions to `pub(in crate::adapters::persistence)`; their
  only callers are persistence tests.
- r2-n2: `Wait("revoking_strays", ZERO)` relies on the listing reflecting the revocations. A
  revoke answering 404 (treated as done) for a credential the listing still shows would loop with
  zero delay.

## Checks run

Root's private cluster, own scratch databases (`b5_p7b_r2_test`, `b5_p7b_r2_probe`), both
dropped. `SQLX_OFFLINE=true CARGO_INCREMENTAL=0`, `DATABASE_URL` unset. The tree is unmodified,
so the shared target was used; no mutated build.
- Selections listed first:
  - `memory_scope_` 28 selected, 28 passed;
  - `memory_integration_` 19 selected, 19 passed (`logs/b5-p7b-r2-memory_*.log`).
- Regressions (`logs/b5-p7b-r2-regressions.log`), all passed:

  | Filter | Passed |
  | --- | --- |
  | `worker_slots_respect_the_pool` | 1 |
  | `credential_rotation` | 14 |
  | `company_invite::` | 33 |
  | `agent::retire_tests` | 3 |
  | `account_deletion` | 2 |
  | `pages::tests::company` | 6 |
  | `the_memory_provider_schema_is_gone` | 1 |
  | `every_workspace_write_refuses` | 1 |
  | `google_vault_callback_user_deletion` | 1 |
  | `delivery::tests::tenant_tests` | 8 |
  | `integrations::tests::lifecycle_tests` | 5 |
  | `routes::company::` | 4 |
  | `ui_companies` | 3 |
  | `--test source_modules` | 8 |

- Two-session deadlock probes with the r2 production statements, in four interleavings: agent
  delete or account deletion, each first and second against company delete. No deadlock.
- Not re-run by me: clippy, fmt and `sqlx prepare --check` (author evidence), the browser check,
  the full suite, and the live tests.

## Verdict

**M1–M4, m1–m5, n1, n3 and R-n1: resolved, with evidence above.** n2 (retention) remains a 06A
carry.

New: four Minor findings (r2-m1 re-drive churn, r2-m2 stuck-work visibility, r2-m3 UI/docs
re-drive mismatch, r2-m4 `Retry-After` floor and the 1,000-credential ceiling) and two nits. No
Major remains. The Minor findings are each small and local. I recommend fixing r2-m1, r2-m3 and
the r2-m4 floor before acceptance, and recording r2-m2 and the r2-m4 ceiling explicitly if they
are deferred.
