# 03 Agent turns and durable ordering

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Addressed agents take durable, ordered turns on the shared thread through the existing workflow
runtime. Humans can keep talking while work runs or waits. Continues in
[04](04-scopes-budgets-and-lifecycle.md).

## Batch 03A Response bindings and admission

1. Extend room-associated workflow bindings with a responding member principal, requiring a live
   agent roster entry in the same room and tenant. One selected response binding per agent member;
   person members have none. Preserve 03-principal-addresses' exclusive channel/principal target
   shape: this binding targets the room channel, with the member as its responder, not as a second
   association target. Extend selected-response ownership from one responder per desk to one per
   addressed room member; never pick the room's first binding. Freeze the turn's response owner
   and review/publication policy. Ordinary desk and direct bindings retain their admission paths.
2. Reuse autonomous workflow provisioning/publication to create the agent's room response binding
   and validate its response contract, agent identity, resources and configured actor. Do not
   reuse the direct binding as the execution target or mutate it. A member binding has one agent
   response owner; a published response that would impersonate another principal is refused.
   This delivery supports the existing autonomous response pattern, including its review gates;
   arbitrary observer/schedule bindings on rooms are refused to avoid a second admission route.
3. Extend `MessageAdmissions` (`src/application/workflow/message_admission.rs`) to select only
   bindings for addressed agent members. The current `MessageFacts.mentioned` is channel-wide;
   it cannot decide which member was named. Freeze the selected binding/configuration revision
   when queuing the request, then capture its normal immutable agent/dependency manifest at
   admission. Reconfiguration or deactivation before admission makes the queued request stale;
   do not substitute a newly selected workflow silently.
4. Configure one automation actor for the room through the existing binding `run_as` mechanism;
   every member binding must agree on it. A room management command changes that configuration
   atomically across the bindings and bumps the settings revision. Require the actor's current
   authority to use the room; do not choose an actor from mention order. Freeze it for each new
   human-triggered discussion and retain the initiating human membership as an additional live
   fence. Continuations preserve that actor; another binding's `run_as` cannot widen it. Apply
   ordinary workflow, agent, resource and approval checks for that actor at every admission. The
   model supplies no actor, workflow revision, author, budget owner or goal id.
5. An accepted message remains visible if an addressed binding cannot run. Record a bounded typed
   outcome per addressee, as 02's admission refusals do, without converting a workflow refusal into
   a delivery retry. Do not run company-wide or unmentioned observer bindings on room messages.

Focused checks: `room_binding_` for provisioning, actor/agent identity and target constraints;
`room_admission_` for addressed-only selection, distinct actors, stale binding and visible
refusals; include the existing desk/direct admission and response-publication regression families.

## Batch 03B Thread order and recovery

1. Add purpose-specific durable room turn rows containing the thread, source association, addressed
   principal, selected binding revision, discussion id, sequence, state and linked workflow run.
   Unique `(company, source association, principal)` makes replays one logical turn. Assign sequence
   under the thread's gate at message acceptance, in first-mention order; later messages append.
   Keep at most 64 pending turns per thread. If the whole incoming batch cannot fit, accept the
   message with a `room.busy` outcome for its agent targets and queue none of that batch.
2. Extend the existing admission queue/worker with the readiness predicate for a room turn.
   Exactly one nonterminal agent turn may own the thread's agent slot. Claiming the next turn,
   admitting its run and linking the durable owner form one transaction; conflicting claimants
   cannot create two runs. Reuse the existing lease, retry/backoff and shutdown machinery. Do not
   introduce a second provider worker pool or a detached room runtime.
3. The turn holds the slot through execution, review and human-input waits until its reviewed
   reply is committed, it completes with `no_reply`, or it terminally fails/is cancelled. Human
   message writes and wait answers do not need the agent slot. Deadlines bound a parked turn;
   another thread remains runnable. A refused/stale turn settles and allows the next queued one.
4. Freeze a canonical-history cutoff when the turn becomes active, after earlier replies commit.
   Build the shared conversation input from visible messages up to that cutoff, with their real
   authors and the agent's addressed request identified. Apply existing token budgeting and
   audience/level checks; record omitted history. Retry uses the saved preparation. Preserve
   per-agent private history boundaries in `application/workflow/agent_history/`; do not treat
   another agent's public reply as this agent's own assistant output or import direct history.
5. Completion atomically commits the authorized authored message, addressed successor intents and
   terminal turn state, then releases the slot. New mentions append after already queued turns;
   they do not jump ahead or coalesce distinct requests to the same agent. A message with no reply
   releases the slot without a fabricated transcript entry. Provider errors are turn state.
6. Worker loss resumes the same linked run; it does not admit another. Expired leases consume
   attempts, poison work backs off and terminates, and stale workers cannot publish or release a
   replacement owner's slot. Retained prepared output is reused where the runtime supports it;
   do not claim exactly-once external provider execution across ambiguous network failures.

Focused checks: `room_turn_order_` proves two agents respond in order and the second sees the
first; `room_turn_competing_` races two admission workers and stale completion against reclaim;
`room_turn_recovery_` injects failures before/after admission, model completion and publication;
`room_turn_wait_` proves human input can settle waits and another thread progresses. Add a
poison-batch regression so two immediate polls do not reclaim unchanged failures.

## Batch 03C Discussion limits and pause

1. Persist a discussion record rooted in one authenticated human message, with its initiating
   membership, execution actor, deadline, allowance owner and reserved/used turn count. Human
   messages with no agent targets need no record. Agent-generated requests inherit the record;
   a model cannot mint a new one. An agent-origin message without an authenticated active room
   discussion starts no turns, even if its text mentions members. Default bounds are 16 total
   agent turns and a 15-minute absolute deadline, enforced as domain constants. Retries consume
   attempts, not fresh logical turns.
2. Reserve turns for all initial and successor targets atomically. A ninth repetition of an
   agent may be valid within the shared bound; the same-message/principal uniqueness still holds.
   At the boundary, publish the agent's valid answer but record each unqueued successor as
   `room.turn_limit` or `room.deadline`, visible in message outcomes. Never restart the budget on
   handoff, an edit or notification. A fresh human addressed message is a new discussion. Close
   the discussion atomically when its last turn settles with no successors; deadline/cancellation
   terminally settles its remaining turns and runtime work through the same lifecycle owner.
3. Any current person participant or the owner can pause agent work for this thread. Commit the
   pause revision, cancellation of queued work and runtime cancellation requests together. Live
   side effects/publication check the fence; await real cancellation using existing ownership
   machinery. Resume is an explicit person action that accepts new requests; old cancelled turns
   do not restart. Room reading and human posts continue while paused.

Focused checks: `room_discussion_limits_` covers branching and cycles across the bound;
`room_pause_competing_` races pause with claim, effect and publication and verifies stale output
cannot appear after the pause fence. Use the README's commands with real competing database workers.
