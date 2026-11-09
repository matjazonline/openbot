# 01 Departments: groups of desks and agents with roles

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed implementation, 2026-11-09, amended the same day to store the lead as a role
and provision an automatic desk for each company agent.
These documents do not implement the feature.

This is the first of seven plans that reshape the organization model. The later six build on it
and are implemented in order, each as its own resettable step:
[02-workflow-roles](../02-workflow-roles/README.md),
[03-principal-addresses](../03-principal-addresses/README.md),
[04-root-unit](../04-root-unit/README.md),
[05-scopes](../05-scopes/README.md),
[06-delegated-administration](../06-delegated-administration/README.md),
[07-unit-resources](../07-unit-resources/README.md).

## Intended behavior

A company can organize its work into departments. A department is a named group inside one
company that:

- owns desks (what the code calls channels: an address where work arrives, with workflow
  bindings deciding who handles it);
- has members, which are principals: agents and people;
- has a charter: one bounded text saying what the department is for, shown to its members and
  leads so they share the same framing;
- has roles held by principals. The only role this plan uses is `lead`, the one who coordinates
  the department's work; later plans add more and let workflows name them;
- may sit under a parent department, so departments form a hierarchy. Desks do not nest.

Coordination is ordinary workflow behavior on ordinary desks. A department's own desk receives the
department's work, and when the lead is an agent, the desk is answered by that agent through the
same autonomous workflow every agent desk uses. The lead dispatches to members by messaging their
desks and escalates by messaging the parent department's desk. No new runtime, scheduler or
routing mechanism is introduced.

The primitives keep their meaning:

| Primitive | Role in the organization | Code |
| --- | --- | --- |
| Department | groups, owns, nests, assigns roles | new `departments`, `department_members`,
`department_roles` |
| Desk | receives work at an address | existing `channels`, now with `department_id` |
| Workflow | decides who handles work, runs it | existing `workflow_*` |
| Agent | does the work | existing `agents`, already a principal |

## Decisions (user, 2026-11-09)

1. **A department is its own entity, not a channel.** A channel's grants mean "may participate or
   view here"; department membership means "works under this lead with this scope". The two stay
   in separate tables, linked by `channels.department_id`.
2. **Channels are called desks in the product.** The word changes in UI copy, help text and
   documentation. Database tables, Rust identifiers, URLs, JSON field names and the
   `X-MailAgents-Channel-*` headers keep `channel`; renaming them is a separate cleanup if ever
   wanted. "Office" was rejected because it reads as a place or tenant and collides with the
   department concept.
3. **Departments nest; desks do not.** `departments.parent_id` carries the hierarchy. Channels get
   no parent link, so routing, the relay loop trace and access checks stay flat.
4. **The lead is a role, held by a principal, and coordination is a workflow.** Roles live in
   `department_roles (department_id, role, principal_id)`. This plan defines one role, `lead`,
   with exactly one holder per department. The holder is an agent or a person. An agent lead
   answers the department desk through the standard autonomous base workflow and dispatches with
   the existing `message.send` step, and `channel.call` with its closure envelope once
   [agent-workflow-call](../agent-workflow-call/README.md) lands, so a member can decline work
   explicitly. Storing the lead as a role rather than a column is what lets
   [02-workflow-roles](../02-workflow-roles/README.md) let workflows name roles without a schema
   change here. The synchronous `channel.call` planned in
   [agent-workflow-call](../agent-workflow-call/README.md) is a later improvement, not a dependency.
5. **Members are principals, so a department can hold agents and people.** A principal may belong
   to several departments. Membership grants nothing by itself; it is a fact other rules read.
   A role holder need not be a member; the lead of a department is usually not one of its workers.
6. **A desk owned by a department is reachable by the whole company unless it is members-only.**
   Ownership and access are separate. A members-only desk is an allowlist channel whose grants are
   materialized from department membership and roles (new provenance `department_membership`) in
   the same transaction as the change, so `Channel::participant_access` and
   `Channel::viewer_access` are not modified.
7. **Hierarchy grants one step of access.** A members-only desk's derived grants cover the
   department's members, its role holders, and the holder of the parent department's `lead` role.
   Nothing is inherited further up or down. Escalation and dispatch therefore work between adjacent
   levels, and a company-wide view still needs the owner exemption or an explicit grant.
8. **Creating a department creates its desk.** Slug and name are the department's. With an agent
   lead the desk is provisioned by `assistant_channel_on` with the autonomous reply mode, as the
   agent default channel is; with a person lead or no lead it is a plain team desk. A taken
   address refuses the whole creation.
9. **Delete is conservative.** A department that owns desks or has child departments cannot be
   deleted; detach or delete those first. Membership and role rows go with the department.
10. **Stalled work has a default destination.** Escalation on failure, expiry or refusal is a
    binding option that notifies the `lead` role, specified in
    [02-workflow-roles](../02-workflow-roles/04-escalation-and-reassignment.md), not a line in the
    lead's prompt. This plan only makes sure every department desk has a department to resolve it.
11. **Departments carry no access level.** Knowledge and memory levels stay on the member.
    Department-scoped memory namespaces are [05-scopes](../05-scopes/README.md);
    this plan gives it a stable `department_id` to scope by and nothing more.
12. **Creating a company agent creates its default desk.** In phase
    [03](03-leads-and-coordination.md), every company-agent creation path provisions a team desk at
    the agent's initial slug, with a published autonomous workflow and an active binding on that
    agent, in the same transaction. Library agents get no desk until copied into a company. A
    department desk is separate, even when the same agent answers both.
    [03-principal-addresses](../03-principal-addresses/README.md) later replaces automatic agent
    desks with direct principal addresses after a database reset; no data conversion or parallel
    addressing path is required.

## What does not change

- The company remains the tenant and the access boundary. `company_members`, roles and access
  levels are untouched. [04-root-unit](../04-root-unit/README.md) revisits this.
- Channel policy (`src/domain/entities/channel.rs:135` and `:174`) is unchanged; departments feed
  it grant rows, nothing else.
- Workflow bindings stay on channels. A department's jobs are its desks' bindings; no
  `department_id` is added to workflow tables.
- Workflows still name a specific agent. Naming a role is
  [02-workflow-roles](../02-workflow-roles/README.md).
- Managing departments requires the same authority as managing channels today: an owner or admin
  (`managed_company`, `src/application/use_cases/company.rs:223`).
  [06-delegated-administration](../06-delegated-administration/README.md) revisits this.

## Documents

1. [Departments, membership and roles](01-departments-and-membership.md): schema, domain,
   persistence, use cases and JSON API.
2. [Desks, ownership and access](02-desks-ownership-and-access.md): `channels.department_id`,
   members-only desks, and the desk vocabulary.
3. [Leads and coordination](03-leads-and-coordination.md): automatic agent desks, the department
   desk, agent leads, dispatch, escalation and the agent directory.
4. [UI and documentation](04-ui-and-docs.md).
5. [Integration and final verification](05-integration-and-final-verification.md).
