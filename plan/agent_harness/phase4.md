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

Create a new timestamped additive migration; the squashed baseline
`migrations/20260817000000_init_schema.sql` has already been applied and must not be edited. The
migration creates `skills`, `agent_skills` and `agent_sub_agents`, then adds the three agent columns
with production-safe defaults for older insert paths. Apply it to an existing database so the
backfill path is exercised, not only to an empty test database.

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
    CONSTRAINT skills_name_bounded CHECK (
        btrim(name) <> '' AND char_length(name) <= 120
    ),
    CONSTRAINT skills_description_bounded CHECK (
        btrim(description) <> '' AND char_length(description) <= 500
    ),
    CONSTRAINT skills_trigger_bounded CHECK (
        btrim(trigger) <> '' AND char_length(trigger) <= 500
    ),
    CONSTRAINT skills_slug_format CHECK (
        char_length(slug::text) <= 120
        AND
        slug::text = lower(slug::text)
        AND slug::text ~ '^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$'
    ),
    CONSTRAINT skills_instructions_shape CHECK (
        jsonb_typeof(instructions) = 'array'
        AND jsonb_array_length(instructions) BETWEEN 1 AND 32
        AND octet_length(instructions::text) <= 524288
    ),
    CONSTRAINT skills_created_by_shape_check CHECK (valid_creation_provenance(created_by))
);

-- `skills_company_slug_key` does not constrain library skills: UNIQUE treats every NULL
-- `company_id` as distinct, so the library needs its own uniqueness over slug alone. Same reasoning
-- as `agents_library_slug_key`.
CREATE UNIQUE INDEX skills_library_slug_key ON skills (slug) WHERE company_id IS NULL;

CREATE INDEX skills_company_updated_idx ON skills (company_id, updated_at DESC, id DESC);
```

`skills_company_updated_idx` matches the exact company cursor query. Before retaining it, load a
representative multi-tenant fixture and record `EXPLAIN (ANALYZE, BUFFERS)` for the first and a deep
page as required by `src/adapters/persistence/AGENTS.md`; remove the index if the measured plan does
not use it. The two reverse join indexes below serve FK delete/cascade probes, not speculative list
sorting.

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
    CONSTRAINT agent_skills_position_check CHECK (position BETWEEN 0 AND 15)
);

CREATE INDEX agent_skills_skill_idx ON agent_skills (skill_id, agent_id);

-- Copies `enforce_channel_agent_scope`: each side must match this row's exact company scope. NULL
-- therefore joins a global agent only to a global skill; it is not a wildcard.
CREATE FUNCTION enforce_agent_skill_scope() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM agents AS agent
        WHERE agent.id = NEW.agent_id
          AND agent.company_id IS NOT DISTINCT FROM NEW.company_id
    ) THEN
        RAISE EXCEPTION 'agent must match the relationship company scope'
            USING ERRCODE = '23514', CONSTRAINT = 'agent_skills_agent_scope_check';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM skills AS skill
        WHERE skill.id = NEW.skill_id
          AND skill.company_id IS NOT DISTINCT FROM NEW.company_id
    ) THEN
        RAISE EXCEPTION 'skill must match the relationship company scope'
            USING ERRCODE = '23514', CONSTRAINT = 'agent_skills_skill_scope_check';
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
            USING ERRCODE = '23503', CONSTRAINT = 'library_skill_delete_guard';
    END IF;
    RETURN OLD;
END;
$$;

CREATE TRIGGER library_skill_delete_guard
BEFORE DELETE ON skills
FOR EACH ROW EXECUTE FUNCTION prevent_assigned_library_skill_delete();
```

The `IS NOT DISTINCT FROM` checks are the database expression of copy-on-pick: a company agent can
attach only company-owned skills from the same company, while a global library agent can attach
only global library skills. There is no company-agent → live-library-skill exception. Copying a
library agent copies its attached skills into company-owned rows and attaches those copies in the
same transaction.

```sql
-- Which siblings this agent may delegate to. Restricts only when non-empty: an agent with no rows
-- reaches every company sibling, which is today's behaviour and stays the default.
CREATE TABLE agent_sub_agents (
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL,
    sub_agent_id UUID NOT NULL,
    -- Ordering is not product-visible, but gives the relationship a SQL-enforceable cardinality
    -- bound and deterministic reads.
    position INTEGER NOT NULL,
    PRIMARY KEY (agent_id, sub_agent_id),
    CONSTRAINT agent_sub_agents_agent_position_key UNIQUE (agent_id, position),
    CONSTRAINT agent_sub_agents_position_check CHECK (position BETWEEN 0 AND 63),
    CONSTRAINT agent_sub_agents_not_self CHECK (agent_id <> sub_agent_id),
    CONSTRAINT agent_sub_agents_agent_fk FOREIGN KEY (company_id, agent_id)
        REFERENCES agents(company_id, id) ON DELETE CASCADE,
    CONSTRAINT agent_sub_agents_sub_fk FOREIGN KEY (company_id, sub_agent_id)
        REFERENCES agents(company_id, id) ON DELETE CASCADE
);

CREATE INDEX agent_sub_agents_sub_idx ON agent_sub_agents (sub_agent_id, agent_id);
```

Composite FKs here, unlike `agent_skills`: delegation is between two agents of the same company, and
the relationship belongs to a company-owned agent. A global library agent cannot persist a
sub-agent allowlist. If it is assigned directly to a company channel, its missing rows deliberately
resolve to `AllCompanySiblings`; copy it into the company before assignment when a restricted scope
is required. Keep that compatibility rule visible in the library picker instead of implying that a
global agent has no reachable siblings at execution time.

New columns are added compatibly to existing agent rows:

```sql
CREATE FUNCTION valid_tool_id_array(ids TEXT[]) RETURNS BOOLEAN
LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT cardinality(ids) <= 32
       AND array_position(ids, NULL) IS NULL
       AND COALESCE(bool_and(btrim(id) <> '' AND char_length(id) <= 120), TRUE)
    FROM unnest(ids) AS id;
$$;

ALTER TABLE agents
    ADD COLUMN harness_kind TEXT NOT NULL DEFAULT 'ai_agents',
    ADD COLUMN granted_tool_ids TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN native_tool_policy JSONB NOT NULL DEFAULT '{"version": 1}',
    ADD CONSTRAINT agents_harness_kind_check
        CHECK (harness_kind IN ('ai_agents')),
    ADD CONSTRAINT agents_granted_tool_ids_bounded
        CHECK (valid_tool_id_array(granted_tool_ids)),
    ADD CONSTRAINT agents_native_tool_policy_shape CHECK (
        jsonb_typeof(native_tool_policy) = 'object'
        AND native_tool_policy ->> 'version' = '1'
        AND octet_length(native_tool_policy::text) <= 16384
    ),
    ADD CONSTRAINT agents_config_v1_shape_check
        CHECK (
            config_json IS NULL
            OR (
                jsonb_typeof(config_json) = 'object'
                AND config_json ->> 'version' = '1'
                AND octet_length(config_json::text) <= 65536
            )
        );
```

### Compatibility preflight and backfill

Adding an empty grant column and then ignoring legacy `config_json.tools` would silently remove
capabilities from existing agents. In the migration, inventory every non-null config object before
the new reader becomes authoritative:

- require legacy `tools` entries to be simple strings, copy only the reviewed, grantable phase-1
  catalogue ids into `granted_tool_ids` in their original order, then remove `tools` from
  `config_json`; excluded ids remain excluded, matching the phase-3 effective grant. The migration
  freezes that reviewed list at migration time, and its test compares the list with that revision's
  `CatalogueTool::grantable()` output so an omitted native or built-in is visible;
- abort with a descriptive migration error if a row contains legacy inline `skills`; migrate those
  deliberately into skill rows with application-generated UUIDs before retrying rather than
  silently deleting them;
- translate the existing bounded values under `tool_security.tools.<native-id>.config` into the
  versioned `native_tool_policy`; reject timeout/output/approval overrides rather than carrying them
  across as authority;
- inventory every remaining top-level key against the new residual-config allowlist. Unsupported
  security/runtime keys fail the migration with row ids and key names so an operator can translate
  or remove them explicitly. Rewrite accepted documents into canonical V1 shape with
  `"version": 1`; leave SQL `NULL` as the canonical default configuration for compatibility with
  older writers.

The executable migration orders this as: preflight legacy JSON shape, size and unsupported keys;
add the columns with safe defaults; backfill and remove translated legacy paths; then validate the
final rows. Run the descriptive preflight before the `ALTER TABLE` sketch above so an incompatible
legacy row reports its id and path rather than only a generic constraint failure. Keep the SQL
defaults: an insert path that omits only the new capability columns still produces a valid,
least-privilege row.

The V1 `config_json` constraint is a contract change and is not compatible with an old binary still
writing unversioned documents. Drain/stop application writers, run the preflight/backfill migration,
deploy the new binary, then resume traffic. If zero-downtime rollout is required, split this into
expand/deploy/contract migrations and specify a bounded, fail-closed V0 reader before coding; do not
call the one-migration path rolling-safe.

Put the inventory query and the tool backfill in a migration test with representative old rows. The
deployment must prove both old-row compatibility and a clean install; resetting an empty database
tests neither the upgrade nor the failure message.

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
    pub(crate) fn normalize(&mut self) -> AppResult<()> { … }
}
```

`normalize()` trims every text field, lowercases and validates the slug (including the 120-character
bound), calls `Skill::validate`, serializes `instructions` once to enforce
`MAX_SKILL_INSTRUCTIONS_JSON_BYTES` (524,288), and returns a worded `BadRequest`. The SQL constraints
mirror the same constants. `Skill` gains `updated_at`; otherwise the schema would store a concurrency
fact no domain/API reader can observe.

`created_by` is assigned by the use case on create and is never accepted as caller authority.
Updates preserve the stored creator and set `updated_at = CURRENT_TIMESTAMP`; neither field is
rewritten from `SkillWrite`.

Residual harness config has its own serialized bound, `MAX_AGENT_HARNESS_CONFIG_BYTES` (65,536),
enforced before persistence and again by a database `octet_length(config_json::text)` constraint in
the additive migration. Every allowed nested option still has its own semantic bound; the document
bound is only the allocation ceiling.

`AgentWrite` gains five fields: `harness_kind: HarnessKind`, `granted_tool_ids: Vec<ToolId>`,
`native_tool_policy: NativeToolPolicy`, `skill_ids: Vec<Uuid>`, `sub_agent_ids: Vec<Uuid>`. The
versioned policy owns the current outreach target/count/timeout settings and directory result bound;
approval requirements, execution timeouts and output ceilings remain server-owned and are not in
it. Parse persisted policy JSON fallibly and validate every numeric/enum field. `normalize()` gains
the bounds:

- every `granted_tool_ids` entry resolves through `CatalogueTool::get` and is grantable — refuse
  with the id named, do not silently drop (dropping is the compiler's job for grants that predate a
  catalogue change; a *submission* of an unknown id is a bug worth surfacing);
- `granted_tool_ids` deduplicated, ≤ 32;
- `skill_ids` ≤ `MAX_AGENT_SKILLS` (16) and deduplicated;
- `sub_agent_ids` ≤ `MAX_AGENT_SUB_AGENTS` (64) and deduplicated.

After tenant-scoped skills are loaded, the write path also bounds the deduplicated union of direct
and skill-implied grants at `MAX_EFFECTIVE_AGENT_TOOLS` (32). This decision needs loaded data, so it
belongs in the use case/capability persistence operation rather than `AgentWrite::normalize()`.

Until the protocol has an explicit ephemeral-child grant, a restricted allowlist is incompatible
with an effective `create_agent_channel` grant. That tool creates an id which cannot already appear
in the persisted list and then asks the model to contact it; silently adding the new id would bypass
the stored authorization boundary, while allowing creation followed by a guaranteed refusal would
leave a surprising orphan channel. Reject this combination during the same loaded-data validation,
whether the tool was granted directly or implied by a skill.

Self-delegation cannot be checked by `normalize()` because a create does not have its id yet. The
update use case checks it once the id is known, and `agent_sub_agents_not_self` remains the final
database guarantee for every path.

The persisted `Agent`/`AgentDb` read model gains `harness_kind`, `granted_tool_ids` and
`native_tool_policy`, parsed fallibly with the agent id in any error. Relationship bodies do not
become fields on `Agent`; the execution read port below returns them as one capability snapshot.
This avoids a general-purpose agent read silently paying for three joins.

## 3. Close the escape hatch

Replace `validate_agent_config`'s reserved-path denylist with a harness-specific, fail-closed
`#[serde(deny_unknown_fields)] AiAgentsAdvancedConfigV1`; do not recursively inspect and then keep an
arbitrary `Value`. The starting surface is deliberately small: bounded reasoning mode/iterations,
bounded reflection mode/retries, and the disambiguation enabled flag. Reject alternate
`judge_llm`/`evaluator_llm`/planner/detection model names, custom prompts and criteria, planning
tool/skill selectors, visible reasoning output, caches and free-form skip conditions. Those fields
either select provider work, influence a prompt, expose hidden reasoning, or create their own work
bounds. Preserve another existing field only after inventorying stored `agents.config_json` values,
proving it cannot grant/register a tool, access host storage, weaken approval/tool bounds, replace
trusted context, or create unbounded provider work, and adding a typed field plus its own bounds.
Unknown top-level and nested keys are rejected with the exact path named. Do not admit free-form
`metadata` merely because the runtime currently treats it as inert; an upstream consumer could turn
an unvalidated bag into behavior.

This is not cosmetic. The pinned runtime adds effective grants from
`spawner.management_tools`, `spawner.orchestration_tools`, and
`persona.evolution.allow_llm_evolve` even when top-level `tools` is empty. Reject `spawner`,
`persona`, `tools`, `skills`, `hitl`, `tool_security`, `context`, `observability`, `storage`,
`runtime`, `process`, `states`, `llms`, `tool_aliases`, and provider-owned `llm` configuration until
each has a typed platform field. The persistence conversion rejects a direct-SQL row that does not
decode as the typed DTO, and the adapter constructs a fresh base rather than sanitizing and merging
the raw document. It stamps server-owned values last. Write-time validation gives a useful client
error; fallible reads and construction from typed fields protect legacy/imported rows.

Represent the accepted value as
`HarnessConfig::AiAgents(AiAgentsAdvancedConfigV1)` in `AgentCapabilitySpec`, not a supposedly
harness-neutral `Value`. The DTO has a structural version and explicit defaults for fields added
compatibly. When a second harness lands, switching `harness_kind` cannot reinterpret the old
runtime's document. The adapter maps this DTO into the upstream `AgentSpec`; it never merges the
stored JSON wholesale. Any internal evaluator/detector reference required by an enabled reviewed
mode is derived from the already resolved company credential; it is not accepted as another model
selector from the stored document.

Update `docs/custom_tools.md` in the same commit: grants are typed fields, and top-level `tools` is
only the ordinary-grant source. Document the separately denied upstream feature-grant paths.

## 4. Ports — `src/application/use_cases/skill.rs` (new)

```rust
#[async_trait]
pub trait SkillManagementPersistence: Send + Sync {
    async fn create_company(&self, company_id: Uuid, write: SkillWrite) -> AppResult<Skill>;
    async fn create_library(&self, write: SkillWrite) -> AppResult<Skill>;
    async fn get_company(&self, company_id: Uuid, skill_id: Uuid) -> AppResult<Option<Skill>>;
    async fn get_library(&self, skill_id: Uuid) -> AppResult<Option<Skill>>;
    async fn list_company_page(&self, company_id: Uuid, page: SkillPageRequest)
        -> AppResult<SkillPage>;
    async fn list_library_page(&self, page: SkillPageRequest) -> AppResult<SkillPage>;
    async fn update_company(
        &self, company_id: Uuid, skill_id: Uuid, write: SkillWrite,
    ) -> AppResult<Skill>;
    async fn update_library(&self, skill_id: Uuid, write: SkillWrite) -> AppResult<Skill>;
    async fn delete_company(&self, company_id: Uuid, skill_id: Uuid) -> AppResult<()>;
    async fn delete_library(&self, skill_id: Uuid) -> AppResult<()>;
}

/// The complete stored configuration for one executable agent, read consistently.
pub struct StoredAgentCapabilities {
    pub agent: Agent,
    pub skills: Vec<Skill>,
    pub sub_agent_scope: SubAgentScope,
}

#[async_trait]
pub trait AgentCapabilityReader: Send + Sync {
    async fn load_for_execution(
        &self,
        execution_company_id: Uuid,
        agent_id: Uuid,
    ) -> AppResult<Option<StoredAgentCapabilities>>;
}
```

`SkillPageRequest.limit` is clamped/rejected at `MAX_SKILL_PAGE_SIZE` (100) and uses the existing
deterministic timestamp-plus-UUID cursor convention. Selection UIs load selected ids explicitly in
addition to a page; no route or picker loads an unbounded company/global library.

**No default bodies.** `AgentPersistence`'s `create_library`/`list_library` defaults
(`use_cases/agent.rs:168`) are the anti-pattern `src/application/AGENTS.md` names — a test double
that quietly returns `Ok(vec![])` lets an authorization or capability test pass while the port is
unimplemented. Every impl and every test double states its behaviour.

Do **not** add independently committed `replace_agent_skills` and `replace_agent_sub_agents`
methods. Extend the existing purpose-specific `OwnedAgentChannelPersistence` create/update
operations so the agent row, owned address, skill selection and sub-agent selection agree in one
transaction. Library-agent create/update gets an equivalent atomic operation for the agent row and
its library-skill selection. An `AgentWrite` is the complete desired state.

`SkillUseCases` holds `Arc<dyn SkillManagementPersistence>` and the company-access port it needs to
authorize tenant operations. Company methods take `user_id` and `company_id`; they verify owner
access before calling the already-company-scoped persistence method. Global-library methods stay
separate and are reached only after the HTTP operator check. Put `#[instrument(skip(self, write))]`
on methods that accept a write and `#[instrument(skip(self))]` on reads; log stable ids as structured
fields and never derive/log a full write containing instruction text.

```rust
/// Insert a company-owned copy of a global library skill.
///
/// A copy, not a reference: mirrors `create_agent_from_library` (`use_cases/agent.rs:560`) so a
/// company can diverge from the library, at the cost of operator fixes not propagating.
pub async fn create_skill_from_library(
    &self, user_id: Uuid, company_id: Uuid, skill_id: Uuid,
) -> AppResult<Skill>;
```

Slug collision on copy is real. Try the original slug, then suffixes `-2` through `-100`, retrying
only a company/slug conflict; after that return a worded `Conflict`. Inside a transaction, use
`INSERT ... ON CONFLICT (company_id, slug) DO NOTHING RETURNING ...` for each candidate rather than
catching `23505`, because an ordinary uniqueness error aborts the PostgreSQL transaction before the
next suffix can be tried. This is bounded and remains correct when two requests copy the same skill
concurrently. Test the competing copy requests rather than only sequential calls. Truncate the stem
before adding a suffix so every candidate remains within the slug bound.

The agent-library copy transaction reuses this exact helper for every attached library skill; it
does not open a second transaction per skill. One failure rolls back the copied agent, channel,
address, skills and relationship rows together.

## 5. Persistence — `src/adapters/persistence/skill.rs` (new)

Follow `company_resend.rs`, the freshest slice:

- a module doc naming the invariant the file holds;
- a shared `SKILL_COLUMNS` const `format!`ed into every statement — never `SELECT *`;
- `SkillDb` (`FromRow`) + **`TryFrom<SkillDb> for Skill`**, not `From`: `instructions` is JSONB and
  can fail to deserialize. After decoding, call `Skill::validate()` and attach the skill id to any
  error. Persisted JSON is untrusted; a row containing an obsolete tool, an over-bound value or a
  final tool step must fail capability resolution explicitly, not panic or die midway through a
  harness run;
- ownership enforced **inside the SQL**, never as a prior query:
  ```rust
  "SELECT {SKILL_COLUMNS} FROM skills AS skill \
   WHERE skill.company_id = $1 AND skill.id = $2"
  ```
  Library methods include `skill.company_id IS NULL` in their own statements; no raw-id mutation
  method serves both scopes.
- table aliases spelled as words with explicit `AS`;
- `23505` → `AppError::Conflict` with a human sentence; `23514` from the scope trigger →
  `AppError::BadRequest`; `23503` from the delete guard → a sentence saying which agents still use
  the skill;
- `#[cfg(test)] #[path = "skill_tests.rs"] mod tests;` — and actually write that file. The sibling
  `company_resend_tests.rs` is currently 0 bytes; do not repeat that.

Register in `src/adapters/persistence/mod.rs`.

The owned-agent create/update transaction deletes and replaces both relationship sets, then writes
dense positions from zero. Validate every caller-supplied skill and sub-agent id in tenant-scoped
statements inside that transaction before deleting the old selection, so one bad id changes
nothing. The library-agent transaction accepts only library skill ids. Map the named FK/check
violations to worded `BadRequest`s.

`AgentCapabilityReader::load_for_execution` returns the agent row, ordered skills and the resolved
sub-agent scope from one consistent snapshot: use one SQL statement or an explicit
`REPEATABLE READ READ ONLY` transaction, not several default `READ COMMITTED` statements. It accepts
`execution_company_id` because a global
library agent may be assigned to a company channel; the SQL admits an agent owned by that company or
the global library and relies on the strict `agent_skills` scope invariant for its skills. A global
row has no persisted `agent_sub_agents`; resolve that case explicitly as `AllCompanySiblings`, not
as an accidental empty vector later in dispatch. Do not assemble this snapshot from independently
committed reads in dispatch.

**Positional binds append, never insert** — `agents` gains three columns, so the INSERT list, the
UPDATE SET list, the shared SELECT const and the `AgentDb` struct all move together, and the new
placeholders go at the end.

## 6. Wiring

`AppState` gains only `skill_use_cases: Arc<SkillUseCases>` for the HTTP surfaces. `infra/setup.rs`
builds it beside the existing use cases. `AgentUseCases` receives the capability persistence needed
for atomic agent/library writes. `ThreadUseCases` receives `Arc<dyn AgentCapabilityReader>` because
dispatch, not `AgentUseCases`, builds executable specs.

Do not add `HarnessRegistry` to `AppState`: phase 3 already constructs it in `infra/setup.rs` and
wires it into `ThreadUseCases`. Phase 5 changes the kind selected from the stored snapshot; it does
not create a second registry path.

## 7. Regenerate the query cache

```sh
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" \
  cargo sqlx prepare -- --all-targets
```

`agent.rs` is one of the four files using compile-time `sqlx::query!` macros, and it gains columns —
so `.sqlx/` genuinely changes here. An empty diff means something was missed. Commit the result.

---

## Tests

**Pure**, in `use_cases/skill.rs`:
- `normalize_lowercases_the_slug_and_trims_every_field`
- `a_write_whose_last_instruction_is_a_tool_step_is_refused`
- `an_agent_write_naming_an_ungrantable_tool_is_refused_with_the_id_named`
- `advanced_config_rejects_tools_spawner_persona_and_security_owned_paths`
- `advanced_config_rejects_an_unknown_key_with_the_full_path_named`
- `advanced_config_accepts_only_the_reviewed_bounded_options`
- `advanced_config_rejects_alternate_llms_prompts_planning_caches_and_visible_reasoning`
- `native_tool_policy_refuses_out_of_bound_counts_timeouts_and_unknown_versions`
- `a_restricted_scope_rejects_direct_or_skill_implied_agent_creation`

**Adapter security regression**, in `adapters/harness/ai_agents`:
- build a runtime from a legacy/direct-SQL spec containing spawner management, orchestration and
  persona-evolution grants, then assert its **effective** available tool ids contain none of them;
- assert user config cannot disable HITL, raise tool ceilings, enable prompt/response observability,
  or replace server runtime context after compilation;
- assert every effective tool id is the intersection of the typed grant/skill requirements, the
  platform catalogue, and native tools actually available to the run.

**DB-backed**, in `skill_tests.rs` — suffix every library slug with `Uuid::new_v4().simple()`, the
index is globally unique and the suite runs in parallel:
- `a_company_skill_round_trips`
- `copying_a_library_skill_produces_a_company_owned_row` — assert the library row is unchanged
- `copying_a_library_skill_twice_uses_bounded_distinct_suffixes`
- `two_competing_library_skill_copies_receive_distinct_slugs`
- `another_companys_skill_is_not_readable`
- `another_companys_skill_is_not_updatable_or_deletable`
- `deleting_an_agent_cascades_its_agent_skills`
- `deleting_a_skill_cascades_its_agent_skills`
- `attaching_another_companys_skill_is_refused_by_the_scope_trigger`
- `deleting_an_in_use_library_skill_is_refused`
- `an_agent_capability_update_is_atomic_and_renumbers_positions`
- `an_invalid_sub_agent_leaves_the_previous_capability_selection_unchanged`
- `too_many_effective_tools_from_selected_skills_leave_the_previous_selection_unchanged`
- `a_company_agent_cannot_attach_a_live_library_skill`
- `a_library_agent_cannot_attach_a_company_skill`
- `a_direct_library_agent_resolves_to_all_company_siblings`
- `granted_tool_ids_beyond_thirty_two_are_refused`
- `skill_position_sixteen_is_refused_by_the_database`
- `sub_agent_position_sixty_four_is_refused_by_the_database`
- `malformed_but_deserializable_skill_json_is_a_contextual_conversion_error`
- `the_tool_array_sql_helper_matches_rust_validation_for_empty_null_blank_long_and_bounded_values`
- `the_migration_grantable_tool_snapshot_matches_the_phase_one_catalogue`
- `the_upgrade_backfills_legacy_simple_tool_grants_and_refuses_unmigrated_inline_skills`

## Done when

```sh
psql "postgres://$(whoami)@localhost:5432/mail_agents" -X \
  -c '\d skills' -c '\d agent_skills' -c '\d agent_sub_agents' -c '\d agents'
SQLX_OFFLINE=true cargo check --locked --all-targets
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo test --locked --all-targets
```

Skills are storable and attachable, and nothing reads them yet — that is phase 5.
