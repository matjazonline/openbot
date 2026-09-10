# Step 7: Application wiring and diagnostics

Dependencies: step 6. Main files: `src/adapters/harness/mod.rs`, `src/infra/setup.rs`,
`src/application/services/agent_runner/`, and proposed Rig diagnostics/error helpers.

## Changes

Construct the immutable provider factory registry and Rig harness in startup wiring. Register
`AiAgentsHarness` and `RigHarness` explicitly in the existing `HarnessRegistry`. Preserve duplicate
and mismatched registration errors. Inject `AppConfig.default_agent_harness` into omission-resolution
paths; require that configured default to be registered at startup. Dispatch always uses the resolved
agent's `harness_kind`, with no runtime fallback if its selected adapter is unavailable.

Validate static deployment facts at startup: dependency features, factory coverage, coherent limits,
and registry wiring. Resolve each company's enabled provider/model/key at write/run time using the
existing connections path. Do not require all tenants' credentials or claim that offline client
construction validates remote authentication or current model availability.

Use a neutral capability description/validator consumed by writes, preflight, and settings if
runtime availability needs to be exposed. Keep concrete Rig types in adapters. Existing pure domain
catalogue validation still owns which tools are permissible; runtime support is an additional check.

Leave `TextClassifier` separately registered as `AiAgentsTextClassifier`. Confirm Rig-selected runs
still receive the existing spam guardrail, and system-prompt generation works before an agent has
been assigned a harness. `DEFAULT_AGENT_HARNESS` controls omitted agent harness selection; it does
not replace this separate classifier. Log the resolved deployment default as a non-secret startup
field. Do not add independent environment reads or defaults inside either adapter.

Normalize Rig output to `AgentExecutionOutput` and the existing `execution_diagnostics` metadata:

- Preserve caller-stamped duration, prompt/response character counts, and history count.
- Aggregate provider token usage across every model call, including skill steps and recoveries.
- Mark reported, estimated, or mixed usage explicitly. Missing usage must not silently become zero
  for a paid call. Do not double-count a final aggregate and its constituent completion records.
- Record harness, logical provider/model, tool counts, supported tool IDs, and bounded reason codes.
- Keep provider-specific metadata optional, small, and explicitly allowlisted. Do not save entire
  Rig responses, conversations, reasoning content, or serialized clients.

Adapt `HarnessTrace` once for tool start/finish, approval requests, delegation where applicable,
and failures. Preserve existing correlation/task/company/agent IDs from the application. Distinguish
executed from denied/cancelled tools, and suspended from failed runs. A model-supplied tool name is
untrusted data: map unknown names to a bounded label in telemetry rather than logging raw content.

Keep content telemetry off. Current Rig exposes `record_content_telemetry(false)` separately from
structural/token telemetry; set it explicitly and verify no additional provider instrumentation
leaks content. Source: [AgentRunner telemetry setting](https://docs.rs/rig/latest/rig/struct.AgentRunner.html).

Reuse harness redaction for content and errors; redact at the adapter boundary before formatting
provider errors for logs. Optional telemetry failure cannot convert a committed run into a retry.

## Verification and acceptance

- Both registered harnesses are reachable through real application wiring; neither is a fallback.
- Missing registration fails clearly and does not invoke another provider/runtime.
- Tests prove the same provider can be selected independently from either harness.
- With no environment property, a newly created agent and no-agent execution resolve to Rig;
  with `DEFAULT_AGENT_HARNESS=ai_agents`, both resolve to ai-agents. Explicit selections always win.
- Multi-turn and skill usage is aggregated once; missing/partial usage is identifiable.
- Captured diagnostics and traces omit credentials, prompts, full arguments/results, and reasoning.
- Rig runs still pass through existing guardrail, memory enrichment, ownership, and delivery paths.
