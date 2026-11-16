# 09 Rooms: people and agents share a conversation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed, 2026-11-16. Ninth and last in the numbered organization sequence, after
[08-goals](../08-goals/README.md). This is an implementation plan; rooms are not implemented by
these documents. The product request is shared rooms with people and agents, using mentions to
address participants. The detailed defaults below are planning choices, not previously confirmed
user decisions.

## Intended behavior

A desk receives work and its configured workflows handle it. A room holds conversations among a
chosen group of people and agents. Direct messages address one principal. All three use the
existing canonical messages, threads, identities and workflow runtime.

For example, a Product launch room contains two people, Researcher and Writer:

> Person: @Researcher check competitors' pricing.
>
> Researcher: Here is the comparison. @Writer draft our positioning from these findings.
>
> Writer: Here is a draft.
>
> Another person, replying to Writer: Focus on small businesses.

Each agent responds under its own identity in the same thread. People can speak at any time.
Messages without an addressed agent start no agent work. A mention of a person notifies them.

## Review of the preceding plans

The READMEs and numbered phase documents of 01–08 were reviewed for this plan. Earlier execution
records qualify their original proposals: 01/02 have acceptance records, 03 has accepted phase
handoffs and an integrated verification phase, and 04–08 describe later contracts. Do not infer
implementation status from an older README's proposal date or treat a pending gate as passed.

| Plan | Contract rooms build on | Consequence here |
| --- | --- | --- |
| [01 Departments](../01-departments/README.md) | Units group principals and own desks; agents and people are principals | A room belongs to one unit and admits principals from several units |
| [02 Workflow roles](../02-workflow-roles/README.md) | Published workflows, frozen agent selections, human decisions and escalation | Every agent turn remains an ordinary authorized workflow run |
| [03 Principal addresses](../03-principal-addresses/README.md) | One address namespace, direct threads and actual principal authors | Mentions resolve stable principals; a room turn stays on the room thread |
| [04 Root unit](../04-root-unit/README.md) | Every channel has a unit; desk access derives from that unit | Room membership is explicit; department-derived desk grants do not populate rooms |
| [05 Scopes](../05-scopes/README.md) | One frozen resolver for memory and knowledge; no desk namespace | Room runs use the room's unit; a room owns history, not a new memory namespace |
| [06 Delegated administration](../06-delegated-administration/README.md) | People manage units; management does not confer conversation access | The unit's manager configures the room; its roster grants reading and participation |
| [07 Unit resources](../07-unit-resources/README.md) | Resource selection and usage follow run scopes and the write unit | Room work uses eligible resources and is charged to its owning unit |
| [08 Goals](../08-goals/README.md) | Goal attribution and unit archive apply across work | Rooms in goal units carry the goal; queued turns count as open work |

The accepted 01 coordination record explains that `message.send` to another desk did not start its
work and returning relay traffic could be refused as a cycle. Rooms therefore use addressed
admission on one shared thread. They do not implement discussion by forwarding through each
agent's direct inbox. [agent-workflow-call](../agent-workflow-call/README.md) remains the mechanism
for delegating work to a desk under that desk's scopes and returning its reviewed result. Its
separate destination thread is appropriate for delegation, not for each conversational turn.

Plan 08 has open proposals. Consume the outcomes recorded when 08 is implemented; this plan does
not approve them implicitly or introduce a second sponsor, archive or goal-budget policy.

## Design

1. **A room is a channel kind.** Add `ChannelKind::Desk | Room`, fixed at creation. Keep channel
   ids, thread targets and transport bindings. The address registry continues pointing to the
   channel; product labels distinguish its kind. Do not add a third thread-target column or a
   separate message store. A department's automatically provisioned desk remains a desk.
2. **One owning unit, an explicit roster.** A room may contain active company people and agents
   from different departments. Roster membership grants view and participate through the existing
   grant mechanism. The owner exemption remains. Department membership, parent leads and unit
   management alone do not grant room reading. Initial rooms have no public ingress or external
   guests. Adding someone exposes existing history they may read under the content-level policy;
   the membership UI states this. It does not add them to the owning unit.
3. **An address identifies a member.** The `@` picker stores a principal id and display span.
   Text transports resolve a bounded `@slug` against current room members at acceptance. Names
   are presentation, not keys; a rename cannot retarget a saved mention. Unknown handles remain
   ordinary text, with unresolved-mention feedback in the composer.
4. **Addressing determines participation.** Explicit mentions take precedence over implicit reply
   addressing. A person's reply with no mentions addresses the author of the replied-to message.
   An agent's ordinary response has reply provenance but does not implicitly invoke that author;
   an agent must explicitly address another member. Repeated targets are deduplicated, self
   mentions start no work, and quotes, code, attachments and tool results are not executable
   mention sources. There is no broadcast mention in this delivery.
5. **One active agent turn per thread.** Multiple mentioned agents receive turns in first-mention
   order. Later requests append in commit order. The next agent sees earlier committed replies
   under the existing history/token rules. Humans can post while an agent is working; a prepared
   run does not silently absorb later messages. Separate threads can progress concurrently.
6. **Each agent has its own room response binding.** Reuse autonomous workflow provisioning and
   publication. The binding targets the room and names its responding member; the member's direct
   binding stays separate. Room bindings share one configured automation actor, frozen for each
   discussion; a handoff cannot switch actors. Only addressed bindings are admitted, with immutable
   selections and existing approval/live authority checks. The roster never grants a tool or
   permission to impersonate another actor.
7. **One bounded discussion per human request.** Agent-generated turns retain their initiating
   human request, shared turn/token allowance and absolute deadline. Retries and handoffs cannot
   reset them. Unit and goal budget checks also apply. Any participating person can pause agents
   in a thread; this fences queued and active work. Resuming permits new requests, without
   automatically replaying cancelled ones.
8. **Room work belongs to the room's unit.** Use 05's resolver with the selected agent and the
   room's owning unit, including that resolver's existing personal-memory and lead rules. Do not
   union the departments, knowledge or connections of everyone in the room. An agent whose
   published dependencies are unavailable here cannot run; show the refusal. Working in another
   department's scope uses an authorized desk call.
9. **Identity and publication remain separate from execution.** Only approved, publishable replies
   enter the shared timeline, authored by the actual selected principal. Private agent exchanges,
   tool transcripts and direct-message history are not the room transcript. Human review, withheld
   replies and `no_reply` keep their current meaning.

## Delivery boundary

Deliver room management, shared threads, in-product and existing email participation, human
notifications, addressed agent turns, pause/resume, and unit/goal integration together. Email uses
the company's existing transport and authenticated identity mapping; an email mention does not
send a second copy to the principal's direct address. An attachment or quoted mention cannot
start work. Additional chat-provider adapters can later map native mentions into this contract;
their installation is not a prerequisite.

Automatic whole-room debate, broadcast mentions, guest invitations, inferred intent routing and
new room-specific tools or memory services are outside this delivery. No LLM router is needed to
decide who a message addresses.

## Documents and verification

1. [Room ownership, membership and access](01-room-membership-and-access.md).
2. [Mentions, replies and notifications](02-mentions-and-addressed-messages.md).
3. [Agent turns and durable ordering](03-agent-turns-and-ordering.md).
4. [Scopes, budgets and goal lifecycle](04-scopes-budgets-and-lifecycle.md).
5. [Room UI, transport and documentation](05-ui-transport-and-docs.md).
6. [Integration and final verification](06-integration-and-final-verification.md).

Execute in order after 08. Before implementation, re-read the accepted predecessor contracts and
trace the affected symbols with Trail; proposed identifiers below describe ownership, not claims
that those symbols already exist. Use the existing runtime and purpose-specific persistence ports.

For each batch's new test family, first run
`cargo test --locked --offline --lib <filter> -- --list` and assert a nonzero count, then run
`cargo test --locked --offline --lib <filter>` with an isolated `TEST_DATABASE_URL`. Record exact
commands, selected/executed counts and results. Unscoped queues/sweeps use `own_database`.
Do not use `ALLOW_MISSING_DATABASE_URL=1` as evidence. Run focused affected authorization checks
throughout and the complete repository/CI gate once after integration, as phase 06 specifies.

Every file here carries the reset note and is registered in `NO_BACKWARD_COMPATIBILITY_NOTES` in
`src/main.rs`. Implementation evidence remains pending until the phases execute.

Planning validation, 2026-11-16: `cargo fmt --all -- --check`, `git diff --check` and
`python3 scripts/check-integration-docs.py` passed. A local Python consistency check passed for
all seven files: six sequential phases, relative links, reset notes, exactly one startup entry per
file, focused/final check sections, whitespace and sequence navigation. `trail check` reported no
conflicts for the tracked diff. Feature, compilation and database suites were not run for this
documentation change and its startup-path registration; phase 06 retains their pending evidence.
