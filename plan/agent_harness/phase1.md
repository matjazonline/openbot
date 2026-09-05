# Phase 1 — Domain: capability spec, skills, tool catalogue

Read [`general_plan_instructions.md`](general_plan_instructions.md) first.

**Goal.** Every type this feature needs, with no dependency on `ai_agents`, `sqlx` or `axum`, and no
I/O. Everything here is unit-testable with no mocks at all — which is the point: the safety
properties of this change (what a skill may contain, what a tool grant may name) become pure
functions with pure tests.

Nothing in this phase is wired to anything. It compiles and its tests pass; the rest is dead code
until phase 3.

---

## 1. Newtypes — `src/domain/entities/value_objects.rs`

Two additions through the existing `string_newtype!` macro (`:12`), which already generates
`new`/`as_str`/`into_string`, `Deref<str>`, `Borrow<str>`, `AsRef<str>`, `Display`,
`From<String>`/`From<&str>`, the `PartialEq` family and `#[serde(transparent)]`.

```rust
string_newtype!(
    /// The id of one grantable tool: an `ai-agents` built-in (`"datetime"`) or one of our own
    /// native tools (`"outreach_and_await_quorum"`). Used as a map key and carried beside
    /// `SkillSlug` in every skill instruction, which is the argument-swap case `src/AGENTS.md`
    /// names.
    ToolId
);

string_newtype!(
    /// A skill's stable identifier within its company, and the `id:` the `ai-agents` YAML
    /// `skills:` entry carries.
    SkillSlug
);
```

`SkillSlug` gets a manual `parse` alongside, matching how `ResendWebhookToken::parse` (:319) and
`AuthservId::parse` (:350) are written — same charset as `agents_slug_format`:

```rust
impl SkillSlug {
    pub const MAX_CHARS: usize = 64;
    /// Lowercase, digits and single hyphens, not leading or trailing. Mirrors the
    /// `skills_slug_format` CHECK so a value that parses always stores.
    pub fn parse(value: &str) -> Result<Self, String> { … }
}
```

`ToolId` needs no `parse` — a tool id is only ever valid by being *in the catalogue*, and
`ToolCatalogue::get` is that check.

## 2. Tool catalogue — `src/domain/entities/tool_catalogue.rs` (new)

The "tools library" is a compile-time const, not a table: the `ai-agents` built-ins are a const
upstream (`BUILTIN_TOOL_IDS`, `[&str; 30]`) and our three native ids are already consts in
`services/*_tool.rs`.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSource {
    /// Ships with the `ai-agents` runtime; registered by `auto_configure_features()`.
    Builtin,
    /// Ours, implemented in `src/application/services/*_tool.rs`.
    Native,
}

pub struct CatalogueTool {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub source: ToolSource,
}
```

### The allowlist

```rust
/// Every `ai-agents` built-in this platform will ever put in a `tools:` list.
///
/// This is an allowlist and not a blocklist on purpose: the upstream `BUILTIN_TOOL_IDS` is a
/// hardcoded 30-entry array that grows when the pinned revision moves, and a blocklist would grant
/// each new arrival by default. There is no environment override and no per-company escape — the
/// same list applies to every agent on the platform.
///
/// The absentees are absent because they execute in this process, on this host, with no sandbox,
/// and inbound mail is an untrusted prompt source: `command` (arbitrary execution), the mutation
/// family `file_write` / `file_edit` / `patch` / `copy_path` / `move_path` / `delete_path`, the
/// read family `file` / `file_read` / `file_list` / `file_info` / `glob` / `grep`, the repository
/// pair `git_status` / `git_diff`, `diagnostics`, `sleep` (an unbounded wall-clock hold inside a
/// leased task), and `ask_user` (blocks for a terminal operator who does not exist in a mail
/// server). Revisit this list when — and only when — a sandboxed `HarnessKind` exists to run them
/// in.
pub const ALLOWED_BUILTIN_TOOL_IDS: [&str; 12] = [
    "calculator", "datetime", "echo", "json", "math", "random",
    "template", "text", "todo", "http", "web_fetch", "web_search",
];
```

`http`, `web_fetch` and `web_search` make outbound requests from the application host and are worth a
second look before phase 6 exposes them — they are server-side request forgery primitives if the
runtime does not restrict the target. They are on the list because they are the reason an agent
would want tools at all; if the runtime turns out not to bound them, drop them here rather than
adding a second checkpoint elsewhere.

### The catalogue and its accessors

```rust
pub const TOOL_CATALOGUE: &[CatalogueTool] = &[ /* 12 builtins + 3 natives, with copy */ ];

impl CatalogueTool {
    pub fn get(id: &ToolId) -> Option<&'static CatalogueTool>;
    /// Grantable ids in stable UI order, grouped by source.
    pub fn grantable() -> impl Iterator<Item = &'static CatalogueTool>;
}

/// Drop every id not in the catalogue, returning what survived and what was refused.
/// Pure and synchronous: this is the function phase 3's compiler calls, and it must contribute no
/// stack frame to the agent chain.
pub fn retain_grantable(ids: &[ToolId]) -> GrantFilter;

pub struct GrantFilter { pub granted: Vec<ToolId>, pub refused: Vec<ToolId> }
```

`refused` is returned rather than silently dropped so the caller can `warn!` each one with the agent
id attached — "make operations traceable", and a capability that vanishes without a log is a support
ticket nobody can answer.

## 3. Skills — `src/domain/entities/skill.rs` (new)

```rust
pub const MAX_SKILL_INSTRUCTIONS: usize = 32;
pub const MAX_SKILL_INSTRUCTION_CHARS: usize = 8_000;
pub const MAX_SKILL_TRIGGER_CHARS: usize = 500;
pub const MAX_SKILL_DESCRIPTION_CHARS: usize = 500;

pub struct Skill {
    pub id: Uuid,
    /// `None` identifies an operator-managed definition in the global skill library.
    pub company_id: Option<Uuid>,
    pub slug: SkillSlug,
    pub name: String,
    /// Required by `ai-agents`; also the picker subtitle.
    pub description: String,
    /// Required by `ai-agents`: when the router should reach for this skill.
    pub trigger: String,
    pub instructions: Vec<SkillInstruction>,
    pub created_by: CreationProvenance,
    pub created_at: DateTime<Utc>,
}

/// One step of a skill, in a form no harness owns.
///
/// Stored as a JSONB array rather than a `steps` table: a skill is a handful of ordered items
/// edited as one document, and a second table would buy referential integrity nobody needs at the
/// cost of a join on every agent run. Externally tagged so a third variant is additive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillInstruction {
    Prompt { text: String },
    Tool {
        tool: ToolId,
        #[serde(default, skip_serializing_if = "Option::is_none")] args: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")] output_as: Option<String>,
    },
}
```

### `Skill::validate` — the phase's centre of gravity

A pure method, no `async`, no `self` beyond the entity, no persistence. Rules, each with its own
test:

| Rule | Why |
|---|---|
| 1..=`MAX_SKILL_INSTRUCTIONS` instructions | "Bound work at every external boundary" |
| Each `Prompt.text` non-blank and ≤ `MAX_SKILL_INSTRUCTION_CHARS` | ditto |
| **The last instruction must be `Prompt`** | `SkillExecutor` returns only on a final prompt step and otherwise errors `"Skill has no prompt step to generate response"` — see `general_plan_instructions.md`. Rejecting at write time turns a mid-run failure into a form message |
| Every `Tool.tool` is in the catalogue and grantable | A step naming an ungranted tool is denied at `runtime.rs:4784`; catching it here means the picker cannot build a skill that cannot run |
| `args`, when present, is a JSON **object** | `render_args` defaults to `{}`; an array or scalar is a template that cannot bind |
| `output_as`, when present, is a non-blank identifier | It names a template variable |
| `description` and `trigger` non-blank, within their char bounds | `ai-agents` requires both, and they are `NOT NULL` in phase 4 |

Plus the accessor phase 3 needs:

```rust
impl Skill {
    /// Tool ids this skill's steps invoke, deduplicated and in first-use order.
    ///
    /// The compiler unions these into the agent's `tools:` grant. Without that, the runtime denies
    /// an ordinary catalogue step: skill calls are checked against the same effective scope as
    /// model-initiated calls. Other upstream feature grants exist, but phase 4 rejects those
    /// configuration paths rather than treating them as skill capability sources.
    pub fn referenced_tool_ids(&self) -> Vec<ToolId>;
}
```

`SkillWrite` mirrors `AgentWrite` (`use_cases/agent.rs:37`) — a single struct so create and update
cannot drift, with a `normalize()` that trims, lowercases the slug and calls `validate()`. It lives
in `use_cases/skill.rs` in phase 4, not here; only the entity and its rules are in scope now.

## 4. Harness kind and capability spec — `src/domain/entities/harness.rs` (new)

Model `HarnessKind` on `MemoryProviderKind` (`entities/memory.rs:63`) exactly — `ALL`, `as_str`,
`label`, `parse`, `Display`, and the doc comment tying `as_str` to the SQL `CHECK`. Iterating `ALL`
is what keeps the wire strings, the `<select>` options and the stored-value parsing from drifting
apart as harnesses are added.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HarnessKind {
    /// The `ai-agents` runtime, in this process. The only harness today.
    #[default]
    AiAgents,
}

impl HarnessKind {
    pub const ALL: [Self; 1] = [Self::AiAgents];
    /// The wire and database value. Must stay in sync with the `agents_harness_kind_check`
    /// CHECK in `migrations/20260817000000_init_schema.sql`.
    pub fn as_str(self) -> &'static str { match self { Self::AiAgents => "ai_agents" } }
    pub fn label(self) -> &'static str  { match self { Self::AiAgents => "ai-agents (in process)" } }
    pub fn parse(value: &str) -> Option<Self> { Self::ALL.into_iter().find(|k| k.as_str() == value) }
}
```

One variant looks silly and is not: it is the enum a second harness becomes a variant of, and the
`match` that then fails to compile everywhere a decision must be made.

```rust
/// What an agent is and may do, in terms no harness owns.
///
/// Boxed wherever it crosses an async boundary — it carries every skill body, so it dominates any
/// enum it lands in (`clippy::large_enum_variant`).
pub struct AgentCapabilitySpec {
    pub harness: HarnessKind,
    pub name: String,
    pub system_prompt: String,
    pub provider: ModelProvider,
    pub model: ModelName,
    pub skills: Vec<Skill>,
    pub granted_tools: Vec<ToolId>,
    pub sub_agents: SubAgentScope,
    /// The residual `agents.config_json` — everything the typed fields above do not own. Merged
    /// over the harness's own defaults by the adapter, exactly as `merge_json` does today.
    pub extra_config: serde_json::Value,
}

/// Which sibling agents this one may delegate to.
///
/// `AllCompanySiblings` is today's behaviour and stays the default; an allowlist restricts only
/// when non-empty, so no existing agent changes.
pub enum SubAgentScope {
    AllCompanySiblings,
    Restricted(Vec<Uuid>),
}

impl SubAgentScope {
    /// Whether this agent may reach `candidate`. An authorization decision, so it lives on the
    /// type and has one implementation — `src/AGENTS.md`, "One decision, one place".
    pub fn allows(&self, candidate: Uuid) -> bool;
}
```

`SubAgentScope::allows` is the function both `agent_directory_tool` and `outreach_tool` call in
phase 5. Writing it here, on the type, is what stops the listing filter and the send-time check
drifting apart — the failure mode being an agent that cannot see a sibling in the directory but can
still mail it by naming the address.

## 5. Registration

`src/domain/entities/mod.rs` gains `pub mod harness; pub mod skill; pub mod tool_catalogue;`.

---

## Tests — inline `#[cfg(test)] mod tests`, no mocks

Sentence-shaped names, matching `entities/company_resend.rs`:

- `a_skill_must_end_on_a_prompt_step`
- `a_skill_step_naming_an_ungrantable_tool_is_refused` — assert on `command` specifically
- `an_empty_instruction_list_is_refused` / `more_than_thirty_two_instructions_are_refused`
- `tool_step_args_must_be_a_json_object`
- `referenced_tool_ids_deduplicates_and_preserves_first_use_order`
- `retain_grantable_refuses_every_host_access_builtin` — loop over the 18 absentees by name
- `every_allowlisted_id_exists_upstream` — **the important one**. Both `BUILTIN_TOOL_IDS` and
  `get_builtin_tool` are private to `ai-agents-tools` and are *not* re-exported through the
  `ai_agents` facade; `create_builtin_registry` is (`ai-agents/src/lib.rs:456`), and it is the
  registry `auto_configure_features()` itself installs. Probe that:
  ```rust
  #[test]
  fn every_allowlisted_id_exists_upstream() {
      // The registry `auto_configure_features()` installs. If the pinned revision renames or drops
      // a built-in, this fails here rather than silently dropping the grant at run time — the
      // runtime would otherwise deny the call at `runtime.rs:4784` with no build-time signal.
      let registry = ai_agents::tools::create_builtin_registry();
      let ids = registry.list_ids();
      for id in ALLOWED_BUILTIN_TOOL_IDS {
          assert!(ids.iter().any(|known| known == id), "{id} is no longer an ai-agents builtin");
      }
  }
  ```
  This test touches `ai_agents`, so it does **not** belong in `src/domain/` — put it in the phase-3
  adapter (`adapters/harness/ai_agents/compile.rs`), where the dependency is legitimate, and leave a
  `// drift test lives in …` comment beside `ALLOWED_BUILTIN_TOOL_IDS`. Confirm `list_ids` is on the
  re-exported `ToolRegistry` when writing it; `canonical_id` is the fallback if it is not.
- `a_sub_agent_scope_with_no_entries_allows_every_sibling`
- `harness_kind_round_trips_through_as_str_and_parse`

## Done when

`cargo test --lib entities::` is green, `cargo clippy --all-targets` is clean, and
`grep -rn "ai_agents" src/domain/` returns **nothing at all** — the one test that needs the crate
lives in the phase-3 adapter.
