# 07 Unit resources: skills, connections, models and budgets belong to a unit

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed, 2026-11-09. Seventh of the organization plans; depends on
[05-scopes](../05-scopes/README.md) for the run scope snapshot and on
[06-delegated-administration](../06-delegated-administration/README.md) for who may configure.

## Intended behavior

Skills (`skills`, `init_schema.sql:4274`), MCP and HTTP connections with their credentials
(`company_mcp_connections` `:3649`, `workflow_http_connections` `:5312`), model connections
(`company_model_connections` `:3701`) and budgets are company-scoped today. After this plan each
belongs to a unit, the root by default, and is usable by procedures running on desks of that unit
and its descendants: the same read set `run_scopes` computes for memory and knowledge. A billing
department's payment connection is not offered to a support procedure; a support skill is not
offered to a billing agent. Usage rolls up by unit, and a unit may carry a spend cap.

## Decisions (user, 2026-11-09)

1. **Ownership is a unit, usability flows down.** Each resource gets `unit_id`, root by default. A
   procedure may select a resource whose unit is in the run's scope units; publication captures the
   unit with the resource snapshot; admission re-checks it against the run's scopes, as it checks
   access levels today. Siblings never share; sharing is moving the resource up.
2. **Access levels stay on the resource.** The connection level rule (a run that has read level N
   cannot send to a destination below N) is unchanged and independent of the unit.
3. **Model defaults cascade.** A unit may set a default model connection; a desk's procedure with
   no pinned model uses the nearest ancestor's default, root last. The company default becomes the
   root's default.
4. **Budgets are per unit and additive down the tree.** `unit_budgets (unit_id, period, cap)` caps
   the spend of runs whose write unit is in that unit's subtree. A run's admission checks every
   ancestor's remaining budget and refuses with `budget.unit_exhausted` naming the first exhausted
   unit, the same way the root budget refuses today (`workflow_root_budgets`,
   `init_schema.sql:5648`).
   Usage attribution is the run's write unit from `workflow_run_scopes`.
5. **Capacity stays global and per company.** `workflow_capacity_policy` (`:5056`) is not
   per unit; a department cannot starve the company by configuration.
6. **Execution backends and machines are unit resources too.** Tenant-owned execution backends
   (`plan/tenant-owned-execution-backends.md`) and Cloudroom company machines attach to a unit
   under the same rule when they are implemented; this plan reserves the `unit_id` and the
   selection check for them.
7. **Configuration authority is the unit's manager** (06). Moving a resource requires managing both
   the source and the target unit.

## What does not change

- Credential storage, rotation and envelopes; a credential follows its connection.
- Tool grants on agents, which still name a connection; the unit check is an additional filter at
  selection and admission.
- Global library skills and agents (`company_id IS NULL`), which have no unit.

## Documents

1. [Ownership and selection](01-ownership-and-selection.md).
2. [Models, budgets and usage](02-models-budgets-and-usage.md).
3. [Integration and final verification](03-integration-and-final-verification.md).
