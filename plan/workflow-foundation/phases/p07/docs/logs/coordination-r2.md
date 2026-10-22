# Plan coordination: workflow-foundation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

- Authority: /Users/mac02/dev/openbot/plan/workflow-foundation/COORDINATION.md
- Writer: phase p07 root session of 2026-10-22 (Claude Code, Opus 5.5/xhigh), worktree
  /Users/mac02/dev/openbot
- Revision: 2

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

### WF-C001 / r1
- State: proposed
- Origin: p07, decision P07-D1 rev 2 §D1.2a, §D1.7, §D1.1 (`phases/p07/docs/logs/decision-D1-r2.md`);
  findings WF-P07-F003, WF-P07-F004.
- Targets: p08
- Instruction/change: `TriggerSource::Child` is not admissible through `WorkflowService::admit`.
  Delegation or other runtime-created children use the runtime child writer
  (`children::create_on`) semantics or a separately reviewed path that satisfies D1.2a (frozen
  pinned graph, inherited deadline, parent wait consuming the wakeup). Manual start/rerun entry
  points expose `AdmitWorkflowRequest.rerun_of`. Every production `DefinitionService`
  composition includes `ChildPublicationDirectory`, or parents with child steps cannot publish.
- Dependencies/order: effective when p07 B1 is accepted; blocks p08 entry-point and delegation
  work that admits runs or composes publication.
- Evidence/review: P07-D1 rev 2, pending B recheck of WF-P07-R001.
- Verification: p08 tests show refusal of child admission, rerun link exposure, and publication
  of a parent with a `workflow.call` step through the production composition.
- Responses: p08 → pending.

### WF-C002 / r1
- State: proposed
- Origin: p07, decision P07-D1 rev 2 §D1.11; finding WF-P07-F007 item 4.
- Targets: p09
- Instruction/change: runtime child runs carry the parent's `binding_id` for provenance only.
  Binding run lists filter `parent_run_id IS NULL` or label children. Run inspection shows
  lineage (parent run/execution, round ordinal) and `rerun_of`.
- Dependencies/order: effective when p07 B1 is accepted; applies to p09 operations UI.
- Evidence/review: P07-D1 rev 2, pending B recheck of WF-P07-R001.
- Verification: p09 run lists and inspection tests with a runtime child and a rerun.
- Responses: p09 → pending.
