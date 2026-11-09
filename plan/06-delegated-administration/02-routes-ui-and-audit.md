# 02 Routes, UI and audit

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Every management route and page shows a person exactly what they manage, refuses the rest the way
it refuses strangers, and every management write records who did it as which role. Continues in
[03](03-integration-and-final-verification.md).

## Batch 02A Routes and pages

1. Every `*_authorization_tests.rs` under `src/adapters/http/routes/` gains the lead cases: a lead
   of the right unit succeeds, a lead of a sibling gets the same `404` a stranger gets. Workflow
   authoring access tests (`routes/workflows/ui/authoring/access_tests.rs`) gain a lead who may
   edit a definition bound in their subtree and may not bind it outside.
2. Lists filter to the managed subtree: departments, desks, agents, definitions, schedules and
   bindings pages show what the viewer manages plus what they may read. The Departments pane shows
   management controls only for departments the viewer manages, and the role picker offers only
   what decision 4 allows.
3. The company settings page shows company-level controls to root managers only; a department lead
   landing there sees the Departments tab scoped to their subtree.
4. The JSON API keeps its shapes. Refusals are `404`, matching the existing "a stranger is told
   nothing" rule.

## Batch 02B Audit

1. Management writes that already audit (`binding_audit_events`, `init_schema.sql:3515`) record
   the acting role: `owner`, `admin`, `lead:<department_id>`. Writes that do not audit today
   (department, role and membership changes from 01-departments) write to a new
   `administration_audit_events (company_id, actor_principal_id, acting_role, target_kind,
   target_id, action, created_at)`, append-only.
2. The Departments pane shows the last ten administration events for the selected department.

## Focused checks

- Route tests as in 02A item 1 for every management route.
- `administration_audit_`: each write kind leaves one row with the right acting role; the pane
  renders them; rows are immutable.
