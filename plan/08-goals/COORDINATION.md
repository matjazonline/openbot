# Plan coordination: 08-goals

- Authority: /Users/mac02/orca/workspaces/openbot/plan08_ph1/plan/08-goals/COORDINATION.md
- Writer: Claude Code session `1e84a7af` in `/Users/mac02/orca/workspaces/openbot/plan08_ph1`. Ownership handoff 2026-11-22: previous writer `plan08-ph1-51` (p02) finished and is idle; the user asked this session to continue plan 08. Earlier: /root session 01a1e095 (p01) to `plan08-ph1-51` on 2026-11-21.
- Revision: 8

## Plan-wide instructions

No additional instructions. Original plan documents and repository instructions apply. r3: p01 accepted and committed (`6c63f292`); origin/main with plans 04–07 merged at `73afd5a7`. p02 authorized 2026-11-21. User confirmed README proposals 2 (sponsor decider manages) and 3 (archive refuses open work) on 2026-11-21.

r4 (2026-11-22): p02 accepted in phase and committed (`fe7791d3`), not integrated. p03 is blocked on `channel.call`, which no branch implements. The user chose to implement [agent-workflow-call](../agent-workflow-call/README.md) first, on this branch, then return to p03 (notice 08G-C001). README proposals 4 and 5 are still open and must be settled before p03 starts.

r7 (2026-11-24): the user confirmed README proposals 4 (a goal's budget counts the work done for it anywhere, so Batch 03C applies) and 5 (a goal with recorded work cannot be deleted, only archived). agent-workflow-call is accepted in phase through p06 and committed (`358e662a`, learnings `c7fbbbbe`). Its completion record lists gates unavailable on this machine and one intermittent gap; it is not declared complete. p03 may start once it records that acceptance identity per 08G-C001. Proposal 1 was built by p01 (`workflow.role_wrong_kind`).

r8 (2026-11-24): p03 started (root `1e84a7af`, workflow implement-opus-reviewed-2_4). Batches: B1 = 03A+03B, B2 = 03C+03D. 08G-C001 is satisfied and 08G-C002 acknowledged for p03.

r9 (2026-11-24): p03 B1 (03A+03B) accepted in phase at `6caa7f19…4e2b`, not committed. Gate 08G-P03-D2 r2 (B2 = 03C+03D) passed. p03 responds to 08G-C001 r2 (b): p03 records refused calls before launch in `workflow_channel_call_refusals`, so the goal rollup counts them as `denied`.

r10 (2026-11-24): p03 B1 committed as `481b14cd`. p03 B2 (03C+03D) is accepted in phase at `a50dab5d…243d` (B PASS
08G-P03-B2-R2-N1) and committed with this revision. New shared contracts: `unit_budgets.period` `total` (goals
only), `goal_spend`, `model_usage_calls`/`model_usage_history.goal_unit_id`, `workflow_channel_call_refusals`,
`schedule_runs` refusal `budget.goal_exhausted`, and the `goal.status` built-in. 08G-C002 is satisfied for p03: its
checks are in the B2 focused run. Open for p04: `goal.status` through the agent runtime is not yet exercised.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | /root; resumed on integrated prerequisite (complete) | /Users/mac02/orca/workspaces/openbot/plan08_ph1 | phases/p01/docs/TASK.md; phases/p01/docs/HANDOFF.md |
| p02 | Claude Code `plan08-ph1-51` | /Users/mac02/orca/workspaces/openbot/plan08_ph1 | phases/p02/docs/TASK.md; phases/p02/docs/HANDOFF.md |
| p03 | Claude Code `1e84a7af` (started 2026-11-24; B1 and B2 accepted in phase and committed; p03 complete in phase) | /Users/mac02/orca/workspaces/openbot/plan08_ph1 | phases/p03/docs/TASK.md; phases/p03/docs/HANDOFF.md |

## Cross-phase notices

### 08G-C001 / r2
- State: active (r2 2026-11-22: adds the consequences of agent-workflow-call gate AWC-P01-D1 r3)
- Origin: coordination r4, user direction 2026-11-22
- Targets: p03, p04
- Instruction/change: p03 does not start until agent-workflow-call is accepted through its p06 (`plan/agent-workflow-call/COORDINATION.md`). That plan's gate AWC-P01-D1 decides that a call's child runs the destination binding as configured, with its own `run_as`; p03's trail and budget work builds on that. agent-workflow-call implements the call clause of 02B item 3 (an archived destination returns a `denied` closure with code `unit.archived`), which p02 left NOT EXERCISED. r2 consequences for p03: (a) until plan 11 supplies an allowance, a call's child reads public data only and a caller holding anything above level 0 cannot call (user, 2026-11-22); (b) a `denied` closure produced before launch (archived unit, unit spend cap, no responder) leaves no rows, so 03D's closure rollup counts only closures of launched calls unless p03 adds a record; (c) agent-workflow-call p03 extends p02's cancel-and-archive to cancel open call children on the archived desks, and will publish a notice here when it does.
- Dependencies/order: blocks p03 entirely.
- Evidence/review: none yet
- Verification: p03 startup records agent-workflow-call's p06 acceptance identity.
- Responses: p03 -> satisfied r8 (agent-workflow-call p06 accepted in phase at `bc8b9841…764f`, committed `358e662a`); p03 -> (b) addressed r9: refused calls before launch are recorded (`workflow_channel_call_refusals`) and counted as `denied` in the goal rollup (gate 08G-P03-D2 r2 F2)

### 08G-C002 / r1
- State: active (r6 2026-11-23)
- Origin: agent-workflow-call p03 (gate `plan/agent-workflow-call/phases/p03/docs/logs/AWC-P03-D1-r1-decision.md` D6, accepted under AWC-P03-B1-R1-N1); required by agent-workflow-call notice AWC-C001 r4
- Targets: p03, and any phase that changes cancel-and-archive (`department/archive.rs::archive_department_on`)
- Instruction/change: p02's cancel-and-archive now also cancels recorded channel-call children (`workflow_channel_calls.child_run_id`) that run on the archived subtree's desks and whose caller's desk is outside the subtree (or absent). Roots are cancelled first, then those children; one `MAX_ARCHIVE_CANCELLATIONS` bound counts both. A cancelled call child wakes its caller, which receives `workflow.child_cancelled`. A caller on the subtree is cancelled with its own tree. Before this change an open call child made the archive non-cancellable.
- Dependencies/order: in effect on branch `plan08_ph1` (uncommitted with agent-workflow-call p01–p03).
- Evidence/review: `channel_call_resume_archive_cancels_call_child`; `unit_archive_` suite passing (agent-workflow-call `phases/p03/docs/logs/AWC-P03-B1-affected.txt`).
- Verification: p03 keeps `channel_call_resume_archive_cancels_call_child` and the `unit_archive_` suite green.
- Responses: p03 -> acknowledged r8 (gate 08G-P03-D1 D1.5 builds the goal open-work count on it); p03 -> satisfied r10 (`unit_archive_` and `channel_call_` in `phases/p03/docs/logs/08G-P03-B2-R2-focused.txt`)

Earlier entries: No other phase is being implemented or assigned. Plan 02 is now present in integrated baseline `8fdb892b6e279ed0c09150e2336bbdc7462f877d`; previous prerequisite blocker resolved by externally updated task branch. No source integration performed by this phase.
