# 04 Authority corrections

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

The remaining mismatches from the review are closed, one batch per proposal of the
[README](README.md). Each batch stands alone and is skipped when its proposal is declined.
Follows [03](03-requests-between-units.md); continues in
[05](05-integration-and-final-verification.md).

## Batch 04A Run control (proposal 3)

Cancel and retry are answered three ways. `LifecycleAuthorizer::authorize`
(`src/application/workflow/authorization.rs:263`) asks for management of the desk's unit and a
view of the desk. `control` (`src/adapters/persistence/workflow/controls.rs:71`) calls
`authorize_company` (`src/adapters/persistence/workflow/authority.rs:46`), which asks for
management of the root. The docs and `administers_company`
(`src/domain/entities/company_member.rs:88`) say owner and admin.

1. A person may control a run when they manage the run's unit and can see its desk. A run with
   no desk belongs to the root. The application check and the locked database check ask the same
   function.
2. `authorize_company` also sits under local agents, memory settings, the usage report,
   escalation reassign and cancel, workflow tools and resource use. Before changing it, list each
   caller with the gate above it, and sort each into one of three: scoped to a unit, the root's
   manager, or company administration. [06](../06-delegated-administration/README.md) gave memory
   settings to the root's manager; the docs give them to the owner and admins. Make the code and
   the docs say the same thing for every caller and record the choice here.
3. Views that list runs across the company (`inspectable_runs`,
   `src/adapters/persistence/thread_activity.rs:36`, and the team work view) stay with company
   administration.

Checks: `run_control_` matrix over owner, root admin, head of the unit, head of an ancestor, head
of a sibling, and a manager without a view of the desk; a database test that the locked recheck
gives the same answer as the application check for each.

## Batch 04B Spend caps (proposal 4)

`authorize_on` (`src/adapters/persistence/unit_budgets.rs:43`) asks for management of the unit
itself, so a unit's own head can raise the cap its parent set for it. Spend is still charged up
the ancestor chain, so the parent's own cap holds; what is lost is the parent's bound on one
child.

1. Writing a unit's cap needs the parent's manager, as role writes do. The root's cap needs the
   owner or a root admin. A unit's manager still reads its cap and spend.
2. This reverses the rule from 07 that a unit's manager sets its caps. Confirm before starting.

Checks: `unit_budget_authority_` over the unit's own head (refused), the parent's head, the
root.

## Batch 04C One default unit (proposal 5)

Three rules pick a default unit today. Knowledge uses the person's single placement, or the root
for an owner or admin with none (`src/adapters/persistence/knowledge/permissions.rs:95`).
`resource_default_unit` (init schema, line 16061) uses the root for a person who manages it, else
the one unit they head. `workflow_creating_department` (20608) uses the single placement below
the root, else the root, and the result cannot be changed later.

1. One function: the root for a person who manages it; else the single unit they head; else
   their single membership below the root; else none, and the form asks.
2. Knowledge, resources and workflow definitions use it. A definition no longer falls back to
   the root for a person who does not manage the root.
3. `docs/departments.md` says the unit is chosen at creation. Say what the code does.

Checks: `default_unit_` matrix shared by the three callers.

## Batch 04D Run-as must see the desk at save (proposal 6)

`requires_human_view` (`authorization.rs:112`) is false for managing a binding or a schedule, by
design: a manager configures without reading conversations. An occurrence later needs the run-as
person to see the desk, and fails then. The lead desk already refuses a lead who cannot see it.

1. Saving a binding or a schedule checks that its run-as person can see the desk, and refuses
   with a named reason. Company-wide bindings are exempt. The actor's own view is still not
   required.

Checks: `binding_run_as_view_`: save refused for a run-as person with no view; accepted after
they are added; a view lost later still refuses the occurrence as today.

## Batch 04E Pickers (proposal 7)

`binding_options` (`src/adapters/persistence/workflow/authoring_reads.rs:365`) lists every desk
of the company to anyone who manages a unit.

1. Desks: those of the units the actor manages.
2. People: confirm separately. A unit's manager must be able to choose a person from elsewhere
   to add them, and a list of everyone is also a roster the unit pages do not show them.

Checks: `picker_scope_`: a unit's head sees its own subtree's desks and no sibling's.

## Batch 04F Agent threads and person senders (proposal 8)

`direct_viewer_access` (`src/domain/entities/participant.rs:86`) lets every team member read
every agent's direct threads.

1. An agent's direct thread is read by its participants and the owner, as a person's is.
2. A person writes to an agent's address when `request_allowance` admits them, with the units
   they belong to as the asking units.

Checks: `agent_thread_view_`: a non-participant is refused; `request_person_`: a person in an
unrelated unit is refused with no rule and admitted with one.
