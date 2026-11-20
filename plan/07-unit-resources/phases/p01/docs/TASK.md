# 07UR-P01: Ownership and selection

## Identity and authority
- Workflow implement-opus-reviewed-2_4; phase p01 = Plan07 doc 01 (01A schema, 01B selection and
  checks); record root plan/07-unit-resources/phases/p01/docs.
- Criteria: ../../../README.md (decisions 1, 2, 6, 7; "What does not change"),
  ../../../01-ownership-and-selection.md; AGENTS.md (no backward compatibility; competing-claimant
  tests for concurrency protocols; focused checks per batch, full gate in p03).
- Root/A: Claude Code Opus 5.5/xhigh (user-confirmed), session
  b36d9dc4-495e-4131-9804-05ebcc381915. B: Opus 5.5 by user choice, agent ID recorded at spawn.
- Worktree /Users/mac02/orca/workspaces/openbot/plan06, branch plan07, starting commit cc545881.
  Preserved pre-existing untracked files (not part of this phase):
  .trail/learnings/2026-11-19-a-local-check-sh-run-can-mac02.md (user declined to commit) and
  plan/06-delegated-administration/phases/p01/docs/logs/B{1,2}-accepted-source.snapshot.tar,
  B3-frozen-source.snapshot.tar.
- Coordination: plan/07-unit-resources/COORDINATION.md r2 (this session is the writer). No
  cross-phase notices. Plan-wide user decisions (a) authority = unit managers everywhere,
  (b) machines out of scope, (c) budget caps by the unit's own manager, (d) agent resources follow
  the agent's own units, workflow-level connections the run's scope — (a), (b), (d) apply here.
- Prerequisites (integrated on origin/main): Plan05 run scopes (`workflow_run_scopes`), Plan06
  delegated administration (unit management predicate and 404/403 convention).
- Resources: UTF-8 PG18 cluster 127.0.0.1:55407 (owned by this session; fresh databases per
  check run, named openbot_p07_*); toolshim for rg/python3.13 in the session scratchpad.
- Snapshot exclusions (to be reused unchanged): plan/07-unit-resources/phases/p01/docs,
  plan/07-unit-resources/COORDINATION.md.

## Requirements (doc 01)
- 01A.1 skills, company_mcp_connections, workflow_http_connections gain `unit_id uuid NOT NULL`
  with a composite FK to departments (company_id, id) ON DELETE RESTRICT; creation defaults to
  the creator's single managed department, else the root; library rows (company_id IS NULL) have
  no unit, enforced by a check.
- 01A.2 Publication snapshots capturing a tool, connection or skill record the unit id with the
  resource, so a later move shows as a mismatch.
- 01A.3 Deleting a department that owns resources is refused by the FKs; the Departments pane
  lists what to move first.
- 01B.1 Authoring reads list only resources whose unit is the authoring unit or an ancestor;
  authoring unit = definition's managing department, or for agents the union of the agent's
  departments.
- 01B.2 Publication refuses a captured resource outside the authoring unit's chain with
  `publication.resource_outside_unit`, naming the step and resource.
- 01B.3 Admission compares every captured resource unit with the run's scope units and refuses
  with `admission.resource_outside_unit` when a resource moved out of reach; live authority
  events unchanged.
- 01B.4 `move_resource_on(tx, kind, id, target_unit)` requires managing source and target,
  updates unit_id, leaves published snapshots alone, and lists affected active bindings.
- Authority (user decision a): create/edit/delete/move of a unit's resource belongs to whoever
  manages that unit; root-owned resources to the owner, root admins and root leads.
- Out of scope (user decision b): local machines; recorded follow-up.
- Focused checks: `unit_resource_schema_`, `unit_resource_selection_`, `unit_resource_competing_`.

## Investigation (2026-11-20, file:line at cc545881)
- Authority today differs per kind: MCP use cases use `managed_company` (owner/admin,
  use_cases/mcp.rs:198-257); HTTP persistence uses `authorize_company` = Root management, so a
  root lead passes the API but the UI detail/save add `managed_company`
  (persistence/workflow/http.rs:150-304, routes/connection_levels.rs:189,218); skill writes are
  owner-only (`authorize_write`, use_cases/skill.rs:192) and the skill UI reads call
  `verify_company_owner`; reads for agent authoring are AnyUnit. Vault integration bindings
  (persistence/integration_bindings.rs:153,261,286) and MCP discovery refresh
  (mcp/discovery.rs:69) use `authorize_company`. HTTP connections have no delete.
- Management predicate (Plan06): SQL `unit_management_role(user,company,unit|NULL=root)` →
  owner/admin/lead:<nearest ancestor>; Rust `management_role_on(db,actor,company,
  ManagementTarget::{Root,Unit,..})`, `managed_units`, `require_management` (404).
  `workflow_creating_department(user,company)` = the creator's single non-root placement, else
  the root (init_schema:20188) — used by definitions; reused here as the default unit.
- departments `UNIQUE (company_id,id)` is the composite FK target; RESTRICT precedents
  `channels_department_fk`, `knowledge_documents_unit_fk`. Department delete
  (`guard_department_delete_on`, department/lifecycle.rs:202) returns explicit Conflicts for
  children/desks/definitions/role history; other FK violations become the generic "still
  referenced" (`delete_error` :311), rendered as one message on the delete card.
- Moves to mirror: knowledge `moves::move_on` (both units, revision, no-op, collision, update);
  desk `update_channel_on` (management of current and destination unit under company lock).
- Selection: agent catalog `AgentAuthoringReads::tool_connections`/`tool_options`
  (agent_authoring/catalog.rs:14-197, company + enabled only); agent skill options come from
  `SkillUseCases::list_company_page` (company only); the workflow editor uses
  `WorkflowAuthoringReads::publication_options(company, ..)` (persistence/workflow/
  authoring_reads.rs:466) and is not given the workflow.
- Publication: workflow `ToolSnapshot` carries an operation contract and a slot *name*; the
  publication selection names a connection (`mcp_publication.rs:40-66`,
  `http_publication.rs:40-70`) only to capture the operation, then drops the id. The connection a
  run uses is fixed by the binding's `workflow_binding_revisions.resources` (slot → id) or, for
  an unslotted HTTP tool, resolved live by `http::registered_on` (exactly one enabled connection
  with an equal operation snapshot, company-wide; also used at runtime, http_runtime.rs:166,236,
  authority.rs:174-186). Agent revisions keep connection ids (`AgentBindingSource::{Http,Mcp}
  {connection}`) and freeze skills as `SkillSnapshot` (agent_publication/capture.rs:151-225,
  skills read FOR SHARE).
- Admission: `admission.rs admit` → `resources::check_readiness` (root binding resources; also
  run at binding save, bindings.rs:75) → `admission_write::create`, which captures the agent
  manifest first (agent_manifest::capture_on: `workflow_edges` restores each reachable bundle and
  runs check_readiness with the root's resources; `agent_edges` reads each agent's live active
  revision and `agent_authority::check_definition_on`) and `run_scopes::capture_on` last, in the
  same transaction. Per-agent scope rows use the desk's chain, or the agent's own placement chain
  without a desk, plus lead report units. No `admission.*` code exists; the definite-refusal
  precedent is `AppError::KnowledgeTrigger` (code, HTTP JSON, message_admission `classify`);
  schedule refusals are a closed CHECK (init_schema:16456).

## Architecture gate G1 (decision revision r1)
Decisions for p01 that are costly to unwind; B passes this revision before dependent edits.

- **G1-D1 Schema.** `skills.unit_id uuid`, `company_mcp_connections.unit_id uuid NOT NULL`,
  `workflow_http_connections.unit_id uuid NOT NULL`, each with FK `(company_id, unit_id) →
  departments(company_id, id) ON DELETE RESTRICT` (immediate, like `channels_department_fk`).
  skills: `CHECK ((company_id IS NULL) = (unit_id IS NULL))`, so library rows have no unit and
  every company row has one (composite FK with MATCH SIMPLE skips library rows). Indexes on
  `(company_id, unit_id)`. Edited in the init migration; no backfill (database reset).
- **G1-D2 Authority (user decision a).** Every write to one of these resources — create, update,
  delete, access level, credential/secret, MCP refresh and tool grants, HTTP operation
  register/revoke and enable, Vault integration propose/confirm, move — requires
  `management_role_on(.., Unit(resource.unit_id))` under the existing company lock, 404 when
  absent (`require_management` wording per kind). Access-level ceilings stay `may_assign_level`
  (403). Configuration lists (MCP/HTTP catalogs, company skills page) need AnyUnit entry and show
  only resources whose unit the caller manages (`managed_units`); a get of a resource outside
  them is 404. Library operator paths and credential key rotation are unchanged.
- **G1-D3 Creation unit.** Create commands take an optional `unit_id`; absent, the unit is
  `workflow_creating_department(actor, company)`. The caller must manage the chosen unit (404
  otherwise). Library copy (`create_skill_from_library`) follows the same rule. Forms offer the
  caller's managed units with that default selected.
- **G1-D4 Move.** `move_resource_on(tx, kind ∈ {skill, mcp, http}, id, expected_revision?,
  target_unit)`: company lock, lock the row FOR UPDATE, require management of the current unit and
  of the target (404), no-op when equal, `UPDATE unit_id` (+ revision bump for MCP/HTTP, updated_at
  for skills); published snapshots untouched. The response lists affected active uses: agents
  whose active revision uses the resource (agent_revision_bindings / agent_revision_skills) and
  active bindings whose current configuration maps a slot to the connection, bounded (first 20 +
  count). JSON `POST …/{kind}/{id}/move` and a UI move form on each detail page.
- **G1-D5 Department delete.** `guard_department_delete_on` adds an explicit Conflict before the
  delete: "still owns skills or connections: <names, at most 10> (and N more). Move them first."
  The FK stays the enforcing guard; the message is the pane's list.
- **G1-D6 What is captured, and where the unit is checked.**
  - Agent revisions capture connection ids and skills, so the unit is captured there:
    `AgentBindingSource::{Http,Mcp}` and `SkillSnapshot` gain `unit_id`, read under the existing
    FOR SHARE row locks at agent publication.
  - A workflow `ToolSnapshot` captures an operation contract, not a connection: the connection is
    fixed by the binding's slot resources or resolved at admission. A unit is not added to
    `ToolSnapshot` (it would break the operation-snapshot equality used by `registered_on`,
    binding slot matching and the operation-row CHECKs, and protect nothing). The unit check for
    workflow-level connections runs where the connection is fixed (binding save) and at
    admission. This is the disposition of 01A.2 for workflow tools.
  - Unslotted HTTP resolution stays company-wide ("exactly one owner", fails closed on
    ambiguity); units that need their own connection for the same operation use a resource slot.
    The resolved connection must still pass the unit check.
- **G1-D7 Authoring unit and offers (01B.1).** "In reach of U" = unit_id is U or an ancestor of U.
  Agent authoring unit = the union of the agent's placement units (department_members ∪
  department_roles), root when it has none; agent tool connections and skill options list only
  resources in reach of any of them. Workflow editor `publication_options` takes the workflow id
  and lists connections in reach of its `managing_department_id`. Binding slot choices list
  connections in reach of the association's target unit chain.
- **G1-D8 Refusals.** One `AppError::ResourceOutsideUnit { stage, kind, resource_id, name,
  unit_id }` with codes `publication.resource_outside_unit` (agent publication; workflow
  publication of a selection naming an out-of-reach connection, reported as a diagnostic naming
  the step and tool), `binding.resource_outside_unit` (binding save: a slot or resolved connection
  outside the association's chain) and `admission.resource_outside_unit`. HTTP 400 JSON
  `{code, error, kind, resource}` like KnowledgeTrigger; message admission `classify` arm;
  `schedule_runs_refusal_check` gains the admission code; agent publication failure class
  `resource_outside_unit`. The binding-time code is an addition to the plan's two codes so a desk
  cannot be saved in a state every run refuses.
- **G1-D9 Admission check (01B.3).** In `admission_write::create`, after `run_scopes::capture_on`
  (same transaction, rows visible): (a) workflow-level connections — every connection the
  admission closure resolves (root bundle, children and agent-tool workflows via the root's
  resources and unslotted resolution) — must have a current unit in the base read set
  (`workflow_run_scopes`, acting_principal_id NULL, access 'read'); (b) for each agent in the
  manifest, each captured connection and skill must have current unit = captured unit (a move
  since publication is a mismatch, even upwards: republish) and that unit in the agent's read set
  (acting_principal_id = agent principal). Any failure refuses the whole admission with
  `admission.resource_outside_unit` before any run row survives (transaction rollback). The read
  set is the one run_scopes computes for memory and knowledge (README: "the same read set").
  Live authority events and dispatch checks are unchanged; an admitted run is not stopped by a
  later move.
- **G1-D10 Concurrency.** *(r2/r3: the refusal of the late case comes from the current unit being out of reach; a move up is not refused.)* Publication and admission read resource rows FOR SHARE (skills already;
  connections added where the unit is read); move takes FOR UPDATE under the company lock. A move
  racing a publication therefore either precedes it (refusal or new unit captured) or follows it
  (old unit captured, refused at admission by D9b). `unit_resource_competing_` exercises both
  orders with real concurrent transactions and asserts the refusal codes.
- **G1-D11 Formats.** Adding `unit_id` to agent revision content changes a persisted format:
  every Rust deserializer and SQL reader of `agent_revisions.content` and of prepared agent
  history (`PREPARED_AGENT_VERSION`, `agent_history_preparation_valid`,
  `agent_history_checkpoint_matches`) is searched exhaustively; versions bump where they encode
  the resolved definition; no dual decoding (database reset).
- **Batches.** B1 = D1–D5 (schema, authority, creation unit, move, delete guidance) with
  `unit_resource_schema_` and authority/move tests. B2 = D6–D11 (capture, offers, refusals,
  admission, competing) with `unit_resource_selection_` and `unit_resource_competing_`. B2 builds
  on accepted B1.
- **Risks named for review.** D2 widens skill writes (owner → any manager of the skill's unit)
  and connection writes (owner/admin → unit managers) by user decision; D6 is a deviation from
  01A.2's literal "ToolSnapshot records the unit"; D8 adds a third code; D9b refuses moved-up
  resources until republish *(superseded by r2 F3: moved-up resources stay usable)*.

## Architecture gate G1 (decision revision r2, 2026-11-20)
Answers B's 07UR-P01-G1-R1 (findings F1–F12) and user decision (d) (coordination r2): an agent's
own tools and skills must be in reach of one of the agent's current departments wherever it
runs; workflow-level connections follow the run's (desk's) scope. r1 stands except where changed
here.

- **D6/F1 capture location.** The unit is captured beside the binding, not in it:
  `ResolvedAgentTool` gains `unit_id` (the connection's unit when the tool has an HTTP/MCP
  connection) and `SkillSnapshot`/`ResolvedAgentSkill` gains `unit_id`. `AgentBindingSource` (the
  binding identity checked exactly by `check_agent_revision_members` and reused by
  `insert_identity_on`/`resolve_bindings_on`) is unchanged.
- **Reach, two scopes (user decision d; replaces D7's agent rule and D9).**
  - *Agent reach*: a resource is in reach of an agent when its unit is one of the agent's current
    placement units (department_members ∪ department_roles; the root when it has none) or an
    ancestor of one. Used by agent authoring offers (union, as 01B.1), agent publication refusal
    (01B.2), Agent Builder name lookups (F8: skill slug `provisioning.rs:576` and tool
    `registered_source_on` treat out-of-reach as not found), admission (D9b), and native MCP
    admission (F4: `native_mcp::admit_on` checks the called tool's connection the same way).
  - *Run reach*: a workflow-level connection is in reach of a run when its unit is in the run's
    base read set (`workflow_run_scopes`, acting_principal_id NULL, access 'read'); at binding
    save, the association's target chain. Used by workflow editor offers (managing department
    chain), binding slot offers and binding save, admission (D9a) and runtime resolution (F2).
- **D9 admission (F3, F9, F12).** (a) every workflow-level connection the admission closure
  resolves (root bundle, children, agent-tool workflows) must be in run reach; (b) for each agent
  in the manifest whose use mode runs tools (Tools mode; ModelOnly uses none), each captured
  connection and skill must have its *current* unit in agent reach, using the agent's current
  placements. A resource moved up stays usable (sharing is moving up); moved out of reach, or the
  agent moved away from it, refuses with `admission.resource_outside_unit`. The captured unit is
  the record of what was published: a captured ≠ current unit is reported (move response, agent
  detail), not refused. Deleted or retired resources keep their existing revocation refusals.
  A two-department agent with resources from both departments runs on any desk and desk-less
  (each resource is checked against the agent's units, not the desk's).
- **F2 unslotted HTTP resolution.** `http::registered_on` takes the reach context and considers
  only enabled connections in run reach, at binding save, admission and every runtime resolution
  (`http_runtime.rs:166,236`, `authority.rs:174-186`); ambiguity is judged within reach, so a
  sibling's identical operation neither substitutes for a disabled in-reach connection nor makes
  in-reach routing ambiguous. Runtime reads the run's base scope rows (child runs copy them).
- **F5 soft-deleted MCP connections.** `company_mcp_connections.unit_id` is NULL exactly when the
  row is deleted: `CHECK ((deleted_at IS NULL) = (unit_id IS NOT NULL))`; `delete_mcp_connection`
  clears it. The FK (MATCH SIMPLE) then never pins a department to a deleted connection. HTTP has
  no delete; skills are hard-deleted.
- **F6 move does not bump revision.** `move_resource_on` changes only `unit_id` (no `revision`,
  `credential_revision` or `updated_at` change), so integration authorization snapshots and
  pending actions are unaffected. Concurrency comes from the FOR UPDATE row lock and re-reading
  the current unit under it; no expected-revision parameter.
- **F7 default unit.** New SQL `resource_default_unit(user, company)`: the root when the person
  manages the root; else the single department they lead when they lead exactly one; else NULL,
  and a create without an explicit unit is refused 400 "Choose a unit for this resource". Library
  agent copies (`prepare_library_agent_on`, channel.rs:644) give copied skills the new agent's
  placement unit (single placement) or the root.
- **D1 teardown (F10).** Company deletion with resources owned by non-root units is exercised;
  if a RESTRICT FK fires during the company cascade, the FKs become `NO ACTION` (still refusing a
  direct department delete at statement end) — decided by that test, recorded with its output.
- **F11 disposition.** Skill and MCP slugs stay unique company-wide; a create conflict reveals only
  that the slug is taken somewhere in the company, as before Plan07. Not changed.
- **D8 codes** unchanged (publication., binding., admission. resource_outside_unit); native MCP
  admission uses the admission code.

## Architecture gate G1 (decision revision r3, 2026-11-20)
Answers 07UR-P01-G1-R2 (F13, F14). r2 stands except where changed here.
- **F13.** The retained r1 mapping rows and notes are corrected in place (marked r2/r3); the
  `registered_on` reach context per caller is in the contract-impact table: pre-run callers
  (binding save, admission preflight, replay) build reach from the association's target chain,
  which equals the run's base read set; callers once the run exists read its base scope rows.
- **F14.** Decision (d) covers the bundle sent to a local machine: `native::selected_with_locks_on`
  checks agent reach for the selected agent's skills and tool connections (current units, agent's
  current placements) before the bundle is selected, refusing with `admission.resource_outside_unit`
  for both conversation-started and workflow native sessions. Machines themselves stay out of
  scope (decision b); only the agent's resources are checked.
- Mapping: the F4 native row also covers a conversation-started native session whose agent skill
  moved out of reach (refused, nothing sent).

### Acceptance mapping additions (r2)
| Criterion | Production path and case | Decisive assertion | Check |
| --- | --- | --- | --- |
| D8 binding save | save a binding whose slot maps to a sibling's connection | `binding.resource_outside_unit`, no binding revision written | `unit_resource_selection_binding_*` |
| D9a closure | root bundle, child bundle and agent-tool workflow each using an out-of-reach connection | admission refused, no run/job rows | `unit_resource_selection_admission_workflow_*` |
| D9b agent reach | agent resource moved up (admitted); moved to sibling (refused); agent moved to another unit (refused); two-department agent on both desks and desk-less (admitted); ModelOnly agent unaffected | outcomes as stated | `unit_resource_selection_admission_agent_*` |
| F2 runtime | sibling enables identical unslotted operation; in-reach connection disabled mid-run | in-reach routing unaffected; no fallback to sibling | `unit_resource_selection_unslotted_*` |
| F4 native | native MCP call with a moved-out connection | `admission.resource_outside_unit`, no native call row | `unit_resource_selection_native_*` |
| refusal settling | schedule run and message intent refused by the new code | schedule refusal row accepted by CHECK; intent reason recorded | `unit_resource_selection_refusal_*` |
| D11 formats | fresh migration: publish agent with tools+skill, prepare and checkpoint a run | SQL validators accept; Rust round-trips | `unit_resource_selection_format_*` |
| F5 / teardown | delete department after its MCP connection was deleted; delete company with unit-owned resources | both succeed | `unit_resource_schema_*` |
| skill-only | agent with only a skill from a moved-out unit | refused at admission | `unit_resource_selection_admission_agent_*` |

### Acceptance mapping (planned; actual tests recorded after implementation)
| Criterion | Production path and case | Decisive assertion | Check |
| --- | --- | --- | --- |
| 01A.1 schema | init schema on a fresh DB | FK rejects foreign/absent unit; library skill with unit and company skill without unit rejected; department delete with an owned resource fails RESTRICT | `unit_resource_schema_*` |
| 01A.1 default (r2 F7) | create MCP/HTTP/skill as owner, root admin, root lead, single-unit lead, two-unit lead, member of a unit who manages the root | root / root / root / lead's unit / 400 "Choose a unit for this resource" without an explicit unit / root; explicit unmanaged unit → 404 | `unit_resource_schema_default_*` |
| D2 authority | each write path as owner, root admin, root lead, unit lead (own, child, sibling), member | managed → success; unmanaged → 404 and no row change; level above own → 403 | `unit_resource_authority_*` |
| 01A.3 / D5 | delete a department owning a skill and a connection | Conflict naming them; delete succeeds after move | `unit_resource_schema_delete_*` |
| 01B.4 move (r2 F6) | move as manager of both / of one | moved; revision, credential_revision and updated_at unchanged; snapshots unchanged; affected uses listed; one side unmanaged → 404 | `unit_resource_move_*` |
| 01B.1 offers | root, child, sibling authoring; agent in two units | only in-reach resources listed; union for two units; moved-up resource listed below | `unit_resource_selection_offers_*` |
| 01B.2 publication | agent publish with out-of-reach skill/connection; workflow publish selecting one | `publication.resource_outside_unit` naming step/resource | `unit_resource_selection_publication_*` |
| 01B.3 admission (r2 F3) | run after a move out of reach, and after a move up | out of reach → `admission.resource_outside_unit`, no run/job rows; moved up → admitted without republish | `unit_resource_selection_admission_*` |
| D10 race | move vs agent publication, both orders | refused publication, or captured old unit refused at admission | `unit_resource_competing_*` |

### Contract impact (planned)
| Changed contract | Producers / consumers | Disposition | Checks |
| --- | --- | --- | --- |
| unit_id columns + FKs | create paths (skill insert/copy, MCP create, HTTP create, library-agent copy channel.rs:644, provisioning slug lookup) ; department delete | all writers set unit; delete guard lists owners | schema, delete |
| resource write authority | MCP/HTTP/skill/Vault/discovery use cases, persistence, UI+JSON routes | unit management per resource | authority |
| agent revision content | capture.rs, agent_revisions SQL checks, preparation history, agent_authority | captured unit added on ResolvedAgentTool/SkillSnapshot; readers updated; formats bumped | selection, competing |
| `http::registered_on` reach (r3 F13) | binding save (association target chain); admission preflight `admission.rs:74` and replay (desk/target department chain via `workflow_target_departments`, equal to the base read set, since scope rows do not exist yet); admission closure in `admission_write::create` and every runtime caller `http_runtime.rs:166,236`, `authority.rs:174-186` (the run's base scope rows) | one `Reach` value (unit ids) built from the chain or from scope rows | selection unslotted, admission |
| native bundle selection (r3 F14) | `native::selected_with_locks_on` (conversation-started and workflow native sessions) | agent reach check on the bundle's skills and tool connections before it is sent | selection native |
| admission refusal | admission_write, message_admission classify, schedule refusal CHECK, HTTP mapping, agent publication failure class | new variant and codes | admission |

## B1 implementation (D1–D5 with G1 r2 F5–F7)
- Schema (init migration): `skills.unit_id` + `skills_unit_check ((company_id IS NULL) = (unit_id
  IS NULL))`; `company_mcp_connections.unit_id` + `company_mcp_connections_unit_check ((deleted_at
  IS NULL) = (unit_id IS NOT NULL))`; `workflow_http_connections.unit_id NOT NULL`; FKs
  `*_unit_fk (company_id, unit_id) → departments(company_id, id) ON DELETE RESTRICT` and
  `(company_id, unit_id)` indexes beside `channels_department_fk`; SQL `resource_default_unit(user,
  company)` (root for a root manager, else the single led unit, else NULL). The company-teardown
  test passes with RESTRICT, so the r2 NO ACTION fallback was not needed.
- Port: `AdministrationPersistence` gains `managed_unit_choices` (names, root first) and
  `default_resource_unit`; `resource_creation_unit` (AnyUnit entry → explicit or default unit →
  400 "Choose a unit for this resource." → Unit management, 404). New `ports::unit_resources`
  (`UnitResourceKind`, `MovedResource`, `ResourceUse{Agent,Binding}`, at most 20 named).
- Persistence: `unit_resources::move_resource_on` (person/company locks, row FOR UPDATE, both
  units managed, only `unit_id` changes, uses = active agent revisions via
  agent_revision_skills/agent_revision_bindings+capability bindings and active bindings whose
  current configuration maps a slot to the connection); `authorize_resource_on` (person locks +
  management of the resource's unit; refusal = the resource's 404); `owned_resources_on` for the
  delete guard.
- Skills: entity/summary/rows carry `unit_id`; create/copy take the unit; update/delete carry the
  authorized unit as a SQL predicate; lists/counts filter `unit_id = ANY(managed units)`; use case
  `verify_skill_manager` (AnyUnit) replaces owner-only `verify_company_owner`; per-skill
  `managed_skill`; `unit_choices`, `move_company`; library-agent copies put skills in the agent's
  placement or the root (company_agent.rs, onboarding.rs → root).
- MCP: entity `unit_id: Option` (None only when deleted); create takes unit; update/delete/token
  check the authorized unit under the company lock (deleted rows clear the unit); list filtered;
  `set_mcp_level`, discovery refresh (`mcp/discovery.rs`), tool operations
  (`workflow/mcp.rs edit_on`) and Vault propose/verify/inspect (`integration_bindings.rs`,
  `integration_bindings/source.rs`) use the connection's unit; `level_actor` needs AnyUnit.
- HTTP: entity/rows carry `unit_id`; `lock_edit_on` = person locks → row lock → unit management →
  revision; list = AnyUnit + `unit_management_role(user, company, unit_id)`; create resolves the
  unit in its transaction; `move_http`. UI detail/save use the MCP use case's level actor and unit
  choices (shared connection ceiling).
- Routes/UI: create takes `unit_id` (JSON skill/MCP/HTTP; UI skill and MCP forms with a unit
  picker and default; skill library copy picker); move: JSON `POST …/{skills|mcp-connections|
  http-connections}/{id}/move {unit_id}` → `{from_unit_id, unit_id, uses, more_uses}`, UI move
  forms on skill/MCP/HTTP detail with a moved notice naming remaining uses. Skill ZIP import uses
  the default unit (a two-unit lead imports after choosing via create or move).
- Department delete: `guard_department_delete_on` lists owned skills/connections (≤10 + count)
  after the existing child/desk checks.
- Unchanged by decision: run-time action authority (`http_for_action`, `mcp_use` keep the frozen
  actor's company check); credential key rotation; library operator paths.
- MCP level save re-render (`connection_levels.rs mcp_save`) shows the move form as the detail
  page does.

### B1 checks
- Fresh init schema on openbot_p07_schema @55407: applies (psql ON_ERROR_STOP).
- Existing focused suites (filters skill department connection_level mcp http_connection
  integration_binding vault), DATABASE_URL openbot_p07_b1 (+ `_test`): 674 passed, 0 failed
  (scratchpad p07-b1-focused-3.txt; the earlier 150 failures were only the missing `_test`
  database). Expected behavior change updated: `skill_bundle_http_members_cannot_upload_and_root_
  admins_manage_root_skills` (root admin now imports root skills, decision a). Raw-SQL fixtures in
  7 test files gained the root unit.
- New: `unit_resource_schema_keys_checks_and_default_unit`, `unit_resource_authority_each_write_
  follows_the_resource_unit`, `unit_resource_move_needs_both_units_and_changes_only_the_unit`,
  `unit_resource_schema_delete_names_what_to_move_and_teardown_keeps_working`,
  `unit_resource_move_lists_the_agents_whose_active_revision_uses_a_skill`: 5 passed.
- Broad lib suite (no filter, Node default 26.13.1): 4882 passed, 7 failed, 30 ignored
  (logs/B1-lib-suite-summary.txt). Rerun of the 7 with Node 24 on PATH (as check.sh's CI-like
  environment has): the 6 `cloudroom_*` local-agent e2e tests pass (their spawned counted peer
  exits early under Node 26: environment, not B1); `doop_agent_current_preparation_reaches_ready_
  before_model_io` fails 8 vs 7, the deferred Doop assertion also red on main
  (logs/B1-lib-rerun-node24.txt).
- Clippy `--all-targets -D warnings`: PASS after moving the test helper before the tests module
  (logs/B1-clippy.txt). `cargo sqlx prepare --check -- --all-targets` on fresh migrated
  openbot_p07_sqlx: PASS, `.sqlx` unchanged (logs/B1-sqlx-check.txt). Formatting and patch
  whitespace clean.
- After the last two small edits (helper move, MCP level re-render move form): connection_level,
  unit_resource, mcp_api, ui_mcp filters 16 passed (logs/B1-focused-after-fix.txt).

## B1-R1 corrections (for recheck 07UR-P01-B1-R2)
- F1: the company Skills tab is editable for anyone managing a unit
  (`SkillUseCases::manages_skills`; ui_companies.rs uses it; DB errors still propagate); the
  read-only text reads "Only a department's managers can edit its skills."
- F2: agent authoring reads skills through `SkillUseCases::list_authoring_page` (AnyUnit entry,
  every company skill, as Plan06 U3 and as the unchanged MCP/HTTP agent offers); the configuration
  list keeps managed units only. B2 narrows authoring to agent reach (D7).
- F3 tests (all pass): `unit_resource_authority_vault_binding_needs_the_connection_unit` (sibling
  propose and source inspect 404; proposer's later loss of the unit refuses confirm, no binding),
  `unit_resource_authority_http_operation_register_needs_the_connection_unit`,
  `unit_resource_authority_mcp_operation_register_needs_the_connection_unit` (also: a move keeps
  catalog revision and approval version), `unit_resource_authority_root_lead_level_ceiling_
  refresh_and_placed_defaults` (root lead configures root-owned skill/MCP/HTTP; a managing lead
  setting L3 above their level is refused 400 — the existing `may_assign_level` mapping, unchanged;
  sibling refresh 404 before any connect; owner placed in a unit and an admin leading one default
  to the root), `unit_resource_library_agent_copies_skills_into_its_placement_or_the_root`,
  `unit_resource_move_lists_active_bindings_of_the_current_configuration` (earlier
  configurations do not count), `unit_resource_skills_tab_and_agent_authoring_reads` (F1/F2).
  Onboarding's library copy passes the root to the same tested copy function (inspection only; no
  onboarding fixture copies library agents).
- F5 (optional, applied): the move response names only uses the mover manages
  (`agent_management_role` / `workflow_definition_management_role`), counting the rest in
  `more_uses`; test `unit_resource_move_counts_but_does_not_name_unmanaged_uses`.
- F4 (optional, not applied): MCP and skill writes keep their pre-B1 shape of checking management
  before the write transaction; a concurrent move is still caught by the unit predicate.
- Test helpers: `PostgresPersistence::test_unit` / `test_unit_lead` (cfg(test)).
- R2 checks: unit_resource 13 passed; affected suites (skill department connection_level mcp
  http_connection integration_binding vault ui_agents agent_authoring binding_tests http_tests
  ui_companies) 975 passed, 0 failed; clippy PASS after removing a `clone` on a Copy fixture.
  Frozen at c8c0b6d3b7f4634542ffc65305506e2e6d26f1ecaf163029e6156bf354e08cfd
  (logs/B1-R2-author-freeze.snapshot.txt); manifest logs/07UR-P01-B1-R2.evidence.json (5 PASS).

## B1-R2 corrections (for recheck 07UR-P01-B1-R3)
- F1: `skills_pane(access, editable)` keeps the company summary counts and settings controls the
  owner's (as on the Settings tab), so a unit manager's Skills tab no longer reaches the
  owner/admin-only channel list; the skills section follows `manages_skills`. Route test
  `unit_resource_skills_tab_renders_editable_for_unit_managers_and_read_only_for_members`
  (owner, root admin, both unit leads: editable section with the unit picker; plain member: the
  read-only text, no section) drives `companies_page` with the Fixture delegation.
- F6: new `may_assign_connection_level(actor, current, requested)` (owner unbounded; otherwise
  current and requested at most the actor's own level); `set_mcp_level`/`set_http_level` use it
  after their unit check and refuse with 403 Forbidden; the level form uses the same rule; the
  connection UI feedback shows the 403 message. Team access levels keep `may_assign_level`
  (owner and root admins only). Tests: domain
  `access_level_connection_assignment_is_bounded_by_the_unit_managers_own_level`; the Sales lead at
  level 2 sets MCP and HTTP levels to 2 and is refused 403 at 3 (in
  `unit_resource_authority_root_lead_level_ceiling_refresh_and_placed_defaults`); the existing race
  `knowledge_run_level_race_live_admin_demotion_precedes_connection_level_write` now expects 403.
- F3 remainder: the owner places a library agent into Sales (owner default = root) and the copied
  skill is in Sales.
- Checks: unit_resource, connection_level, ui_companies, ui_mcp, access_level, mcp_api,
  http_connection: 82 passed; knowledge_http, knowledge_mcp, knowledge_e2e_mcp,
  local_agent_mcp_history, native_mcp_native, ui_agents, skill, department: 304 passed; clippy
  PASS; whitespace clean.

## Status
- G1 r1 frozen at digest c2d338160c48d4d2eabaa84b5bd5fa5cd389b58d2a93db85478eb2e05fb9ac33 (HEAD
  cc545881; exclusions as above). B (Opus 5.5, model opus / effort xhigh as for Plan06's reviewer)
  agent a42fa3c1931ec8e75 reviewed G1 r1: changes required (07UR-P01-G1-R1, F1–F10 actionable,
  F11–F12 optional). User decision (d) taken on F9. G1 r2 written (same digest: records only);
  recheck 07UR-P01-G1-R2: F1–F12 resolved/dispositioned, new low F13, F14. G1 r3 written.
- **G1 r3 PASS** (07UR-P01-G1-R3, digest c2d33816…, coordination r2). Optional F15 adopted for
  B2: `agent_manifest/capture.rs:41` and `channel_reply.rs:314` run before scope rows exist and
  use the desk's department chain; B2 review confirms the native maintenance sweep still retires
  in-flight Status/Cancel after a reach loss.
- B1 (D1–D5 with r2 F5–F7) implemented; frozen at digest
  2c55a5762e07d9e7f2a7d6aaf9a7fffe06571ab1351f8c666a97f9a8a3dad81a (base/HEAD cc545881 + working
  tree; exclusions unchanged; logs/B1-author-freeze.snapshot.txt); manifest
  logs/07UR-P01-B1-R1.evidence.json validated (6 PASS, 2 FAIL = the broad suite and its rerun,
  dispositioned above). B1 review 07UR-P01-B1-R1 requested.
- **B1 accepted in phase** 2026-11-20: B PASS 07UR-P01-B1-R3 at
  2849d92a62feef47d12e7f73961195e804ef247edbe9801adb55770106493b42 (rounds B1-R1 changes required
  F1–F5, B1-R2 F1/F6/F3 open, B1-R3 PASS; F4 optional not applied, accepted non-blocking); root
  re-digest identical (logs/B1-root-acceptance.snapshot.txt); R3 manifest validates; coordination r2
  unchanged. trail check: 2 keyword matches on init-schema `resource_default_unit` (teardown
  ordering, company-wide admission learnings) — not those mechanisms; reason in the commit.
  Committed locally (not pushed). Reviewer note for B2: run-time action authority and
  `ResourceDirectory::inspect` still require root management (pre-B1).
- Implementation: B2 (D6–D11) next.
