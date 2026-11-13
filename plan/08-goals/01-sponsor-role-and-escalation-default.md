# 01 Sponsor role and escalation default

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A unit can name its sponsors and which of them decides, a workflow step can ask "the sponsor" the
way it asks "the lead", and a unit can say who is told when work on its desks stalls. Starts when
[02-workflow-roles](../02-workflow-roles/README.md) is complete; needs nothing later. Continues in
[02](02-unit-lifecycle.md).

File anchors are on `main` as of 2026-11-13. Symbols that 02 introduces are named without a line,
because 02 was still in progress on that day.

## Batch 01A The role and its decider

1. `DepartmentRole` (`src/domain/entities/department.rs:47`) gains `Sponsor`, not single-holder.
   `department_roles_role_check` (`init_schema.sql:15542`) is widened, and so is the role check on
   `workflow_run_role_bindings` from 02.
2. `department_roles` (`init_schema.sql:15534`) gains `decider boolean NOT NULL DEFAULT false`,
   with `CHECK (NOT decider OR role = 'sponsor')` and a partial unique index on `(department_id)`
   where `role = 'sponsor' AND decider`. The database guarantees at most one decider; the
   application keeps exactly one on every write a person makes.
3. `assign_department_role_on` (`src/adapters/persistence/department.rs:528`) takes the decider
   flag for `Sponsor`. The first sponsor of a unit becomes the decider. Naming a new decider moves
   the flag in the same statement. `clear_department_role_on` (`:678`) on the decider while other
   sponsors remain is refused with `department.sponsor_needs_decider` unless the request names the
   successor; clearing the last sponsor is allowed.
4. Principal retirement clears roles without choosing a successor (02, batch 04B item 4), so a
   retired decider can leave sponsors with no decider. Nothing is promoted automatically. The next
   resolution reports it (item 01B.2) and the escalation ladder carries it to a person.
5. Holders may be person or agent principals, which `guard_department_principal_kind`
   (`init_schema.sql:15612`) already enforces for every role.

## Batch 01B Resolution

1. `RoleHolder` in `src/application/workflow/roles.rs` (02) gains `decider`. For `Sponsor`,
   `resolve_role` considers the decider alone: the nearest unit in the chain that has any sponsor
   supplies it, and the kind check applies to the decider and not to the other holders.
2. `RoleRefusalKind` gains `NoDecider` with code `workflow.role_no_decider`, returned when the
   nearest unit with sponsors has none marked. Resolution does not walk past that unit, as it does
   not walk past a `WrongKind`.
3. `RoleNeed::People` with an agent decider and `RoleNeed::Agent` with a person decider are
   `WrongKind`. So `decision.human` with `role: sponsor` among its reviewers needs a person, and
   `agent.run`, `decision.agent` and `ai.classify` with `role: sponsor` need an agent. Nothing in
   the step registry changes beyond the vocabulary; the contracts already take any `DepartmentRole`.
4. The other sponsors are frozen with the run. The sealed role entry and its
   `workflow_run_role_bindings` rows gain `capacity`, `decides` or `informed`; every role other
   than `sponsor` writes `decides`. Only `decides` rows become `workflow_decision_reviewers` rows
   (`init_schema.sql:5248`). A person holder with `informed` receives the decision notice marked as
   information and cannot settle it. An agent holder receives no notice.
5. `reviewers_follow_role` (02, batch 04B) covers `sponsor`: when the decider changes while a
   decision is parked, the decision moves to the new decider and the informed rows are replaced in
   the same commit.

## Batch 01C A unit's escalation default

1. `departments` (`init_schema.sql:15492`) gains `escalation jsonb`, null or one
   `EscalationPolicy { notify, hops }` as 02 defines it in
   `src/application/workflow/binding/escalation.rs`, validated by the same type and by a shape
   check. `update_department_on` (`department.rs:387`) writes it.
2. When the ladder triggers for a run whose binding has no `escalation` of its own, it uses the
   default of the desk's unit, else of the nearest ancestor that has one, within the depth bound of
   8. A binding's own policy wins. No default anywhere means no ladder, as in 02.
3. The default is read when the ladder triggers, not at admission. `workflow_run_escalations`
   records the role and principal each hop went to, which is what makes the live read auditable.
4. `notify: sponsor` sends hop 1 to the decider. With no sponsor or no decider, the ladder goes to
   hop 2 as it does for any role with no holder. Hop 2 is unchanged: the parent unit's `lead`.

## Batch 01D UI and authoring

1. The department page (`src/adapters/http/pages/company_departments.rs`) lists sponsors under the
   lead, with one marked "decides", and lets a manager add, remove and re-mark them. The JSON API
   (`src/adapters/http/routes/department.rs`) carries `decider` on role writes and reads.
2. The department page gains "When work stalls, tell" with the role and hop choices, shown as
   inherited from the named ancestor when the unit has none of its own.
3. The role groups that 02 adds to the agent and reviewer pickers list `sponsor` from
   `DepartmentRole::ALL`. The reviewer preview marks today's decider.
4. Authoring help and `docs/workflow-language-v1.md` say that `sponsor` resolves to one actor and
   which steps need which kind. The sample list gains
   `src/application/workflow/fixtures/role-sponsor-approval.yaml`: `agent.run` with `role: lead` →
   `decision.human` with the `sponsor` role as reviewer → `reply`.

## Focused checks

- `department_sponsor_`: several sponsors with one decider; the first becomes decider; moving the
  flag; clearing the decider without a successor is refused; a second decider is refused by the
  index; `decider` on another role is refused by the check; retirement leaves none.
- `workflow_role_resolve_sponsor_`: unit coverage of the resolver for the decider alone, the
  ancestor walk, `NoDecider`, both `WrongKind` directions, and mixed holder kinds with a valid
  decider.
- `workflow_role_sponsor_admission_`: with the scripted-model `Deployment` harness, one published
  procedure bound on two units parks for two different deciders; informed holders get a notice and
  cannot settle; a decider change after admission does not move a decision without
  `reviewers_follow_role` and does with it.
- `workflow_escalation_default_`: a unit default applies to a binding with none; a binding's own
  policy wins; the nearest ancestor supplies it; no default produces no hop; a default changed
  after admission is the one used.
- `department_sponsor_competing_`: two writers naming different deciders leave exactly one; a
  retirement racing an admission leaves either a refused admission or a run bound to the old
  decider, never a run with no decider.
- Rendering tests for the sponsor list, the escalation setting and the picker preview.
