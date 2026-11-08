# Plan coordination: cloudroom-local

- Authority: `/Users/mac03/dev/openbot/plan/cloudroom-local/COORDINATION.md`
- Writer: `/root`, main worktree `/Users/mac03/dev/openbot`
- Revision: 2
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

### CL-C001 / r1
- State: proposed
- Origin: p01, /root, CL-P01-D01/r1 (phases/p01/docs/logs/CL-P01-D01-r1.md)
- Targets: p01, p04, p06, p07
- Instruction/change: identify integration Cloudroom builds by the fixed source base plus the reviewed local-only optional-onboarding patch; protected local APIs and hosted cloud entitlements retain authentication.
- Dependencies/order: architecture gate must PASS before applying patch/build-helper edits; no contract activation yet.
- Evidence/review: decision r1, proposed patch and exact source/patch hashes in the decision; review pending.
- Verification: source guards, native permission/UI recovery without hosted account, bounded reproducible source build.
- Responses: p01 -> acknowledged proposed r1; not implemented or satisfied. Future phases evaluate if activated.

