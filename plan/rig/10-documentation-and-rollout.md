# Step 10: Documentation and rollout

Dependencies: step 9. Main files: `README.md`, `docs/custom_tools.md`, `docs/deploy.md`, and a focused
Rig adapter/operator note if the material does not fit those documents.

## Documentation

Explain how to select Rig through the UI and API using `harness_kind: "rig"`, with a tested minimal
`config_json` example. Explain that the displayed ai-agents label corresponds to `ai_agents` on the
wire. Document the implemented property with this precedence table:

| Agent selection | `DEFAULT_AGENT_HARNESS` | Result |
| --- | --- | --- |
| Omitted for a new agent/run | Unset | `rig` |
| Omitted for a new agent/run | `rig` | `rig` |
| Omitted for a new agent/run | `ai_agents` | `ai_agents` |
| Explicit or already stored | Either valid value or unset | Preserve that selection |
| Any | Empty/invalid | Startup configuration error |

Removing the property intentionally restores Rig as the default for new/unresolved selections;
changing it does not migrate already stored agents. Omitted update fields preserve stored values.

Publish the verified provider/tool/skill compatibility table, execution limits, approval/suspension
behavior, and diagnostics fields. Describe Rig as in-process execution, without implying a sandbox.
State that the separate spam/prompt-generation classifier still uses ai-agents.

Document company HTTP MCP setup and agent multi-selection from [step 4a](04a-http-mcp.md), separately
from model credentials. Definitions and secrets are company-owned; agents store references only.
Cover shared configuration changes and their effect on selecting agents, plus
supported transport/authentication, endpoint restrictions, connection tests, selected tool grants,
execution without MCP approval prompts, explicit workflow checkpoints, schema changes, and
remote-effect recovery. Pilot against a controlled MCP
endpoint before enabling connection settings; apply its credential/connection migrations first.

Document that credentials come from company model connections and are not configured via Rig's
example environment keys. Name only options the application actually reads. Distinguish startup
checks, per-agent validation, stubbed contract tests, and optional remote authentication/model tests.
Do not claim all providers or all models Rig supports are available in this application.

Record the exact pinned Rig version/features, any provider transport choices, and the compatibility
bridge for ai-agents built-ins. Upgrades require rerunning provider, approval, suspension, and stack
fixtures; an upstream version bump is not sufficient evidence of compatibility.

## Deployment sequence

1. Set `DEFAULT_AGENT_HARNESS=ai_agents` explicitly for a staged upgrade and apply the additive
   constraint migration under the normal deployment process. Audit old writers before the SQL
   default changes: all must bind `ai_agents` explicitly. If any omit it, coordinate/drain the old
   deployment before applying the default change so old readers cannot encounter unexpected Rig rows.
2. Deploy code that can read both harness values to every web and worker instance. Keep explicit
   creation of Rig agents unavailable until all readers understand `rig`; the default environment
   override alone does not disable explicit choices. Use an implemented rollout mechanism or a
   coordinated deployment if none exists.
3. Verify registry wiring/readiness and exercise ordinary ai-agents traffic. Build output must still
   run as a non-root user; no new runtime privilege or privileged port is required.
4. Pilot an explicitly selected Rig agent in a controlled company/thread. Verify text response,
   native tool execution, approval parking/resumption, a skill, usage, and timeout with approved
   fixtures/targets before using side-effecting customer scenarios.
5. Enable the selector and remove `DEFAULT_AGENT_HARNESS` after the release matrix passes, making
   Rig the default. Keep the explicit ai-agents override only on deployments that choose it.
   Existing agents retain their stored harness; there is no automatic migration or fallback.

## Rollback and operations

Monitor outcomes by harness/provider: completion, suspension, retry, terminal failure, timeout,
usage, and latency. Compare representative workloads while accounting for model differences.
Diagnostic failures must not cause repeated paid calls or repeated tool effects.

The database constraint expansion can remain in place during rollback. An old binary cannot parse
existing `rig` rows, including library definitions, so reverting directly to an ai-agents-only
binary is unsafe. Prefer reverting to a version that still understands both values while disabling
new Rig admission through an implemented control or coordinated operational procedure.
Setting `DEFAULT_AGENT_HARNESS=ai_agents` changes omitted new selections only; it is neither an
admission block for explicit Rig choices nor a migration of existing Rig agents.

To remove Rig completely, inventory referencing agents, definitions, queued tasks, suspended
approvals, and active runs. Drain or explicitly reconcile those runs, then convert configurations
through validated application writes. Do not switch suspended work mid-recipe or mass-rewrite the
harness column without translating its typed config and capabilities. Only then consider deploying
an old binary or narrowing the constraint in a separate reviewed migration.

## Acceptance

- Docs match implemented keys, limits, and verified compatibility; examples pass parser tests.
- A pilot covers ordinary completion and durable suspension/resume on Rig.
- ai-agents regression checks remain green and existing agents retain their runtime.
- A deployment with the property absent creates/runs Rig by default; the explicit ai-agents
  override works and changing the environment leaves stored choices intact.
- Rollout avoids mixed-version readers encountering unsupported stored values.
- Recovery/rollback has a concrete procedure for Rig rows and in-flight work.
