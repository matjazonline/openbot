# 01 Ownership and selection

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Skills and connections belong to a unit; authoring offers, publication captures and admission
re-checks only resources in the run's scope units. Continues in
[02](02-models-budgets-and-usage.md).

## Batch 01A Schema

1. `skills`, `company_mcp_connections` and `workflow_http_connections` gain `unit_id uuid NOT NULL`
   with a composite foreign key to `departments (company_id, id) ON DELETE RESTRICT`. Creation
   defaults it to the creator's single managed department, else the root. Global library rows
   (`company_id IS NULL`) have no unit, enforced by a check.
2. Publication snapshots that capture a tool, connection or skill (`ToolSnapshot`,
   `src/application/workflow/publication/types.rs`; `agent_skill_assignments`) record the unit id
   with the resource, so a later move is visible as a mismatch rather than silently widening.
3. Deleting a department that owns resources is refused by the foreign keys; the Departments pane
   lists what to move first.

## Batch 01B Selection and checks

1. Authoring reads (`AgentAuthoringReads::tool_options`, `connection_options`,
   `src/application/agent/authoring_reads.rs`) take the authoring unit and list only resources whose
   unit is that unit or an ancestor. The authoring unit is the definition's managing department
   (06, Batch 01B item 4) or, for agent authoring, each department the agent belongs to, union.
2. Publication refuses a captured resource outside the authoring unit's chain with
   `publication.resource_outside_unit`, naming the step and resource.
3. Admission compares every captured resource unit with the run's scope units from
   `workflow_run_scopes` (05-scopes) and refuses with `admission.resource_outside_unit` when a
   resource has since moved out of reach. Live authority events (connection disabled, credential
   revoked) are unchanged.
4. Moving a resource: `move_resource_on(tx, kind, id, target_unit)` requires `manages_unit` on
   source and target, updates `unit_id`, and leaves published snapshots alone; affected active
   bindings are listed in the response so the manager can republish or move desks.

## Focused checks

- `unit_resource_schema_`: foreign keys, default unit, library rows without unit, department delete
  refusal.
- `unit_resource_selection_`: authoring lists for root, child and sibling; publication refusal;
  admission refusal after a move; a resource moved up becomes selectable below; an agent in two
  departments sees the union.
- `unit_resource_competing_`: a move racing a publication leaves either a refused publication or a
  snapshot carrying the old unit that admission then refuses.
