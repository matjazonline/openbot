# Plan coordination: workflow-foundation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

- Authority: /Users/mac02/dev/openbot/plan/workflow-foundation/COORDINATION.md
- Writer: phase p08 root session of 2026-10-22 (Claude Code, Opus 5.5/xhigh), worktree
  /Users/mac02/dev/openbot (r5: p07 is complete and committed; its root no longer writes)
- Revision: 7

Ownership handoff (r2): the p07 work moved from `/Users/mac03/dev/openbot` to
`/Users/mac02/dev/openbot` (user commit `2d608f2` carries r1 and the p07 records). The p07 root
session of 2026-10-22 resumed p07 and took over as writer; the earlier session is stopped.

## Plan-wide instructions

None beyond the repository `AGENTS.md` files and the plan README. Phases run sequentially in the
primary worktree; earlier phases (01–06) were executed under the 2.1 workflows with records in
`plan/workflow-foundation/docs/` (removed by the user's retrospective cleanup; retrievable from git
history at `921079d`).

## Phase directory
| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p07 | root session 2026-10-22 (implement-opus-reviewed-2_2) — complete, committed `bd2ef36` | /Users/mac02/dev/openbot (branch `workflows`) | plan/workflow-foundation/phases/p07/docs/TASK.md, HANDOFF.md |
| p08 | root session 2026-10-22 (implement-opus-reviewed-2_2), started at `da86871` | /Users/mac02/dev/openbot (branch `workflows`) | plan/workflow-foundation/phases/p08/docs/TASK.md, HANDOFF.md |

## Cross-phase notices

### WF-C001 / r2
- State: active (r3: activated 2026-10-22 after B PASS WF-P07-R004; r4: revised to r2 after B
  PASS WF-P07-R010 for P07-D2 rev 4)
- Origin: p07, decision P07-D1 rev 2 §D1.2a, §D1.7, §D1.1 (`phases/p07/docs/logs/decision-D1-r2.md`);
  findings WF-P07-F003, WF-P07-F004.
- Targets: p08
- Instruction/change: `TriggerSource::Child` is not admissible through `WorkflowService::admit`.
  Delegation or other runtime-created children use the runtime child writer
  (`children::create_on`) semantics or a separately reviewed path that satisfies D1.2a (frozen
  pinned graph, inherited deadline, parent wait consuming the wakeup). Manual start/rerun entry
  points expose `AdmitWorkflowRequest.rerun_of`. Every production `DefinitionService`
  composition includes `ChildPublicationDirectory`, or parents with child steps cannot publish.
  r2 (P07-D2 rev 4 §D2.8): `ChildPublicationDirectory` is nested **outside**
  `AgentPublicationDirectory` (it collects pins from captured workflow-tool snapshots too), and
  every production `AgentActionTools` composition includes the workflow tool runtime.
- Dependencies/order: effective when p07 B1 is accepted; blocks p08 entry-point and delegation
  work that admits runs or composes publication.
- Evidence/review: P07-D1 rev 4 (`phases/p07/docs/logs/decision-D1-r4.md`, sha256 5c076606…adc4), B PASS WF-P07-R004.
- Verification: p08 tests show refusal of child admission, rerun link exposure, and publication
  of a parent with a `workflow.call` step through the production composition.
- Responses: p08 → acknowledged r2 (2026-10-22, p08 TASK "Coordination"): production composition in p08 B1; manual start/rerun (`rerun_of`) in p08 B2; routes in p08 B3 (r6 corrects the batch placement); verification pending.

### WF-C002 / r1
- State: active (r3: activated 2026-10-22 after B PASS WF-P07-R004)
- Origin: p07, decision P07-D1 rev 2 §D1.11; finding WF-P07-F007 item 4.
- Targets: p09
- Instruction/change: runtime child runs carry the parent's `binding_id` for provenance only.
  Binding run lists filter `parent_run_id IS NULL` or label children. Run inspection shows
  lineage (parent run/execution, round ordinal) and `rerun_of`.
- Dependencies/order: effective when p07 B1 is accepted; applies to p09 operations UI.
- Evidence/review: P07-D1 rev 4 (`phases/p07/docs/logs/decision-D1-r4.md`, sha256 5c076606…adc4), B PASS WF-P07-R004.
- Verification: p09 run lists and inspection tests with a runtime child and a rerun.
- Responses: p09 → pending.

### WF-C003 / r1
- State: active (r4: activated 2026-10-22 after B PASS WF-P07-R010)
- Origin: p07, decision P07-D2 rev 4 §D2.0, §D2.8 (`phases/p07/docs/logs/decision-D2-r4.md`,
  sha256 ebf212db…dfab7); user decisions P07-Q1 (named workflow tool registrations) and P07-Q2
  (revoke stops, re-pin/archive don't); findings WF-P07-F023 item 4.
- Targets: p08, p09
- Instruction/change: p08 exposes company-scoped HTTP routes over the `WorkflowTools` commands
  (register with expected revision, revoke, list) and over agent tool grants
  (`agents.workflow_tools`, which no route sets today), with the semantics of P07-Q2. p09 builds
  the admin UI for both. It warns when a revoked or re-pointed tool name is reused, because the
  old pins stay refused. It shows tool-call children in run inspection (the parent's saved call
  id) as it does step children (WF-C002), and covers workflow-tool children in sample execution
  ("zero production effects, including through direct agent tools and children").
- Dependencies/order: effective when p07 B2 is accepted; p08 routes before p09 UI.
- Evidence/review: P07-D2 rev 4, B PASS WF-P07-R010.
- Verification: p08 route tests (authority, revision conflict, revoke) and p09 UI/inspection and
  sample-execution tests with a workflow-tool child.
- Responses: p08 → acknowledged r1 (2026-10-22; routes in p08 B3 — r6 corrects the batch placement); verification pending. p09 → pending.

### WF-C004 / r1
- State: active (r7: published and activated 2026-10-23 by p08 after B PASS WF-P08-R006)
- Origin: p08 B1, finding WF-P08-F029 (root cause) and F030 (`phases/p08/docs/TASK.md` "B1
  corrections for WF-P08-R005"; `agent-review-findings.md`).
- Targets: all current and future phases (p08, p09, later).
- Instruction/change: sqlx-postgres 0.8.6 `begin()` cancelled after `BEGIN` reached the server
  returns its connection to the pool **inside an open transaction** (the depth counter is
  incremented only after the reply, and the drop guard does nothing at depth 0); later
  "autocommit" statements on it stay uncommitted. Every Postgres pool built in production code or
  tests must be built from `crate::infra::db::discard_leaked_transactions(PgPoolOptions::new())`,
  whose `after_release` hook closes a connection released inside a transaction. Do not hold a
  `PoolConnection` across a cancelled `begin` and keep using it (the hook only sees releases).
  Lazy pools that never connect are exempt. The regression test
  `infra::db::tests::a_cancelled_begin_never_leaks_its_transaction` fails, saying so, once sqlx
  fixes the bug; then the hook may be removed together with that test.
- Dependencies/order: effective now (p08 B1 accepted at digest `b90a7f9c…a468`, uncommitted
  working tree on branch `workflows`).
- Evidence/review: B PASS WF-P08-R006 (`phases/p08/docs/agent-review-notes.md`); sqlx-core 0.8.6
  release-path trace in that note.
- Verification: `grep -rn "PgPoolOptions::new()" src` shows only guarded constructions and the
  regression test's deliberate bare pool; p08 B2 routes the remaining test-only pools (F030)
  through the guard.
- Responses: p08 → acknowledged r1 (origin; F030 in p08 B2); p09 → pending.
