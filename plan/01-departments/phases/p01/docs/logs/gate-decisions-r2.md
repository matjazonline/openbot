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

## Check results

(none yet)
