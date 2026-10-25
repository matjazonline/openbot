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

- **Skills** are prompt-only. An `agent.run` step appends each attached skill's instructions to
  the agent's prompt. That executes nothing; the model follows the prompt steps. See
  [Rig agents](rig.md).
- **Workflow tools.** An agent's `workflow_tools` names the workflow tool types it may use. The
  list is frozen into a workflow at publication and revoked if the agent loses it later. Calls are
  workflow actions dispatched through the application's action service, never runtime tools.
- **Company HTTP MCP** tools are reached the same way: through workflow MCP actions against the
  company catalog and its reviewed grants. See [Company HTTP MCP](rig.md#company-http-mcp).

An agent has no runtime configuration of its own, so nothing stored on it can grant tools beyond
the two lists above.

## Agent Builder

The server installs an `agent-builder` definition into the global agent library after database
migrations, inserted only when that library slug is absent. It guides a user through the agent's
purpose, constraints, system prompt, least-privilege workflow tools and company skills. It is
granted two workflow tools:

- `list_company_agents`: the company's agents, so it does not duplicate one;
- `create_agent_channel`: provisions the new agent together with a team (never public) channel,
  bound to the reply-mode workflow the conversation chose (p08 D2). The requester must be an
  owner or admin, and becomes the binding's run-as member.

An owner or admin can provision the same way from the UI ("Create assistant channel").
