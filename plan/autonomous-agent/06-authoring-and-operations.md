# 06 — Agent authoring and execution visibility

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [05](05-provisioning-children-and-samples.md). Users configure and publish reusable
agents from the agent page, select them in workflows, and can identify exactly which revision a
run used. UI and API commands share the application authorization and validation boundary.

## Ownership anchors

- [Agent routes](../../src/adapters/http/routes/agent.rs),
  [agent UI routes](../../src/adapters/http/routes/ui_agents.rs), and
  [agent settings](../../src/adapters/http/pages/agent_settings.rs).
- [Existing workflow tool editor](../../src/adapters/http/routes/workflows/ui/authoring/tools.rs)
  and [its pages](../../src/adapters/http/pages/workflow_authoring/tools.rs): move agent grant
  ownership to agent authoring while reusing appropriate registration controls.
- [MCP routes](../../src/adapters/http/routes/mcp.rs) and
  [MCP UI](../../src/adapters/http/routes/ui_mcp.rs): reuse company connection/catalog management.
- [Company skill pages](../../src/adapters/http/pages/company_skills.rs): keep shared skill
  management and show how changes enter a new agent revision.

## Batch 06A — Configure, validate, and publish agents

1. Add agent-owned tool selection for built-ins, company HTTP operations, reviewed MCP tools on
   explicit connections, and registered workflow tools. Display the actual target beside the
   agent-visible alias; do not make users type workflow slot names or ambiguous global names.
2. Reuse ordered skill assignment and expose missing required tools with actionable validation.
   Discovery, connection grant, reviewed operation policy, and agent assignment are distinct
   permissions; selecting a tool must not silently authorize the earlier stages.
3. Provide draft save, validation/preview, and publication through the shared application ports.
   Show draft versus active revision clearly. Failed validation or stale expected revisions keep
   user input and leave the active agent unchanged; replay follows publication idempotency rules.
4. Explain publication in product terms: future runs use the new revision, running work keeps its
   revision, and removed/revoked access may stop running work. Expose immediate revocation with
   the actual affected binding identity and existing authorization requirements.
5. Preserve authentication/CSRF, company scoping, escaped rendering, bounded discovery/results,
   accessible labels/focus/error states, and existing partial-page navigation behavior. Neither
   HTML nor general JSON detail/list endpoints may disclose credentials.

Focused checks: `autonomous_agent_authoring_` route/form/API tests for round trips, unauthorized
and foreign-tenant IDs, stale edits/publication, skill errors, restricted built-ins, misleading
aliases, and credential omission. Add focused browser interaction coverage where dynamic selectors
or navigation behavior changes; retain injection/session-revocation coverage.

## Batch 06B — Show workflow references and run provenance

1. Workflow authoring selects an agent identity and displays its current capabilities without
   requiring duplicated MCP slots. Clearly identify capability previews as current information;
   they are not the revision that every future run is guaranteed to use.
2. Run and tool-call views show actual agent revision, binding/target identity, workflow version,
   and child dependency versions from the durable run manifest. Historical views must not rebuild
   labels or instructions from today's agent configuration and misrepresent what executed.
3. Report publication/admission failures, revoked capability, unavailable connection, and invalid
   reviewed schema distinctly. Correlate events with company/run/agent/revision/call identifiers;
   do not log prompts, secrets, raw authorization, or sensitive arguments.
4. Bound revision history and dependency views. Keep queries tenant-scoped and avoid per-row
   repeated hydration. Show explicit truncation for any bounded operational projection.
5. Update user/API documentation, examples, generated samples, and built-in agent guidance. State
   that agent revisions update independently, that retries retain their selections, and that
   classify/decide remain model-only. Do not promise unrestricted standalone agent execution.

Focused checks: `autonomous_agent_views_` tests for v1/v2 provenance, unchanged workflow version,
child views, revoked targets, bounded history, and access-controlled projections. Run affected
agent/runtime, workflow-run, conversation, and sample browser protocols.

## Acceptance and evidence

A user can create an MCP-enabled agent once, publish it, and choose it in two workflows without
editing either workflow's resources. After publishing the agent again, users can distinguish old
and new runs accurately. Record focused commands/results and register any new required browser
gate in CI. Proceed to [07](07-integration-and-verification.md).
