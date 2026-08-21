# Agent-to-agent calling: one generalized send tool + a discovery tool

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

Two gaps remain:

1. **No discovery.** Nothing exposes the sibling-channel list to the model, so callable addresses
   must be hardcoded into each system prompt, where they rot silently when a channel is renamed.
2. **Internal delegation and external outreach can't be governed differently.** Today the only
   discriminator is the tool ID, so an agent that must do both has to accept one approval policy
   for both. `docs/inter_channel_agent_communication.md` sidesteps this by making the coordinator
   `same_company_channels`-only.

Gap 2 does **not** need a second tool. `AgentApprovalHandler::request_approval`
(`agent_runner.rs:568`) already receives `ApprovalTrigger::Tool { name, args }` with the full
argument payload and is `async`, so approval can be decided per call from the resolved targets.
One tool, one abstraction, policy where policy belongs.

### Rejected alternatives

**A separate `delegate_to_agent` tool.** Rejected: it splits one concept across two IDs and two
config blocks, and the only thing it bought was the approval split, which the handler does better —
from authoritative resolved targets rather than from which tool the model happened to pick.

**Pure-YAML `hitl.conditions`.** Rejected: `when` supports numeric comparisons and `in [...]` /
`not in [...]` on a scalar field, which cannot express "every element of `target_emails` resolves to
a same-company channel." It also fires "only when the named field exists in the tool's arguments" —
an omitted field means no approval, which is fail-open for external mail.

**A model-declared `audience: internal | external` argument.** Rejected: makes a
security-relevant value model-supplied. Safe only because `execute` would re-validate and reject a
mismatch, but there is no reason to accept the claim when we can compute it.

**The library's `spawner` / `orchestration` tools** (`spawner.management_tools`,
`orchestration_tools`; `auto_configure_spawner()` is already called at `agent_runner.rs:1109`).
Rejected: `AgentRegistry` (`crates/ai-agents-runtime/src/spawner/registry.rs`, `ai-agents` 1.0.4 rev
`9ea972e`) is a fresh in-process `HashMap` per builder whose `send()` calls `target.chat()` inline —
no task row, no thread, no outbox, no approval, nothing surviving a restart or visible to another
worker. `auto_spawn` also resolves child YAML paths relative to the parent YAML *file*, and we build
from a string (`AgentBuilder::from_yaml`), so `yaml_dir` is `None`. Orchestration patterns each
complete inside one `chat()` call, which cannot express "pause four days for a supplier reply."
They remain available for ephemeral in-run sub-personas.

**Renaming the tool.** `outreach_and_await_quorum` reads external-flavoured for what is now
explicitly a dual-purpose tool, but `docs/custom_tools.md:161` makes the ID canonical, and a rename
means a JSONB migration over `agents.config_json` and `channels.channel_config` to rewrite `tools:`
grants plus `hitl` / `tool_security` keys. Deferred; the tool description carries the meaning.

## Approach

### 1. Shared eligibility predicate (do this first)

`normalize_targets` (`outreach_tool.rs:328`) inlines the internal-target rules. Both the directory
tool and the approval policy need the same verdict, and drift between them would surface as either
a confusing mid-conversation error or a silently skipped approval.

Add to `src/application/use_cases/channel.rs`, beside `parse_recipient_address_pipeline`:

```rust
pub enum InternalTargetRejection { CrossCompany, SelfCall, Disabled, NoAgent, NotDirectAddress }

pub fn check_internal_target(
    channel: &Channel,
    caller_company_id: Uuid,
    caller_channel_id: Uuid,
) -> Result<(), InternalTargetRejection>
```

Rewrite that block of `normalize_targets` to call it. Per `src/AGENTS.md` this is a pure decision on
already-loaded data — unit-tests with no mocks.

### 2. Generalize the tool's arguments

In `OutreachInput`, make the quorum knobs optional so a single delegation is as easy to call as a
broadcast:

- `completion_threshold_percent: Option<f64>` — default `100.0`
- `timeout_hours: Option<u32>` — default from `config_u32(&ctx.custom_config,
  "default_timeout_hours", 96)`, matching the timeout guidance in
  `docs/inter_channel_agent_communication.md`

A delegation then reads `{ target_emails: ["b@acme.example"], subject, body }`.

**Resolve the defaults inside `ValidatedOutreach::from_input`, before
`idempotency_key` is computed** — the key hashes threshold and timeout
(`outreach_tool.rs:257`), so defaulting after hashing would let a retry mail everyone twice.

Rewrite `description()` to cover both uses explicitly; it is the only thing telling the model that
this one tool delegates to colleagues *and* contacts third parties. Existing agents are unaffected —
they pass both fields, and `#[serde(default)]` + `Option<T>` only relaxes the schema.

### 3. Per-call approval policy in `AgentApprovalHandler`

Add `channel_persistence: Arc<dyn ChannelPersistence>` and a resolved policy to
`AgentApprovalHandler` (`agent_runner.rs:560`). At the top of `request_approval`, **before** the
existing empty-approver guard at line 572:

- If the trigger is `Tool { name, args }` with `name == OUTREACH_TOOL_ID`, resolve every entry in
  `args["target_emails"]` through `check_internal_target`.
- If **all** targets are internal same-company channels and the policy permits it, return
  `ApprovalResult::Approved` without creating a row or emailing anyone.
- Anything else — any external target, any unresolvable address, any lookup error — falls through
  to today's path. Per `src/AGENTS.md`, a persistence error here propagates rather than collapsing
  into a default; this is an authorization decision.

Placing it before the approver guard also fixes a live bug: a coordinator channel with no configured
participant currently cannot delegate at all, because it fails the "no approver configured" check
before anything else is considered.

Policy knob: `internal_requires_approval: bool` under the existing
`tool_security.tools.outreach_and_await_quorum.config` block, **defaulting to `true`** so behaviour
is unchanged until an operator opts in. `AgentRunner` already has the merged JSON config in hand
where it calls `provider_config_from_agent_config` (`agent_runner.rs:~810`); read the flag there and
pass it into the handler at construction (`build_agent`, line 1080).

> Wiring caveat: I have not traced how `AgentRunner` hands state to `AgentTask` closely enough to
> promise this is a two-line change. The values all exist in the right scope; the plumbing may want
> a small named struct rather than another positional field.

### 4. `list_company_agents` — new `src/application/services/agent_directory_tool.rs`

Discovery is a genuinely different operation from sending, so it stays its own read-only tool.

- `pub const AGENT_DIRECTORY_TOOL_ID: &str = "list_company_agents";`
- Holds `Arc<dyn ChannelPersistence>`, `Arc<dyn AgentPersistence>`, and a context capturing
  `company_id`, `company_slug`, `source_channel_id`, `app_domain_name`. **No argument carries
  identity** — `docs/custom_tools.md:55` is explicit about this.
- `execute`: `ChannelPersistence::list_by_company_id` (`use_cases/channel.rs:132`) +
  `AgentPersistence::list_by_company_id` (`use_cases/agent.rs:46`), filtered through
  `check_internal_target`, returning `[{ address, channel_name, agent_name, description }]` with
  `Channel::address_for` for the address. Cap with
  `config_usize(&ctx.custom_config, "max_results", 50)`, reusing the helper at
  `outreach_tool.rs:404`.
- `safety_metadata`: `read_only: true`, `operation: ToolOperationKind::Read`,
  `side_effect_level: ToolSideEffectLevel::None`, `default_requires_approval: false`.

### 5. `description` column on agents

Gives the directory something worth reading.

- Migration `migrations/20260826000000_agent_description.sql`:
  `ALTER TABLE agents ADD COLUMN description TEXT;`
- `pub description: Option<String>` on `Agent` (`src/domain/entities/agent.rs`) and `AgentWrite`
  (`src/application/use_cases/agent.rs`); update the `SELECT`/`INSERT`/`UPDATE` in
  `src/adapters/persistence/agent.rs`.
- Add the field to the agent settings form (`pages/agent_settings.rs`, near the `config_json`
  textarea at line 418) and thread it through `routes/ui_agents.rs` and `routes/agent.rs`.
- **Then `cargo sqlx prepare`** — `src/AGENTS.md`; a stale `.sqlx/` cache breaks the Fly.io build.

### 6. Registration and defaults

- `agent_runner.rs:1111` — register `list_company_agents` next to the existing outreach tool, in the
  same `if let Some(...) = self.outreach.clone()` block. Registration does not expose it; the
  agent's YAML must still grant it under `tools:`.
- `AgentRunner` needs `Arc<dyn AgentPersistence>` for the directory tool — add it to the
  `outreach_tool(...)` builder call in `dispatch.rs` `run_agents` (~line 139), where
  `outreach_context_for` already assembles the trusted context.
- `base_agent_config()` (`agent_runner.rs:145`) — add `list_company_agents` under `tool_security`
  with `config: { max_results: 50 }` and no HITL entry; extend the existing outreach `config` block
  with `default_timeout_hours: 96` and `internal_requires_approval: true`. Channel and agent config
  still merge over these.

### 7. Docs

- `docs/custom_tools.md` — the optional-argument shape, the two default keys, and the fact that one
  tool now covers delegation and outreach. The "Single-Party Delegation" section becomes the
  short-form call.
- `docs/inter_channel_agent_communication.md` — update the "Tool Call" example to the short form,
  add `list_company_agents` to "Agent Configuration", and document
  `internal_requires_approval` in the scope table.

## Verification

1. `cargo test` — pure unit tests for `check_internal_target` (cross-company, self-call, disabled,
   agent-less) and for default resolution in `ValidatedOutreach::from_input`, including that
   `idempotency_key` is identical whether the caller passes `100`/`96` explicitly or omits them.
   Existing `outreach_tool.rs` tests (`MockChannelPersistence`, ~line 429) must pass unchanged —
   that is the regression signal for step 1.
2. `DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo sqlx prepare -- --all-targets`,
   then `cargo build` against the offline cache to prove the Fly.io build still works.
3. End-to-end on the local server (port 3001). Use the simulation page (`pages/simulation.rs` —
   `resolved_agent_config` renders the effective merged config) to confirm the new defaults land.
   - Two channels in one company, each with an agent; give agent B a `description`.
   - Grant A `list_company_agents` + `outreach_and_await_quorum` with
     `allowed_target_scope: any` and `internal_requires_approval: false`.
   - Send A a message needing B's knowledge. Expect: A calls the directory, sees B's address and
     description, calls the send tool with the short form, **no approval email is sent**, task A →
     `waiting_for_third_party_reply`, a new thread + task appears for B, B replies, A's *original*
     task resumes (no second A task) and answers the human.
   - Inspect `task_outreaches` / `task_outreach_targets` / `email_outbox` for one outreach, one
     target, and no `human_approvals` row.
4. The discriminator test that justifies this design: same agent, one call to an **external**
   address — an approval email must still be sent and the task must park in `pending_approval`.
   Then a mixed call (one internal + one external target) must **also** require approval.
5. Negative checks: A's own address absent from the directory output; disabled and agent-less
   channels absent from it and rejected by the send tool; cross-company address rejected.
