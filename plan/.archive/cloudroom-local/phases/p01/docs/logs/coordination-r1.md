# Plan coordination: cloudroom-local

- Authority: `/Users/mac03/dev/openbot/plan/cloudroom-local/COORDINATION.md`
- Writer: `/root`, main worktree `/Users/mac03/dev/openbot`
- Revision: 1
- Workflow record, not an additional implementation plan.

## Plan-wide instructions

- CL-I001/r1: Execute groups p01–p07 in dependency order from the original numbered plan files. Accept each group before dependent implementation. No commits, deployment, production resets, or provider inference outside disposable authorized projects.
- CL-I002/r1: Preserve pre-existing deletions in `.agents/skills/implement-astra-reviewed-2-2/`. All work is serialized in this checkout; no concurrent source writers.
- CL-I003/r1: Follow repository reset policy; no backward compatibility. Source inspection and fixtures never establish real-provider/platform acceptance.
- CL-I004/r1: Focused checks per group; repository full verification once at final integration. Record unavailable proof without claiming completion.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | /root (author) | /Users/mac03/dev/openbot | phases/p01/docs/{TASK,HANDOFF}.md |

## Cross-phase notices

None.
