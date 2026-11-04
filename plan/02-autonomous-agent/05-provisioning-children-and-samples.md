# 05 — Provisioning, child workflows, and samples

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [04](04-tool-dispatch-and-live-authority.md). Integrate reusable agent revisions through
every producer and execution variant. All entry points use the same publication/admission/dispatch
contracts, including built-in agents, library copies, child workflows, and isolated samples.

## Ownership anchors

- [Agent provisioning](../../src/adapters/persistence/workflow/provisioning.rs), `compiled`,
  `provision_on`, and `create_assistant_channel`: currently captures agent facts while compiling.
- [Built-in agents](../../src/application/use_cases/builtin_agent_library.rs) and
  [agent templates](../../src/application/workflow/templates/agent_workflow.rs).
- [Library skill-copy coverage](../../src/adapters/persistence/skill_tests.rs): skills already
  copy with a library agent into company ownership.
- [Workflow-tool execution tests](../../src/adapters/persistence/workflow/workflow_tool_tests.rs)
  and [revocation tests](../../src/adapters/persistence/workflow/workflow_tool_revocation_tests.rs).
- [Sample tool execution](../../src/application/workflow/sample/tools.rs), `agent_tools`, and
  [sample authoring](../../src/adapters/http/routes/workflows/ui/authoring/sample.rs).

## Batch 05A — Unify creation and admission paths

1. Make ordinary agent creation, Agent Builder provisioning, library selection, and templates
   produce the new agent-owned binding/skill configuration. Reuse the publication operation;
   a runnable provisioned agent must have an active revision before admitting its workflow.
2. Preserve existing atomic/idempotent agent/channel/workflow creation. If preparation and commit
   span transactions, use the existing durable provisioning operation identity and recovery
   rather than independent best-effort creates or compensating deletion of user resources.
3. Company-specific connections cannot be copied blindly from a library agent or another tenant.
   Copy portable instructions/skills and require explicit local connection selection before
   publication. Built-in provisioning capabilities remain restricted to the authorized Builder.
4. Inventory manual, message-triggered, scheduled, provisioned, and child admissions through graft
   callers and existing admission ports. Route them all through the phase-03 selection contract;
   no entry point may build a private mutable snapshot or defer selection until worker claim.
5. Keep message response/delivery ownership in the existing channel/workflow system. Coordinate
   shared admission/context edits with [channel response work](../03-channel-handles-send/01-contracts-and-response-ownership.md)
   if implemented concurrently; that plan is not a prerequisite for agent reuse.

Focused checks: `autonomous_agent_provisioning_` and `autonomous_agent_entrypoints_` tests for
duplicate provisioning, partial-failure recovery, authorized built-ins, company-local library
binding, and each admission source. Assert one active revision and one intended logical run.

## Batch 05B — Child workflows and execution dependencies

1. Dispatch a workflow tool using the exact version and parameters recorded by the selected agent
   revision. Preserve current registration revocation checks and transactional child-start/wait
   parking. Republishing an agent or repinning a registration cannot alter an existing call.
2. Child runs, explicit workflow calls, and resumed parents reuse the root dependency manifest.
   A child's agents retain captured revisions, including when a child is first started after a
   later agent activation. Validate that no path falls back to the child's old embedded agent
   snapshot or loads the current active revision.
3. Preserve combined dependency bounds, cycle refusal, continuation limits, child isolation, and
   tenant scoping. A child never inherits message/approval authority merely because its agent is
   reusable. Do not introduce a separate child engine or a second revision-selection policy.

Focused checks: `autonomous_agent_children_` tests for publish-between-parent-and-child, nested
workflow tools, registration repins/revocation, undeclared dependencies, cycles, and competing
child dispatch/resume claimants. Assert exact revision/version identities and one child per call.

## Batch 05C — Samples and previews use sealed definitions

1. Resolve samples through the same agent definition semantics and bounded dependency validation.
   Freeze the selected agent revisions at sample start; when sampling an unpublished draft,
   capture an explicitly sample-only definition without activating a production revision.
2. Update sample tool lookup to use agent-local bindings. Intercept direct MCP/HTTP/message tools
   and workflow-tool children through the existing sample boundary. A sample cannot contact live
   external services, borrow production credentials, or start production children.
3. Record selected agent revisions or draft sample hashes in sample reports. A later edit must
   not mutate the report's configuration or imply that an old sample validated a new revision.
4. Update templates, fixtures, sample scripts, and API contracts coherently. Remove obsolete
   workflow-owned agent capture paths after their final consumers are switched.

Focused checks: `autonomous_agent_samples_` tests for no-slot MCP simulation, different aliases,
draft isolation, changed active revisions, nested sample children, and forbidden live effects.
Run existing sample-boundary Python checks and affected sample/browser protocol tests.

## Acceptance and evidence

No production or sample entry point bypasses the agent-owned capability model. Record exact
focused commands/results and the inventory of admission/creation consumers updated. The final
cross-system proof is in [07](07-integration-and-verification.md).
