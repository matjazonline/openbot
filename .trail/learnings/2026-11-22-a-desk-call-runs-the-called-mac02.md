---
title: "A desk call runs the called desk's workflow under that desk's own run-as identity, not the caller's"
author: Mac02
date: 2026-11-22
branch: plan08_ph1
cost: { minutes: 17, tokens: 201942 }
id: l-c0bc0871
scope: repo
status: active
src: agent
touches:
  - plan/agent-workflow-call/phases/p01/docs/logs/AWC-P01-D1-r1-decision.md
  - plan/agent-workflow-call/COORDINATION.md
  - plan/agent-workflow-call/README.md
  - plan/11-access-rules/README.md
  - plan/agent-workflow-call/phases/p01/docs/TASK.md
  - src/adapters/persistence/workflow/children.rs
  - src/application/use_cases/thread/ingest/policy.rs
  - src/adapters/persistence/workflow/knowledge.rs
---

## Decided
User decision, 22 Nov 2026, for `channel.call` (plan agent-workflow-call), adopted before plan 11's request rules exist:
- The child runs the destination desk's selected binding as configured: its run_as, params, resources, agent and unit scopes. The caller is recorded only as the requester and gains no access to the destination.
- Until request rules land, a call is admitted when the calling agent may write to the destination desk under the rule an internal relay from that agent already obeys. No actor is exempt, the owner included.
- The child reads no higher than the lower of the caller's ceiling and the run_as person's level. A request labelled above that, or above the desk's audience, is refused before anything is stored.

## Tried and ruled out
- Running the child as the calling run's actor, which needs operations authority and caller-side checks on the destination's resources (the original plan text). Ruled out because it gives the person behind a Sales run reach into Finance, which plan 11 decision 1 rejects.

## Watch out
- Without request rules, the only gate is the agent's write right on the desk, so a caller can cause work under the destination's identity. The user accepted this. Plan 11 03B swaps the write rule for the request rule, and 03C adds the allowance to the ceiling.
- Unlike an ordinary child, a call child must not copy the parent's unit scopes or binding identity.

In the code:
- Gate text: plan/agent-workflow-call/phases/p01/docs/logs/AWC-P01-D1-r1-decision.md
- Ordinary children copy the parent's scopes and use its binding as provenance only: src/adapters/persistence/workflow/children.rs (`configured_child`, `insert_child_on`)
- Agent sender write rule: src/application/use_cases/thread/ingest/policy.rs `internal_context`, routing.rs `authorize_channel`
- Ceiling: src/adapters/persistence/workflow/knowledge.rs `actor_level_on`
