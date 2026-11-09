# Every new agent gets a default channel

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed implementation, 2026-11-09. These documents do not implement the feature.

## Intended behavior

Creating a company agent also creates one channel for it, so every agent can receive internal
messages. Creation goes through the UI form, the JSON API, a library copy or onboarding. All of it
happens in the transaction that creates the agent, and it reuses the provisioning that onboarding
and "Create assistant channel" already share (`assistant_channel_on`,
`src/adapters/persistence/workflow/provisioning.rs:812`):

- a team channel whose slug is the agent's slug and whose name is the agent's name;
- a workflow copied from the hardcoded base `src/application/workflow/fixtures/autonomous-response.yaml`
  (`context.history` → `agent.run` → `reply`), its `agent.run` pointed at the agent, published;
- an active binding on that channel, triggered by direct messages, running as the creating user;
- the channel's Automatic response set to that binding (`bootstrap_native_on`,
  `src/adapters/persistence/workflow/channel_reply.rs:66`);
- the agent's first published revision.

Deleting an agent that has never handled a message removes all of the above with it.

This partly reverses p08 D4.7 ("agent-only creation"). It does not bring back the retired
owned channel: there is no ownership column, trigger or table, and
`retired_agent_and_channel_runtime_columns_and_tables_are_gone`
(`src/adapters/persistence/agent_config_tests.rs:7`) keeps passing unchanged.

## Decisions (user, 2026-11-08 and 2026-11-09)

1. **A model is required to create an agent.** `agent.run` runs only on an explicit model, so an
   agent without one could not answer its channel. Library copies are pinned to the company default
   model, as onboarding already does. A company with no default model connection cannot create
   agents until it adds one.
2. **Delete removes what creation provisioned.** Publishing the agent would otherwise make every
   new agent undeletable ("This agent has published revisions and cannot be deleted. Deactivate it
   instead."). Delete stays refused once the agent or its channel has history.
3. **A taken address refuses the whole creation.** If a channel already uses the agent's slug as
   its address, no agent is created and the form shows the conflict.
4. **Created by default, not an invariant.** After creation, people with permission may still pause
   the binding, edit the workflow, or add more channels. Nothing marks the channel as owned, and
   code that sends to an agent still resolves the channel through its bindings.
5. **Channel settings as provisioning makes them now.** The reply mode is Autonomous. The channel
   is a team channel, and `add_3rd_party` is copied from the company default
   (`channel_write`, `provisioning.rs:639`).
6. **Keep the `reply` step.** `AgentWorkflow::compose` requires it
   (`src/application/workflow/templates/agent_workflow.rs:88`). It is where Reviewed mode and
   custom steps attach, so the default channel keeps the same base as every other agent channel.
7. **The binding runs as the creating user**, like every other provisioning path. A run-as user who
   later leaves the company is a general binding problem, not one this plan solves.

## What does not change

- Global library and built-in agents (`company_id IS NULL`) get no channel.
- The Agent Builder's `create_agent_channel` tool already creates an agent and its channel
  together; it only gains the creation record described in [01](01-creation.md).
- "Create assistant channel" still gives an existing agent additional channels.
- Renaming an agent does not rename its channel; they are separate records.
- Existing agents get no channel. Reset the database instead of backfilling.

## Documents

1. [Creation](01-creation.md): one shared creation path that provisions the channel.
2. [Deletion](02-deletion.md): delete removes the provisioned resources, or refuses.
3. [Integration and final verification](03-integration-and-final-verification.md).
