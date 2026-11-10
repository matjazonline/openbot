# 03 Leads and coordination

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Every company agent gets a default desk at its initial slug, and every department has a separate
desk at its slug. When the lead is an agent, the department desk is answered by the
lead through the standard autonomous workflow, and the lead can find its members and their desks,
dispatch work to them, and escalate to the parent department. When the lead is a person, the desk
is a plain team desk they read. Continues in [04](04-ui-and-docs.md).

## Batch 03A Automatic agent desks

1. Every path that creates a company agent (UI, JSON API, library copy, onboarding and the Agent
   Builder) uses one shared creation path: insert the agent and its principal, apply placement
   from 03C item 0, then call `assistant_channel_on` in that transaction with command key
   `agent:{agent_id}`, the agent's initial slug and name, and `ReplyMode::Autonomous`. The helper
   must target the agent just inserted, without recursively calling company-agent creation.
   Reuse existing publication and company-write-lock ordering; do not add a second runtime or
   an asynchronous desk-repair job.
2. The commit contains the agent, principal, default desk, published workflow, active binding and
   provisioning command, or none of them. Pin an omitted model to the configured company default
   before provisioning; a missing usable model, unresolved publication prerequisite or taken
   address refuses the whole creation with the existing worded error. A library agent with no
   company has no desk; its company copy follows the same creation and readiness rules.
3. The default desk starts as an ordinary team desk, with `department_id = NULL` and
   `members_only = false`; agent membership does not choose one owning department for it. A user
   can assign it to a department through normal desk settings. No special agent-ownership column
   or trigger is added: `agent:{agent_id}` records which desk was initially provisioned. Desk
   edits, deletion and retained-history guards remain ordinary desk operations; renaming an
   agent does not silently rename its desk. Directory reads resolve the recorded desk id to its
   current address and omit a deleted desk rather than recreating it.
4. Onboarding reads back the committed default desk instead of creating it again in its channel
   step. The Builder reuses that desk when its requested desk has the same slug and settings;
   incompatible settings at that slug refuse the whole command. An explicitly requested
   additional desk has a different slug and is provisioned in the same outer transaction.
   Retries reuse the existing provisioning command and cannot create another default binding.
5. Default agent desks are the addressing model for this plan and 02-workflow-roles. The later
   [03-principal-addresses](../03-principal-addresses/README.md) plan removes automatic desk
   provisioning and binds the autonomous workflow to the principal after a database reset.

Focused checks for this batch are `agent_default_desk_` and `agent_default_desk_competing_` below;
list their nonzero selected counts before running them. Keep the full suite for phase 05.

## Batch 03B The department desk

1. `create_department_on` (in `src/adapters/persistence/department.rs`, from
   [01](01-departments-and-membership.md)) provisions the desk in the same transaction:
   - with an agent lead, through `assistant_channel_on`
     (`src/adapters/persistence/workflow/provisioning.rs:812`) with command key
     `department:{department_id}`, `ReplyMode::Autonomous`, channel slug and name equal to the
     department's, as batch 03A does for `agent:{agent_id}`;
   - otherwise, through `insert_channel_row_on` (`src/adapters/persistence/channel.rs:298`) and
     `insert_channel_slugs` (`:152`) as a team desk with the company's `add_3rd_party` default.

   Both then set `channels.department_id`. A provisioning refusal becomes the function's error
   (`Conflict` for a taken address, `BadRequest` for an invalid one) and the caller rolls back, so
   a refused creation leaves no department, desk, workflow or binding.
2. The department desk is marked by the `department:{id}` row in `workflow_provisioning_commands`
   when provisioned through `assistant_channel_on`, and by `channels.department_id` plus slug
   equality otherwise. No special department-desk ownership column or trigger is added: after
   creation the desk is an ordinary desk, separate from the lead agent's default desk.
3. Reassigning the `lead` role from one agent to another re-points the desk's `agent.run`. The
   role write and the workflow change are one transaction, through `assign_role_on` from
   [01](01-departments-and-membership.md). Reuse the publication path the Agent Builder uses when it
   edits an existing agent channel's workflow (`apply`, `provisioning.rs:192`) rather than editing
   the workflow source by hand. Changing the lead role from an agent to a person pauses the binding;
   from a person to an agent provisions the workflow and binding on the existing desk through the
   same `assistant_channel_on` internals, factored so they can target an existing channel. Record in
   the plan which of these factorings turned out to be needed.
4. Department delete runs after the department desk is detached or deleted by the user (decision
   9). The desk's own delete rules apply (`channel.rs:401`).

   Factorings that turned out to be needed (p03 B2, gate `01DEP-P03-D9`): `apply` has no edit
   path, so re-pointing is new code over the existing primitives -- `save_draft_on` (out of
   `save_draft`), `configure_reply_on` and `native_contract` (out of `configure_channel_reply` and
   `bootstrap_native_on`), `provisioned_reconfiguration` and `provisioned_activation` (beside
   `provisioned_binding`), and `provision_workflow_on` reachable for an existing desk without the
   reply bootstrap. Both desk kinds are marked by the `department:{id}` command row, which also
   records the binding the department owns; a slug edit therefore never loses the desk.

## Batch 03C What a lead can see and do

0. Placement. Every path that creates a company agent (the UI form, the JSON API, a library copy,
   onboarding, and the Agent Builder's `create_agent_channel` tool) takes an optional department
   and an optional role to assign in it, written in the creation transaction through
   `add_member_on` and `assign_role_on` from [01](01-departments-and-membership.md). The form and
   the tool default to the creator's single department; onboarding creates the company's first
   department when the person names one and places the first agent there. `list_company_agents`
   accepts a department filter. The tool descriptions in `docs/agent-builder-context.md` and
   `docs/custom_tools.md` say so.

1. Agent directory. `AgentDirectoryEntry` (`src/application/agent/authoring_reads.rs:138`) gains
   `departments: Vec<DepartmentRef { id, name, slug, roles: Vec<DepartmentRole> }>` and the entry's
   default desk address and, when the desk's selected procedure declares one, its intake schema
   ([agent-workflow-call](../agent-workflow-call/01-contracts-and-authority.md), Batch C), so an
   agent lead can address a member and shape the request without a separate lookup. The
   directory's paging key is unchanged. The directory tool's description tells the model that a
   department's members are the agents listing that department. `DepartmentRef` also carries the
   charter, and a run admitted on a department desk gets the owning department's name and charter
   in its `agent.run` system context, after the agent's own prompt, so members and leads see the
   same framing without a memory lookup.
2. Dispatch is `message.send` (`src/application/workflow/registry/actions.rs:158`) to the member's
   desk address over the internal relay. The member's own desk binding runs the member and replies;
   reply correlation offers the answer to the waiting run
   (`application/workflow/message_admission.rs`). Nothing new is needed for a lead that dispatches
   and waits for replies. Once `channel.call` lands, the lead fixture switches to it: the call
   returns a typed closure (`done`, `denied`, `handed_off`) and takes a per-call deadline, which
   is what makes a dispatch an owned piece of work rather than a message
   ([agent-workflow-call](../agent-workflow-call/README.md), contract decisions). Add the sample
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

- `agent_default_desk_`: UI, JSON API, library copy, onboarding and Builder each leave one default
  team desk at the new agent's slug, one published autonomous workflow and one active binding;
  a message to the desk is answered through the scripted `Deployment` harness. A library-only
  row gets no desk. A missing model, publication refusal or taken slug leaves no partial agent,
  principal, placement, desk, workflow, binding or command. Onboarding and command replay add no
  second desk; renaming the desk changes its directory address; a deleted desk is not recreated.
  Existing agent deletion guards still refuse retained references without partially removing its
  desk or bindings.
- `agent_default_desk_competing_`: competing creates for the same address leave one committed
  agent and desk; competing retries of one Builder command leave one default desk and binding.
- `department_lead_desk_`: creating a department with an agent lead leaves one desk at the
  department slug, owned by it, with an active autonomous binding on the lead; a person lead
  leaves a team desk with no binding; a taken slug refuses the whole creation and leaves no rows;
  re-leading to another agent re-points `agent.run`; re-leading to a person pauses the binding.
  Provisioning or re-leading never replaces either lead agent's separate default desk.
- `department_lead_round_trip_`: using the scripted-model `Deployment` harness from
  `src/adapters/persistence/workflow/agent_history_channel_tests.rs`, a message to the department
  desk is answered by the lead; a lead that sends to a member desk gets the member's reply offered
  to its run; a send to the parent desk is admitted and a send from a non-member department is
  refused.
- `department_lead_loop_`: the chain test from 03C.4.
- `department_directory_`: entries list departments and held roles; an agent in no department has
  an empty list; the page key is unaffected.
