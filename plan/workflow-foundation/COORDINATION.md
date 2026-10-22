# Plan coordination: workflow-foundation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

- Authority: /Users/mac03/dev/openbot/plan/workflow-foundation/COORDINATION.md
- Writer: phase p07 root session (Claude Code, Opus 5.5/xhigh), worktree /Users/mac03/dev/openbot
- Revision: 1

## Plan-wide instructions

None beyond the repository `AGENTS.md` files and the plan README. Phases run sequentially in the
primary worktree; earlier phases (01–06) were executed under the 2.1 workflows with records in
`plan/workflow-foundation/docs/` (removed from the working tree by the user's retrospective
cleanup; retrievable from git history at `921079d`).

## Phase directory
| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p07 | root session 2026-10-21 (implement-opus-reviewed-2_2) | /Users/mac03/dev/openbot (branch `workflows`) | plan/workflow-foundation/phases/p07/docs/TASK.md, HANDOFF.md |

## Cross-phase notices

None.
