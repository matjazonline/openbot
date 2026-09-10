# Step 8: API, settings, library, and simulation

Dependencies: steps 3 and 7. Main files:
`src/adapters/http/routes/{agent,ui_agents,agent_library}.rs`,
`src/adapters/http/pages/{agent_settings,agent_library_multi_select,simulation}.rs`, and affected
application provisioning/library commands. Read their HTTP/page `AGENTS.md` before implementing.

## Changes

Offer both harnesses using the existing `HarnessKind::ALL` selector. Submit the selected value and
remove the assumption that the control is always disabled with a hidden ai-agents value. Preselect
the configured deployment default on create/onboarding/native agent provisioning: Rig when
`DEFAULT_AGENT_HARNESS` is absent, ai-agents when explicitly configured. Edit forms show the stored
selection, independent of the current deployment default.

Show advanced configuration fields and help for the selected harness. Replace the unconditional
ai-agents label with appropriate copy. Rig exposes only its typed V1 settings. Keep provider/model,
tool grants, skills, and sub-agent controls conceptually separate from runtime selection.

Add company **MCP servers** settings and company-scoped API operations from
[step 4a](04a-http-mcp.md): HTTP definition CRUD, write-only credentials, connection test/tool refresh,
and company-level remote tool grants. Agent UI only selects multiple entries from that company's
catalog, and agent writes store connection IDs rather than configuration or credentials. MCP calls
require no approval; expose no per-tool approval setting. Preserve selections on unrelated edits,
never echo/copy/export credentials, and show unsupported-harness, unavailable-definition, or
changed-schema errors without silently dropping selections. Company edits affect every selecting
agent; removing an agent selection must not remove a shared company definition.

When switching harnesses, show which configuration fields must change and whether any attached
capability is unsupported. Require explicit target config submission; do not silently clear stored
values in JavaScript or drop granted tools/skills. Apply the same rules server-side without JS.

Update request parsing, form carry/draft fields (`agent_harness_kind`), validation-error rendering,
create/update endpoints, global library JSON, copying, and built-in definitions as needed. An
explicit `rig` choice must survive every round trip. Unknown/unavailable choices and active-run
switch conflicts receive actionable errors with the user's draft retained.

Use runtime capability information to constrain provider/tool choices. Keep the five logical
provider names and company model enablement policy. Do not show a Rig provider/tool as available
before its compatibility fixtures pass. Do not require a new credential when switching harnesses
within the same company connection.

Update simulation's ai-agents-specific base-config display to render a harness-neutral resolved
summary plus the selected harness's reviewed settings. Never display compiled secret-bearing
configuration. Confirm the actual simulation execution path receives the same registry/spec as
ordinary runs; changing a label alone is insufficient.

Keep historical task diagnostic readers tolerant of absent harness-specific fields. Show the actual
harness for new executions without relabeling old ai-agents history as Rig.

## Verification and acceptance

- HTTP/HTML tests cover create/edit, failed validation, no-JS form submission, selector persistence,
  library create/copy, onboarding defaults, and native provisioning defaults.
- API consumers can send either explicit harness value. Omission on create uses the configured
  default; omission on update preserves the stored harness. Test both environment configurations.
- A Rig agent edited only for its description retains its harness and config.
- Harness switching rejects incompatible config and active/suspended task conflicts.
- Simulation executes the selected harness and displays only sanitized configuration.
  Drive its queued execution through the existing test-only simulated model endpoint, including
  a tool-call/result continuation. The product simulation page is not itself a provider stub;
  keep endpoint/scenario selection in test fixture wiring, outside user prompts and agent JSON.
- Existing page snapshot/CSS/navigation checks pass where affected.
