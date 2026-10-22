# Plan coordination: workflow-foundation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

- Authority: /Users/mac02/dev/openbot/plan/workflow-foundation/COORDINATION.md
- Writer: phase p07 root session of 2026-10-22 (Claude Code, Opus 5.5/xhigh), worktree
  /Users/mac02/dev/openbot
- Revision: 4

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
| p07 | root session 2026-10-22 (implement-opus-reviewed-2_2) | /Users/mac02/dev/openbot (branch `workflows`) | plan/workflow-foundation/phases/p07/docs/TASK.md, HANDOFF.md |

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
- Responses: p08 → pending.

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
- Responses: p08 → pending; p09 → pending.
