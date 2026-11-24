# 08G-P03 reviewer findings

Owner: Reviewer B. Workflow `implement-opus-reviewed-2_4`. Phase `08G-P03`, worktree
`/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`. Entries are appended; resolved
entries are never deleted. Review-round notes: [agent-review-notes.md](agent-review-notes.md).

Common context for the entries dated 2026-11-24 below: batch B1 (03A + 03B), stage architecture gate
08G-P03-D1 decision revision r1, round note 08G-P03-D1-R1-N1, reviewer `claude-fable-5-2` (effort as assigned
by root: medium), review base = HEAD = `c7fbbbbe`, digest
`a078b8caf6589f4b68f104ff572ff864ad8b9d31698a1f1531abede9c8fa9a99`, task record [TASK.md](TASK.md) section
"Gate 08G-P03-D1 r1". All originate in the gated decision; no implementation exists.

## 08G-P03-D1-F1 — The insert guard refuses inherited goals after a kind change

- **Classification:** actionable defect, severity high, category correctness / architecture (D1.1 + D1.3 + D1.4).
- **Evidence:**
  - D1.1 lets a goal become standing while it is active, with no open-work condition, and keeps recorded
    `goal_unit_id`s.
  - D1.4's `GoalCandidate { unit, archived }` carries no kind. The rerun, parent and thread candidates are
    therefore returned whenever the unit is not archived.
  - D1.3's `BEFORE INSERT` guard on `workflow_runs` and `threads` requires the named unit to be *currently*
    `kind = 'goal'`.
  - So after goal G becomes standing: a live parent recorded against G that starts any child
    (`children.rs::insert_child_on`, every origin), a `channel.call` from such a run (the fresh thread in
    `channel_call_source::store_on` and the child), a later reply on a call thread that carries G, and a rerun
    of a run recorded against G all resolve to G and are refused by the trigger. The failure is a constraint
    error inside the parent's transition or the admission, not a worded refusal, and it repeats on retry.
  - D1.5's re-read under `FOR SHARE` re-reads `archived_at` only, so it does not catch this either.
- **Required:** decide what a candidate whose unit is no longer a goal means, and make resolver, lock re-read
  and guard agree. The plan text only says a kind change "does not move past work". Either the candidate counts
  as none like an archived one (then `GoalCandidate` carries the kind, and the guard stays), or inherited
  goals survive a kind change (then the guard cannot require the current kind for them). Add a `goal_of_`
  case and a `goal_trail_` case: kind flipped to standing while a parent is open, then a child, a call and a
  reply on the old call thread all succeed with the decided goal value.
- **Instruction lesson:** observation: a defense-in-depth guard was specified against the resolver's fresh
  input (desk chain) and not checked against its inherited inputs. Candidate: when a gate adds a DB guard
  below an application rule, list each producer of the guarded value and the state changes allowed between
  capture and write.
- **Resolution history:** 2026-11-24 open.
  - 2026-11-24 recheck (08G-P03-D1-R2-N1, decision r2, same digest): fixed in the decision. A non-goal or archived candidate counts as none; candidates are read `FOR SHARE`, which conflicts with a kind change and with archive; the guard takes the same lock and checks the same two facts. Tests `goal_of_non_goal_candidate_falls_through` and `goal_trail_kind_change_keeps_history_and_admits_new_work` are planned. Execution is unverified until the batch review.

## 08G-P03-D1-F2 — The production native writer is misidentified and loses the thread's goal

- **Classification:** actionable defect, severity medium, category requirements / contract impact (D1.4,
  Investigation).
- **Evidence:**
  - TASK names `autonomous_agent_storage_native_runs.rs:97` as the native run writer. That file is a test
    fixture (`pub(in crate::adapters::persistence::workflow::tests)`).
  - The production writer is `src/adapters/persistence/workflow/native_mcp/mod.rs:114` (`admit_on`). It is
    absent from the TASK inventory.
  - Its `owner.association` comes from `source_facts_on` (`native_mcp/mod.rs:39`): for a conversation it is
    that thread, and for `SessionAssociation::Workflow { run_id }` it copies the source run's channel,
    principal and thread. A native MCP run is therefore usually a run on an existing thread.
  - D1.4 gives a native run "chain only". 03B.2 says the thread's goal applies "for a run on an existing
    thread". A native call made on a Sales call thread that carries goal G, or from a workflow session whose
    run serves G, would record none. That breaks README decision 6 and would escape the goal cap in B2.
- **Required:** correct the inventory, and resolve a native run like any root on a thread (thread, then
  chain). State the disposition for `SessionAssociation::Workflow`: whether the source run's goal applies or
  the thread rule is enough. Add a `goal_trail_` case for a native run on a thread that carries a goal. Its
  archive behavior already follows: a native run is never cancellable (`archive.rs`, `work.native`).
- **Instruction lesson:** observation: the caller list was taken from a grep hit without checking the module's
  visibility. Candidate: a writer inventory in a gate names each caller's entry point and says whether it is
  production or fixture.
- **Resolution history:** 2026-11-24 open.
  - 2026-11-24 recheck (08G-P03-D1-R2-N1): fixed in the decision. `native_mcp/mod.rs::admit_on` is the writer; a Workflow session puts the source run's goal in the parent position, then thread, then chain; a Conversation session starts at the thread. `owner.source` carries the session association, so the writer has the run id. Tests planned for both variants.

## 08G-P03-D1-F3 — The cancellable classification has no decided computation and no positive test

- **Classification:** actionable defect, severity medium, category verification / architecture (D1.5, 08G-C002).
- **Evidence:**
  - `archive.rs::archive_department_on` computes `cancellable` before cancelling anything, from
    `work.native` and the bounded list of roots and outside-caller call children. `work.open` is only a count.
  - D1.5 adds goal runs elsewhere to the count and splits them in two: inside a tree whose root is on the
    subtree's desks (cancellable through the cascade), or under a root outside (not cancellable). The gate
    does not say how that split is computed, under what bound, or how it relates to
    `MAX_ARCHIVE_CANCELLATIONS`.
  - The verification plan tests only the negative case (`goal_trail_archive_counts_goal_runs_elsewhere`,
    `cancellable = false`). The main path of the feature is the positive one: a goal root on the goal desk
    with an open call child in Sales. That child is now counted before the cancel and again in the
    `remaining` recheck. No planned test shows `OpenWork::Refuse` reports `cancellable = true` for it, or that
    cancel-and-archive then succeeds and leaves no open run recorded against the goal.
- **Required:** state the bounded computation (for example by the run's root, or by an ancestry walk with a
  stated limit and its behavior at the limit). Add the positive test through the production archive write in
  both modes, next to the negative one, and keep `channel_call_resume_archive_cancels_call_child` and
  `unit_archive_` in the B1 commands as planned.
- **Instruction lesson:** cause unknown beyond the gate omitting the computation.
- **Resolution history:** 2026-11-24 open.
  - 2026-11-24 recheck (08G-P03-D1-R2-N1): fixed in the decision. Uncovered goal runs are those off the subtree's desks with no ancestor-or-self in the cancel list, by `parent_run_id` to depth 64 (the bound in `channel_call_source.rs:98`); past the bound a run counts as uncovered, which is the safe side. `cancel_tree_on` cascades through waiting `child_run` waits, which `write_on` writes for call and inherited children alike, so a covered run is reached; any difference between the two walks is caught by the post-cancel recount, which rolls back. Positive and negative tests planned.

## 08G-P03-D1-F4 — The rerun rule departs from the plan's archived-candidate rule without a reason

- **Classification:** actionable defect, severity low, category requirements (D1.4).
- **Evidence:**
  - 03B.2: "A candidate that is archived counts as none", and the function "returns the first that applies".
    03B.3: "A rerun takes the goal of the run it reruns."
  - D1.4: the rerun decides alone, and an archived original goal gives none without falling through. No
    reason is recorded.
  - Consequence: a rerun on a desk whose own chain has a live goal G2 records none when the original's goal
    was none or is archived (for example the original was a call child from archived G1 into G2's desk, or
    the desk's unit became a goal later). G2's own-desk run is then missing from its trail, and in B2 from
    its cap.
- **Required:** either fall through to thread and chain when the rerun candidate is archived (and decide the
  original-had-none case), or record the reason for stopping. Cover the chosen behavior in `goal_of_`.
- **Instruction lesson:** candidate: a gate that departs from plan wording states the departure and its reason
  beside the decision.
- **Resolution history:** 2026-11-24 open.
  - 2026-11-24 recheck (08G-P03-D1-R2-N1): fixed in the decision. The rerun original is the first candidate and falls through like the others; `goal_of_rerun_falls_through_when_original_goal_closed` planned.

## 08G-P03-D1-F5 — No planned test rejects a goal in another company

- **Classification:** actionable defect, severity low, category verification.
- **Evidence:** `src/adapters/persistence/AGENTS.md`, "Preserve invariants at the database boundary": a new
  composite foreign key gets "a rollback-only or transaction-scoped database test that demonstrates the
  invalid cross-tenant row is rejected". D1.3 adds two such keys. The D1 verification plan has no such case;
  `goal_trail_column_is_immutable_and_goal_only` covers the update and the standing/archived inserts only.
- **Required:** add the cross-tenant insert for `workflow_runs` and `threads` to that test.
- **Resolution history:** 2026-11-24 open.
  - 2026-11-24 recheck (08G-P03-D1-R2-N1): fixed in the plan. The cross-company insert for both tables is added to `goal_trail_column_is_immutable_and_goal_only`.

## 08G-P03-D1-S1 — Take the share lock inside the insert guard (optional)

- **Classification:** optional suggestion, category architecture (D1.3, D1.5).
- **Evidence:** D1.5 asks every writer to take `FOR SHARE` on the goal row and re-read before the insert. The
  guard trigger already reads that row. If the trigger takes the share lock itself (as
  `guard_workflow_role_context` takes the company lock), the protocol holds for a writer that forgets it, and
  the application read is only for the fall-through. Today every run writer already holds the company row
  `FOR NO KEY UPDATE` before its run rows (`refuse_archived_desk_on`, `waits::lock` with `Transition`,
  `lease::lock_scope`), as archive does, so the share lock is a second line and adds no lock-order edge.
  TASK's lock-order paragraph should say that this company-first order is what excludes a deadlock with
  cancel-and-archive, which locks run rows after department rows. The header of `children.rs` ("never company
  authority") reads otherwise and is stale.
- **Resolution history:** 2026-11-24 open (optional).
  - 2026-11-24 (08G-P03-D1-R2-N1): adopted in r2; the lock-order statement now names company-first ordering.

## 08G-P03-D1-S2 — A worded refusal for deleting a goal with recorded work (optional)

- **Classification:** optional suggestion, category correctness / UX (D1.3).
- **Evidence:** `guard_department_delete_on` (`department/lifecycle.rs`) preflights each retained reference with
  a worded `Conflict`; `department.rs` says writes are "preflighted against the rules the schema enforces".
  D1.3 relies on the generic "still referenced" mapping, which does not tell the person to archive instead
  (README proposal 5). `delete_department_on` also retires namespaces before the FK fails; the rollback
  covers it.
- **Resolution history:** 2026-11-24 open (optional).
  - 2026-11-24 (08G-P03-D1-R2-N1): adopted in r2.

## 08G-P03-D1-S3 — Cover a non-call child and the user-decision point (optional)

- **Classification:** optional suggestion, category verification / process.
- **Evidence:**
  - The `goal_trail_` plan exercises the call child only. `workflow.call`, `flow.repeat` rounds
    (`child_waits.rs::next_round`) and workflow tool calls (`workflow_tool_calls.rs`) all pass through
    `insert_child_on`; one inherited-child case would show the parent rule on that branch.
  - D1.5 decides that a goal run under an outside root is not cancellable from the goal and marks it as the
    user's to change. It is consistent with U3 and 08G-C002, but it means the goal's manager may be unable to
    archive until another department's run ends. Carry it to the handoff as an open question for the user.
- **Resolution history:** 2026-11-24 open (optional).
  - 2026-11-24 (08G-P03-D1-R2-N1): adopted in r2; the outside-root choice is carried to HANDOFF.

---

Common context for the entries below, dated 2026-11-24: batch B1 (03A + 03B), stage batch review, round note
08G-P03-B1-R1-N1, reviewer `claude-fable-5-2` (effort as assigned by root: medium), review base `c7fbbbbe`
(nothing committed since; the batch is the working tree), digest
`df55a3111d6577894597abbf0616b21036c593d695ff080367497f9da302f27b`, task record [TASK.md](TASK.md) section
"B1 implementation progress". These are implementation findings, not defects of gate D1 r2.

## 08G-P03-B1-F1 — The open-work refusal misdescribes a goal's runs elsewhere

- **Classification:** actionable defect, severity low, category correctness (user-facing text), 03B.5.
- **Evidence:**
  - `archive.rs::open_work_on` now counts runs recorded against a goal in the subtree wherever they run, and
    `uncovered_goal_run_on` makes the refusal non-cancellable.
  - `src/adapters/http/routes/ui_departments.rs:697-710` renders every non-cancellable refusal as
    "{count} run(s) still open here or below, more than can be cancelled at once. Let them finish or cancel
    some first." For one Sales run that carries the goal, both halves are false: the run is not here or
    below, and the reason is not the number.
  - `src/application/app_error.rs:29` (the JSON error text) says "unfinished run(s) on this department's
    desks".
  - `docs/departments.md` "Archiving a goal" describes the behavior correctly, so the product and its
    documentation disagree. B1 has no goal page or board filter yet (03D), so the person has no other pointer
    to where the run is.
- **Required:** word the refusal for the new case, in the form message and the error text: runs done for this
  goal in other departments are counted, and one that started outside the goal's own run trees is ended by
  its department. The route needs to tell the over-the-bound case from this one, or use wording true of both
  (and of the existing native-call case). Cover the text in a route or rendering test.
- **Instruction lesson:** observation: the count's meaning changed and its readers were not re-read. The
  contract-impact table in the verification-contract reference already asks for consumers of a changed value;
  the TASK account lists none for `UnitArchiveOpenWork`.
- **Resolution history:** 2026-11-24 open.
  - 2026-11-24 recheck (08G-P03-B1-R2-N1, digest `6caa7f19…4e2b`): fixed. `UnitArchiveOpenWork` carries `elsewhere`; `uncovered_goal_runs_on` returns the count and `cancellable` needs 0; the error text, the JSON body and `ui_departments::open_work_refusal` word the case. `goal_unit_archive_refusal_names_runs_in_other_departments` asserts the three wordings and forms, and `goal_trail_archive_counts_goal_runs_elsewhere` asserts `elsewhere: 1` through the production archive; both pass in `logs/b1-r2-focused-tests.txt`.

## 08G-P03-B1-F2 — Two promised checks are not delivered as described

- **Classification:** actionable defect, severity low, category verification.
- **Evidence:**
  - Gate D1 r2 (F2) promised `goal_trail_native_call_carries_the_thread_goal` "plus a Workflow-session case".
    The `SessionAssociation::Workflow` arm of `native_mcp/mod.rs::admit_on` is not executed by any test. TASK
    and the manifest record it honestly as NOT EXERCISED, but no disposition is agreed. No existing test
    builds a workflow session for a native call, so the cost is a fixture, not a line.
  - The focused check `goal_competing_` reads "an archive racing a child admission".
    `goal_competing_archive_and_child_admission` (`unit_archive_runs_tests.rs`) races the archive against a
    root admission on a thread that carries the goal. No child writer (`children::insert_child_on` through
    `park_child` or a call dispatch) is a claimant. TASK's gaps mention the unforced order only, and the
    mapping row does not say the claimant is a root.
  - By inspection both are sound: the arm passes the session's run as the parent to `resolve_on`, which reads
    it within the company; and every child writer takes the company row before its run rows
    (`waits::lock` with `Transition`, `lease::lock_scope`), as the root admission in the test does.
    Inspection does not fulfil a behavioral test obligation (verification-contract reference).
- **Required:** either add the two cases, or record each as a named gap with what the stand-in leaves unproved
  and get an explicit disposition from the user or root before acceptance. Rename or re-describe the race
  test so its name does not claim a child.
- **Resolution history:** 2026-11-24 open.
  - 2026-11-24 recheck (08G-P03-B1-R2-N1): fixed. `goal_trail_native_workflow_session_call_carries_the_source_run_goal` makes a real native call from a workflow session after the desk moved to a second goal, and asserts the session's source run and the first goal; only the Workflow arm gives that result. The race test is renamed `goal_competing_archive_and_thread_admission` and forces each order once before two released-together rounds. The disposition for a child claimant is recorded in the test and TASK: a child exists only under an open parent that the archive already counts or cancels, under the company lock. I accept it; it is root's disposition, by reasoning and not by a test. Both tests pass in the R2 log.

## 08G-P03-B1-S1 — Smaller points (optional)

- **Classification:** optional suggestions.
- **Evidence:**
  - `persistence/workflow/goals.rs::resolve_on`: `unit.id = (SELECT department FROM placed) AND (SELECT
    count(*) FROM placed) = 1` relies on the planner turning the count into a one-time filter so the
    two-row scalar subquery is never evaluated. It does today (checked with EXPLAIN and a two-row `placed` on
    the scratch database: no error, no rows). A `LIMIT 1` or an aggregate in the first subquery removes the
    dependence. No test places an agent in two units for a deskless run.
  - The create form takes one deciding sponsor; D1.2 named "Other sponsors" too. `docs/departments.md` says
    so, and the JSON create takes the list. TASK does not mark it as a departure from the gate.
  - `PATCH` from goal to standing needs `target_date: null` in the same request when a date is set
    (`department.target_date_goal_only`). The docs row says a standing department refuses a date but not
    this.
  - `guard_department_delete_on` says "Goal '…' has recorded work" for a unit that is standing now but has
    recorded goal history.
  - `docs/departments.md` "Archive" still says an archived department "can be emptied and deleted"; an
    archived goal with recorded work cannot. The "Goals" section says so, the "Archive" list does not.
- **Resolution history:** 2026-11-24 open (optional).
  - 2026-11-24 (08G-P03-B1-R2-N1): adopted. The chain query uses `IN`; the delete refusal is reworded; the docs cover PATCH to standing, Archive and Delete; the single-sponsor form is recorded as a departure from D1.2.

## 08G-P03-B1-S2 — "for this goal" when the archived unit is a standing ancestor (optional)

- **Context:** 2026-11-24, recheck 08G-P03-B1-R2, note 08G-P03-B1-R2-N1, digest `6caa7f19…4e2b`, reviewer
  `claude-fable-5-2`.
- **Classification:** optional suggestion, category wording.
- **Evidence:** `ui_departments::open_work_refusal` says "still open for this goal" whenever `elsewhere > 0`.
  Archiving a standing unit whose subtree holds the goal (a goals office) gets the same sentence, and
  `count` then also includes runs on the subtree's desks that serve no goal.
- **Resolution history:** 2026-11-24 open (optional).

---

Common context for the entries below, dated 2026-11-24: batch B2 (03C + 03D), stage architecture gate
08G-P03-D2 decision revision r1, round note 08G-P03-D2-R1-N1, reviewer `claude-fable-5-2` (effort as assigned
by root: medium), base = HEAD = `c7fbbbbe`, digest
`6caa7f19d63b49cfae39c91f042e35032ed05112664eba146469bd652d3b4e2b` (the accepted B1 tree; no B2 code), task
record [TASK.md](TASK.md) section "Gate 08G-P03-D2 r1". All originate in the gated decision.

## 08G-P03-D2-F1 — The unchanged chain query fails on a `total` cap

- **Classification:** actionable defect, severity high, category correctness (D2.1 + D2.3).
- **Evidence:**
  - D2.1 stores the goal's cap as a `unit_budgets` row with `period = 'total'`. D2.3 runs the goal check "after
    the unchanged chain query".
  - That query (`unit_budgets.rs:182-186`) joins every `unit_budgets` row of the chain's units and evaluates
    `date_trunc(budget.period, …)` in the join to `unit_spend`. `date_trunc('total', …)` raises `unit "total"
    not recognized` (checked on the scratch database).
  - The chain is the run's write unit and its ancestors. For any run on the goal's own desk, or on a desk
    below a goal, the chain contains the goal, so once the goal has a `total` cap every such admission and
    every `channel.call` into those desks would fail with a database error instead of being admitted or
    refused. If the planner happened to skip the expression, a `total` row would instead be compared with a
    missing `unit_spend` row and silently never refuse.
  - The D2 verification plan would not catch it: `goal_budget_total_cap_refuses_admission_and_names_the_goal`
    admits on a goal thread in another department, whose chain does not contain the goal.
- **Required:** the chain query reads `day` and `month` caps only; `total` is compared with the `goal_spend`
  sum and nowhere else. `states_on` must not pass `total` through `date_trunc` either. Add a test that admits
  on the goal's own desk with a `total` cap set: under the cap it admits, a `day` cap on the goal still
  refuses as a unit cap, and at the cap it refuses with `budget.goal_exhausted`.
- **Instruction lesson:** observation: a new value was added to a stored discriminator and its SQL readers
  were described as unchanged. The contract-impact table (verification-contract reference) asks for an
  exhaustive search of readers of a changed stored value; the gate lists none for `unit_budgets.period`.
- **Resolution history:** 2026-11-24 open.
  - 2026-11-24 recheck (08G-P03-D2-R2-N1, decision r2, same digest): fixed in the decision. The chain query reads `day` and `month` only; `states_on` keeps its `VALUES` list and takes the `total` row from a separate query; `report.rs::unit_budgets_on` drives periods from `VALUES` (confirmed in R1). `goal_budget_total_cap_leaves_desks_in_the_goal_admitting` admits on the goal's own desk and below it with a cap set. Execution unverified until the batch review.

## 08G-P03-D2-F2 — Refused calls leave no row, so the rollup cannot count them

- **Classification:** actionable defect, severity medium, category requirements (D2.4, 03D.1, 08G-C001 r2 (b)).
- **Evidence:**
  - 03D.1 asks for "calls made and their closures: `done`, `denied`, `handed_off`". D2.4 counts
    `workflow_channel_calls` joined to `workflow_channel_call_results`.
  - A destination refusal before launch (`ChannelCallOutcome::refused`: archived unit, disabled desk, no
    responder, unit cap, and D2.3's new goal cap) rolls back the savepoint and writes neither row; the result
    table's key references the call record. Only a procedure's own `channel.deny` is stored as `denied`.
  - Notice 08G-C001 r2 (b) says exactly this and leaves the choice to p03: the rollup "counts only closures of
    launched calls unless p03 adds a record". The gate neither adds a record nor states the limit.
  - The consequence is sharpest for 03C: when the goal's cap stops its dispatches, the goal's page and
    `goal.status` show no denied call at all.
- **Required:** decide it in the gate. Either record refused calls (a schema and writer decision that belongs
  here, with its retention and the savepoint in mind), or keep the count to launched calls and say so on the
  page, in the JSON field name or docs, and in the `goal.status` description, with a test that a
  budget-refused call is absent by design. Respond to the notice in coordination either way.
- **Resolution history:** 2026-11-24 open.
  - 2026-11-24 recheck (08G-P03-D2-R2-N1): fixed in the decision by recording refusals in `workflow_channel_call_refusals`. Checked against code: every pre-launch `denied` closure in production is built by `channel_call_target::denied`, called at the four `start_on` returns named (target decision, dead `run_as` account, destination admission, and the `create_call_on` refusal, which covers `children.rs:338` and `:365`); `channel_call_dispatch.rs:270` is a launched call's stored result, and `sample/tools.rs:220` is the scripted sample, which persists nothing. `ChannelCall::dispatch` commits the transaction before it returns an `Answered` observation, so the caller is told `denied` only after the row committed; the fourth site writes after the savepoint rolled back, in the outer transaction. The reverse does not hold exactly: a row can commit and the answer be lost, after which the call is judged again; the upsert and the launched-key exclusion cover a later refusal or launch, and a later input error leaves one stale row. The coordination response to 08G-C001 (b) is still to be written.

## 08G-P03-D2-F3 — The board's goal filter lists every goal in the company

- **Classification:** actionable defect, severity medium, category authorization (D2.6 against D2.5).
- **Evidence:**
  - D2.6: "The dropdown lists the company's active goal units, as the department dropdown lists departments.
    The goal id is checked against the company's goals".
  - The department dropdown is not company-wide. It comes from `DepartmentUseCases::desk_departments`
    (`use_cases/department.rs:514`): everything for an owner or admin, and otherwise the departments the
    viewer may see plus the owners of desks they can read. `known_department` checks against that list.
  - D2.5 gives `NotFound` on a goal to anyone without desk view or management, and limits the goals list the
    same way. A company-wide dropdown names every goal to anyone who can open the board, and a company-wide
    id check tells a stranger whether an id is a goal.
- **Required:** the dropdown and the id check use the D2.5 set (or the viewer-scoped department list
  restricted to goals); an id outside it is refused as an unknown one is. Add the case to
  `goal_authorization_`: a stranger's board shows no option for the goal and a request with its id is refused.
- **Resolution history:** 2026-11-24 open.
  - 2026-11-24 recheck (08G-P03-D2-R2-N1): fixed in the decision. The dropdown and the id check use the D2.5 set; the stranger case is added to `goal_authorization_stranger_and_manager`.

## 08G-P03-D2-F4 — Reads outside the stated bound, and readers of the new period not accounted for

- **Classification:** actionable defect, severity low, category architecture / contract impact (D2.1, D2.4, D2.6).
- **Evidence:**
  - D2.6's goals list shows open, waiting and failed counts for up to 200 goals from "one grouped query". No
    run bound is given; a failed count is a count over each goal's whole history. 03D.1 requires the read to
    be bounded and to say when it was truncated; the list has neither.
  - D2.4 takes "first run" as the minimum `created_at` of the newest `limit` runs. When truncated that is not
    the first run, and the time from first run to last finish is wrong without saying so. The index makes the
    true first run one row.
  - `model_usage/report.rs:279-286` (the usage page's cap status) reads `unit_budgets` against a fixed
    `('day'), ('month')` list. A `total` cap will not appear there. The gate does not say whether that is
    intended. D2.1's `states_on` adds the row; this reader is not mentioned.
  - A unit that was a goal and is standing again keeps its recorded runs and `goal_spend`. D2.5 answers
    `NotFound` for a standing unit and the card shows only for a goal; D2.8 does not say what `goal.status`
    returns to an open run that still carries that unit. The history then has no reader.
- **Required:** state the bound and truncation for the list counts; take the first run from the index or mark
  the times as truncated; give a disposition for the usage report and for a formerly-goal unit (page, API and
  tool the same way).
- **Resolution history:** 2026-11-24 open.
  - 2026-11-24 recheck (08G-P03-D2-R2-N1): fixed in the decision. List counts are bounded to 500 runs per goal with a cut marker; the first run is read on its own; the usage page stays on day and month by recorded choice, with `total` on the goal's Budgets and status cards; a unit that is no longer a goal answers `NotFound` on page, API and list, and `goal.none` from the tool.

## 08G-P03-D2-S1 — Smaller points (optional)

- **Classification:** optional suggestions.
- **Evidence:**
  - D2.1 keeps "no `total` cap on a standing unit" at the database only from the `unit_budgets` side. A kind
    change with a cap present is refused in the application alone. A check in a `departments` update trigger
    would close the other direction (`src/adapters/persistence/AGENTS.md`, database boundary).
  - D2.3's reading of "admit at most what it allows" is the one Plan07 agreed
    (`plan/07-unit-resources/COORDINATION.md`, decision k: "an admission that orders after spend reached the
    cap is refused"). Cite it in the gate and in the handoff, since the plan sentence reads stronger.
  - The rollup shows reviewers' names and department names from other departments to anyone who may view the
    goal's desk, including everyone in the company for a public desk. The plan asks for "who it waits on" and
    names Sales in its example, so this is as designed; say it in the docs.
  - `goal.status` runs the full rollup read (up to 10,000 runs) on each call. A smaller limit for the tool, or
    a per-step call bound, would keep an agent loop cheap.
- **Resolution history:** 2026-11-24 open (optional).
  - 2026-11-24 (08G-P03-D2-R2-N1): the database check, the Plan07 citation and the docs note are adopted. The smaller tool limit is declined with a recorded reason (03D.4 asks for the page's numbers); accepted.
