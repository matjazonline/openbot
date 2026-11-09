# Departments: groups of desks and agents with a lead

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed implementation, 2026-11-09. These documents do not implement the feature.

## Intended behavior

A company can organize its work into departments. A department is a named group inside one
company that:

- owns desks (what the code calls channels: an address where work arrives, with workflow
  bindings deciding who handles it);
- has members, which are principals: agents and people;
- has one lead, an agent or a person, who coordinates the department's work;
- may sit under a parent department, so departments form a hierarchy. Desks do not nest.

Coordination is ordinary workflow behavior on ordinary desks. A department's own desk receives the
department's work, and when the lead is an agent, the desk is answered by that agent through the
same autonomous workflow every agent desk uses. The lead dispatches to members by messaging their
desks and escalates by messaging the parent department's desk. No new runtime, scheduler or
routing mechanism is introduced.

The three primitives keep their meaning:

| Primitive | Role in the organization | Code |
| --- | --- | --- |
| Department | groups, owns, nests | new `departments`, `department_members` |
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
4. **The lead is a principal and coordination is a workflow.** The lead is an agent or a person. An
   agent lead answers the department desk through the standard autonomous base workflow and
   dispatches with the existing `message.send` step. The synchronous `channel.call` planned in
   [agent-workflow-call](../agent-workflow-call/README.md) is a later improvement, not a dependency.
5. **Members are principals, so a department can hold agents and people.** A principal may belong
   to several departments. Membership grants nothing by itself; it is a fact other rules read.
6. **A desk owned by a department is reachable by the whole company unless it is members-only.**
   Ownership and access are separate. A members-only desk is an allowlist channel whose grants are
   materialized from department membership (new provenance `department_membership`) in the same
   transaction as the membership change, so `Channel::participant_access` and
   `Channel::viewer_access` are not modified.
7. **Hierarchy grants one step of access.** A members-only desk's derived grants cover the
   department's members, its lead, and the lead of its parent department. Nothing is inherited
   further up or down. Escalation and dispatch therefore work between adjacent levels, and a
   company-wide view still needs the owner exemption or an explicit grant.
8. **Creating a department creates its desk.** Slug and name are the department's. With an agent
   lead the desk is provisioned by `assistant_channel_on` with the autonomous reply mode, as the
   agent default channel is; with a person lead or no lead it is a plain team desk. A taken
   address refuses the whole creation.
9. **Delete is conservative.** A department that owns desks or has child departments cannot be
   deleted; detach or delete those first. Membership rows go with the department.
10. **Departments carry no access level.** Knowledge and memory levels stay on the member.
    Department-scoped memory namespaces and SOP roles that name a department lead are follow-ups
    for [memory-service](../memory-service/README.md) and
    [standard-operating-procedures](../standard-operating-procedures/README.md); this plan gives
    them a stable `department_id` to scope by and nothing more.

## What does not change

- The company remains the tenant and the access boundary. `company_members`, roles and access
  levels are untouched.
- Every agent still gets its own default desk at its slug. A department does not replace it.
- Channel policy (`src/domain/entities/channel.rs:135` and `:174`) is unchanged; departments feed
  it grant rows, nothing else.
- Workflow bindings stay on channels. A department's jobs are its desks' bindings; no
  `department_id` is added to workflow tables.
- Managing departments requires the same authority as managing channels today: an owner or admin
  (`managed_company`, `src/application/use_cases/company.rs:223`).

## Documents

1. [Departments and membership](01-departments-and-membership.md): schema, domain, persistence,
   use cases and JSON API.
2. [Desks, ownership and access](02-desks-ownership-and-access.md): `channels.department_id`,
   members-only desks, and the desk vocabulary.
3. [Leads and coordination](03-leads-and-coordination.md): the department desk, agent leads,
   dispatch, escalation and the agent directory.
4. [UI and documentation](04-ui-and-docs.md).
5. [Integration and final verification](05-integration-and-final-verification.md).
