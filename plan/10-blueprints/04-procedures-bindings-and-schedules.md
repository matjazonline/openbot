# 04 Procedures, bindings and schedules

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A document says what each desk does: who answers it, which procedure runs on it, with which
parameters and on which schedule. A company's own procedures travel with the document, so an
exported setup works when it is recreated. Continues in
[05](05-builder-agent-and-approval.md).

A procedure here is a workflow definition, as 06 decision 3 uses the word.

## Batch 04A Desk bindings

1. A desk in a document may state `bindings`. Each takes one of three forms:
   - `responder: {agent, reply, steps}`: the agent answers the desk, autonomously or reviewed,
     with optional custom steps. This is what `create_desk_for_agent` provisions today
     (`provision_guarded`, `src/adapters/persistence/workflow/provisioning.rs:481`), with the same
     limits: the step types of `BUILDER_STEP_TYPES` and at most `MAX_CUSTOM_STEPS`
     (`src/application/workflow/templates/agent_workflow.rs:27`).
   - `template: {name, parameters, trigger}`: a shipped procedure, named as the template catalogue
     names it (`TemplateCatalogue`, `src/application/workflow/templates/catalogue.rs:81`),
     published for the company if it is not yet, and bound. The role-based samples
     (`role-lead-response.yaml`, `department-lead.yaml`, and 08's `goal-plan-approve-dispatch.yaml`)
     are what let a new unit work without authoring; trace which of them the catalogue offers.
   - `procedure: {id, parameters, trigger}`: a procedure of this company, by definition id, bound
     at its current published version.
2. A unit's own desk is the desk whose slug is the unit's. Listing it configures its bindings;
   omitting it leaves what creating the unit gave it.
3. A binding is identified by its desk and what it binds: the responding agent, the template, or
   the definition. A document that states two bindings of one procedure on one desk is refused
   with `blueprint.ambiguous_binding`, and export lists such a desk in `omitted`.
4. `active` is stated per binding. Apply configures the binding and then activates or pauses it
   through the binding lifecycle (`src/application/workflow/lifecycle/bindings.rs`). No operation
   deletes a binding.
5. `run_as` names a person by slug. Unstated on a new binding, it is the acting person, which is
   what provisioning records today (the binding revision's actor is the requester,
   `delegated_provisioning_locks_requester_before_company`).
6. A binding whose procedure names a role that the desk's unit chain cannot fill is refused by the
   existing binding configuration (02 decision 3). Plan reports that refusal with the document
   path of the binding.
7. Bindings that exist because something else does are not listed: an agent's direct binding
   comes with the agent (03 decision 6) and a room's response bindings come with its roster (09).
8. Authority is the existing one: configuring a binding needs the manager of the desk's unit, and
   binding a procedure to a desk outside the units its manager manages is refused with
   `binding.outside_managed_units` (06, batch 01B items 3 and 4).

## Batch 04B Schedules

1. A binding may state `schedules`, with the fields the schedule form writes. Trace the schedule
   tables and the schedule management use case for the field list, identity and bounds, and record
   them here before implementing.
2. Apply creates, changes and pauses schedules through that use case. It deletes none. Schedule
   management authority is unchanged
   (`schedule_management_requires_an_owner_or_admin_who_sees_the_channel` and its 06 successors).
3. A schedule on a desk of an archived unit is refused with `unit.archived` (08, batch 02B).

## Batch 04C Procedures carried in the document

1. A unit may state `procedures: [{id, title, description, source}]`. The unit is the procedure's
   managing department (`workflow_definitions.managing_department_id`). `source` is workflow YAML
   v1 text under its existing bound (`MAX_SOURCE_BYTES`); the whole document stays under the
   document bound.
2. Apply saves the source as the definition's draft, validates and compiles it, and publishes a
   version, through the definition lifecycle the editor uses
   (`src/application/workflow/lifecycle/definitions.rs`) and under its authority (06, batch 01B
   item 4). A `procedure` binding in the same document binds the version this apply published.
3. Identity is the definition id. Trace whether ids are unique per company or globally: if a
   shared document's id can collide with another company's, apply mints a new id for a definition
   the target company does not have and rewrites the document's bindings to it within the plan.
4. A stated source equal to the current published source is no operation. A different one is one
   operation: a new revision and its publication. The preview shows the source as a text diff.
5. Export writes the procedures managed by the subtree's units, each with its current published
   source. It lists in `omitted` what the format does not say: unpublished drafts, and bindings
   pinned to a version older than the current one.
6. An invalid source refuses the whole apply with the compiler's diagnostics and the document
   path. Nothing is published from a document that does not fully apply.
7. If proposal 2 of the [README](README.md) is confirmed, a plan with origin `Agent` refuses any
   `procedures` entry with `blueprint.procedure_source`. Agent-authored documents then bind
   templates and existing procedures and add `responder` steps only.

## Focused checks

- `blueprint_binding_`: each of the three forms through apply leaves the binding, version and
  state the product's own path leaves; a paused binding stays paused; `run_as` defaults to the
  actor; the unit's own desk is configured in place; an ambiguous pair is refused.
- `blueprint_binding_roles_`: a role-based template bound on a new unit's desk resolves the unit's
  lead at admission, with the scripted-model `Deployment` harness; a unit chain that cannot fill
  the role is refused at plan.
- `blueprint_schedule_`: create, change and pause; no delete; an archived unit is refused.
- `blueprint_procedure_`: a carried source publishes the same compiled version the editor
  publishes for it; an unchanged source plans nothing; an invalid source refuses the whole apply
  and leaves no draft; a binding outside the managed units is refused; an agent-origin plan with
  a `procedures` entry is refused, if proposal 2 is confirmed.
- `blueprint_procedure_competing_`: a publication in the product racing an apply that carries the
  same procedure leaves either a stale apply or the apply's version, never two half-written
  revisions.
- `blueprint_roundtrip_`, extended: a unit whose desks use all three binding forms, a schedule
  and a carried procedure exports, applies in a second company and exports equal.
