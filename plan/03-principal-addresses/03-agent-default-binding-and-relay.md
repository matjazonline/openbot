# 03 Agent default binding and relay identity

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Creating an agent gives it a working address answered by itself, without a desk; deleting an
unused agent removes it; and a relayed message carries the agent principal as its sender.
Continues in [04](04-integration-and-final-verification.md).

## Batch 03A Provisioning

1. Split `assistant_channel_on` (`src/adapters/persistence/workflow/provisioning.rs:812`) into
   `provision_agent_workflow_on` (copy the autonomous base, point `agent.run`, publish) and the
   channel-specific remainder. Agent creation calls the first and then
   `bind_on_principal(agent_principal)`. No per-agent desk or `agent:{agent_id}` provisioning key
   is created after this plan's database reset: phase 01 disables the automatic agent-desk call
   from 01-departments batch 03A, and this batch removes its unused helper and wires the direct
   binding into every company-agent creation path. The `workflow_provisioning_commands` comment
   (`init_schema.sql:5594`) lists only the `department:{id}` key. Existing shared-desk provisioning
   remains available; there is no conversion or forwarding path for old automatic desks.
2. Agent delete refuses while the agent has direct threads or runs; otherwise it removes the
   binding and workflow with the agent while preserving retained-history and publication-reference
   guards. Do not retain the automatic-desk lookup or cleanup path from 01-departments.
3. The Agent Builder tool `create_agent_channel` is renamed `create_agent`; the desk-staffing
   variant is `create_desk_for_agent`. Both go through the same provisioning functions and keep
   the department and role placement from
   [01-departments](../01-departments/03-leads-and-coordination.md),
   Batch 03C item 0.
4. The company-agent directory and department dispatch sample use the agent principal's address
   instead of its former default desk. Agent settings and onboarding show that direct address;
   the separate action to create a shared desk for the agent remains. Update the automatic-desk
   creation tests from 01-departments to assert a principal binding and no automatic channel.

## Batch 03B Relay identity

1. `authorize_relay` (`application/use_cases/thread/mod.rs`) accepts as sender either the running
   agent's own address or a desk address bound to the run's binding. The internal-origin message
   carries the agent principal id (`IngressOrigin::InternalChannel`,
   `src/application/use_cases/thread/ingest/relay.rs`, gains it; rename to
   `IngressOrigin::Internal`).
2. The receiving side builds `PrincipalAccessContext` (`participant.rs:261`) from that principal,
   so desk grants to agent principals, including the derived department grants from
   [01-departments](../01-departments/02-desks-ownership-and-access.md), are what admit the
   message. Remove the person fallback if the 01-departments test found one.
3. The hop trace records principal addresses; the loop refusal applies to them.

## Focused checks

- `agent_address_create_`: a new agent has an address, a published workflow and an active binding
  on its principal, and no channel; a message to the address is answered (scripted `Deployment`
  harness); a taken slug refuses the whole creation.
- `agent_address_delete_`: delete removes binding and workflow when unused and refuses otherwise.
- `relay_identity_`: an agent member's send to a members-only department desk is admitted on its
  own grant; a non-member agent's is refused; a run cannot send as another agent's address.
