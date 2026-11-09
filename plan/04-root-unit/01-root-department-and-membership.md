# 01 Root department and membership

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Every company has a root department; membership, admin and owner facts come from it; the company
pages and invites operate on it. Continues in [02](02-access-modes.md).

## Batch 01A Schema

1. `departments.is_root boolean NOT NULL DEFAULT false`; `UNIQUE (company_id) WHERE is_root`;
   `departments_root_shape CHECK (is_root = (parent_id IS NULL))`. The slug of the root is the
   company slug. `guard_department_shape` refuses deleting or re-parenting the root.
2. `department_members.access_level public.access_level`, with
   `department_members_level_on_root CHECK ((access_level IS NULL) = NOT is_root)` enforced by a
   trigger joining `departments` (a check cannot cross tables). The owner's root membership row is
   level 9, as `company_members_owner_level_check` (`init_schema.sql:914`) says today.
3. `DepartmentRole` gains `Owner` and `Admin`; `department_roles_role_check` widened; partial
   unique index for `owner`; trigger `guard_root_roles` refuses `owner`/`admin` off the root and
   keeps the `owner` holder equal to the person principal of `companies.user_id`.
4. Drop `company_members` (`:906`). Every foreign key and function that referenced it
   (`notification_principal_can_view`, `:941`, among others) reads root membership instead.

## Batch 01B Application

1. One read, `membership_of(user, company) -> (CompanyMembership, AccessLevel)`, in the company
   persistence, replacing the per-table joins. `managed_company` and `readable_company`
   (`src/application/use_cases/company.rs:223`) use it. `PrincipalAccessContext`
   (`participant.rs:261`) is built from it.
2. Company creation and onboarding (`src/adapters/persistence/onboarding.rs`) create the root
   department, the owner's person principal, its root membership at level 9 and its `owner` role in
   the company's transaction.
3. Invites (`src/application/use_cases/company_invite.rs`, `company_invite_levels.rs`) add root
   membership with the invited level; promotion to admin assigns the `admin` role; the team pages
   (`ui_team_authorization_tests.rs`) read the root.
4. Account deletion and user lifecycle (`src/adapters/persistence/user_lifecycle.rs`,
   `account_deletion.rs`) remove root membership and roles where they removed `company_members`.

## Focused checks

- `root_department_`: a company has exactly one root, with the company slug; it cannot be deleted
  or re-parented; a second root is refused; `owner` off the root is refused; the owner role follows
  `companies.user_id`.
- `root_membership_`: owner, admin, member and stranger are derived correctly; invites, promotion,
  level changes and account deletion round-trip; every authorization test suite that used
  `company_members` fixtures passes against root membership.
- `root_competing_`: concurrent invite acceptance and account deletion leave a consistent row set.
