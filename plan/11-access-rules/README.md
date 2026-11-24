# 11 Access rules: one answer per question, and agents that ask across units

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed, 2026-11-22. It comes from a read-only review of the access rules for people,
units, agents and workflows, and from the design discussion that followed. It follows the
organization plans [01](../01-departments/README.md) to [10](../10-blueprints/README.md). It adds
one structure, the request rule, and otherwise makes existing rules agree with each other. This
is an implementation plan; nothing here is implemented by these documents.

File anchors are on branch `plan08_ph1` at `42bc327e` (2026-11-22) with part 02 of
[08-goals](../08-goals/README.md) uncommitted in the working tree. Lines move; re-trace every
anchor with Trail before implementation.

## Intended behavior

**Each access question has one answer, written once.** Today several questions are answered in
more than one place and the copies differ. After this plan:

| Question | Answer |
| --- | --- |
| Who heads a unit | Its lead, or its sponsor decider when that decider is a person |
| Who manages a unit | The owner, a root admin, or a person who heads the unit or an ancestor |
| Who sees a unit's desk | Its members and role holders, the heads of its parent and of its direct children, and the owner |
| What a run reads | The unit of the desk it runs on and that unit's ancestors; the direct children of a unit the acting agent leads (05, unchanged) |
| Which tools a run may use | Resources of the run's unit and its ancestors (07, unchanged) |
| Who manages an agent | The manager of the agent's home unit |
| Who may ask a unit for work | Its members and role holders, a head one step up or down, and the units named in its request rules |
| How high a run may read | The level of the person who started it, of its run-as person, or of the least-cleared person on the thread; for a request, the lowest of the caller's ceiling, the rule's level and the asked desk's run-as level |

**An agent belongs to one unit**, its home. Only people who manage the agent can place it
anywhere else.

**Agents in different units cooperate by asking.** One agent sends a request to another unit's
desk or agent. The asked agent does the work with its own unit's knowledge, memory and tools, and
only its answer comes back. The asked unit decides who may ask and up to which level:

> Finance's head, on the Finance page: adds "Sales may ask, up to level 3".
>
> Sales agent, working a quote on the Sales desk: asks the Finance desk whether customer 42 is
> over its credit limit.
>
> Finance agent: runs on the Finance desk, reads Finance knowledge up to level 3, uses the Finance
> ledger connection, and answers "No. Limit 50k, used 12k."
>
> Sales agent: continues. Without the rule the request is refused, and the Sales agent is told so.

## Decisions (user, 2026-11-22)

1. **Agents cooperate across units by asking, never by reach.** No grant lets an agent read
   another unit's data or run another unit's tools. There is no case that needs it. A tool that two
   units both need moves to their common parent, which works today.
2. **The limit on asking is a unit and a level.** A request rule names the unit that may ask and
   the highest level the asked run may read.
3. **The hierarchy supplies the default rules; everything else needs a rule from the asked
   unit's manager.** A rule for a unit covers the units below it. The defaults are the one-step
   desk rule that already exists (members, role holders, a head one step up or down), so asking a
   desk and seeing a desk cannot disagree. The discussion sketched "any ancestor" for asking
   upward; the existing one-step rule is kept instead.
4. **One company-wide level scale. Level is the only label that travels with content.** A person
   or agent does not get a different level per unit. Accepted consequence: once an answer reaches
   another unit, any reader cleared to its level may later see it. "Finance to Sales only" cannot
   be expressed; a unit marks such data above the level of its rules.
5. **One definition of who heads a unit, and the one-step desk view follows it.** A person who
   is sponsor decider gets the same one-step desk grants as a lead.

## Proposed here, confirm before implementation

These are my recommendations. None was ruled on. Proposal 1 changes a contract decision of
another plan and blocks the cross-unit part of [03](03-requests-between-units.md).

1. **A call admitted by a request rule runs under the asked unit's authority.**
   [agent-workflow-call](../agent-workflow-call/README.md) decides that `channel.call` runs as the
   calling run's actor, who must hold operations authority, see the destination desk and be
   authorized for the destination's resources. That fits a caller with company-level authority.
   It does not fit decision 1: the person behind a Sales run has no authority in Finance, and
   giving them some is the reach that was ruled out. Proposed: the asked desk's binding runs as it
   is configured, as for a message arriving on that desk, and the caller is recorded as the
   requester. The request rule is the only way in; no actor is exempt, the owner included.

   **Confirmed by the user on 2026-11-22 and adopted in agent-workflow-call before the request
   rules exist.** Until 03 lands, a call is admitted when the calling agent may write to the
   destination desk under the existing internal-relay rule for an agent sender, and the asked
   run's ceiling is the lowest of the caller's ceiling, the allowance and the `run_as` person's
   level. Until 03C supplies the allowance from a rule it is level 0, so calls carry public data
   only; the user accepted that calls are of limited use until then. 03B item 3 replaces the write
   rule with the request rule. Authoritative text:
   `plan/agent-workflow-call/phases/p01/docs/logs/AWC-P01-D1-r3-decision.md`.
2. **An agent has one home unit.** Adding an agent to a unit, as member or role holder, needs a
   person who manages the agent as well as the destination. Today the destination's manager alone
   can add any agent of the company, and then edit, publish and equip it.
3. **Run control belongs to the manager of the run's unit.** Cancel and retry are answered three
   ways today: the application asks unit management, the database asks root management, and the
   docs say owner and admin.
4. **A unit's spend cap is written by its parent's manager.** This reverses the rule from
   [07-unit-resources](../07-unit-resources/README.md) that a unit's own manager sets it.
5. **One rule picks the default unit** for new knowledge, resources and workflow definitions.
6. **A binding or schedule is refused at save when its run-as person cannot see the desk**, so
   the failure is not left for the first occurrence.
7. **Pickers list only what the actor's units hold.**
8. **An agent's direct threads are read by their participants and the owner**, as a person's
   are, and a person writes to an agent's address under the same request rule as an agent.

Left open and not planned: a role falls back to the nearest ancestor's holder, while acting on a
desk needs the one-step desk view. Which refusal a distant holder meets was not traced. Trace it
before proposing a rule.

## What does not change

- The tenant boundary, the level scale, the release rule and the distance of the one-step rule.
- Scopes from 05: a run reads and writes by the desk it runs on.
- Resources from 07: a resource belongs to one unit and is usable from it and below.
- Who answers a unit's desk and who receives the second escalation hop: the lead.
- Role resolution and the run snapshot from 02.
- People's desk access. A request rule gives nobody a view of a desk.
- Rooms from [09](../09-rooms/README.md): the roster stays explicit and a request rule adds
  nobody to a room.

## Effect on plans that are not implemented yet

- [agent-workflow-call](../agent-workflow-call/README.md), 01 Batch B.2, B.3 and B.7: proposal 1
  and the ceiling rule of [03](03-requests-between-units.md). Record the outcome in that plan when
  proposal 1 is settled.
- [09-rooms](../09-rooms/README.md), design 8: "an authorized desk call" from a room is checked
  against the request rules with the room's unit as the asking unit.
- [10-blueprints](../10-blueprints/README.md): a unit's request rules are part of its setup.
  Adding them to the document is that plan's work if it lands after this one.

## Out of scope

- Direct reach grants, a level per unit, and a label naming the unit a piece of content came
  from. Decisions 1 and 4 rule them out.
- A spend limit per request rule. Work done for another unit is charged to the asked unit and
  bounded by that unit's caps.
- External senders and public desks.
- Memory service internals, native agent permissions and the attribution service. The review did
  not cover them.

## Documents

1. [Unit head](01-unit-head.md). After part 02 of 08-goals. Implements decision 5.
2. [Agent home](02-agent-home.md). After 06. Needs proposal 2.
3. [Requests between units](03-requests-between-units.md). After 01 and 02 of this plan. Its
   `channel.call` batch waits for agent-workflow-call and needs proposal 1.
4. [Authority corrections](04-authority-corrections.md). Independent batches, one per proposal
   3 to 8.
5. [Integration and final verification](05-integration-and-final-verification.md).

For each batch's new test family, first run `cargo test --locked --offline --lib <filter> --
--list` and assert a nonzero count, then run the family with an isolated `TEST_DATABASE_URL`.
Record exact commands, counts and results. Tests that sweep a table use `own_database`. Do not use
`ALLOW_MISSING_DATABASE_URL=1` as evidence. The full gate runs once, in 05.

Every file here carries the reset note and is registered in `NO_BACKWARD_COMPATIBILITY_NOTES` in
`src/main.rs`.

Planning validation, 2026-11-22: `python3 scripts/check-integration-docs.py`, `git diff --check`
and `rustfmt --check` on `src/main.rs` passed. Compilation and the database suites were not run
for this documentation change and its startup-path registration: another session held the build
with the final gate of 08-goals part 02.
