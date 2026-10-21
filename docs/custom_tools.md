# Agent Tools

Phase 06C removed the legacy agent-runtime tool layer:

- the static tool catalogue and its ten built-ins (`calculator`, `datetime`, `echo`, `json`,
  `math`, `random`, `template`, `text`, `todo`, `web_fetch`);
- the native tools `create_agent_channel`, `outreach_and_await_quorum`, `list_company_agents`,
  `transfer_or_release_task` and `request_approval`;
- per-agent `granted_tool_ids`, `native_tool_policy` and runtime (`harness_kind`) selection;
- agent MCP selections and executable skill tool steps.

Requests that still send `harness_kind`, `granted_tool_ids` or `native_tool_policy` to the agent
JSON API are rejected as unknown fields.

## What an agent can use now

- **Skills** are prompt-only. Attached skills appear in a scoped catalog and the model loads one
  with `read_resource`, the only tool the Rig runtime exposes. Loading executes nothing; the model
  follows the prompt steps. See [Rig agents](rig.md).
- **Workflow tools.** An agent's `workflow_tools` names the workflow tool types it may use. The
  list is frozen into a workflow at publication and revoked if the agent loses it later. Calls are
  workflow actions dispatched through the application's action service, never runtime tools.
- **Company HTTP MCP** tools are reached the same way: through workflow MCP actions against the
  company catalog and its reviewed grants. See [Company HTTP MCP](rig.md#company-http-mcp).

The stored `config_json` is not runtime YAML and cannot grant tools. Rig V1 accepts only
`version` and `max_turns` (1–16, default 8):

```json
{ "version": 1, "max_turns": 8 }
```

## Agent Builder

The server installs an `agent-builder` definition into the global agent library after database
migrations, inserted only when that library slug is absent. It guides a user through the agent's
purpose, constraints, system prompt, least-privilege workflow tools and company skills, and ends
with a specification the user creates in the company agent settings. It no longer creates agents
itself.

## Planned

Phase 08 rebuilds agent creation (`create_agent_channel`) and the company agent directory
(`list_company_agents`) as workflow tools, together with production workflow worker wiring.
