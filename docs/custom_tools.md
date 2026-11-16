# Agent tools

An agent's published revision freezes its approved tool bindings, aliases, ordered skills and
supporting skill files. Each call uses the application's action service and current authority;
a frozen definition does not preserve a revoked grant. See [Agent authoring](agent-authoring.md)
for the UI and JSON assignment contract.

An agent can receive these tool sources:

- Built-ins such as message send/reply and knowledge search, within the run's actual target and
  source-message authority.
- Company HTTP and MCP operations, with an approved connection, reviewed operation and exact
  per-agent assignment. Discovery alone grants no execution authority.
- Registered workflow tools, pinned to an immutable workflow version and parameter configuration.

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
- `create_desk_for_agent` creates an optional shared desk answered by an existing agent. The
  requester becomes the binding's run-as member. The desk has its own access settings and address.

The ordinary agent authoring API cannot acquire Builder privileges. The UI uses the same
provisioning owners and offers **Create additional desk** beside the agent's direct address.
A direct address is separate from the desks the agent staffs; public access to either is configured
independently. See [Agent authoring](agent-authoring.md#direct-address-and-shared-desks).
