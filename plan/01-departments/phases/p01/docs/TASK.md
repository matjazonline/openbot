# p01 Departments, membership and roles — TASK

## Identity

- Plan / phase: `plan/01-departments/`, phase `p01` (document `01-departments-and-membership.md`)
- Workflow: `implement-opus-reviewed-2_2` (skill dir `/Users/mac02/.claude/skills/implement-opus-reviewed-2_2`)
- Record root: `/Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/phases/p01/docs/`
- Root = Author (A): session 68933786-cbe7-4b6b-9387-b5dcb29689a4, Opus 5.5 / xhigh (user confirmed
  2026-11-09; global settings `modelSettings.claude-opus-5-5.effortLevel = xhigh`)
- Reviewer (B): Opus 5.5 / xhigh general-purpose subagent, agent ID ad409b2d3d149c2c6 (spawned 2026-11-09 for gate G1)
- Worktree: `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`
- Phase starting commit: `3752ea3540e3a2bf6d17178bf887fe2e99a384ac` (clean tree; no pre-existing changes)
- Snapshot exclusions (exact, reused by A, B, root):
  `plan/01-departments/phases/p01/docs` `plan/01-departments/COORDINATION.md`
- Coordination: `/Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/COORDINATION.md`,
  revision 1 checked 2026-11-09. Applicable: 01DEP-I001/r1, I002/r1, I003/r1 (acknowledged). No notices.
- Authorizations: local commit of this phase once accepted (user, 2026-11-09). No push.
- Context: `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75` effective in this session (printenv).

## Requirements (original sources)

- `plan/01-departments/README.md` (decisions 1–12, "What does not change")
- `plan/01-departments/01-departments-and-membership.md` (Batches 01A, 01B, focused checks)
- `AGENTS.md`, `src/AGENTS.md`, `src/application/AGENTS.md`, `src/adapters/persistence/AGENTS.md`,
  `src/adapters/http/AGENTS.md`, `src/infra/AGENTS.md`

Later phases depend on this one: p02 calls `sync_department_grants_on` inside every membership/role
change and re-parent; p03 calls `create_department_on`, `assign_role_on`, `add_member_on` inside
other transactions. The `_on` contracts below are therefore stable contracts.

## Acceptance criteria

- AC1 Schema: `departments`, `department_members`, `department_roles` with the constraints in 01A,
  composite tenant FKs, `guard_department_shape` (cycle, depth > 8), `guard_department_principal_kind`
  (external/system refused), one lead per department, role string restricted to `lead`.
- AC2 Provenance: `GrantProvenance::DepartmentMembership` (`department_membership`) in Rust and in
  `channel_principal_grants_provenance_check`; enum tests enumerate every variant.
- AC3 Domain: `Department`, `DepartmentId`, `DepartmentSlug`, `DepartmentRole::Lead` (`as_str`/`FromStr`),
  `DepartmentRoleHolder`, `DepartmentWrite::normalize`.
- AC4 Persistence: `DepartmentPersistence` port + Postgres adapter; `_on` forms on the caller's
  transaction; every write takes the company write lock first then the department row `FOR UPDATE`;
  cycle/depth preflight → `BadRequest` naming the ancestor; taken slug → `Conflict`; lead replace in
  one transaction.
- AC5 Use cases: `DepartmentUseCases` wired beside `ChannelUseCases`; every method starts with
  `managed_company`; `visible_departments(user, company)` for plain members.
- AC6 JSON API: list/create, get/patch/delete, member put/delete, role put/delete; authorization
  tests mirroring `channel_authorization_tests.rs`.
- AC7 Focused checks `department_schema_`, `department_write_`, `department_competing_` and unit tests
  as listed in the plan, each filter with a nonzero count.

## Architecture gate — decisions (revision r2, 2026-11-09)

Gate needed: new schema and FK/cascade behaviour, a new per-company write/concurrency protocol that
p02/p03 extend, and the authorization model for a new resource. r1 text is preserved verbatim in
`logs/gate-decisions-r1.md`; r2 answers `01DEP-P01-G1-F1..F8` (see "r2 changes" per decision).

### DEP-P01-D1 Schema placement and cascade semantics (r2)

- The three tables, their keys, FKs, indexes, the shape function and the triggers go in **one
  self-contained block appended to the end** of `migrations/20260817000000_init_schema.sql`
  ("Departments (01-departments p01)"), not split into the dump's table/constraint/FK sections.
  Reason: the composite FKs need `principals_company_id_id_key` and `departments(company_id,id)`,
  which the dump declares after its tables; recent features (e.g. "Channel response ownership")
  use appended blocks. `channel_principal_grants_provenance_check` is edited in place (line ~890).
- `departments`: `id uuid PK`, `company_id uuid NOT NULL → companies(id) ON DELETE CASCADE`,
  `name text NOT NULL` (`departments_name_not_blank`), `slug citext NOT NULL`
  (`departments_slug_format`, same regex as `agents_slug_format`/`channel_slugs_format`),
  `description text NULL`, `charter text NULL` (`departments_charter_size`: `octet_length <= 4096`),
  `parent_id uuid NULL`, `created_by jsonb NOT NULL` (`valid_creation_provenance`),
  `created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP`; `UNIQUE (company_id, slug)`,
  `UNIQUE (company_id, id)`; `departments_parent_fk (company_id, parent_id) → departments(company_id, id)`
  with the default **NO ACTION** (deleting a parent that keeps children is refused at statement
  end; a company teardown removes the whole tree). *r2: the r1 claim that RESTRICT would break
  teardown is withdrawn (F6, reviewer probe on PG 18.6); NO ACTION is simply the default and p02
  keeps its plan's RESTRICT for `channels.department_id`.* `departments_not_own_parent`.
- `department_members (company_id, department_id, principal_id, created_at)`,
  PK `(department_id, principal_id)`; `(company_id, department_id) → departments(company_id,id)
  ON DELETE CASCADE`; `(company_id, principal_id) → principals(company_id,id) ON DELETE CASCADE`.
- `department_roles (company_id, department_id, role, principal_id, created_at)`,
  PK `(department_id, role, principal_id)`, same two FKs, `department_roles_role_check
  CHECK (role = 'lead')`, partial unique index `department_roles_one_lead ON (department_id, role)
  WHERE role = 'lead'` (literal deliberately scoped: only `lead` is single-holder).
- Secondary indexes for the reverse reads: `department_members (company_id, principal_id)`,
  `department_roles (company_id, principal_id)`, `departments (company_id, parent_id)`.
- **r2 (F1): a principal leaves departments when it stops being a person or agent.** Removing a
  teammate (`company_invite.rs` `remove_member`), deleting an agent (`agent.rs` `delete`) and
  deleting an account (`user_lifecycle.rs`) never delete the principal; they demote it to
  `kind = 'external'` with its user/agent link cleared. The principal-side CASCADE only covers
  company teardown. New trigger `department_principal_demoted`:
  `AFTER UPDATE OF kind ON principals FOR EACH ROW WHEN (NEW.kind NOT IN ('person','agent'))`
  deletes that principal's `department_members` and `department_roles` rows (company-scoped). It
  runs inside whichever transaction demotes, whatever locks that path holds, so it does not rely
  on the company write lock (agent delete and account deletion do not take it). Losing the lead
  this way leaves the department without a lead. **p02 contract:** p02 extends this same trigger
  to delete the demoted principal's `department_membership` grants (they are derived from rows
  this trigger removes; leaving them would keep access for an address the demoted principal may
  still be resolved from), with p02's own revision/lock rules recorded in its gate.

### DEP-P01-D2 Shape rule and principal kind (r2)

- `public.department_shape_problem(checked_company uuid, checked_department uuid,
  proposed_parent uuid) RETURNS text` (STABLE): `NULL` when acceptable, `'cycle'` when
  `proposed_parent = checked_department` or the upward walk from `proposed_parent` reaches
  `checked_department`, `'depth'` when `ancestor_depth(proposed_parent) + subtree_height(checked_department) > 8`
  (a root department is level 1; a chain of 8 is allowed, 9 refused). Both walks are recursive CTEs
  bounded at 9 steps so corrupt data cannot loop. The downward walk is required because
  re-parenting a node with children deepens its whole subtree.
- Trigger `guard_department_shape` (`BEFORE INSERT OR UPDATE OF parent_id`) calls it and raises
  `23514` with constraint name `departments_shape`. Persistence preflights with the same function
  inside the locked transaction and returns `BadRequest` naming the department and the parent.
- `guard_department_principal_kind` (`BEFORE INSERT OR UPDATE` on both join tables) refuses a
  principal whose kind is not `person`/`agent` (`23514`, constraint `department_principal_kind`).
  **r2 (F1 race): it reads the principal `FOR KEY SHARE`**, and so does the persistence preflight.
  `kind` is part of `principals_company_id_id_kind_key`, so a demotion is a key update holding the
  principal `FOR UPDATE`; `FOR KEY SHARE` conflicts with it. Either the join-row writer waits for
  the demotion and then sees `external` (refused), or the demotion waits for the writer's commit
  and its trigger (fresh statement snapshot) deletes the committed row. Neither order leaves a
  department row on a non-person/agent principal.

### DEP-P01-D3 Write protocol and lock order (r2)

- Every department write runs in one transaction with `lock_timeout`/`statement_timeout` bounds
  (`agent_publication::begin_bounded`), then **`agent_publication::lock_company_on`** (r2, F7):
  the actor's `users` row `FOR SHARE`, then `workflow::authority::authorize_company` —
  `companies FOR NO KEY UPDATE`, then the actor's member/principal `FOR SHARE`, refusing anyone but
  an owner/admin with `NotFound`. This is the account → company order `remove_member` and agent
  publication use, so p03 can nest agent publication inside a department write or vice versa.
  The `NotFound` it returns is re-worded to `company_not_found()` (F8).
- **Plan-wide lock order (r2, F2), binding on p02/p03:** account row → company row
  (`FOR NO KEY UPDATE`) → department rows (`FOR UPDATE`, a named parent `FOR SHARE`) → principal
  rows (`FOR KEY SHARE`, D2) → the affected desks' channel gates, all in **one**
  `acquire_channel_gates_on` call (sorted) → channel rows. Every writer of department-derived
  grants takes the company write lock **before** any channel gate or channel row. In particular
  p02's channel-settings edit that changes `department_id` or `members_only` must take the
  company lock before its gate (today's `update_channel_on` takes gate → row with no company
  lock); a department writer re-deriving grants computes its full desk set first — the
  department's members-only desks, and when its lead changes, also its child departments'
  members-only desks (decision 7) — and acquires those gates together. The demotion trigger (D1)
  is the one writer outside this order; p02 records how its grant deletion composes.
- What each lock is relied on for: the **company lock** serializes every department writer of the
  company, which is what makes the shape preflight, the slug check and lead replacement
  race-free. The **department row `FOR UPDATE`** is the in-tenant existence check taken under that
  lock, and it orders department writes against writers that may hold the company lock only
  through a nested path (p03); it is not relied on for tree consistency. The **parent
  `FOR SHARE`** keeps a named parent from being deleted under a create/re-parent even by a
  writer that skipped the company lock (only raw SQL can).
- `_on` contract (stable for p02/p03): `pub(crate)` adapter functions in
  `src/adapters/persistence/department.rs` taking `&mut Transaction<'_, Postgres>` whose caller has
  already taken the company write lock (`lock_company_on` or `authorize_company`):
  `create_department_on`, `update_department_on`, `delete_department_on`,
  `assign_department_role_on`, `clear_department_role_on`, `add_department_member_on`,
  `remove_department_member_on`. Each locks its department row itself. The application port cannot
  name sqlx (src/AGENTS.md "Preserve dependency direction"), so the `DepartmentPersistence` trait
  exposes whole-transaction methods that wrap these. No lock-witness type (F9, optional, declined:
  the `_on` callers are few and each sits behind `lock_company_on`/`assistant_channel_on`, which
  already take the lock; a witness would have to thread through p03's existing provisioning API).
- Lead semantics: `assign_role(Lead, p)` replaces any current holder; assigning the current holder
  again is a no-op success. `clear_role(Lead, p)` deletes that holder's row; clearing someone who
  does not hold it is `NotFound`. `create(.., lead: Option<PrincipalId>)` assigns in the same
  transaction. A role holder need not be a member (decision 5).
- Add member twice: idempotent success (`ON CONFLICT DO NOTHING`); remove a non-member: `NotFound`.
- Delete: refused with `Conflict` when the department has child departments (and, from p02, owned
  desks); members and roles cascade.

### DEP-P01-D4 Authorization model (r2)

- `DepartmentUseCases` writes and manager reads start with `managed_company`; its `Forbidden`
  for a plain member is converted to the same `NotFound` a stranger gets, as the plan's focused
  check requires ("a member or non-team user gets `NotFound` on every write, as the schedule tests
  assert"). Persistence re-authorizes under the lock (D3). **r2 (F8): this deliberately differs
  from `channel_authorization_tests.rs`' `denial_cases()` (member → 403); the department route
  tests assert 404 for the member and do not reuse `denial_cases()`.**
- **r2 (F4) reads:** `visible_departments(user, company)` and `visible_department(user, company,
  id)` need only company membership (`readable_company`). Owners and admins see every department;
  a plain member sees departments where their `person` principal is a member or holds a role,
  with those departments' members and role holders. A department the member cannot see is
  `NotFound`; strangers get `NotFound`. `list_departments` (manager-only) remains for the phase 04
  settings pane.
- JSON reads open to a plain member: `GET /api/companies/{id}/departments` (their departments) and
  `GET .../departments/{dep}` (only one they can see). Every write route (POST, PATCH, DELETE, and
  member/role PUT/DELETE) returns 404 to a member, a stranger and for a foreign company.
- Department ids and slugs from another company are indistinguishable from missing ones.

### DEP-P01-D5 Types and placement (r2)

- `src/domain/entities/department.rs`: `DepartmentId` (uuid newtype, serde transparent),
  `Department`, `DepartmentRole::Lead` with `as_str`/`FromStr`/serde `snake_case`,
  `DepartmentRoleHolder { role, principal_id }`, `HeldDepartmentRole { department_id, role }`,
  `DepartmentMember { principal_id, created_at }`.
- `DepartmentSlug` via `string_newtype!` in `value_objects.rs` beside `ChannelSlug`.
- `DepartmentWrite` (create) and **r2 (F3): `DepartmentEdit` (update)** live in
  `src/application/ports/department.rs` beside the port, like `ChannelWrite`; their `normalize()`
  lives in `use_cases/department.rs` like `ChannelWrite::normalize`. `DepartmentEdit` has
  `Option` fields (`name`, `slug`) and tri-state `Option<Option<_>>` fields (`description`,
  `charter`, `parent_id`: absent = unchanged, `null` = clear / make root). Persistence merges the
  edit onto the row **after** taking its `FOR UPDATE` lock, so two concurrent PATCHes of different
  fields cannot lose either. Normalization: trimmed name, lowercased/hyphenated slug through
  `validate_slug(.., SlugKind::DepartmentAddress)` plus a Rust format check (so a malformed slug is
  `BadRequest`), blank description/charter → `None`, charter over 4 KiB refused.
- Taken slug: `AppError::Conflict("Department address '<slug>' is already in use in this company.")`.
  The plan's "reuse `slug_conflict_error`" is not followed literally: that helper returns
  `BadRequest` and names a channel. The p03 department desk uses the channel slug namespace and
  keeps `slug_conflict_error` there.
- JSON API under the existing JSON prefix `/api/companies/{company_id}/departments...` (the
  plan's `/companies/{id}/departments` path is the HTML convention; phase 04's UI routes take it).
  Create body `{ name, slug?, description?, charter?, parent_id?, lead? }` (slug derived from the
  name when omitted, as for channels). **PATCH body: only present fields change** (`DepartmentEdit`).
  `GET` one returns the department with its `members` and `roles`. Role path segment parsed with
  `DepartmentRole::from_str` (unknown → 400).

## Verification plan (mapping filled with actual names after implementation)

| Criterion | Production path and case | Decisive assertion | Check | Status |
| --- | --- | --- | --- | --- |
| AC1 cycle | trigger on direct UPDATE (bypassing preflight) | `23514`, constraint `departments_shape` | `department_schema_*` | planned |
| AC1 depth 9 | trigger on INSERT of 9th level; re-parent of a 7-deep subtree under level 2 (refused) and under a root (accepted, exactly 8) | `departments_shape` / success | `department_schema_*`, `department_write_*` | planned |
| AC1 cross-company parent / role holder | raw INSERT | FK violation names composite FK | `department_schema_*` | planned |
| AC1 external lead/member | raw INSERT | `department_principal_kind` | `department_schema_*` | planned |
| AC1 second lead, unknown role | raw INSERT | unique index / role check | `department_schema_*` | planned |
| AC1 cascade | DELETE department; company delete with nested tree | members/roles gone; teardown succeeds | `department_schema_*` | planned |
| AC4/AC5 round trip | use cases on Postgres | create/rename/re-parent/lead assign-replace-clear/members | `department_write_*` | planned |
| AC4 conflict | create/rename to taken slug | `AppError::Conflict` | `department_write_*` | planned |
| AC4 preflight | re-parent into own descendant | `BadRequest` naming ancestor | `department_write_*` | planned |
| AC5 authz | member and stranger on every write | `AppError::NotFound` | `department_write_*` | planned |
| AC5 visibility | member sees only theirs; admin/owner all | exact id sets | `department_write_*` | planned |
| AC4 competing | two re-parents forming a cycle; same-slug creates; two lead assigns — lined up behind a held company row on a third connection, released together (`own_database`) | re-parents: one Ok + one BadRequest, one parent set; creates: one Ok + one Conflict, one row; leads: both Ok, one holder | `department_competing_*` | planned |
| AC2/AC3 units | `DepartmentWrite::normalize`, `DepartmentRole`, `GrantProvenance` | every variant round-trips | unit tests | planned |
| AC6 routes | every write handler for member/stranger/foreign; list+detail for member | 404 on writes; member list shows only theirs; detail of an unseen one 404 | `department_routes_*` | planned |
| D1 demotion | raw `UPDATE principals SET kind='external'`; `remove_member` and agent `delete` production paths | member/lead rows gone | `department_schema_*`, `department_write_*` | planned |
| D2 kind race | add-member waiting behind a held demotion | `BadRequest`, no row | `department_competing_*` | planned |

Not proved by these: p02 grant re-derivation and p03 desk provisioning (later phases).

## Contract impact (planned)

| Changed contract/value | Producers / consumers | Disposition | Checks |
| --- | --- | --- | --- |
| `channel_principal_grants.provenance` gains `department_membership` | `GrantProvenance` enum + `FromStr`; readers decoding grants (`participant` persistence) | enum/match updated; no writer in p01 | provenance unit test |
| new `DepartmentPersistence` port | `PostgresPersistence` impl; `DepartmentUseCases`; AppState `FromRef` | new | write/route tests |
| `SlugKind` gains `DepartmentAddress` | `validate_slug` noun match | exhaustive match | normalize unit test |
| `AppState` gains `department_use_cases` | test AppState builders | compiler-driven | build |

## Batches

- **B1 (whole phase 01: 01A + 01B)** — schema, domain, persistence, use cases, JSON API, tests.
  Focused checks: `department_schema_`, `department_write_`, `department_competing_`, unit tests for
  normalize/role/provenance, the department route tests, plus affected existing tests
  (`grant_provenance`/participant enum tests, `postgres_channel_persistence_works` sanity).

## Resources

- Test database: `TEST_DATABASE_URL=postgres://mac02@localhost:5432/openbot_dept_p01_test`
  (phase-owned; created 2026-11-09). `DATABASE_URL` for `sqlx prepare`: a phase-owned fresh DB
  `openbot_dept_p01_dev` (created when needed). Never the shared `open_bots` / `open_bots_test`.
- Build: worktree `target/` (no shared `CARGO_TARGET_DIR`). Test threads 4 (machine limit).

## Status

- 2026-11-09: records established; architecture gate r1 written, awaiting B.
- 2026-11-09: G1-R1 changes required (F1–F5 actionable, F6–F9 optional). r2 written answering all:
  F1 demotion trigger + FOR KEY SHARE kind reads; F2 plan-wide lock order; F3 DepartmentEdit;
  F4 member reads; F5 test assertions; F6 rationale withdrawn; F7 lock_company_on; F8 404 wording;
  F9 declined with reason. Awaiting B recheck of r2.
- 2026-11-09: G1-R2 PASS for r2 (01DEP-P01-G1-R2). B1 implemented, focused checks pass; frozen for
  batch review B1-R1.
- 2026-11-09: B1-R1 changes required (B1-F1 Low); corrected; B1-R2 **PASS** for digest
  ccc4ee26be865cf9806a1e192f05bdddb8753eb4993704e2b8f98fcc78f03880. Root recomputed the digest,
  confirmed coordination rev 1 unchanged (no notices), validated the R2 manifest and the saved
  notes/findings: **accepted in phase** (not integrated with p02/p03). Committed locally.
  Carry-over (trivial, B1-F4 leftover): `DepartmentWrite` struct doc says "create or edit" — fix
  in p02. Full repository gate not run (I003).

## B1 implementation (2026-11-09)

Files: `migrations/20260817000000_init_schema.sql` (provenance check in place; appended
"Departments" block: 3 tables, `department_shape_problem`, `guard_department_shape`,
`guard_department_principal_kind` with `FOR KEY SHARE`, `department_principal_demoted`),
`src/domain/entities/{department.rs, participant.rs, value_objects.rs, mod.rs}`,
`src/application/ports/{department.rs, mod.rs}`, `src/application/use_cases/{department.rs,
department_tests.rs, channel.rs (SlugKind::DepartmentAddress), mod.rs}`,
`src/adapters/persistence/{department.rs, department_tests.rs, mod.rs}`,
`src/adapters/http/routes/{department.rs, department_tests.rs, mod.rs}`,
`src/adapters/http/app_state.rs`, `src/infra/setup.rs`, `src/infra/setup/accounts.rs`.
Every `_on` function that writes a join row for a principal (`assign_department_role_on`,
`add_department_member_on`, and `create_department_on` through assign) reads that principal
`FOR KEY SHARE` first (`check_principal_on`), per 01DEP-P01-G1-F10. `clear_*`/`remove_*` only delete.

## Verification mapping (actual, B1)

| Criterion | Test (lib) | Decisive assertion | Status |
| --- | --- | --- | --- |
| AC1 cycle / own parent (trigger, bypassing preflight) | `department_schema_refuses_a_cycle_and_its_own_parent` | constraint `departments_shape` on raw UPDATE | PASS |
| AC1 depth 9; 7-deep subtree under level 2 refused, under root accepted | `department_schema_refuses_a_ninth_level_and_a_subtree_moved_too_deep` | `departments_shape`; success at exactly 8 | PASS |
| AC1 cross-company parent / lead / member / department | `department_schema_refuses_cross_company_parents_and_principals` | `departments_parent_fk`, `department_roles_principal_fk`, `department_members_principal_fk`, `department_members_department_fk` | PASS |
| AC1 external & system lead/member refused; agent accepted | `department_schema_refuses_external_and_system_principals` | `department_principal_kind` | PASS |
| AC1 second lead, unknown role | `department_schema_refuses_a_second_lead_and_an_unknown_role` | `department_roles_one_lead`, `department_roles_role_check` | PASS |
| AC1 cascade; parent with child refused | `department_schema_cascades_members_and_roles_and_keeps_children` | `departments_parent_fk`; counts (0,0) | PASS |
| D1 company teardown with nested tree | `department_schema_tears_down_a_nested_tree_with_its_company` | delete succeeds, 0 departments (rolled back) | PASS |
| D1 demotion trigger (raw) | `department_schema_drops_a_demoted_principal_from_every_department` | demoted principal's member+lead rows gone, others kept | PASS |
| D1 demotion via production paths | `department_write_removing_a_teammate_or_deleting_an_agent_leaves_their_departments` | after `CompanyInvitePersistence::remove_member` and `AgentPersistence::delete`, rows gone | PASS |
| AC2 provenance (Rust ↔ SQL) | `department_schema_grant_provenance_check_accepts_every_variant`, `every_grant_provenance_round_trips_through_its_stored_string` | every `GrantProvenance::ALL` accepted; unknown refused by `channel_principal_grants_provenance_check` | PASS |
| AC4/AC5 round trip | `department_write_round_trips_create_rename_reparent_and_delete` | normalized create, admin edit, re-parent, root, re-read equality, delete | PASS |
| D5 PATCH keeps absent fields; `null` parent → root (F3) | `department_write_a_name_only_edit_keeps_every_other_field`; route `department_routes_round_trip_and_refuse_an_unknown_role` (`{"charter":null}`, `{"name":..}` keeps slug) | full-struct equality except name / parent | PASS |
| AC4 lead assign/replace/clear | `department_write_assigns_replaces_and_clears_the_lead` | exact holder lists; clear non-holder NotFound | PASS |
| AC4 members add/remove, idempotent, external BadRequest, foreign NotFound | `department_write_adds_and_removes_members` | exact member sets | PASS |
| AC4 taken slug Conflict; malformed BadRequest | `department_write_refuses_a_taken_slug_with_conflict` | `AppError::Conflict` on create and rename | PASS |
| AC4 preflight names departments; depth boundaries at use case | `department_write_preflight_names_the_departments_in_a_cycle_or_too_deep` | message contains `'Middle' sits inside 'Top'` / `under 'Level 8'`; subtree under level 2 refused, under root ok | PASS |
| AC4 delete with children | `department_write_refuses_deleting_a_department_with_children` | `Conflict` | PASS |
| AC5 authz | `department_write_refuses_members_and_strangers_with_not_found` | every write + manager list → NotFound for member/stranger; foreign dept NotFound for owner; nothing changed | PASS |
| AC5 visibility | `department_write_shows_a_member_only_their_departments` | exact id sets for member/owner/admin; unseen detail NotFound | PASS |
| AC4 competing re-parents | `department_competing_reparents_never_form_a_cycle` | held company row, 2 blocked, one Ok + one BadRequest, one parent | PASS |
| AC4 competing creates | `department_competing_creates_of_one_slug_leave_one_row` | one Ok + one Conflict, one row | PASS |
| AC4 competing leads | `department_competing_lead_assignments_leave_one_holder` | both Ok, exactly one holder | PASS |
| D2 kind race (add behind demotion) | `department_competing_member_add_behind_a_demotion_is_refused` | BadRequest, no row | PASS |
| D2 kind race reverse (F10) | `department_competing_demotion_behind_a_member_add_removes_the_row` | add Ok, demotion Ok, no row | PASS |
| AC3 units | `department_write_normalize_trims_and_canonicalizes`, `department_write_normalize_refuses_what_the_schema_would`, `department_edit_normalize_touches_only_the_fields_it_sets`, `every_department_role_round_trips_through_its_stored_string`, `an_unknown_department_role_is_refused_by_name`, `the_lead_has_a_single_holder` | | PASS |
| AC6 routes | `department_routes_hide_foreign_companies_and_refuse_members_and_strangers`, `department_routes_round_trip_and_refuse_an_unknown_role` | 404 on every write/detail for member/stranger/foreign, body hides foreign name; member list `[]`; 201/200 bodies; unknown role 400 | PASS |

Stand-in limits: route tests call handlers directly, not through the axum `Router`; route
registration is covered only by `workflow_routes_mount_in_the_app_router` building the full router
(no conflict panic). Account deletion (`user_lifecycle.rs`) is covered by the trigger mechanism
(raw demotion test) and its existing tests still pass, not by a department-specific test of that
path. The competing tests depend on releasing the held lock inside `begin_bounded`'s 1 s
`lock_timeout`; a timeout surfaces as `DatabaseTimeout` and fails the assertions, never as the
expected variant.

## Contract impact (actual, B1)

| Changed contract/value | Producers / consumers | Disposition | Checks |
| --- | --- | --- | --- |
| `channel_principal_grants.provenance` gains `department_membership` | `GrantProvenance` enum/`FromStr`/serde; `CHANNEL_SELECT` decodes `principal_grants` jsonb via serde | variant added; no writer in p01 | provenance tests; `channel::tests::` |
| `principals.kind` updates now fire `department_principal_demoted` | `company_invite::remove_member`, `agent::delete`, `user_lifecycle` account deletion, invite acceptance (external→person, trigger `WHEN` excludes) | trigger deletes only department rows | `remove_member`/`agent_retire`/`user_lifecycle`/`account_deletion`/`company_invite` filters (48 pass) |
| new public triggers/functions | catalog-enumerating tests (`legacy_runtime_schema`, `revision_overflow`, `state_witness`) | names contain no legacy terms; snapshots are per-test | same run, pass |
| new `DepartmentPersistence` port; `AppState.department_use_cases` | `PostgresPersistence`; `infra::setup` (only AppState literal); `FromRef` | new | compile; router mount tests |
| `SlugKind::DepartmentAddress` | `validate_slug` noun match (exhaustive) | added | normalize unit tests; `use_cases::channel::` tests |
| new source files | Cargo module tree | registered | `--test source_modules` (8 pass) |

## Check results (B1-R1, 2026-11-09)

Fixture: `TEST_DATABASE_URL=postgres://mac02@localhost:5432/openbot_dept_p01_test` (phase-owned),
`own_database` for competing tests; `SQLX_OFFLINE=true`; `--test-threads=4`.

| Command | Result | Log |
| --- | --- | --- |
| `cargo test --locked --offline --lib department -- --test-threads=4` | 32 passed | `logs/B1-R1-department-tests.txt` |
| `cargo test --locked --offline --lib -- --test-threads=4 the_lead_has_a_single_holder every_grant_provenance participant:: use_cases::channel:: adapters::persistence::tests:: test_support::tests:: channel::tests::` + `cargo test --locked --offline --test source_modules` | 54 passed; 8 passed | `logs/B1-R1-affected-tests.txt` |
| `cargo test --locked --offline --lib -- --test-threads=4 remove_member agent_retire user_lifecycle account_deletion legacy_runtime_schema revision_overflow state_witness company_invite` | 48 passed | `logs/B1-R1-demotion-callers.txt` |
| `cargo test --locked --offline --lib -- --test-threads=4 workflow_routes_mount_in_the_app_router authoring::security_tests` | 3 passed | `logs/B1-R1-router-mount.txt` |
| `cargo clippy --locked --offline --lib --tests -- -D warnings` | clean | `logs/B1-R1-clippy.txt` |
| `cargo fmt --all -- --check` | clean (after `cargo fmt --all`) | — |
| `sqlx migrate run` on fresh `openbot_dept_p01_dev` + `SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets` | pass; `.sqlx` unchanged (department queries are runtime) | `logs/B1-R1-sqlx-prepare.txt` |
| `git diff --check` | source clean; only psql-output logs carry trailing spaces | — |
| Test inventory | `logs/B1-test-inventory.txt` (filters nonzero: schema 9, write 12 incl. 2 unit, competing 5, routes 2) | |

Not run (I003): full `scripts/check.sh`, all-target clippy, network isolation, stack/lease budgets.

## B1 correction (B1-R2, 2026-11-09) — answers 01DEP-P01-B1-F1..F4

- F1 (Low, actionable): `department_write_persistence_reauthorizes_every_write_under_the_lock`
  calls every `DepartmentPersistence` write directly with a member's and a stranger's scope →
  `NotFound` each, departments/members/roles unchanged; and
  `competing::department_competing_write_behind_an_admin_demotion_is_refused` holds the company row
  while demoting the admin, lets the admin's create pass `managed()` and wait on the lock, commits
  the demotion → `NotFound`, 0 departments.
- F2: `departments_parent_fk` comment no longer gives the withdrawn NO ACTION rationale.
- F3: route test adds list 404 for owner→foreign and stranger, and 500 for create/list with a
  failing company persistence.
- F4: `RowLock` enum replaces the `&str` lock clause; `DepartmentWrite.created_by` doc fixed; shape
  comment says "stop at level ten"; persistence tests split into `department_tests.rs` (fixture +
  write, 923 lines), `department_schema_tests.rs` (`tests::schema`), `department_competing_tests.rs`
  (`tests::competing`). Inventory diff `logs/B1-R2-test-inventory-diff.txt`: 14 moved (each maps to
  the same leaf name under the new module), 2 new, none lost.
- Checks: department filter 34 passed (`logs/B1-R2-department-tests.txt`) after recreating the
  phase test DB — the first run failed every DB test with `VersionMismatch(20260817000000)` because
  the comment edits changed the init schema checksum (`logs/B1-R2-department-tests-stale-db.txt`,
  environment, not code). `source_modules` 8 passed; clippy `-D warnings` clean; fmt clean;
  `git diff --check -- src migrations` clean. B1-R1 caller/catalog runs reused (only SQL comments
  and tests changed since). Frozen digest `ccc4ee26be865cf9806a1e192f05bdddb8753eb4993704e2b8f98fcc78f03880`,
  manifest `logs/01DEP-P01-B1-R2.evidence.json` (4 PASS, 1 FAIL retained, 1 NOT EXERCISED).
