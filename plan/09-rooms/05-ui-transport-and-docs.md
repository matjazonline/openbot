# 05 Room UI, transport and documentation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

People can create rooms, choose participants, discuss work and see which agent will respond next.
Existing mail users can participate under the same room policy. Continues in
[06](06-integration-and-final-verification.md).

## Batch 05A Lists, settings and conversation

1. Add Rooms alongside Desks and Direct in navigation, using the existing channel list and
   conversation components with kind-aware labels. Rooms group/filter by owning unit and goal.
   Show only rooms the viewer may read; configuration listings for managers expose configuration
   without previews, unread counts or message content when they cannot read the room.
2. Creation asks for name, address, owning unit and selected people/agents. Use 06's managed-unit
   picker and current company principal identities from 03. State that invited members can see
   existing history within their access level and that the owner retains access. Settings show
   roster, agent response readiness and archive status. Show resource/actor refusals as actionable
   configuration errors, without exposing connection secrets.
3. `@` opens a bounded, paginated member picker showing person/agent type and distinct handles.
   Keyboard navigation, accessible names, focus, escape and selection must work. Store structured
   spans, preserve them across composer edits, and show exactly who will be addressed on send.
   Reply selects a same-thread message and displays implicit or explicit recipients; unresolved
   typed handles remain text with feedback. Ordinary human posts need no mention.
4. Timeline entries show the actual author, reply linkage, mentions and per-agent state: queued,
   working, waiting for review/input, replied, no reply, refused, failed or cancelled. Use the
   existing SSE/read model with current access checks. Do not show hidden run internals as message
   text. Surface bounded-history notices and discussion limit/deadline outcomes.
5. Provide Pause agents and Resume agents for participating people; explain that pausing cancels
   queued/active work and resuming accepts new requests. Users can still post while paused or
   while a run waits. Review/approval uses the existing decision UI and cannot be settled merely
   by writing an approval-sounding room message.

Focused checks: `room_ui_` and `room_routes_` cover lists, roster, real authors, implicit replies,
status and pause; browser tests cover keyboard mention selection, edits, focus and live revocation
using native rendered markup. Run affected desk wording tests; update their rule to permit Room
only for room kinds while preserving Desk for ordinary channels.

## Batch 05B Existing transports and API

1. Expose room kind, roster, addressed-message and pause operations through the existing channel/
   thread API owners. Product routes may use `/rooms`; keep one underlying policy/use-case path.
   Validate body/item bounds and idempotency keys at the boundary and recheck authority at commit.
   Generic channel endpoints cannot change a room into a public desk or bypass member admission.
   Address directories and `_help` label rooms by kind and list only authorized destinations.
2. Use the existing channel email binding and company address namespace for the room address.
   SMTP and authenticated provider webhooks apply the same roster, author, quoting and revision
   rules. A forged From or display name does not become a person/agent principal. The room's
   address routes the conversation; a textual `@agent` targets an existing room member, not that
   member's direct inbox. Preserve reply identifiers and actual-author presentation in outbound
   mail while the authenticated transport sender remains the room address it may send for.
3. Membership controls readership, not automatic email fanout. Reuse the authorized existing
   thread/transport recipients and preferences. Adding a roster member alone sends them no history
   by email; a mention alone creates the in-product notification from 02. Outbound recipients are
   rechecked against current room membership and content policy; never CC every roster member or
   copy a private reply to arbitrary email addresses in the body.
4. Room-local turns are not internal mail relays and do not relax the relay's trace/loop checks.
   `channel.call` continues targeting a desk's selected responder; refuse room destinations rather
   than choosing an arbitrary member. Generic `message.send` retains its existing non-admitting
   note semantics; only this plan's addressed room publication path creates continuation turns.
   Existing protocol-specific quiet/no-agent controls suppress agent admission on a room too.
   Native provider mention identifiers may be normalized later using the same addressed-member
   contract; do not add a Slack/other-provider dependency to this plan.

Focused checks: `room_transport_` exercises in-product, SMTP and signed provider-webhook acceptance,
deduplication, quoted mentions, quiet messages, reply correlation, actual authors and outbound
recipient revocation. Use fake delivery/providers; an internal-only relay test is insufficient.

## Batch 05C Documentation

Write `docs/rooms.md` with the conversation example, roster/history visibility, reply precedence,
agent handoffs, limits and pause semantics. Update transport, principal-address, department,
workflow authoring/operations and agent tool guides where their contracts now distinguish rooms.
Document scopes and billing with an example of an agent invited from a sibling department.
Update the page-level AGENTS vocabulary rule to recognize channel kinds when implementing the UI.
Name only implemented behavior; retain 08's resolved decisions and existing desk/direct behavior.

Focused checks: local links, actual configuration/limit names and the existing integration-docs
check. Record exact commands/results alongside the phase's focused UI and transport checks.
