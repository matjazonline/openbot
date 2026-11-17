# Departments

A department is a named group inside one company. It owns desks, has people and agents as members,
gives one of them the `lead` role, carries a charter, and may sit under a parent department. Desks
are what the code calls channels: an address where work arrives, with workflow bindings deciding
who handles it. Departments nest; desks do not.

Every company has exactly one root department, created in the company's transaction. Company
membership is membership of that root. Company access levels live on root member rows; admin
status comes from the root `admin` role. The company owner remains `companies.user_id`, mirrored
by the root's single `owner` role. The company remains the tenant boundary: the root is inside
that boundary and cannot be deleted or re-parented.

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
| `parent_id` | The parent department; only the root has none. Omitting it for a new department selects the root. Departments nest at most 8 levels; a change that would form a cycle is refused. |

**Members** are principals: the company's people and agents. A principal may be in several
departments. Membership materializes access to the owning department's desks according to the one-step rule below; root membership alone does not give membership of every child department.

**Roles** are held by principals. On ordinary departments, `lead` has at most one holder: assigning a new
lead replaces the old one, and clearing it leaves none. An agent lead must be a company agent that
can be published. `reviewer` may have several holders. A holder need not be a member. Workflows can
name a role instead of a fixed person or agent (see [Roles](workflow-language-v1.md#roles)), and a
desk's workflow binding can escalate to a role's holders (see "Escalation"). The root also has
`owner` and `admin`: they cannot be placed on child departments. The owner mirror cannot be
cleared or edited independently. Root team/admin mutations retain the Team page's owner-only
policy; ordinary department management remains available to admins.

Deleting an agent, or removing a person from the company, clears every role they hold in the same
transaction and records each cleared role as a department administration event. Nobody is
appointed in their place. Until someone is, a workflow role step and an escalation's first hop
fall back to the nearest ancestor department with a holder of that role; when there is none, the
step is refused with `workflow.role_no_holder` and the escalation moves on as "Escalation"
describes.

## Desks and ownership

Every desk belongs to one department (`department_id`), the root by default. The **Department**
select is required. The JSON create API may omit the owner to select root; a PUT may omit it to
preserve the current owner. Explicit `null`, a missing stored owner or an owner from another
company is refused. Reassign a desk to another department instead of clearing its owner.

### Members and Public

Every desk carries department-derived `participate` and `view` grants for:

- the department's members and all its role holders;
- the holder of its parent department's `lead` role; and
- the `lead` of each direct child department.

Nothing further up or down is granted: one step of the hierarchy each way. Membership, role,
tree and ownership changes update these grants in the same transaction, including on root and
Public desks. Unchanged grants and explicit grants retain their identity and provenance.

A **Members** desk admits this audience plus the named addresses under **Also allow these
addresses**, with the company owner exempted. A root desk therefore admits root members; a
child desk does not admit every company member. Adding addresses adds access and never narrows
the department audience. A signed-in member keeps department grants when their email is
unverified; additional email-based permissions require proof of their current address.

**Open to anyone** selects **Public** intake (`@public` in the JSON email list). Anyone may send
mail to it, while explicitly granted senders and current company members are trusted. Public
intake does not grant outsiders UI reads: reading still requires current company membership.
Members desks additionally require a view grant, with the company owner exempted.

Internal relays obey the receiving Members desk's grants, including on root desks. The actual
authenticated sending agent must hold a receiving `participate` grant through membership, a
qualifying role or explicit permission. A desk's address alone is not agent proof. Tenant,
source-identity, incarnation, hop-budget and loop checks remain enforced (see
[internal communication](inter_channel_agent_communication.md)). This admission does not itself
grant access to the desk's history.

### Agents' direct addresses

Every company agent gets a principal address at its slug and an active published workflow binding
when created through the agents page, JSON API, library copy, onboarding or Agent Builder. This
creates no desk. A keyed model connection is required and a taken address refuses the whole
transaction. Agent settings show the direct address and departments. **Create additional desk**
creates an optional shared desk answered by that agent. Library agents receive company addresses
when copied into a company.

The agent's direct address has its own **Allow external messages** setting. A private agent can
answer a public department desk without making its direct address public. Department membership
and role-derived desk grants apply to an authenticated agent's relay identity.

### The department's desk

Creating a department creates its desk at the department's slug, in the same transaction; a taken
address refuses the creation. With an agent lead the desk is answered by that agent through the
autonomous workflow; with a person lead, or none, it is a plain Members desk.

Changing the lead re-points the department-owned binding on the desk to the new agent lead, or
pauses it for a person lead or none; the binding then runs as the admin who changed the lead. A
plain desk that gets an agent lead is given the autonomous workflow, so it starts answering. A desk
whose responder someone chose on its settings is left as they set it. The change is refused, and
nothing moves, when the desk asks reviewers to approve replies, when the admin cannot see the desk,
or when its workflow uses resources, has edits the desk does not run, or has no agent reply step to
hand over. Reassign the desk, or fix its workflow, first.

## Leads

An `agent.run` on a desk a department owns gets the department's context as system text after
the agent's own instructions: the department's name and charter, that the agent answers as the
department's lead when it holds the `lead` role, and — on a desk that is not public — the desks
around the department: its own desk, its parent's, its direct children's and its agents' principal addresses,
each by address (at most 20 children and 50 agents, with a note when there are more). People are
not listed. The context is frozen with the run's prepared request, so a retry sees what the first
attempt saw.

The Agent Builder's `list_company_agents` tool names each agent's departments, roles and direct
address and can list one department (`department` slug); it stays an owner's or admin's tool. The
plan's intake schema for a member's desk arrives with `channel.call`; the directory does not carry
one yet.

## Dispatch and escalation

A lead hands work to a member, or escalates to the parent department, by sending a message to that
desk's address. Today such a message is delivered as an outbound message on the target desk; it
does not start the target desk's workflow, and a reply that comes back along the same trace is
refused as a loop. Dispatch that starts the member's work and returns an answer waits for
`channel.call` (agent-workflow-call).

## Escalation

A workflow binding on a department-owned desk can opt into escalation in its configuration:

```json
"escalation": {"notify": "lead", "hops": 2}
```

`notify` is `lead` or `reviewer`; `hops` is 1 or 2. Without it nothing escalates. Activating a
binding that escalates on a desk without a department, or detaching the department from a desk
with such a binding active, is refused with `binding.escalation_needs_department`.

A binding's work escalates when it fails and no authored error route handles the failure:

- a role refusal at admission (no run is created);
- a run failed by a guard, the wait sweeper, its deadline or recovery;
- a `decision.human` past its deadline, or a rejected or expired action approval or token
  allowance;
- a refused delivery, final response or local agent call;
- a child failure that no parent route consumes, escalated once from the root parent, never from
  the child;
- a parked decision that follows its role and is left with no reviewers.

Each run, or refused admission, opens at most one case, and the case keeps its first reason. A
user cancelling a run does not escalate, and work started by an escalation notice never opens
another case.

- **Hop 1** notifies every holder of the `notify` role in the desk's department or, when it has
  none, in the nearest ancestor that has holders. More than 16 holders is refused as a recipient
  limit rather than notifying some of them.
- If hop 1 reaches nobody who can receive it, or nobody settles it within `ESCALATION_ACK_HOURS`
  (default 24), and `hops` is 2, **hop 2** notifies the parent department's lead.
- After the last hop the case waits for the company owner in **Waiting on a person**. Nothing is
  sent a third time.

People get an in-app notification, and email unless they opted out of it. Agents get an internal
message on their exact principal binding and act through the `read_escalation` and `respond_to_escalation` tools
when their published tools include them; a successful run is not an acknowledgement. A person's
notice only says that something needs attention and links to the escalation decision, which shows
the run, the step and the reason. An agent's notice also names the reason code and the step.

The escalation decision (`/ui/workflow-decisions/escalations/{decision_id}`) offers
**Acknowledge**, **Reassign** to another person or agent of the company, and **Cancel** the run.
The current recipients and the company's owners and admins can acknowledge or reassign;
cancelling also needs permission to control that run and is unavailable for a refused admission.
Reassigning stops the automatic ladder and notifies the new assignee, who can acknowledge or
reassign again, up to 32 assignments per case. A role grants no other access: a holder still
needs to be able to see the run to act on it.

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
- **`_help` replies and undeliverable-address bounces** list writable desks
  under their department headings, followed by **People and agents** with writable principal addresses.
- **JSON**: desk responses carry required `department_id` and `access_mode` (`members` or `public`); agent responses carry
  `departments` (`id`, `name`, `slug`, `roles`) and `more_departments`.

Owners and admins see every department in these filters and headings. A plain member sees the
departments they belong to or lead and the department of each desk they can read, so the
department of a desk in front of them is always named; any other department is not found as a
filter.

## Delete

The root cannot be deleted. A child department is deleted only when it owns no desks and has no child departments: reassign or delete
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
| `PATCH /departments/{department_id}` | Changes only the fields present; `null` clears an optional field, and for a child's `parent_id` selects the root. The root cannot be re-parented. |
| `DELETE /departments/{department_id}` | Deletes it (see "Delete"). |
| `PUT` / `DELETE /departments/{department_id}/members/{principal_id}` | Adds or removes a member. |
| `PUT` / `DELETE /departments/{department_id}/roles/{role}/{principal_id}` | Assigns or clears a role: `lead` replaces the current lead; `reviewer` adds or removes that holder. |

Refusals: `409 Conflict` for a taken slug or a department that still owns desks or has children;
`400 Bad Request` for a cycle, too deep a tree, or one of the company's principals that is neither
a person nor an agent; `404 Not Found` for a principal or parent department that is not the
company's.
