---
title: "A desk call runs the called desk's workflow under that desk's own run-as identity, and reads public data only until request rules exist"
author: Mac02
date: 2026-11-22
branch: plan08_ph1
cost: { minutes: 9, tokens: 68195 }
id: l-a5048a8f
scope: repo
status: active
replaces: l-c0bc0871
src: agent
touches:
  - plan/agent-workflow-call/phases/p01/docs/logs/AWC-P01-D1-r2-decision.md
  - src/adapters/persistence/workflow/children.rs
  - src/application/use_cases/thread/ingest/policy.rs
  - src/adapters/persistence/workflow/knowledge.rs
---

## Decided
User decisions, 22 Nov 2026, for `channel.call` (plan agent-workflow-call):
- The child runs the destination desk's selected binding as configured: its run_as, params, resources, agent and unit scopes. The caller is only recorded as the requester.
- A call is admitted when the calling agent may write to the desk under the existing internal-relay rule for an agent sender. The person behind the calling run is not checked beyond being a live member, and nobody is exempt, the owner included.
- The child's ceiling is the lowest of the caller's ceiling, plan 11's allowance and the run_as person's level. Until plan 11 adds request rules, the allowance is level 0, so calls carry public data only. The user accepted that calls are of limited use until then.

## Tried and ruled out
- Running the child as the calling run's actor, with operations authority (the original plan text). This gives the caller reach into another unit.
- Gating on the actor's operations authority or desk view instead of the agent's write right.
- Ceiling = lower of caller and run_as with no allowance. Nothing would cap what crosses units before plan 11.

## Watch out
- A call child must not copy the parent's unit scopes or binding identity, but it does inherit the parent's delivery policy (simulation) and tree limits.
- A call child's destination audience is its own ceiling, not its agent-only thread. Otherwise it always reads at level 0, even once plan 11 raises the allowance.

In the code:
- Gate text: plan/agent-workflow-call/phases/p01/docs/logs/AWC-P01-D1-r3-decision.md
- src/adapters/persistence/workflow/children.rs (`configured_child`, `insert_child_on`)
- src/application/use_cases/thread/ingest/policy.rs `internal_context`; routing.rs `authorize_channel`
- src/adapters/persistence/workflow/knowledge.rs `actor_level_on`, `destination_level_on`
