# AWC-P02 — Target snapshots and dependency scopes

## Identity

- Plan `plan/agent-workflow-call`, phase `p02`, doc [02-target-snapshots-and-dependency-scopes.md](../../../02-target-snapshots-and-dependency-scopes.md).
- Workflow implement-opus-reviewed-2_4; records at
  `/Users/mac02/orca/workspaces/openbot/plan08_ph1/plan/agent-workflow-call/phases/p02/docs`.
- Root/A: Claude Code `1e84a7af`, Opus 5.5/xhigh (user-confirmed). B: Fable/medium `a8d0fa1fbd0b8b88e` (continued from p01).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, HEAD `fe7791d3`.
- Prerequisite: p01 accepted in phase at digest `2ebbc0cd…feaa` (uncommitted working tree; p02 builds on it).
- Coordination: `plan/agent-workflow-call/COORDINATION.md` r10; AWC-C001 r4 (authority model), AWC-C002 r1 (none targeted at p02).
- Resources: scratch DBs `openbot_awc_mac02`, `openbot_awc_mac02_test` (shared with p01 records; same session).

## Status

- Gate AWC-P02-D1 r1: not passed (B note AWC-P02-D1-R1-N1, findings F1–F4 actionable, F5 optional).
- Gate AWC-P02-D1 r2 (`logs/AWC-P02-D1-r2-decision.md`) answered F1–F5; not passed (AWC-P02-D1-R2-N1: F6
  medium, F7 low).
- Gate AWC-P02-D1 r3 (`logs/AWC-P02-D1-r3-decision.md`, a delta on r2) answers F6–F7. **PASSED** (AWC-P02-D1-R3-N1, digest `7e189031…2125`).
  Deferred to the batch: F8 (low): map non-role capture refusals (graph unavailable/cyclic/over limits) to
  `destination_unavailable` under the same savepoint, with one test.
- Batch AWC-P02-B1 in implementation (2026-11-22). Done so far:
  - migration: `workflow_channel_calls` (named FKs, immutability guard `workflow_channel_call_immutable`, deferred
    insert guard `workflow_channel_call_snapshot`); `run_dependency_manifests` FK to budgets replaced by FK to runs and
    the deferred three-case trigger `run_dependency_scope`; column comment.
  - `agent_manifest::CaptureScope` shared by `agent_manifest::capture_on` and `run_scopes::capture_on`; admission
    passes `CaptureScope::admission`.
  - `action_account_hints`: `Purpose::ChannelCall`, `CallAccountHint`, `prelock_channel_call_on` (optional run_as).
  - `domain::entities::channel::participant_authorized`, shared by `Channel::participant_access`.
  - `children::create_call_on` (savepoint; role/graph refusal → destination_unavailable; unit cap →
    budget.unit_exhausted) and `CallChild`; `create_on` unchanged for ordinary children.
  - `channel_call_target.rs`: lock order doc, `start_on` steps 1–9, `record_on`, desk facts, ancestry.
    Module carries `cfg_attr(not(test), expect(dead_code))` until p03's port calls it.
  - Tests `channel_call_snapshot_tests.rs` (V0–V15, F8 classification). One batch AWC-P02-B1 covers doc 02 A and B.
- **Batch AWC-P02-B1 implemented and frozen for review R1** (2026-11-22): digest
  `543ef9e9e262f537971b9bc5fd6abcb6038825c712893c811a7036ae97181777`, base `fe7791d3` = HEAD, exclusions
  `plan/agent-workflow-call/phases/p02/docs` and `.trail/learnings` (`logs/AWC-P02-B1-R1-snapshot.txt`). The
  digest includes p01's accepted uncommitted changes; p02's own delta is what changed since `2ebbc0cd…feaa`.
  Evidence manifest `logs/AWC-P02-B1-R1.evidence.json` (validated: 5 PASS, 4 FAIL, 5 NOT EXERCISED).
  With B (`a8d0fa1fbd0b8b88e`) for review.

## Acceptance

**p02 accepted in phase** (2026-11-22) at digest `0639c0f03d391ed087ddf434513e123753578451483602bd069fd989b3122112`
(base `fe7791d3`, exclusions `plan/agent-workflow-call/phases/p02/docs`, `.trail/learnings`), recomputed by root.
- B's explicit PASS: `AWC-P02-B1-R2-N1`.
- F1–F4 closed; F5 optional, not taken.
- Evidence `logs/AWC-P02-B1-R2.evidence.json` validated.
- Coordination rechecked at r10 → r11: AWC-C001 r4 satisfied for p02; AWC-C003 r2 issued to p03.
- Not integrated or committed.
- Open verification gaps, carried by p03 or the final gate:
  - the AWC-C003 NOT EXERCISED items;
  - two load-sensitive tests: `agent_history_recovery_parent_cancel_private_child_and_other_root_admission_do_not_deadlock`
    and `workflow_action_reconciliation_bounds_128_prior_consuming_129_total_then_lost_refuses`, each passing
    alone;
  - two failures unrelated to p02: `doop_agent_current_preparation_reaches_ready_before_model_io`
    (version 7 vs 8) and the cloudroom e2e timeout under load.

## Batch AWC-P02-B1 — R1 corrections (round R2)

Frozen for recheck R2 at `0639c0f03d391ed087ddf434513e123753578451483602bd069fd989b3122112` (base `fe7791d3`, same
exclusions; `logs/AWC-P02-B1-R2-snapshot.txt`); evidence `logs/AWC-P02-B1-R2.evidence.json` (5 PASS, 2 FAIL, 5 NOT
EXERCISED). Checks:
- `channel_call_`: 38 passed (`AWC-P02-B1-R2-focused.txt`).
- Affected (`channel_call_ workflow_child workflow_tool_ autonomous_agent_children_ autonomous_agent_storage_manifest_ run_scope workflow_action_ message_send reply_`, 6 threads): 827 passed, 1 failed (`AWC-P02-B1-R2-affected.txt`).
  The failure is `workflow_action_reconciliation_bounds_128_prior_consuming_129_total_then_lost_refuses`. It passed
  in R1's affected run and passes 3/3 alone at ~115 s each (`AWC-P02-B1-R2-bounds-rerun.txt`). It goes through the
  account prefix that F4 restructured (no behaviour change), so it is reported as **load-sensitive, not proven
  unrelated**.
- sqlx check exit 0; fmt clean; `source_modules` 8 passed; clippy: the same 9 pre-existing lints.

Review R1 (`AWC-P02-B1-R1-N1`) required changes: F1–F3 (tests), F4 (code style). F5 optional, not taken:
narrowing `destination_refusal`'s `BadRequest` arm needs a typed capture refusal that does not exist yet; a
capture defect surfacing as `BadRequest` reads as `destination_unavailable`, which is the gated mapping.

- **F1.** `CallSetup` puts the desk in its own unit under the root (`test_unit`, `finance-unit`). V0 asserts that
  the child's write unit is the desk unit, that the caller's is the root, and that the two differ. V15 admits its
  two callers on a desk in a sibling unit (`claim_from`, association `Channel`), so the two cap chains share only
  the root. An exhausted cap on the caller's unit: the call starts and the child's write unit is the desk's. An
  exhausted cap on the desk's unit: `budget.unit_exhausted`, fence held, no child of that caller, one record.
  Caps roll up to ancestors (`unit_budgets::check_admission_on`), so a cap on a shared ancestor refuses both by
  Plan07 design. The gate's "a cap on the caller's unit alone does not refuse" holds only for a caller unit that
  is not the desk's ancestor, which is the case tested.
- **F2.** New `channel_call_target_facts_participate_grant_opens_members_only_desk`: members-only desk; grant
  `participate` to the calling agent's principal → `Started`; revoke → `target_unavailable`; one record.
- **F3.**
  - V2 now runs every case through `forged()`, which reports the constraint that refused the statement itself,
    or the named deferred constraint made immediate. New cases:
    - same invocation key twice → `workflow_channel_calls_pkey`;
    - another company's channel (a real second company) → `workflow_channel_call_binding_fk`.
  - V2's actor case asserts `workflow_channel_call_snapshot` by name. The UPDATE succeeds; the record's
    deferred check refuses it.
  - "Other binding revision" cannot be forged in a state that reaches the snapshot clause:
    - the record is immutable;
    - `channel_reply_settings` refuses a binding-revision change without a new settings revision ("invalid
      channel response settings revision");
    - with a new settings revision it refuses a non-current binding revision ("invalid channel responder
      selection").

    The case is dropped. V4 covers the real path: reconfiguration selects one whole revision. Correction from
    R2 (`AWC-P02-B1-R2-N1`): a forged INSERT naming another existing revision would reach the clause, so
    "unforgeable" overstates it. B accepted the omission because the clause sits in the same conjunction that
    four other V2 cases show refusing.
  - V11: new `channel_call_target_facts_archived_unit_declines` archives the desk's unit through
    `archive_department` and expects `unit.archived` with zero rows.
  - V3: the loser is `AppError::Execution(OwnershipLost)`, so the fence is the first arbiter. The primary-key
    backstop is now refused directly in V2.
- **F4.** `action_account_hints::prelock_with_native` returns the named struct `Prelocked { actor, source, locked }`.
- B's open question on the archive race: `begin_department_write` → `company_level_authority::lock_team` takes
  `companies … FOR NO KEY UPDATE` (`company_level_authority.rs:114`). Dispatch takes the company lock at step 1,
  so an archive and a dispatch serialize.

## Batch AWC-P02-B1 — contract changes found in implementation (not in gate r3)

1. **`guard_workflow_scope_seal` extended** (init schema). The seal guard required every child's unit scopes to
   copy its sealed parent's exactly, which contradicts gate D4 (a call child's scopes are the desk's). After
   `inherited := admitted.parent_run_id`, it now clears `inherited` when a `workflow_channel_calls` row names this
   run as the child of that parent. Only a recorded call child is exempt; an ordinary child still copies.
2. **Write order in `children::write_call_on`**: `record_on` runs before `run_scopes::capture_on` (the guard reads
   the record). Order: run row/job/admission/wait (`write_on`) → `agent_manifest::capture_on` → `persist_on` →
   `record_on` → `run_scopes::capture_on` → `unit_budgets::check_admission_on`, all under savepoint
   `channel_call_child`. The record's deferred guards still check the finished shape at commit.
3. **`run_dependency_manifests` FK to budgets replaced** by `(company_id, run_id) → workflow_runs` (deferrable),
   as gated (D2); the three-case `run_dependency_scope` trigger carries the scope rule.
4. Capture signatures kept on `Transaction` (a `PgConnection` change cascaded into `resources::check_readiness`);
   the savepoint is a nested `sqlx::Connection::begin` in `children::create_call_on`.

## Verification mapping (batch AWC-P02-B1)

All in `src/adapters/persistence/workflow/channel_call_snapshot_tests.rs`; log `logs/AWC-P02-B1-focused.txt`.

| ID | Test | Decisive assertion | Leaves unproved |
| --- | --- | --- | --- |
| V0 | `channel_call_snapshot_positive_control_commits` | Child row = (actor run_as, desk channel, responder binding, ceiling 0, content 0, parent caller, `deliver`); manifest scope root = child while budget root = caller's root; child write unit = the desk's non-root unit, caller's = root, and they differ (R2, F1); record = (settings rev 1, responder, run_as, child). | `content_exact` not asserted directly. |
| V1 | `…_record_is_immutable_and_replays` | Same key+digest → `Replayed{same child}`; other digest → `invalid_input`; one record; UPDATE and DELETE refused by `workflow_channel_call_immutable`. | Replay through the p03 port. |
| V2 | `…_sql_refuses_forged_rows` | Each forged statement under a savepoint is refused by its named constraint, at the statement or as a deferred check made immediate (`forged()`): same key twice → `workflow_channel_calls_pkey`; another company's channel → `workflow_channel_call_binding_fk`; foreign binding → `workflow_channel_call_binding_fk`; wrong scope manifest → `workflow_channel_call_scope_fk`; child = caller → `workflow_channel_call_snapshot`; settings revision bumped → `workflow_channel_call_snapshot`; child actor ≠ run_as → `workflow_channel_call_snapshot`. Untampered: both new guards hold. | Other-binding-revision is unforgeable (see R2 F3), covered by V4. |
| V3 | `…_duplicate_key_converges` | Two concurrent `start` on one key: exactly one Ok, the loser `Execution(OwnershipLost)` (fence first); one record. The PK itself: V2. | Fence convergence through the p03 port. |
| V4 | `…_reconfiguration_selects_one_whole_revision` | Concurrent reselection to another responder/run_as: the record is (rev1, old, old run_as, child actor old) or (rev2, new, new run_as, new actor), or `Conflict` with zero rows. | Which interleaving occurred is not forced. |
| V5 | `…_competes_with_run_as_account_write` | `users FOR UPDATE` on run_as then company lock, concurrently with dispatch: both succeed, no deadlock. | — |
| V6 | `…_revoked_or_unready_target_is_denied` | run_as loses admin on the desk unit → `destination_unavailable`, zero rows. | `verify_selection` failure and resource revocation are not separate cases. |
| V7 | `channel_call_manifest_call_scope_requires_invocation` | V0's scope shape without a record → `run_dependency_scope` refuses. | — |
| V8 | `…_child_cannot_take_sibling_or_foreign_scope` | Sibling's and foreign tree's `(root, manifest)` → `run_dependency_scope` refuses. | — |
| V9 | `…_pins_survive_publication` | Republish (save rev1 → publish rev2) after capture; the child still resolves the captured workflow version. | **Deviation:** checks the workflow version pin, not an agent revision pin. |
| V10 | `…_descendant_inherits_call_scope` | An ordinary child of the call child takes case (c) on the call scope; the caller's selections are not visible. | — |
| V11 | `channel_call_target_facts_decide_before_any_row`, `…_participate_grant_opens_members_only_desk`, `…_archived_unit_declines`, `…_ancestry_includes_self_and_nested_calls` | members-only without grant and foreign/missing desk → `target_unavailable`; members-only with the agent's `participate` grant → `Started`, revoked → `target_unavailable`; archived desk unit → `unit.archived`; disabled → `desk_disabled`; cleared → `no_responder`; zero rows. Ancestry: call child's own desk is in its ancestry, another desk and the caller's are not. | Empty-desk-under-bootstrap not exercised. |
| V12 | `channel_call_snapshot_intake_refused_before_write` | Invalid field → `intake_invalid` with `schema` object and `paths` array; no intake + fields → `intake_not_declared`; zero rows. | — |
| V13 | `…_audience_refused_above_ceiling` | Caller content 1 → `audience_refused`, zero rows (level-0 commit is V0). | — |
| V14 | `…_role_refusal_is_denied` | Required role with no holder → `destination_unavailable`; zero children in the same tx after savepoint rollback; `lock_fence` still true; zero rows after commit. | **Deviation:** gate said ambiguous holders; uses NoHolder (same `WorkflowRole` path). |
| V15 | `…_unit_cap_is_denied` | Callers admitted on a desk in a sibling unit (cap chains share only the root): caller's unit exhausted → call starts, child's write unit is the desk's; desk unit exhausted → `budget.unit_exhausted`, fence held, no child of that caller, one record. | A cap on a shared ancestor refuses both (Plan07 rollup, by design). |
| F8 | `…_destination_refusals_are_classified` | BadRequest(graph)/NotFound/Forbidden/UnitArchived → denial; Timeout/Database/Conflict/Internal propagate. | No DB-backed graph-unavailable refusal (classification only). |

## Checks (batch AWC-P02-B1), env `DATABASE_URL=…/openbot_awc_mac02 TEST_DATABASE_URL=…/openbot_awc_mac02_test SQLX_OFFLINE=true`

| Check | Result | Log |
| --- | --- | --- |
| `cargo test --offline --lib channel_call_ -- --test-threads 4` (after fresh DB recreate + migrate) | 36 passed | `AWC-P02-B1-focused.txt` (attempts 1–4 retained: fixture/schema fixes, and the seal-guard finding in attempt 2) |
| affected: `cargo test --offline --lib -- adapters::persistence::workflow:: …agent_publication …agent_authoring …department …unit_budgets application::workflow:: --test-threads 6` | 2869 passed, 2 failed (unrelated, below) | `AWC-P02-B1-affected.txt` |
| child-writer consumers after the clippy refactor: `… -- channel_call_ workflow_child workflow_tool_ autonomous_agent_children_ autonomous_agent_storage_manifest_ run_scope --test-threads 6` | 84 passed, 1 failed (flake, below) | `AWC-P02-B1-final-focused.txt` |
| `cargo sqlx prepare --check -- --all-targets` | exit 0 | `AWC-P02-B1-sqlx-check.txt` |
| `cargo clippy --offline --locked --all-targets -- -D warnings` | exit 101: 9 `redundant_field_names` in 8 test files unmodified since `fe7791d3`; none in p02 files | `AWC-P02-B1-clippy.txt` |
| `cargo fmt --all -- --check && git diff --check` | exit 0 | `AWC-P02-B1-fmt.txt` |
| `cargo test --locked --offline --test source_modules` | 8 passed | `AWC-P02-B1-source-modules.txt` |

Unrelated failures (code untouched by p02):
- `cloudroom_e2e_cut_points_reconstruct_server_plugin_and_workers_without_blind_native_retry`: timeout under
  6-thread load; passes alone (`AWC-P02-B1-affected-rerun.txt`).
- `doop_agent_current_preparation_reaches_ready_before_model_io`: fails alone too; expects preparation format 7,
  `PREPARED_AGENT_VERSION` is 8 since commit `2edd0a73`.
- `agent_history_recovery_parent_cancel_private_child_and_other_root_admission_do_not_deadlock`: statement timeout
  under load in the final focused run; 3/3 alone (`AWC-P02-B1-deadlock-rerun.txt`). It exercises the child writer
  p02 changed, so it is reported as **load-sensitive, not proven unrelated**.

NOT EXERCISED in p02 (owned by p03 under AWC-C003 r2): fence convergence of two claimants through dispatch; replay
through dispatch; changed arguments on a real saved call; launch-time authority controls (AWC-C002).
- Placements in r2: p02 takes 03 A.1 (typed child identity in `children::create_on`); p03 keeps the
  `call_channel` port and dispatch (notice AWC-C003 r1, proposed).
- Coordination checked at r10: AWC-C001 r4 applies; AWC-C002 r1 targets p03–p05; AWC-C003 r1 originates here.

## Investigation notes (2026-11-22)

- `run_dependency_manifests(company_id, run_id, root_run_id, manifest_id)`: every run resolves agent and
  workflow pins through its own row (`agent_manifest/storage.rs` `selected_with_locks_on`,
  `workflow_bundle_on`, `attach_child_on`), so consumers already read the run's own scope.
- The obstacle: `root_run_id` is tied by FK both to `workflow_run_budgets(company_id, run_id, root_run_id)`
  (the causal/budget tree root) and to `agent_dependency_manifests(company_id, id, root_run_id)` (the
  manifest's root, `UNIQUE(company_id, root_run_id)`). The completeness trigger
  `check_agent_manifest_members` requires a row with `run_id = root_run_id = manifest root`. So a
  scope root and a causal root are one run today. A call child needs its own scope root inside the
  caller's budget tree, so the schema must separate the two: an explicit scope root (02 B.2).
- Manifest capture `agent_manifest::capture_on` takes a `PreparedAdmission`; it must accept the
  destination binding, its `run_as` actor (U1) and the desk association without one (02 B.1).
- Other readers of these columns to trace: init schema lines ~14607, 14817, 16593–16640 (role bindings),
  16998, 17039, 17132–17151 (`workflow_follow_manifest_ownership`), 17259, 17942, 20908.
