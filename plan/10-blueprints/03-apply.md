# 03 Apply

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A person applies a planned document in one commit. Every change is made by the code that already
owns that write, under that person's authority, and a replay, a race or a stale plan is refused
without leaving part of a setup behind. This phase covers units, members, roles, skills, agents,
desks, rooms and unit settings; bindings follow in
[04](04-procedures-bindings-and-schedules.md).

## Batch 03A The application record

1. `blueprint_applications` in the init schema, one row per applied document:
   `company_id`, `id`, the target unit's id and its label as written then, `origin` (`person` or
   `agent`), the actor's principal id, label and acting role, `command_key` and
   `command_fingerprint`, `document_digest`, `plan_digest`, a bounded `summary jsonb` with a
   version tag (operation kinds and slugs, no prompt text), and `created_at`. An `agent` row also
   carries the run, the action invocation and the proposing agent's principal and label.
2. It follows `department_administration_events` (`init_schema.sql:17213`): ids with labels
   captured at write time and no foreign key that blocks deleting a unit, a check that ties the
   agent columns to `origin`, `valid_acting_role` on the role, and a trigger that refuses update
   and delete. `UNIQUE (company_id, command_key)`.
3. Every `department_administration_events` row an apply writes carries the application's id as
   its `operation_id`, so one apply reads as one operation in the history 06 added. Other audit
   tables keep their own shape; the application row is what ties them together.
4. `summary` is persisted JSON and is decoded fallibly with its version, per
   `src/adapters/persistence/AGENTS.md`.

## Batch 03B The apply operation

1. An application port, `BlueprintApply`, with one purpose-specific method that takes
   `ApplyCommand { company, actor, origin, target, document, plan_digest, key }` and returns the
   application or a typed refusal. The actor is a person. An agent actor is refused before any
   lookup, as 06 decision 6 refuses it everywhere else.
2. The transaction starts as other management writes do: the actor's account before company
   authority (`begin`, `src/adapters/persistence/workflow/management.rs:11`), then the company row
   lock every department write already takes. Trace the real lock order of each owner in batch
   03C before composing them, and keep one order.
3. Under the lock it reads the state and references again and calls the same `plan` function. A
   digest that differs from the command's is `blueprint.stale`, with the fresh digest in the
   refusal so a client can show the new preview.
4. It then runs the operations in order. Each operation calls the existing `_on` function for
   that write with the acting person, so the owner's own authority recheck runs (06, batch 01A
   item 4). The blueprint module adds no second authorization rule and no second copy of a write.
5. All or nothing. `apply_on(tx, command)` does its writes inside a savepoint it owns. Any
   operation's refusal rolls the savepoint back and is returned naming the operation. The HTTP
   path then rolls back; the agent path commits its refusal receipt with no effect
   ([05](05-builder-agent-and-approval.md), batch 05B).
6. On success it inserts the application row in the same transaction.
7. Replays: a command whose `key` already has a row returns that row when the fingerprint
   matches and `blueprint.key_conflict` when it does not. The key is bounded as existing command
   keys are (`MAX_COMMAND_KEY_BYTES`, `src/application/workflow/provisioning.rs:275`).
8. Keep the `async` chain shallow, per `src/AGENTS.md`. The loop body is one helper that takes one
   operation and returns an outcome enum; planning stays synchronous; the calls that descend into
   agent publication and template compilation are `Box::pin`ned with a comment, as
   `ProvisioningEffects::apply` already does
   (`src/adapters/persistence/workflow/provisioning.rs:206`). Measure with
   `scripts/stack-frames.sh` before and after. If `scripts/stack-budget.sh` fails, shrink the
   chain; raising `STACK_BUDGET_KIB` needs the reason recorded, as the root `AGENTS.md` requires.

## Batch 03C Operations and their owners

The table records which existing function performs each operation and whose authority 06 batch
01B expects it to check. The owner function is the authority: where it differs from this table,
the function wins and the table is corrected.

| Operation | Owner today | Expected authority |
| --- | --- | --- |
| Create a unit | `create_department_with_id_on` (`department/lifecycle.rs:31`) | Manages the parent |
| Change a unit's name, description, charter or escalation default | `update_department_on` (`department/lifecycle.rs:126`) | Its own check |
| Archive or restore a unit | 08's lifecycle writes | Manages the parent |
| Add or remove a member | `add_department_member_on`, `remove_department_member_on` (`department.rs:546`, `:556`) | Manages the unit |
| Assign or clear a role, name a sponsor decider | `assign_department_role_on`, `clear_department_role_on` (`department.rs:343`, `:500`) | Manages the parent |
| Create or change a skill | The company skill writes (`src/application/use_cases/skill.rs:245`, `:287`) with 07's owning unit | Manages the owning unit |
| Create an agent | `create_agent_on` (`workflow/provisioning_builder.rs:17`), which also writes its principal and direct binding | Manages the unit it is placed in |
| Change an agent | The agent save and publication path (`src/application/use_cases/agent.rs`, `src/adapters/persistence/agent_publication/`) | Manages one of the agent's units |
| Create a desk or change its settings | The channel write with unit ownership (`set_desk_ownership_on`, `department_desks.rs:307`) | Manages the unit |
| Create a room, change its roster | 09's room commands | Manages the unit |
| Set a default model or a budget | 07's writes | Manages the unit |

1. Paths are under `src/adapters/persistence/` unless written in full.
2. An agent change is a new revision and its publication in the same transaction, so later
   admissions use it and runs already admitted keep what they froze.
3. A unit's own desk is created by the unit's creation (01 decision 8) and is not a separate
   operation.
4. Creating a unit provisions its memory namespace in the same transaction (05 decision 1); a
   blueprint needs no step for it.
5. Where an owner exists only as a use-case method that opens its own transaction, extract its
   body into an `_on(tx, ..)` function the use case and the apply both call. Do not copy it.

## Batch 03D Apply route

1. `POST /api/companies/{company_id}/departments/{department_id}/blueprint/apply` takes
   `{document, plan_digest, key}` from a signed-in person and returns the application id and the
   operations performed.
2. It answers `404` to anyone who does not manage the target unit, `409` with the fresh digest for
   a stale plan, and the operation's own refusal otherwise.

## Batch 03E Competing writers

Apply is a new writer of state that other writers change, so it gets competing-claimant tests and
not only sequential ones.

1. Two applies with one key: one application, both callers see it.
2. Two applies that create the same slug: one commits; the other is stale or a conflict and
   leaves nothing.
3. A product edit to a field the plan touches, between plan and apply: stale. An edit to an
   entity it does not touch: the apply commits.
4. The actor's role is removed while the apply waits for the company lock: the apply is refused
   whole or commits under the old role; never half.
5. An archive of the target racing the apply: a refused apply or a refused archive.

## Focused checks

- `blueprint_apply_`: each operation through apply leaves the rows the same write leaves through
  its own use case; an injected failure in a late operation leaves no unit, agent, skill, desk,
  principal or revision behind, in the manner of
  `autonomous_agent_provisioning_library_install_failure_rolls_back_portable_skills_and_principal`;
  an empty plan writes nothing.
- `blueprint_apply_authorization_`: owner, root admin, the target's lead, the parent's lead, a
  sibling's lead, a member, a stranger and an agent actor, against a document whose operations
  need different managers; the persistence recheck refuses when the use case is skipped.
- `blueprint_apply_idempotent_`: same key and fingerprint replays; same key with another document
  conflicts.
- `blueprint_apply_competing_`: the five cases of batch 03E with real concurrent transactions.
- `blueprint_roundtrip_`: export a unit that holds every carried kind with every field set,
  apply the document in a second company from `own_database`, export again, and compare. The test
  destructures each write type without `..` beside the exclusion list of
  [01](01-document-and-export.md), batch 01A item 8.
- `blueprint_application_record_`: one row per apply with the actor and acting role; the
  administration events share its id; rows refuse update and delete.
