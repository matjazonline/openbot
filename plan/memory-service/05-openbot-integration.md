# 05 OpenBot integration

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [04](04-observation-generation.md). OpenBot uses the service through a narrow adapter.
All OpenBot-specific scope, workflow, capability and recipient behavior stays here; phases 01–04
must remain independently buildable and usable. Operations and final checks follow in
[06](06-operations-and-verification.md).

## Reuse existing audience and consumed levels

Reuse [AccessLevel and RunKnowledge](../../src/domain/entities/access_level.rs),
[audience and run admission](../../src/adapters/persistence/workflow/knowledge.rs), and the
existing consumption/release checks. Do not introduce a parallel OpenBot clearance system.
The service implements only phase 01's generic numeric-level contract and has no dependency on
these modules or OpenBot's participant directory.

For message-driven calls, use the existing lowest-human-reader rule for the actual authorized
conversation audience. Current admission resolves thread participants; do not assume checking
only the sender covers all channel readers or intended destinations. Manual/scheduled calls use
their existing actor/run-as authority and child calls inherit their parent's context restrictions.
Agent/system principals do not raise human clearance. Preserve existing public handling for
external readers and fail closed on malformed authority.

The effective memory read ceiling is the minimum of the admitted run ceiling, current authorized
audience/actor clearance and service credential/namespace read grants. Participants at 6, 8 and 9
therefore permit reads at 0–6 when the other bounds allow 6. A later audience increase cannot raise
the admitted ceiling. Recheck audience reductions before further model entry, context reuse and
disclosure; an already consumed level-6 context cannot continue for a level-3 audience.

For a memory produced by an agent, use the run's trusted monotone `content_level` as the consumed
watermark, not `knowledge_ceiling` or the highest level in only the latest search response. Audit
every model-visible input path: classified messages, instructions/context, frozen history,
knowledge/memory snippets and details, tool results, and inherited/approval continuations must be
accounted for before exposure. If precise consumption accounting is incomplete, use the trusted
authorized ceiling of the producing context as the fallback: ceiling 6 saves at 6. Preserve that
ceiling with the source context; a later audience reduction cannot replace it with a lower one.
Fix untracked input paths, and refuse if no trusted bound exists or known content exceeds it.
Never assume that missing evidence means level 0.

Save at that consumed level, bounded below by contributing source levels. A ceiling-6 run that
has consumed only level-4 information saves at 4. If its history or any other consumed input was
level 6, a new note saves at 6 even when the latest recalled notes were public. The model cannot
select a lower level or discard earlier exposure. Do not reset the watermark mid-run to obtain a
lower label. When the exact consumed level is unavailable, the ceiling fallback takes the same
role. Namespace grants, write authorization and existing egress checks still apply.

## Batch 05A Connect current OpenBot capabilities to the service

1. Trace current workflow preparation, action authorization, checkpoint consumption and company
   lifecycle owners before editing. Define the memory port in the consuming application layer;
   implement authenticated bounded HTTP in an adapter. Do not import the service package, share
   a SQL pool, restore legacy memory-provider registries, or implement memory storage in OpenBot.
2. Map a company to the same opaque tenant ID without making the service understand companies.
   Resolve explicitly granted company/agent/user scopes to service namespaces. Missing user or
   agent identity omits/refuses that scope and never widens it. Keep source/channel/run identifiers
   as provenance, not automatic access grants. Scope provisioning and credentials to this client.
3. Implement the existing `memory.load` and `memory.save` contracts coherently through the normal
   workflow runtime and authoring paths. Reconcile their schemas with bounded provenance-aware
   results. Remove their unavailable status only after authorization, runtime handling, samples and
   tests exist. Stable logical writes must retain the same service idempotency key across retries.
   Keep authority fields out of model-selectable schemas; derive the read ceiling and consumed
   watermark in the host. Add level-aware results and durable save evidence where those contracts
   need it rather than exposing a caller-controlled classification override.
4. Expose agent capabilities for search, timeline, selected details and intentional save using the
   established tool-publication/dispatch model. Select scopes and ceilings from trusted execution
   state; an agent cannot choose arbitrary tenant IDs, impersonate a user or lower classification.
   The standalone service MCP surface remains available for other clients; it is not a shortcut
   around OpenBot's native consumption and egress checks.
5. Apply existing live capability, membership, audience and ownership checks before requests and
   before consuming results. Atomically record the observation/checkpoint result, raise the run's
   content level, and record live consumption before any returned text reaches the model. Reuse
   the pattern in [knowledge consumption](../../src/adapters/persistence/workflow/knowledge_checkpoint.rs)
   without making memory depend on the knowledge-library storage or embedding model.
   Freeze a save's content, sources and consumed watermark under the same live ownership/run
   transaction as its delivery intent. Concurrent recall and save must serialize classification
   accounting so no result exposed before the save can escape its label. Keep the frozen label
   on retry; an earlier completed save does not authorize a new lower-labelled memory.
6. Extend samples with isolated fixtures/intercepted writes; samples cannot contact the service
   or a paid provider. Reconcile old tests asserting the legacy provider schema is absent without
   restoring that schema. Only service connection/configuration and delivery intents belong in
   OpenBot's database; the memory corpus stays in the service database.

Focused checks: `memory_integration_` and `memory_scope_` exercise missing identities, ungranted
tools, tampered arguments, cross-company IDs, revoked credentials, lost workflow ownership,
classification rise, recipient restrictions, sample isolation and duplicate saves. Add
`memory_access_level_` cases for participants 6/8/9, read levels 0–6, consumed-4 saves at 4,
consumed-6 saves at 6 despite public latest recall, missing precise evidence saving at 6, forged
labels, a missing trusted ceiling and an audience drop to 3. Run real competing recall/save
transactions and verify both ordering outcomes preserve the exact or fallback watermark. Run
affected workflow publication, tool dispatch, knowledge-level and sample-boundary regressions.

## Batch 05B Add automatic context and authorized capture

1. Add bounded automatic recall at logical agent preparation, before provider entry. Combine a
   small memory pack with existing history/current input under the total context budget; do not
   replace history or reload memory on every tool round. Preserve the caller's mandatory current
   input and reserved output capacity.
2. Put retrieved records in the existing delimited untrusted context with IDs, provenance,
   timestamps and classifications. Put only usage policy in system instructions. Keep pure prompt
   construction free of network calls. Treat memory as evidence that can be stale or corrected,
   not as instructions or an authorization policy.
3. Freeze the selected pack and its content integrity in the execution's durable preparation.
   Retries replay it rather than choosing a new pack under an existing checkpoint. Revalidate
   live audience, authority and source validity before resume; revocation/deletion produces the
   existing controlled refusal, not a different prompt or a lower content level.
   Replay the pack's recorded classification too; selecting only lower-level memories later
   cannot erase higher-level information already present in the producing context.
4. Define optional recall failure separately from no matches: an eligible new run may continue
   with a recorded unavailable state if policy permits. Authorization failures fail closed.
   Intentional saves and accepted generation requests must not be reported successful when lost.
5. Capture only authorized completed activity windows and relevant session outcomes, with
   privacy filtering and the source classification intact. Submitting capture data alone cannot
   enable inference; explicitly configured capture policy supplies the generation allowance and
   expiry. Failed/cancelled runs, samples, credentials and unrelated private activity are excluded.
6. Where a committed OpenBot outcome promises saved-memory delivery, commit a bounded delivery
   intent with that outcome and use supervised, fenced, idempotent delivery. Delivering a finished
   memory may retry safely; never turn its retry into a new generation request. Generation-intent
   delivery keeps its original logical identity/expiry and cannot replenish its spend allowance.
   Both forms retain the consumed watermark captured at their authorized source boundary. A
   changed audience or retry must not recompute the saved label from only the latest inputs.
7. Order company suspend/delete and connection revocation before further delivery or generation.
   Send reliable lifecycle requests to the service, retain deletion barriers, and prevent delayed
   provisioning/capture from recreating removed namespaces.

Focused checks: `memory_preparation_` and `memory_capture_` inspect actual model context and prove
bounded packing, no instruction promotion, exact retry snapshots, classification-aware resume,
optional unavailability, no duplicate capture, no secret leakage, and durable promised delivery.
Use concurrent delivery claimants and deletion/delivery races against real PostgreSQL. Run affected
agent preparation, history, checkpoint, budget, cancellation and lifecycle regressions.

## Verification record

P7b first substep, 2026-11-08, base `a6bed48a`. Environment: rustc via the root toolchain,
`SQLX_OFFLINE=true`, `CARGO_INCREMENTAL=0`, `DATABASE_URL` unset,
`TEST_DATABASE_URL=postgres://postgres@127.0.0.1:55432/open_bots_p7b_test` (root's private
PostgreSQL 18.6 cluster, a disposable database). Logs: `tmp/memory-service-run/logs/a6-p7b-*.log`.

- `cargo check --locked --offline --lib --tests`: passed, 0 warnings.
- `cargo test --lib -- --list`: 3,562 tests; selections confirmed nonzero before running.
- `memory_integration_`: 9 selected, 9 passed. `memory_scope_`: 1 selected, 1 passed.
- `the_memory_provider_schema_is_gone` (DB): 1 selected, 1 passed. `credential_rotation` (DB): 13
  selected, 13 passed.
- `cargo fmt --all -- --check` and `git diff --check` passed; the no-index whitespace check on the
  new files found nothing.
- `scripts/workflow-sample-boundary.py`: passed. `scripts/transport-boundary-check.sh`: fails on one
  rule in `src/adapters/persistence/workflow/channel_reply_continuation_views_support.rs:59`, a file
  this work did not touch (unchanged since `4ab45555`). The failure predates this work; its
  memory-related finding was fixed.
- Not run: clippy and the full suite (focused checks only, per the assignment); the live-service
  integration tests (not written yet).

P7b second substep (review `p7b1-code-r1` fixes, opt-in, owner locks, reconciler, settings),
2026-11-08, base `a6bed48a`. Same environment, on a recreated
`postgres://postgres@127.0.0.1:55432/open_bots_p7b_test` (the init schema changed); memory tests
take `own_database` on that cluster. Logs: `tmp/memory-service-run/logs/a7-*.log`.

- `cargo check --locked --offline --lib --tests`: passed, 0 warnings.
- `cargo clippy --locked --offline --all-targets -- -D warnings` (root crate, once at the end of
  P7b): passed (`a7-clippy-2.log`; the first run's one finding, a bool `assert_eq!`, was fixed).
- Selections listed before running (`--list`, nonzero): `memory_scope_` 19, `memory_integration_`
  18. Final run, those plus the regressions below in one invocation: 119 selected, 119 passed
  (`a7-final-focused.log`). The new cases:
  - missing identities and cross-company ids: settings, lazy provisioning and the settings route;
  - a revoked provisioner;
  - a foreign-managed tenant (`tenant_conflict`, then `tenant_unmanageable`);
  - delete-after-create (only a 200 settles);
  - a credential-id mismatch refused without contact;
  - lifecycle resurrection refused;
  - lease CHECKs;
  - stolen and superseded claims fenced;
  - an ambiguous issue's stray revoked, and a namespace removed;
  - present → absent for a vanished owner;
  - a poison batch leaving the due set;
  - attempts and expired leases bounded;
  - shutdown joining.
- The competing cases use real concurrent transactions and `pg_blocking_pids` waits, in both
  orders:
  - company delete vs enable;
  - agent delete vs lazy provisioning;
  - member removal vs provisioning;
  - disable vs enable and provisioning.
- Mutation check: with company delete's `FOR UPDATE` reverted to the old unlocked read,
  `memory_scope_competing_company_delete_and_enable_serialize_in_both_orders` fails. The delete
  races past an uncommitted enable and leaves a `present` lifecycle for a deleted company
  (`a7-mutation-company-delete-unlocked.log`). The lock was restored.
- Regressions (counts from `--list`, nonzero):

  | Filter | Tests |
  | --- | --- |
  | `agent::retire_tests` | 3 |
  | `company_invite::` | 33 |
  | `delivery::tests::tenant_tests` | 8 |
  | `integrations::tests::lifecycle_tests` | 5 |
  | `credential_rotation` (now with the memory arm) | 14 |
  | `the_memory_provider_schema_is_gone` | 1 |
  | `worker_slots_respect_the_pool` | 1 |
  | `pages::tests::company` | 6 |
  | `account_deletion` | 1 |
  | `workflow_authoring_jev` | 2 |
  | `infra::setup::` | 1 |
  | `routes::company::` | 4 |
  | `ui_companies` | 3 |
  | `google_vault_callback_user_deletion` | 1 |

  All of them pass.
- Adapter behaviour was checked against the contract fake, which validates every request and 200
  response against `services/memory/openapi.json` and fails the test on any violation.
- `cargo sqlx prepare --check -- --all-targets` against a freshly migrated, then dropped,
  `open_bots_p7b_prepare`: passed. No macro was added, and `.sqlx/` is unchanged.
- `cargo fmt --all -- --check`, `git diff --check`, and the no-index whitespace check of the new
  files: passed. `python3 scripts/workflow-sample-boundary.py`: passed.
- `scripts/transport-boundary-check.sh`: fails on the one pre-existing rule
  (`channel_reply_continuation_views_support.rs:59`). On a clean `git archive a6bed48a` export
  it fails byte-identically (`a7-transport-boundary-base.log`), so the failure predates this
  work. Not fixed (unrelated).
- `graft build`: refreshed.
- Not run:
  - the full OpenBot suite (06C);
  - `scripts/stack-budget.sh` (the 05 gate; no new agent-chain seam here);
  - the live-service `memory_integration_live_` tests (not written yet; 05 gate);
  - a browser check of the new settings section in both themes (pages AGENTS). It reuses the
    TypeSafe section's classes only.

P7b correction round 1 (reviews `p7b-code-r1` and its findings recheck), 2026-11-08, base
`a6bed48a`. Same environment, on a recreated `open_bots_p7b_test` (the schema gained
`memory_lifecycles.transient_retries`). Logs: `tmp/memory-service-run/logs/a7-p7b-r1-*.log`.

- `cargo clippy --locked --offline --all-targets -- -D warnings`: passed (`a7-p7b-r1-clippy.log`).
- Selections listed first (`a7-p7b-r1-list.log`, nonzero): `memory_scope_` 28,
  `memory_integration_` 19.
- The regressions, listed in `a7-p7b-r1-regression-list.log`:

  | Filter | Tests |
  | --- | --- |
  | `agent::retire_tests` | 3 |
  | `company_invite::` | 33 |
  | `delivery::tests::tenant_tests` | 8 |
  | `integrations::tests::lifecycle_tests` | 5 |
  | `credential_rotation` | 14 |
  | `the_memory_provider_schema_is_gone` | 1 |
  | `worker_slots_respect_the_pool` | 1 |
  | `pages::tests::company` | 6 |
  | `account_deletion` | 2 (both memory tests, already in the `memory_scope_` count) |
  | `workflow_authoring_jev` | 2 |
  | `infra::setup::` | 1 |
  | `routes::company::` | 4 |
  | `ui_companies` | 3 |
  | `google_vault_callback_user_deletion` | 1 |
  | `every_workspace_write_refuses` | 1 |

- One invocation of the memory filters and the regressions: 130 selected, 130 passed
  (`a7-p7b-r1-final-focused.log`).
- New cases:
  - failed provisioning re-driven to `Ready`, with the tenant wait charged no attempt (M1);
  - a deletion surviving twelve consecutive 503s (M2);
  - a fresh budget after supersede, on both the crash path and the completion path (M3);
  - agent delete and account deletion against company delete, in both orders (M4);
  - concurrent claimants, the `SKIP LOCKED` expiry sweep, and a completion racing company delete
    ending `Lost` (m1);
  - mismatch refusal on tenant-absent and namespace-absent (m2);
  - shutdown during an in-flight request (m3);
  - the CSRF enumeration including the memory route (m4);
  - a returning member getting a new namespace id (n3);
  - failed rows listed at startup.
- Mutation probe, in an isolated copy with its own `CARGO_TARGET_DIR` (since removed): agent
  delete without its company `FOR KEY SHARE`. `memory_scope_competing_agent_delete_and_company_delete_never_deadlock`
  fails with `deadlock detected` (`a7-p7b-r1-mutation-agent-no-company-lock.log`).
- `cargo sqlx prepare --check -- --all-targets` against a freshly migrated, then dropped,
  database: passed (the moved `query!` texts are unchanged).
- `cargo fmt --all -- --check`, `git diff --check`, the untracked-file whitespace check and
  `scripts/workflow-sample-boundary.py`: passed.
- `scripts/transport-boundary-check.sh`: the pre-existing failure only, byte-identical to the
  `a6bed48a` export.
- `graft build`: refreshed.
- Not run (unchanged from the previous entry):
  - the full suite;
  - `scripts/stack-budget.sh`;
  - the live-service tests;
  - the browser check in both themes.

P7b correction round 2 (review `p7b-code-r2`), 2026-11-08, base `a6bed48a`. The environment and
database are unchanged (no schema change this round). Logs: `tmp/memory-service-run/logs/a7-p7b-r2-*.log`.

- Selections listed first (`a7-p7b-r2-list.log`, nonzero): `memory_scope_` 29,
  `memory_integration_` 21.
- The regressions (`a7-p7b-r2-regression-list.log`):

  | Filter | Tests |
  | --- | --- |
  | `agent::retire_tests` | 3 |
  | `company_invite::` | 33 |
  | `credential_rotation` | 14 |
  | `the_memory_provider_schema_is_gone` | 1 |
  | `worker_slots_respect_the_pool` | 1 |
  | `pages::tests::company` | 6 |
  | `account_deletion` | 2 (memory tests, already counted) |
  | `infra::setup::` | 1 |
  | `routes::company::` | 4 |
  | `every_workspace_write_refuses` | 1 |
  | `workflow_authoring_jev` | 2 |

- One invocation: 116 selected, 116 passed (`a7-p7b-r2-final-focused.log`).
- New cases:
  - definite failures not churned by `ensure`, the cool-down honoured, and a conflicting
    namespace replaced by a new id (r2-m1);
  - stuck outage rows listed at startup (r2-m2);
  - the owner retry: shown only on failure, owner/admin only, revision-checked, no epoch change
    (r2-m3);
  - the `Retry-After` floor and cap (r2-m4);
  - a 404-only revocation batch spending an attempt (r2-n2).
- `cargo clippy --locked --offline --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`, `git diff --check`, the untracked whitespace check and
  `scripts/workflow-sample-boundary.py`: passed.
- `cargo sqlx prepare --check -- --all-targets` against a fresh, then dropped, database: passed.
- `graft build`: refreshed.
- Not run: as in the previous entries.

P7b correction round 3 (review `p7b-code-r3`, r3-m1), 2026-11-08, base `a6bed48a`, same
environment. Logs: `tmp/memory-service-run/logs/a7-p7b-r3-*.log`.

- Selections listed first (`a7-p7b-r3-list.log`, nonzero): `memory_scope_` 30,
  `memory_integration_` 21. Together: 51 selected, 51 passed (`a7-p7b-r3-focused.log`).
- New cases:
  - the deleted-tenant 409 fails as `tenant_conflict`;
  - no new namespace id or lifecycle row is minted across repeated `ensure` and drain cycles,
    past the cool-down;
  - settings show the refusal at tenant level;
  - a fresh `namespace_deleted` tombstone is not replaced within the cool-down.
- `cargo clippy --locked --offline --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`, `git diff --check` and the untracked whitespace check: passed.
- No SQL schema change, and no macros added.

p05-B2 (P7c: 05A.3 steps, 05A.5 exactness, delivery and the step-level E6 race, 05A.6 samples),
2026-11-08, base `a6bed48a`, author generation 8. Environment as for P7b; disposable databases on
root's private cluster (`open_bots_a8_test`, recreated after each schema edit; `own_database` for every
claim or sweep; `open_bots_a8_prepare` migrated for the sqlx check, then dropped). Logs:
`tmp/memory-service-run/logs/a8-*.log`; acceptance copies and the manifest under
`plan/memory-service/phases/p05/docs/logs/` (`MEM-P05-B2-R1.evidence.json`). The criterion → test →
assertion mapping is in `phases/p05/docs/TASK.md`.

- Selections listed first (`a8-list-1.log`, `a8-p7c1-list-2.log`; nonzero): `memory_access_level_` 18
  at the end (exactness, saved labels, E6 race and exposure order), `memory_integration_` and
  `memory_scope_` with the delivery and step tests.
- Exactness and its hooks: 18 targeted tests passed (`a8-p7c1-focused-2.log`).
- Delivery queue, disable/owner/run-cancel settlement, the E9 race: with `memory_scope_` and
  `memory_integration_`, 60/60 (`a8-p7c2a-delivery-3.log`).
- Steps, gating and registry: 144/144 (`a8-p7c2c-steps-2.log`); the step round trip, disabled steps and
  run cancellation 3/3 (`a8-p7c2c-cancel-1.log`).
- Regression, `persistence::workflow::`, `persistence::thread::`, `application::workflow::`,
  `persistence::memory::`, `infra::workflow::` and `registry::` in one invocation: 2,066 passed and 4
  failed (`a8-p7c-regression-2.log`). The four were sample fixtures still scripting the old memory
  output shapes. They were updated, and `workflow::sample::` with the memory tests then passed 46/46
  (`a8-p7c-sample-1.log`). An earlier run of the same set (before the B2a test edits) passed
  2,006/2,006 (`a8-p7c1-workflow-1.log`).
- Final focused run on the formatted tree, one invocation (`memory_`, `workflow::sample::`, `registry::`,
  `infra::workflow::`, the schema allowlist, `worker_slots_respect_the_pool`, the six exactness-carrying
  regressions and `credential_rotation`): 194/194 (`a8-p7c-final-focused.log`).
- E6 mutations, both applied in an isolated copy with its own `CARGO_TARGET_DIR` under
  `tmp/memory-service-run/` (since removed), each failing its test (`a8-mutation-e6-ab.log`):
  (A) the freeze reads the run's levels without the run lock, so
  `memory_access_level_competing_recall_and_save_freeze_preserve_the_watermark` sees no lock wait
  (`Elapsed`); (B) the load's output is returned although its observation failed, so
  `memory_access_level_load_output_never_precedes_its_committed_observation` receives the output.
- `cargo clippy --locked --offline --all-targets -- -D warnings`: passed (`a8-clippy-1.log`).
  `cargo fmt --all -- --check`, `git diff --check` and the untracked-file whitespace check: passed.
- `cargo sqlx prepare --check -- --all-targets` on a freshly migrated database: passed
  (`a8-sqlx-prepare-check.log`; no macro was added, `.sqlx/` is unchanged).
- `scripts/workflow-sample-boundary.py`: passed (`a8-sample-boundary-1.log`).
- Not run: the full suite (06C); `scripts/stack-budget.sh` (05 gate); the live-service tests; the
  browser check; Docker/CI (unavailable here).
- Not implemented in this batch (handed on, see TASK "B2d"): the four agent memory tools, their
  consumption records and the tool-level E6 race; capture (05B).

p05-B2 correction round R2 (review `MEM-P05-B2-R1`, findings F1–F13), 2026-11-09, base `a6bed48a`,
author generation 8. Environment and fixtures as for R1; `open_bots_a8_test` recreated after the
schema edit (nullable `request`, its CHECK, the three indexes, the ordered run-cancel trigger). Logs:
`tmp/memory-service-run/logs/a8-r2-*.log`; acceptance copies and `MEM-P05-B2-R2.evidence.json`
(digest zero for root) under `plan/memory-service/phases/p05/docs/logs/`. Resolution per finding,
tests and stand-ins: `phases/p05/docs/TASK.md`, "p05-B2 correction round R2".

- Restore: a mutation probe in this worktree filled the disk. `memory/deliveries.rs` and
  `actions/failure.rs` were restored from the frozen R1 tree `acbda41e` and the R2 edits
  re-applied. `failure.rs` is byte-identical to its recorded pre-mutation hash; `deliveries.rs` has
  the same SQL as the mutation patches' context. Every check below ran on the restored tree.
- Selection listed first: 208 tests, nonzero, each R2 test present once (`a8-r2-final-list.log`).
- Focused run on the formatted tree (`memory_`, `registry::`, `infra::workflow::`,
  `workflow::sample::`, `actions::failure::`, the schema allowlist, `worker_slots_respect_the_pool`,
  `credential_rotation`, `the_catalogue_`): 208/208 (`a8-r2-final-focused-2.log`, listed 208 in
  `a8-r2-final-list-2.log`); 208/208 before rustfmt (`a8-r2-final-focused.log`). Final run after
  the F2 follow-up, with the forced ordering test and the three retargeted tests added: listed 212,
  212/212 (`a8-r2-final-list-3.log`, `a8-r2-final-focused-3.log`).
- Regression, `persistence::workflow::`, `persistence::thread::`, `application::workflow::`,
  `persistence::memory::`, `infra::workflow::` and `registry::` in one invocation: first run 2,082
  passed, 3 failed, 2 ignored (`a8-r2-regression.log`; the three also fail in isolation,
  `a8-r2-regression-isolated.log`). They were base recovery tests that used `memory.save` as their
  unknown-effect example, which F2 makes retry-safe. Root chose option 1: each test now starts from
  an unresolved ActionService marker (`unresolved_marker`, the `action_uncertainty_tests`
  pattern), with its assertions unchanged; the three passed 3/3 (`a8-r2-retarget-1.log`). Rerun:
  2,086 passed, 0 failed, 2 ignored (`a8-r2-regression-2.log`).
- B3 ordering, forced: `memory_integration_disable_and_run_cancel_overlapping_in_id_order_never_deadlock`
  passed 10/10 under the concurrent regression (`a8-r2-order-forced-repeat.log`). A holder
  transaction locks the middle intent, and the cancel stand-in is a bare run → `cancelled` update.
  A user cancel cannot serve: it and disable both lock the company row.
- Mutations (`a8-r2-mutation-*.log`, patches kept): A (freeze from the claim-time `RunKnowledge`),
  B (freeze read without `FOR UPDATE`), C (stored agent bit ignored), D (`memory.save` not
  marker-owned) and E (no in-flight company exclusion) each fail their discriminating test. F (DESC
  settlement order) survives the barrier test (`a8-r2-mutation-F-unordered-settlement-1.log`) but
  fails the forced test with `deadlock detected` (`a8-r2-mutation-F-unordered-settlement.log`).
  A–C were applied in this worktree with their own target. D–F ran in an isolated copy with its
  own target, deleted after each run.
- `cargo clippy --locked --offline --all-targets -- -D warnings`: passed, and passed again after the
  F2 follow-up (`a8-r2-clippy-2.log`). `cargo fmt --all -- --check` and `git diff --check`: passed,
  both times. The untracked-file whitespace check finds only blank lines at EOF
  in verbatim R1 log copies.
- `cargo sqlx prepare --check -- --all-targets` on a freshly migrated `open_bots_a8_prepare`
  (sqlx-cli 0.8.6, then dropped): passed. Not rerun after the follow-up, which added no query macro. `scripts/workflow-sample-boundary.py`: passed.
  `scripts/transport-boundary-check.sh` fails on `channel_reply_continuation_views_support.rs`,
  which this work does not change (pre-existing on base).
- Not run: as for R1 (full suite, stack budget, live-service tests, browser, Docker/CI). Not
  implemented: B2d (handed on); F10 (optional) recorded as defence in depth.

p05-B2 correction round R3 (recheck `MEM-P05-B2-R2`: F14, F15, F11 nit), 2026-11-09, base `a6bed48a`.
Environment and fixtures as for R2. Logs `tmp/memory-service-run/logs/a8-r3-*.log`; manifest
`MEM-P05-B2-R3.evidence.json` (digest zero for root). Resolutions: TASK, "p05-B2 correction round R3".

- Selection listed first: 213 tests, nonzero (`a8-r3-list.log`), with `worker_slots_respect_the_pool`,
  the 12 delivery tests (lanes, fairness, fences), the 11 memory step tests and the new F11 unit test.
  Run: 213/213 (`a8-r3-focused.log`).
- F14 mutation G (the lanes no longer take turns on the store), in an isolated copy with its own
  target (deleted afterwards), fails `memory_integration_delivery_lanes_send_companies_concurrently`:
  peak concurrent store calls 2, expected 1 (`a8-r3-mutation-G-ungated-store-lanes.log`).
- `cargo clippy --locked --offline --all-targets -- -D warnings`, `cargo fmt --all -- --check` and
  `git diff --check`: passed.
- Not rerun: the wider regression and `sqlx prepare --check`. The change has no query macro or
  schema change, and its surfaces are in the focused selection.
