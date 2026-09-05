# Phase 4 — Schema, persistence, use cases

Read [`general_plan_instructions.md`](general_plan_instructions.md) first. Depends on phases 1–3,
which must be green — this is the first phase that changes behaviour.

**Goal.** Store skills, store an agent's tool grant and skill selection, and close the
`config_json` escape hatch that would otherwise walk straight past phase 1's allowlist.

Follow the vertical-slice shape `company_resend` established on this branch:
`migration → domain entity → use case (ports + write DTO + parse) → persistence adapter →
mod registrations → AppState field → infra/setup.rs construction → cargo sqlx prepare`.

---

## 1. Migration

Edit `migrations/20260817000000_init_schema.sql` in place and recreate both databases — see
`general_plan_instructions.md`. Put `skills` immediately after `agents` so the file reads in
dependency order.

```sql
-- One reusable procedure an agent can be taught: a trigger that says when to reach for it, and an
-- ordered list of instructions that say what to do. A NULL `company_id` is an operator-managed
-- global library skill, visible to every company and owned by none -- the same arrangement
-- `agents` uses.
--
-- `instructions` is a JSONB array of typed values rather than a `skill_steps` table on purpose: a
-- skill is a handful of ordered items edited as one document, and a second table would buy
-- referential integrity nobody needs at the cost of a join on every agent run. It is deliberately
-- not named `steps`: `steps` is what the `ai-agents` harness calls its compiled form, and another
-- harness will render the same instructions as one markdown document instead.
CREATE TABLE skills (
    id UUID PRIMARY KEY,
    company_id UUID REFERENCES companies(id) ON DELETE CASCADE,
    slug CITEXT NOT NULL,
    name TEXT NOT NULL,
    -- Both required by `ai-agents`' SkillDefinition, which is deny_unknown_fields and has no
    -- default for either.
    description TEXT NOT NULL,
    trigger TEXT NOT NULL,
    instructions JSONB NOT NULL,
    created_by JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT skills_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT skills_company_slug_key UNIQUE (company_id, slug),
    CONSTRAINT skills_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT skills_description_not_blank CHECK (btrim(description) <> ''),
    CONSTRAINT skills_trigger_not_blank CHECK (btrim(trigger) <> ''),
    CONSTRAINT skills_slug_format CHECK (
        slug::text = lower(slug::text)
        AND slug::text ~ '^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$'
    ),
    CONSTRAINT skills_instructions_shape CHECK (
        jsonb_typeof(instructions) = 'array'
        AND jsonb_array_length(instructions) BETWEEN 1 AND 32
    ),
    CONSTRAINT skills_created_by_shape_check CHECK (valid_creation_provenance(created_by))
);

-- `skills_company_slug_key` does not constrain library skills: UNIQUE treats every NULL
-- `company_id` as distinct, so the library needs its own uniqueness over slug alone. Same reasoning
-- as `agents_library_slug_key`.
CREATE UNIQUE INDEX skills_library_slug_key ON skills (slug) WHERE company_id IS NULL;

CREATE INDEX skills_company_created_idx ON skills (company_id, created_at DESC, id DESC);
```

```sql
-- Which skills one agent carries, in the order they are offered to the router.
--
-- `company_id` is nullable for the same reason `agents.company_id` is: a global library agent can
-- be assigned straight to a channel -- that is what `prevent_assigned_library_agent_delete` guards
-- -- so it must be able to carry library skills. A composite FK on `(company_id, agent_id)` would
-- exclude exactly those rows and lose the capability silently.
CREATE TABLE agent_skills (
    company_id UUID REFERENCES companies(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    skill_id UUID NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (agent_id, skill_id),
    CONSTRAINT agent_skills_agent_position_key UNIQUE (agent_id, position),
    CONSTRAINT agent_skills_position_check CHECK (position >= 0)
);

CREATE INDEX agent_skills_skill_idx ON agent_skills (skill_id, agent_id);

-- Copies `enforce_channel_agent_scope`: both sides must belong to this row's company or to the
-- global library, which is the tenancy check the single-column FKs above can no longer make.
CREATE FUNCTION enforce_agent_skill_scope() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM agents AS agent
        WHERE agent.id = NEW.agent_id
          AND agent.company_id IS NOT DISTINCT FROM NEW.company_id
    ) THEN
        RAISE EXCEPTION 'agent must belong to the row company or the global library'
            USING ERRCODE = '23514';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM skills AS skill
        WHERE skill.id = NEW.skill_id
          AND (skill.company_id IS NULL OR skill.company_id = NEW.company_id)
    ) THEN
        RAISE EXCEPTION 'skill must belong to the row company or the global library'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_skills_scope_check
BEFORE INSERT OR UPDATE ON agent_skills
FOR EACH ROW EXECUTE FUNCTION enforce_agent_skill_scope();

-- Nothing cascades a library skill away, so an in-use one needs the same guard
-- `prevent_assigned_library_agent_delete` gives library agents.
CREATE FUNCTION prevent_assigned_library_skill_delete() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.company_id IS NULL
       AND EXISTS (SELECT 1 FROM agent_skills WHERE skill_id = OLD.id) THEN
        RAISE EXCEPTION 'library skill is assigned to one or more agents'
            USING ERRCODE = '23503';
    END IF;
    RETURN OLD;
END;
$$;

CREATE TRIGGER library_skill_delete_guard
BEFORE DELETE ON skills
FOR EACH ROW EXECUTE FUNCTION prevent_assigned_library_skill_delete();
```

Copy-on-pick means a company agent only ever attaches skills its own company owns, so in normal use
`agent_skills.company_id` is set and the trigger is a formality. It earns its place on the one path
that is not copy-on-pick — a global library agent assigned directly to a channel, referencing a
library skill live.

```sql
-- Which siblings this agent may delegate to. Restricts only when non-empty: an agent with no rows
-- reaches every company sibling, which is today's behaviour and stays the default.
CREATE TABLE agent_sub_agents (
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL,
    sub_agent_id UUID NOT NULL,
    PRIMARY KEY (agent_id, sub_agent_id),
    CONSTRAINT agent_sub_agents_not_self CHECK (agent_id <> sub_agent_id),
    CONSTRAINT agent_sub_agents_agent_fk FOREIGN KEY (company_id, agent_id)
        REFERENCES agents(company_id, id) ON DELETE CASCADE,
    CONSTRAINT agent_sub_agents_sub_fk FOREIGN KEY (company_id, sub_agent_id)
        REFERENCES agents(company_id, id) ON DELETE CASCADE
);
```

Composite FKs here, unlike `agent_skills`: delegation is between two agents of the same company, and
a library agent has no siblings to delegate to.

New columns written **inline in `CREATE TABLE agents`**, not as an `ALTER` — the file is a squashed
baseline:

```sql
    -- Which runtime executes this agent. Must stay in sync with `HarnessKind::as_str`.
    harness_kind TEXT NOT NULL DEFAULT 'ai_agents'
        CHECK (harness_kind IN ('ai_agents')),
    -- Catalogue ids, not rows: the grantable tools are a compile-time const
    -- (`entities/tool_catalogue.rs`), so there is nothing to reference. Validated against the
    -- catalogue in `AgentWrite::normalize` and filtered again when the spec is compiled.
    granted_tool_ids TEXT[] NOT NULL DEFAULT '{}',
    CONSTRAINT agents_granted_tool_ids_bounded CHECK (
        cardinality(granted_tool_ids) <= 32
        AND array_position(granted_tool_ids, NULL) IS NULL
    ),
```

## 2. Domain

`SkillWrite` in `use_cases/skill.rs` (not the entity module) mirrors `AgentWrite`
(`use_cases/agent.rs:37`) — one struct so create and update cannot drift, values normalized once:

```rust
pub struct SkillWrite {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub trigger: String,
    pub instructions: Vec<SkillInstruction>,
    pub created_by: Option<CreationProvenance>,
}

impl SkillWrite {
    pub(crate) fn normalize(&mut self) -> AppResult<()> { … }   // trims, lowercases slug, then Skill::validate
}
```

`AgentWrite` gains four fields: `harness_kind: HarnessKind`, `granted_tool_ids: Vec<ToolId>`,
`skill_ids: Vec<Uuid>`, `sub_agent_ids: Vec<Uuid>`. Its `normalize()` gains the bounds:

- every `granted_tool_ids` entry resolves through `CatalogueTool::get` and is grantable — refuse
  with the id named, do not silently drop (dropping is the compiler's job for grants that predate a
  catalogue change; a *submission* of an unknown id is a bug worth surfacing);
- `granted_tool_ids` deduplicated, ≤ 32;
- `skill_ids` ≤ `MAX_AGENT_SKILLS` (16) and deduplicated;
- `sub_agent_ids` deduplicated and excludes the agent's own id.

## 3. Close the escape hatch

`validate_agent_config` (`use_cases/agent.rs:123`) already rejects reserved paths `name`,
`system_prompt`, and `llm.provider|model|api_key`. Add `tools` and `skills`:

```rust
for reserved in ["name", "system_prompt", "tools", "skills"] {
```

**This is not cosmetic.** Without it, typing `{"tools": ["command"]}` into the config textarea puts
`command` into `extra_config`, which `merge_json` folds into the config before the compiler
overwrites `tools:` — and the whole of phase 1's tool policy becomes decorative for anyone who
notices. Phase 3's compiler filters it anyway (step 3 runs after step 2, deliberately), so this is
the second of two independent defences; both should exist, and the error message should name the
picker as the alternative.

Update `docs/custom_tools.md` in the same commit: the grant is a typed field now, and its statement
that *"the runtime's effective tool set is built from the top-level `tools:` list"* stays true but
the authoring path changes.

## 4. Ports — `src/application/use_cases/skill.rs` (new)

```rust
#[async_trait]
pub trait SkillPersistence: Send + Sync {
    async fn create(&self, company_id: Uuid, write: SkillWrite) -> AppResult<Skill>;
    async fn create_library(&self, write: SkillWrite) -> AppResult<Skill>;
    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Skill>>;
    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Skill>>;
    async fn list_library(&self) -> AppResult<Vec<Skill>>;
    async fn update(&self, id: Uuid, write: SkillWrite) -> AppResult<Skill>;
    async fn delete(&self, id: Uuid) -> AppResult<()>;
    /// Skills attached to one agent, in `position` order. Read on every agent run.
    async fn list_for_agent(&self, agent_id: Uuid) -> AppResult<Vec<Skill>>;
}
```

**No default bodies.** `AgentPersistence`'s `create_library`/`list_library` defaults
(`use_cases/agent.rs:168`) are the anti-pattern `src/application/AGENTS.md` names — a test double
that quietly returns `Ok(vec![])` lets an authorization or capability test pass while the port is
unimplemented. Every impl and every test double states its behaviour.

`AgentPersistence` gains `replace_agent_skills` and `replace_agent_sub_agents`, each taking the full
ordered list and applying it in one transaction — not `add`/`remove` pairs, which invite a partial
update on error.

`SkillUseCases` holds `Arc<dyn SkillPersistence>`, with `#[instrument(skip(self))]` on every public
async method and `info!(%company_id, %skill_id, …)` on each write.

```rust
/// Insert a company-owned copy of a global library skill.
///
/// A copy, not a reference: mirrors `create_agent_from_library` (`use_cases/agent.rs:560`) so a
/// company can diverge from the library, at the cost of operator fixes not propagating.
pub async fn create_skill_from_library(&self, company_id: Uuid, skill_id: Uuid) -> AppResult<Skill>;
```

Slug collision on copy is real — a company may already have `summarize-thread`. Suffix and retry
once (`summarize-thread-2`), or surface a `Conflict` with the offending slug named. Decide and test
it; do not leave the 23505 to reach the user as "Conflict".

## 5. Persistence — `src/adapters/persistence/skill.rs` (new)

Follow `company_resend.rs`, the freshest slice:

- a module doc naming the invariant the file holds;
- a shared `SKILL_COLUMNS` const `format!`ed into every statement — never `SELECT *`;
- `SkillDb` (`FromRow`) + **`TryFrom<SkillDb> for Skill`**, not `From`: `instructions` is JSONB and
  can fail to deserialize. `src/adapters/persistence/AGENTS.md` — treat persisted JSON as untrusted,
  never `expect` it. A row whose instructions no longer parse must surface as an error naming the
  skill id, not a panic on the agent run path;
- ownership enforced **inside the SQL**, never as a prior query:
  ```rust
  "SELECT {SKILL_COLUMNS} FROM skills AS skill \
   JOIN companies AS company ON company.id = skill.company_id \
   WHERE skill.id = $1 AND company.user_id = $2"
  ```
- table aliases spelled as words with explicit `AS`;
- `23505` → `AppError::Conflict` with a human sentence; `23514` from the scope trigger →
  `AppError::BadRequest`; `23503` from the delete guard → a sentence saying which agents still use
  the skill;
- `#[cfg(test)] #[path = "skill_tests.rs"] mod tests;` — and actually write that file. The sibling
  `company_resend_tests.rs` is currently 0 bytes; do not repeat that.

Register in `src/adapters/persistence/mod.rs`.

`replace_agent_skills` is one transaction: `DELETE FROM agent_skills WHERE agent_id = $1` then a
positional insert. Positions are dense from 0, so `agent_skills_agent_position_key` holds.

**Positional binds append, never insert** — `agents` gains two columns, so the INSERT list, the
UPDATE SET list, the shared SELECT const and the `AgentDb` struct all move together, and the new
placeholders go at the end.

## 6. Wiring

`AppState` gains `skill_use_cases: Arc<SkillUseCases>`; `infra/setup.rs` builds it from
`postgres_arc.clone()` beside the existing use cases. `AgentUseCases` gains the
`Arc<dyn SkillPersistence>` it needs to load an agent's skills when building a spec.

## 7. Regenerate the query cache

```sh
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo sqlx prepare -- --all-targets
```

`agent.rs` is one of the four files using compile-time `sqlx::query!` macros, and it gains columns —
so `.sqlx/` genuinely changes here. An empty diff means something was missed. Commit the result.

---

## Tests

**Pure**, in `use_cases/skill.rs`:
- `normalize_lowercases_the_slug_and_trims_every_field`
- `a_write_whose_last_instruction_is_a_tool_step_is_refused`
- `an_agent_write_naming_an_ungrantable_tool_is_refused_with_the_id_named`
- `validate_agent_config_rejects_a_tools_key` / `…_a_skills_key`

**DB-backed**, in `skill_tests.rs` — suffix every library slug with `Uuid::new_v4().simple()`, the
index is globally unique and the suite runs in parallel:
- `a_company_skill_round_trips`
- `copying_a_library_skill_produces_a_company_owned_row` — assert the library row is unchanged
- `copying_a_library_skill_twice_does_not_collide` — whichever resolution was chosen
- `another_companys_skill_is_not_readable`
- `deleting_an_agent_cascades_its_agent_skills`
- `deleting_a_skill_cascades_its_agent_skills`
- `attaching_another_companys_skill_is_refused_by_the_scope_trigger`
- `deleting_an_in_use_library_skill_is_refused`
- `replace_agent_skills_is_atomic_and_renumbers_positions`
- `granted_tool_ids_beyond_thirty_two_are_refused`

## Done when

```sh
psql "postgres://mac03@localhost:5432/mail_agents" -c '\d skills' -c '\d agent_skills' -c '\d agents'
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo test
SQLX_OFFLINE=true cargo build
```

Skills are storable and attachable, and nothing reads them yet — that is phase 5.
