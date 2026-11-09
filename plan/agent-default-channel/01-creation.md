# 01 Creation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Every production path that creates a company agent provisions its default channel in the same
transaction, or creates nothing. Continues in [02](02-deletion.md).

## Batch 01A One creation path

1. Add one persistence function for creating a company agent together with its channel. Call it
   `create_agent_with_channel_on(tx, templates, company, actor, prepared)`, next to provisioning in
   `src/adapters/persistence/workflow/`. Inside the caller's transaction it:
   - prepares a library copy when the agent comes from the library (`prepare_library_agent_on`);
   - inserts the agent (`insert_agent_on`) and its principal (`create_agent_principal_on`);
   - calls `assistant_channel_on` with command key `agent:{agent_id}`, `ReplyMode::Autonomous`,
     channel slug = agent slug and channel name = agent name.

   A provisioning refusal becomes the function's error: `Conflict` for a taken address and
   `BadRequest` for an invalid one. The caller then rolls back, so a refused creation leaves no
   agent, principal, channel, workflow or binding.
2. Expose it through a creation port that holds the `ProvisioningDeployment`, the same way
   `PostgresOnboarding` does (`src/adapters/persistence/onboarding.rs:26`, wired in
   `src/infra/workflow/services.rs:178`). Pass it to `AgentUseCases`
   (`src/infra/setup/accounts.rs:36`). Route `AgentUseCases::create_agent`
   (`src/application/use_cases/agent.rs:288`) and `create_agent_from_library` (`:341`) through it.
3. Remove the production agent-only paths: `AgentPersistence::create`
   (`src/adapters/persistence/agent.rs:320`) and `LibraryAgentCopyPersistence::create_agent_from_library`
   (`src/adapters/persistence/channel.rs:505`). Keep an agent-only insert only as a test fixture
   (`#[cfg(test)]` or test support) for tests that need an agent without a channel. `create_library`
   and `ensure_library_agent` stay as they are, since global agents have no company.
4. Require a model. `prepared_agent_write` (`agent.rs:321`) refuses a company agent without
   provider and model, with a `BadRequest` that says why. Library copies always pin the company
   default (`pinned_default_model`, `agent.rs:169`). Delete `LibraryCopyModel` (`agent.rs:184`):
   only its `PinDefault` case remains. With no keyed default connection, a library copy is refused
   with a message telling the user to add one.
5. Onboarding (`src/adapters/persistence/onboarding.rs:146` and `:184`) calls the same function,
   so its key becomes `agent:{agent_id}`. Its `NoModel` and `Refused` channel outcomes
   (`src/adapters/http/pages/onboarding.rs:150`) are removed: an agent onboarding cannot give a
   channel is now refused as a whole step through `OnboardingOutcome::Rejected`. The completion
   page re-reads the stored result through `assistant_channel_result` using the new key.
6. The Agent Builder tool path (`apply`, `src/adapters/persistence/workflow/provisioning.rs:192`)
   also writes a `workflow_provisioning_commands` row with key `agent:{agent_id}` and the same
   result JSON. Every agent created with its channel then has the same record, which
   [02](02-deletion.md) uses. Update the table comment (`init_schema.sql:5638`), which currently
   says the tool path needs none.
7. Routes keep their shapes: the UI create (`src/adapters/http/routes/ui_agents.rs:315`), the
   library pick (`:415`), and the API create (`src/adapters/http/routes/agent.rs:309`, `:528`).
   A refusal renders on the form, or returns 409 or 400 from the API. Update the D4.7 doc comments
   that now mislead: `agent.rs:285`, `LibraryCopyModel`, `src/adapters/http/pages/agent_settings.rs:423`
   and `src/adapters/http/pages/channels.rs:38`.

## Focused checks

Run with an isolated `TEST_DATABASE_URL` and list the filters before executing them, so each has a
nonzero test count.

- `agent_default_channel_create_`: a UI, API, library or onboarding create leaves exactly one
  channel. Its slug and name match the agent, its access mode is team, and `add_3rd_party` equals
  the company default. Its workflow is published from the autonomous base, with `agent.run` on
  this agent. Its active binding uses a direct trigger and runs as the creator, and the channel's
  Automatic response is that binding. The agent has a published revision.
- `agent_default_channel_round_trip_`: an internal message to the new agent's address is admitted
  and answered by the agent. Reuse the scripted-model `Deployment` harness from
  `agent_history_channel_tests.rs`.
- `agent_default_channel_refusal_`: no model → `BadRequest`; a taken address → `Conflict`; a
  library copy with no default connection is refused. In each case no agent, principal, channel,
  workflow, binding or provisioning row exists afterwards.
- `agent_default_channel_competing_`: two concurrent creates with the same slug, and an agent create
  racing a plain channel create for the same address. Exactly one wins, the other gets `Conflict`,
  and the loser leaves no rows. Run on real concurrent transactions, not sequential mocks.
- Update the tests that assert agent-only creation: `src/application/use_cases/agent_tests.rs:221`
  and `:704`, `src/adapters/http/routes/agent_api_tests.rs:192`, and the onboarding route test
  `the_channel_step_gives_each_modelled_agent_one_assistant_channel`
  (`src/adapters/http/routes/onboarding.rs:569`). Then rerun the existing `provisioning_` and
  `channel_reply_` filters.
