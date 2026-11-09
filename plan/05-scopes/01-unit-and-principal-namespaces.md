# 01 Unit and principal namespaces

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Every department and every agent or person principal has exactly one live memory namespace,
created and retired with its owner; nothing else has one. Continues in
[02](02-scope-resolution-and-tools.md).

## Batch 01A Schema

1. `memory_namespaces.scope_kind` becomes `unit` or `principal`; `agent_id` and `user_id` are
   replaced by `unit_id` and `principal_id`, plain ids as today (the owner may be deleted while
   removal reconciles). The shape check requires exactly the matching id. The live-scope unique
   index `memory_namespaces_live_scope` keys on company, kind and the two coalesced ids.
2. `memory_deliveries` and any provenance that recorded `agent_id`/`user_id` record `principal_id`
   and `unit_id`, plus `subject_principal_id NULL` for records about a person (decision 3). The
   service-side observation request carries `subject` as an opaque provenance string; the service
   needs no new field semantics.
3. A trigger refuses a `principal` namespace for an `external` or `system` principal
   (`principals_kind_check`, `init_schema.sql:932`), and a `unit` namespace for a unit of another
   company, through the composite foreign keys the departments plan adds.

## Batch 01B Owners and lifecycle

1. `NamespaceOwner` (`src/application/memory/settings.rs:164`) becomes
   `Unit(DepartmentId) | Principal(PrincipalId)`. `MemoryStepScope`
   (`src/application/workflow/memory.rs:27`) becomes `Unit(UnitSelector) | Agent(Uuid)`, where
   `UnitSelector` is `Own | Root`, resolved at
   admission against the desk's department (own) or the company's root. `memory_scopes_resolve`
   (`src/application/workflow/lifecycle/definitions.rs:227`) keeps refusing an agent key the bundle
   did not capture.
2. Provisioning moves from lazy to deterministic. `create_department_on` and the root creation in
   company onboarding ([04-root-unit](../04-root-unit/01-root-department-and-membership.md)) call
   `ensure_namespace` for the unit; `create_agent_principal_on` and person principal creation call
   it for the principal. The lazy path in the memory tools runtime
   (`provision_memory_scope`, `src/application/workflow/memory_tools/runtime.rs:102`) stays as the
   backstop for a tenant enabled after its departments exist, and provisions nothing else.
3. Retirement: `delete_on` for a department calls `retire_namespaces_on` for its unit, as agent and
   user deletion already do (`agent.rs:472`, `user_lifecycle.rs:88`). Re-parenting a department
   touches no namespace. Removing a member from a department touches no namespace; its reads
   change through [02](02-scope-resolution-and-tools.md). Knowledge documents are not namespaces
   and are covered in [03](03-knowledge-scopes.md).
4. A returning principal or a re-created department slug gets a new namespace id, as today (E9);
   the tombstone cool-down applies.

## Focused checks

- `memory_scope_shape_`: the kind and id checks, the external/system refusal, the live index with
  one unit and one principal namespace per owner, `subject_principal_id` on deliveries.
- `memory_scope_lifecycle_`: creating a department or principal provisions one namespace in the
  same transaction; deleting retires it; re-parenting and membership changes leave namespaces
  untouched; a re-created department gets a new id; the reconciler tests
  (`src/adapters/persistence/memory/lifecycle_tests.rs`) pass with the renamed kinds.
