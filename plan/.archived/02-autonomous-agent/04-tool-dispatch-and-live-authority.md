# 04 — Direct tool dispatch and live authority

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [03](03-workflow-references-and-run-snapshots.md). Execute the run-selected agent
revision with direct tool bindings, including MCP, through the existing durable action machinery.
Preserve current authorization, reviewed schemas, approvals, recovery, and bounded execution.

## Ownership anchors

- [Agent preparation](../../src/adapters/persistence/workflow/agent_preparation.rs), `prepare_agent`,
  `targets`, and `current_on`: today capabilities come from a workflow bundle and MCP uses slots.
- [Agent execution types](../../src/application/workflow/agent.rs), `AgentToolTarget`,
  `PreparedAgent`, and `AgentToolDispatch`.
- [Tool dispatcher](../../src/adapters/workflow_agent/mod.rs), `AgentActionTools::dispatch`:
  existing HTTP, MCP, message, provisioning, and child-workflow dispatch.
- [MCP runtime](../../src/application/workflow/mcp_runtime.rs), `McpToolCall`,
  `McpActionPreparation`, and `McpActionUse`, plus its
  [persistence adapter](../../src/adapters/persistence/workflow/mcp_runtime.rs).
- [Reviewed operation validation](../../src/application/workflow/mcp.rs), `discovery_matches`
  and `McpOperation::validate`: exact schemas and live connection grants remain mandatory.

## Batch 04A — Prepare and dispatch from agent bindings

1. Load the exact agent revision selected in the run manifest. Build prompts, skill instructions,
   tool schemas, aliases, and targets from that revision, not from current drafts or workflow
   resource slots. Maintain the current model-only variants for classify/decide.
2. Replace the agent MCP target's slot reference with the explicit connection, remote tool,
   registration/policy identity, and agent binding identity required by preparation/reservation.
   Keep explicit workflow MCP steps using their existing workflow resource bindings.
3. Resolve a model's agent-local name to one prepared binding. Reject unadvertised names, invalid
   arguments, alias ambiguities, and target substitution before dispatch. The remote call uses
   its remote name; it cannot treat an alias as a different server tool.
4. Extend existing MCP request preparation to accept this trusted agent binding. Reuse connection
   opening, reviewed discovery comparison, secret loading, action markers, policy approval,
   uncertain-outcome reconciliation, timeouts, and cleanup. Do not add a second MCP executor.
5. Keep stable logical call identity through retries and continuations. Scope saved calls to the
   run/execution, selected agent revision, and binding so equal aliases on different agents never
   share a receipt or dispatch target. Preserve HTTP, message, and provisioning behavior.

Focused checks: `autonomous_agent_dispatch_` tests with real persistence and counted scripted
MCP/HTTP/model peers. Prove an agent calls MCP with no workflow slot, two agents map the same alias
to different connections, invalid calls make zero remote requests, and replay makes no duplicate
known-completed effect. Include connection/session cleanup on failure and cancellation.

## Batch 04B — Enforce current authority at every effect boundary

1. Use the intersection of the immutable selected bindings and current live authority. Verify
   agent/binding identity, tenant, model availability, connection status, tool grant, reviewed
   operation version/schema, and relevant action policy at the same guarded reservation/use
   boundaries as existing actions. A successful prepare does not authorize a later effect.
2. Preserve current checks for message routes, run/channel context, approvers, and protected
   actions. Agent ownership of a capability does not let it manufacture sender, destination,
   actor, or approval authority from its prompt or arguments.
3. Publishing a removal or explicitly revoking a binding blocks subsequent use in old runs.
   Adding a binding cannot expand an old revision. Retargeting the same alias cannot redirect
   an old call. Shared skill edits keep the pinned instructions; assignment revocation still
   blocks further use according to the shared phase-01 contract.
4. Credential rotation uses existing secret/revision handling. Incompatible endpoint, catalog,
   schema, or approval changes fail closed instead of silently changing the frozen tool contract.
   Ordinary agent edits do not disable independently authorized old configurations accidentally.
5. Preserve live-lease fencing, worker cancellation, action approvals, call/round budgets, spend
   limits, suspension deadlines, and uncertainty handling. A remote in-flight request cannot be
   recalled by revocation; cancellation and reconciliation must record that boundary accurately.

Focused checks: `autonomous_agent_authority_` and `autonomous_agent_recovery_` database tests for
revocation between preparation and reservation, competing dispatchers, approval replay, catalog
drift, cross-tenant injection, alias retargeting, model removal, lease loss, and crash/reconstruction.
Use real races rather than sequential mocks; assert provider entry/effect counts and saved states.
Retain existing MCP approval/recovery, agent replay, message, HTTP, and workflow-tool regressions.

## Acceptance and evidence

The same published agent can execute its full tool set from unrelated workflows without copied
slots. Its revision identifies behavior; current authority can still refuse it. Record focused
commands/results and broaden action/lease checks when touching shared reservation code. The final
full gate remains [07](07-integration-and-verification.md).
