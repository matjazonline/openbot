# Plan coordination: cloudroom-local

- Authority: `/Users/mac03/dev/openbot/plan/cloudroom-local/COORDINATION.md`
- Writer: `/root`, main worktree `/Users/mac03/dev/openbot`
- Revision: 4
- Workflow record, not an additional implementation plan.

## Plan-wide instructions

- CL-I001/r1: Execute groups p01–p07 in dependency order from the original numbered plan files. Accept each group before dependent implementation. No commits, deployment, production resets, or provider inference outside disposable authorized projects.
- CL-I002/r1: Preserve pre-existing deletions in `.agents/skills/implement-astra-reviewed-2-2/`. All work is serialized in this checkout; no concurrent source writers.
- CL-I003/r1: Follow repository reset policy; no backward compatibility. Source inspection and fixtures never establish real-provider/platform acceptance.
- CL-I004/r1: Focused checks per group; repository full verification once at final integration. Record unavailable proof without claiming completion.

- CL-I005/r1: User explicitly chose “Leave Linux acceptance unverified” on 2026-11-07. Complete implementation and other required verification; report native headless Linux/platform acceptance as NOT EXERCISED. This does not waive Rust/database/plugin/browser checks or macOS proof.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | /root (author) | /Users/mac03/dev/openbot | phases/p01/docs/{TASK,HANDOFF}.md |

## Cross-phase notices

### CL-C001 / r1
- State: active
- Origin: p01, /root, CL-P01-D01/r1 (phases/p01/docs/logs/CL-P01-D01-r1.md)
- Targets: p01, p04, p06, p07
- Instruction/change: identify integration Cloudroom builds by the fixed source base plus the reviewed local-only optional-onboarding patch; protected local APIs and hosted cloud entitlements retain authentication.
- Dependencies/order: CL-P01-D01/r1 passed; effective before patch application/build-helper edits in p01 and every later integration source build.
- Evidence/review: CL-P01-D01/r1, PASS in reviewer note CL-P01-D01-R1; exact source/patch hashes retained in phase decision.
- Verification: source guards, native permission/UI recovery without hosted account, bounded reproducible source build.
- Responses: p01 -> acknowledged active r1, implementation/verification pending. p04/p06/p07 must acknowledge when started.

