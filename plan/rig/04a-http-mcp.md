# Step 4a: Company HTTP MCP catalog and agent selections

Dependencies: steps 2–4; durable invocation/recovery integration is completed with step 6.
This is planned functionality. Configure and store HTTP MCP definitions once per company. Each
agent selects zero or more definitions from its company's catalog. Agents store selection
references only; endpoint configuration and credentials remain company-owned, separate from model
connections and agent `config_json`.

## Configuration and persistence

Add typed `CompanyMcpConnection`, `AgentMcpSelection`, and `McpToolRef` application/domain values
and dedicated persistence ports. Proposed tables are `company_mcp_connections`,
`company_mcp_tool_grants`, and `agent_mcp_selections`. Use additive migrations and composite
company/connection and company/agent foreign keys so cross-company selections are impossible.
Store endpoint, credentials, enabled state, discovery metadata, and tool grants on the company
definition. Agent selections contain only company/agent/connection references, with selection
revision tracking for execution fencing; no copied settings or per-agent overrides.

Model a many-to-many relationship: one company definition can be selected by multiple agents, and
one agent can select multiple company definitions. Enforce unique `(company_id, slug)` definitions
and `(company_id, agent_id, connection_id)` selections. Effective MCP tools are the union of the
company-configured tools from the agent's selected, enabled definitions. A tool grant identifies its
company connection ID and exact remote tool name, not a display label. Keep these dynamic grants
separate from the closed built-in/native catalogue.

The proposed company catalog response with two MCP definitions is:

```json
{
  "mcp_connections": [
    {
      "id": "c04230c9-49bc-43db-8398-e7d1ab54ab13",
      "slug": "crm",
      "endpoint_url": "https://crm.example.com/mcp",
      "transport": "streamable_http",
      "enabled": true,
      "auth": { "type": "bearer", "secret_set": true },
      "tool_grants": [
        { "name": "search_contacts" },
        { "name": "create_note" }
      ]
    },
    {
      "id": "e6560e01-e1a0-4b87-bc69-b3838c6f370a",
      "slug": "knowledge",
      "endpoint_url": "https://knowledge.example.com/mcp",
      "transport": "streamable_http",
      "enabled": true,
      "auth": { "type": "none", "secret_set": false },
      "tool_grants": [
        { "name": "search_documents" }
      ]
    }
  ]
}
```

Connection creation accepts one definition without the server-assigned `id` or read-only
`secret_set`; updates target its company-scoped connection ID. The collection endpoint lists the
company's definitions. The agent's selection update contains only references:

```json
{
  "mcp_connection_ids": [
    "c04230c9-49bc-43db-8398-e7d1ab54ab13",
    "e6560e01-e1a0-4b87-bc69-b3838c6f370a"
  ]
}
```

Omitting `mcp_connection_ids` preserves selections; an empty list clears them. Validate every ID
against the agent's company and replace the selection set atomically with an expected revision.
Reject inline endpoint/auth/tool configuration in agent writes. Selecting a definition grants its
company-configured tool set; there is no separate per-agent MCP tool-settings editor.

Treat this as a versioned proposed connection schema until parsing and enforcement exist. V1
supports no-auth and bearer-token endpoints. Set/replace/delete the token through a separate
write-only credential operation; GET returns only authentication type and whether a secret is set.
Reuse existing envelope-encryption/key-rotation primitives under
`src/adapters/persistence/credentials/`, extending rotation coverage to the new credential rows.
Bind encrypted values to their owning connection/tenant. Tokens must never enter model arguments,
task checkpoints, traces, validation echoes, exports, or library definitions.

Require company-integration management authorization for definition, credential, discovery, and
tool-grant changes; agent-management authorization controls selections from that company's catalog.
Ordinary agent edits preserve selections. Company definition edits preserve tokens unless explicitly
replaced/removed, except endpoint changes as described below. Company edits affect every selecting
agent through reference resolution, without rewriting agent rows. Removing a selection or deleting
an agent must not delete the shared definition or its credentials.

Same-company agent copy may retain selections as references through the authorized copy operation;
cross-company copy/library export omits active selections and credentials. Templates may name
connection requirements for an operator to map to the destination company catalog. Native agent
provisioning must not inherit MCP selections automatically or create/change company definitions.

Proposed initial server-enforced bounds: 64 definitions per company, 8 selections per agent,
2,048 characters per URL, 16 KiB per bearer secret, 100 discovered tools per connection,
32 effective MCP tools per agent,
64 KiB per tool schema, and 1 MiB cumulative discovery data. Validate at write/discovery/run time;
oversized discovery fails visibly rather than silently selecting an arbitrary subset. Discovery
and invocation also share step 6's prompt, result, deadline, and connection-concurrency ceilings.

## Transport and endpoint policy

Use MCP Streamable HTTP, which supports JSON responses and SSE responses on the configured MCP
endpoint. Pin a compatible `rmcp` client with the selected Rig release and prove transport behavior
in step 2. Source: [MCP HTTP transport specification](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports),
[Rust MCP client transports](https://docs.rs/rmcp/latest/rmcp/transport/index.html).

V1 supports Streamable HTTP only; legacy HTTP+SSE and local stdio/process launch are not offered.
HTTP is the protocol family; production endpoints require HTTPS. Permit plain HTTP only through
an explicit deployment-controlled development/internal-endpoint policy, never a model argument.
Validate URLs and connected addresses, reject userinfo/fragments, and reject redirects. Reuse or
extract the existing guarded HTTP destination validation so DNS rebinding and private/metadata
addresses cannot bypass the outbound policy. If internal endpoints are needed, allow exact
destinations through operator policy; company configuration cannot widen it. Endpoint changes must
clear the bound secret and require explicit credential rebinding to the new destination.

Implement an application-owned MCP client port with `rmcp` types confined to an outer adapter such
as `src/adapters/mcp/`. It owns initialization, protocol negotiation, bounded discovery/calls,
authentication, sessions, and cancellation. Keep sessions scoped to company connection, credential
revision, agent, and logical run. Shared configuration does not imply shared mutable session state.
Supervise transport workers and close streams/sessions on cancellation and bounded shutdown.

Register no server-initiated sampling, filesystem roots, or elicitation capability in V1. MCP
resources/prompts are also separate capabilities, not implicitly enabled by a tool connection;
step 5's skill `read_resource` remains restricted to its skill catalog.

## Discovery, grants, and Rig dispatch

Provide an explicit **Test connection / Refresh tools** operation that performs initialization and
bounded `tools/list`, without invoking business tools. Saving validates configuration locally;
it must not be described as proof that authentication or the remote server works. Store bounded
discovery metadata and schema fingerprints for tool selection and change detection.

No newly discovered tool is automatically granted. Refresh and tool selection occur at company
scope; notifications may refresh metadata but cannot expand the company tool grants. Changes to
those grants affect all selecting agents, so validate aggregate per-agent tool limits before
committing the change. Fail preflight on selected, enabled connections with granted tools that
are unreachable, removed, or incompatible; disabled/ungranted connections cause no run-time network
work. Revalidate selected definitions against saved fingerprints before execution. A schema change
requires explicit refresh/review and cannot mutate a saved pending call.

Compile each granted tool's name, description, and input schema into a Rig dynamic declaration.
Generate deterministic provider-compatible names, such as `mcp_crm_search_contacts` with a stable
disambiguating suffix where needed, and retain the authoritative connection/tool mapping. Never
accept an endpoint URL, credential, or arbitrary remote tool name through a generic model router.
Server text is untrusted tool metadata/data and must not be promoted to system instructions.

Rig supplies MCP helpers, but our calls must pass through the application bridge rather than an
automatically populated tool server that bypasses grants. Source:
[Rig MCP integration](https://docs.rs/rig/latest/rig/tool/rmcp/index.html).

```text
company MCP catalog + agent's selected connection IDs
  → enabled selected definitions + company-configured tool grants
  → bounded discovery and schema validation
  → Rig model-facing tool declarations
  → model selects a tool
  → application grant / argument / lease / budget checks
  → durable invocation → MCP tools/call → durable result
  → model continues
```

Extend harness-neutral capability resolution and host declarations to support dynamic descriptions,
names, and MCP tool identities; current static native-tool fields cannot represent arbitrary remote
definitions. Preserve the built-in/native allowlist. Skill prompt instructions can direct the model
to exposed MCP tools; direct stored `SkillInstruction::Tool` references to MCP require a separately
validated connection-scoped reference design before being advertised. Do not loosen the current
static skill tool validator into accepting arbitrary strings.

## Execution policy and durable continuation

MCP tool calls do not require human approval. Granted calls execute after ordinary authorization,
argument validation, lease, and budget checks. Set their declarations to
`requires_approval_by_default = false` and skip `HarnessApprovals::decide` for MCP dispatch; do not
create an approval row or park a task merely because an MCP tool is called, including a tool with
remote side effects. There is no per-MCP-tool approval setting in persistence, API, or UI.

An agent may separately call our custom `request_approval` for an explicit workflow checkpoint.
That checkpoint can suspend the conversation before later MCP work, but resuming it does not add
another approval gate to the MCP call. Automatic approval rules for existing native tools remain
as defined in step 6.

Use step 6's stable invocation IDs and checkpoint storage. On retry or continuation after a
separate checkpoint, resolve current company definitions and agent selections, reload credentials
and current grants, and check the definition/schema and selection revisions,
then execute saved pending arguments and store the result. Revoked credentials/grants stop
execution. Company disable/delete or removal from an agent's selections must take effect for pending
calls. Definition/endpoint/tool edits fence or invalidate affected continuations across all selecting
agents. Retain a tombstone or durable connection identity for saved invocation receipts after
deletion. Test company edits and selection updates racing claims/dispatch with transaction-level
revision checks; never silently substitute another definition for a removed connection ID.

Persist completed MCP results for replay. A transport request ID is not a business idempotency key.
If the server accepts an effect and the response is lost, mark the invocation indeterminate unless
that tool has an explicitly supported idempotency/reconciliation contract; do not retry it blindly.
An HTTP disconnect does not establish remote cancellation. Send protocol cancellation where
supported, stop subsequent work locally, and retain the uncertain effect for reconciliation.
Sessions are not durable agent context: reconnect after restart without losing saved results, and
never reinitialize then silently replay a possibly committed call.

## API, UI, and release evidence

Add a company **MCP servers** settings page for definition CRUD, endpoint/authentication settings,
write-only token controls, connection tests, discovery, and company-level remote tool grants.
Show which agents select a definition when changing/disabling/removing it. Manage each definition
independently; changing one must preserve the others.

Agent settings contain only an MCP multi-select from the current company catalog, with labels and
availability information. Multiple selections are supported; no endpoint, credential, discovery,
or tool-grant editing occurs in the agent UI. Company-disabled selections remain visible as
unavailable so the operator can remove or replace them. New selections must reference available
definitions from the same company.

Use company-scoped definition/credential/discovery endpoints and a separate agent selection
operation. Apply existing CSRF/access
checks and retain non-secret form drafts on errors. Report failure per connection, never raw server
responses or secret-bearing headers. Reuse these capabilities in no-JS forms and API clients.

Initially advertise MCP for Rig only. Switching to ai-agents with enabled MCP grants must fail
unless its equivalent contract is implemented; never silently drop capabilities. Simulation must
not acquire unrestricted remote access: offer only explicitly allowed calls with ordinary policy,
and require durable task context for side-effecting calls, without adding MCP approval prompts.

Required fixtures use local scripted Streamable HTTP servers, without live credentials:

For complete agent flows, pair those MCP servers with our existing simulated agent endpoint from
[step 9](09-verification-and-ci.md#existing-endpoint-and-tool-call-support). The model scenario emits
the selected MCP tool call; Rig executes discovery and the real guarded MCP client request, then
the model endpoint checks the returned tool result before replying. Keep both external boundaries
local and assert the MCP server's observed calls as well as the model conversation.

- One agent selecting two company definitions discovers and calls tools from both in the same run.
  Identical remote tool names remain distinguishable by connection; each request uses that
  definition's credentials. Editing/disabling/removing one leaves the other unchanged.
- Two agents select the same company definition without copying configuration or credentials.
  Rotation/disable/grant changes apply to both; removing one agent's selection leaves the other
  agent and company definition unchanged. Deleting an agent preserves shared definitions.
- Agent selection writes accept IDs only, preserve omission, clear on an empty list, and reject
  cross-company IDs and inline configuration. Company UI manages definitions; agent UI only selects.
- Agent session/tenant isolation, token masking/rotation, missing credentials, HTTP authentication failure,
  copy/export behavior, endpoint edits, limits, and cross-tenant database reference rejection.
- Initialization, JSON/SSE results, pagination bounds, unavailable server, malformed schemas,
  duplicate names, selected schema drift, and notifications that never grant new tools.
- URL/DNS/redirect restrictions, allowed test transport, deadlines, bounded streams/results, and
  cancellation/cleanup of supervised transport workers.
- Granted MCP calls, including side-effecting calls, invoke without an approval-service request,
  approval notice, or approval suspension; denied grants still prevent remote execution.
- An explicit `request_approval` checkpoint stops later MCP calls until resumed, then those calls
  execute without another approval. Restart preserves the saved arguments/context and results.
- Committed-result reuse, indeterminate remote effects, revocation, and competing connection-edit/
  run claimants remain covered.
- No generic arbitrary endpoint dispatch; static tool allowlists and custom sub-agent routing hold.

Include the migration/offline SQLx metadata, transport proof, UI/API tests, and database-backed
recovery matrix in step 9 CI. Document actual supported authentication/transport and configuration
keys in step 10; do not claim compatibility with every MCP server or exactly-once remote effects.
