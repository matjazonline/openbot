# 03 Leads and coordination

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Every department has a desk at its slug. When the lead is an agent, that desk is answered by the
lead through the standard autonomous workflow, and the lead can find its members and their desks,
dispatch work to them, and escalate to the parent department. When the lead is a person, the desk
is a plain team desk they read. Continues in [04](04-ui-and-docs.md).

## Batch 03A The department desk

1. `create_department_on` (in `src/adapters/persistence/department.rs`, from
   [01](01-departments-and-membership.md)) provisions the desk in the same transaction:
   - with an agent lead, through `assistant_channel_on`
     (`src/adapters/persistence/workflow/provisioning.rs:812`) with command key
     `department:{department_id}`, `ReplyMode::Autonomous`, channel slug and name equal to the
     department's, as the agent default channel plan does for `agent:{agent_id}`;
   - otherwise, through `insert_channel_row_on` (`src/adapters/persistence/channel.rs:298`) and
     `insert_channel_slugs` (`:152`) as a team desk with the company's `add_3rd_party` default.

   Both then set `channels.department_id`. A provisioning refusal becomes the function's error
   (`Conflict` for a taken address, `BadRequest` for an invalid one) and the caller rolls back, so
   a refused creation leaves no department, desk, workflow or binding.
2. The department desk is marked by the `department:{id}` row in `workflow_provisioning_commands`
   when provisioned through `assistant_channel_on`, and by `channels.department_id` plus slug
   equality otherwise. No ownership column or trigger is added (the agent default channel plan's
   decision 4 applies here too): after creation the desk is an ordinary desk.
3. Changing the lead from one agent to another re-points the desk's `agent.run`. Reuse the
   publication path the Agent Builder uses when it edits an existing agent channel's workflow
   (`apply`, `provisioning.rs:192`) rather than editing the workflow source by hand. Changing the
   lead from an agent to a person pauses the binding; from a person to an agent provisions the
   workflow and binding on the existing desk through the same `assistant_channel_on` internals,
   factored so they can target an existing channel. Record in the plan which of these factorings
   turned out to be needed.
4. Department delete runs after the department desk is detached or deleted by the user (decision
   9). The desk's own delete rules apply (`channel.rs:401`).

## Batch 03B What a lead can see and do

1. Agent directory. `AgentDirectoryEntry` (`src/application/agent/authoring_reads.rs:138`) gains
   `departments: Vec<DepartmentRef { id, name, slug, lead: bool }>` and the entry's default desk
   address, so an agent lead can address a member without a separate lookup. The directory's
   paging key is unchanged. The directory tool's description tells the model that a department's
   members are the agents listing that department.
2. Dispatch is `message.send` (`src/application/workflow/registry/actions.rs:158`) to the member's
   desk address over the internal relay. The member's own desk binding runs the member and replies;
   reply correlation offers the answer to the waiting run
   (`application/workflow/message_admission.rs`). Nothing new is needed for a lead that dispatches
   and waits for replies. Add the sample
   `src/application/workflow/fixtures/department-lead.yaml`: `context.history` → `agent.run`
   with a prompt that says to use the directory and send to members → `reply`. It is a fixture
   for tests and for the authoring sample list, not a hardcoded base; the base stays
   `autonomous-response.yaml`.
3. Escalation is `message.send` to the parent department's desk address. The parent desk's
   grants admit the child lead by decision 7. The parent's own binding decides what runs.
4. Loop safety is the existing hop budget and trace (`MAX_INGRESS_HOPS`, `EmailRelayTrace`,
   `docs/inter_channel_agent_communication.md`): a lead that dispatches to itself or a member that
   bounces back to the lead is refused when the channel reappears in the trace. Add a test with a
   two-department chain to show a reply chain of lead → member → lead is admitted (the reply
   continues the trace) and a new send lead → member → lead is refused.
5. People as leads or members see the department's desks in the UI and get its notifications
   through the grants from [02](02-desks-ownership-and-access.md); nothing further.
6. When `channel.call` lands ([agent-workflow-call](../agent-workflow-call/README.md)), a lead can
   call a member's desk synchronously and get the reviewed answer as a tool result. Nothing in this
   plan prevents it; the department desk is an ordinary selected responder.

## Focused checks

- `department_lead_desk_`: creating a department with an agent lead leaves one desk at the
  department slug, owned by it, with an active autonomous binding on the lead; a person lead
  leaves a team desk with no binding; a taken slug refuses the whole creation and leaves no rows;
  re-leading to another agent re-points `agent.run`; re-leading to a person pauses the binding.
- `department_lead_round_trip_`: using the scripted-model `Deployment` harness from
  `src/adapters/persistence/workflow/agent_history_channel_tests.rs`, a message to the department
  desk is answered by the lead; a lead that sends to a member desk gets the member's reply offered
  to its run; a send to the parent desk is admitted and a send from a non-member department is
  refused.
- `department_lead_loop_`: the chain test from 03B.4.
- `department_directory_`: entries list departments and lead flags; an agent in no department has
  an empty list; the page key is unaffected.
