# Plan coordination: 02-autonomous-agent

- Authority: /Users/mac02/dev/openbot-recovery/plan/02-autonomous-agent/COORDINATION.md
- Writer: /root, Sol 6.1/xhigh (user confirmed), primary /Users/mac02/dev/openbot-recovery
- Revision: 1

## Plan-wide instructions

- AUTO-I001/r1: Execute original numbered phases 01 through 07 sequentially. Reuse durable workflow/action execution; no additional scheduler or agent engine. Reset incompatible disposable data; no backward compatibility.
- AUTO-I002/r1: Root is sole author. Each phase has an independent Astra/high reviewer with no inherited conversation and no project authorship. Use implement-astra-reviewed-2-2 and phase-local records; gates for consequential decisions, one cohesive whole-phase batch review.
- AUTO-I003/r1: User authorized phase commits and local main fast-forwards; publish main after the whole autonomous plan completes. Knowledge prerequisite main ce93ff340a54db18770d8a5eec1c1d2008c5e5b1 is accepted and pushed. No deployment or production reset authorized.
- AUTO-I004/r1: Latest user instruction: “we should run only targeted tests”. Apply to authors, reviewers, corrections and final integration. Supersedes manual broad/full test runs in original phase 07 and repository guidance for this task. Preserve CI configuration and existing limits/guards. Record exactly selected/executed checks; do not claim a full suite pass or request that decision again.
- AUTO-I005/r1: Preserve accepted fix-app and knowledge contracts. Knowledge authority r24 (adcb7e538bb0f63433e72244c8e447cf051699325435626d52d7e5eaadca9fcd), immutable pin in p01 logs. Preserve independent AUTH_ATTEMPT_SECRET, Caller/owner/retention/usage semantics, knowledge authorization, run ceiling/content monotonicity, atomic search observation, live release checks and shared native model ownership.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | /root author; reviewer pending | /Users/mac02/dev/openbot-recovery/.worktrees/autonomous-p01 (autonomous/p01) | plan/02-autonomous-agent/phases/p01/docs/{TASK,HANDOFF}.md within worktree |

## Cross-phase notices

None yet. Phase p01 owns initial shared binding/revision/storage contracts; consequential contract must pass G1 before dependent edits.
