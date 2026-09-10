# Step 1: Architecture and compatibility contract

Dependencies: none. Output: a reviewed behavior matrix and focused baseline fixtures.

## Changes

Keep the existing direction of control:

```text
company connection + agent + attached capabilities
  -> ResolvedAgentCapabilities / AgentCapabilitySpec
  -> application AgentRunner (prompt, guardrail, native ports)
  -> HarnessRegistry.require(spec.harness)
       -> AiAgentsHarness
       -> RigHarness -> ProviderRegistry -> selected provider/model
  -> AgentExecutionOutput -> existing durable task and delivery flow
```

Place Rig implementation types under `src/adapters/harness/rig/`. Keep Rig types out of domain,
application ports, SQL rows, public request/response types, and persisted execution state.
`HarnessRegistry` selects a runtime; `ProviderRegistry` constructs models within the Rig adapter.
These have different responsibilities and must remain separate.

Record the compatibility contract before implementation:

| Concern | Required Rig behavior |
| --- | --- |
| Selection | Exact `rig` dispatch; missing implementation is an error; no fallback |
| Defaults | Absent `DEFAULT_AGENT_HARNESS` means Rig; `ai_agents` is an explicit environment override |
| Precedence | Explicit/stored selection wins; omitted new selections use the configured default |
| Credentials | Resolve the requesting company's enabled connection and model |
| Prompt | Preserve application fencing, history, notes, memory enrichment, and recipient role |
| Tools | Grant union includes attached skills; platform allowlist and runtime availability both apply |
| Skills | Ordered prompt/tool instructions, arguments, and named outputs retain defined meaning |
| Approvals | Shared deterministic policy and durable approval identity |
| Suspension | Pending approval or a suspending native tool parks the task without a reply |
| Delegation | Existing directory/outreach ports enforce same-company and sub-agent scope |
| Lifecycle | Existing timeout, lease, retry, shutdown, and durable delivery owners remain authoritative |
| Results | Completed/suspended output, token usage, sanitized diagnostics; errors remain errors |

Distinguish hard requirements from model behavior: exact natural-language output and skill-routing
choices need not match between LLM runtimes. Authorization, instruction order once a skill starts,
durable state, and accounting must be testable without probabilistic assertions.

Inventory grants and attached skills used by built-in/library definitions. Characterize the pinned
ai-agents skill router and template/output semantics from its source and existing tests. Do not
assume that putting a recipe in a preamble reproduces executable tool steps.

Audit existing task resumption semantics. Establish what happens if an agent's harness or config is
edited while a task is active or suspended. Use the existing snapshot/version mechanism if present;
otherwise reject harness changes for such agents until their runs settle. Do not resume a partially
executed ai-agents run inside Rig.

## Verification and acceptance

- Baseline fixtures cover plain text, one native tool, ordered skill steps, pending/rejected
  approval, outreach suspension, restricted delegation, and timeout.
- Each supported capability has an implementation step and a release test.
- Any unavoidable limitation is rejected before a provider call and shown in settings; it cannot
  be implemented as silently dropping a required capability.
- No new workflow engine, credential store, memory store, or delivery owner is introduced.
