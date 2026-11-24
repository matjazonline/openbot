# 08G-P03 reviewer notes

Owner: Reviewer B. Entries are appended; earlier outcomes are preserved.
Findings: [agent-review-findings.md](agent-review-findings.md).

## 08G-P03-D1-R1-N1 — architecture gate 08G-P03-D1 r1 (2026-11-24)

**Identity**
- Workflow `implement-opus-reviewed-2_4`; batch B1 (03A + 03B); stage: architecture gate, decision revision
  D1 r1 (D1.1–D1.5). Task record: [TASK.md](TASK.md), "Gate 08G-P03-D1 r1". Review index: [REVIEW.md](REVIEW.md).
- Reviewer model `claude-fable-5-2`; effort as assigned by root (medium), not observable from inside.
- Phase `08G-P03`; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; branch `plan08_ph1`.
- Phase start = review base = HEAD = `c7fbbbbe50827683aed89a15e1e18c551ddff809`.
- Digest `a078b8caf6589f4b68f104ff572ff864ad8b9d31698a1f1531abede9c8fa9a99`, exclusions
  `plan/08-goals/phases/p03/docs` and `.trail/learnings`. Confirmed at the start and again before returning.
- Coordination: `plan/08-goals/COORDINATION.md` revision 8. The pinned copy
  `logs/08G-P03-coordination-r8.md` is byte-identical (diff). Notices: 08G-C001 r2 (satisfied), 08G-C002 r1
  (acknowledged).

**Coverage and evidence** (inspection only; no implementation exists, and no test was run)
- Requirements read from the originals: doc 03 batches 03A/03B and focused checks, README decisions 1–7 and
  proposals 1–5, doc 02, root `AGENTS.md`, `src/adapters/persistence/AGENTS.md` (database boundary, migration).
- Schema: `departments` DDL and checks, `delete_company_tree` and its trigger, the company cascades on
  `workflow_runs` and `threads`, `threads_channel_fk`. Teardown is compatible with RESTRICT: runs and threads
  go with the company cascade or the channel delete before departments are deleted.
- Delete: `department/lifecycle.rs::delete_department_on`, `delete_error`, `guard_department_delete_on`.
- Run writers: every `insert_run` caller and the single production `INSERT INTO workflow_runs`. Found the
  production native writer `native_mcp/mod.rs:114`, which TASK does not list (F2).
- Thread writers: the three production `INSERT INTO threads` sites and the two `NewThread` builders.
- Locks: `archive.rs` in full; `admission.rs::refuse_archived_desk_on`; `waits::lock`;
  `lease::lock_scope`/`lock_fence`; the call dispatch order in `channel_call_target.rs`;
  `controls.rs::cancel_for_archive_on`. Every run writer takes the company row `FOR NO KEY UPDATE` before run
  rows, and so does archive, so the added department share lock creates no inversion against
  cancel-and-archive. This is by inspection; the planned `goal_competing_` test must use the production
  archive write and a production child writer.
- Authority: `use_cases/department.rs` (`create_department`, `managed_roles`, `update_department`) and
  `department/roles.rs::assign_sponsor_on`. Create and role assignment both need managing the parent; the
  first holder becomes decider. D1.2's authority claim holds.
- Investigation anchors checked and correct, apart from the native writer.
- Reviewed lightly: the UI and JSON surfaces of D1.2, the sample fixture, onboarding. Not examined: B2.

**Outcome:** changes required. Actionable: 08G-P03-D1-F1 (high), F2 (medium), F3 (medium), F4 (low), F5 (low).
Optional: 08G-P03-D1-S1, S2, S3. No pass is given for D1 r1.

**Process and instruction feedback**
- The gate was warranted and sized well: schema, guards and the resolver were reviewable together, and F1 is
  an interaction between three of the five decisions.
- TASK's investigation with current anchors was useful and almost entirely accurate.
- Candidates (not adopted): a writer inventory names production versus fixture callers (F2); a guard added
  below an application rule is checked against each producer of the value and the state changes allowed
  between capture and write (F1); a departure from plan wording carries its reason (F4).

## 08G-P03-D1-R2-N1 — correction recheck, gate 08G-P03-D1 r2 (2026-11-24)

**Identity**
- Workflow `implement-opus-reviewed-2_4`; batch B1; stage: architecture gate recheck, decision revision D1 r2
  ([TASK.md](TASK.md), "Gate 08G-P03-D1 r2 — corrections to r1"). Follows 08G-P03-D1-R1-N1.
- Reviewer `claude-fable-5-2`; effort as assigned by root (medium).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, HEAD = review base =
  `c7fbbbbe`. Digest recomputed: `a078b8caf6589f4b68f104ff572ff864ad8b9d31698a1f1531abede9c8fa9a99`, same
  exclusions; no source changed. Coordination revision 8 and its pinned copy as before.

**Coverage and evidence** (inspection; nothing was run, and no implementation exists)
- Rechecked by finding ID: F1–F5 and the adopted S1–S3. Dispositions are in the findings log.
- F1: `FOR SHARE` conflicts with the `FOR NO KEY UPDATE` of a kind change and with archive's `FOR UPDATE`, so
  the resolver and the guard read the same state in one transaction.
- F3: compared the computation with `archive.rs` and `controls.rs::cancel_tree_on`. The cancel list is the
  subtree-desk roots and the 08G-C002 call children; the cascade follows waiting `child_run` waits, which
  `children.rs::write_on` writes for both child identities. The ancestry walk and the cascade are different
  walks; the post-cancel recount covers a disagreement by rolling back. The 64 bound matches
  `channel_call_source.rs:98`, and the bound fails toward non-cancellable.
- F2: both `SessionAssociation` variants against `native_mcp/mod.rs::source_facts_on` and `admit_on`. A native
  run elsewhere that carries the goal is an uncovered root, and one on a subtree desk is already
  non-cancellable through `work.native`, so the archive rule agrees.
- Reused from R1 without re-reading: schema and teardown, thread writers, sponsor authority, lock order.

**Outcome:** PASS for decision revision D1 r2 at the digest above. It accepts the decisions and the
verification plan, not the execution of the planned tests. No new findings. One point for the batch review:
the uncovered-run check must compare against the whole cancel set, not the list truncated at
`MAX_ARCHIVE_CANCELLATIONS + 1` (above the bound the result is non-cancellable either way).

**Process and instruction feedback:** one correction round closed all five; the correction text stated each
changed rule with its tests, which made the recheck short. No instruction change is supported beyond the R1
candidates.

## 08G-P03-B1-R1-N1 — batch review, B1 (2026-11-24)

**Identity**
- Workflow `implement-opus-reviewed-2_4`; batch B1 (03A + 03B); stage: whole-batch review, round
  08G-P03-B1-R1. Task record: [TASK.md](TASK.md), "B1 implementation progress". Gated decisions: 08G-P03-D1 r1
  with the r2 corrections (passed in 08G-P03-D1-R2-N1).
- Reviewer `claude-fable-5-2`; effort as assigned by root (medium).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`. Phase start = review base =
  HEAD = `c7fbbbbe`; the batch is the working tree against it, plus six untracked source files. Preserved
  changes that are not B1: the Plan08 README, doc 03 and COORDINATION edits, and the agent-workflow-call p06
  HANDOFF.
- Digest `df55a3111d6577894597abbf0616b21036c593d695ff080367497f9da302f27b`, exclusions
  `plan/08-goals/phases/p03/docs` and `.trail/learnings`; recomputed at the start and before returning.
- Coordination revision 8, pinned at `logs/08G-P03-coordination-r8.md`; 08G-C001 r2 satisfied, 08G-C002 r1
  acknowledged.
- Evidence manifest `logs/08G-P03-B1-R1.evidence.json`: the checker reports it valid for this digest (5 PASS,
  1 FAIL, 2 NOT EXERCISED).

**Coverage and evidence**
- Read in full: the schema diff; `department/archive.rs`, `department/lifecycle.rs`, `department.rs`;
  `workflow/goals.rs` (both layers); the writer diffs (`admission_write.rs`, `children.rs`,
  `native_mcp/mod.rs`, `channel_call_source.rs`, `message_writer.rs`, `message.rs`, `message_planner.rs`);
  ports, use case, domain; both route files and the page diff; the sample fixture; `docs/departments.md`;
  `goal_trail_tests.rs`, the archive and race tests, the native test, the sample compile test, and most of
  `department_goal_tests.rs`.
- Writers: one production `INSERT INTO workflow_runs` (`insert_run`), whose three production callers all pass
  a resolved goal; `RunRow.goal` is a required field. Three production `INSERT INTO threads`: the workflow
  message writer binds the column, and `thread/mod.rs:340` and `thread/inbound.rs:659` leave it null. Both
  `message.send` planners set none. None missed.
- Carried item: `uncovered_goal_run_on` joins against the whole `CANCEL_SET`; only the list fetched for
  cancelling carries `LIMIT MAX_ARCHIVE_CANCELLATIONS + 1`. Confirmed.
- Lock order, by inspection: every path that reaches `resolve_on` or the guard holds the company row first
  (`refuse_archived_desk_on`; `waits::lock` with `Transition` for parks and for resume in `wait_commit.rs`,
  which covers `next_round`; `lease::lock_scope` for tool calls and call dispatch; the native caller).
  Archive and department edits take it first too. The share locks are taken in id order.
- Archive: the count, the uncovered rule and the unchanged post-cancel recount match D1 r2. A native goal run
  elsewhere stays non-cancellable through `work.native`.
- Delete: the worded guard, the FK below it and the teardown are each asserted in
  `goal_trail_column_is_immutable_and_goal_only`.
- Tests inspected as passing in logs, not rerun by me: all 25 `goal_` tests at 2 MiB in
  `logs/b1-stack-budget.txt` (written after the last source edit, by file times);
  `channel_call_resume_archive_cancels_call_child` and 48 `unit_archive` tests in
  `logs/b1-focused-tests.txt`; the catalog pin and the sample test in `logs/b1-catalog-tests.txt`.
- The broad run's two failures: the catalog pin is a B1 regression, fixed and passing in the catalog run;
  `doop_agent_current_preparation_reaches_ready_before_model_io` fails at an assertion in
  `agent_history_freeze_guard_tests.rs:435` and B1 changes nothing under `src/application/agent` or
  `workflow/agent.rs`, so I accept it as unrelated. The broad run was not repeated after the two test-only
  edits.
- One probe run by me, read-only, on `openbot_p08p03_mac02`: the chain query with a two-row placement
  (see S1).
- Reviewed lightly: the route and page tests, `goals_tests.rs` (names and the log only), fixture plumbing in
  `fixtures/mod.rs`. Not run: browser checks (none exist for these forms), the full suite (p04).

**Outcome:** changes required. Actionable: 08G-P03-B1-F1 (low), 08G-P03-B1-F2 (low). Optional: 08G-P03-B1-S1.
No correctness defect was found in the schema, the resolution, the writers, the lock protocol, the archive
rule or the delete refusal.

**Process and instruction feedback**
- The batch was a good size for one review. The TASK account was accurate where checked, and its gaps section
  was honest; F2 is about disposition and naming, not concealment.
- The gate paid off: the r2 rules were implemented as decided and needed no reopening.
- Candidate (not adopted): when a batch changes what a reported number or flag means, the account lists the
  places that render it (F1).

## 08G-P03-B1-R2-N1 — correction recheck, B1 (2026-11-24)

**Identity**
- Workflow `implement-opus-reviewed-2_4`; batch B1; stage: correction recheck, round 08G-P03-B1-R2, after
  08G-P03-B1-R1-N1. Task record: [TASK.md](TASK.md), "08G-P03-B1-R1 corrections" and "R2 checks".
- Reviewer `claude-fable-5-2`; effort as assigned by root (medium).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, base = HEAD = `c7fbbbbe`.
- Digest `6caa7f19d63b49cfae39c91f042e35032ed05112664eba146469bd652d3b4e2b`, same exclusions; recomputed at
  the start and before returning.
- Coordination revision 8, identical to the pin (diff). 08G-C001 r2 satisfied, 08G-C002 r1 acknowledged.
- Evidence manifest `logs/08G-P03-B1-R2.evidence.json`: valid for this digest (4 PASS, 1 FAIL, 1 NOT EXERCISED).

**Coverage and evidence**
- There is no commit between the rounds, so the delta was taken from the files changed after the R1 manifest
  (13 files) and read against the R1 text in this log: `app_error.rs`, `app_error_impl.rs`,
  `department/archive.rs`, `ui_departments.rs`, `department/lifecycle.rs`, `workflow/goals.rs`, the docs,
  and the three changed or new tests. `native_mcp/mod.rs` was touched for the mutation check and its diff
  against the base equals the R1 code.
- Rechecked by finding ID: F1, F2, S1. Dispositions are in the findings log.
- Every production match on `UnitArchiveOpenWork` either carries `elsewhere` or ignores the fields.
- Logs inspected, not rerun by me: the broad run, taken after the last source edit by file times, has 2086
  passed and 1 failed; the four correction tests and `channel_call_resume_archive_cancels_call_child` are ok
  in it; 27 `goal_` tests pass at 2 MiB; clippy 1.95 and fmt pass. The one failure is
  `doop_agent_current_preparation_reaches_ready_before_model_io`, accepted as unrelated in R1 for the same
  reason. The mutation check for the native arm is the author's claim; the test's construction supports it.
- Reused from R1 without re-reading: schema, writers, lock order, the cancel-set comparison, delete and
  teardown. SQLx prepare was not repeated; the delta adds no checked macros.
- Not exercised, as before: browser checks for the forms; the full suite (p04). The thread variant of the
  native test still stamps the thread's goal directly.

**Outcome:** PASS for batch B1 at digest `6caa7f19d63b49cfae39c91f042e35032ed05112664eba146469bd652d3b4e2b`.
08G-P03-B1-F1 and F2 fixed; S1 adopted. New optional suggestion 08G-P03-B1-S2. The pass covers the focused
scope above; the unrelated failing test and the full suite stay outside it.

**Process and instruction feedback:** one correction round closed both findings. Without a commit or a saved
copy per round, the correction delta had to be rebuilt from file times; a retained patch per round
(`git diff` into `logs/`) would make a recheck exact. Candidate only, not adopted.

## 08G-P03-D2-R1-N1 — architecture gate 08G-P03-D2 r1 (2026-11-24)

**Identity**
- Workflow `implement-opus-reviewed-2_4`; batch B2 (03C + 03D); stage: architecture gate, decision revision
  D2 r1 (D2.1–D2.9). Task record: [TASK.md](TASK.md), "Gate 08G-P03-D2 r1".
- Reviewer `claude-fable-5-2`; effort as assigned by root (medium).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, base = HEAD = `c7fbbbbe`.
- Digest `6caa7f19d63b49cfae39c91f042e35032ed05112664eba146469bd652d3b4e2b` (accepted B1; no B2 code), same
  exclusions; recomputed at the start and before returning.
- Coordination revision 8, identical to the pin. Applicable: 08G-C001 r2, whose consequence (b) bears on D2.4
  (F2); 08G-C002 r1.

**Coverage and evidence** (inspection, plus one read-only probe; no B2 code exists)
- Requirements from the originals: doc 03 batches 03C and 03D and their focused checks, README proposal 4.
- 03C against code: `unit_budgets.rs` in full; the schema for `unit_budgets`, `unit_spend`,
  `charge_unit_spend`, `forget_unit_spend`, `attribute_model_usage`; the two callers of `check_admission_on`;
  `children.rs::write_call_on`; `DeniedCode` and `ChannelCallOutcome::refused`; `BudgetExhausted`;
  `workflow_channel_call_results`; the other reader of `unit_budgets` (`model_usage/report.rs`).
- Probe on `openbot_p08p03_mac02`: `date_trunc('total', …)` raises an error (F1).
- D2.2: per-(goal, unit) rows with no department key and the stated lock order are consistent with 07's
  `unit_spend`; settlements take `unit_spend` rows in one order and then one goal row, so I see no cycle.
- D2.3: charging every run but checking only root admission and call children matches 07's two check sites.
  The reading of "admit at most" is 07's agreed one (`plan/07-unit-resources/COORDINATION.md`, decision k).
- D2.5: desk view or management matches 03D.5; run links keeping their own check is what the plan describes.
- D2.6: the board's department filter is viewer-scoped (`desk_departments`, `known_department`), which the
  goal filter as written is not (F3).
- D2.8: the role check at call time is right; holders change after publication, and a run in another
  department carries the goal without its agent holding a role on it.
- Reviewed lightly: the built-in tool touch points and their access-level handling (not traced),
  `attention.rs` patterns, the page layout, D2.7, D2.9. Not examined: how the rollup's joins perform at the
  10,000-run limit.

**Outcome:** changes required; no pass for D2 r1. Actionable: 08G-P03-D2-F1 (high), F2 (medium), F3 (medium),
F4 (low). Optional: 08G-P03-D2-S1.

**Process and instruction feedback**
- The gate was warranted: F1 and F2 are schema and contract decisions that would have been built on.
- Candidate (not adopted), from F1 and F4: when a gate adds a value to a stored discriminator, it lists every
  SQL reader of that column with a disposition. The same lesson as 08G-P03-B1-F1, for a different value.
- The requested open points for the tool's output level are not answered by this review; I did not trace
  them.

## 08G-P03-D2-R2-N1 — correction recheck, gate 08G-P03-D2 r2 (2026-11-24)

**Identity**
- Workflow `implement-opus-reviewed-2_4`; batch B2; stage: architecture gate recheck, decision revision D2 r2
  ([TASK.md](TASK.md), "Gate 08G-P03-D2 r2: corrections to r1"). Follows 08G-P03-D2-R1-N1.
- Reviewer `claude-fable-5-2`; effort as assigned by root (medium).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, base = HEAD = `c7fbbbbe`.
  Digest `6caa7f19d63b49cfae39c91f042e35032ed05112664eba146469bd652d3b4e2b`, same exclusions, recomputed at
  the start and before returning; no B2 code. Coordination revision 8, identical to the pin.

**Coverage and evidence** (inspection; nothing was run)
- Rechecked by finding ID: F1–F4 and S1. Dispositions are in the findings log.
- For F2, read `channel_call_target::start_on` in full, `denied`, the dispatch transaction in
  `channel_call_dispatch.rs` (`dispatch`, `dispatch_on`), and every production construction of a `DeniedCode`
  or `ChannelCallOutcome::refused`. The four returns cover every pre-launch denial, and the transaction
  commits before the observation is returned.
- Reused from R1: the 07 schema and check sites, the counter design, D2.5, D2.8.
- Not traced, as in R1: the built-in tool touch points and the level of the tool's output; rollup cost at
  the limit.

**Outcome:** PASS for decision revision D2 r2 at the digest above. It accepts the decisions and the
verification plan, not the planned tests. No new findings. Points for the batch review:
- the coordination response to 08G-C001 r2 (b) must exist before acceptance;
- the new table and triggers need the catalog pins updated, as B1's trigger did;
- the refusal row is written outside the child's savepoint at the `create_call_on` site;
- a refusal whose call is later answered with an input error leaves a stale row; say whether the rollup
  accepts that.

**Process and instruction feedback:** one round closed all four. No new instruction candidate.
