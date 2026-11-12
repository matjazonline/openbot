# Departments

A department is a named group inside one company. It owns desks, has people and agents as members,
gives one of them the `lead` role, carries a charter, and may sit under a parent department. Desks
are what the code calls channels: an address where work arrives, with workflow bindings deciding
who handles it. Departments nest; desks do not.

Owners and admins manage departments on the company's **Departments** tab
(`/ui/companies?company_id={company}&tab=departments`) or through the JSON API below. A plain member
sees, read-only, the departments they belong to or lead; a write from them is answered `404`, as
for someone outside the company.

## What a department holds

| Field | Meaning |
| --- | --- |
| `name` | What people call it. |
| `slug` | Unique in the company; the address its own desk is created at (`slug@{company}.{application-domain}`). Desk address rules apply. Editing it later does not move the desk. |
| `description` | One line, optional. |
| `charter` | What the department is for, at most 4096 bytes. Agents answering on its desks read it (see "Leads"). |
| `parent_id` | The parent department, or none for a top-level one. Departments nest at most 8 levels; a change that would form a cycle is refused. |

**Members** are principals: the company's people and agents. A principal may be in several
departments. Membership grants nothing by itself; it is a fact the desk rules below read.

**Roles** are held by principals. The only role is `lead`, with at most one holder: assigning a
new lead replaces the old one, and clearing it leaves none. The lead need not be a member. An agent
lead must be a company agent that can be published.

## Desks and ownership

A desk can belong to one department (`department_id`). Ownership alone changes nobody's access:
the desk keeps the access its own settings give it (the company team, its Participant Emails, or
`@public`), whichever department owns it. Ownership is set
on the desk's settings (the **Department** select) or with the desk JSON fields `department_id`
(`null` detaches) and `members_only`; leaving both out of a PUT keeps them.

### Members-only desks

A desk marked **members only** must belong to a department. It is an allowlist desk whose
grants are derived from the department, in the same transaction as every change that affects
them:

- the department's members and its role holders;
- the holder of the parent department's `lead` role;
- the `lead` of each direct child department.

Nothing further up or down is granted: one step of the hierarchy each way. The addresses in the
desk's Participant Emails still have access as before, and the company owner keeps access as on
every desk; a members-only desk cannot be open to `@public`. Everyone else in the company, and
anyone outside it, is refused.

Messages that other desks in the company hand to a members-only desk are still admitted, bound by
the usual hop budget and loop refusal (see `inter_channel_agent_communication.md`): the
restriction is on people and outside senders, not on desk-to-desk relays.

### Agents' own desks

Every company agent gets its own desk when it is created, on every creation path (the agents page,
the JSON API, a library copy, onboarding, the Agent Builder): a team desk at the agent's slug,
answered by that agent through the autonomous workflow. Creating the agent needs a model
connection with an API key, because the desk answers on an explicit model; a taken address refuses
the whole creation. The agent's settings show this desk's address and the departments it is in;
"Create additional desk" adds more. Library agents get no desk until copied into a company.

An agent's own desk belongs to no department, even when the agent is a member of one.

### The department's desk

Creating a department creates its desk at the department's slug, in the same transaction; a taken
address refuses the creation. With an agent lead the desk is answered by that agent through the
autonomous workflow; with a person lead, or none, it is a plain team desk.

Changing the lead re-points the department-owned binding on the desk to the new agent lead, or
pauses it for a person lead or none; the binding then runs as the admin who changed the lead. A
plain desk that gets an agent lead is given the autonomous workflow, so it starts answering. A desk
whose responder someone chose on its settings is left as they set it. The change is refused, and
nothing moves, when the desk asks reviewers to approve replies, when the admin cannot see the desk,
or when its workflow uses resources, has edits the desk does not run, or has no agent reply step to
hand over. Detach the desk, or fix its workflow, first.

## Leads

An `agent.run` on a desk a department owns gets the department's context as system text after
the agent's own instructions: the department's name and charter, that the agent answers as the
department's lead when it holds the `lead` role, and — on a desk that is not public — the desks
around the department: its own desk, its parent's, its direct children's and its agents' own desks,
each by address (at most 20 children and 50 agents, with a note when there are more). People are
not listed. The context is frozen with the run's prepared request, so a retry sees what the first
attempt saw.

The Agent Builder's `list_company_agents` tool names each agent's departments, roles and own
desk and can list one department (`department` slug); it stays an owner's or admin's tool. The
plan's intake schema for a member's desk arrives with `channel.call`; the directory does not carry
one yet.

## Dispatch and escalation

A lead hands work to a member, or escalates to the parent department, by sending a message to that
desk's address. Today such a message is delivered as an outbound message on the target desk; it
does not start the target desk's workflow, and a reply that comes back along the same trace is
refused as a loop. Dispatch that starts the member's work and returns an answer waits for
`channel.call` (agent-workflow-call).

## Where departments show up

- **Desk lists** group desks under their department, root departments first, "No department"
  last, and badge each owned desk.
- **Run board** (`/ui/workflow-runs`): a department filter beside the workflow and agent filters
  (runs on the desks the department owns now), and a **Waiting on a person** view listing the
  open decisions and the run problems escalated to a person, grouped by department.
- **Work queue** (`/ui/work`) and the **dashboard**'s outstanding jobs: grouped by department,
  with a department filter. On the dashboard the filter narrows the outstanding jobs; the counts and
  charts stay the whole company's.
- **Model usage**: "Group by Department" shows spend by the department that owns each run's desk
  now. Archived usage keeps no run and counts under "No department".
- **`_help` replies and undeliverable-address bounces** list the desks a teammate may write to
  under their department's heading, root first.
- **JSON**: desk responses carry `department_id` and `members_only`; agent responses carry
  `departments` (`id`, `name`, `slug`, `roles`) and `more_departments`.

Owners and admins see every department in these filters and headings. A plain member sees the
departments they belong to or lead and the department of each desk they can read, so the
department of a desk in front of them is always named; any other department is not found as a
filter.

## Delete

A department is deleted only when it owns no desks and has no child departments: detach or delete
its desk (including its own desk) and move or delete its children first. Its members and role
rows go with it.

## JSON API

All under `/api/companies/{company_id}`. Writes are for owners and admins; reads are open to a
plain member for the departments they belong to or lead. Anything else is answered `404`, as for
someone outside the company.

| Method and path | Does |
| --- | --- |
| `GET /departments` | Lists the departments the caller can see. |
| `POST /departments` | Creates one: `name`, optional `slug` (from the name when left out), `description`, `charter`, `parent_id`, `lead` (a principal id). |
| `GET /departments/{department_id}` | One department with its `members` and `roles`. |
| `PATCH /departments/{department_id}` | Changes only the fields present; `null` clears an optional field, and for `parent_id` makes it top level. |
| `DELETE /departments/{department_id}` | Deletes it (see "Delete"). |
| `PUT` / `DELETE /departments/{department_id}/members/{principal_id}` | Adds or removes a member. |
| `PUT` / `DELETE /departments/{department_id}/roles/lead/{principal_id}` | Assigns or clears the lead. |

Refusals: `409 Conflict` for a taken slug or a department that still owns desks or has children;
`400 Bad Request` for a cycle, too deep a tree, or one of the company's principals that is neither
a person nor an agent; `404 Not Found` for a principal or parent department that is not the
company's.
