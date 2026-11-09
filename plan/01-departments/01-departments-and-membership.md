# 01 Departments, membership and roles

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A company owner or admin can create, rename, nest and delete departments, add or remove members,
and assign the `lead` role. Every write is one transaction under the company's write lock, and the
schema refuses what the application does not check. Continues in
[02](02-desks-ownership-and-access.md).

## Batch 01A Schema

Edit `migrations/20260817000000_init_schema.sql` in place, next to `channels` (`:882`) and
`principals` (`:922`).

1. `departments`:
   - `id uuid`, `company_id uuid`, `name text`, `slug citext`, `description text`,
     `parent_id uuid`, `created_by jsonb`, `created_at`.
   - `departments_name_not_blank` and `departments_slug_format`, the same shape as
     `agents_name_not_blank` and `agents_slug_format` (`:3469`), so a department slug is a legal
     channel slug and can be the department desk's address.
   - `UNIQUE (company_id, slug)` and `UNIQUE (company_id, id)`, the second so every reference can
     be a composite foreign key that pins the company, as `workflow_parent_execution_fk` does.
   - `FOREIGN KEY (company_id, parent_id) REFERENCES departments (company_id, id)` and
     `departments_not_own_parent CHECK (parent_id IS NULL OR parent_id <> id)`.
   - `created_by` validated by `valid_creation_provenance`, as every created row is.
2. `department_members`: `company_id`, `department_id`, `principal_id`, `created_at`;
   `PRIMARY KEY (department_id, principal_id)`; composite foreign keys to `departments` with
   `ON DELETE CASCADE` and to `principals`.
3. `department_roles`: `company_id`, `department_id`, `role text`, `principal_id`, `created_at`;
   `PRIMARY KEY (department_id, role, principal_id)`; the same composite foreign keys;
   `department_roles_role_check CHECK (role = 'lead')`, widened by later plans;
   `UNIQUE (department_id, role) WHERE role = 'lead'` as a partial unique index, so a department
   has at most one lead while later roles may have several holders. The role string is the
   domain's `DepartmentRole` newtype rendered, never free text from a form.
4. Trigger `guard_department_shape` (`BEFORE INSERT OR UPDATE` on `departments`) walks `parent_id`
   with a recursive CTE and raises on a cycle or a depth above 8. Trigger
   `guard_department_principal_kind` on `department_members` and `department_roles` refuses a
   principal whose kind is `external` or `system` (`principals_kind_check`, `:932`). Existing
   guards show the house style: `guard_agent_owned_identity` (`:12349`).
5. Deleting a department that still owns channels is refused by the `channels.department_id`
   foreign key added in [02](02-desks-ownership-and-access.md) (`ON DELETE RESTRICT`), and one
   that has children by the parent foreign key. Members and roles cascade. Nothing else is needed
   for decision 9.

## Batch 01B Domain and persistence

1. `src/domain/entities/department.rs`: `Department { id, company_id, name, slug: DepartmentSlug,
   description, parent_id, created_by, created_at }`, `DepartmentId` and `DepartmentSlug` newtypes
   beside `ChannelSlug`, `DepartmentRole` with the single variant `Lead` and `as_str`/`FromStr`
   like `GrantProvenance`, `DepartmentRoleHolder { role, principal_id }`, and `DepartmentWrite`
   with `normalize()` (trimmed name, lowercased slug, same validation as `ChannelWrite`).
   Add `GrantProvenance::DepartmentMembership` (`src/domain/entities/participant.rs:218`,
   string `department_membership`) and extend `channel_principal_grants_provenance_check`
   (`init_schema.sql:874`). Its tests enumerate every variant; extend them.
2. `src/adapters/persistence/department.rs` implementing a `DepartmentPersistence` port in
   `src/application/ports/`:
   - `create_on(tx, company, write, lead: Option<PrincipalId>)`, `update_on`, `delete_on`,
     `assign_role_on`, `clear_role_on`, `add_member_on`, `remove_member_on`, `get`,
     `list_by_company`, `members(department)`, `roles(department)`, `memberships(principal)`,
     `roles_held(principal)`; the `_on` forms take the caller's transaction so
     [02](02-desks-ownership-and-access.md) can re-derive grants and
     [03](03-leads-and-coordination.md) can provision the desk in the same commit.
   - Every write takes the company write lock first, in the order the schedule writes already use
     (`set_enabled`, `src/adapters/persistence/workflow/schedule.rs`), then `FOR UPDATE` on the
     department row. A membership or role change also locks the channels the department owns,
     because it rewrites their grants.
   - Preflight the cycle and depth rule with the same recursive CTE the trigger uses, so the
     user gets a `BadRequest` that names the ancestor instead of a constraint error; the trigger
     stays as the backstop.
   - A slug conflict maps to `Conflict` through `slug_conflict_error`
     (`src/adapters/persistence/channel.rs:180`), reused. Assigning `lead` when one is held
     replaces the holder in one statement (delete then insert in the same transaction), so the
     partial unique index is never the user's error.
3. `DepartmentUseCases` in `src/application/use_cases/department.rs`, wired in
   `src/infra/setup/accounts.rs` beside `ChannelUseCases`. Every method starts with
   `managed_company` (`src/application/use_cases/company.rs:223`), as `ChannelUseCases` does.
   Reads for a plain member (listing the departments they belong to or hold a role in) go through a
   `visible_departments(user, company)` method that filters on membership and roles, with the
   owner seeing all.
4. JSON API in `src/adapters/http/routes/department.rs`, mounted beside `routes/channel.rs`:
   `GET/POST /companies/{id}/departments`, `GET/PATCH/DELETE /companies/{id}/departments/{dep}`,
   `PUT/DELETE /companies/{id}/departments/{dep}/members/{principal}`,
   `PUT/DELETE /companies/{id}/departments/{dep}/roles/{role}/{principal}`. Authorization tests
   mirror `channel_authorization_tests.rs`.

## Focused checks

Run with an isolated `TEST_DATABASE_URL`; list the filters before executing so each has a nonzero
count.

- `department_schema_`: a cycle, a depth of 9, a cross-company parent, a cross-company role holder,
  an external lead, an external member, a second lead and an unknown role string are each refused
  by the named constraint or trigger; deleting a department cascades its members and roles.
- `department_write_`: create, rename, re-parent, assign and replace the lead, clear the lead, add
  and remove members round-trip; a taken slug is `Conflict`; a member or non-team user gets
  `NotFound` on every write, as the schedule tests assert
  (`schedule_management_requires_an_owner_or_admin_who_sees_the_channel`,
  `src/adapters/persistence/workflow/schedule_tests.rs:598`).
- `department_competing_`: two concurrent re-parents that would together form a cycle leave
  exactly one applied; two concurrent creates with the same slug leave one row; two concurrent
  lead assignments leave exactly one holder. These are the competing-claimant tests AGENTS.md
  requires for a new write protocol.
- Unit tests for `DepartmentWrite::normalize`, `DepartmentRole` and the provenance enum.
