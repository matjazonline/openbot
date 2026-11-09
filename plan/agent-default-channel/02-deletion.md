# 02 Deletion

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Deleting an agent that has never handled a message also removes what its creation provisioned,
and the company returns to the state it was in before the agent existed. Otherwise delete refuses
and changes nothing, as it does today. Continues in [03](03-integration-and-final-verification.md).

## What blocks it today

- The agent's publication writes `agent_publication_requests`. Its guard
  (`guard_agent_publication_request`, `init_schema.sql:12615`) allows a delete only when the whole
  company is being deleted, and its deferred foreign key to `agents` then fails at commit.
- Channel delete refuses any channel that has a workflow binding (`src/adapters/persistence/channel.rs:401`),
  and production code never deletes a binding or a workflow definition.
- The guards already allow these deletes once the parent row is gone:
  - `agent_revisions` (`guard_agent_owned_identity`, `:12387`), after the agent row;
  - `workflow_versions` and `workflow_binding_revisions` (`guard_workflow_publication_snapshot`,
    `:11698`), when cascaded from their definition or binding;
  - `channel_reply_settings` (`:12752`), when cascaded from the channel.

## Batch 02A Delete with the provisioned resources

1. Schema (edited in place in `migrations/20260817000000_init_schema.sql`):
   `guard_agent_publication_request` also returns `OLD` on `DELETE` when the agent row no longer
   exists, mirroring `guard_agent_owned_identity`.
2. `AgentPersistence::delete` (`src/adapters/persistence/agent.rs:398`) keeps its agent
   `FOR UPDATE` lock as the first lock. Right after it, delete reads the `agent:{agent_id}` row of
   `workflow_provisioning_commands` `FOR UPDATE`. With no row it behaves exactly as today. If delete
   has meanwhile been split into a lock phase and a finish phase, for example by memory-namespace
   retirement, put this step between the two and keep whatever lock order they establish.
3. With a row, it locks the recorded channel `FOR UPDATE` and refuses with `Conflict`, writing
   nothing, if any of these hold:
   - the channel has threads, schedules or runs, or any binding other than the recorded one;
   - another workflow version lists the agent in `workflow_versions.agent_ids`
     (`init_schema.sql:5871`) — that is, another workflow uses this agent.

   Update the `agent_ids` column comment (`:5888`): delete authorization becomes a second reader.
   Run history and retained conversations are still refused by the existing foreign keys and the
   `agent_delete_error` mapping (`agent.rs:439`).
4. Otherwise it deletes, in the same transaction and in this order:
   - the recorded binding, which cascades its revisions, events and intent bindings;
   - the channel, which cascades its slugs, email binding, grants and response settings;
   - the recorded workflow definition, which cascades its versions and definition events;
   - the agent, as delete does today: demote its principal, then delete the row;
   - the agent's revisions and publication requests;
   - the provisioning row.

   Deferred foreign keys are checked at commit, so any reference this list missed fails the whole
   delete instead of leaving a partial one.
5. Serialize delete with workflow publication. `capture_agents_on`
   (`src/adapters/persistence/workflow/agent_publication.rs:48`) reads the agent without a lock, so
   a concurrent publish could capture an agent that is being deleted. Take `FOR KEY SHARE` on the
   agent row there. The publish then either commits first, and delete sees its `agent_ids` and
   refuses, or waits and finds no agent.
6. Check the lock order against message admission, which locks the channel `FOR SHARE`
   (`capture_channel_reply_context`). Delete takes the agent before the channel, so confirm no
   admission or agent-preparation path takes them in the opposite order. If one does, take the
   channel lock first in delete, and note why in the code.

## Focused checks

- `agent_default_channel_delete_`: create, then delete. Afterwards the company's rows in every
  public table match a snapshot taken before the create; compare per-table counts for the company.
- `agent_default_channel_delete_refusal_`: refuses after one handled message; after a second
  channel was given to the agent with "Create assistant channel"; and after another workflow
  referencing the agent was published. Each refusal leaves every row in place.
- `agent_default_channel_delete_competing_`, on real concurrent transactions:
  - delete racing an inbound message to the agent's address: either the message is admitted and
    delete refuses, or delete wins and the message is refused as undeliverable;
  - delete racing a publication of another workflow that references the agent: either the publish
    wins and delete refuses, or delete wins and the publish is refused.
- Rerun the existing `agent_retire_` and channel-delete tests.
