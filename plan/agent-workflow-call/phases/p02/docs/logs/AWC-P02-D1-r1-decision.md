# AWC-P02-D1 r1 — architecture gate: invocation record, target capture, explicit dependency scope

Author: root/A (Claude Code `1e84a7af`, Opus 5.5/xhigh), 2026-11-22. Anchors are at `fe7791d3`
with p01's accepted working tree (digest `2ebbc0cd…feaa`). Governing authority: AWC-P01-D1 r3
(D1.1 order, D1.2 fact table), notices AWC-C001 r4 and AWC-C002 r1.

This gate covers both batches of [02](../../../../02-target-snapshots-and-dependency-scopes.md),
because B depends on A's table and A's capture needs B's capture refactor. They are implemented
and reviewed as one batch, AWC-P02-B1.

## Facts the decisions rest on (traced)

- **Saved calls have no row.** An agent's saved tool call lives in
  `task_harness_runs.checkpoint->'invocations'`, keyed by `invocation_id` (a `ToolCallId` uuid)
  and validated by `agent_origin::saved_call_on` (`agent_origin.rs:164`). A workflow-tool child is
  found again only through `workflow_waits.child_tool_call_id`
  (`workflow_tool_calls.rs:122 replayed_call_on`). The agent origin of an action is recorded as
  columns `agent_manifest_id, agent_id, agent_revision_id, agent_binding_id`, with FKs to
  `run_dependency_manifests(company,run,manifest)` and `agent_manifest_revisions`
  (`workflow_action_origin_*`, init schema ~12922).
- **The responder snapshot already exists for messages.** `capture_channel_reply_context()` (init
  schema ~13089) runs on intent insert. It locks `channels … FOR SHARE` and copies
  `channel_reply_settings` (`binding_id, binding_revision, revision, contract, approval_policy`)
  plus the pinned revision's `run_as_user_id` into `channel_reply_contexts`. Settings are one
  mutable row per desk. They are written only by `configure_reply_on` (`channel_reply.rs:134`),
  which takes `users FOR SHARE` (approval accounts) → actor authority → `channels FOR UPDATE` →
  read/write settings. The settings guard requires the selected binding to be active,
  desk-level and at its current revision when selected. Binding revisions are immutable rows.
- **Manifest capture is admission-shaped only in its argument.** `agent_manifest::capture_on`
  (`agent_manifest/mod.rs:55`) reads from `PreparedAdmission` only: company, actor, the binding's
  bundle and resources, association (role context department) and `proposed_run_id` (manifest
  root). It sets its own `lock_timeout=1000ms` and `statement_timeout=5000ms` and has a 5 s
  deadline. `persist_on` writes the manifest and `run_dependency_manifests(run, root=run)`.
- **Scope root and budget root are welded by one FK.** `run_dependency_manifests(company_id,
  run_id, root_run_id)` references `workflow_run_budgets(company_id, run_id, root_run_id)`, the
  causal tree root. `root_run_id` also references `agent_dependency_manifests(company,id,root_run_id)`.
  Every reader treats `ownership.root_run_id` as the run that owns the manifest:
  - role bindings are stored at the manifest root (`reviewer_resolution.rs:44`, `reads.rs:246`,
    `role_views.rs:55,85`, `agent_manifest/storage.rs:251`, and FK `(manifest_id, run_id) →
    agent_dependency_manifests(id, root_run_id)`);
  - `follow_reviewers` and the follow guard (~17259) copy it;
  - `run_provenance` reports it as `RunDependencies.root`.

  No reader uses it as the budget root. The budget root is always read from
  `workflow_run_budgets`.
- **Child rows inherit through the parent's row.** `attach_child_on` (`storage.rs:41`) reads it,
  and is called only from `children::insert_child_on`. Budget rows are written by a trigger on
  `workflow_runs` insert (init ~776). Retention deletes whole budget trees
  (`storage_retention.rs:48`).

## D1 — The invocation record: `workflow_channel_calls`

One immutable row per saved channel call, written in the same transaction as the child launch
(p03 A.4). It owns the facts no other row owns:

| Column | Meaning |
| --- | --- |
| `company_id, run_id, execution_id, call_id` | **PK**. The calling execution and saved `ToolCallId`. |
| `arguments_digest` | `canonical::digest` of the saved call's canonical arguments (text, 64 hex). |
| `agent_manifest_id, agent_id, agent_revision_id, agent_binding_id` | The calling agent's origin, as `workflow_action_intents` records it. |
| `channel_id` | The destination desk. |
| `binding_id, binding_revision` | The responder selected by the settings revision below. |
| `settings_revision, contract, approval_policy` | Copied from `channel_reply_settings`, as `channel_reply_contexts` copies them. |
| `run_as_user_id` | The pinned revision's `run_as`, which is the child's actor (U1). |
| `child_run_id` | **UNIQUE (company_id, child_run_id)**. |
| `scope_manifest_id` | The child's own dependency manifest (D2). |
| `created_at` | |

Constraints:

- FK `(company_id, run_id, execution_id) → workflow_executions(company_id, run_id, id) ON DELETE
  CASCADE`. Teardown follows the caller's run.
- FK `(company_id, run_id, agent_manifest_id) → run_dependency_manifests(company_id, run_id,
  manifest_id)` and FK `(company_id, agent_manifest_id, agent_id, agent_revision_id) →
  agent_manifest_revisions`. The calling agent must be selected in the caller's own scope.
- FK `(company_id, channel_id, binding_id) → workflow_bindings(company_id, channel_id, id)` and
  FK `(company_id, binding_id, binding_revision) → workflow_binding_revisions`. Both are
  DEFERRABLE and have no cascade, as for `channel_reply_contexts`.
- FK `(company_id, child_run_id) → workflow_runs(company_id, id)` DEFERRABLE.
- FK `(company_id, child_run_id, scope_manifest_id) → run_dependency_manifests(company_id, run_id,
  manifest_id)` DEFERRABLE.
- `contract` and `approval_policy` carry the same CHECKs as `channel_reply_contexts`
  (`valid_channel_reply_approval`, size, `format_version`).
- **Immutable.** A BEFORE UPDATE OR DELETE trigger refuses unless the calling run is gone (the
  `guard_agent_manifest` pattern), so the record survives while the caller waits and goes with
  its tree.
- **Insert guard**, a deferred constraint trigger, which proves the row against live rows at
  commit:
  1. The child's budget row has `parent_run_id = run_id` and the caller's `root_run_id`. Tree
     limits and lineage stay with the caller (D1.2).
  2. The child's `actor_id = run_as_user_id = workflow_binding_revisions.run_as_user_id` of the
     recorded revision.
  3. The child's `binding_id` and `channel_id` equal the recorded ones. The child's association
     is the desk inbox, never the caller's thread or unit.
  4. Settings as read under the insert's own lock: `channel_reply_settings` for `channel_id` has
     `revision = settings_revision` with the same binding, revision, contract and policy. The
     transaction holds `channels FOR SHARE` (D3), so this is the configuration that was current
     when the call committed.
  5. The child's `delivery_policy` equals the caller's (D1.2).

  The child's trigger cause (`ChildCause::ToolCall` with this execution and `call_id`) is checked
  by p03's launch guard, because p03 owns the source message and the wait. p03 also adds FK
  `workflow_waits(company, run, execution, child_tool_call_id, child_run_id) →
  workflow_channel_calls` for channel-call waits.
- **No status column.** Child, wait, review and tool-result rows already own call state (02 A.2).

Replay: a lookup by PK returns the record. Equal digest returns the same invocation; a different
digest is the error `channel_call.invalid_input` ("this call ID was already used with other
arguments") and writes nothing. A replay never re-reads settings (02 A.5), and the deadline is
resolved from the record first (AWC-C002).

Convergence: dispatch holds the job fence (`lease::lock_fence`) before any write, so two
claimants cannot both reach the insert. The PK is the second guard. Test: two transactions
insert the same key and exactly one commits.

## D2 — Explicit dependency scope

`run_dependency_manifests.root_run_id` keeps its name and is documented
(`COMMENT ON COLUMN`) as the **scope root**: the run that owns the manifest. That is already what
every reader means by it, so renaming would only churn ~15 statements.

- Drop FK `(company_id, run_id, root_run_id) → workflow_run_budgets(company_id, run_id,
  root_run_id)`. Add FK `(company_id, run_id) → workflow_runs(company_id, id) ON DELETE CASCADE
  DEFERRABLE INITIALLY DEFERRED`. Teardown is unchanged, because the budget row cascaded from the
  run as well.
- Add a deferred constraint trigger `check_run_dependency_scope` AFTER INSERT. A row is valid in
  exactly one of three cases:
  - **(a) Root.** `run_id = root_run_id` and the run's budget row has `parent_run_id IS NULL`.
  - **(b) Call scope.** `run_id = root_run_id` and a `workflow_channel_calls` row has
    `child_run_id = run_id` and `scope_manifest_id = manifest_id`.
  - **(c) Inherited.** `run_id <> root_run_id` and the parent's own row (parent from
    `workflow_run_budgets.parent_run_id`) has the same `(root_run_id, manifest_id)`.

  These cases prove three things. A random child cannot claim a fresh scope, because there is no
  invocation for it. A child cannot take a sibling's or a foreign tree's scope, because it must
  match its own parent. An inherited row cannot skip a level, because each level checks its
  parent. Existing ordinary children satisfy (c) unchanged.
- The manifest tables, the members/completeness trigger (`run_id = root_run_id = manifest root`)
  and the immutability guards are unchanged. A call-scope manifest has `root_run_id = child run`,
  so its role bindings are stored at the child (FK `(manifest_id, run_id) → (id, root_run_id)`).
  Its role context is the destination desk's unit.
- Ordinary descendants of a call child inherit the call child's scope through (c) and
  `attach_child_on`, unchanged (02 B.5). A nested `channel.call` makes a new scope through (b).
  Tree limits stay on the budget root.

## D3 — Target capture, capture refactor and lock order

**Capture refactor (02 B.1).** `agent_manifest::capture_on(tx, &CaptureScope)`, where
`CaptureScope { company, actor, binding: &ConfiguredBinding, association, scope_root: RunId }`:

- Admission passes `CaptureScope::admission(command)`, with the same values it uses today.
- A call passes the destination run_as actor, the pinned revision as a `ConfiguredBinding`, the
  desk inbox as association, and the proposed child id as scope root.

The walk, bounds, timeouts, grant checks, bundle verification and `persist_on` are shared,
unchanged. Nothing is preloaded into the caller's manifest (02 B.2).

**Target read (02 A.1, A.4).** New module `adapters/persistence/workflow/channel_call_target.rs`,
beside `channel_reply.rs`:

- `desk_facts_on(tx, caller, desk) -> Option<DeskFacts>` supplies p01's `decide_target`.
  - `writable`: the calling agent's internal-relay write right, as in `routing.rs authorize_channel`.
  - `in_ancestry`: a recursive walk up `workflow_run_budgets.parent_run_id` from the caller,
    collecting each run's `channel_id` and each recorded invocation's `channel_id`. It is bounded
    by the existing tree depth.
  - `unit_archived`: as `refuse_archived_desk_on`.
  - `enabled` and `responder`: from settings.
- `capture_on(tx, company, desk) -> CaptureOutcome::{Denied(DeniedCode), Captured(ChannelCallTarget)}`.
  It reuses `settings_on` and restores the pinned revision through the binding-row reader with
  bounded bundle restoration. Then it re-proves the destination as message admission does:
  - `management::authorize_association_on(run_as, inbox(desk))`;
  - `resources::association_reach_on` + `check_readiness` (stage Admission);
  - `ChannelReplyContract::validate` against the output schema.

  Each failure here is the denied closure `channel_call.destination_unavailable` (D1.1 step 8).
- `record_on(tx, &ChannelCallRecord)` inserts the D1 row. `replayed_on(tx, scope, call_id)`
  reads one back.

**Lock order**, for one transaction (recorded as the module doc of `channel_call_target.rs`):

1. caller `action_authority::prelock_on`;
2. `lease::lock_fence`;
3. replay lookup;
4. unlocked settings read to learn run_as;
5. `authorize_association_on(run_as, inbox(desk))`, which takes person/company locks and then
   `channels FOR SHARE`;
6. settings re-read under that share lock;
7. resource readiness;
8. manifest capture (its own 1 s/5 s bounds);
9. p03's launch writes and `record_on`.

Steps 5–6 take users before channels, the same order as `configure_reply_on` (users share →
channels update), so the two cannot deadlock. If the re-read in step 6 differs from step 4
(settings changed between), the attempt rolls back and redoes steps 4–6 once. A second mismatch
returns `Conflict`, and the job retries under its existing claim rules. No lock is held across a
model or provider call: dispatch runs after the model turn has been saved.

**Port placement (amends 02 A.1).** The application port is one method,
`call_channel(fence, call) -> AppResult<AgentToolObservation>`. It mirrors `call_workflow_tool`,
because only the launch transaction may compose capture, record and child creation (p03 A.4).
p03 owns it. p02 delivers the persistence functions above, and its DB tests compose them in one
test transaction with a child row written by `admission_write::insert_run` (ParentLink to the
caller, actor run_as, desk association).

**Reconfiguration races (02 A.5).** `configure_reply_on` needs `channels FOR UPDATE`, which the
capture's share lock blocks until commit. A call therefore records either the complete old
settings or the complete new ones. Binding revisions are immutable, and the insert guard (D1.4)
re-proves the settings at commit.

**Pre-launch refusal (02 A.6).** A refusal or denied closure returns before `record_on` and any
child write. The transaction rolls back or writes only the tool result, so no child, thread,
message, job or response obligation exists. A denied closure has no invocation row, so a replay
re-decides it from current facts. That is harmless, because nothing was launched.

## Invariants

1. One invocation per (company, execution, call id). Its arguments are bound by digest. It is
   immutable while the caller exists.
2. A child's fresh scope exists only with an invocation naming that child and that manifest.
3. A call child's actor, binding, desk, delivery policy and budget lineage are the recorded
   ones; the database proves them at commit.
4. Recorded settings were the current ones under the capture's own lock.
5. Existing root and ordinary-child scope behavior is unchanged.

## Failure modes

| Failure | Result |
| --- | --- |
| Lost response, then retry | PK replay returns the same invocation and child. Settings are not re-read. |
| Same call ID, other arguments | `channel_call.invalid_input`; nothing written. |
| Responder replaced mid-dispatch | Blocked by the share lock, or detected at step 6 and retried once. |
| Params or resources edited mid-dispatch | A new binding revision cannot be selected without a settings write, which is blocked. A resource revocation fails readiness: `destination_unavailable`. |
| Capture timeout | `AppError::Timeout`. The transaction rolls back and the existing job retry applies. |
| Forged SQL insert (foreign company, wrong parent, sibling scope, unrecorded revision) | Rejected by the FKs and triggers above. |

## Affected contracts and consumers (02 B.4)

| Consumer | Disposition |
| --- | --- |
| Agent preparation and origin (`selected_with_locks_on`, `agent_origin`) | Read the run's own row: correct per scope. |
| Role bindings and follow decisions | Keyed by the scope root: correct. A call child's roles resolve for the destination unit. |
| History selection (`agent_history_sessions::capture`) | p03: it takes `PreparedAdmission` today; the call launch supplies the child's own session. |
| `run_provenance` and dependency views | `root` already means scope root. p05 B.2 labels it. |
| Samples | p05 B.3; samples create no record. |
| Revocation and liveness | D1.5 (p03). |
| Retention and teardown | Budget-tree deletion removes caller, child, record and manifests together. The immutability guards allow deletion once the run is gone. |
| `run_scopes::capture_on` | p03: computed for the destination desk (D1.2). Not changed here. |

## Verification plan

Fresh isolated schema `openbot_awc_mac02_test`, recreated after the migration edit.

| ID | Test (prefix) | Decisive assertion |
| --- | --- | --- |
| V1 | `channel_call_snapshot_record_is_immutable_and_replays` | Insert, then replay with equal digest returns the same row. Other digest: `invalid_input` and no second row. UPDATE and DELETE are refused. |
| V2 | `channel_call_snapshot_sql_refuses_forged_rows` | Raw inserts fail at commit: cross-company channel or binding; child not parented by the caller; child actor ≠ run_as; stale settings revision; manifest not of the child. |
| V3 | `channel_call_snapshot_concurrent_dispatch_converges` | Two tasks capture and record the same key; exactly one commits and one row exists. |
| V4 | `channel_call_snapshot_reconfiguration_selects_one_whole_revision` | Capture competes with `configure_reply_on` to another responder and with a params/resource edit. The record equals exactly one complete settings revision. Count = 1. |
| V5 | `channel_call_snapshot_revoked_or_unready_target_is_denied` | Run_as loses desk management, or a resource is revoked: `destination_unavailable`, with zero rows in record/child/manifest. |
| V6 | `channel_call_manifest_call_scope_requires_invocation` | A child row with `run_id = root_run_id` and no invocation fails at commit. With an invocation it commits, and its manifest root is the child. |
| V7 | `channel_call_manifest_child_cannot_take_sibling_or_foreign_scope` | Inheriting a sibling's or another tree's `(root, manifest)` fails. |
| V8 | `channel_call_manifest_pins_survive_publication` | Publish a new destination agent revision after capture; the child resolves the captured revision. |
| V9 | `channel_call_manifest_descendant_inherits_call_scope` | An ordinary child of the call child attaches to the call scope; the caller's scope does not leak. |
| V10 | `channel_call_target_facts_*` | Ancestry includes self and nested calls, but not finished sibling calls. Archived unit and no responder give the denied codes. |
| Regression | `autonomous_agent_storage_manifest_`, `autonomous_agent_children_`, `agent_preparation`, `workflow_tool_`, `channel_reply_binding_`, child-schema groups | Existing behavior unchanged. Run now (shared infrastructure, 02 B). |
| Gate | `cargo sqlx prepare --check`, offline `cargo check --tests`, `cargo fmt --check` | |
