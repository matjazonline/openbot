# Pluggable agent harness, with skill and tool libraries — shared instructions

Read this before any phase file. Each phase lives in `plan/agent_harness/phase{n}.md` and assumes
everything here.

| Phase | File | Goal |
|---|---|---|
| 1 | [`phase1.md`](phase1.md) | Domain: `HarnessKind`, `AgentCapabilitySpec`, `Skill`, the tool catalogue — pure, no DB, no `ai_agents` |
| 2 | [`phase2.md`](phase2.md) | `AgentHarness` port + `HarnessRegistry`, and the two sub-ports that keep `ai_agents` out of the application layer |
| 3 | [`phase3.md`](phase3.md) | `adapters/harness/ai_agents/` — the spec→YAML compiler and every `impl ai_agents::*` move |
| 4 | [`phase4.md`](phase4.md) | Schema, persistence, `SkillUseCases`, and closing the `config_json` escape hatch |
| 5 | [`phase5.md`](phase5.md) | Dispatch wiring: resolve a harness per agent, apply the sub-agent scope |
| 6 | [`phase6.md`](phase6.md) | UI: company skill library, global skill library, agent capability picker |

Phases 1–3 are one landable unit — a pure refactor with identical behaviour, which is the point:
nothing in it should change a single agent's output. Phase 4 adds storage, 5 makes it reachable,
6 makes it editable. Do not start 4 until `cargo test` is green on 1–3 *and* `scripts/stack-budget.sh`
still passes.

---

## Context

Agents run on exactly one runtime: the `ai-agents` crate (git dep pinned at rev
`9ea972e3e3a5b777496c6d6b1b471cac4513a1e4`, `Cargo.toml:28`), called directly from
`src/application/services/agent_runner.rs`. Three problems follow, and this plan fixes all three.

**The harness is not abstracted, and it leaks upward.** `ai_agents` is imported by nine files under
`src/application/`: `agent_runner.rs`, `agent_trace_hooks.rs`, `outreach_tool.rs`,
`agent_channel_tool.rs`, `agent_directory_tool.rs`, `llm_guardrail.rs`, `use_cases/agent.rs`,
`use_cases/approval.rs`, `services/test_support.rs`. `src/AGENTS.md` — "Preserve dependency
direction" — says the application layer describes the ports it needs and adapters implement them.
An external agent runtime is exactly that kind of thing. There is no seam at which a second harness
could attach. `plan/opencode_microvm_sandbox_plan.md` is a written-but-unbuilt design for one, and
it predates the current `AgentRunner`/`AgentTask` split, so its proposed seam no longer exists.

**Capability is a free-form blob.** What an agent may do lives in `agents.config_json` — which *is*
the `ai-agents` YAML schema, deep-merged over `base_agent_config()` by `merge_json`
(`agent_runner.rs:436`) and edited through a raw `<textarea name="config_json" rows="4">` on the
agent settings page (`pages/agent_settings.rs`, inside the collapsed "Custom model & config"
`<details>`). Granting a tool means hand-typing `{"tools": [{"name": "..."}]}`. There is no library
to pick from, no validation, and no way to express a grant a *different* harness could honour.

**Nothing constrains which built-in tools an agent gets.** `auto_configure_features()` registers all
30 `ai-agents` built-ins into the tool registry. Only the `tools:` list decides what is granted — and
that list is whatever someone typed into the textarea. `command`, `file_write`, `file_edit`,
`patch`, `delete_path`, `move_path` and `copy_path` execute **in this process, on this host**. There
is no sandbox. A grant of `command` is remote code execution reachable by prompt injection through
inbound mail.

The outcome, in the shape of vercel.com/eve: an agent is configured by *selecting* from libraries —
skills, tools, and (already) channels and sibling agents — and a harness-neutral spec describes that
selection. Each harness compiles the spec into its own dialect: `ai-agents` gets YAML `skills:` with
`steps:` and a `tools:` grant list; a future markdown-driven harness flattens the same skills into
one `.md` document.

---

## Decisions already taken — do not relitigate

1. **Scope is the whole thing** — spec, port, adapter, libraries and UI. The request is literally
   "an abstract way to define harness", and the skill design turns on "depending on harness …
   transform instructions into steps", which only means something once the port exists.
2. **Skills have no `steps` column.** One `instructions` JSONB array of *typed values*, compiled per
   harness. `ai-agents` gets `steps:`; another harness gets one combined markdown document.
3. **The skill library is copy-on-pick**, mirroring `create_agent_from_library`
   (`use_cases/agent.rs:560`) — picking a global skill inserts a company-owned copy. Operator fixes
   do not propagate; companies can diverge.
4. **The tool library is a hardcoded catalogue**, not a table. The `ai-agents` built-ins are a
   compile-time const upstream; our three native tools are already consts. Nothing to store.
5. **The built-in tool allowlist is static, with no exceptions and no environment override.** An id
   absent from it never reaches the YAML, for every agent on the platform. This was asked for
   explicitly; do not add an env var "for flexibility".
6. **Sub-agents keep today's behaviour by default.** An optional allowlist restricts only when
   non-empty.
7. **Sandbox is not built here.** `HarnessKind` is where it attaches.

---

## What the pinned `ai-agents` revision actually accepts

Verified by reading the vendored checkout at
`/Users/mac03/.cargo/git/checkouts/ai-agents-88620047fa1a3df1/9ea972e/`, not from documentation
(`https://ai-agents.rs/docs/` returns 403 to automated fetches). Re-read the source, not the site,
whenever this is revisited — and re-read it if the pinned rev moves.

**`SkillDefinition`** — `crates/ai-agents-skills/src/definition.rs:9`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillDefinition {
    #[serde(alias = "skill")]
    pub id: String,
    pub description: String,
    pub trigger: String,
    pub steps: Vec<SkillStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub reasoning: Option<ReasoningConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub reflection: Option<ReflectionConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub disambiguation: Option<SkillDisambiguationOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum SkillStep {
    Tool   { tool: String, #[serde(default)] args: Option<Value>, #[serde(default)] output_as: Option<String> },
    Prompt { prompt: String, #[serde(default)] llm: Option<String> },
}
```

`id`, `description` and `trigger` are all **required and non-optional** — every one becomes a `NOT
NULL` column in phase 4. `deny_unknown_fields` on both types means an extra emitted key is a hard
parse failure, which is why phase 3's compiler gets a round-trip test rather than a string
comparison.

**A skill must end on a prompt step.** `SkillExecutor::execute_with_invoker`
(`crates/ai-agents-skills/src/executor.rs:78`) returns only when `index == skill.steps.len() - 1`
on a `Prompt` step, and otherwise falls out of the loop into
`Err(AgentError::Skill("Skill has no prompt step to generate response"))`. A skill ending on a tool
step compiles fine and fails at run time. Phase 1 rejects it at write time.

**`ToolEntry`** — `crates/ai-agents-runtime/src/spec/tool.rs:18` — is untagged
`Simple(String) | Structured { name, type, ..flattened }`, where `type: "mcp"` deserializes into
`MCPWrapperConfig`. We emit only the `Simple` form.

**`BUILTIN_TOOL_IDS`** — `crates/ai-agents-tools/src/builtin/mod.rs:44` — is a hardcoded
`[&str; 30]`:

```
calculator  echo       datetime   json       random     file
glob        grep       file_read  file_write file_edit  patch
copy_path   move_path  delete_path file_list file_info  git_status
git_diff    diagnostics ask_user  todo       sleep      web_fetch
web_search  command    text       template   math       http
```

`auto_configure_features()` (`crates/ai-agents-runtime/src/builder.rs:407`) registers **all of them**
into the registry whenever `self.tools.is_none()`.

**The `tools:` list is the only grant, and it is enforced on every call.** `declared_tool_ids` is
built solely from `spec.tools` plus explicit spawner/persona feature grants
(`crates/ai-agents-runtime/src/builder.rs:1190`), and `execute_tool_record_inner`
(`crates/ai-agents-runtime/src/runtime.rs:4784`) denies any invocation whose canonical id is not in
the current scope:

```rust
let initial_scope_snapshot = self.get_available_tool_ids_snapshot().await?;
if !initial_scope_snapshot.tool_ids.iter().any(|id| id == &canonical_id) {
    // ... ToolPolicyDecisionRecord::deny("Tool '{}' is not granted by the current
    //     top-level and state tool scope")
}
```

This runs for model-initiated calls **and** for skill tool-steps, which reach it through
`impl ToolInvoker for RuntimeAgent` (`runtime.rs:12542`). Two consequences the phases depend on:

- Filtering the compiled `tools:` list is a **complete** enforcement point. Nothing else can reach an
  ungranted tool, so the allowlist needs no second checkpoint inside the tools themselves.
- A skill's tool steps **must** be unioned into the agent's `tools:` grant, or the skill dies
  mid-run with a denied step. Phase 3 does that union; phase 1 supplies
  `Skill::referenced_tool_ids()`.

**`AgentSpec`** — `crates/ai-agents-runtime/src/spec/mod.rs:46` — the full surface `config_json` can
reach: `name, version, description, system_prompt, llm, llms, skills, memory, storage, tools,
max_iterations, max_context_tokens, error_recovery, tool_security, process, context, states,
parallel_tools, streaming, hitl, reasoning, reflection, disambiguation, observability, runtime,
tool_aliases, metadata, spawner, persona`. The server already owns and overwrites `name`,
`system_prompt`, `llm.{provider,model,api_key,max_tokens,tool_choice}`, `observability`, `hitl`,
`tool_security` and `context`. After phase 4 it also owns `tools` and `skills`.

---

## The execution path as it stands

Every one of these is load-bearing for some phase.

```
inbound_event_worker → thread/ingest → background_tasks
        ↓
task_worker::run_task (:873) → while_leased(…)          ← lease heartbeat, 15 min
        ↓
thread/dispatch::run_agents (:791)                       ← per matched channel, pipeline order
        ↓  first_agent_for(channel)                      ← channel_agents position 0
           resolve_agent_params(…)                       ← company credential + merged config
           memory.recall(…) appended to prompt
           tokio::time::timeout(agent.run_timeout(global), runner.execute())
        ↓
AgentRunner::execute (agent_runner.rs:1020)
   compose_prompt(UntrustedFence) → LlmSpamGuardrail::evaluate → ensure_config_fields
   → serde_yaml::to_string(config) → AgentTask{ config_yaml, … }.run()
        ↓
AgentTask::run (:1294)
   Box::pin(self.build_agent()).await
   Box::pin(agent.chat(&self.full_prompt)).await
```

- **`ResolvedAgentParams`** — `agent_runner.rs:255`, `{provider, model, api_key, config}`. Built by
  `resolve_agent_params` (:377) from the company's encrypted model connection; an agent may pick a
  model only within a provider the company enabled. Provider allowlist hardcoded at :306 and again
  in `use_cases/company.rs:51`: `google | openai | anthropic | groq`.
- **`base_agent_config()`** — :160. Server-owned defaults: `observability` (gated by
  `ENABLE_AI_AGENTS_OBSERVABILITY`, read directly from the environment at :145, not through
  `AppConfig`), `hitl` with per-tool `require_approval`/`approval_context`, `tool_security` with
  per-tool `timeout_ms`/`max_output_chars`/`config`, and `context` sources.
- **`ensure_config_fields`** — :464. Stamps `llm.provider/model/api_key`, defaults `max_tokens` to
  8192 (the SDK's own default of 2,048 truncated long replies mid-sentence), and defaults
  `llm.tool_choice` to `"auto"` when `tools:` is non-empty. That last one exists because a grant
  without a tool choice runs with **no tools at all, silently** — read the comment at :518 before
  touching this.
- **`AgentApprovalHandler`** — :599–:790, `impl ai_agents::hitl::ApprovalHandler`. Hashes a
  `step_key`, checks for a prior decision, otherwise emails an approval link, sets `suspended` and
  returns `rejected_with_reason`, which becomes `AgentExecutionDisposition::Suspended` and leaves the
  durable task open.
- **`InternalDelegationPolicy`** — :575. Classifies outreach recipients against the channel directory
  so an all-internal call can skip human approval. This is an authorization decision.
- **The three native tools** — `outreach_and_await_quorum` (`outreach_tool.rs`, 1,106 lines),
  `create_agent_channel` (`agent_channel_tool.rs`), `list_company_agents`
  (`agent_directory_tool.rs`). Registered in `build_with_tools` (:1424) only when the run supplies
  the matching context. Registration is **not** a grant — see `docs/custom_tools.md`.
- **Two more `ai_agents` agents exist** outside the runner: the spam/injection classifier in
  `llm_guardrail.rs` (`AgentBuilder::from_yaml(&classifier_config_yaml(…))`) and the
  "generate a system prompt" helper at `use_cases/agent.rs:941`. Both need the phase-3
  `TextClassifier` port, or `src/application/` does not end at zero `ai_agents` imports.

### What deliberately does not change

`compose_prompt`, `render_history`, `UntrustedFence`, `LlmSpamGuardrail::static_pattern_check`,
memory recall, the `tokio::time::timeout` wall clock, the `while_leased` heartbeat, the
`Suspended`-leaves-the-task-open rule, and dispatch's "if any matched agent failed, none of them may
land" (`dispatch.rs:718`). If a phase finds itself editing one of these, stop and re-read the phase
file — it has drifted.

---

## The stack budget is the constraint that will bite

Root `AGENTS.md`: *"A raised limit must leave something that fails when it is approached again."*
`.cargo/config.toml` gives test threads 16 MiB and `RUNTIME_THREAD_STACK_BYTES` defaults to the same,
because the task-worker → dispatch → agent-runner chain once reached 1,997 KiB of a 2,080 KiB stack
and aborted the process inside `serde_yaml` parsing an unremarkable agent config. The chain is 347
KiB today. `scripts/stack-budget.sh` re-runs the suite at the stock 2 MiB and is the thing that
actually catches regression.

This plan adds a `dyn` dispatch level to exactly that chain. From `src/AGENTS.md`, in order of what
actually helps:

1. **Extract a synchronous helper.** A non-`async fn` contributes no future and no frame. This is
   why the phase-3 spec→YAML compiler is a sync free function, and why `Skill::validate` and the
   allowlist filter are sync.
2. **Never add a forwarding `async fn`.** `AgentHarness::run` replaces `AgentTask::run`; it does not
   wrap it.
3. **`Box::pin` the seam, with a comment saying why.** `AgentHarness::run` is that seam — one boxed
   call standing where `Box::pin(self.build_agent())` and `Box::pin(agent.chat(..))` stand today.

Record `scripts/stack-frames.sh` numbers **before phase 2 and after phase 3** in `phase3.md`, and
keep `clippy::large_enum_variant` enabled — `AgentRun<'_>` and `AgentCapabilitySpec` both cross an
async boundary.

---

## Migration style

**Reset and edit in place.** New tables and columns go into
`migrations/20260817000000_init_schema.sql` itself; both databases are recreated because the file's
checksum changes. `company_resend_integrations` set this precedent on the current branch, against
`src/adapters/persistence/AGENTS.md` §1's additive-migration rule — the reset workflow is the one in
force.

```sh
dropdb --if-exists mail_agents      && createdb mail_agents
dropdb --if-exists mail_agents_test && createdb mail_agents_test
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" sqlx migrate run
```

Schema conventions, all uniform across the ~45 existing tables: app-side `UUID PRIMARY KEY` (never
`gen_random_uuid()`); `TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP` (a test at
`src/adapters/persistence/mod.rs:121` fails the build on a naive timestamp); **no Postgres enum
types** — `TEXT` plus a named `CHECK (col IN (...))` mirrored by a Rust enum; `CITEXT` for slugs and
emails; composite tenancy FKs (`REFERENCES agents(company_id, id)`) backed by a
`UNIQUE (company_id, id)` on the parent; `ON DELETE CASCADE` from `companies(id)`; named
`CHECK (btrim(x) <> '')` on every user-supplied name; bounded arrays; and a prose rationale comment
above every table.

---

## House rules that bite on this change

Full rules in `AGENTS.md`, `src/AGENTS.md`, `src/application/AGENTS.md`,
`src/adapters/persistence/AGENTS.md`, `src/adapters/http/AGENTS.md`,
`src/adapters/http/pages/AGENTS.md`. The ones this feature walks straight into:

- **Preserve dependency direction.** The whole point. Application code must not import `ai_agents`,
  `sqlx`, `axum` or `lettre`, and *"an abstraction must not live inside the outer adapter it is
  intended to abstract"* — `HarnessRegistry` lives beside the code that consults it, in
  `src/application/services/`, not under `src/adapters/harness/`. `TransportRegistry`
  (`src/application/transport/ports.rs`) carries the same comment explaining why.
- **Ports have no defaulted correctness methods.** `src/application/AGENTS.md` is explicit: a
  silently-successful default is how a broken protocol passes its tests. `AgentHarness`,
  `HarnessApprovals` and `HarnessToolHost` get no default bodies. Contrast `AgentPersistence`
  (`use_cases/agent.rs:168`), whose `create_library`/`list_library` defaults are the anti-pattern —
  do not copy them for `SkillPersistence`.
- **Newtypes over bare `String`.** `ToolId` and `SkillSlug` travel beside each other and beside
  `ChannelSlug`/`CompanySlug`; a tool id is also a map key. Extend `value_objects.rs` via
  `string_newtype!` rather than hand-rolling.
- **Split along phases; keep `async fn` chains shallow.** See the stack-budget section. Also:
  `agent_runner.rs` is 2,933 lines against a ~1,000-line threshold and its test module is well past
  ~500 — phase 3 splits it into a directory module with sibling test files.
- **Name your tuples; no flag parameters.** `AgentRun<'_>` is a struct, not a nine-argument call.
  `AgentWrite` (`use_cases/agent.rs:37`) is the model for the write DTO gaining four new fields.
- **Don't collapse errors into defaults on authorization paths.** The sub-agent scope in phase 5 is
  an authorization decision; `.ok().flatten()` and `.unwrap_or(false)` are banned on it.
- **Bound work at every external boundary.** `MAX_SKILL_INSTRUCTIONS`, `MAX_AGENT_SKILLS`,
  `MAX_GRANTED_TOOLS`, and the existing `MIN/MAX_AGENT_RUN_TIMEOUT_SECS`. Advertising a limit
  without rejecting input that exceeds it is not enforcement — each gets a SQL `CHECK` *and* a
  `normalize()` rejection.
- **Scope the object being used, not a sibling.** Every caller-supplied `skill_id` and `agent_id`
  loads through an ownership predicate in the same statement. `src/adapters/http/AGENTS.md`
  requires a test that attempts another tenant's id for every new route.
- **Make operations traceable without leaking data.** A tool grant dropped by the allowlist is
  `warn!(tool_id = %id, agent_id = %agent_id, "…")` — structured fields, never an interpolated
  sentence, and never the compiled YAML (it carries the API key until `sanitize_text` has run).
- **Escape for the output context** — `escape_html_text` for text nodes, an attribute encoder for
  attributes and `hx-confirm`. Phase 6 renders user-authored skill instructions, so this is not
  theoretical.

### DB-backed tests share one database

From `src/adapters/persistence/AGENTS.md`, and each has cost real debugging time:

- Suffix every database-wide unique value — company slugs, agent slugs, **skill library slugs** —
  with `Uuid::new_v4().simple()`. `skills_library_slug_key` is a global unique index; two parallel
  tests both inserting `test-skill` will collide.
- Never assert on a global query's totals. Assert *your* row by id.
- Establish a green baseline with `git stash` over three or four runs before calling a failure
  pre-existing.

```sh
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo test --lib   # lands on _test
```

---

## Verification, once every phase has landed

```sh
cargo fmt --check
cargo clippy --all-targets

dropdb --if-exists mail_agents      && createdb mail_agents
dropdb --if-exists mail_agents_test && createdb mail_agents_test
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" sqlx migrate run
psql "postgres://mac03@localhost:5432/mail_agents" -c '\d skills' -c '\d agent_skills' -c '\d agents'

DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo sqlx prepare -- --all-targets
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo test
SQLX_OFFLINE=true cargo build          # the offline path CI and Fly.io use
./scripts/stack-budget.sh              # the chain gained a dyn dispatch level
./scripts/transport-boundary-check.sh
```

End to end, server on `:3001`:

1. Reset both databases as above.
2. Company settings → **Skills** → create a skill with two instructions: a `Tool` step on `datetime`,
   then a `Prompt` step. Confirm saving one that *ends* on the tool step is refused with a worded
   message, not a 500.
3. Agent settings → attach the skill, tick `calculator` in the tool grid, save.
4. Mail the agent's channel. In the task trace confirm the run compiled both `skills:` and `tools:`,
   that the `datetime` step executed with `ToolCallSource::Skill`, and that the reply landed.
5. **Prove the allowlist.** Put `{"tools": ["command"]}` in the agent's raw config textarea — expect
   a `BadRequest` naming `tools` as reserved. Then insert the grant directly, bypassing the use case:
   ```sh
   psql "postgres://mac03@localhost:5432/mail_agents" \
     -c "UPDATE agents SET granted_tool_ids = ARRAY['command','calculator'] WHERE slug = '…';"
   ```
   Re-run: the run must succeed, log `warn!` that the grant was dropped, and the compiled YAML must
   contain `calculator` and **not** `command`.
6. Pick a global library skill into a company; confirm a company-owned **copy** appears and editing it
   leaves the library row untouched.
7. Attempt `/ui/skill-library` as a non-operator — expect `NotFound`, not `403`.
8. Attempt another company's `skill_id` on every new route — expect `NotFound`.
9. Set a sub-agent allowlist on an agent; confirm `list_company_agents` returns only those siblings
   **and** that outreach to an excluded sibling's address is refused. Clear the allowlist; confirm
   behaviour returns to today's.
10. Delete a skill that an agent uses; confirm the agent still runs, with the skill gone.
