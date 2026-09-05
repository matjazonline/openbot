# Phase 3 — The `ai_agents` adapter

Read [`general_plan_instructions.md`](general_plan_instructions.md) first. Depends on phases 1–2.

**Goal.** Move every `ai_agents` import out of `src/application/` into
`src/adapters/harness/ai_agents/`, behind the phase-2 port. Write the spec→YAML compiler that makes
`AgentCapabilitySpec` executable, including the tool allowlist filter and the skill-tool union.

This is the largest phase and the one with the highest regression risk, because it moves ~2,000
lines of working code. **Move, do not rewrite.** The only new logic is `compile.rs`.

Behaviour after this phase is byte-identical for every existing agent: `spec.granted_tools` and
`spec.skills` are both empty until phase 4 stores anything, so the compiled config is what
`ensure_config_fields` produces today.

---

## Record the stack baseline first

```sh
./scripts/stack-frames.sh > /tmp/frames-before.txt
```

Paste the `AgentRunner::execute` / `AgentTask::run` / `build_agent` numbers here before starting:

Measured on arm64/macOS, debug, at the end of phase 2 (`./scripts/stack-frames.sh target/debug/mail_agents 1`).
Phase 2 added no level to the run chain: its one new `async` level is
`AiAgentsApprovalShim::request_approval` on the approval callback, at 1 KiB, and that callback is
not on this path.

| Frame | Before | After |
|---|---|---|
| `AgentRunner::execute` | 25 KiB | |
| `AgentTask::run` → `AgentHarness::run` | 22 KiB | |
| `build_agent` | 174 KiB | |
| `build_with_tools` | 72 KiB | |
| `builder_with_provider` | 48 KiB | |
| whole task chain | 351 KiB — `run_task` 16 + `run_agents` 42 + `execute` 25 + `run` 22 + `build_agent` 174 + `build_with_tools` 72 (the 347 KiB in `src/AGENTS.md` is the same path, measured before this branch) | |

## Module layout

```
src/adapters/harness/
    mod.rs                      pub mod ai_agents;
    ai_agents/
        mod.rs                  impl AgentHarness for AiAgentsHarness
        compile.rs              AgentCapabilitySpec -> YAML   (new logic, synchronous)
        tools.rs                impl ai_agents::Tool shims over HarnessToolHost
        approval.rs             impl ai_agents::hitl::ApprovalHandler over HarnessApprovals
        hooks.rs                impl ai_agents::AgentHooks over HarnessTrace
        classifier.rs           impl TextClassifier (guardrail + prompt generator)
```

## 1. `compile.rs` — the only new logic

```rust
/// Turn a capability spec into the YAML `AgentBuilder::from_yaml` accepts.
///
/// Synchronous and free-standing: `src/AGENTS.md` prefers a sync helper over another async level,
/// and it makes the allowlist filter a pure, mock-free test. It is also the deepest point of the
/// old `build_agent` frame, which is exactly the frame the stack budget cares about.
fn compile(spec: &AgentCapabilitySpec, api_key: &str) -> AppResult<CompiledConfig>;

pub struct CompiledConfig {
    pub yaml: String,
    /// Grants the allowlist refused, for the caller to log. Never silently dropped.
    pub refused_tools: Vec<ToolId>,
}
```

Steps, in order — the order matters:

1. **Start from `base_agent_config()`.** It moves here from `agent_runner.rs:160` with the
   `ENABLE_AI_AGENTS_OBSERVABILITY` read (`:145`) and the `hitl` / `tool_security` / `context`
   blocks intact.
2. **Merge `spec.extra_config` over it** with `merge_json` (`:436`), exactly as today. Note
   `merge_json` replaces arrays wholesale rather than merging them — that is why step 3 builds
   `tools:` *after* the merge and overwrites it, and why phase 4 forbids `tools` in `config_json`
   rather than trying to reconcile two sources.
3. **Build the tool grant:**
   ```rust
   let mut wanted: Vec<ToolId> = spec.granted_tools.clone();
   for skill in &spec.skills {
       wanted.extend(skill.referenced_tool_ids());   // union — see below
   }
   wanted.extend(tool_host_available);               // the natives this run can actually serve
   let GrantFilter { granted, refused } = retain_grantable(&wanted);
   config["tools"] = granted.iter().map(|id| json!(id.as_str())).collect();
   ```
   **The union is not optional.** `declared_tool_ids` is built only from `tools:`
   (`builder.rs:1190`) and `execute_tool_record_inner` checks every invocation against it
   (`runtime.rs:4784`) — a skill's tool step included, because it arrives through
   `impl ToolInvoker for RuntimeAgent`. A skill referencing `datetime` without `datetime` in
   `tools:` fails mid-run with "not available in the current scope".

   **The filter is not advisory.** `retain_grantable` is the platform's only enforcement point, and
   it is sufficient because nothing else can reach an ungranted tool. Emit only the `Simple(String)`
   `ToolEntry` form; we grant no MCP entries.
4. **Build `skills:`** as inline `SkillDefinition`s:
   ```rust
   json!({
       "id":          skill.slug.as_str(),
       "description": skill.description,
       "trigger":     skill.trigger,
       "steps":       steps,
   })
   ```
   and **nothing else**. `SkillDefinition` is `#[serde(deny_unknown_fields)]`, so one stray key is a
   hard parse failure at `AgentBuilder::from_yaml`. `reasoning`, `reflection` and `disambiguation`
   are `Option` and omitted.

   Instruction → step:
   | `SkillInstruction` | `SkillStep` |
   |---|---|
   | `Prompt { text }` | `{ "prompt": text }` — omit `llm`, we register one provider |
   | `Tool { tool, args, output_as }` | `{ "tool": tool, "args": args?, "output_as": output_as? }`, each `Option` omitted when `None` |

   `SkillStep` is `untagged`, so the discriminator is the key name: a map with `prompt` is a prompt
   step, a map with `tool` is a tool step. Emitting both keys in one map, or neither, fails to
   deserialize with an unhelpful untagged error. Phase 1's `Skill::validate` is what prevents that
   reaching here.
5. **`ensure_config_fields`** (`:464`) moves here unchanged and runs last: it stamps
   `llm.provider/model/api_key`, defaults `max_tokens` to 8192, and defaults `llm.tool_choice` to
   `"auto"` when `tools:` is non-empty. Read the comment at `:518` before touching it — a grant with
   no tool choice runs with **no tools at all, silently**, and now that `tools:` is populated from a
   picker rather than by hand, that failure mode is one checkbox away for every user.
6. `serde_yaml::to_string`.

`append_base_context_prompt` (`:415`) and `full_system_prompt` (`:432`) come along too — the
`UNTRUSTED_INPUT_SYSTEM_PROMPT` + `BASE_CONTEXT_SYSTEM_PROMPT` composition is what the `context:`
block's template variables resolve against, so the two must not be separated.

## 2. `mod.rs` — `impl AgentHarness`

`AgentTask` (`agent_runner.rs:1267–1485`) becomes this module's private executor. `AgentTask::run`
becomes the body of `AgentHarness::run`; `builder_with_provider` (`:1384`) and `build_with_tools`
(`:1424`) come with it verbatim, including their doc comments about why they are synchronous — those
comments record a measured 292 KiB → 174 KiB win and must not be lost in the move.

```rust
async fn run(&self, run: AgentRun<'_>) -> AppResult<AgentExecutionOutput> {
    let CompiledConfig { yaml, refused_tools } = compile(&run.spec, run.api_key)?;
    for id in &refused_tools {
        // A capability that vanishes without a log is a support ticket nobody can answer.
        warn!(tool_id = %id, agent = %run.spec.name, "tool grant is not in the platform allowlist");
    }
    // The provider call descends into the `ai_agents` runtime, whose own `async fn` chain is not
    // ours to shrink. Boxing here caps what this side of the boundary contributes to it.
    let agent = Box::pin(self.build_agent(&yaml, &run)).await?;
    let response = Box::pin(agent.chat(run.full_prompt)).await?;
    …
}
```

Token counting (`count_tokens`, `:1509`), `sanitize_text` (`:121`), the observability report and
`AgentExecutionDiagnostics` all move with it. `sanitize_text` is used by callers outside this
adapter too (`dispatch.rs` logs run failures through it), so re-export it from
`services::harness` and have the adapter use that one — do not leave two copies.

`build_agent` keeps its shape:

```rust
let builder = builder_with_provider(…)?.auto_configure_features()?;
let builder = Box::pin(builder.auto_configure_mcp()).await?;
let builder = Box::pin(builder.auto_configure_spawner()).await?;
build_with_tools(builder, …)
```

`auto_configure_mcp` stays even though we grant no MCP entries — it is a no-op on an empty list, and
removing it is a decision about MCP, not about this refactor.

## 3. `tools.rs` — the shims

Each of the three native tools currently *is* an `ai_agents::Tool`. Split each into:

- **decision logic**, staying in `src/application/services/{outreach,agent_channel,agent_directory}_tool.rs`
  as plain `async fn call(&self, input: Input) -> AppResult<Output>` plus a `fn schema() ->
  serde_json::Value`;
- **a shim** here, `impl ai_agents::Tool`, that deserializes args, calls the host, and wraps the
  result.

The shim is uniform enough to be one generic type parameterised by `ToolId`, dispatching through
`HarnessToolHost::invoke`. Resist writing three near-identical shims.

Sizes to expect: `outreach_tool.rs` is 1,106 lines, `agent_directory_tool.rs` 551,
`agent_channel_tool.rs` 296. Almost all of that is decision logic that does not move — the
`ai_agents::Tool` impl is the top ~60 lines of each.

`OutreachAndAwaitQuorumTool` carries `with_policy_config` and the `suspended` flag, and
`CreateAgentChannelTool` is idempotent via a `request_hash`. Both are behaviour, not plumbing; they
stay on the application side.

## 4. `approval.rs` and `hooks.rs`

Thin translation, both directions:

```rust
impl ai_agents::hitl::ApprovalHandler for ApprovalShim {
    async fn request(&self, req: ApprovalRequest) -> ApprovalResult {
        let ask = match &req.trigger {
            ApprovalTrigger::Tool { name, args }       => ApprovalAsk::Tool { name, args },
            ApprovalTrigger::Condition { name, matched } => ApprovalAsk::Condition { name, matched: *matched },
            ApprovalTrigger::State { from, to }        => ApprovalAsk::State { from, to },
        };
        match self.approvals.decide(ask).await {
            Ok(ApprovalVerdict::Approved)          => ApprovalResult::Approved,
            Ok(ApprovalVerdict::Rejected { reason })=> ApprovalResult::rejected_with_reason(reason),
            Err(e)                                  => ApprovalResult::rejected_with_reason(e.to_string()),
        }
    }
}
```

The `Err` arm reproduces `agent_runner.rs:784` exactly — a failed approval lookup rejects. That is
the fail-closed choice and it is deliberate; keep the arm explicit rather than collapsing it into a
`?`, per *"Don't collapse errors into defaults on authorization paths"*.

`hooks.rs` does the same for `AgentHooks` → `HarnessTrace`, mapping `ToolCallSource` to
`ToolTraceSource`.

## 5. `classifier.rs` — the two other `ai_agents` agents

`llm_guardrail.rs` builds a second full agent for spam/injection classification
(`AgentBuilder::from_yaml(&classifier_config_yaml(provider, model, api_key)?)`), and
`use_cases/agent.rs:941` builds a third for the "generate a system prompt" helper. Both must move
behind a port or `src/application/` does not reach zero `ai_agents` imports.

```rust
// src/application/services/harness/mod.rs
#[async_trait]
pub trait TextClassifier: Send + Sync {
    /// One-shot completion against the caller's own provider credential. No tools, no memory,
    /// no approvals — the two callers both want a single classified answer, not an agent.
    async fn complete(&self, request: ClassificationRequest<'_>) -> AppResult<String>;
}
```

Deliberately narrower than `AgentHarness`: *"Split a broad trait instead of adding optional methods
with safe-looking defaults."* The guardrail wants a string back, not an `AgentExecutionOutput`.

`LlmSpamGuardrail::static_pattern_check` is offline and stays exactly where it is — it runs before
any LLM call and is the cheap rejection.

## 6. Split `agent_runner.rs`

2,933 lines against `src/AGENTS.md`'s ~1,000-line threshold, with a test module well past ~500.
After the move it should be roughly:

```
src/application/services/agent_runner/
    mod.rs            AgentRunner: compose, guardrail, resolve harness, record metrics
    params.rs         ResolvedAgentParams, resolve_agent_params, merge_json
    prompt.rs         compose_prompt, render_history, full_system_prompt
    diagnostics.rs    AgentExecutionDiagnostics, attach_*, estimate_tokens
    mod_tests.rs      #[cfg(test)] #[path = "…"] mod tests;
```

Do the split as its own commit, after the move compiles and tests pass. Two large mechanical changes
in one diff is how a regression hides.

---

## Tests

**Compiler unit tests** (`compile.rs`, pure, no mocks) — these carry the safety properties:

- `command_is_dropped_from_the_grant` and, separately,
  `command_is_dropped_even_when_it_arrives_through_extra_config` — the second is the one that proves
  step 3 runs after step 2
- `every_host_access_builtin_is_dropped` — loop the 18 absentees
- `a_skills_referenced_tools_are_unioned_into_the_grant`
- `a_skill_referencing_an_ungrantable_tool_loses_the_grant_and_is_reported` — the skill still
  compiles; the run will deny the step. Decide and assert: refuse at compile time instead if that
  reads better, but do not leave it undefined
- `the_compiled_yaml_deserializes_into_ai_agents_agent_spec` — **the important one**. Because
  `SkillDefinition` and `SkillStep` are `deny_unknown_fields`, this round-trip is what catches an
  extra emitted key. Assert on the parsed `AgentSpec`, never on the YAML string
- `a_prompt_only_skill_emits_no_tool_key_and_vice_versa` — the untagged discriminator
- `tool_choice_defaults_to_auto_when_the_grant_is_non_empty` — port the existing test at
  `agent_runner.rs:2198` rather than rewriting it
- `every_allowlisted_id_exists_upstream` — moved here from phase 1, using
  `ai_agents::tools::create_builtin_registry()`

**Regression tests**: every existing test in `agent_runner.rs`'s test module must still pass,
unmodified where possible. If one needs editing to compile, that is a signal the move changed
behaviour — check why before editing it.

**Stack budget**, the one that fails the build if this went wrong:

```sh
./scripts/stack-frames.sh > /tmp/frames-after.txt
diff /tmp/frames-before.txt /tmp/frames-after.txt
./scripts/stack-budget.sh          # must pass at the stock 2 MiB
```

Fill in the table at the top of this file either way.

## Done when

```sh
grep -rn "ai_agents" src/application/ src/domain/     # returns nothing
cargo test
./scripts/stack-budget.sh
```

and an end-to-end mail round trip produces the same reply it did before the phase.
