# AWC-P03-D1 r1 — architecture gate: dispatch, launch, consumption and lifecycle of a channel call

Author: root/A (Claude Code `1e84a7af`, Opus 5.5/xhigh), 2026-11-22.

## Inputs

- Requirements: doc 03 (Batch A, Batch B, acceptance).
- p01 gate AWC-P01-D1 r3: D1.1 order, D1.4 levels, D1.5 live authority, and D2 result envelope and error list.
- p02 gate AWC-P02-D1 r3, and accepted p02 code: `channel_call_target::start_on` steps 1–9,
  `children::create_call_on` under a savepoint, `workflow_channel_calls`, and `run_dependency_scope`.
- Coordination r11:
  - AWC-C001 r4;
  - AWC-C002 r1: replay resolves from the record before the deadline is re-parsed;
  - AWC-C003 r2: list of what p03 owns.

This phase is gated because it changes the following:

- the dispatch transaction;
- a schema guard on canonical messages;
- a launch guard;
- the parent park/resume protocol;
- the agent tool port;
- the cancel-and-archive of Plan08 p02.

## Facts (code as of `0639c0f0…2112`)

1. **Placeholder route.** The runtime routes `ToolInvocation::ChannelCall` to a placeholder that answers
   `target_unavailable` (`adapters/workflow_agent/mod.rs:170,238`). The port to mirror is
   `WorkflowToolDispatch` (`application/workflow/tools.rs:88`).
2. **Workflow-tool lifecycle** (`workflow_tool_calls.rs`):
   - `call_on`: authorize, then `lock_fence`, replay, limits, `children::create_on`, `park_on`.
   - Resume: `continue_call` settles the `child_run` wait, sets the run back to running and reopens the job.
   - The resumed attempt re-dispatches the saved call. `replayed_call_on` then answers from immutable facts.
   - `call_result` validates output against the parent's manifest pin.
   - A call child resolves its destination workflow through its own scope, so the parent's manifest pin
     does not apply to it.
3. **Child waits.** `children::write_on` with `ChildOrigin::Call{call, attempt}` writes the `child_run` wait with
   `child_tool_call_id`. p02's `create_call_on` uses this, so a call child's wait is already a tool-call wait.
4. **Origin proof for workflow tools.** `agent_origin::workflow_tool_on` (`agent_origin.rs:282`) proves an agent
   origin: the manifest selection, the tool binding's snapshot, and `saved_call_on` (the call exists in the
   harness checkpoint with the same arguments). p02's `start_on` currently trusts its `origin` argument.
5. **Canonical message storage.**
   - `message_writer::write_destination_on` (`message_writer.rs:15`) creates a thread and calls
     `thread::insert_message_on` with `MessageStampSource::Workflow{run}` inside the caller's transaction.
     It writes a delivery only when one is planned.
   - Ingress is the `workflow_message_intents` row written by `commit_as_on` (`inbound.rs:187`).
     `channel_reply_contexts` is written by a trigger on intents. `insert_message_on` writes neither.
   - The deferred `check_workflow_message_producer_seal` (init schema, at the
     `check_workflow_message_producer_seal` function) requires, for a Workflow-stamped message,
     either a `workflow_message_destinations` row or a published `channel_reply_decisions` row. It also checks
     `workflow_message_author_valid` and the notification origin.
6. **History.** It is captured only by `admission_history::capture(PreparedAdmission)`, for a `Thread`
   association. The child writer captures none, and a missing snapshot reads as empty
   (`history_reader.rs:105`).
7. **Terminal facts.** `workflow_run_events` carries only `terminal_state`, so deadline expiry is `failed`.
   Per p01 F6, `workflow.child_deadline` is not added.
8. **Tree bounds.**
   - Static depth is bounded at publication.
   - Root budgets bound activations, model calls and repetitions as executions run.
   - Per agent step, `WORKFLOW_JOB_CONTINUATION_LIMIT` (63) applies.
   - Nothing bounds nesting depth for dynamic channel calls beyond the cycle check over distinct desks (and
     `MAX_ANCESTRY` = 1000).
9. **Audience.** `knowledge::destination_level_on` (`knowledge.rs:214`) has one caller (memory reads). For a
   `Thread` association it is the thread's audience; otherwise the actor's level.
10. **Archive.** `department::archive_department_on` counts every unfinished run on the subtree's desks as open
    work, but cancels only roots (`archive.rs:131`). An open call child therefore makes any archive of its desk's
    unit non-cancellable. `controls::cancel_for_archive_on` cancels a tree. A cancelled child's terminal fact
    wakes its parent with no parent lock.
11. **Reply projection.** `ChannelReplyContract::project(output_schema, result)` (`channel_reply/contract.rs:76`)
    is pure and returns `CanonicalContent`.

## D1 — Port and dispatch transaction

- **Application port**, in `application/workflow/channel_call.rs`:
  `trait ChannelCallDispatch { async fn call_channel(&self, fence, call: ChannelCallToolCall) -> AppResult<AgentToolObservation>; }`
  with `ChannelCallToolCall { origin: AgentToolOrigin, call: ToolCallId, arguments: Value }`.
  - `ToolInvocation::ChannelCall` carries that call.
  - `AgentActionTools::with_channel_calls` wires it, at the composition points that wire
    `with_workflow_tools`.
  - A worker without it answers `ServiceUnavailable`, like knowledge tools. The placeholder is removed.
- **Persistence implementation.** One transaction under an outer 10 s `tokio::time::timeout`, mapped to
  `lease::timed_out()` as workflow tools do. `dispatch_on` runs in this order:
  1. `start_on` steps 1–3 unchanged: account union and company lock, then `lock_fence`, then the replay lookup.
  2. **Replay (AWC-C002).** A record for `(run, execution, call)`:
     - **same digest** → D4 consumption;
     - **other digest** → `invalid_input`.

     This happens before any request parse, so a replayed call never re-reads its deadline or the settings.
  3. **Origin proof.** New `agent_origin::channel_call_on`, mirroring `workflow_tool_on`'s agent arm:
     - the selection;
     - the tool binding whose snapshot `is_trusted_channel_call_snapshot`;
     - `saved_call_on` with the declared alias and the exact arguments.

     Then `agent_authority::check_execution_on(.., Tools)`. A failure is an origin refusal (`missing`), not a
     tool result (D1.1.2). `start_on` receives the proven `AgentToolOrigin` rather than trusting a caller value.
  4. **Parse.** `ChannelCallRequest::parse(arguments, now, run.deadline)`. An error is a tool result.
  5. **Tree limits** (AWC-C003), each `workflow.tool_limit`:
     - continuation count ≥ `WORKFLOW_JOB_CONTINUATION_LIMIT`;
     - recorded channel-call nesting of the calling run ≥ `MAX_CHANNEL_CALL_DEPTH` = 8. This is a new bound.
       Its test approaches it (depth 7 admitted, depth 8 refused). It is computed from the same recursive
       ancestry walk as the cycle check.
  6. **`start_on` steps 4–9**, extended by D2. The child deadline is `request.child_deadline(run.deadline)`.
  7. **Park.** On `Started`: `park_on(fence, wait)` → `Parked(wait)`. On `Answered`, the tool result. On
     `Replayed`, D4.
- **Conflict.** Settings that changed since the hint (D3 step 5 of p02) return `Conflict`. `call_channel`
  retries the whole transaction up to 3 times inside the 10 s bound. After that it returns `ServiceUnavailable`,
  and the agent attempt fails and retries like any transient tool error. Nothing persists between attempts.

## D2 — Destination thread and source message

All of this runs inside p02's savepoint `channel_call_child`, before the child write, so a refusal at step 9
also removes the thread and message.

- **Thread.** A new thread on the desk (`NewThread` Desk target). Its subject is the request's subject.
- **Message.** Stored with `thread::insert_message_on` and `MessageStampSource::Workflow { run: caller }`, so it
  carries the calling run's content level and exactness (D1.4 request release).
  - It is `MessageWrite::internal`:
    - author `Principal(calling agent's principal)` with role Agent;
    - direction Inbound and entry kind Delegation, as the internal relay uses;
    - no other participants.
  - It has no delivery, no intent, no email metadata and no `external_*` rows. So no observer, no responder
    admission, no `channel_reply_contexts` and no transport.
- **Schema.**
  - `workflow_channel_calls` gains `thread_id` and `source_message_id` (NOT NULL) and
    `UNIQUE(company_id, source_message_id)`. The message is tied by FK
    `(company_id, thread_id, source_message_id)` to `thread_messages`, and the thread by FK
    `(company_id, channel_id, thread_id)` to `threads`.
  - `workflow_channel_call_snapshot` also requires `child.thread_id = NEW.thread_id`.
  - `check_workflow_message_producer_seal` gains a third proof arm: a `workflow_channel_calls` row with
    `source_message_id = owner` and `run_id = producer`.
  - `workflow_message_author_valid` gains the matching arm: the author principal is the principal of the
    record's `agent_id`.
  - The notification-origin check is unchanged. The message takes the producer run's origin as any
    Workflow-stamped message does.
- **Child.**
  - Association `Thread{Desk(desk), thread}`. The admission checks keep authorizing on `inbox(desk)`.
  - Input `request.trigger_input(source_message, caller_address)`. The caller address is the agent's company
    address, or its principal display label when it has none.
  - History: `admission_history`'s capture is factored over `(company, run, association, ceiling, binding)` and
    called by `write_call_on`. The child's agent history is then exactly this fresh thread at its ceiling.

## D3 — Launch guard

Deferred constraint trigger `workflow_channel_call_launch` on `workflow_channel_calls` insert. It requires a
`workflow_waits` row with:

- `(company_id, run_id, execution_id)` = the record's;
- `reason='child_run'`;
- `child_tool_call_id = call_id`;
- `child_run_id = child_run_id`.

This ties the record, the wait and `ChildCause::ToolCall` (a tool-call wait) without a plain FK, as AWC-C003
requires. Readers distinguish a channel call from a workflow tool by the record's existence, keyed on
`child_run_id`.

## D4 — Consumption, live authority and result

- **Wakeup** (`waits` child terminal → `continue_call`). For a child with a record, settle the wait, return the
  run to running and reopen the job, without reading the output. The workflow-tool branch is unchanged.
- **Replay of a record** (the resumed attempt re-dispatching the saved call):
  1. A wait still `waiting` cannot coexist with a live fence: `invalid`, as for workflow tools.
  2. **Live authority (D1.5),** re-read under the locks already held:
     - the origin proof (step 3, without the harness re-check of a completed call);
     - the agent's write right on the desk (`participant_authorized`);
     - the desk enabled;
     - the destination binding active and not deleted.

     A failure answers `channel_call.revoked` and discloses nothing: no content, no absorption.
  3. **Result** from the child's terminal fact:
     - `succeeded`: the committed output is validated against the destination's pinned output schema (the
       child's own scope), then `record.contract.project(..)` → `Done{child, reply{content, fields:
       output.fields}}`. A projection failure → the new error `channel_call.response_invalid`.
     - `failed` → `workflow.child_failed`.
     - `cancelled` → `workflow.child_cancelled`.

     A result over `MAX_TOOL_RESULT_BYTES` → `workflow.tool_result_too_large`.
  4. **Absorb.** Before returning a result, `knowledge::absorb_content_on(caller, child level, exactness)`.
- **Phase boundary.** p04 replaces only the `succeeded` arm with response ownership, review, `channel.deny`,
  `channel.hand_off` and `channel.no_reply`. p03's projection is that arm's interim form. Until p04, a destination
  that ends in `channel.no_reply` has no projectable output and answers `response_invalid`.
- **Exactly once.** Replay reads immutable facts. Repeated replays return the same value, and none starts a child
  or re-selects settings (V-replay). The harness saves the result once, as for workflow tools.

## D5 — Destination audience

`destination_level_on` gains the call case, keyed on a record with `child_run_id = run`. The destination level is
the lower of:

- the child's stored `knowledge_ceiling`;
- the lowest level of a human reader on the child's thread, if any.

Agent principals do not lower it. Without this case, an agent-only thread would force level 0 and the ceiling
would never take effect (p01 D1.4).

## D6 — Cancellation and archive

- **Parent cancel.** It already cascades through the `child_run` wait to the call child (`cancel_tree_on`). A late
  child terminal cannot resurrect the cancelled parent: the existing wakeup guard applies.
- **Cancel-and-archive.** The cancellable set is extended from roots to roots plus recorded call children on the
  subtree's desks whose caller is not itself on the subtree. Each is cancelled with `cancel_for_archive_on`, and
  its caller gets `workflow.child_cancelled` through the wakeup.
  - The bound `MAX_ARCHIVE_CANCELLATIONS` counts both kinds.
  - Plan08 is notified (new Plan08 notice, AWC-C001's requirement).

## D7 — Stack

- `call_channel` boxes its transaction future, as `call_workflow_tool` does.
- `start_on` and `create_call_on` stay boxed at their seams.
- Frames are measured with `scripts/stack-frames.sh` before and after, and `scripts/stack-budget.sh` stays green.
  No stack limit is raised.

## Invariants

1. One saved call → at most one record, one thread, one source message, one child, one wait. The fence, then the
   primary key, enforce it.
2. A refused call leaves only its tool result: the savepoint covers thread, message and child.
3. The source message is never ingress: zero intents, deliveries, reply contexts or external rows.
4. A result is disclosed only if authority is still live at consumption, and only after its level is absorbed.
5. No parent wait is immortal. The child is bounded by its deadline (≤ the parent's), by cancellation and by
   archive.

## Failure modes

| Failure | Behaviour |
| --- | --- |
| Lease loss at commit | Park fails (`persistence_bound::refused`); the whole transaction rolls back, including the child. |
| Timeout | Rollback; `timed_out`; the attempt retries. |
| Settings race | `Conflict` retry ×3. |
| Crash after commit | The resumed attempt replays. |
| Crash after the child terminal | The wakeup is durable. |
| Crash after the tool result is saved | The harness has the result and no re-dispatch happens. |

## Verification plan

The test files are `channel_call_launch_tests.rs` and `channel_call_resume_tests.rs`, plus affected
`workflow_tool_`, child, message and history suites.

| ID | Test | Assertion |
| --- | --- | --- |
| L1 | `channel_call_launch_parks_and_commits_atomically` | One Started dispatch: record + thread + Workflow-stamped source message (caller level) + child (Thread association, deadline = min) + wait + parked job; zero intents, deliveries, reply contexts, external rows. |
| L2 | `channel_call_launch_refusal_leaves_no_thread_or_message` | Unit cap and role refusal under the savepoint: zero threads/messages/records/children. |
| L3 | `channel_call_launch_origin_is_proven` | Forged origin (other agent, no saved call, other arguments, non-channel.call binding) → origin refusal, zero rows. |
| L4 | `channel_call_launch_limits` | Continuation 63 → `workflow.tool_limit`; depth 7 admitted, depth 8 → `workflow.tool_limit`. |
| L5 | `channel_call_launch_guard_names_the_wait` | Forged record without its tool-call wait → `workflow_channel_call_launch`; forged Workflow message without a record → producer seal refusal. |
| L6 | `channel_call_launch_two_targets_differ` | Two desks with different params and resources: each child runs its own binding; parent without a channel. |
| L7 | `channel_call_launch_history_is_the_fresh_thread` | Native history template: the child's snapshot holds only the source message. |
| L8 | `channel_call_launch_competing_claimants_converge` | Two dispatches of one saved call through `call_channel`: one parks, one `OwnershipLost`, one child (p02 NOT EXERCISED). |
| R1 | `channel_call_resume_scripted_agent_continues` | Scripted parent: calls, parks, child succeeds, wakeup, the resumed attempt replays `done` with projected content; second model turn in the same execution; continuation 1. |
| R2 | `channel_call_resume_replay_never_reselects` | After a settings change, the replay returns the same child's result; changed arguments on the saved call → `invalid_input` (p02 NOT EXERCISED). |
| R3 | `channel_call_resume_outcomes` | Failed, cancelled and deadline-expired children → typed errors; invalid projection → `response_invalid`; oversize → `tool_result_too_large`; level absorbed. |
| R4 | `channel_call_resume_revoked` | Grant removed, desk disabled or binding deactivated while waiting → `revoked`, no content, no absorption, parent continued. |
| R5 | `channel_call_resume_competing_resumers` | Two resumers: one continues; one saved result; no stranded wait. |
| R6 | `channel_call_resume_cancel_races` | Cancel racing launch and completion: no live child under a cancelled parent; no resurrection. |
| R7 | `channel_call_resume_archive_cancels_call_child` | Cancel-and-archive of the desk's unit with an open call child: the child is cancelled and the caller gets `workflow.child_cancelled`. |
| R8 | `channel_call_resume_destination_audience` | With a raised allowance (unit-level), an agent-only thread has destination = ceiling; a human reader at a lower level lowers it. |

Affected suites:

- `workflow_tool_` (dispatch, race and continuation);
- `workflow_child`;
- `message_thread_`, `channel_reply_` and the producer-seal tests;
- `unit_archive_`;
- `knowledge_` memory read ceilings;
- `stack-budget.sh`.

## Batches

- **B1 — launch:** D1, D2, D3, D7, and L1–L8.
- **B2 — resume and lifecycle:** D4, D5, D6, and R1–R8.

B2 builds on B1's accepted dispatch.
