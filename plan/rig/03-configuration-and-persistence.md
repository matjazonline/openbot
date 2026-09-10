# Step 3: Typed configuration and persistence

Dependencies: step 1 and execution-bound decisions from steps 2/6. Main files:
`src/domain/entities/harness.rs`, `src/application/use_cases/agent.rs`,
`src/adapters/persistence/agent.rs`, `src/infra/config.rs`, and a new migration.

## Changes

Add `HarnessKind::Rig`, `as_str() == "rig"`, a human label, and inclusion in `ALL`. Make Rig the
type's static default, while retaining `ai_agents` as the existing wire value. Update stale comments
that describe only one harness.

Add `AppConfig.default_agent_harness`, parsed once from `DEFAULT_AGENT_HARNESS`. Absent means Rig;
explicit `rig` or `ai_agents` selects that default. Trim surrounding whitespace, but reject blank or
unknown values at startup. Keep environment reads in infra; domain defaults never read process env.

Carry an omitted harness as `Option<HarnessKind>` at external create boundaries until the application
can apply the injected configured default. Audit serde defaults, form parsing, constructors,
`map_or_else(HarnessKind::default, ...)`, onboarding, native provisioning, and no-agent execution:
none may replace an omission with Rig before an explicit environment override can be applied.
On update, omission preserves the stored harness. Explicit library definitions retain their choice;
product presets intended to inherit deployment policy must represent that omission explicitly.
Persist the resolved selection so later environment changes affect only unresolved/new selections.

Add `HarnessConfig::Rig(RigAdvancedConfigV1)` and extend `empty`, `parse`, and `to_json`. The
ai-agents accessor must return `None` for Rig, and the Rig compiler must reject a mismatched spec.
Use the existing path-reporting parser, byte bound, version validation, and unknown-field rejection.

Start with this proposed shape, subject to verified Rig support:

```json
{
  "version": 1,
  "max_turns": 8
}
```

`max_turns` is an optional requested reduction of the server's model-call ceiling: default 8,
accepted range 1–16, with the effective value additionally capped by server execution policy.
Count the initial call, continuations, recovery attempts, and skill prompt calls against the same
run budget. Establish these as explicit new limits; do not infer them from ai-agents reasoning
iterations, which are a different resource.

Keep `{ "version": 1 }` as the canonical empty JSON so current null/default normalization in agent
validation and persistence remains valid. More sampling/reasoning options can be added only with
provider-specific compatibility checks and enforcement tests; V1 does not accept free-form
`additional_params` or transplant ai-agents `reasoning`, `reflection`, and `disambiguation` fields.

Model, credential, endpoint, tools, approval policy, lease/retry policy, and output/prompt ceilings
remain outside advanced agent JSON. Preserve separate `native_tool_policy`, grants, and skills.

Add company-owned HTTP MCP definitions, encrypted credentials, and remote tool grants, plus a
many-to-many agent selection table as specified in [step 4a](04a-http-mcp.md). Agents store only
company connection IDs, outside `config_json`; no endpoint settings or credentials are copied onto
agents. Preserve selections on unrelated edits and enforce company/selection revisions, shared
credential rotation, and cross-company foreign keys.

Add an additive migration replacing `agents_harness_kind_check` with a constraint accepting exactly
`ai_agents` and `rig`. Set the column's static default to `rig` without rewriting existing row
values. Every production application insert must bind its resolved harness explicitly, so SQL's
static default cannot bypass `DEFAULT_AGENT_HARNESS=ai_agents`. Coordinate this default change with
the mixed-version rollout in step 10. Do not edit the applied initial schema or reset the database.
Check other tables/JSON envelopes that embed agent
definitions before concluding no additional constraint needs widening.

On an explicit harness change, validate the submitted target configuration as one update. Reject
incompatible carried configuration with an actionable field error; do not silently discard it.
Implement step 1's rule for active/suspended tasks at the application write boundary, including
library-derived agent updates and concurrent run admission if that rule requires locking.

## Verification and acceptance

- Both variants round-trip through serde, persistence, API payloads, and library copying.
- Existing rows retain their explicit harness. Omitted creates use Rig with no env property and
  ai-agents with the override; omitted updates preserve the stored selection. Null config uses the
  resolved harness's empty typed config.
- Test configuration parsing through a pure function, without racing process-environment mutation
  in parallel tests. Cover unset, both valid values, blank, unknown, and explicit-agent precedence.
- Unknown harness/version/field, cross-harness JSON, zero/excessive turns, and oversized config fail.
- A database fixture proves both valid values and rejects an unknown value, on a fresh database and
  on an upgrade containing existing ai-agents rows.
- Any changed SQL regenerates committed `.sqlx/` metadata. Test the actual active-run edit fence
  with competing operations if introducing or changing that concurrency protocol.
