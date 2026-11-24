# AWC-P04 review findings

Schema line numbers are in `migrations/20260817000000_init_schema.sql` at the reviewed state.

## AWC-P04-D1-F1 — The ownership change names two guards and one reader; more depend on `intent_id`

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); phase AWC-P04, stage architecture gate; round
  [AWC-P04-D1-R1-N1](agent-review-notes.md#awc-p04-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD
  `fe7791d3`; decision [AWC-P04-D1 r1](logs/AWC-P04-D1-r1-decision.md), sha256 `56d9c53b8151ebc6cff9d5aabb3d9a1d3fca36c97df17dc919c0cb9806d4b9c5`.
  Originates in the decision.
- **Classification:** actionable; medium; schema and reader completeness (D1, invariant 5).
- **Evidence.** D1 changes `guard_channel_reply_run`, `require_channel_reply_proposal_links`,
  `channel_reply_publication_matches` and `root_on`. These also key on the ingress owner and
  are not mentioned:
  - `guard_channel_reply_proposal` (13410): `channel_reply_runs … AND intent_id=NEW.intent_id`.
    With a null `intent_id` the equality is null, so every call proposal is refused.
  - `require_channel_reply_decision` (13489): `intent_id=decision.intent_id`, same effect for a
    succeeded call child; it fires on runs, owner rows and decisions.
  - `guard_channel_reply_decision` (13450-13451): the immutability row lists `intent_id` and
    must list the new key.
  - `approval_accounts::load_on` (`approval_accounts.rs:29-33`): reads the policy through
    runs ⋈ contexts, so a call child's reviewers would not be found for the account prelock.
  - `agent_preparation::owned_agent_desk_on` (`agent_preparation.rs:80-92`): requires a
    context and `run.parent_run_id IS NULL`, so an agent responder in a call child is not
    "owned".
  - `notification/projection.rs:117`, `channel_reply_views.rs:201`, `channel_reply.rs:407,432`
    and `message_intents.rs:143` join contexts by intent.
- **Required.** Enumerate every SQL object and Rust statement that reads
  `channel_reply_runs/proposals/decisions.intent_id` or joins `channel_reply_contexts`, and give
  each a disposition: reads the view, keeps ingress-only with the reason, or gains a call arm.
  State what an agent responder in a call child is authorized by.
- **Instruction lesson (candidate).** When a key becomes nullable, list its readers by search,
  not from memory; an equality on a nullable key fails closed silently.
- **Resolution history.** 2026-11-23 open.

## AWC-P04-D1-F2 — `channel.hand_off` takes the nested answer from its decision, not from what this run was allowed to consume

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); phase AWC-P04, stage architecture gate; round
  [AWC-P04-D1-R1-N1](agent-review-notes.md#awc-p04-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD
  `fe7791d3`; decision [AWC-P04-D1 r1](logs/AWC-P04-D1-r1-decision.md), sha256 `56d9c53b8151ebc6cff9d5aabb3d9a1d3fca36c97df17dc919c0cb9806d4b9c5`.
  Originates in the decision.
- **Classification:** actionable; medium; authority and classification (p01 D1.5, D1.4;
  doc 04 B.4).
- **Evidence.** D2: the hand-off is valid when this run recorded a call to `with.run_id` and
  that call "closed `done`, read from its settled decision"; its content is "that nested
  call's published content". p03's consumption discloses a nested result only after the live
  authority check and absorbs its level first. A nested call whose consumption answered
  `revoked` (or was never consumed) still has a `done` decision. The hand-off would then
  project content this run was refused, at this run's level, to its own caller.
- **Required.** Make the hand-off depend on a durable fact that this run consumed that call as
  `done` (after authority and absorption), or repeat the authority check and the absorption at
  the terminal. Add both cases to A4 (revoked nested call; nested child above the run's level).
- **Instruction lesson (candidate).** A second reader of a guarded result must pass through
  the same guard as the first, not read the result's source.
- **Resolution history.** 2026-11-23 open.

## AWC-P04-D1-F3 — The output-schema exception does not say what other consumers of a terminal output do

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); phase AWC-P04, stage architecture gate; round
  [AWC-P04-D1-R1-N1](agent-review-notes.md#awc-p04-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD
  `fe7791d3`; decision [AWC-P04-D1 r1](logs/AWC-P04-D1-r1-decision.md), sha256 `56d9c53b8151ebc6cff9d5aabb3d9a1d3fca36c97df17dc919c0cb9806d4b9c5`.
  Originates in the decision.
- **Classification:** actionable; medium; contract (D2).
- **Evidence.** Today every terminal output is validated against the workflow's schema
  (`batch.rs:144`), `channel.no_reply` included. D2 exempts `channel.deny` and
  `channel.hand_off`. Other readers assume a succeeded run's output fits its schema or equals
  the proposal: a static or `workflow.call` parent (`child_completion_plan.rs:93`), a workflow
  tool caller (`workflow_tool_calls.rs:313`), and `require_channel_reply_decision`
  (`proposal.business_output=execution.committed_output`). The decision does not say where the
  two steps may be used (only in a message-triggered responder, or anywhere), nor what a
  non-channel parent, a manual run or a schedule gets from a `{reason}` or `{run_id}`
  output.
- **Required.** State the compile-time rule for where each step is allowed, and the behaviour
  of each consumer above for such a terminal, with a test for a workflow ending in
  `channel.deny` that is run as a static child and as a workflow tool.
- **Resolution history.** 2026-11-23 open.

## AWC-P04-D1-F4 — `review_rejected`/`review_expired` must be tied to the failing terminal, not to any wait with that code

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); phase AWC-P04, stage architecture gate; round
  [AWC-P04-D1-R1-N1](agent-review-notes.md#awc-p04-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD
  `fe7791d3`; decision [AWC-P04-D1 r1](logs/AWC-P04-D1-r1-decision.md), sha256 `56d9c53b8151ebc6cff9d5aabb3d9a1d3fca36c97df17dc919c0cb9806d4b9c5`.
  Originates in the decision.
- **Classification:** actionable; low; result mapping (D3).
- **Evidence.** D3 reads "the child's final-reply decision wait settled with
  `approval_rejected`/`approval_expired`". A rejected final can be routed by the child, which
  then fails later for another reason; and other approval waits in the child settle with the
  same codes (`decision_approvals.rs`, `wait_commit.rs`). Either would misreport an ordinary
  failure as a review outcome.
- **Required.** Key the mapping on the proposal of the run's terminal (failed) execution and
  its `approval_decision_id` wait. Add a routed-rejection-then-failure case to B2.
- **Resolution history.** 2026-11-23 open.

## AWC-P04-D1-F5 — D5 describes a retention order the code does not have

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); phase AWC-P04, stage architecture gate; round
  [AWC-P04-D1-R1-N1](agent-review-notes.md#awc-p04-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD
  `fe7791d3`; decision [AWC-P04-D1 r1](logs/AWC-P04-D1-r1-decision.md), sha256 `56d9c53b8151ebc6cff9d5aabb3d9a1d3fca36c97df17dc919c0cb9806d4b9c5`.
  Originates in the decision.
- **Classification:** actionable; low; retention claim (D5, doc 04 B.6).
- **Evidence.** D5: "run retention deletes leaves first" and "deleting a caller during review
  is refused while the child exists". `storage_retention.rs:47-49` deletes every run of a
  budget tree in one statement once the tree is eligible; a caller and its call child share a
  tree, so they go together. The guards D1 adds (owner row to record, record to thread and
  message, proposal and decision to owner) are then exercised by one cascading delete, not by
  an order.
- **Required.** Restate D5 from the actual mechanism, say what makes a tree with a pending
  review ineligible, and make B5 run the production retention on a tree holding a finished
  call and on one holding a call under review.
- **Resolution history.** 2026-11-23 open.

## AWC-P04-D1-F6 — D6 changes a stated requirement and leaves its failure case open

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); phase AWC-P04, stage architecture gate; round
  [AWC-P04-D1-R1-N1](agent-review-notes.md#awc-p04-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD
  `fe7791d3`; decision [AWC-P04-D1 r1](logs/AWC-P04-D1-r1-decision.md), sha256 `56d9c53b8151ebc6cff9d5aabb3d9a1d3fca36c97df17dc919c0cb9806d4b9c5`.
  Originates in the decision.
- **Classification:** actionable; low; requirement deviation (doc 04 C).
- **Evidence.** Doc 04 C: "Both writes happen in the transaction that settles the call". D6
  writes the destination reply through the existing publisher in its own transaction. The
  reuse follows doc 04 A.1 and C.1 ("the same way the native `reply` step publishes"), so the
  choice is defensible, but it changes plan text, and B cannot accept that for the user. The
  decision also does not say what the desk sees when publication is refused at re-validation
  or fails after its attempts: the caller already has the answer and the desk thread has no
  reply, which is the loss of sight batch C exists to prevent.
- **Required.** Record the deviation for the user's confirmation or amend doc 04 C in the same
  change with the reason. State the failed-publication behaviour and add it to C1.
- **Resolution history.** 2026-11-23 open.

## AWC-P04-D1-F7 — Three behaviours left undecided

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); phase AWC-P04, stage architecture gate; round
  [AWC-P04-D1-R1-N1](agent-review-notes.md#awc-p04-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD
  `fe7791d3`; decision [AWC-P04-D1 r1](logs/AWC-P04-D1-r1-decision.md), sha256 `56d9c53b8151ebc6cff9d5aabb3d9a1d3fca36c97df17dc919c0cb9806d4b9c5`.
  Originates in the decision.
- **Classification:** actionable; low; decision completeness.
- **Evidence.**
  - `policy_current_on` (`channel_reply_completion.rs:108-124`) refuses a final when the
    desk's current policy differs from the captured one. For a call that makes a policy edit
    during the call fail the child. Doc 04 B.3 asks that edits not replace the captured
    policy; the decision says only "run unchanged".
  - A7 leaves `message.ask` on the call source as "works or refuses (recorded either way)".
    Doc 04 A.6 requires unsupported interactive shapes to fail before creating a wait; the
    source's author is an agent principal, which cannot answer. The gate should decide.
  - D3 narrows AWC-C004's reuse of `response_invalid`; the notice needs a new revision.
- **Required.** Decide each and add the policy-edit case to the tests.
- **Resolution history.** 2026-11-23 open.

### AWC-P04-D1-F1…F7 — resolution updates (recheck AWC-P04-D1-R2-N1, decision r2 sha256 `b1c7ff9a…4927`)

- **F1.** 2026-11-23, fixed in the decision. A non-null generated `owner_id` keeps every
  equality guard; the reader table gives each reader a disposition. B repeated the search:
  the non-test files that name `intent_id` and a reply table are exactly those in the table
  (`channel_reply_completion.rs` 8, `channel_reply_proposals.rs` 3,
  `channel_reply_publish_read.rs` 2, `channel_reply.rs`, `channel_reply_views.rs/.sql`,
  `approval_accounts.rs`, `agent_preparation.rs`, `notification/projection.rs`). The other
  reply-table readers (`actions.rs`, `pending_recovery.rs`, `wait_commit.rs`, `lease_claim.rs`,
  `channel_reply_delivery.rs`, `channel_reply_publish.rs`, `channel_reply_publication.rs`,
  `channel_reply_resume.rs`, `agent_history_views/rows.rs`, `storage_retention_eligible.sql`)
  key on `run_id` and carry no root-run condition. Closed.
- **F2.** 2026-11-23, fixed in the decision (D7). A consumption fact is written only by a
  disclosing consumption; `revoked` writes none; the hand-off reads this run's `done` fact.
  See F8 for two details. Closed.
- **F3.** 2026-11-23, fixed in the decision (D2a). The two steps fail in an unowned run before
  any output is committed, so no schema-assuming consumer reads their output; A8. Closed.
- **F4.** 2026-11-23, fixed in the decision. Keyed on the terminal execution's proposal and
  its decision wait; B2a. Closed.
- **F5.** 2026-11-23, fixed in the decision. Restated from `storage_retention.rs:47-49`;
  eligibility already excludes a tree with a waiting decision or a pending or claimed final
  (`storage_retention_eligible.sql:16-25`); B5 runs the production retention. Closed.
- **F6.** 2026-11-23, closed. Doc 04 C is amended in the working tree and coordination r15
  records user decision U5. The quotation is the author's record; B cannot verify it. The
  failed-publication view is stated.
- **F7.** 2026-11-23, fixed in the decision (D8, AWC-C004 r2 on pass); A10, A11. Closed.

## AWC-P04-D1-F8 — Two details of the consumption fact to state in the batch

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); phase AWC-P04, stage gate recheck; round
  [AWC-P04-D1-R2-N1](agent-review-notes.md#awc-p04-d1-r2-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD
  `fe7791d3`; decision [AWC-P04-D1 r2](logs/AWC-P04-D1-r2-decision.md), sha256 `b1c7ff9a1f0601f6509bd52621d4df92d7eace814362143ccf6c3077f5df4927`.
  Originates in the decision.
- **Classification:** actionable; low; not blocking the gate. B checks both in the batch.
- **Evidence.**
  - D7: "A later replay returns this row." r1 D3 puts the result "after p03's live-authority
    check", and D7 says `revoked` is re-judged on each replay, so the check should still run
    before a stored fact is returned. The text does not say so in one place; a replay that
    returned the stored content without the check would disclose after revocation.
  - The table's FK to `workflow_channel_calls` has no stated delete rule. The record is
    removed by cascade when its tree goes; the fact must go with it (B5 asserts "all call
    rows go with the tree").
- **Required (in the batch).** Keep the live-authority check ahead of returning a stored fact,
  with a test (fact stored, then revoked, replay answers `revoked`); give the FK a cascade
  that the tree delete satisfies. Record both in TASK.
- **Resolution history.** 2026-11-23 open; deferred by B to batch AWC-P04-B1.


### AWC-P04-D1-F8 — resolution update

  - 2026-11-23, fixed (batch review AWC-P04-B1-R1-N1, digest `555771ee…0a7c`): `consume_on`
    runs `still_authorized_on` before `stored_on` (`channel_call_dispatch.rs`), and
    `channel_call_response_consumed_once` stores a fact, disables the desk and reads `revoked`,
    then the stored value again once re-enabled. The facts table's FK is
    `ON DELETE CASCADE` to the record and has an immutability trigger. Closed.

## AWC-P04-B1-F1 — `channel_call_resume_cancel_races` fails intermittently on this digest

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P04-B1, stage batch review; round
  [AWC-P04-B1-R1-N1](agent-review-notes.md#awc-p04-b1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P04; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3` (work uncommitted); artifact digest `555771ee2d824f5a556e08cf1b7e1746528a754fadb9e8daeb1020911b2b0a7c`. Originates in the batch (or in p03's test, exposed here); not in TASK.
- **Classification:** actionable; medium; reproducibility of a concurrency test the batch
  relies on (doc 03 B "race cancel with launch and completion"; AGENTS.md competing-claimant
  rule).
- **Evidence.** B's run of `cargo test --locked --offline --lib channel_call_ --
  --test-threads 4`: 79 passed, 1 failed. The failure, and 1 of 5 runs of that test alone:
  `channel_call_resume_tests.rs:63` unwraps `Err(Conflict("Workflow lifecycle revision or
  command conflict"))` returned by `cancel` ([log](logs/AWC-P04-B1-R1-reviewer-focused.txt)).
  The helper retries `CancelResult::RevisionConflict` but not this error
  (`workflow/mod.rs:148`). TASK reports 80/80 and three clean repeats of the review races
  only. B did not find which racing transition returns the error, or whether p04 changed how
  often it happens (B's single p03 run passed).
- **Required.** Find the path: say whether a cancel racing a call's launch or completion is
  meant to return this conflict to its caller. Then either retry it in the helper with that
  reason recorded, or fix the production path. Show a repeated run (20 or more) of the test
  clean, and the whole `channel_call_` group clean.
- **Instruction lesson (candidate).** A race test is evidence only with its repeat count; one
  pass of a test that fails one run in five proves nothing either way.
- **Resolution history.** 2026-11-23 open.

## AWC-P04-B1-F2 — Gate cases for A4 and A8 that the tests leave out

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P04-B1, stage batch review; round
  [AWC-P04-B1-R1-N1](agent-review-notes.md#awc-p04-b1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P04; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3` (work uncommitted); artifact digest `555771ee2d824f5a556e08cf1b7e1746528a754fadb9e8daeb1020911b2b0a7c`. Originates in the batch's tests; not declared in TASK.
- **Classification:** actionable; low; verification gap against gate r2's rows.
- **Evidence.** Gate A4: hand-off "to an uncalled run, a revoked/unconsumed call, or a
  non-`done` call gives `hand_off_invalid`". The test covers only a random run id
  (`channel_call_response_tests.rs:341-352`). The filter `result.closure='done'`
  (`channel_call_results.rs`, and the guard `channel_reply_proposal_hand_off`) is never
  exercised with an existing fact of another closure, and the case F2 of the gate was about
  (a nested call answered `revoked`, so no fact) is not shown. Gate A8 names a
  `workflow.call` child; the test covers a workflow-tool child and a manual run.
- **Required.** Add a hand-off naming a nested call whose fact is `denied` or `no_reply`, and
  one whose consumption answered `revoked`; add the static-child case to A8 or record why the
  tool child stands for it.
- **Resolution history.** 2026-11-23 open.

## AWC-P04-B1-F3 — Optional notes

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P04-B1, stage batch review; round
  [AWC-P04-B1-R1-N1](agent-review-notes.md#awc-p04-b1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P04; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3` (work uncommitted); artifact digest `555771ee2d824f5a556e08cf1b7e1746528a754fadb9e8daeb1020911b2b0a7c`.
- **Classification:** optional; low.
- **Evidence.** `message.ask` is refused only when the asking run itself is the call owner
  (`message.rs:181-191`); a static child of a call child inherits the thread but owns no
  response, so its `message.ask` is not refused by this rule. B3's approval-versus-revocation
  outcome (approval applies, release answers `revoked`) matches gate D4 and is accepted as
  declared. The positive hand-off test completes the agent step with a fixed output; the
  nested call runs through the production port, which is the part under test.
- **Resolution history.** 2026-11-23 recorded.

### AWC-P04-B1-F1…F3 — resolution updates (recheck AWC-P04-B1-R2-N1, digest `7af1aa2e…c83b`)

Recheck by B' (Opus 5.5, effort as assigned: medium; Opus substitute for the Fable reviewer),
round [AWC-P04-B1-R2-N1](agent-review-notes.md#awc-p04-b1-r2-n1), 2026-11-23.

- **F1, fixed (test defect).** `channel_call_resume_tests.rs:46-73` now takes a fresh command key
  per attempt (`{key}-{attempt}`) and retries only `RevisionConflict`. The cause is confirmed
  independently of the author's account: `control_tests.rs:77-121`
  (`workflow_control_competing_cancel_replay_closes_exact_expired_attempt`) asserts that the same
  cancel key replayed with another expected revision is `Err(Conflict(_))`, the error B saw in R1.
  So the production idempotency rule was right and the helper was wrong. Evidence: the author's
  25/25 (`logs/AWC-P04-B1-F1-repeats.txt`) and B's 10/10 interleaved with `channel_call_review_races`
  ([log](logs/AWC-P04-B1-R2-reviewer-repeats.txt)). `ToolSetup::cancel`
  (`workflow_tool_support.rs:362-382`) still asserts `Applied` without a retry; in B's 10 runs of
  `channel_call_review_races` the race-cancel arm never conflicted. Closed.
- **F2, fixed.** `hand_off_through` (`channel_call_response_tests.rs:1377-1488`) drives a real
  nested `channel.call` from the destination's agent. `channel_call_response_hand_off_needs_a_done_call`
  covers a stored `denied` fact and a revoked nested call (no fact). Each case asserts the
  stored closure, child `failed`, `hand_off_invalid` audited, no proposal, and caller `child_failed`.
  The static-child A8 case is `channel_call_response_steps_require_owner_in_static_child`. Never
  consumed and revoked are the same "no fact" state for `handed_off_on`, so the revoked case
  stands for both. The DB guard `channel_reply_proposal_hand_off` is a backstop the Rust precheck
  always reaches first; it is verified by inspection only. Closed.
- **F3 (optional), taken.** `message.rs:179-201` walks the ancestry (depth ≤ 64) to a
  `workflow_channel_calls.child_run_id` before any route or intent, so the walk is false for every
  ingress tree and the ingress path is otherwise unchanged. B ran the ingress question/ask suites
  after the change (see R2 note). New test `channel_call_response_ask_refused_in_static_child`
  passes. Closed.

## AWC-P04-B1-F4 — A call-owned review page links "Source message" to the caller tree's ingress message

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P04-B1, stage batch review (full, R2); round
  [AWC-P04-B1-R2-N1](agent-review-notes.md#awc-p04-b1-r2-n1); reviewer B', Opus 5.5 (effort as
  assigned: medium), the Opus substitute for the Fable reviewer; phase AWC-P04; worktree
  `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3` (work uncommitted); artifact digest
  `7af1aa2ed955dfc0599bfa4bd4c0501c3ebacefc9d143b2d4ed84f3bb84cc83b`. Originates in the batch:
  p03 declined reviewed desks, so p04's D4 makes this page reachable for a call. Not a gate defect.
- **Classification:** actionable; low; requirements (doc 04 B.1 display; gate D4 "Display").
- **Evidence (inspection).**
  - `decisions/read.rs:71-76` sets `trigger_message` from `workflow_message_intent_bindings` of
    `budget.root_run_id`. A call child shares its caller's budget tree, so for a call-owned final
    review this is the caller tree's root ingress message, if any. The CHECK on
    `workflow_run_budgets` makes a child's `root_run_id` differ from its own id.
  - `workflow_decision.rs:110-121` renders it as "Source message", linking
    `/ui/message?channel_id=<destination desk>&thread_id=<call thread>&message_id=<caller's message>`.
  - `ui_message.rs` scopes the message to the thread, so the link resolves to "Message not found
    in this thread". Content is not disclosed, but the caller's ingress message id is exposed
    across desks.
  - The destination's reviewer therefore gets a broken "Source message" link that points toward
    the caller's customer conversation, beside the new `response.note` saying the answer
    returns to an agent. The call's real source is the call's `source_message_id`
    (`channel_reply_owners.message_id`).
  - No test covers this: B1 (`channel_call_review_approve_releases`) uses a caller with no ingress
    root and does not assert `context.trigger_message`.
- **Required.** For a run owned by a call, take the trigger from the owner, for example
  `channel_reply_owners.message_id` for the reviewed run when it has an owner row, otherwise the
  current root lookup. Alternatively, show no source message. Assert `view.context.trigger_message`
  in B1, ideally with a caller whose root was admitted from a message.
- **Instruction lesson (candidate).** When a child gains a new owner kind across a tree boundary,
  check that the page readers keyed on "the tree's root" (here, `budget.root_run_id`) still mean the
  right thing. The gate's reader table was built from `channel_reply_*` and `intent_id` searches,
  so `root_run_id`-keyed readers fell outside it.
- **Resolution history.** 2026-11-23 open.

## AWC-P04-B1-F5 — Doc 04 surfaces with no test and no recorded disposition

- **Context:** as AWC-P04-B1-F4 (round AWC-P04-B1-R2-N1, digest `7af1aa2e…c83b`). Originates in the
  batch's verification record.
- **Classification:** actionable; low; verification (mapping completeness). It can be closed by a
  record change; B's inspection below supplies the basis.
- **Evidence.**
  - Doc 04 B's focused checks require that raw output never leaks "through a tool result, sample
    preview, notification, run view, or logging path". Gate r1 B4 names "the run views".
  - `channel_call_privacy_no_raw_leak` reads tables: the stored fact, the caller's executions,
    notifications and messages. It does not read a run-view projection, a sample preview or logs,
    and TASK's "Not proved by stand-ins" does not say so.
  - B's inspection supplies the dispositions:
    - In a sample, the `channel.call` tool returns its fixture (`sample/tools.rs:123-128`,
      `ToolKind::ChannelCall`), so no destination runs.
    - The call path logs ids only (`channel_call_dispatch.rs:72-77`,
      `channel_reply_completion.rs:390-392, 485`).
    - A caller-run view reads the caller's own executions, which the test covers. The child's run
      is behind the child's own visibility.
  - Doc 04 A's "suppression isolation" (gate A3: "the parent's own later response is unaffected")
    is not asserted. The caller in `channel_call_response_no_reply` owns no response. Structurally,
    decisions are keyed per run (`PRIMARY KEY (company_id, run_id)` and `UNIQUE (company_id,
    owner_id)`), so a child's `suppressed` row cannot be the caller's.
- **Required.** Record these dispositions, or add assertions, in TASK's mapping and its "Not proved
  by stand-ins" list: run view, sample preview, logging, and A3's caller-response isolation.
- **Instruction lesson.** None new beyond the existing verification-contract rule: "record
  mismatches between promised behavior and actual fixture paths/assertions".
- **Resolution history.** 2026-11-23 open.

## AWC-P04-B1-F6 — Optional test tightenings

- **Context:** as AWC-P04-B1-F4 (round AWC-P04-B1-R2-N1, digest `7af1aa2e…c83b`).
- **Classification:** optional; low; verification.
- **Evidence and suggestions.**
  - B5 (`channel_call_privacy_retention`): the finished tree is retired with 0 stored facts,
    because the caller is cancelled before consuming. So the facts FK `ON DELETE CASCADE` (gate F8)
    is not exercised by production retention, only by inspection. A consumed-then-aged tree would
    show it.
  - B5: "a tree with an open review is not eligible" is in the comment but not asserted. A
    `retire` before the cancel would assert it.
  - A11 static (`channel_call_response_ask_refused_in_static_child`): asserts the failure code
    but not the gate's "no wait exists".
  - A1: the "both keys" row is asserted by SQLSTATE `23514`, not by constraint name. The gate's
    "ingress run naming a call" forged case is not exercised.
  - The R2 clippy run allows `redundant_field_names` and `nonminimal_bool` crate-wide, so a new
    instance in p04 code after `try2` would be hidden. A run without the allows, filtered to
    changed files, would show it.
- **Resolution history.** 2026-11-23 recorded.

### AWC-P04-B1-F4…F6 — resolution updates (recheck AWC-P04-B1-R3-N1, digest `7b3f300c…8a13`)

Recheck by B' (Opus 5.5, Opus substitute reviewer), round
[AWC-P04-B1-R3-N1](agent-review-notes.md#awc-p04-b1-r3-n1), 2026-11-23.

- **F4, fixed.**
  - `channel_call_source.rs:87-107::enclosing_call_source_on` returns the source of the nearest
    call child at or above the run (depth ≤ 64, nearest first).
  - `decisions/read.rs:71-86` uses it first and falls back to the tree root's ingress lookup only
    outside a call subtree, so the link is now the call's source on the call's desk thread.
    A static child inherits the call's channel and thread, so this holds for it too.
  - `message.rs:183-194` uses the same helper; its behaviour is unchanged from R2.
  - `channel_call_review_approve_releases` asserts `context.thread = record.thread_id` and
    `trigger_message = record.source_message_id` (`channel_call_response_tests.rs:614-615`).
  - The `run_views::trigger_message` disposition is accepted (`run_views.rs:371-380`). It reads
    the run's own ingress binding, so a call child shows no link rather than a wrong one.
  - B's run passed 42/42 ([log](logs/AWC-P04-B1-R3-reviewer-focused.txt)). Closed.
- **F5, closed by record.** TASK's R2-correction entry records the run view, sample preview,
  logging and A3 dispositions with code references. They match B's R2 inspection.
- **F6.**
  - The no-wait assertion was taken (`channel_call_response_tests.rs:558-563`).
  - The other items are dispositioned with reasons, which are acceptable for an optional finding.
    One correction to F6: an open-review tree has no `finished_at`, so asserting it is ineligible
    would be vacuous, as the author says.
  - Closed.

## AWC-P04-B2-F1 — Optional notes on the caller note

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P04-B2, stage batch review; round
  [AWC-P04-B2-R1-N1](agent-review-notes.md#awc-p04-b2-r1-n1); reviewer B', Opus 5.5 (effort as
  assigned: medium), the Opus substitute reviewer; phase AWC-P04; worktree
  `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3` (uncommitted); digest
  `46e25d956441f5ef6901f1294b044abfa81842d199f39a64c36243111dfa995c`.
- **Classification:** optional; low; requirements record and verification.
- **Evidence and suggestions.**
  - Doc 04 C.2 names `add_internal_note` and the `internal_notes` table. `channel_call_note::file_on`
    instead writes a platform-authored `MessageWrite::internal` with entry kind `Note` through
    `insert_message_on`. That is the same shape the B1 `ReplyInternal` reply uses.
    `create_internal_note` (`thread/internal_note.rs:142`) needs a human principal's direct authority
    and a command id, and writes an `internal_notes` row, which a system note cannot honour. So the
    choice is sound, but TASK does not say that it departs from C.2's named writer, and it should.
  - The note is written for error closures too: `closure: error (code)`. No test covers an error
    closure's note body.
  - Idempotence under competing consumers rests on the fact's primary key, `ON CONFLICT DO NOTHING`
    in the same transaction: a second inserter waits on the first and then sees `false`. That is
    correct by inspection. The note tests replay sequentially only.
  - A failure to insert the note fails the consumption, so the caller cannot read its result.
    Gate r2 chose this by placing the note in the consumption transaction. It is acceptable while
    the caller's thread always exists for a live caller run.
- **Resolution history.** 2026-11-23 recorded.
