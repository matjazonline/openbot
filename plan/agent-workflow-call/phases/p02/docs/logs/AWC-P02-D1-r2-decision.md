# AWC-P02-D1 r2 — architecture gate: invocation record, call-child identity, explicit dependency scope

Author: root/A (Claude Code `1e84a7af`, Opus 5.5/xhigh), 2026-11-22. Anchors are at `fe7791d3`
with p01's accepted working tree. Supersedes [r1](AWC-P02-D1-r1-decision.md), which is kept
unchanged. Answers review `AWC-P02-D1-R1-N1`, findings F1–F5 (dispositions at the end).
Governing authority: AWC-P01-D1 r3 (D1.1 order, D1.2 fact table, D1.4 levels), notices
AWC-C001 r4 and AWC-C002 r1.

What changed from r1:

- **D3's lock order** is rewritten. `run_as` joins the caller's account union (F1).
- **Child identity is decided in the existing child writer**, a placement moved from p03 A.1
  (D4). The tests can then use the production writer (F2).
- **The writable check runs in this transaction.** It reads the same grant rows as
  `agent_preparation`.
- **New notice AWC-C003** tells p03 what it owns (F3).
- **The precision fixes** of F4.

This gate covers both batches of [02](../../../../02-target-snapshots-and-dependency-scopes.md).
They are implemented and reviewed as one batch, AWC-P02-B1.

## Facts the decisions rest on (traced)

- **Saved calls have no row.** An agent's saved tool call lives in
  `task_harness_runs.checkpoint->'invocations'`, keyed by `invocation_id` (a `ToolCallId` uuid),
  and is validated by `agent_origin::saved_call_on` (`agent_origin.rs:164`). A workflow-tool
  child is found again through `workflow_waits.child_tool_call_id` (`workflow_tool_calls.rs:122`).
  An action's agent origin is recorded as `agent_manifest_id, agent_id, agent_revision_id,
  agent_binding_id`, with FKs to `run_dependency_manifests(company,run,manifest)` and
  `agent_manifest_revisions` (init ~12922).
- **The message responder snapshot.** `capture_channel_reply_context()` (init ~13089) locks
  `channels FOR SHARE` and copies `channel_reply_settings` plus the pinned revision's
  `run_as_user_id`.

  Desk settings have three writers:
  - `configure_reply_on` (`channel_reply.rs:134`) locks the actor, the reviewers and the new
    responder's `run_as` in `users … ORDER BY id FOR SHARE`, then authorizes (company lock),
    then takes `channels FOR UPDATE`.
  - `bootstrap_native_on` (`:67`) only inserts the first settings of a desk that has none, and
    holds `channels FOR SHARE`.
  - `agent.rs:683` clears principal-inbox settings, which calls never address.

  The settings guard requires the selected binding to be active, desk-level and at its current
  revision at selection. Binding revisions are immutable.
- **Manifest and unit-scope capture are admission-shaped only in their argument.**
  - `agent_manifest::capture_on` (`agent_manifest/mod.rs:55`) reads company, actor, binding,
    association and `proposed_run_id`. It sets `lock_timeout=1000ms` and
    `statement_timeout=5000ms` for the rest of the transaction and has a 5 s deadline.
  - `run_scopes::capture_on` (`run_scopes.rs:42`) reads company, association and
    `proposed_run_id`. Its personal-scope owner comes from the run row.
- **Scope root and budget root are welded by one FK.** `run_dependency_manifests(company_id,
  run_id, root_run_id)` references `workflow_run_budgets(company_id, run_id, root_run_id)`, the
  causal root. Every reader treats `ownership.root_run_id` as the run that owns the manifest,
  never the budget root. B confirmed this for every SQL and non-test Rust use (N1).
- **One child writer, and one creation guard.**
  - `children::create_on` / `insert_child_on` (`children.rs:232`, `:328`) is the only runtime
    child writer. It writes the run (parent actor, parent association, parent knowledge), its
    first job, admission, admitted event and the parent's `child_run` wait. It attaches the
    parent's scope (`attach_child_on`) and copies the parent's unit scopes.
  - The deferred guard `workflow_child_parent_wait` (init 1418) refuses a child without that
    wait and a matching admission.
  - No trigger on `workflow_runs` forces a child's actor, binding or desk to equal its parent's.
    The triggers are `workflow_parent_link`, `workflow_root_budget_link`, `workflow_runs_active_unit`,
    notification/native seals and wakeup/finish stamps (F5).
- **An agent's write right on a desk** is `Channel::participant_access` with the internal
  context of an agent principal: authorized when the desk is public or the agent holds a
  `participate` grant (`domain/entities/channel.rs:137`, `ingest/policy.rs:155`).
  `agent_preparation.rs:97-108` already reads an agent principal's grants on a desk
  `FOR SHARE` in persistence.
- **Account protocol.** One sorted account union is locked before company ownership. It is
  built from unlocked hints, with a post-lock equality check. A replacement account is never
  acquired after the company lock (`action_account_hints.rs:1`, `:365-430`).
  `user_lifecycle.rs:30-62` locks `users FOR UPDATE` before the company.

## D1 — The invocation record: `workflow_channel_calls`

One immutable row per saved channel call, written in the transaction that writes the call child:

| Column | Meaning |
| --- | --- |
| `company_id, run_id, execution_id, call_id` | **PK**. The calling execution and saved `ToolCallId`. |
| `arguments_digest` | `canonical::digest` of the saved call's canonical arguments (text, 64 hex). |
| `agent_manifest_id, agent_id, agent_revision_id, agent_binding_id` | The calling agent's origin, as `workflow_action_intents` records it. |
| `channel_id` | The destination desk. |
| `binding_id, binding_revision` | The responder selected by the settings revision. |
| `settings_revision, contract, approval_policy` | Copied from `channel_reply_settings`. |
| `run_as_user_id` | The pinned revision's `run_as` (U1), `REFERENCES users(id)`, as in `channel_reply_contexts`. Its key check takes `FOR KEY SHARE` on a row this transaction already holds `FOR SHARE` (D3 step 1), so it adds no new wait. |
| `child_run_id` | `UNIQUE (company_id, child_run_id)`. |
| `scope_manifest_id` | The child's own manifest (D2). |
| `created_at` | |

Constraints:

- FK `(company_id, run_id, execution_id) → workflow_executions(company_id, run_id, id) ON DELETE
  CASCADE`.
- FK `(company_id, run_id, agent_manifest_id) → run_dependency_manifests(company_id, run_id,
  manifest_id)` and FK `(company_id, agent_manifest_id, agent_id, agent_revision_id) →
  agent_manifest_revisions`.
- FK `(company_id, channel_id, binding_id) → workflow_bindings(company_id, channel_id, id)` and
  FK `(company_id, binding_id, binding_revision) → workflow_binding_revisions`. Both are
  DEFERRABLE, with no cascade.
- FK `(company_id, child_run_id) → workflow_runs(company_id, id)` DEFERRABLE.
- FK `(company_id, child_run_id, scope_manifest_id) → run_dependency_manifests(company_id, run_id,
  manifest_id)` DEFERRABLE.
- `contract` and `approval_policy` carry `channel_reply_contexts`' CHECKs.
- **Immutable.** A BEFORE UPDATE OR DELETE trigger refuses unless the calling run is gone, as in
  `guard_agent_manifest`.
- **Insert guard** `workflow_channel_call_snapshot`, a deferred constraint trigger checked at
  commit:
  1. The child's budget row has `parent_run_id = run_id` and the caller's `root_run_id`.
  2. The child's `actor_id = run_as_user_id =` the recorded revision's `run_as_user_id`.
  3. The child's `binding_id`, `binding_revision` and `channel_id` equal the recorded ones. Its
     `principal_id` and `thread_id` are NULL at insert (the inbox association), or p03's fresh
     destination thread.
  4. `channel_reply_settings` for `channel_id` has `revision = settings_revision` with equal
     binding, revision, contract and policy. The writer holds `channels FOR SHARE` until commit
     (D3), so `configure_reply_on` cannot interleave. The only writer that can interleave is
     `bootstrap_native_on`, which inserts only on an empty desk, and a call never captures an
     empty desk.
  5. The child's `delivery_policy` equals the caller's.
- No status column.

Replay: a PK lookup returns the record. Equal digest returns the same invocation; a different
digest is `channel_call.invalid_input`, with nothing written. A replay never re-reads settings.

The saved-call link is not a plain FK, because workflow-tool waits use the same
`workflow_waits` columns. p03's launch guard ties the record to the wait and to the child's
`ChildCause::ToolCall`. It needs either a trigger, or a discriminator column with a matching
unique key; p03 chooses (AWC-C003).

## D2 — Explicit dependency scope

Unchanged from r1:

- `run_dependency_manifests.root_run_id` is documented (`COMMENT ON COLUMN`) as the scope root.
- The FK to `workflow_run_budgets(company_id, run_id, root_run_id)` is replaced by `(company_id,
  run_id) → workflow_runs(company_id, id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED`.
- A deferred constraint trigger `run_dependency_scope` admits exactly three cases:
  - **(a) Root.** `run_id = root_run_id` and the budget row has no parent.
  - **(b) Call scope.** `run_id = root_run_id` and a `workflow_channel_calls` row has
    `child_run_id = run_id` and `scope_manifest_id = manifest_id`.
  - **(c) Inherited.** `run_id <> root_run_id` and the budget parent's own row has the same
    `(root_run_id, manifest_id)`.

  The only writers are `persist_on` (case a or b) and `attach_child_on` (case c). B confirmed
  both stay admitted (N1).
- Manifest tables and their guards are unchanged. A call-scope manifest's root is the child, so
  its role bindings and role context are the destination's.

## D3 — Target read and lock order

**Capture input.** `CaptureScope { company, actor, binding: &ConfiguredBinding, association,
scope_root: RunId }`. Both `agent_manifest::capture_on` and `run_scopes::capture_on` take it.
Admission passes `CaptureScope::admission(command)`, with today's values. The walk, bounds,
grant checks, bundle verification, `persist_on` and the unit-scope projection are shared. The
caller's manifest is never widened.

**Module** `adapters/persistence/workflow/channel_call_target.rs`, beside `channel_reply.rs`. Its
module doc records this lock order.

1. **Account union.** A new `Purpose::ChannelCall { desk }` in `action_account_hints`. Its
   unlocked source reads the desk's current settings and the pinned revision's `run_as`. The
   union is: caller actor, requester and `run_as`, locked sorted `users FOR SHARE`. Then
   `authorize_run_with_locks`, the company lock, as every action prelock does. After it, the
   existing post-lock recheck compares the source again. A changed source is `Conflict`, as for
   every other purpose.
2. **`lease::lock_fence`** (the `background_tasks` row of the job).
3. **Replay lookup** by PK. A hit returns before any destination lock.
4. **Desk facts**, read in this transaction:
   - `channels` (`access_mode`, unit, enabled) `FOR SHARE`;
   - the calling agent's principal `FOR SHARE`, and its `participate`/`view` grants on the desk
     `FOR SHARE`, as `agent_preparation.rs:97-108` reads them;
   - the unit's archived state, as `refuse_archived_desk_on`;
   - the ancestry walk up `workflow_run_budgets.parent_run_id` (no locks), collecting run and
     recorded-call `channel_id`s, bounded by tree depth;
   - settings, read under the share lock just taken. They are authoritative from here on.

   `writable` is evaluated by a domain predicate extracted from `Channel::participant_access`
   (`participant_authorized(access_mode, listed, is_owner)`). The SQL then never restates the
   rule. p01's `decide_target` turns the facts into an error, a denied closure, or proceed.
5. **Settings check.** If the locked settings' pinned `run_as` or revision differs from step 1's
   source, return `Conflict`. A new account is never locked after the company lock.
6. **Destination authority**:
   - `management::authorize_association_on(run_as, inbox(desk))`. It locks
     `companies FOR NO KEY UPDATE` (already held from step 1), `department_members`/`principals
     FOR SHARE` and `channels FOR SHARE` (already held). It takes no `users` lock and returns
     `run_as`'s level.
   - Restore the pinned revision with `binding_rows::read_binding_at`.
   - `resources::association_reach_on` + `check_readiness` (stage Admission).
   - `ChannelReplyContract::validate` against the output schema.

   Each failure is the denied closure `channel_call.destination_unavailable`.
7. **Manifest and unit-scope capture**, under capture's own 1 s lock and 5 s statement bounds.
8. **The child writer (D4)**, then `record_on`.

**Deadlocks.** All account rows are locked in step 1, sorted, before the company. No later step
locks `users`, and the `run_as_user_id` FK touches only an already share-locked row. Against
`configure_reply_on`: the call holds `channels FOR SHARE` and the configurer waits for its
`FOR UPDATE`. The configurer's `users FOR SHARE` does not conflict with the call's. Against an
account writer (`users FOR UPDATE` → company): the call either waits at step 1 before holding
the company, or holds the share lock that the writer waits on. There is no cycle.

**Mismatch handling.** There is no in-transaction retry. `Conflict` (step 1 recheck or step 5)
ends the dispatch as `call_workflow_tool`'s errors do. Nothing was written, so the saved call
re-dispatches. p03 owns and tests that handling (AWC-C003).

**Outer bound.** Capture takes up to 5 s, where `call_workflow_tool` is bounded at 2 s. p03's
`call_channel` uses an outer timeout of 10 s. The park re-checks `lock_expires_at`, so a lease
that expires inside the window rolls everything back.

## D4 — Call-child identity in the existing child writer (moved from p03 A.1)

`ChildStart` gains `identity: ChildIdentity<'a>`:

- **`Inherited`**: today's behavior. Parent actor, association and knowledge;
  `attach_child_on`; `run_scopes::copy_child_on`.
- **`ChannelCall(&CallChild)`**. `CallChild` carries the captured target, the `run_as` actor and
  level, the desk inbox association, the child ceiling (p01 `child_ceiling(caller, PUBLIC,
  run_as)`), and the caller origin for the record. `insert_child_on` then:
  1. writes the run with those values;
  2. calls `agent_manifest::capture_on` and `persist_on` with `scope_root = child`, which is
     case (b);
  3. calls `run_scopes::capture_on` for the destination desk (D1.2), not `copy_child_on`;
  4. calls `record_on`.

  The job, admission, admitted event and `child_run` wait are written as today, so
  `workflow_child_parent_wait` holds.

`ChildOrigin::Call { call, attempt }` stays the cause. Static, round and workflow-tool callers
pass `Inherited`, with no behavior change. This moves 03 A.1 into p02, because p02's DB proof
needs the production writer (F2), and 02 B.3 already changes attachment inside it.

**p03 keeps** (notice AWC-C003):

- the port `call_channel(fence, call)`, its dispatch transaction, outer bound and
  `Conflict`/timeout handling;
- the fresh destination thread and canonical source message (03 A.2–A.3), and the launch guard
  tying the record to the wait and cause;
- the parent park, the deadline (03 A.5) and the cycle re-check at launch;
- replay through real dispatch, resume, cancellation and liveness (03 B, D1.5).

**Pre-launch refusal (02 A.6).** Errors and denied closures return before D4. No child,
manifest, record, thread, message, job or obligation exists. A denied closure has no record,
and a replay re-decides it from current facts.

## Invariants

1. One invocation per (company, execution, call id), bound to its arguments by digest and
   immutable while the caller exists.
2. A fresh scope exists only with an invocation naming that child and manifest.
3. A call child's actor, binding and revision, desk, delivery policy and budget lineage are the
   recorded ones, proven at commit.
4. Recorded settings were current under the writer's own share lock.
5. Every account the transaction locks is in one sorted union taken before the company lock.
6. Root and ordinary-child behavior is unchanged.

## Failure modes

| Failure | Result |
| --- | --- |
| Lost response, then retry | PK replay returns the same invocation and child. |
| Same call id, other arguments | `channel_call.invalid_input`; nothing written. |
| Responder or `run_as` replaced mid-dispatch | The share lock blocks the write, or step 1/5 detects it: `Conflict`, nothing written, re-dispatch. |
| Params or resources edited | A new revision needs a settings write (blocked). A revoked resource fails readiness: `destination_unavailable`. |
| Account write on `run_as` concurrently | Serialized at step 1. No deadlock. |
| Capture timeout | `AppError::Timeout`, and the transaction rolls back. |
| Forged SQL rows | Refused by the named FK or trigger (verification plan). |

## Consumers (02 B.4)

Unchanged from r1, with two changes:

- `run_scopes` is now covered here (D4).
- History selection (`agent_history_sessions::capture`) for a call child stays with p03. The
  child writes no history session in p02, as ordinary children do not.

## Verification plan

Fresh schema `openbot_awc_mac02_test`, recreated after the migration edit.

**Fixture** (`channel_call_test_support.rs`):
- the existing workflow-tool parent fixture (`workflow_tool_support` `ToolSetup`): a claimed
  agent execution with a saved tool call and a held fence;
- a destination desk, its responder binding with `run_as`, and settings selecting it;
- one production composition: steps 1–8 through `children::create_on(ChildIdentity::ChannelCall)`;
- the parent park, as `workflow_tool_calls::park_on` does.

The positive control commits. Each negative case changes one fact and asserts the refusing
constraint by name: SQLSTATE plus `CONSTRAINT` name or FK name, with
`SET CONSTRAINTS <name> IMMEDIATE` where deferred.

| ID | Test | Decisive assertion |
| --- | --- | --- |
| V0 | `channel_call_snapshot_positive_control_commits` | The composed write commits. The child's actor is `run_as`, its association the desk, its manifest root the child, its unit scopes the desk's; the record matches the settings. |
| V1 | `channel_call_snapshot_record_is_immutable_and_replays` | Equal digest returns the same row. Other digest: `invalid_input`, one row. UPDATE/DELETE refused by `workflow_channel_call_immutable`. |
| V2 | `channel_call_snapshot_sql_refuses_forged_rows` | Separately: foreign company channel or binding (FK names); child not parented by the caller, child actor ≠ run_as, stale settings revision, other binding revision (each `workflow_channel_call_snapshot`); manifest not the child's (FK). |
| V3 | `channel_call_snapshot_duplicate_key_converges` | Two transactions write the same key; one commits, one row. Primary key only (fence convergence: NOT EXERCISED, p03). |
| V4 | `channel_call_snapshot_reconfiguration_selects_one_whole_revision` | Capture competes with `configure_reply_on` to another responder and another `run_as`. The result is the old revision whole, the new revision whole, or `Conflict` with zero rows. Never a mixture. |
| V5 | `channel_call_snapshot_competes_with_run_as_account_write` | Dispatch competes with a `users FOR UPDATE` → company account write on `run_as`. Both finish with no deadlock error. |
| V6 | `channel_call_snapshot_revoked_or_unready_target_is_denied` | `run_as` loses desk management, or a resource is revoked: `destination_unavailable`, with zero record, child or manifest rows. |
| V7 | `channel_call_manifest_call_scope_requires_invocation` | From V0's shape without the record: `run_dependency_scope` refuses. V0 is the positive. |
| V8 | `channel_call_manifest_child_cannot_take_sibling_or_foreign_scope` | A sibling's or a foreign tree's `(root, manifest)`: `run_dependency_scope` refuses. |
| V9 | `channel_call_manifest_pins_survive_publication` | Publish after capture; the child resolves the captured revision. |
| V10 | `channel_call_manifest_descendant_inherits_call_scope` | An ordinary child of the call child gets case (c) on the call scope; the caller's selections are not visible. |
| V11 | `channel_call_target_facts_*` | The write right (public; members-only with and without grant); ancestry with self and nested calls; archived unit; no responder (including an empty desk under bootstrap). |
| Regression | `autonomous_agent_storage_manifest_`, `autonomous_agent_children_`, `agent_preparation`, `workflow_tool_`, `workflow_child`, `channel_reply_binding_`, `run_scopes`/unit-scope groups, admission groups | Unchanged behavior. Run now (shared infrastructure). |
| Gate | `cargo sqlx prepare --check`, offline `cargo check --tests`, `cargo fmt --check` | |

**NOT EXERCISED in p02** (owned by p03, AWC-C003):
- convergence of two claimants under the job fence;
- replay through the real `call_channel` dispatch without re-reading settings;
- changed arguments on a real saved call through dispatch;
- `Conflict`/timeout handling in the agent runtime;
- the launch guard tying record, wait and cause.

## Finding dispositions (r1 → r2)

| Finding | Disposition |
| --- | --- |
| F1 | Fixed. `run_as` joins the step-1 account union through a new purpose with the existing recheck. Step 6's row locks are listed. The `run_as_user_id` FK is kept and argued safe. V5 is the competing test. |
| F2 | Fixed. D4 lets tests use the production child writer. Fixture rows are named. Each negative asserts its constraint by name. V0 is the positive control. |
| F3 | Fixed. Notice AWC-C003 targets p03; the NOT EXERCISED list is above. |
| F4 | Fixed: settings writers listed; `binding_revision` added to item 3; the wait link is not a plain FK; no in-transaction retry (`Conflict` instead); outer bound 10 s stated. |
| F5 | Answered in Facts: no actor/binding/desk equality guard exists. V0 is the commit-level positive control. |
