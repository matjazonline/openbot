# 04 Root unit: the company is the root department

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed, 2026-11-09. Fourth of five organization plans, followed by
[05-scopes](../05-scopes/README.md); depends on
[01-departments](../01-departments/README.md), [02-workflow-roles](../02-workflow-roles/README.md)
and [03-principal-addresses](../03-principal-addresses/README.md).

## Intended behavior

There is one tree. Every company has exactly one root department, created with it. Company
membership is membership in the root; the company admin is the `admin` role on the root; the
company owner stays `companies.user_id` and holds the `owner` role on the root as a mirror. Every
desk belongs to a department, the root by default. A desk's audience is then always "the members
of the department that owns it, one step up", so the `team` and `allowlist` access modes and the
`members_only` flag from 01-departments collapse into one rule with explicit grants on top.

The company remains the tenant and the hard security boundary. Nothing crosses it; the root is a
department inside it, not a department above it.

## Decisions (user, 2026-11-09)

1. **One root per company, created in the company's transaction.** `departments.is_root` with a
   partial unique index per company; `parent_id IS NULL` iff `is_root`. Onboarding and company
   creation write it; it cannot be deleted or re-parented.
2. **`company_members` is replaced by root membership.** `department_members` gains
   `access_level` (the `access_level` domain, `init_schema.sql:903`), meaningful on root rows and
   `NULL` elsewhere by check. Invites join the root. `CompanyMembership`
   (`src/domain/entities/company_member.rs:63`) is derived: owner from `companies.user_id`, admin
   from the root `admin` role, member from root membership, else none. The 114 files that read
   `company_members` move to one `membership_of(user, company)` read.
3. **Roles on the root: `owner`, `admin`.** `DepartmentRole` gains both; `owner` has exactly one
   holder, kept equal to `companies.user_id` by trigger; `admin` may have several. Non-root
   departments may not hold them.
4. **Every desk has a department.** `channels.department_id NOT NULL`, defaulting to the root at
   creation. The `access_mode` check becomes `members` or `public`; `allowlist` and
   `members_only` are removed. Access is: members of the owning department, its role holders and
   the parent's `lead`, plus explicit grants, plus the owner exemption. A root desk is therefore
   reachable by the whole company, which is what `team` meant.
5. **Explicit grants stay.** `configured_allowlist` grants still admit named outside addresses on
   any desk; `department_membership` grants are still materialized rows, so
   `Channel::participant_access` keeps reading rows and one function.
6. **The one-step rule is unchanged.** A root member is not thereby a member of every department;
   a sub-department's members-only desk still needs membership, a role, or the parent lead.

## What does not change

- The tenant boundary, composite foreign keys that pin `company_id`, and every "stranger is told
  nothing" refusal.
- Access levels as a knowledge and memory classification (206 files). They move column, not
  meaning.
- Workflow admission, roles and principal addresses from the earlier plans.

## Documents

1. [Root department and membership](01-root-department-and-membership.md).
2. [Access modes](02-access-modes.md).
3. [Integration and final verification](03-integration-and-final-verification.md).
