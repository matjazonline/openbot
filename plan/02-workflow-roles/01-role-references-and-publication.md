# 01 Role references and publication

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Workflow source can say `role: lead` where it says `agent: <key>` today, the compiler validates it,
and the published version records the requirement. Continues in
[02](02-admission-resolution-and-snapshots.md).

## Batch 01A Source and compiler

1. Widen `DepartmentRole` (`src/domain/entities/department.rs`, from 01-departments) with
   `Reviewer`; widen `department_roles_role_check` and drop the single-holder partial index's
   coverage to `lead` only (it already is). Extend the enum tests.
2. In the step registry (`src/application/workflow/registry/`), every contract that takes `agent`
   (`agent.run`, `decision.agent`, `ai.classify`; see `contract_tests.rs:5`) accepts exactly one of
   `agent` or `role`. `role` is a `DepartmentRole` string. Both absent or both present is a
   compile error with a stable code `registry.agent_or_role`.
3. The compiler (`src/application/workflow/compiler/{wire,compile}.rs`) carries role references
   into the compiled graph as `RoleReference { step, role, mode: AgentUseMode }`. Add them to the
   compiled identity so a role edit republishes, as an agent edit does.
4. Publication (`src/application/workflow/publication/`): `agent_universe` and `build` ignore
   role-filled steps when capturing agent snapshots and record the `RoleReference` list in the
   version beside `AgentReference`. Capability resolution (`resolve`) treats a role-filled step as
   satisfied at publication; the admission check in [02](02-admission-resolution-and-snapshots.md)
   is where it is enforced. Update `workflow_capabilities_classify_captures_and_requires_its_agent`
   (`publication/capabilities_tests.rs:172`) with a role twin that captures no agent.
5. Binding configuration (`src/application/workflow/binding/`): activating a binding whose version
   has role references requires `channel_id` to be set and the channel to have a department, or
   it is refused with `binding.role_needs_department` naming the first such step. Deactivation is
   unaffected. Clearing a desk's department while an active binding needs roles is refused on the
   channel settings side with the same code; this is the one place department ownership blocks a
   desk edit.

## Batch 01B Authoring help and samples

1. Authoring help for the affected steps documents `role`, lists the vocabulary and states the
   resolution rule in one sentence. The sample list gains
   `src/application/workflow/fixtures/role-lead-response.yaml`: `context.history` →
   `agent.run` with `role: lead` → `reply`.
2. `docs/workflow-language-v1.md` gains a "Roles" section with the same content.

## Focused checks

- `workflow_role_source_`: `agent` and `role` are mutually exclusive; unknown role strings and
  role on a step that takes no agent are refused with the named codes; the compiled identity
  changes when a role changes.
- `workflow_role_publication_`: a role-filled version captures no agent snapshot and records the
  reference; a mixed version records both; forged snapshots are still refused.
- `workflow_role_binding_`: activation on a department desk succeeds; on a desk without a
  department or company-wide it is refused with the step name; clearing the department of a desk
  with an active role binding is refused.
