# Rig harness implementation plan

Add **Rig** as the default in-process agent harness alongside selectable **ai-agents**. When
`DEFAULT_AGENT_HARNESS` is absent, an omitted harness selection resolves to `rig`. Setting
`DEFAULT_AGENT_HARNESS=ai_agents` changes that deployment default. Explicit agent selections,
including existing stored `ai_agents` values, take precedence. Model creation uses an adapter-owned
`ProviderRegistry`.

This is an implementation plan, not a description of shipped functionality. No application code,
dependencies, database state, or defaults are changed by saving these files.

## Repository baseline

- `src/domain/entities/harness.rs` owns `HarnessKind`, versioned `HarnessConfig`, and
  `AgentCapabilitySpec`. The only current variant is `AiAgents`.
- `src/application/services/harness/` already owns `AgentHarness`, `HarnessRegistry`,
  `HarnessToolHost`, `HarnessApprovals`, `HarnessTrace`, and the separate `TextClassifier` port.
- `src/application/services/agent_runner/` resolves company credentials and capabilities,
  composes prompts, runs the guardrail, and dispatches through the registry.
- `src/adapters/harness/ai_agents/` compiles configuration and adapts tools, approvals, and traces.
  Its runtime also supplies the ten grantable built-in tools and executes structured skills.
- `src/infra/setup.rs` registers ai-agents and separately installs `AiAgentsTextClassifier`.
- Agent persistence already stores `harness_kind` and `config_json`. The initial migration's
  `agents_harness_kind_check` accepts only `ai_agents`.
- The settings UI iterates `HarnessKind::ALL`, but disables its single-option selector and contains
  ai-agents-specific advanced-config copy. Simulation also reads the ai-agents base configuration.

## Steps

| Step | File | Depends on |
| --- | --- | --- |
| 1 | [Architecture and compatibility contract](01-architecture-and-compatibility.md) | Existing code |
| 2 | [Pinned dependency and ProviderRegistry](02-dependency-and-provider-registry.md) | 1 |
| 3 | [Typed configuration and persistence](03-configuration-and-persistence.md) | 1; bounds agreed with 2 and 6 |
| 4 | [Tool declarations and execution bridge](04-tool-bridge.md) | 1–3 |
| 4a | [Company HTTP MCP catalog and agent selections](04a-http-mcp.md) | 2–4; recovery completed with 6 |
| 5 | [Skills and runtime context](05-skills-and-context.md) | 4 |
| 6 | [Bounded execution, approvals, and suspension](06-execution-and-approvals.md) | 2–5 |
| 7 | [Application wiring and diagnostics](07-wiring-and-diagnostics.md) | 6 |
| 8 | [API, settings, library, and simulation](08-api-and-user-interface.md) | 3, 7 |
| 9 | [Contract tests and CI gates](09-verification-and-ci.md) | 1–8 |
| 10 | [Documentation and rollout](10-documentation-and-rollout.md) | 9 |

Add focused verification with each step. Step 9 completes the release matrix; it is not the first
time the implementation is tested. Keep the new option out of a released UI until runtime and
capability support are ready.

## Scope decisions

Default selection has one precedence rule: explicit request/definition or existing stored agent
selection, then `AppConfig.default_agent_harness`, whose unset-environment value is `Rig`.
Resolve omissions before persistence; do not reassign existing agents whenever environment changes.
Accept only `rig` and `ai_agents` for the proposed environment property. Empty/invalid values fail
startup instead of silently choosing a runtime. The property is implemented and tested in steps 3/7.

Target all five currently allowed logical providers: `google`, `openai`, `anthropic`, `groq`, and
`xai`. Company connections remain the source of credentials and enabled models. Provider support
must be demonstrated with tool-call fixtures; advertising a provider because its text endpoint
works is insufficient.

Reuse our existing simulated agent endpoint (`scripted_llm`) for model-backed tests. It already
emits tool calls that the real harness executes. Extend it with explicit request-checked scenarios,
provider-specific wire formats, and local-only network execution; select scripts in test setup
rather than by magic strings in prompts. [Step 9](09-verification-and-ci.md) specifies the fixture
contract and coverage; begin that support with the dependency proof in step 2.

Preserve native-tool authorization, approval, suspension, delegation restrictions, memory context,
and ordered skill execution. Rig is in process and confers no sandbox privileges. Do not enable
shell, filesystem, provider-hosted tools, or new providers as a side effect. Company-owned HTTP MCP
definitions/credentials/tool grants and multiple selections per agent are in scope through step 4a;
agents store references only. MCP tool calls require no approval. Model-selected arbitrary
endpoints, local MCP processes, and automatic grants from discovery remain excluded.

Leave the separate deployment-wide classifier on `AiAgentsTextClassifier` in this release; a Rig
agent still uses the existing spam guardrail and prompt-generation path. Replacing that classifier,
removing ai-agents, streaming the UI, and adding Rig-managed memory/vector stores are separate work.

## Sources and version policy

Research checked on 2026-09-09. The requested
[dynamic model creation playbook](https://book.rig.rs/playbook/dynamic-model-creation.html)
is the registry design reference. The [Rig API documentation](https://docs.rs/rig/latest/rig/)
served version **0.42.0** during research. Use that as the candidate baseline, then compile and pin
the chosen published release in step 2. Do not mix older `rig-core` examples with newer facade APIs.
The [upstream migration guide](https://github.com/0xPlaygrounds/rig/blob/main/MIGRATING.md) is a
version-compatibility reference, not a dependency on upstream `main`.

Names for new Rust types/files below are proposed implementation targets. Configuration examples
are proposed schemas until the corresponding parser and enforcement exist. Repository and
subsystem `AGENTS.md` rules apply, including additive migrations, SQLx metadata regeneration,
dependency direction, cancellation, and the stock-stack CI budget.
