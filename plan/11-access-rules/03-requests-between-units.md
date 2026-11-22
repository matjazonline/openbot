# 03 Requests between units

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A run in one unit can ask a desk or an agent in another unit when the asked unit allows it, and
the asked run reads no higher than the rule allows. Implements decisions 1 to 4 of the
[README](README.md). Follows [01](01-unit-head.md) and [02](02-agent-home.md); continues in
[04](04-authority-corrections.md).

## Where it stands

- An agent's own address accepts any sender of the company (`direct_access` in
  `src/domain/entities/participant.rs`). A members-only desk accepts an agent that holds a grant.
- Agents have no level. `reader` (`src/adapters/persistence/workflow/knowledge.rs:21`) counts an
  agent as no reader, and an audience with no reader is level 0 (`AccessLevel::lowest`,
  `src/domain/entities/access_level.rs:53`). A message-started run takes its ceiling from the
  thread's audience (`admission_levels_on`, `knowledge.rs:113`), so a request between two agents
  with no person on the thread reads public data only, and a request labelled above level 0 is
  refused. This was read in the code, not run.
- A child run copies its parent's ceiling (`insert_child_on`,
  `src/adapters/persistence/workflow/children.rs:328`).
- `channel.call` is planned in [agent-workflow-call](../agent-workflow-call/README.md) and has no
  producer in the code yet.

## Batch 03A The rule and the allowance

1. Table `unit_request_rules`: the unit being asked, the unit that may ask, and `max_level`.
   One row per pair. Both units are tied to the company in the foreign keys. A unit cannot name
   itself. At most 64 rules per asked unit.
2. The manager of the asked unit writes its rules, rechecked under the lock and recorded in the
   administration audit like other management writes. `max_level` cannot exceed the writer's own
   level, the rule `may_assign_level` already applies to people's levels
   (`access_level.rs:116`).
3. One SQL function, `request_allowance`, takes the sending principal, the asking unit and the
   asked unit, and returns the highest level the request may read, or nothing when the request is
   refused:
   - the same unit, a sender who is a member or role holder of the asked unit, or a sender who
     heads the asked unit's parent or one of its direct children: level 9, the rule of
     [01](01-unit-head.md) Batch 01B;
   - otherwise the highest `max_level` among the asked unit's rules that name the asking unit or
     one of its ancestors;
   - otherwise nothing.
   An archived unit on either side allows nothing.
4. The asking unit of a run is the unit of the desk it runs on. For a run with no desk it is the
   acting agent's home ([02](02-agent-home.md), Batch 02A).

## Batch 03B Where it is checked

1. **An agent writes to another agent's address.** `direct_access` accepts a proved agent sender
   only when the allowance for the target agent's home is not nothing. Person senders are
   unchanged here; proposal 8 covers them.
2. **An agent writes to a members-only desk.** Accepted when the sender holds a grant, as today,
   or when the allowance for the desk's unit is not nothing. Public desks are unchanged.
3. **`channel.call`**, when agent-workflow-call is implemented. This step needs proposal 1.
   The call is admitted only when the allowance for the destination desk's unit is not nothing,
   with the calling agent as sender and the calling run's unit as the asking unit. The child then
   runs the destination's selected binding as it is configured, with its own `run_as`, agent and
   resources. The caller's actor is recorded as the requester and gains no access to the
   destination. Missing and refused destinations stay indistinguishable to the caller, as that
   plan requires, and the model receives its typed `denied` closure.
4. **A rule that is removed or lowered** refuses new requests. For a call already running it is
   the loss of caller access that agent-workflow-call 01 Batch B.4 describes: no further
   protected effects and no result disclosure.

A request is stored on the asked desk in a thread of its own. The asking side reads nothing else
on that desk, and a request rule creates no desk grant.

## Batch 03C Levels

1. The asked run's ceiling is the lowest of the calling run's ceiling, the allowance, and the
   level of the asked binding's run-as person.
   - For a `channel.call` child, `insert_child_on` writes that value in place of a copy of the
     parent's ceiling.
   - For a relayed message from an agent, the relay records the sending run's ceiling and the
     allowance on `message_internal_relays` (init schema, line 19240). The message arm of
     `admission_levels_on` uses them, together with any people on the thread, when the trigger is
     an internal relay. A thread with only agents on it is then no longer level 0.
2. A request whose own content is above that ceiling is refused before anything is stored, with
   the code `request.above_level` and both levels named. The author raises the rule or asks from
   a run that has read less. This is the existing `content > ceiling` refusal with a reason the
   asking side can act on.
3. The answer is labelled by what the asked run read and raises the calling run's content level,
   as a child's result does today. The release rule then decides where the caller may send it.
4. Nothing records which unit content came from.

## Batch 03D Pages

1. The unit page gains "Requests from other units": the rules, a form to add one with a unit
   and a level no higher than the actor's own, and remove. Managers of the unit see it.
2. `docs/custom_tools.md` and `docs/agent-builder-context.md` say that a refused request returns
   a reason code and never a principal or a unit name the caller could not already see.

## Focused checks

- `request_rule_`: a rule across companies is refused by the foreign key; a unit naming itself
  and a 65th rule are refused; only the asked unit's manager writes; a level above the writer's
  is refused; each write leaves an audit row.
- `request_allowance_`: SQL matrix over the same unit, a member, a head one step up and one step
  down, a rule on the asking unit, a rule on its ancestor, a sibling and a cousin with no rule,
  an archived unit on each side, and the highest of several rules.
- `request_relay_`: an agent writing to another unit's agent address is refused with no rule and
  admitted with one; the same for a members-only desk; person senders behave as before.
- `request_level_`: the ceiling is the lowest of the three; an agent-only thread reads above
  level 0 when the relay carries a ceiling; a request above the ceiling is refused with
  `request.above_level`; the answer raises the caller's content level.
- `request_competing_`: a rule removal racing an admission leaves the request refused, or
  admitted with the old rule's ceiling, in both orders.
- `channel_call_authority_` in agent-workflow-call gains the request-rule cases and a control
  showing that an owner's run gets no exemption.
