---
title: "Where each access rule is checked, and the mismatches between them that are still undecided"
author: Mac02
date: 2026-11-22
branch: plan08_ph1
cost: { minutes: 9, tokens: 251849 }
id: l-b98b6f34
scope: repo
status: active
src: agent
touches:
  - migrations/20260817000000_init_schema.sql
  - src/adapters/persistence/department_desks.rs
  - src/adapters/persistence/department.rs
  - src/application/use_cases/department.rs
  - src/domain/entities/company_member.rs
  - src/adapters/persistence/workflow/authority.rs
  - src/application/workflow/authorization.rs
  - src/adapters/persistence/workflow/controls.rs
  - src/adapters/persistence/knowledge/permissions.rs
  - src/adapters/persistence/unit_budgets.rs
---

## Decided
Nothing was changed. A read-only review of the access rules (22 Nov 2026) mapped where each rule is enforced and found places where the rules disagree. The owner has not ruled on any of them yet, so treat everything under Watch out as an open question, not as a decision to follow.

The rules reduce to a few shared checks:
- Managing a unit (configuration): owner, admin, or the lead or deciding sponsor of the unit or any ancestor. Covers the whole subtree.
- Seeing and writing on a desk: the unit's members and all its role holders, plus the lead one step up and one step down. The owner is always exempt; an admin is not.
- Reading knowledge by hand: own placements and their ancestors, plus the managed subtree. Owner and admin read everything.
- Agent run memory and knowledge: the desk's unit and its ancestors, plus direct children when the agent holds lead.
- Using a skill, connection or model: the unit or any ancestor; for an agent, any unit it is placed in.

## Tried and ruled out
Nothing was tried; no code changed.

## Watch out
- Adding a member checks only that the actor manages the destination unit, and the picker offers every principal in the company. An agent is managed by whoever manages any unit it is placed in, and reaches the resources of each. So a unit manager can add another unit's agent and gain its authoring and publication.
- "Company-level" has two meanings. The domain check is owner or admin. The database check is "manages the root", which also admits a root lead and a root deciding sponsor. Run cancel and retry, local agents, memory settings and workflow tools use the database one.
- Run cancel and retry: the application preflight asks for managing the desk's unit plus desk view; the database command asks for managing the root plus view. A unit lead passes the first and is refused by the second.
- The deciding sponsor counts as a manager in the management check only. The one-step desk grant and the child reads for knowledge and run scopes still name the lead role literally.
- Role fallback picks the nearest ancestor holder at any distance, but acting on a decision needs desk view, which the hierarchy grants one step only and to leads only.
- A unit's own manager sets that unit's spend cap, while its name, roles and archive belong to the parent's manager.
- Three different default-unit rules exist for knowledge uploads, resources and workflow definitions. A lead who is also a member of another unit defaults to the root for a new workflow definition and is refused.

In the code:
- migrations/20260817000000_init_schema.sql: unit_management_role, any_unit_management_role, agent_management_role, agent_reaches, unit_reaches, resource_default_unit, workflow_creating_department, workflow_definition_management_role, workflow_follow_reviewer_eligible
- src/adapters/persistence/department_desks.rs: ELIGIBLE (derived desk grants)
- src/adapters/persistence/department.rs: add_member; src/application/use_cases/department.rs: principal_choices
- src/domain/entities/company_member.rs: administers_company; src/adapters/persistence/workflow/authority.rs: authorize_company
- src/application/workflow/authorization.rs: LifecycleAuthorizer; src/adapters/persistence/workflow/controls.rs: control
- src/adapters/persistence/knowledge/permissions.rs: load, ordinary
- src/adapters/persistence/unit_budgets.rs: authorize_on
- src/domain/entities/channel.rs: viewer_access; src/domain/entities/participant.rs: direct_viewer_access
