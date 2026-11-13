# 08 Goals: a temporary unit with a sponsor

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed, 2026-11-13. Eighth of the organization plans. It has two start points:
[01](01-sponsor-role-and-escalation-default.md) starts when
[02-workflow-roles](../02-workflow-roles/README.md) is complete and depends on nothing later;
[02](02-unit-lifecycle.md) and [03](03-goal-units-and-tracking.md) start after
[07-unit-resources](../07-unit-resources/README.md), and 03 also needs `channel.call` from
[agent-workflow-call](../agent-workflow-call/README.md).

## Intended behavior

Departments are standing capabilities: repeatable work that benefits from its own knowledge,
memory, tools and budget. A goal is the other kind of work: it has an end, and it usually needs
several departments. This plan adds goals without adding a second structure. A goal is a unit in
the same tree, with three things a standing department does not need:

- a **sponsor**: the actors accountable for it, one of whom decides;
- an **end**: it is archived when done, which stops new work and keeps the history;
- a **trail**: every run done on its behalf, in any department, is recorded against it.

The recommended shape, which the product does not enforce, is one standing department for the
people and agents who plan and drive goals, with one goal unit under it per goal:

```
Root
├── Goals office          standing. PM agents and a human head; holds the planning memory.
│   ├── Launch X          goal. lead = a PM agent, sponsor = the accountable people.
│   └── Cut churn         goal. Archived when done.
├── Sales                 standing
└── Support               standing
```

A goal run starts on the goal's desk, where the lead plans, asks the sponsor to approve with
`decision.human`, and dispatches work to department desks with `channel.call`. The department does
the work under its own procedures, memory and budget, so what it learns stays with the department.
The goal sees the status of everything done for it and what it cost. A goal that needs one
department only may sit under that department instead of under the goals office.

## Decisions (user, 2026-11-13)

1. **A goal is a unit, not a new entity.** What a goal needs beyond a department is an end, so
   archiving becomes an option of every unit and goals reuse departments for everything else:
   desks, members, roles, memory, knowledge, resources and budgets.
2. **One goals department, one sub-unit per goal.** This is a convention the docs recommend and a
   sample sets up. Nothing in the schema names a goals office; a goal may sit under any unit.
3. **Human approval is `decision.human`.** The step exists and parks the run for its reviewers. A
   goal procedure names the sponsor role as the reviewer; no new approval mechanism is added.
4. **`sponsor` is a role held by several actors, one of whom decides.** Holders are person or agent
   principals. Exactly one holder is the decider while the role has holders; the others are
   informed. A step that names `sponsor` resolves to the decider.
5. **Who is told when work stalls is configurable per unit.** A unit sets a default escalation
   policy that its desks inherit; a binding's own policy still wins.
6. **Everything is tracked by goal.** Each run records the goal it serves, so status and analysis
   read one column and not a reconstruction.
7. **This follows 02 and does not amend it.** 02 is being implemented; the sponsor work starts
   when 02 is complete.

## Proposed here, confirm before implementation

These were raised in the design discussion and not ruled on.

1. **The step decides what the decider must be.** `decision.human` with `role: sponsor` needs a
   person decider and refuses admission otherwise (`workflow.role_wrong_kind`), because a human
   approval that an agent can fill is not one. `decision.agent` and `agent.run` with
   `role: sponsor` need an agent decider. A step that takes either kind is not added.
2. **A person who is the decider manages the unit.** `manages_unit` from
   [06-delegated-administration](../06-delegated-administration/README.md) is true for the sponsor
   decider of the unit or an ancestor, when that decider is a person. This gives a goal led by an
   agent a human who can configure it. An agent decider does not administer, as an agent lead does
   not.
3. **Archiving refuses while work is open.** A unit with runs that are not finished, on its desks
   or recorded against it, cannot be archived until they finish or are cancelled.
4. **A goal's budget counts the work done for it anywhere.** A cap on a goal unit bounds the spend
   of every run recorded against the goal, in addition to the caps of the unit that does the work.
5. **A goal with recorded work cannot be deleted, only archived.**

## Why two start points and not three

The first sketch put archiving after 04-root-unit. It moves to after 07 because an archived unit
has to be respected by memory and knowledge (05), by authority (06) and by resources and budgets
(07). Built after 04 it would be reopened three times; built after 07 it is written once. The
sponsor role and the escalation default are pure extensions of 02 and need none of that, so they
are not held back.

If goals are wanted sooner, the smallest early slice is 02A and 02B of
[02](02-unit-lifecycle.md), which need only 04, and 03A of
[03](03-goal-units-and-tracking.md), which needs `channel.call`. Taking it means 05, 06 and 07 each
add the archived-unit rule when they land; record that choice here if it is made.

## What does not change

- The tenant boundary, access levels, desk read policy and the one-step rule.
- Role resolution, the run snapshot and the escalation ladder from 02; this plan adds a role, a
  default and a refusal kind, and changes no existing resolution.
- Scopes from 05: a run reads and writes by the desk it runs on. Work done for a goal in Sales
  reads and writes Sales memory, not the goal's.
- `channel.call`: its authority checks, closure and deadline. The goal trail rides on its child
  run and thread and adds no parameter the model can set.
- A standing department. With no sponsor, no archive and no goal it behaves exactly as before.

## Out of scope

- Voting or quorum among sponsors. One actor decides.
- A debrief step that copies lessons from a closed goal to its parent. The parent's lead already
  reads its direct children's memory (05, decision 4); the procedure that does the debrief is
  [standard-operating-procedures](../standard-operating-procedures/README.md) work.
- Goal templates, dependencies between goals, and scheduling views.

## Documents

1. [Sponsor role and escalation default](01-sponsor-role-and-escalation-default.md). After 02.
2. [Unit lifecycle](02-unit-lifecycle.md). After 07.
3. [Goal units and tracking](03-goal-units-and-tracking.md). After 02 of this plan and
   `channel.call`.
4. [Integration and final verification](04-integration-and-final-verification.md).
