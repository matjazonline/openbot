# 02 Plan and diff

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

One pure function turns a document and the live state into an ordered list of changes, or into
every reason the document cannot be applied. A person can preview a document against a unit they
manage. Nothing is written yet. Continues in [03](03-apply.md).

## Batch 02A Target and identity

1. `plan(document, target, state, references, origin) -> Result<ChangePlan, Vec<Refusal>>` in
   `src/application/blueprint/plan.rs`. It takes loaded data and returns a verdict: no `async`,
   no persistence, no clock. It is the "pure decisions, separately" rule of `src/AGENTS.md`, and
   it is unit-tested without mocks.
2. The target is an existing unit `T`. The document's root unit is placed by its slug:
   - it is `T`'s slug: the document describes `T` in place;
   - it is the slug of a direct child of `T`: it describes that child in place;
   - no address in the company uses it: it is a new child of `T`;
   - anything else is `blueprint.conflict`.

   So a document exported from a unit applies at that unit or at its parent, and a shared document
   becomes a new child of the unit the importer chooses.
3. Identity is the slug. A unit, desk, room, agent or person is found through the company address
   namespace (`addresses`, 03); a skill through the skill slug. A slug held by another kind of
   thing is `blueprint.conflict`, naming both.
4. A document may reference anything the target may use and may define only what lives in the
   target subtree. Defining an agent that belongs to no unit of the subtree, a skill owned outside
   it, or a desk owned outside it is `blueprint.outside_target`.
5. A document that places an existing unit under a different parent, or an existing desk, room or
   skill under a different owning unit, is `blueprint.move_unsupported`. Moves stay in the product.
6. `origin` is `Person` or `Agent { own_unit, selected_agents }`. The agent rules are in
   [05](05-builder-agent-and-approval.md), batch 05B; they are refusals of this function, so a
   preview and an apply cannot disagree about them.

## Batch 02B What a document means

1. For each entity the document names: absent means create; present means compare the fields the
   document states and update the ones that differ; equal means nothing.
2. A stated field is authoritative. That includes lists: stated `members`, role holders, an
   agent's tools and skills, a room's roster, aliases and `budgets` replace the live list, so the
   plan shows each addition and each removal.
3. An unstated field is untouched, and so is an entity the document does not name. `Operation`
   has no variant that deletes a unit, agent, skill, desk, room, procedure or binding.
4. `archived: true` on an active unit plans 08's archive; `archived: false` on an archived unit
   plans its restore. The refusals of 08 batch 02B, open work among them, come from the archive
   write itself at apply.
5. An agent written under a unit is a member of that unit. If the unit also states `members`, the
   agent is added to the stated list rather than refused.
6. Operations are ordered so each one's prerequisites exist: units, parents first; skills; agents;
   members; roles; desks and rooms; procedures, bindings and schedules
   ([04](04-procedures-bindings-and-schedules.md)); unit settings; archives last. Trace what
   creating a unit with a lead does to the unit's own desk (01 decision 8) and order the lead
   assignment so the result equals creating the unit with that lead in the product.
7. `ChangePlan` holds the operations and `plan_digest`: SHA-256 over canonical JSON of the format
   version, the document digest, the target unit id and the operations. An operation carries the
   ids of the existing rows it touches or references and, for an update, the value it replaces.
   A slug that points somewhere else by apply time, or a touched field that someone changed,
   therefore changes the digest; a change to an entity the plan does not touch does not.
8. An empty plan is a valid result: the unit already equals the document.

## Batch 02C References, authority and refusals

1. `BlueprintReads::references(company, target, names)` resolves every name the document uses and
   does not define, against what the target unit may use: active company principals; skills and
   connections with their operations owned by the target or an ancestor (07 decision 1); models
   that are ready (`ready_models_on`, `src/adapters/persistence/workflow/provisioning.rs:406`);
   shipped templates (`TemplateCatalogue`, `src/application/workflow/templates/catalogue.rs:81`).
   A sibling unit's skill or connection is not found, by 07's rule.
2. Every missing name is reported, not the first: one `blueprint.unknown_reference` per name with
   its path in the document. This list is what someone adapting a shared document works from.
3. Each operation records the management target its owner checks, from the table in
   [03](03-apply.md), batch 03C. Given a viewer, the plan marks the operations that viewer could
   not perform. The marks are for the preview; apply is the check.
4. Refusals are collected up to 64, each `{code, path, message}`, and the result says when it
   stopped at the bound. The codes: `blueprint.invalid`, `blueprint.conflict`,
   `blueprint.outside_target`, `blueprint.move_unsupported`, `blueprint.unknown_reference`,
   `blueprint.root_membership`, and the agent-origin codes of 05. `unit.archived` from 08 is
   reused for a target or parent that is archived.

## Batch 02D Preview route

1. `POST /api/companies/{company_id}/departments/{department_id}/blueprint/plan` takes a document
   as JSON or YAML and returns `{plan_digest, operations, refusals}`. The body is bounded at the
   document bound before parsing.
2. It requires managing the target unit and answers `404` otherwise. It runs in one read-only
   transaction: state and references are read in the same snapshot.
3. The response is bounded. An operation's short values are returned whole. A long text, such as
   a prompt, skill instructions or procedure source, is returned as before and after text only
   when the pair is under 64 KiB, and as two digests with byte counts otherwise.

## Focused checks

- `blueprint_plan_`: the pure matrix. Create, update and no-op for each entity kind; a stated list
  replaces and an unstated one is untouched; no operation removes an unnamed entity; archive and
  restore; operation order; each placement of the root slug; conflict, outside-target and move
  refusals; root restrictions; the refusal bound.
- `blueprint_plan_digest_`: equal inputs give equal digests; a changed touched field, a changed
  reference target and a changed document each change it; a change to an untouched entity does
  not.
- `blueprint_plan_roundtrip_`: for a generated set of states, `plan(export(state), state)` is
  empty. This is the half of the round trip that needs no database.
- `blueprint_plan_references_`: with a database, every missing person, skill, connection, model
  and template is reported together; an ancestor's skill resolves and a sibling's does not; an
  archived target is refused.
- `blueprint_plan_authorization_`: the route answers `404` to a sibling's lead, a member, a
  stranger and an agent actor; a unit's lead sees the operations that need its parent's manager
  marked.
