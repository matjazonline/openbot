# Agent tools

An agent's published revision freezes its approved tool bindings, aliases, ordered skills and
supporting skill files. Each call uses the application's action service and current authority;
a frozen definition does not preserve a revoked grant. See [Agent authoring](agent-authoring.md)
for the UI and JSON assignment contract.

An agent can receive these tool sources:

- Built-ins such as message send/reply and knowledge search, within the run's actual target and
  source-message authority.
- Opt-in built-ins that nothing grants automatically: `channel.call` (ask another desk and wait for
  its answer; see [Workflow operations](workflow-operations.md#channel-calls)) and `goal.status`
  (read the status of the goal the calling run serves, for an agent holding the goal's lead or
  sponsor role; it takes no arguments and never acts; see [Departments](departments.md#goals)).
- Company HTTP and MCP operations, with an approved connection, reviewed operation and exact
  per-agent assignment. Discovery alone grants no execution authority.
- Registered workflow tools, pinned to an immutable workflow version and parameter configuration.

Every company HTTP connection, MCP connection, model connection and skill belongs to a department
(see [Departments](departments.md#resources)). An agent may be assigned, and may run, only those
of its own departments or the departments above them (the root's when it is in none): editing an
agent offers no others, a new agent's form lists every model with its department and refuses one
out of reach, publication refuses one out of reach with `publication.resource_outside_unit`,
and a run is refused with `admission.resource_outside_unit` once one has moved out of reach. A
native session checks its bundle's tools and skills the same way whenever it is selected.

Skills supply instructions and supporting files. Importing them executes nothing on the server;
a native session receives its frozen bundle under the selected execution policy. The agent's
`native_permissions` chooses `auto`, `accept_edits` or `full` for native execution and grants no
additional hosted workflow tools. Direct-address public access is a separate admission control.

## Agent Builder

The server installs an `agent-builder` library definition after migrations when its slug is absent.
Its trusted provisioning tools are available only to company owners/admins writing in the app:

- `list_company_agents` lists agents, their departments and roles, and their principal addresses.
  The `department` slug filter limits entries to that department's members and role holders.
- `create_agent` creates the agent, its canonical principal address and active published default
  workflow binding in one transaction. `department` and `role` place the agent in the organization.
  Its model, tools and skills must belong to that department or one above it (the root's when it
  names none); `list_company_agents` names the department owning each model.
- `create_desk_for_agent` creates an optional shared desk answered by an existing agent. The
  requester becomes the binding's run-as member. The desk has its own access settings and address.

The ordinary agent authoring API cannot acquire Builder privileges. The UI uses the same
provisioning owners and offers **Create additional desk** beside the agent's direct address.
A direct address is separate from the desks the agent staffs; public access to either is configured
independently. See [Agent authoring](agent-authoring.md#direct-address-and-shared-desks).
