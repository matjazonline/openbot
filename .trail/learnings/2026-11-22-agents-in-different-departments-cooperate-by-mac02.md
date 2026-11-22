---
title: "Agents in different departments cooperate by asking each other, limited by department and level on the request, never by reaching into the other department's d…"
author: Mac02
date: 2026-11-22
branch: plan08_ph1
cost: { minutes: 1, tokens: 4004 }
id: l-75975006
scope: repo
status: active
src: agent
touches:
  - src/adapters/persistence/workflow/knowledge.rs
  - src/domain/entities/participant.rs
  - src/adapters/persistence/workflow/run_scopes.rs
  - src/adapters/persistence/department_desks.rs
  - src/adapters/persistence/knowledge/permissions.rs
---

## Decided
Agreed with the user on 22 Nov 2026, as the design for cross-unit agent work.

- One agent asks another; the asked agent runs with its own unit's data and tools. Only its answer crosses.
- The limit sits on the request rule: which unit may ask, and up to which level. The asked run's ceiling is the lower of the caller's ceiling and that cap, so nothing above the cap is read.
- The hierarchy gives the default rules: an agent may ask agents in its own unit and in any ancestor, and a lead may ask one step down. Siblings and cousins need a rule, added by the manager of the unit that owns the data. A rule for a unit covers the units below it.
- Agent addresses follow the same rules.
- One company-wide level scale. Level is the only label that travels with content.

## Tried and ruled out
- Direct reach (an agent reads another unit's data and runs its tools): the asking side would drive the owner's tools, both units' data mix in one run, and it needs grants, their inheritance and mid-run revocation. The user has no case that needs it.
- A different level per department for a person or agent: a run tracks what it has seen as one number, so a level must mean the same everywhere.
- A second label naming the origin department on every message and memory: too heavy.

## Watch out
"Finance to Sales only" cannot be expressed: once an answer reaches another unit, any reader at that level may later see it. The user accepted this; departments mark such data above the cap. When two units both need a tool, move it to their common parent.

In the code:
- `src/adapters/persistence/workflow/knowledge.rs`: `admission_levels_on` sets a run's ceiling
- `src/domain/entities/participant.rs`: `direct_access`
- `src/adapters/persistence/workflow/run_scopes.rs`
