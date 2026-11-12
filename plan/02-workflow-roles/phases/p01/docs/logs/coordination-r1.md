# Plan coordination: 02-workflow-roles

- Authority: /Users/mac02/.codex/worktrees/2050/openbot/plan/02-workflow-roles/COORDINATION.md
- Writer: `/root` in `/Users/mac02/.codex/worktrees/2050/openbot`
- Revision: 1

## Plan-wide instructions

### I001 / r1
No backward compatibility; edit the init schema and use fresh disposable databases.

### I002 / r1
Root/A is Sol 6.1/xhigh, confirmed by the user earlier in this session. Independent reviewer B is Astra/high with no inherited conversation. Execute phases sequentially, accepting each before dependent work. Existing authorization covers implementation; no new publishing or deployment request is inferred.

### I003 / r1
Run focused checks in implementation phases and the full repository/CI gate once at this plan's final integration phase. Retain failed checks. Do not raise limits or weaken assertions.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | /root; starting | /Users/mac02/.codex/worktrees/2050/openbot | phases/p01/docs/TASK.md; HANDOFF.md |
| p02 | /root; queued | /Users/mac02/.codex/worktrees/2050/openbot | phases/p02/docs/TASK.md; HANDOFF.md |
| p03 | /root; queued | /Users/mac02/.codex/worktrees/2050/openbot | phases/p03/docs/TASK.md; HANDOFF.md |
| p04 | /root; queued | /Users/mac02/.codex/worktrees/2050/openbot | phases/p04/docs/TASK.md; HANDOFF.md |
| p05 | /root; queued | /Users/mac02/.codex/worktrees/2050/openbot | phases/p05/docs/TASK.md; HANDOFF.md |

## Cross-phase notices

### 02WR-C001 / r1
- State: active
- Origin: user scope clarification, 2026-11-11
- Targets: p01–p05
- Instruction/change: Defer only the denied `channel.call` escalation trigger until `channel.call` exists. Implement all escalation triggers available today. The user explicitly selected this disposition. Do not introduce `channel.call` or revive deferred automated `message.send` dispatch.
- Dependencies/order: Applies throughout this plan and final completeness report.
- Evidence/review: User reply in this session; p04 must record the deferred criterion explicitly.
- Verification: Existing triggers exercised; unavailable child-call trigger clearly deferred.
- Responses: p01 acknowledged (TASK); remaining phases evaluate at startup.
