# AWC-P03-D1 r2 — architecture gate: dispatch order, interim review rule, bounds and checks

Author: root/A (Claude Code `1e84a7af`, Opus 5.5/xhigh), 2026-11-22. Answers review `AWC-P03-D1-R1-N1`,
findings F1–F6.

**r2 is [r1](AWC-P03-D1-r1-decision.md) with the replacements below.** r1 is kept unchanged, and everything not
replaced here stays in force. r2 replaces:

- D1's ordered list;
- D4's "Replay of a record" item 2 and its "Phase boundary" bullet;
- D2's schema bullet on the new keys;
- the verification table rows L4, R2, R3 and R4.

It also adds rows and notices.

## Facts added

- **Desk before parse.** `start_on` step 1 needs the desk before locking: `Purpose::ChannelCall { desk }` feeds the
  account union (`channel_call_target.rs:167-171`). Today the desk comes from the fully parsed request.
- **Deadline judged in parse.** `ChannelCallRequest::parse` judges the deadline against `now`
  (`channel_call.rs:112-134`). Parsing a replayed call would therefore answer `deadline_invalid` once its own
  deadline has passed.
- **p01 D1.1 order:**
  1. parse;
  2. origin;
  3. write right;
  4. cycle;
  5. desk states;
  6. intake;
  7. levels;
  8. destination admission, which includes `workflow.tool_limit`.
- **Approval policy.** `ChannelReplySettings.approval_policy: Option<ReplyApprovalPolicy>`
  (`channel_reply/mod.rs:63`): `None` means the desk configures no final review. p02's record stores it
  (`workflow_channel_calls.approval_policy`).
- **Deleting a record.** `guard_workflow_channel_call` allows a delete only once the calling run no longer exists
  (init schema, `guard_workflow_channel_call`). That is how run retention removes a record.

## D1 ordered list, replaced (F1, F3)

`dispatch_on`, one transaction:

0. **Desk extraction, with no locks.** A new function `ChannelCallRequest::desk(arguments) -> Option<Uuid>` reads
   only `channel_id` as a UUID, with no other judgement. If it is missing or malformed, the answer is
   `invalid_input` and the transaction ends.
   - A record cannot exist for such arguments. A record is only written for a call whose `channel_id` parsed, and
     a saved call's arguments are immutable (`saved_call_on` compares them exactly).
   - So answering first loses no replay.
1. **`start_on` steps 1–3, unchanged.** These are:
   - the account union with `Purpose::ChannelCall { desk }`, then the company lock;
   - `lock_fence`;
   - the replay lookup by `(run, execution, call)`.
2. **Replay (AWC-C002).**
   - A record with the same digest goes to D4 consumption.
   - A record with another digest answers `invalid_input`.
   - No full parse happens, so a call past its own deadline still answers its result (new case in R2).
3. **D1.1 step 1, the full parse.** `ChannelCallRequest::parse(arguments, now, run.deadline)`. An error is a tool
   result.
4. **D1.1 step 2, the origin proof.** `agent_origin::channel_call_on` (r1 D1 step 3), then
   `agent_authority::check_execution_on(.., Tools)`. A failure is an origin refusal and not a tool result.
5. **`start_on` steps 4–8** in p01's order: write right, cycle, desk states, settings check, intake, levels, then
   destination admission. Destination admission gains two items, at the end of step 8:
   - **Tree limit.** A continuation count ≥ `WORKFLOW_JOB_CONTINUATION_LIMIT` answers `workflow.tool_limit`.
   - **Interim review rule (F2).** A recorded `approval_policy` that is `Some` answers the denied closure
     `destination_unavailable`, before any child exists.
     - Until p04 there is no review path for a call result. A desk that requires final review cannot release a
       result to a calling agent, so it declines up front rather than running work whose answer could not be
       disclosed.
     - p04 removes this item when it adds review (notice AWC-C004).
6. **Step 9 under the savepoint**, extended by r1 D2.
7. **Park, or answer** (r1 D1 step 7).

**`MAX_CHANNEL_CALL_DEPTH` is dropped (F3).** Nested depth is already bounded on three axes:

- **Cycle.** The cycle check excludes any desk already in the active ancestry, so each level adds a distinct desk.
- **Ancestry walk.** `in_ancestry_on` is bounded by `MAX_ANCESTRY`. A deeper chain reads as in-ancestry and
  answers `cycle`, so the walk's cost is bounded.
- **Tree total.** The root budget's activations, model calls and repetitions bound the whole tree, including
  every nested call child, as it executes.
- **Lifetime.** Every child's deadline is at most its parent's.

A separate depth constant would protect nothing these leave open. No new model-visible limit is introduced.

## D4 "Replay of a record" item 2, replaced (F5)

**Live authority at consumption (p01 D1.5).** The replay transaction already holds:

- the account union, including the destination `run_as`, then the company lock;
- the fence.

It then re-reads, in this order and each under the lock named:

1. **The origin proof.** The manifest selection and the channel.call binding snapshot, through
   `agent_origin::authority_on`, which takes the selection locks it takes for workflow tools. Then
   `agent_authority::check_execution_on(.., Tools)`.

   The harness check `saved_call_on` is included: the call is the checkpoint's pending call.
2. **The desk row**, `FOR SHARE OF desk FOR KEY SHARE OF unit`, as `target_on` takes it, for the desk's
   `enabled` flag and access mode.
3. **The agent's `participate` grant**, through `agent_participates_on` (`FOR SHARE`). This and the desk row give
   `participant_authorized`.
4. **The destination binding row** (`workflow_bindings`, `FOR SHARE`): active, and the recorded
   binding still present.

A failure answers `channel_call.revoked`, with no content and no absorption. Writers of grants, desk enablement
and binding activation take the row they change `FOR UPDATE` (or stronger), so they conflict with these share
locks: a consumption and a revocation serialize. B2's R4 tests each writer racing a consumption.

## D4 "Phase boundary", replaced (F2, F5)

- p04 replaces the `succeeded` arm with response ownership, review, `channel.deny`, `channel.hand_off` and
  `channel.no_reply`, and it removes the interim review rule from D1 step 5.
- Until then:
  - a call reaches a child only on a desk with no final review, so the projected reply is the desk's released
    answer;
  - a destination ending in `channel.no_reply` has no projectable output and answers the error
    `channel_call.response_invalid`.
- `channel_call.response_invalid` (lifecycle, after launch) is added to p01 D2's error list, after
  `workflow.child_cancelled`, and to the README's error list in this phase's change. Doc 04 A.4 requires such a
  typed error ("invalid projection … is a typed error"), and p04 reuses it (AWC-C004).

## D2 schema bullet on the new keys, replaced (F6)

The FKs `(company_id, thread_id, source_message_id) → thread_messages` and `(company_id, channel_id, thread_id) →
threads` are `NO ACTION`.

- While the record exists, which is while its calling run exists, the call's thread and source message cannot be
  deleted. Doc 04 B.6 ("retain call source … while either side needs them") requires this.
- Run retention deletes the caller and then the record, after which the thread may go.
- p04 B.6 owns the remaining retention races. Desk and channel deletion are already refused while bindings and
  runs on the desk exist.

## Verification table, replaced and added rows (F1–F4)

| ID | Test | Assertion |
| --- | --- | --- |
| L4 | `channel_call_launch_continuation_limit` | Continuation 63 gives `workflow.tool_limit`, after an unwritable desk still gives `target_unavailable` (order); zero rows. |
| L9 | `channel_call_launch_lease_expired_at_commit` | The fence lease expires between start and park: the park is refused and the transaction rolls back; zero record, thread, message, child or wait rows; the job is reclaimable. |
| L10 | `channel_call_launch_review_desk_declines` | A desk whose settings carry an approval policy gives the denied closure `destination_unavailable`, with zero rows. |
| L11 | `channel_call_launch_malformed_desk_answers_first` | Arguments without a UUID `channel_id` give `invalid_input` with no locks taken (no wait on a held company lock). |
| L12 | `workflow_child_timeout_fails_child_and_reaches_parent_error_route` gains the shorter case | A call with its own deadline: child deadline = the call's < the parent's; its expiry fails the child, and the caller gets `workflow.child_failed`. |
| R2 | `channel_call_resume_replay_never_reselects` | After a settings change, and after the call's own deadline has passed, the replay returns the same child's result. Changed arguments on the saved call give `invalid_input`. |
| R3 | `channel_call_resume_outcomes` | Failed and cancelled children give typed errors, deadline expiry gives `child_failed`, a `channel.no_reply` destination gives `response_invalid`, an oversize result gives `tool_result_too_large`, and the level is absorbed only when a result is disclosed. |
| R4 | `channel_call_resume_revoked` | While waiting, each of these gives `revoked`, no content and no absorption, and the parent continues: capability withdrawn (agent republished without channel.call), grant removed, desk disabled, binding deactivated. Each writer also races a consumption, with no deadlock. |
| R9 | `channel_call_resume_restart_at_each_boundary` | The worker is dropped and the job reclaimed at each of these boundaries: after the dispatch commit, after the child terminal, after the wakeup, after the replay before the harness saves. Each yields one child, one result, the same parent execution ID, monotone continuation and no stranded wait. |

## Notices

- **AWC-C004** (to p04, on this gate's pass) says:
  - `channel_call.response_invalid` is reserved and used by p03's interim success arm;
  - p04 removes the interim review rule (D1 step 5) when review lands;
  - p04 inherits the `NO ACTION` retention keys.
- **Plan08 notice** (r1 D6), on B2's acceptance: cancel-and-archive also cancels recorded call children on the
  subtree's desks.

## Finding dispositions (r1 → r2)

| Finding | Disposition |
| --- | --- |
| F1 | Desk extraction at step 0 with no locks; replay before the full parse; replay after the call's deadline is in R2. |
| F2 | Interim review rule at admission (L10); p04 removes it. |
| F3 | Tree limit placed at D1.1 step 8; `MAX_CHANNEL_CALL_DEPTH` dropped with its reason. |
| F4 | L9, L12, R4 (capability) and R9 added. |
| F5 | `response_invalid` added to the error lists with notice AWC-C004; consumption reads and locks stated exactly, with `check_execution_on` included. |
| F6 | The new keys are `NO ACTION`, with the reason. |
