# Step 4: Tool declarations and execution bridge

Dependencies: steps 1–3. Main files: proposed `src/adapters/harness/rig/tools.rs`,
`builtins.rs`, and harness-neutral capability checks where consumed.

## Changes

Compute requested tools through `AgentCapabilitySpec::required_tool_ids()` so skill dependencies
remain included. Apply the existing platform catalogue allowlist, then intersect with tools the
Rig adapter implements and native declarations supplied by this run's `HarnessToolHost`.
Share pure grant-selection logic in an inner layer if both adapters need it; Rig must not import
the ai-agents YAML compiler to reach a neutral decision.

Distinguish prohibited grants, unsupported Rig implementations, and missing per-run context.
Reject persisted unsupported capabilities at write/preflight time. Context-dependent tools may be
omitted as today with diagnostics; a selected skill requiring one must fail before starting its
steps. Check the effective grant again at invocation, including calls initiated by skills.

Build native tool declarations from `NativeToolDeclaration` without duplicating schemas or tool
names. Current Rig's `DynamicTool` takes a runtime name, description, schema, and callback; it fits
the existing host dispatcher. Verify callback/context details against the pinned release.
Source: [DynamicTool API](https://docs.rs/rig/latest/rig/agent/tool/struct.DynamicTool.html).

Use stable catalogue IDs as model-facing names and map call identifiers deliberately. Preserve the
provider's wire IDs in conversation history; do not replace those with an approval key. Pass a
validated correlation ID to `HarnessToolHost::invoke`. Rig distinguishes its correlation handle
from provider-issued IDs, including the two OpenAI Responses IDs. Source:
[ToolCall API](https://docs.rs/rig/latest/rig/message/struct.ToolCall.html).

Support all four current native tools through the generic bridge: `list_company_agents`,
`create_agent_channel`, `outreach_and_await_quorum`, and `transfer_or_release_task`. Business rules
stay in their application implementations. Approval and suspension handling are completed in step 6.

Add the planned `request_approval` native tool through the same catalogue, grants, declarations,
and host bridge. It creates an explicit human checkpoint and suspends through the task system;
its body owns that approval, so do not add a second pre-execution approval gate around it. Its
schema, identity, resume behavior, and harness compatibility gate are specified in
[step 6](06-execution-and-approvals.md#explicit-checkpoints-through-request_approval).

Extend this guarded bridge to company HTTP MCP tools selected by each agent in
[step 4a](04a-http-mcp.md). Resolve selected company definitions, expose their configured tool grants,
discover remote schemas through those connections, and retain a stable
connection/tool identity through dispatch and persistence. MCP calls need no human approval and
must skip the generic approval gate. Do not register remote tools outside
this bridge or merge arbitrary server tool names into the static native/built-in catalogue.

## Built-in compatibility

The catalogue's ten built-ins are ai-agents implementations, not automatically present in Rig:
`calculator`, `datetime`, `echo`, `json`, `math`, `random`, `template`, `text`, `todo`, `web_fetch`.

Prefer a narrow adapter-owned wrapper around the pinned standalone tool implementations where
their public APIs permit reuse without starting an ai-agents agent. This release already retains
that dependency. If an implementation is inseparable from its runtime, provide a bounded equivalent
with fixture-proven schema/behavior compatibility before marking the ID supported. Do not fabricate
a generic JSON echo implementation or silently omit tools during a harness switch.

Keep mutable tool state scoped to one run, especially `todo`; do not share it through a singleton
registry. Preserve `web_fetch` URL/DNS/IP and redirect protections, response-size bounds, and
timeouts. No replacement may broaden host/network access. Keep denied shell/filesystem tools and
arbitrary MCP/process access denied; configured HTTP MCP access follows step 4a's endpoint policy.

Enforce argument bytes, output/result size, per-tool deadlines, and total tool invocations. Apply
`NativeToolSafety` limits and policy rather than treating them as descriptive metadata. Start with
sequential tool execution so suspension stops later side effects within the same model response.

## Verification and acceptance

- Use the existing simulated agent endpoint to emit tool calls, then validate the actual results
  in its next request before returning a final answer, as specified in
  [step 9](09-verification-and-ci.md#request-checked-scenarios). Cover multi-call responses and
  malformed/forged calls through the provider parser and real guarded dispatcher, with assertions
  on invocation counts and effects as well as conversation messages.
- Fixtures cover granted/ungranted/unknown IDs, skill-implied grants, schema fidelity, malformed
  arguments, missing context, output truncation, timeout, and every built-in offered by the UI.
- A model inventing a tool cannot reach the host. Display names cannot bypass canonical IDs.
- Concurrent runs have isolated mutable built-in state.
- `web_fetch` denial fixtures still pass; tools and skills have one guarded dispatch path.
