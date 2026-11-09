# 06 Delegated administration: a lead manages its unit

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed, 2026-11-09. Sixth of the organization plans; depends on
[01-departments](../01-departments/README.md), [02-workflow-roles](../02-workflow-roles/README.md)
and [04-root-unit](../04-root-unit/README.md). Followed by
[07-unit-resources](../07-unit-resources/README.md).

## Intended behavior

Every management action today is gated by one question: is the actor the company owner or an
admin (`CompanyMembership::manages_company_operations`,
`src/domain/entities/company_member.rs:85`, used by `authorize`,
`src/application/workflow/authorization.rs:150`, and `managed_company`,
`src/application/use_cases/company.rs:223`). A company with ten departments then has a handful of
people doing every configuration change. After this plan the question is: does the actor manage the
unit this thing belongs to?

A person who holds `lead` in a department manages that department: its desks and their settings,
bindings and schedules; its members and roles; its child departments; the agents that belong to it;
its knowledge, memory settings and, after 07, its resources. Managing a unit includes everything
below it. The company owner manages the root and therefore everything. A root `admin` manages the
root as well, which is what admin meant.

## Decisions (user, 2026-11-09)

1. **One predicate, `manages_unit(actor, unit)`.** True when the actor is the owner, holds `admin`
   on the root, or holds `lead` on the unit or any ancestor. It replaces
   `manages_company_operations` everywhere that check guards a unit-owned thing; a company-level
   thing (billing, model connections, memory tenant, invites to the root) is "the root unit".
2. **Agents are managed through their departments.** An agent in one or more departments is managed
   by anyone who manages any of them. An agent in none is managed only by root managers. Agent
   authoring, publication, tool grants and deletion all go through this.
3. **Procedures are managed where they are bound.** A workflow definition is managed by whoever
   manages a unit that one of its active bindings belongs to; an unbound definition by its creating
   department, recorded at creation, defaulting to the root. Publishing a version that binds to a
   desk in a unit the actor does not manage is refused.
4. **A lead cannot widen its own authority.** Role assignment on a unit requires managing its parent
   (the root's roles require the owner). A lead may assign roles in child departments and add or
   remove members of its own department, but not make itself lead of a sibling or ancestor.
5. **Visibility is unchanged.** Managing a unit does not grant reading a members-only desk the
   manager is not on; the one-step desk grants from the departments plan stand. A manager sees
   configuration, not conversations, unless desk grants say otherwise.
6. **Agents acting as leads do not administer.** The predicate takes a person. An agent lead
   coordinates work; configuration is done by people. `WorkflowActor` for an agent run therefore
   never satisfies `manages_unit`.

## What does not change

- Desk and thread read policy, access levels, and the owner exemption.
- Company creation, deletion, billing and the memory tenant: owner only, as today.
- The JSON API shapes; only who may call them.

## Documents

1. [The predicate and its owners](01-predicate-and-owners.md).
2. [Routes, UI and audit](02-routes-ui-and-audit.md).
3. [Integration and final verification](03-integration-and-final-verification.md).
