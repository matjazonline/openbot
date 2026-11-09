# p01 Departments, membership and roles — TASK

## Identity

- Plan / phase: `plan/01-departments/`, phase `p01` (document `01-departments-and-membership.md`)
- Workflow: `implement-opus-reviewed-2_2` (skill dir `/Users/mac02/.claude/skills/implement-opus-reviewed-2_2`)
- Record root: `/Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/phases/p01/docs/`
- Root = Author (A): session 68933786-cbe7-4b6b-9387-b5dcb29689a4, Opus 5.5 / xhigh (user confirmed
  2026-11-09; global settings `modelSettings.claude-opus-5-5.effortLevel = xhigh`)
- Reviewer (B): Opus 5.5 / xhigh subagent, agent ID recorded in REVIEW.md / HANDOFF.md once spawned
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

## Architecture gate — decisions (revision r1, 2026-11-09)

Gate needed: new schema and FK/cascade behaviour, a new per-company write/concurrency protocol that
p02/p03 extend, and the authorization model for a new resource.

### DEP-P01-D1 Schema placement and cascade semantics (r1)

- The three tables, their keys, FKs, indexes, the shape function and both triggers go in **one
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
  with **NO ACTION** (not RESTRICT) so a company teardown cascade, which deletes parent and child
  in one statement, is checked at statement end, while deleting a parent that still has children
  is refused; `departments_not_own_parent`.
- `department_members (company_id, department_id, principal_id, created_at)`,
  PK `(department_id, principal_id)`; `(company_id, department_id) → departments(company_id,id)
  ON DELETE CASCADE`; `(company_id, principal_id) → principals(company_id,id) ON DELETE CASCADE`
  (same as `channel_principal_grants_principal_fk`: removing a teammate or agent principal must not
  be blocked by a department row; membership grants nothing by itself, decision 5).
- `department_roles (company_id, department_id, role, principal_id, created_at)`,
  PK `(department_id, role, principal_id)`, same two FKs, `department_roles_role_check
  CHECK (role = 'lead')`, partial unique index `department_roles_one_lead ON (department_id, role)
  WHERE role = 'lead'` (literal deliberately scoped: only `lead` is single-holder).
- Secondary indexes for the reverse reads: `department_members (company_id, principal_id)`,
  `department_roles (company_id, principal_id)`, `departments (company_id, parent_id)`.

### DEP-P01-D2 Shape rule: one SQL definition for trigger and preflight (r1)

- `public.department_shape_problem(checked_company uuid, checked_department uuid,
  proposed_parent uuid) RETURNS text` (STABLE): `NULL` when acceptable, `'cycle'` when
  `proposed_parent = checked_department` or the upward walk from `proposed_parent` reaches
  `checked_department`, `'depth'` when `ancestor_depth(proposed_parent) + subtree_height(checked_department) > 8`
  (a root department is level 1; a chain of 8 is allowed, 9 refused). Both walks are recursive CTEs
  bounded at 9 steps so corrupt data cannot loop. The downward walk is required because
  re-parenting a node with children deepens its whole subtree; the plan's "walks parent_id" alone
  would miss that.
- Trigger `guard_department_shape` (`BEFORE INSERT OR UPDATE OF parent_id`) calls it and raises
  `23514` with constraint name `departments_shape` and the code in the message.
- Persistence preflight calls the same function inside the locked transaction and turns a problem
  into `BadRequest` naming the department and the offending ancestor/parent by name.
- `guard_department_principal_kind` (`BEFORE INSERT OR UPDATE` on both join tables) refuses a
  principal of kind `external`/`system` (`23514`, constraint `department_principal_kind`).
  Persistence preflights the principal's kind and company and returns `NotFound` (no such
  principal in this company) or `BadRequest` (external/system) before the trigger is reached.

### DEP-P01-D3 Write protocol and lock order (r1)

- Every department write runs in one transaction with `lock_timeout`/`statement_timeout` bounds
  (as `agent_publication::begin_bounded`), then **`workflow::authority::authorize_company(tx,
  company, actor)`** — `companies FOR NO KEY UPDATE`, then the actor's member/principal `FOR SHARE`,
  refusing anyone but an owner/admin with `NotFound`. This is the company write lock the schedule
  writes take (`schedules::managed`), and doubles as the under-lock re-authorization.
- Then the department row(s) `FOR UPDATE` within the tenant (`NotFound` when absent). Order:
  company → department → (p02) the department's members-only channels. Creating locks the parent
  row `FOR SHARE` when one is named.
- Because all department writes of one company serialize on the company row, the shape preflight
  reads a stable tree (two competing re-parents cannot jointly form a cycle), a competing create
  sees the committed slug (`UNIQUE` is the backstop and maps to `Conflict`), and lead replacement
  (`DELETE` the current lead row, then `INSERT`) leaves exactly one holder. The triggers and unique
  index are backstops only.
- `_on` contract (stable for p02/p03): `pub(crate)` adapter functions in
  `src/adapters/persistence/department.rs` taking `&mut Transaction<'_, Postgres>` whose caller has
  already taken the company write lock (`authorize_company` or `lock_company_on`):
  `create_department_on`, `update_department_on`, `delete_department_on`,
  `assign_department_role_on`, `clear_department_role_on`, `add_department_member_on`,
  `remove_department_member_on`. Each locks its department row itself. The application port cannot
  name sqlx (src/AGENTS.md "Preserve dependency direction"), so the `DepartmentPersistence` trait
  exposes whole-transaction methods that wrap these.
- Lead semantics: `assign_role(Lead, p)` replaces any current holder; assigning the current holder
  again is a no-op success. `clear_role(Lead, p)` deletes that holder's row; clearing someone who
  does not hold it is `NotFound`. `create(.., lead: Option<PrincipalId>)` assigns in the same
  transaction. A role holder need not be a member (decision 5).
- Add member twice: idempotent success (`ON CONFLICT DO NOTHING`); remove a non-member: `NotFound`.
- Delete: refused with `Conflict` naming the reason when the department has child departments
  (and, from p02, owned desks); members and roles cascade.

### DEP-P01-D4 Authorization model (r1)

- `DepartmentUseCases` writes and manager reads start with `managed_company`; its `Forbidden`
  for a plain member is converted to the same `NotFound` a stranger gets
  (`department_not_found()`/`company_not_found()` wording), as the plan's focused check requires
  ("a member or non-team user gets `NotFound` on every write, as the schedule tests assert").
  Persistence then re-authorizes under the lock (D3), so a demotion between the two cannot let a
  write through.
- `visible_departments(user, company)`: any company member (`readable_company`). Owners and admins
  (who may manage departments) see every department; a plain member sees departments where their
  `person` principal is a member or holds a role. Strangers get `NotFound`.
- Department ids and slugs from another company are indistinguishable from missing ones.

### DEP-P01-D5 Types and placement (r1)

- `src/domain/entities/department.rs`: `DepartmentId` (uuid newtype, serde transparent),
  `Department`, `DepartmentRole::Lead` with `as_str`/`FromStr`/serde `snake_case`,
  `DepartmentRoleHolder { role, principal_id }`, `HeldDepartmentRole { department_id, role }`,
  `DepartmentMember { principal_id, created_at }`.
- `DepartmentSlug` via `string_newtype!` in `value_objects.rs` beside `ChannelSlug`.
- `DepartmentWrite` lives in `src/application/ports/department.rs` beside the port, like
  `ChannelWrite`; its `normalize()` lives in `use_cases/department.rs` like `ChannelWrite::normalize`
  (the domain does not depend on `AppError`). It trims the name, lowercases/hyphenates the slug,
  runs `validate_slug(.., SlugKind::DepartmentAddress)` (reserved suffixes, so the slug can be the
  p03 desk address), checks the slug format in Rust (so a malformed slug is `BadRequest`, not a
  check violation), drops blank description/charter, and refuses a charter over 4 KiB.
- Taken slug: `AppError::Conflict("Department address '<slug>' is already in use in this company.")`.
  The plan's "reuse `slug_conflict_error`" is not followed literally: that helper returns
  `BadRequest` and names a channel. The p03 department desk uses the channel slug namespace and
  keeps `slug_conflict_error` there.
- JSON API under the existing JSON prefix `/api/companies/{company_id}/departments...` (the
  plan's `/companies/{id}/departments` path is the HTML convention; phase 04's UI routes take it).
  Bodies: `DepartmentJsonPayload { name, slug?, description?, charter?, parent_id?, lead? }`
  (create) and PATCH as the full editable set `{ name, slug?, description?, charter?, parent_id? }`
  (PATCH carries the whole write, like the channel PUT); `GET` returns the department with its
  `members` and `roles`. Role path segment parsed with `DepartmentRole::from_str` (unknown → 400).

## Verification plan (mapping filled with actual names after implementation)

| Criterion | Production path and case | Decisive assertion | Check | Status |
| --- | --- | --- | --- | --- |
| AC1 cycle | trigger on direct UPDATE (bypassing preflight) | `23514`, constraint `departments_shape` | `department_schema_*` | planned |
| AC1 depth 9 | trigger on INSERT of 9th level and re-parent of a subtree | `23514` | `department_schema_*` | planned |
| AC1 cross-company parent / role holder | raw INSERT | FK violation names composite FK | `department_schema_*` | planned |
| AC1 external lead/member | raw INSERT | `department_principal_kind` | `department_schema_*` | planned |
| AC1 second lead, unknown role | raw INSERT | unique index / role check | `department_schema_*` | planned |
| AC1 cascade | DELETE department; company delete with nested tree | members/roles gone; teardown succeeds | `department_schema_*` | planned |
| AC4/AC5 round trip | use cases on Postgres | create/rename/re-parent/lead assign-replace-clear/members | `department_write_*` | planned |
| AC4 conflict | create/rename to taken slug | `AppError::Conflict` | `department_write_*` | planned |
| AC4 preflight | re-parent into own descendant | `BadRequest` naming ancestor | `department_write_*` | planned |
| AC5 authz | member and stranger on every write | `AppError::NotFound` | `department_write_*` | planned |
| AC5 visibility | member sees only theirs; admin/owner all | exact id sets | `department_write_*` | planned |
| AC4 competing | two re-parents forming a cycle; same-slug creates; two lead assigns, concurrently on separate connections | exactly one applied / one row / one holder | `department_competing_*` | planned |
| AC2/AC3 units | `DepartmentWrite::normalize`, `DepartmentRole`, `GrantProvenance` | every variant round-trips | unit tests | planned |
| AC6 routes | JSON handlers with member/foreign/failing cases | 404 for member and foreign | `department_` route tests | planned |

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

## Check results

(none yet)
