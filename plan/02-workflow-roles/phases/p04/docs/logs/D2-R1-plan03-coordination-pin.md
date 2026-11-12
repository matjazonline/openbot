# Plan coordination: 03-principal-addresses

- Authority: /Users/mac02/.codex/worktrees/2050/openbot/plan/03-principal-addresses/COORDINATION.md
- Writer: `/root` in `/Users/mac02/.codex/worktrees/2050/openbot`
- Revision: 2

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
| p01 | /root; queued | /Users/mac02/.codex/worktrees/2050/openbot | phases/p01/docs/TASK.md; HANDOFF.md |
| p02 | /root; queued | /Users/mac02/.codex/worktrees/2050/openbot | phases/p02/docs/TASK.md; HANDOFF.md |
| p03 | /root; queued | /Users/mac02/.codex/worktrees/2050/openbot | phases/p03/docs/TASK.md; HANDOFF.md |
| p04 | /root; queued | /Users/mac02/.codex/worktrees/2050/openbot | phases/p04/docs/TASK.md; HANDOFF.md |

## Cross-phase notices

### 03PA-C001 / r1
- State: active instruction for future integration; producer not yet accepted
- Origin: plan02 root, active02WR-C005/r3 at authoritative /Users/mac02/.codex/worktrees/2050/openbot/plan/02-workflow-roles/COORDINATION.md revision22; immutable design plan/02-workflow-roles/phases/p04/docs/logs/D1-r3.md, B architecturePASS02WR-P04-D1-R3/N1
- Targets: all plan03 phases
- Instruction/change: After consuming accepted plan02, replace agent escalation notice routing from default desks with principal target binding/ingress after reset. Preserve authenticated notification-origin recursion suppression, targeted actual binding/revision admission and accountable root, neutral all-holder recipient semantics, canonical People follow resolutions/frozen context/current authority/history/notice proof, and ordered operation-specific account prelock protocols. Do not preserve a parallel default-desk path as compatibility. Principal addressing may require its own architecture gate for affected contexts/associations.
- Dependencies/order: All phases still await COMPLETE accepted02-workflow-roles p05; p04 designPASS is not implementation acceptance or a usable producer artifact.
- Evidence/review: active02WR-C005/r3 and p04 D1/r3, future acceptedp04/p05 snapshots required.
- Verification: principal-notice direct ingress and targeted binding/origin/owner-ladder tests plus affected plan02 regressions; plan03 final gate.
- Responses: p01–p04 queued; evaluate/acknowledge actual accepted producer at startup.
