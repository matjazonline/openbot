# Agent-to-agent calling: discovery + a first-class delegate tool

## Context

The question was "what do we need for an agent to call other company agents?" — the answer is
**less than expected, because the transport already exists**.

`outreach_and_await_quorum` (`src/application/services/outreach_tool.rs`) already delegates to a
sibling agent when `tool_security.tools.outreach_and_await_quorum.config.allowed_target_scope` is
`same_company_channels` or `any`. `normalize_targets` (line 328) validates the internal target
(same company, not self, enabled, has ≥1 agent, no `+` pipeline or context-only suffix), and
delivery bypasses SMTP entirely through `prepare_internal_channel_delivery` /
`ingest_prepared_internal_message` (`task_worker.rs:445`, `dispatch.rs:561`). Loop safety is real:
`MAX_CHANNEL_HOPS = 5` (`email_parser.rs:10`), `trace_channels` cycle detection
(`thread/ingest.rs:399`), durable outbox, and B→A resumes A's original task rather than creating a
child. `docs/inter_channel_agent_communication.md` is the spec for all of it.

Two gaps remain, and this plan closes them:

1. **No discovery.** Nothing exposes the sibling-channel list to the model, so callable addresses
   must be hardcoded into each system prompt. They then rot silently when a channel is renamed.
2. **Internal and external sends share one tool ID.** `hitl.tools.<id>` is keyed by tool ID, so an
   agent cannot require approval for external outreach while delegating internally without it.
   `docs/inter_channel_agent_communication.md` sidesteps this by making the coordinator
   `same_company_channels`-only; a coordinator that must do both is currently unrepresentable.

### Rejected alternatives (checked against the vendored `ai-agents` 1.0.4, rev `9ea972e`)

The library ships `spawner.management_tools` (`list_agents`, `send_agent_message`) and
`spawner.orchestration_tools` (`route_to_agent`, `handoff_conversation`, …), and
`auto_configure_spawner()` is already called at `agent_runner.rs:1109`. Neither fits:

- `AgentRegistry` (`crates/ai-agents-runtime/src/spawner/registry.rs`) is a fresh in-process
  `HashMap` per `AgentBuilder`, and `send()` calls `target.chat()` inline — no task row, no thread,
  no outbox, no approval, nothing surviving a restart or visible to another worker.
- `auto_spawn` resolves child YAML paths relative to the *parent YAML file's* directory. We build
  from a string (`AgentBuilder::from_yaml`), so `yaml_dir` is `None` and file-based spawning cannot
  resolve at all.
- Orchestration patterns each complete inside one `chat()` call, which cannot express "pause for
  four days until a supplier replies." Static chains are already covered by `+`-pipeline addressing.

They stay available for ephemeral in-run sub-personas. They are not the transport for calling a
durable, addressable company agent.

## Approach

Keep the existing outreach machinery as the transport. Add a **read-only directory tool** and a
**thin `delegate_to_agent` wrapper** that reuses it, plus the `agents.description` column that makes
the directory worth reading.

### 1. `description` column on agents

- New migration `migrations/20260826000000_agent_description.sql`:
  `ALTER TABLE agents ADD COLUMN description TEXT;`
- Add `pub description: Option<String>` to `Agent` (`src/domain/entities/agent.rs`) and to
  `AgentWrite` (`src/application/use_cases/agent.rs`).
- Update the `SELECT`/`INSERT`/`UPDATE` in `src/adapters/persistence/agent.rs`.
- Add a `Description` textarea to the agent settings form (`pages/agent_settings.rs`, alongside the
  existing `config_json` field near line 418) and thread it through
  `routes/ui_agents.rs` and the JSON API in `routes/agent.rs`.
- **Then run `cargo sqlx prepare`** — see `src/AGENTS.md`; a stale `.sqlx/` cache breaks the Fly.io
  build.

### 2. Shared eligibility predicate (do this before the tools)

`normalize_targets` in `outreach_tool.rs:328` currently inlines the internal-target rules. Extract
them so the directory can never advertise a channel the send path will reject — a drift bug that
would surface only as a confusing tool error mid-conversation.

Add to `src/application/use_cases/channel.rs` (next to `parse_recipient_address_pipeline`):

```rust
pub enum InternalTargetRejection { CrossCompany, SelfCall, Disabled, NoAgent, NotDirectAddress }

pub fn check_internal_target(
    channel: &Channel,
    caller_company_id: Uuid,
    caller_channel_id: Uuid,
) -> Result<(), InternalTargetRejection>
```

Rewrite the corresponding block of `normalize_targets` to call it. Per `src/AGENTS.md` this is a
pure decision on already-loaded data, so it unit-tests with no mocks.

### 3. `list_company_agents` — new file `src/application/services/agent_directory_tool.rs`

- `pub const AGENT_DIRECTORY_TOOL_ID: &str = "list_company_agents";`
- Holds `Arc<dyn ChannelPersistence>`, `Arc<dyn AgentPersistence>`, and a context struct capturing
  `company_id`, `company_slug`, `source_channel_id`, `app_domain_name`. **Takes no arguments that
  carry identity** — `docs/custom_tools.md` line 55 is explicit about this.
- `execute`: `ChannelPersistence::list_by_company_id` (`use_cases/channel.rs:132`) +
  `AgentPersistence::list_by_company_id` (`use_cases/agent.rs:46`), filter through
  `check_internal_target`, return
  `[{ address, channel_name, agent_name, description }]` using `Channel::address_for` for the
  address. Cap results via `config_usize(&ctx.custom_config, "max_results", 50)` — the same helper
  pattern `outreach_tool.rs:404` already uses.
- `safety_metadata`: `read_only: true`, `default_requires_approval: false`,
  `operation: ToolOperationKind::Read`, `side_effect_level: ToolSideEffectLevel::None`.

### 4. `delegate_to_agent` — in `outreach_tool.rs` (it shares the internals)

- `pub const DELEGATE_TOOL_ID: &str = "delegate_to_agent";`
- Input: `{ agent_address: String, request: String, subject: String, timeout_hours: Option<u32> }`.
  No threshold, no target array — the model gets one obvious shape.
- Internally builds exactly the call the docs describe today: one target, 100% threshold, default
  `timeout_hours` from `ctx.custom_config` (`default_timeout_hours`, fall back to 96 per the
  timeout guidance in `docs/inter_channel_agent_communication.md`).
- Reuses `ValidatedOutreach`, `idempotency_key`, `build_target_requests`, and
  `create_outreach_and_pause` unchanged, and sets `suspended` the same way.
- Target scope is **forced** to same-company internal — this tool never reaches an external
  address, which is what makes a separate HITL entry meaningful.
- Factor the shared body out of `OutreachAndAwaitQuorumTool::execute` rather than copying it;
  `execute` is already near the ~80-line limit in `src/AGENTS.md`.

### 5. Registration and defaults

- `agent_runner.rs:1111` — register both new tools next to the existing
  `OutreachAndAwaitQuorumTool`, in the same `if let Some(...) = self.outreach.clone()` block.
  Registration does not expose them; the agent's YAML must still grant them under `tools:`.
- `AgentRunner` needs an `Arc<dyn AgentPersistence>` for the directory tool — add it to the
  `outreach_tool(...)` builder call in `dispatch.rs` (`run_agents`, line ~139), where
  `outreach_context_for` already assembles the trusted context.
- `base_agent_config()` (`agent_runner.rs:145`) — add default `hitl` and `tool_security` entries:
  `delegate_to_agent` with `require_approval: false` and
  `config: { default_timeout_hours: 96, max_timeout_hours: 168 }`; `list_company_agents` with
  `config: { max_results: 50 }` and no HITL. Channel and agent config still merge over these.

### 6. Docs

- `docs/custom_tools.md` — it currently says "`mail-agents-server` provides one application-owned
  custom tool" and "There is no separate `delegate_to_third_party` tool." Both statements change.
- `docs/inter_channel_agent_communication.md` — replace the hand-built quorum call in the
  "Tool Call" section with the `delegate_to_agent` form, and add the directory tool to
  "Agent Configuration".

## Verification

1. `cargo test` — new pure unit tests for `check_internal_target` (cross-company, self-call,
   disabled, agent-less) and for `delegate_to_agent` argument validation. Existing
   `outreach_tool.rs` tests (`MockChannelPersistence`, line ~429) must still pass unchanged; that
   is the regression signal for the extraction in step 2.
2. `DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo sqlx prepare -- --all-targets`
   then `cargo build` with `.sqlx/` offline, to prove the Fly.io build still works.
3. End-to-end on the local server (port 3001), via the simulation page
   (`pages/simulation.rs` — `resolved_agent_config` shows the effective merged config, so use it to
   confirm the new `hitl`/`tool_security` defaults land):
   - Create two channels in one company, each with an agent; give agent B a `description`.
   - Grant A `list_company_agents` + `delegate_to_agent` in its config JSON.
   - Send A a message that requires B's knowledge. Expect: A calls the directory, sees B's address
     and description, calls `delegate_to_agent`, task A → `waiting_for_third_party_reply`, a new
     thread + task appears for B, B replies, A's *original* task resumes (no second A task) and
     answers the human.
   - Check the task monitor page and `task_outreaches` / `task_outreach_targets` /`email_outbox`
     rows for the expected single outreach with one target.
4. Negative checks: A's own address is absent from the directory output; a disabled channel and an
   agent-less channel are both absent and also rejected by `delegate_to_agent`; a cross-company
   address is rejected.
