# 01 The predicate and its owners

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

One function answers "may this person manage this unit", every management use case asks it with
the unit the target belongs to, and the database refuses what the application does not check.
Continues in [02](02-routes-ui-and-audit.md).

## Batch 01A The predicate

1. `manages_unit(actor: UserId, unit: DepartmentId) -> bool` in
   `src/application/use_cases/administration.rs`, backed by one SQL statement: owner of the company,
   or `admin` role on the root, or `lead` role on the unit or an ancestor (recursive CTE, depth
   bound 8). It replaces `managed_company` (`src/application/use_cases/company.rs:223`) for
   unit-owned targets; `managed_company(user, company)` remains as `manages_unit(user, root)`.
2. `CompanyMembership::manages_company_operations` (`company_member.rs:85`) is removed. The
   callers found by `graft callers manages_company_operations --depth all` each name the unit they
   are about: a channel's `department_id`, a binding's channel's department, a schedule's channel's
   department, an agent's departments, a definition's managing department (decision 3), a
   department itself or its parent for role writes (decision 4).
3. `WorkflowAuthorization::authorize` (`src/application/workflow/authorization.rs:150`) resolves the
   association to a unit and asks the predicate; `RelatedAssociation::Company` means the root. An
   agent actor is refused before the lookup (decision 6).
4. Persistence rechecks under the company lock the way schedule writes do
   (`schedule_management_requires_an_owner_or_admin_who_sees_the_channel`,
   `src/adapters/persistence/workflow/schedule_tests.rs:598`): the write statement joins the
   predicate's SQL so a request that skipped the use case is refused just the same.

## Batch 01B Owners that change

1. **Departments.** Create a child, rename, re-parent within the managed subtree, delete: the
   parent's manager. Add or remove members: the department's manager. Assign or clear a role: the
   parent's manager, owner for the root (decision 4). Re-parenting out of the managed subtree is
   refused.
2. **Desks.** Create a desk in a department, edit settings, assign to a department within the
   managed subtree, delete: the department's manager. A desk with no department (impossible after
   04-root-unit) is the root's.
3. **Bindings and schedules.** Configure, activate, pause: the desk's department manager. A
   company-wide binding is the root's.
4. **Workflow definitions.** `workflow_definitions` gains `managing_department_id`, set at creation
   to the creator's single department or chosen, root by default. Editing a draft and publishing:
   the managing department's manager. Binding a version to a desk outside the managed subtree is
   refused with `binding.outside_managed_units`.
5. **Agents.** Create (placing it in a department the actor manages), edit, publish, assign tools
   and skills, delete: a manager of any department the agent belongs to (decision 2). Library
   copies the same. The Agent Builder tools (`create_agent_channel`, `list_company_agents`) act
   with the run's actor and therefore the same rule.
6. **Knowledge and memory.** Knowledge upload authority from 05-scopes already follows the one-step
   rule; a unit manager additionally has it for the whole subtree. Memory settings stay company
   level: the root's manager.
7. **Invites.** Inviting a person into the company is the root's manager; adding them to a
   department is that department's manager.

## Focused checks

- `manages_unit_`: pure matrix over owner, root admin, lead of the unit, lead of an ancestor, lead
  of a sibling, lead of a child, plain member, stranger, agent actor; depth 8 bound.
- `delegated_admin_`: one database test per owner in 01B asserting the manager succeeds, a sibling
  lead gets `NotFound`, and the persistence recheck refuses a write that bypassed the use case.
- `delegated_admin_competing_`: a role removal racing a management write leaves either the write
  refused or applied under the old role, never half applied.
