# 08G-P03 — Goal units and tracking

Current state (2026-11-24): investigation done. Gate 08G-P03-D1 r2 PASSED (Fable 5.2, 08G-P03-D1-R2-N1, at
`a078b8ca…9a99`). B1 implementation is in progress. Not accepted in phase; not integrated.

## Request and identity

- **Original request** (user, 2026-11-24, after agent-workflow-call was accepted and committed): "continue",
  following "implement agent-workflow-call first as its own plan, then return to 08 p03". README proposals 4 and
  5 were confirmed the same day ("commit learnings and confirm both, drop scratch dbs").
- **Requirements:**
  - `plan/08-goals/03-goal-units-and-tracking.md`;
  - `plan/08-goals/README.md` (decisions 1–7; proposals 1–5 now built or confirmed);
  - the continuation in `plan/08-goals/04-integration-and-final-verification.md`.
- **Phase:** p03, ID prefix `08G-P03`. Workflow: implement-opus-reviewed-2_4. Installed skill:
  `/Users/mac02/orca/workspaces/openbot/plan08_ph1/.claude/skills/implement-opus-reviewed-2_4`.
- **Record root:** `/Users/mac02/orca/workspaces/openbot/plan08_ph1/plan/08-goals/phases/p03/docs`.
- **Root/A:** Claude Code session `1e84a7af`, Opus 5.5 / `xhigh`, as confirmed and recorded for
  agent-workflow-call in this session.
- **B:** a fresh Fable / `medium` reviewer (`general-purpose`, no fork). Agent ID: see HANDOFF.
- **Worktree:** `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`.
- **Commits:**
  - Phase starting commit: `c7fbbbbe` (agent-workflow-call `358e662a` + learnings).
  - Pre-existing uncommitted changes at start: Plan08 README / doc 03 / COORDINATION r7 (proposal confirmations),
    and the agent-workflow-call p06 HANDOFF note.
- **Starting digest:** `0f798ffe2b01d1c9468329b1be2ada48ee9dd736163b6121fe99c01faf10e36f`. Exclusions:
  `plan/08-goals/phases/p03/docs`, `.trail/learnings`.
- **Resources:** scratch databases `openbot_p08p03_mac02` (SQLx/dev) and `openbot_p08p03_mac02_test` (tests),
  on localhost:5432. Every command sets `DATABASE_URL` explicitly; `ALLOW_MISSING_DATABASE_URL` is never used.

## Coordination

- **Authoritative file:** `/Users/mac02/orca/workspaces/openbot/plan08_ph1/plan/08-goals/COORDINATION.md`. Writer:
  this session. Revision 8 (p03 registered; 08G-C001 satisfied, 08G-C002 acknowledged).
- **08G-C001 r2** (targets p03):
  - p03 does not start until agent-workflow-call is accepted through its p06.
  - Satisfied: p06 was accepted in phase at `bc8b9841…764f` (Fable PASS AWC-P06-B-R2-N1) and committed as
    `358e662a`.
  - Its completion record lists unavailable gates and one intermittent gap. These do not block use of
    `channel.call`.
- **08G-C002 r1** (targets p03; from agent-workflow-call p03):
  - Cancel-and-archive also cancels recorded call children on the subtree's desks whose caller is outside.
  - A caller on the subtree is cancelled with its own tree, including its call children elsewhere.
  - p03 keeps `channel_call_resume_archive_cancels_call_child` and the `unit_archive_` suite green.
  - Acknowledged at gate D1 r1. D1.5 builds on it.
- **AWC-P01-D1 r3 consequences** (in 08G-C001): a call child runs the destination binding as configured, and the
  caller is only the requester. The goal trail rides on the child run and its thread and adds no model-settable
  parameter. This is consistent with 03B.4.

## Batches

- **B1 = 03A + 03B:** goal units and the trail. Gate 08G-P03-D1 covers the schema, the resolution and lock
  protocol, the archive count, and delete behavior.
- **B2 = 03C + 03D:** the goal budget, the rollup, pages, the API and `goal.status`. It gets its own gate,
  08G-P03-D2, once B1 is accepted, because the budget counters and the authorization of the rollup are costly
  to unwind.

## Investigation (2026-11-24)

The plan's line anchors have moved, so the current locations are given here.

- **`departments`** (`init_schema.sql:15755`):
  - It has `is_root` and `archived_at` (p02).
  - The company FK is deferred NO ACTION. `companies_delete_tree` (`:20493`) deletes channels, departments and
    principals in an AFTER DELETE statement trigger, after the ordinary tenant cascades. So runs and threads
    are already gone when departments go.
  - Department delete maps FK violations to `Conflict("…still referenced…")`
    (`department/lifecycle.rs::delete_error`).
- **Creation:** every production create goes through `create_department_with_id_on → insert_department_on`
  (`department/lifecycle.rs:31,88`). Callers:
  - the use case `create_department` (`use_cases/department.rs:262`), which requires managing the parent;
  - HTTP JSON `create_department_json`;
  - the UI form `ui_departments.rs::write`;
  - onboarding.

  The lead role is already assigned in that transaction through `assign_department_role_on`. The first sponsor
  holder automatically becomes the decider (`department/roles.rs:147`). `DepartmentWrite`
  (`ports/department.rs:27`) has about 25 test construction sites.
- **Runs:**
  - `admission_write::insert_run` (`:146`) is the only production insert into `workflow_runs`. Its callers:
    - external admission `create` (`:7`), which writes `rerun_of`;
    - runtime children, including call children (`children.rs:524`, with a parent link);
    - native runs (`autonomous_agent_storage_native_runs.rs:97`).
  - Admission captures run scopes after the insert (`run_scopes::capture_on`). It already resolves the run's
    local unit through `workflow_target_departments(company, desk, target)` and walks the chain to
    `MAX_DEPARTMENT_DEPTH` (`run_scopes.rs:89`).
  - The Plan07 unit-cap check runs last (`unit_budgets::check_admission_on`).
- **Threads:**
  - The call's fresh destination thread is created by `channel_call_source::store_on`, through the shared
    workflow message writer `create_thread_on` (`message_writer.rs:51`, `NewThread`).
  - `message.send` uses the same writer.
  - Ingress threads are created in `thread/inbound.rs:659` and `thread/mod.rs:340`.
- **Archive open work** (p02): `archive.rs::open_work_on` counts unfinished runs on the subtree's desks. The
  subtree is locked `FOR UPDATE OF department` in id order (`archive.rs:47`).

## Gate 08G-P03-D1 r1 (B1 decisions, for independent review)

### D1.1 Unit kind and target date (03A.1–3)

- **Schema:**
  - `departments` gains `kind text NOT NULL DEFAULT 'standing' CHECK (kind IN ('standing','goal'))` and
    `target_date date`.
  - New checks: `departments_goal_target CHECK (kind = 'goal' OR target_date IS NULL)` and
    `departments_root_standing CHECK (NOT (is_root AND kind = 'goal'))`.
- **Domain:** `DepartmentKind { Standing, Goal }` sits beside `DepartmentRole` in `domain/entities/department.rs`.
  `Department` gains `kind` and `target_date`.
- **Edit:** `DepartmentEdit` gains `kind: Option<DepartmentKind>` and `target_date: Option<Option<NaiveDate>>`.
  - The edit needs `managed_parent` (the existing edit authority, "whoever manages its parent").
  - An archived unit already refuses configuration (p02).
  - Normalization:
    - a resulting standing unit with a target date is refused with `department.target_date_goal_only`;
    - the form clears the date when standing is chosen.
  - A kind change updates only the unit row. Recorded `goal_unit_id`s keep pointing at it ("rewrites no
    history").
  - In B2, D2 adds the rule for a `total` cap when a goal becomes standing.
- **Failure:**
  - The root cannot become a goal: `department.root_is_standing`, plus the DB check.
  - A target date on a standing unit is refused.

### D1.2 Creation takes kind, target date and sponsors (03A.4)

- `DepartmentWrite` gains `goal: Option<GoalWrite>`, where `GoalWrite { target_date: Option<NaiveDate>,
  sponsors: Vec<PrincipalId> }`. `None` means standing. Every test site gets `goal: None`.
- `insert_department_on` binds `kind` and `target_date`. In the same transaction, after the lead,
  `create_department_with_id_on` assigns each sponsor through `assign_department_role_on`, with `decider =
  (index == 0)`. The first sponsor decides; the role code would also make the first holder decider. A duplicate
  is refused up front.
- **Authority:**
  - Creating already requires managing the parent, and that manager manages the new unit's subtree, so the
    role assignments need no further check.
  - The principals pass the same eligibility as a role assignment (the shared assign path).
- **Surfaces:**
  - The UI create form gains Kind (Standing/Goal), Target date, Deciding sponsor and Other sponsors. The goal
    fields are only read when Kind is Goal.
  - The JSON create takes `kind`, `target_date` and `sponsors: [principal_id]`, in order, the first deciding.
  - Edit gains kind and target date (UI and JSON).
  - Onboarding stays standing.
- **Sample:** `src/application/workflow/fixtures/goal-plan-approve-dispatch.yaml`:
  - `agent.run` `role: lead` to plan → `decision.human` with reviewer role `sponsor` → `agent.run` `role: lead`
    with the `channel.call` capability → `reply`.
  - It is registered in the sample list. It is compiled by the all-fixtures compile test (learning "Compile
    shipped workflow samples in focused checks").
  - Binding it is the user's step: "published once and bound on each goal's desk" describes use, not automatic
    provisioning. A goal desk is provisioned like any department desk.

### D1.3 The trail columns and their guards (03B.1)

- **Columns:** `workflow_runs.goal_unit_id uuid` and `threads.goal_unit_id uuid`, each with `FOREIGN KEY
  (company_id, goal_unit_id) REFERENCES departments(company_id, id) ON DELETE RESTRICT`.
- **Deleting a goal:**
  - A direct delete of a goal with recorded runs or threads fails on the FK, mapped to the existing `Conflict`
    ("still referenced").
  - Company teardown is unaffected, because `companies_delete_tree` deletes departments after the cascades have
    removed runs and threads. This is verified by an existing teardown test plus a new goal-trail teardown
    assertion.
- **Index:** `workflow_runs_goal_idx ON workflow_runs (company_id, goal_unit_id) WHERE goal_unit_id IS NOT
  NULL`, for the archive count, the B2 rollup and the board filter.
- **Immutability:**
  - A `BEFORE UPDATE OF goal_unit_id` trigger on each table refuses any change: "a run's goal is fixed at
    insert", and "a thread's goal is set at creation". There is no exception, so a kind change cannot move past
    work.
- **Insert guard:**
  - A `BEFORE INSERT` trigger on each table requires that a non-null `goal_unit_id` names a unit that is
    currently `kind = 'goal'` and not archived.
  - This is defense in depth below `goal_of`. The model and transports never reach these columns, but a
    future writer that bypasses the resolver would otherwise record a standing or closed unit.

### D1.4 `goal_of` and where it runs (03B.2–4)

- **Pure function** `application/workflow/goals.rs`:
  `goal_of(rerun: Option<GoalCandidate>, parent: Option<GoalCandidate>, thread: Option<GoalCandidate>,
  chain: &[ChainUnit]) -> Option<DepartmentId>`.
  - `GoalCandidate { unit, archived }`; `ChainUnit { id, kind, archived }`, ordered from the desk's own unit
    upward.
  - Order:
    1. **rerun:** when this is a rerun, the original run's goal decides alone, even when it is none
       ("takes the goal of the run it reruns");
    2. the parent run's goal;
    3. the thread's goal;
    4. the nearest `Goal` unit in the chain.
  - A candidate that is archived counts as none, and resolution continues with the next source. Exception: a
    rerun with an archived original goal gets none.
  - The chain is cut at `MAX_DEPARTMENT_DEPTH`. Units past the bound are ignored, the same bound the scope
    chain uses.
- **Inputs:** `goals::inputs_on(db, company, desk, target, parent, thread, rerun_of)` in persistence (new
  `workflow/goals.rs`). It loads:
  - the rerun original's, the parent's and the thread's goal, each with its unit's archived state;
  - the chain from `workflow_target_departments` with `kind` and `archived_at`. This is the same placement
    query and bound as `run_scopes::chain_on`.
- **Writers:** `insert_run` gains `RunRow.goal: Option<DepartmentId>`. Each caller resolves it:
  - **admission** `create`: rerun, thread (`association.thread_id`) and chain;
  - **runtime child, including call children:** parent first, then the child's own thread and chain;
  - **native run:** chain only.
- **Call thread:** `NewThread` gains `goal_unit_id: Option<Uuid>`.
  - `channel_call_source::store_on` sets it to the calling run's `goal_unit_id`, read from the caller's run
    row. The column is immutable (D1.3), so the read needs no lock; D1.5's FOR SHARE rule still applies before
    the write.
  - `message.send` and ingress set `None`.
  - So a later reply on a call thread carries the goal through the thread. A fresh ingress thread carries
    none, and its run's goal comes from the desk chain.
- **The model cannot set it:**
  - No tool or `channel.call` parameter carries a goal.
  - `EmailRelayTrace` and mail headers are not read.
  - The insert guard (D1.3) refuses non-goal units.

### D1.5 Archive and concurrency (03B.5, `goal_competing_` archive half)

- **Open-work count:** `open_work_on` also counts unfinished runs whose `goal_unit_id` is in the subtree's
  units, wherever they run. A run on a subtree desk is counted once.
- **Cancel-and-archive** (p02 U3, extended by 08G-C002): it cancels the trees of open roots on the subtree's
  desks. Their descendants, including call children in other departments, go with those trees. It also cancels
  recorded call children on the subtree's desks.
  - A goal run elsewhere that is inside one of those trees is therefore cancellable.
  - A goal run elsewhere whose tree root is outside the subtree is not. An example is a Sales root admitted
    later on a call thread that carries the goal. Such a run makes `cancellable = false`, and the refusal
    reports the full count.
  - Reason: U3 authorized the parent's manager over the subtree's work, not over a Sales root. *(If the user
    prefers that such runs be cancellable from the goal, this is the place to change it.)*
- **Lock protocol (invariant: an archived goal never has an open run recorded against it):**
  - Before recording a non-null goal, every run and thread writer takes `SELECT … FROM departments WHERE id =
    goal FOR SHARE` and re-reads `archived_at`. If the unit is now archived, the candidate counts as none
    (D1.4), and resolution continues.
  - Archive takes `FOR UPDATE` on the subtree (p02) and then counts. `FOR SHARE` conflicts with `FOR UPDATE`,
    so:
    - if a writer holds the share first, archive waits and counts the new run (refused archive);
    - if archive commits first, the writer re-reads archived and records no goal.
  - A root run on a goal's own desk is refused by p02's archived-unit admission rule instead ("refused
    child").
  - Lock order: the writer's share lock is taken after the locks admission already holds (company, desk gate,
    run rows), on a single department row. Archive takes department rows only after its own management
    checks. This is verified by a competing-claimant test.
- **Later replies:** a reply on an archived goal's thread is admitted by its department, with goal none
  (D1.4).

### D1 verification plan (B1)

| Criterion | Test (planned name) | Decisive assertion |
| --- | --- | --- |
| 03A.1–3 | `goal_unit_kind_and_target_date_round_trip`, `goal_unit_target_date_on_standing_is_refused`, `goal_unit_root_cannot_be_goal`, `goal_unit_kind_change_keeps_recorded_goals` | DB row and domain values; the refusal codes; `goal_unit_id` unchanged after the kind flips |
| 03A.4 | `goal_unit_create_assigns_sponsors_in_one_transaction`, plus a JSON and a form route test | sponsor rows with exactly one decider (the first); a failed assignment rolls back the unit |
| 03A.5 | the existing all-samples compile test, plus `goal_unit_sample_procedure_compiles` | the sample compiles with `decision.human` sponsor and the `channel.call` capability |
| 03B.2 | `goal_of_*` unit tests | each branch: rerun alone; parent over thread and desk; thread over desk; nested goals pick the nearest; an archived candidate falls through; depth bound |
| 03B.1, 03B.3–4 | `goal_trail_*` on the scripted `Deployment` call harness | the call child and its thread carry the caller's goal; a later reply carries it; after archive, the reply is admitted with none; a rerun keeps it; an unrelated desk has none; deleting a goal with runs gives Conflict; company teardown still succeeds |
| D1.3 guards | `goal_trail_column_is_immutable_and_goal_only` | UPDATE refused on both tables; an INSERT naming a standing or archived unit is refused |
| 03B.5 | `goal_trail_archive_counts_goal_runs_elsewhere` | the count includes the Sales run; `cancellable = false` |
| D1.5 race | `goal_competing_archive_and_child_admission` | two transactions, both orders: either the archive is refused, or the run records none (never an archived goal with an open run) |

**Focused commands (B1):** `cargo test --offline --locked --lib -- goal_ department_ channel_call_ unit_archive
run_scope admission workflow_sample`. Also clippy 1.95, fmt, the stack budget on `goal_`, SQLx prepare check on a
fresh database, and the affected browser checks if forms change.

## Gate 08G-P03-D1 r2 — corrections to r1 (2026-11-24)

R1 returned changes required (Fable 5.2, note 08G-P03-D1-R1-N1). r2 changes only what is listed here; the rest of r1
stands.

- **F1 (high), fixed: one rule for a goal that is no longer a goal.**
  - A candidate counts as none when its unit is archived **or no longer `kind = 'goal'`**. `GoalCandidate` and
    `ChainUnit` both carry `kind` and `archived`.
  - Resolution reads each candidate unit `FOR SHARE` before deciding. A kind change (`UPDATE departments`,
    `FOR NO KEY UPDATE`) and archive (`FOR UPDATE`) both conflict with that lock. So the resolver's answer,
    and the insert guard that re-checks the same two facts, can never disagree inside one transaction.
  - A kind change therefore never fails a child, call, reply or rerun. New work after the change records
    none (or a live goal further up), and past work keeps its `goal_unit_id`.
  - Tests:
    - `goal_of_non_goal_candidate_falls_through`;
    - `goal_trail_kind_change_keeps_history_and_admits_new_work`, where a goal with an open parent becomes
      standing, then a child and a call are admitted with goal none (or the next live goal), and the
      recorded rows are unchanged.
- **F2 (medium), fixed: the native writer.** The production writer is `native_mcp/mod.rs::admit_on`; the earlier
  inventory named a test fixture.
  - Its candidates:
    - for `SessionAssociation::Workflow { run_id }`, the source run's goal, in the parent position ("on whose
      behalf");
    - then the association's thread goal;
    - then the desk chain.
  - A `Conversation` session has no source run, so it starts at the thread.
  - Test: `goal_trail_native_call_carries_the_thread_goal` (a conversation on a call thread), plus a
    Workflow-session case.
- **F3 (medium), fixed: how `cancellable` is computed.**
  - `archive_department_on` already lists the runs it would cancel: subtree-desk roots, plus 08G-C002 call
    children whose caller is elsewhere.
  - It additionally counts **uncovered goal runs**: unfinished runs with `goal_unit_id = ANY(units)`, not on a
    subtree desk, with no ancestor-or-self in that list. The ancestry walks `parent_run_id` in one recursive
    statement, bounded at depth 64 (the bound `enclosing_call_source_on` uses) and `LIMIT 1`.
  - `cancellable` is false when any uncovered run exists. The existing post-cancel recount stays the backstop:
    if anything remains, the refusal is non-cancellable and the transaction rolls back.
  - Tests:
    - positive: `goal_trail_archive_cancels_goal_tree_with_call_child_elsewhere`, where a goal root has an open
      call child in Sales; the refusal says `cancellable = true`, and cancel-and-archive cancels both and
      archives;
    - negative: `goal_trail_archive_counts_goal_runs_elsewhere`, where a Sales root carries the goal through a
      call thread; the refusal says `cancellable = false` with the full count.
  - The choice that such an outside root is not cancellable is carried to the user (HANDOFF).
- **F4 (low), fixed: rerun.** A rerun's original goal is the first candidate in the same list. When it is none,
  archived or no longer a goal, resolution falls through to the thread and the chain, like every other source
  ("returns the first that applies").
  - Order: rerun original, then parent (or native source run), then thread, then chain.
  - Test: `goal_of_rerun_falls_through_when_original_goal_closed`.
- **F5 (low), fixed:** `goal_trail_column_is_immutable_and_goal_only` also inserts a run and a thread naming
  another company's goal unit, and asserts the composite FK refuses both
  (`src/adapters/persistence/AGENTS.md`).
- **S1, adopted:**
  - The insert guard itself takes `FOR SHARE` on the named unit and re-checks kind and archived, so no writer
    can skip the lock.
  - Lock order: every run writer takes the company row `FOR NO KEY UPDATE` before run rows, and archive and kind
    changes take the company lock first too. Company-first ordering is what excludes a deadlock with
    cancel-and-archive. The department share lock is taken last, on one row.
- **S2, adopted:** `guard_department_delete_on` refuses a goal with recorded runs or threads with a worded
  `Conflict`: "This goal has recorded work and cannot be deleted. Archive it instead." The FK stays as the
  backstop.
- **S3, adopted:** `goal_trail_` adds an inherited non-call child (`workflow.call`) that carries its parent's goal.

**Gate D1 r2 PASSED** (2026-11-24): Fable 5.2, 08G-P03-D1-R2-N1, decision revision r2 at `a078b8ca…9a99`. No new
findings. Carried into the batch review: the uncovered-run check compares against the whole cancel set, not the
list truncated at `MAX_ARCHIVE_CANCELLATIONS + 1`.

## B1 implementation progress

Status (2026-11-24): implemented, focused checks recorded below; awaiting the batch review 08G-P03-B1-R1.

### What changed

- **Schema** (`migrations/20260817000000_init_schema.sql`): `departments.kind`/`target_date` with
  `departments_kind`, `departments_goal_target`, `departments_root_standing`. `workflow_runs.goal_unit_id` and
  `threads.goal_unit_id` with composite FKs `ON DELETE RESTRICT` and indexes. `guard_goal_unit_insert()`
  (`goal_unit_live`, takes `FOR SHARE` on the unit) and `guard_goal_unit_fixed()` (`goal_unit_fixed`) on both
  tables.
- **Domain and ports:** `DepartmentKind` beside `DepartmentRole`; `Department.kind`/`target_date`;
  `DepartmentWrite.goal: Option<GoalWrite { target_date, sponsors }>`; `DepartmentEdit.kind`/`target_date`.
  Duplicate sponsors are refused (`department.sponsor_duplicate`).
- **Persistence lifecycle** (`department/lifecycle.rs`): create assigns the lead, then sponsors (first decides),
  in the creation transaction. `unit_kind_shape` refuses a goal root and a standing target date on create and
  edit. `guard_department_delete_on` refuses deleting a goal with recorded runs or threads.
- **HTTP:** JSON create takes `kind`, `target_date`, `sponsors`; PATCH takes `kind`, `target_date`. The create
  form has kind, target date and a deciding-sponsor select; the edit form has kind and target date (not on the
  root).
- **Resolution:** pure `goal_of` (`src/application/workflow/goals.rs`): rerun, parent, thread, desk chain to
  `MAX_DEPARTMENT_DEPTH`; archived or non-goal counts as none. `persistence/workflow/goals.rs::resolve_on` loads
  the candidates and locks them `FOR SHARE ORDER BY id`. Writers: `admission_write::create` (rerun and
  association), `children::insert_child_on` (parent), `native_mcp::admit_on` (Workflow session's run as parent;
  else thread and desk), `channel_call_source` stamps the caller's goal on the fresh thread
  (`goals::for_thread_of_on`), `message_writer::create_thread_on` inserts it.
- **Archive** (`department/archive.rs`): `open_work_on` also counts runs with `goal_unit_id = ANY(units)`.
  `CANCEL_SET` is the subtree desks' roots plus call children whose caller is elsewhere; `uncovered_goal_run_on`
  walks each open goal run's ancestry (to depth 64) against the whole cancel set, without the
  `MAX_ARCHIVE_CANCELLATIONS + 1` truncation. `cancellable` also needs no uncovered goal run.
- **Sample:** `fixtures/goal-plan-approve-dispatch.yaml` (`Fixture::GoalPlanApproveDispatch`, in `ALL`).
- **Docs:** `docs/departments.md` "Goals" section, JSON API rows and error codes.

### Verification mapping (actual)

| Criterion | Test | Decisive assertion |
| --- | --- | --- |
| 03A.1–3 | `goal_unit_kind_and_target_date_round_trip`, `goal_unit_target_date_on_standing_is_refused`, `goal_unit_root_cannot_be_goal` | row and domain values after create and edit; `department.target_date_goal_only` and `department.root_is_standing` on create and edit, and the CHECK constraints on raw SQL |
| 03A.3 history | `goal_trail_kind_change_keeps_history_and_admits_new_work` | after the goal flips to standing the recorded run keeps its goal and a new run records none |
| 03A.4 | `goal_unit_create_assigns_sponsors_in_one_transaction` | sponsor rows with only the first deciding; a refused sponsor leaves no unit behind |
| 03A.4 routes | `goal_unit_json_create_and_edit_carry_kind_target_date_and_sponsors`, `goal_unit_form_creates_a_goal_and_edits_it_back_to_standing`, `goal_unit_create_form_offers_kind_target_date_and_sponsor` | JSON and form paths reach the same write; the form renders the fields and the root has none |
| 03A.5 | `goal_unit_sample_procedure_compiles`, plus `workflow_storage_restores_all_frozen_fixtures_and_child_identity` over `ALL` | `plan`/`dispatch` reference `lead` (Tools), `approve` references `sponsor` (People); no pinned agent |
| 03B.2 | `goal_of_*` (9) | each branch and fall-through, depth bound |
| 03B.1, 03B.3 | `goal_trail_call_child_thread_reply_and_rerun_carry_the_goal` | call child and its thread carry the caller's goal; a reply on that thread carries it; a rerun keeps it; an unrelated run records none; once the goal is marked archived (raw UPDATE), a later reply records none |
| 03B.3 inherited | `goal_trail_inherited_child_carries_the_parent_goal` | a `workflow.call` child records its parent's goal |
| 03B.3 native | `goal_trail_native_call_carries_the_thread_goal` | a local-agent call records its desk's goal; when its conversation's thread carries another goal, the thread's goal wins |
| D1.3 guards | `goal_trail_column_is_immutable_and_goal_only` | UPDATE refused (`goal_unit_fixed`) on both tables; INSERT naming standing or archived unit refused (`goal_unit_live`); cross-company FK refused; deleting a goal with runs gives the worded Conflict; company teardown succeeds |
| 03B.5 | `goal_trail_archive_counts_goal_runs_elsewhere` | the count includes the run elsewhere; `cancellable = false` while it is uncovered |
| 03B.5 cancel | `goal_trail_archive_cancels_goal_tree_with_call_child_elsewhere` | cancel-and-archive cancels the goal root and its call child on another unit's desk; archive succeeds |
| D1.5 race | `goal_competing_archive_and_child_admission` | two real transactions released together, twice: either the archive is refused with count 1 and the run keeps the goal, or the archive wins and the run records none |

**Gaps.**
- The native Workflow-session branch (a local-agent call from a workflow run takes that run's goal) is not
  exercised. Only the thread branch is, and it uses a thread whose goal is stamped directly under
  `session_replication_role = replica`, because session inputs are immutable and the session fixture cannot
  open a goal thread through `channel.call`.
- `goal_competing_` releases both claimants together; which one wins depends on scheduling. The two runs do not
  force each order.

### B1 checks (2026-11-24)

Fixture: scratch databases `openbot_p08p03_mac02` (SQLx, recreated and migrated fresh for the prepare check) and
`openbot_p08p03_mac02_test`. Toolchain: local rustc 1.99.1; clippy on 1.95.0.

| Check | Command | Result | Log |
| --- | --- | --- | --- |
| fmt | `cargo fmt --check` | PASS | `logs/b1-fmt.txt` |
| clippy | `cargo +1.95.0 clippy --offline --locked --all-targets -- -D warnings -A clippy::redundant_field_names -A clippy::nonminimal_bool` (the two allows cover pre-existing lints in untouched files under 1.95) | PASS | `logs/b1-clippy.txt` |
| focused tests | `cargo test --offline --locked --lib -- goal_ department unit_archive channel_call_ run_scope admission workflow_sample fixture native_mcp thread` | FAIL: 1993 passed, 2 failed | `logs/b1-focused-tests.txt` |
| catalog and correction | `cargo test --offline --locked --lib -- legacy_runtime_schema reviewer_history_hint agent_history_metadata_capacity claim_sql_multiple_applicable goal_unit_sample schema` | PASS: 141 | `logs/b1-catalog-tests.txt` |
| stack budget | `bash scripts/stack-budget.sh --offline -- goal_` | PASS: 25 at 2 MiB | `logs/b1-stack-budget.txt` |
| SQLx | fresh DB, `cargo sqlx migrate run`, `SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets` | PASS | `logs/b1-sqlx-prepare.txt` |
| trail | `trail build` | PASS (exit 0) | none |

The two focused failures:
- **`workflow_action_reconciliation_claim_sql_multiple_applicable_candidates`** was a regression from B1, now
  fixed. `capture_catalog` pins the list of `workflow_runs` BEFORE UPDATE row triggers, and B1 adds
  `workflow_runs_goal_fixed`. The pinned list in `action_reconciliation_claim_sql_candidate_support.rs` now names
  it, and the test passes in the catalog run. Other tests that read `pg_trigger` check one named trigger, except
  `legacy_runtime_schema`, which also passes.
- **`doop_agent_current_preparation_reaches_ready_before_model_io`** is unrelated and pre-existing. It asserts
  the prepared `version` is the literal 7, while `PREPARED_AGENT_VERSION` is 9. AWC p06 recorded it the same way
  (`plan/agent-workflow-call/phases/p06/docs/TASK.md:267`). B1 changes neither the test nor
  `src/application/agent`/`src/application/workflow/agent.rs`.

Not run in B1: browser scripts. No department-form browser script exists under `scripts/tests`, so the forms are
covered by the page and route tests only. The full suite is not run here; it belongs to p04.

**Frozen for 08G-P03-B1-R1:** base `c7fbbbbe`, digest `df55a3111d6577894597abbf0616b21036c593d695ff080367497f9da302f27b`,
exclusions `plan/08-goals/phases/p03/docs`, `.trail/learnings` (`logs/08G-P03-B1-R1-snapshot.txt`). Evidence
`logs/08G-P03-B1-R1.evidence.json` (validator: 5 PASS, 1 FAIL, 2 NOT EXERCISED). Coordination r8 is unchanged from
the pin. The batch review has been requested from B.

### 08G-P03-B1-R1 corrections (2026-11-24)

B's review 08G-P03-B1-R1-N1 at `df55a311…f27b` did not pass, with two low findings and five suggestions.

- **F1 (low), fixed:** the open-work refusal now names a goal's runs in other departments.
  - `AppError::UnitArchiveOpenWork` gains `elsewhere`: the open runs that serve a goal in the subtree in other
    departments, which no cancelled tree reaches. `uncovered_goal_run_on` became `uncovered_goal_runs_on` and
    returns that count; `cancellable` needs it to be 0.
  - The error text says the runs are in other departments, which only those departments can end. JSON adds
    `"elsewhere"`.
  - The archive card's wording moved to `ui_departments::open_work_refusal`. When `elsewhere > 0` it reads
    "N run(s) still open for this goal, M in other departments that its own work did not start. Only those
    departments can end it/them; archive once they finish."
  - Tests:
    - `goal_unit_archive_refusal_names_runs_in_other_departments` covers all three wordings and forms.
    - `goal_trail_archive_counts_goal_runs_elsewhere` now asserts `elsewhere: 1` and the JSON response
      (`count`, `elsewhere`, `cancellable`, text without "on this department's desks").
  - Existing exact patterns gain `elsewhere: 0`.
  - Docs: the Goals and JSON API sections.
- **F2 (low), fixed:**
  - **Native Workflow-session branch:** `goal_trail_native_workflow_session_call_carries_the_source_run_goal`
    is in `local_agent_mcp_tests.rs`. `originating_at` is split into `configured` and
    `originate(s, association)`; existing callers pass `Company`. The test admits the source run on a public
    desk under goal G1 (the Company association has no desk, so no chain), then moves that desk to goal G2
    before a real local-agent call. It asserts the session is a Workflow session of that run, and that the
    call run records G1, not the desk's G2.
    - Mutation check: with the Workflow arm in `admit_on` returning `None`, the test fails (G2 recorded).
    - The first correction run (351 passed, 1 failed) failed this test at its fixture, because a Company
      association has no desk. Its log was overwritten by the rerun recorded below; no manifest relied on
      it.
  - **Race:** renamed `goal_competing_archive_and_thread_admission`, because it is a thread admission. It now
    forces each order once (admission then archive: refused with count 1 and the goal kept; archive then
    admission: archived, run records none) and then releases both together twice.
    - Disposition for the plan's "child admission" wording: a child is inserted only while its parent is
      open, and a goal-carrying parent is already counted, so the archive refuses or cancels that tree
      whichever commits first. The goal-specific race is a new root on a goal thread, which this test covers.
      A root on the goal's own desk is refused by p02's archived-unit rule.
- **S1, adopted:**
  - The chain query uses `unit.id IN (SELECT …)`, so it no longer depends on a scalar subquery's row count.
  - The delete refusal reads "Department '…' has work recorded for it as a goal and cannot be deleted.
    Archive it instead." It holds for a unit that is standing again.
  - Docs: PATCH from goal to standing sends `"target_date": null`; the Archive and Delete sections say a
    department with work recorded for it as a goal is never deleted.
  - **Departure from D1.2, recorded:** the create form names one deciding sponsor; more are added on the
    Sponsors card. The JSON create takes the whole list.

Mapping updates: the D1.5 race row is now `goal_competing_archive_and_thread_admission`, with forced orders.
Row 03B.3 native adds `goal_trail_native_workflow_session_call_carries_the_source_run_goal`. Row 03B.5 adds
`goal_unit_archive_refusal_names_runs_in_other_departments`. The native gap (Workflow-session branch not
exercised) is closed. The thread variant still stamps the thread's goal directly, as recorded.

**R2 checks (2026-11-24):**
- **fmt:** PASS (`logs/b1-r2-fmt.txt`).
- **clippy 1.95:** PASS (`logs/b1-r2-clippy.txt`).
- **Stack budget on `goal_`:** PASS, 27 at 2 MiB (`logs/b1-r2-stack-budget.txt`).
- **Broad focused run** (R1 filters plus `local_agent_mcp legacy_runtime_schema schema`): 2086 passed and 1
  failed (`logs/b1-r2-focused-tests.txt`).
  - The only failure is the pre-existing, unrelated `doop_agent_current_preparation_reaches_ready_before_model_io`
    (prepared `version` 9 vs literal 7), as in R1.
- **SQLx:** the corrections add no `query!` macros, so the R1 prepare check still applies.
- **Frozen for 08G-P03-B1-R2:** digest `6caa7f19d63b49cfae39c91f042e35032ed05112664eba146469bd652d3b4e2b`, same
  base and exclusions (`logs/08G-P03-B1-R2-snapshot.txt`).
- **Evidence:** `logs/08G-P03-B1-R2.evidence.json` (validator: 4 PASS, 1 FAIL, 1 NOT EXERCISED).
- **Coordination:** still r8.
- **Status:** recheck requested from B.

### B1 accepted in phase (2026-11-24)

- **Review:** B (Fable 5.2) passed at R2 (08G-P03-B1-R2-N1). Root recomputed digest
  `6caa7f19d63b49cfae39c91f042e35032ed05112664eba146469bd652d3b4e2b` and validated `logs/08G-P03-B1-R2.evidence.json`.
  Coordination is still r8.
- **Status:** accepted in phase. Not integrated and not committed.
- **Exact base for B2's delta:** `logs/08G-P03-B1-accepted.diff.txt` (`git diff c7fbbbbe` at acceptance) and
  `logs/08G-P03-B1-accepted-untracked.txt`. This follows B's process note.
- **Remaining verification gaps:**
  - The pre-existing version-7 test, which is unrelated.
  - No browser script covers the department form.
  - The native thread variant stamps the thread's goal directly.
  - The "child admission" race is accepted by reasoning, not by a test.
  - The full suite belongs to p04.
- **Optional S2, deferred to B2:** the open-work wording says "for this goal" even when a standing ancestor is
  archived and `elsewhere > 0`. B2 adds the goal pages and rollup and touches this wording anyway, so it is
  revisited there.

## Gate 08G-P03-D2 r1 (B2 decisions, for independent review)

B2 builds on accepted B1 (`6caa7f19…4e2b`). Requirements: `plan/08-goals/03-goal-units-and-tracking.md`, batches
03C and 03D, with focused checks `goal_budget_`, `goal_rollup_`, `goal_authorization_`, `goal_competing_`
(budget half) and rendering tests. README proposal 4 is confirmed: a goal's budget counts the work done for it
anywhere. Coordination r8 applies.

### D2.1 A goal's cap (03C.1)

- **Period:** `unit_budgets.period` gains `total`. The CHECK becomes `day`, `month` or `total`.
- **`total` only on a goal:**
  - A `BEFORE INSERT OR UPDATE` trigger on `unit_budgets` locks the unit `FOR SHARE`. For `total` it requires
    `kind = 'goal'`, else it raises 23514 with constraint `unit_budgets_total_goal_only`.
  - `unit_budgets::period()` accepts `total`. `set_on` refuses `total` on a standing unit with
    `budget.total_goal_only` before the database does.
  - Changing a goal to standing while it has a `total` cap is refused with `department.goal_budget_on_standing:
    clear the goal's total cap first`. It is checked in the department update under the unit's `FOR UPDATE`
    lock, so a concurrent `set_on` either sees the new kind or blocks the update.
  - Day and month caps keep their 07 meaning on any unit, goals included. They bound spend by the unit's own
    desks and below.
- **Who sets it:** whoever manages the unit, as in 07; an archived goal's caps are frozen, as in 02C.3.
  `states_on` adds a `total` row for a goal: the cap and the goal's counted spend.

### D2.2 The goal's spend counter (03C.1)

- **Recording:** `model_usage_calls` gains `goal_unit_id uuid` (a recorded id, no foreign key, like `unit_id`).
  `attribute_model_usage` (BEFORE INSERT) sets it from the run's `goal_unit_id`. That is the run's goal at call
  time, and it is fixed.
- **Counter:** a new table `goal_spend(company_id → companies ON DELETE CASCADE, goal_unit_id, unit_id,
  spent_micros, unpriced_tokens, updated_at, PRIMARY KEY (company_id, goal_unit_id, unit_id))`.
  - It has no key to departments. As with `unit_spend` (07 decision j), charging never locks a department; an
    FK check would take `FOR KEY SHARE` and meet archive's `FOR UPDATE`.
  - `unit_id` is the call's own unit (the unit that did the work), not its ancestors. The total is the sum of a
    goal's rows; there is at most one row per department.
- **Charging:**
  - `charge_unit_spend` (the deferred settlement trigger) upserts the goal row after the `unit_spend` rows,
    when `NEW.goal_unit_id` is set, with the same priced cost and saturation.
  - Lock order: `unit_spend` rows in `(unit, period, start)` order, then the one `goal_spend` row.
  - Concurrent settlements for one goal from different units touch different goal rows; from one unit they
    queue on one row.
- **Lifetime:** a goal with spend has recorded runs, so it cannot be deleted (B1). When a working unit is
  deleted, its goal rows are kept, like an ancestor's totals in 07, and the rollup shows the unit as deleted.
  Company deletion cascades.

### D2.3 The admission check (03C.2–3)

- **Where:** `unit_budgets::check_admission_on` runs at the two places 07 checks caps: root admission
  (`admission_write::create`) and a `channel.call` child (`children::write_call_on`; the destination pays).
- **Goal check:** after the unchanged chain query, if the run's `goal_unit_id` is set and that goal has a
  `total` cap, it sums `goal_spend.spent_micros` for the goal (bounded: one row per department). At or over
  the cap, it refuses with `AppError::BudgetExhausted { unit: goal, name, period: "total" }`.
  - `BudgetExhausted::code()` returns `budget.goal_exhausted` for `total` and `budget.unit_exhausted` otherwise.
  - Display: "The goal {name} has spent its budget; new work for it starts once its cap is raised".
  - When both a unit and the goal are exhausted, the unit is named (03C.2: the goal check comes after the
    chain check).
- **Not checked, as in 07:** inherited children (`workflow.call`) and native local-agent calls are not
  budget-checked. A run already admitted is not stopped.
- **Children:** `write_call_on` maps a `total` refusal to the new `DeniedCode::GoalBudgetExhausted`. Its code is
  `budget.goal_exhausted`; its reason is "The goal this work serves has used its spend cap". The caller gets a
  `denied` closure with that reason (03C.3). The existing `UnitBudgetExhausted` mapping is unchanged.
- **Locks:** the read happens under the company lock the writers already hold, as in 07 decision k.
  Settlement never takes the company lock. So an admission waiting on that lock while a crossing charge
  commits sees the charge and is refused.
- **The plan's "two admissions racing the last of a goal cap admit at most what it allows":** with 07's
  settled-spend semantics, an admission is allowed while settled spend is under the cap. Two admissions under
  the cap both admit; once a crossing charge commits, every later admission is refused. The competing test
  asserts exactly this: one admission waits on the company lock while the crossing goal charge from another
  unit commits, and it is refused. It does not assert reservation-style exclusion, which 07 does not have.

### D2.4 The rollup read (03D.1)

One persistence read, `goal_rollup_on(db, company, goal, limit) -> GoalRollup`. Production calls pass
`GOAL_ROLLUP_RUN_LIMIT = 10_000`; tests pass a small limit to reach truncation. It runs in one read-only
transaction.

- **Base set:** the goal's newest `limit + 1` runs,
  `WHERE company_id = $1 AND goal_unit_id = $2 ORDER BY created_at DESC, id DESC LIMIT $3`.
  - `truncated` means `limit + 1` rows came back; everything below counts the newest `limit`.
  - The B1 index becomes `workflow_runs_goal_idx (company_id, goal_unit_id, created_at DESC, id DESC)` so the
    read is an index range scan.
- **Runs by state and by working unit:**
  - Each base run's working unit is its write scope (`workflow_run_scopes`, `acting_principal_id IS NULL AND
    access = 'write'`), with the root as fallback, as in 07.
  - Grouped by unit and state (`queued`, `running`, `waiting`, `succeeded`, `failed`, `cancelled`). Open means
    `queued | running | waiting`.
- **Waiting on people:** open decisions on base runs (`submitted_at IS NULL`, wait state `waiting`) that have a
  person reviewer (`workflow_decision_reviewers` → `principals.kind = 'person'`). Each item carries:
  run id, working unit, the reviewers' display names, and age. The list holds the 50 oldest plus a total count.
- **Open escalations:** a count of base runs whose escalation case is open (`workflow_escalation_progress.state
  IN ('waiting','manual','owner_waiting')`), following `attention.rs`.
- **Calls:** `workflow_channel_calls` whose caller is a base run, left-joined to `workflow_channel_call_results`.
  Counted by closure (`done`, `denied`, `handed_off`, `no_reply`, `error`), with `open` when no result row
  exists yet.
- **Spend:** the `goal_spend` rows by working unit (deleted units shown as deleted), the total, and the
  `total` cap, if any. Spend is the counter, so it is not truncated.
- **Time:**
  - first run (min `created_at`) and last finish (max `finished_at`);
  - time waiting on people: the sum over person-reviewed decisions of `COALESCE(submitted_at, now) -
    created_at`;
  - last activity: the greatest of run created/finished and decision submitted.
- **Content:** the rollup carries counts, states, closures, spend, unit names, run ids, reviewer names and
  times. It carries no message, input, output, decision proposal or desk name. This is what 03D.5 calls "never
  content".

### D2.5 Who sees it (03D.5)

- **May see the rollup:**
  - anyone with `Channel::viewer_access` on a desk of the goal unit (`channels.department_id = goal`), the
    rule desks use for viewing;
  - anyone with a management role on the goal (`unit_management_role`: owner, root admin, or lead or deciding
    sponsor of the goal or an ancestor).
- **Everyone else gets `NotFound` ("Goal not found").** So does a standing unit or a unit of another company.
- **Run links** are plain links to today's run page, which keeps its own check (`viewer_access` plus
  management of the desk's unit). A goal member who cannot inspect a Sales desk sees Sales's counts and a link
  that answers `NotFound`.
- **Goals list:** only goals the viewer may see, by the same rule.

### D2.6 Pages and the board filter (03D.2)

- **Goals tab:** a new `goals` tab on the company page, routed at `/ui/companies/{c}/goals`.
  - It lists the active goals the viewer may see: name, lead, deciding sponsor, target date, open, waiting
    and failed counts, and spend against the `total` cap.
  - Archived goals sit behind an "Archived" toggle (`?archived=true`).
  - One grouped query over at most 200 goals.
- **A goal's page:** the existing department detail gets a "Goal status" card when the unit is a goal and the
  viewer may see the rollup. It shows a table by department (runs by state, spend) and the waiting list. Opening
  a goal from the Goals tab opens the department detail.
- **Run board:** list, board and waiting views gain a `goal_id` filter beside `department_id`.
  - `RunBoardScope.goal` adds a `goal_unit_id = $` predicate in `board_reads::push_scope` and `reads.rs`.
  - The dropdown lists the company's active goal units, as the department dropdown lists departments. The
    goal id is checked against the company's goals, as `known_department` does.
  - The board's existing per-desk visibility still applies.

### D2.7 JSON API (03D.3)

- `GET /api/companies/{c}/departments/{d}/rollup` returns the `GoalRollup` JSON that the page renders,
  under the D2.5 rule.
- No goals-list JSON route is added; the existing department list carries `kind`.

### D2.8 `goal.status` (03D.4)

- **Tool:** a new read-only built-in, `AgentBuiltinTool::GoalStatus` (`goal.status`).
  - It takes no arguments: input schema `{type: object, additionalProperties: false}`.
  - `ActionEffect::Read`, opt-in per agent in the author's catalog like the other built-ins.
  - It is wired through the touch points listed in the investigation (snapshot and contract, `ToolKind`,
    `AgentToolTarget`, exactness, context description, dispatch, reserved name, resource kind, catalog).
- **At call time it:**
  1. reads the calling run's `goal_unit_id`; with none, it fails with tool error `goal.none` ("This run serves
     no goal");
  2. requires the calling agent's principal to hold `lead` or `sponsor` on that goal (`department_roles`);
     otherwise it fails with `goal.not_authorized` ("Only the goal's lead or a sponsor can read its status").
     Decider or not, any sponsor holder counts;
  3. returns `goal_rollup_on` for that goal, the same numbers as the page and the API.
- **Scope:** a run cannot name another goal. An archived goal's status stays readable to a run that recorded
  it.

### D2.9 Deferred B1-S2

The open-work refusal for `elsewhere > 0` will say "for this department or a goal in it", so it is right
when a standing ancestor is archived.

### D2 verification plan (B2)

| Criterion | Test (planned name) | Decisive assertion |
| --- | --- | --- |
| 03C.1 | `goal_budget_total_cap_only_on_a_goal` | `total` on standing refused (app code and trigger constraint); goal→standing with a `total` cap refused; day/month unchanged |
| 03C.1 counter | `goal_budget_spend_counts_work_in_every_department` | settlements in three units for one goal add up in `goal_spend` per unit; `model_usage_calls.goal_unit_id` set from the run |
| 03C.2 | `goal_budget_total_cap_refuses_admission_and_names_the_goal` | root admission on a goal thread refused with `budget.goal_exhausted` naming the goal; a unit cap still refuses first when both apply; a run with no goal admits |
| 03C.3 | `goal_budget_refused_call_child_is_a_denied_closure` | `channel.call` from a goal run to another department after the goal cap is reached gives a `denied` closure with the goal reason |
| 03C race | `goal_competing_admission_waits_and_sees_the_crossing_goal_charge`, `goal_competing_concurrent_goal_settlements` | as 07 D8: refused once the crossing charge commits; parallel settlements give exact totals without deadlock |
| 03D.1 | `goal_rollup_counts_waits_calls_and_spend_match_the_rows` | three departments: states by unit, waiting list, escalations, closures, spend, times equal hand counts |
| 03D.1 limit | `goal_rollup_reports_truncation` | limit 2 with 3 runs: `truncated` and counts over the newest 2 |
| 03D.4 | `goal_rollup_status_tool_returns_the_rollup_and_refuses_without_a_goal` | lead's agent gets the same JSON as `goal_rollup_on`; no goal → `goal.none`; a non-holder → `goal.not_authorized` |
| 03D.5 | `goal_authorization_member_sees_counts_not_runs`, `goal_authorization_stranger_and_manager` | a desk viewer sees Sales counts and gets `NotFound` on the Sales run; a stranger gets `NotFound` on the rollup (page, JSON); a manager of the parent sees it |
| 03D.2 | `goal_rollup_goals_page_lists_visible_goals`, `goal_rollup_goal_card_renders`, `goal_rollup_board_filters_by_goal` | rendering; archived toggle; the board's `goal_id` filter limits runs |

**Focused commands (B2):** `cargo test --offline --locked --lib -- goal_ unit_budget department channel_call_
board attention`, clippy 1.95, fmt, the stack budget on `goal_`, the SQLx prepare check on a fresh database,
and the catalog pins (`legacy_runtime_schema schema claim_sql_multiple_applicable`) because triggers and columns
are added.

## Gate 08G-P03-D2 r2: corrections to r1 (2026-11-24)

B's review 08G-P03-D2-R1-N1 did not pass, with F1 (high), F2 and F3 (medium), F4 (low) and S1. r2 replaces the
named parts of r1; everything else in r1 stands.

- **F1 (high), fixed in design.** The 07 chain query and `states_on` call `date_trunc(budget.period, …)`, which
  errors on `total`.
  - D2.3 now says the chain query reads only `budget.period IN ('day','month')`.
  - `states_on` keeps `VALUES ('day'),('month')` for the dated rows and adds the `total` row from a separate
    query over `goal_spend`.
  - `model_usage/report.rs::unit_budgets_on` already drives its periods from `VALUES` (day and month), so it
    neither errors nor shows `total`. See F4 for that choice.
  - New test `goal_budget_total_cap_leaves_desks_in_the_goal_admitting`:
    - with a `total` cap set and not reached, a root admission on the goal's own desk and on a desk of a
      unit below the goal both admit, and a `channel.call` into the goal's desk launches;
    - once the cap is reached, the same root admission on the goal's desk (which carries the goal) is refused
      with `budget.goal_exhausted`.
- **F2 (medium), decided: record refused calls.** This is notice 08G-C001 r2 (b).
  - **New table `workflow_channel_call_refusals`:**
    - columns `company_id`, `run_id`, `execution_id`, `call_id`, `channel_id`, `code` (a `DeniedCode`
      string), `created_at`;
    - primary key `(company_id, run_id, execution_id, call_id)`;
    - foreign key to the caller run `ON DELETE CASCADE`, and a company cascade;
    - no key to channels, since a desk may be deleted later.
  - **Writing it:** `channel_call_target::start_on` writes it, in the caller's transaction, at every `denied(code)`
    return after the replay check (lines 226, 245, 270 and the `create_call_on` refusal at 319).
    - `ON CONFLICT DO UPDATE` sets the latest code and time. A call id is judged again until a call launches,
      because refusals are not replayed (AWC-C002 replays only launched calls).
    - Input errors (`INVALID_INPUT`, a missing desk) are not refusals and are not recorded.
    - The row commits or rolls back with the transaction that answers the call, so it exists exactly when the
      caller was told `denied`.
  - **Rollup:** calls by closure now also count refusals of base runs, by code, as `denied`, together with
    `denied` results of launched calls. A refusal whose key later launched (a `workflow_channel_calls` row with
    the same key) is not counted.
  - **Responses and tests:** COORDINATION gets a p03 response to 08G-C001 (b). The rollup test asserts a
    goal-cap refusal and an archived-destination refusal appear as `denied`.
- **F3 (medium), fixed.**
  - The board's goal dropdown and the `goal_id` check use the D2.5 set (goals the viewer may see), not the
    company's goals. A `goal_id` outside that set is treated as unknown, the way `known_department` treats one.
  - `goal_authorization_stranger_and_manager` adds a stranger case: the board neither offers nor accepts the
    goal.
- **F4 (low), fixed:**
  - **Goals-list counts:** per goal they count only the newest 500 runs (`LATERAL … LIMIT 501` over the goal
    index), and show "500+" when cut. The list is at most 200 goals.
  - **First run:** read on its own (`ORDER BY created_at ASC, id ASC LIMIT 1` on the goal index), so
    truncation never moves it.
  - **The usage page** stays on day and month unit caps, by choice. A goal's `total` cap and spend show on the
    goal's Budgets card (the `states_on` `total` row) and on its status card. The Budgets form gains a
    `total` field for goals only; `replace_unit_budgets` takes `total` and refuses it on a standing unit.
  - **A unit that is no longer a goal:**
    - the rollup page card, the JSON route and the goals list need `kind = 'goal'`; for a standing unit they
      answer "Goal not found" (`NotFound`);
    - `goal.status` on a run whose recorded goal is now standing answers `goal.none` ("This run's goal is no
      longer a goal");
    - the recorded rows stay and show again if the unit becomes a goal again;
    - an archived goal stays visible behind the archived toggle and on its page.
- **S1, adopted:**
  - **Database check on kind changes:** a `BEFORE UPDATE OF kind` trigger on `departments` raises 23514,
    constraint `departments_goal_budget`, when `NEW.kind = 'standing'` and a `total` cap exists. The update holds
    the row `FOR UPDATE` and the `unit_budgets` guard takes `FOR SHARE`, so the two serialize.
  - **Plan07's reading:** D2.3's reading of "admit at most what it allows" is Plan07's decision k
    (`plan/07-unit-resources/COORDINATION.md`), as B confirmed.
  - **Docs:** `docs/departments.md` says goal viewers see reviewer names and department names from other
    departments.
  - **Not adopted: a smaller run limit for `goal.status`.** 03D.4 asks for the same numbers as the page, so
    the tool uses the same limit. Each call is one model tool call, already bounded by the agent's tool-call
    and step limits.

**Gate D2 r2 PASSED** (2026-11-24): Fable 5.2, 08G-P03-D2-R2-N1, decision revision r2 at `6caa7f19…4e2b`. No new
findings. Carried into the B2 batch review:
- a COORDINATION response to 08G-C001 r2 (b) (posted r9);
- catalog pins updated for the new table and triggers;
- the refusal write at the `create_call_on` site sits in the outer transaction, after the child's savepoint
  rolled back;
- the stale-row case is accepted. If a refusal row commits and its answer is lost, the call is judged again; a
  later refusal or launch corrects the count, while a later input error would leave one stale `denied`. That
  needs a reused call id whose arguments changed into an invalid request, which the replay digest otherwise
  rejects. The rollup documents that refusal counts are what the caller was told, at least once.

## B2 implementation progress

Status (checkpoint, 2026-11-24). Done and compiling:
- **Schema:**
  - `total` period, `unit_budgets_total_goal_only`, `departments_goal_budget`;
  - `model_usage_calls.goal_unit_id` set in `attribute_model_usage`;
  - the `goal_spend` table and its charge in `charge_unit_spend`;
  - `workflow_runs_goal_idx` gains `created_at DESC, id DESC`;
  - the `workflow_channel_call_refusals` table.
- **Budget code:**
  - `unit_budgets.rs`: period `total`, the chain query limited to day and month, the goal check after it, a
    `total` row in `states_on`, and `set_on` refusing `budget.total_goal_only`;
  - `BudgetExhausted::is_goal`/`GOAL_CODE` and its wording;
  - `DeniedCode::GoalBudgetExhausted` and its mapping in `children.rs`;
  - `channel_call_target::refused_on` at the four refusal returns;
  - the lifecycle refusal `department.goal_budget_on_standing`;
  - `replace_unit_budgets(.., total: Option<Option<i64>>)`, with a goal-only `total_cap` field on the
    Budgets card.
- **Rollup:**
  - the types in `ports/department.rs`;
  - `persistence/department/goal_rollup.rs` (`goal_visible_on`, `goal_rollup_on`, `visible_goals_on`);
  - `DepartmentPersistence::goal_rollup` and `visible_goals`.

Next: use cases, the JSON route, the Goals tab and goal card, the board filter, `goal.status`, docs and tests.
