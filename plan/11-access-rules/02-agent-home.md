# 02 Agent home

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

An agent belongs to one unit, and only people who manage the agent can place it anywhere. Needs
proposal 2 of the [README](README.md). Follows [01](01-unit-head.md); continues in
[03](03-requests-between-units.md).

## Where it stands

Adding a member checks only the destination: `add_member`
(`src/adapters/persistence/department.rs:735`) asks whether the actor manages that unit, and
`check_principal_on` (`:331`) asks only that the principal belongs to the company. The picker,
`principal_choices` (`src/application/use_cases/department.rs:597`), offers every principal of
the company to anyone who manages any unit.

Two rules then follow the new placement. `agent_management_role` (init schema, line 20630) lets a
person manage an agent through any unit the agent is placed in. `agent_reaches` (15900) lets the
agent use resources of any unit it is placed in and of that unit's ancestors, archived units
included.

So the manager of one unit can add another unit's agent, then edit it, publish it and attach
their own connections to it.

## Batch 02A One home

1. An agent has exactly one membership, its home. Enforce it in the database with one membership
   row per agent principal. An agent created with no unit chosen is a member of the root. Roles
   in other units stay possible: a planning agent homed in the goals office can lead a goal unit.
2. Moving an agent's home needs a person who manages the agent and manages the destination.
   Giving an agent a role in a unit needs the existing rule for role writes (the parent's
   manager) and a person who manages the agent. Both are rechecked under the lock where
   `add_member` and the role writes recheck today, so a request that skipped the use case is
   refused the same way.
3. `agent_management_role`: the manager of the home unit, and nobody else. A role held
   elsewhere gives that unit's manager no authority over the agent.
   `workflow_definition_management_role` (20657) follows it for agent principals.
4. `agent_reaches`: the home and its ancestors, and each unit where the agent holds a role with
   that unit's ancestors. Archived units reach nothing. A role now carries the consent of the
   agent's manager, so this reach is no longer something a stranger can create.
5. A run with no desk uses the acting agent's home. This replaces "the single unambiguous
   placement, otherwise the root".

## Batch 02B The picker

`principal_choices` offers the people of the company, as today, and only the agents the actor
manages.

## Focused checks

- `agent_home_`: a second membership for one agent is refused by the constraint; creation with
  no unit lands on the root; a move needs both managers.
- `agent_home_takeover_`: the manager of unit B cannot add unit A's agent as member or role
  holder and gets `NotFound`, as a stranger does. After A's manager makes the agent lead of a unit
  under B, B's manager still cannot edit or publish it. The persistence recheck refuses a write
  that skipped the use case.
- `agent_home_competing_`: two moves of one agent leave one home; a move racing a publish leaves
  the publish either refused or applied under the old home's manager.
- Existing `delegated_agent_management_` and `run_scope_` families pass with their fixtures
  updated to one home.
