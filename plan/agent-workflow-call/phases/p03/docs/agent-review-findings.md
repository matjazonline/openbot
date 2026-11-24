# AWC-P03 review findings

## AWC-P03-D1-F1 — Step 1 needs the desk from the arguments, but the parse is step 4

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); phase AWC-P03, stage architecture gate; round
  [AWC-P03-D1-R1-N1](agent-review-notes.md#awc-p03-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD
  `fe7791d3`; decision [AWC-P03-D1 r1](logs/AWC-P03-D1-r1-decision.md), sha256 `0e239c5336296c0d6d2a531572379a4b8104b6fae0b4ee6e5f21ace25835445c`;
  pre-gate artifact digest `d824fa22592fc256b5e8bdbd1ef64089963eb95016924c10478bfe16d2701dcf`. Originates in the decision.
- **Classification:** actionable; medium; transaction order (D1) and AWC-C002.
- **Evidence.** D1 step 1 keeps `start_on` steps 1–3 unchanged. Step 1 there is
  `prelock_channel_call_on(tx, action, desk)` with `desk = dispatch.request.channel`
  (`channel_call_target.rs:167-171`), and `CallDispatch` carries an already parsed
  `ChannelCallRequest`. D1 step 2 says replay happens "before any request parse", and step 4
  parses with `ChannelCallRequest::parse(arguments, now, run.deadline)`, one function that
  also judges the deadline. As written the account union cannot be built before the parse,
  and a parse before the replay lookup re-reads the deadline, which AWC-C002 forbids (a
  resumed call whose own deadline has passed would answer `deadline_invalid` instead of its
  result). Malformed arguments (no `channel_id`) also have no stated path past step 1.
- **Required.** State how step 1 gets the desk: for example a shape-only parse that does not
  judge the deadline, with the deadline judged at step 4 only when no record exists; or a
  replay lookup that needs no desk. Say what `invalid_input` does before step 1, and change
  `CallDispatch`/`start_on` accordingly. R2 must include a replay after the call's own
  deadline has passed.
- **Instruction lesson (candidate).** When a decision reorders steps of accepted code, check
  each step's inputs against the new order, not only its effects.
- **Resolution history.** 2026-11-22 open.

## AWC-P03-D1-F2 — The interim success arm releases a response the desk's review has not approved

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); phase AWC-P03, stage architecture gate; round
  [AWC-P03-D1-R1-N1](agent-review-notes.md#awc-p03-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD
  `fe7791d3`; decision [AWC-P03-D1 r1](logs/AWC-P03-D1-r1-decision.md), sha256 `0e239c5336296c0d6d2a531572379a4b8104b6fae0b4ee6e5f21ace25835445c`;
  pre-gate artifact digest `d824fa22592fc256b5e8bdbd1ef64089963eb95016924c10478bfe16d2701dcf`. Originates in the decision.
- **Classification:** actionable; medium; authority/confidentiality at the p03/p04 boundary.
- **Evidence.** D4 step 3: a `succeeded` child's output is projected with the recorded
  contract and returned as `done`. The record also holds the desk's `approval_policy` (p02
  D1), which D4 never reads. Doc 03 B.2 keeps "child execution termination" apart from
  "callable response readiness"; doc 04 B.1: "Child workflow completion must not release an
  unapproved response to the model." p01 D2 lists `review_rejected`/`review_expired` for 04.
  Between p03 and p04 a desk configured for human review of its final response would return
  that response to any agent allowed to write to it, unreviewed.
- **Required.** In p03 the success arm discloses only when the record's approval policy needs
  no review. Otherwise it answers a typed error without content or absorption (or the call is
  refused at admission until p04); name the code and add the case to R3. p04 then replaces
  the arm.
- **Instruction lesson (candidate).** An interim form of a later phase's behaviour must fail
  closed on every control the later phase is there to enforce.
- **Resolution history.** 2026-11-22 open.

## AWC-P03-D1-F3 — Tree limits run ahead of D1.1's order, and the new depth bound has no stated source

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); phase AWC-P03, stage architecture gate; round
  [AWC-P03-D1-R1-N1](agent-review-notes.md#awc-p03-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD
  `fe7791d3`; decision [AWC-P03-D1 r1](logs/AWC-P03-D1-r1-decision.md), sha256 `0e239c5336296c0d6d2a531572379a4b8104b6fae0b4ee6e5f21ace25835445c`;
  pre-gate artifact digest `d824fa22592fc256b5e8bdbd1ef64089963eb95016924c10478bfe16d2701dcf`. Originates in the decision.
- **Classification:** actionable; low; contract order and scope.
- **Evidence.** p01 D1.1 puts `workflow.tool_limit` in step 8, after the desk and write-right
  check, cycle, closures, intake and levels; its error list is "in D1.1 order". D1 step 5
  runs both limits before `start_on` step 4, so a caller gets `tool_limit` where D1.1 gives
  `target_unavailable`, `cycle` or an intake error. The depth count is "computed from the
  same recursive ancestry walk as the cycle check", which runs later, at step 4.
  `MAX_CHANNEL_CALL_DEPTH = 8` is new: doc 03 A.5 asks to preserve existing bounds, Fact 8
  says root budgets already bound activations, and no requirement or user decision names 8.
- **Required.** Place the limits at D1.1 step 8 (inside or after `start_on` steps 4–7), or
  amend the order explicitly with the reason. For the depth bound, state what it protects
  that the root budgets and the cycle check do not, or drop it; if kept, record it as a new
  model-visible limit for the user in TASK and the README.
- **Instruction lesson.** None new.
- **Resolution history.** 2026-11-22 open.

## AWC-P03-D1-F4 — Verification table omits checks doc 03 names

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); phase AWC-P03, stage architecture gate; round
  [AWC-P03-D1-R1-N1](agent-review-notes.md#awc-p03-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD
  `fe7791d3`; decision [AWC-P03-D1 r1](logs/AWC-P03-D1-r1-decision.md), sha256 `0e239c5336296c0d6d2a531572379a4b8104b6fae0b4ee6e5f21ace25835445c`;
  pre-gate artifact digest `d824fa22592fc256b5e8bdbd1ef64089963eb95016924c10478bfe16d2701dcf`. Originates in the decision.
- **Classification:** actionable; low; verification plan.
- **Evidence.** Doc 03 B focused checks: "expire a lease at commit; restart after every
  durable boundary". The failure table describes lease loss at commit, but no test does it;
  no test restarts after the commit, after the child terminal and after the saved result as
  separate boundaries (R1 covers one path). Doc 03 A.5 names
  `workflow_child_timeout_fails_child_and_reaches_parent_error_route` as gaining the shorter
  call-deadline case; the table has only L1's "deadline = min". R4 has no case for the first
  D1.5 item (the calling agent's capability or execution authority withdrawn).
- **Required.** Add the lease-expiry-at-commit test (zero rows, including thread and
  message), the restart cases per boundary, the shorter-deadline case in the named test, and
  the capability-withdrawn case in R4, or mark each NOT EXERCISED with its owner.
- **Resolution history.** 2026-11-22 open.

## AWC-P03-D1-F5 — `response_invalid` and the replay re-check are under-specified

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); phase AWC-P03, stage architecture gate; round
  [AWC-P03-D1-R1-N1](agent-review-notes.md#awc-p03-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD
  `fe7791d3`; decision [AWC-P03-D1 r1](logs/AWC-P03-D1-r1-decision.md), sha256 `0e239c5336296c0d6d2a531572379a4b8104b6fae0b4ee6e5f21ace25835445c`;
  pre-gate artifact digest `d824fa22592fc256b5e8bdbd1ef64089963eb95016924c10478bfe16d2701dcf`. Originates in the decision.
- **Classification:** actionable; low; contract and authority detail.
- **Evidence.**
  - `channel_call.response_invalid` is not in p01 D2's error list. Doc 04 A.4 does require a
    typed error for an invalid projection, so the code is reasonable, but p01's list, the
    README and any model-visible text of the built-in must name it, and p04 must be told it
    exists (it owns the final success path).
  - D4 step 2 re-reads authority "under the locks already held". On a replay p02's `start_on`
    returns at step 3 and takes no desk, grant or binding lock ("a hit takes no destination
    lock"). The decision does not say whether `check_execution_on` is part of the re-check
    (D1.5's first item), nor which lock makes the three desk reads stable.
- **Required.** Add the code to the p01 contract list and README in this phase and raise a
  coordination notice to p04. State the replay re-check's reads and locks exactly: either
  take the share locks, or say the read is unlocked and why a concurrent revocation that
  commits after it is acceptable.
- **Resolution history.** 2026-11-22 open.

## AWC-P03-D1-F6 — Optional: deletion behaviour of the new thread and message keys

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); phase AWC-P03, stage architecture gate; round
  [AWC-P03-D1-R1-N1](agent-review-notes.md#awc-p03-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD
  `fe7791d3`; decision [AWC-P03-D1 r1](logs/AWC-P03-D1-r1-decision.md), sha256 `0e239c5336296c0d6d2a531572379a4b8104b6fae0b4ee6e5f21ace25835445c`;
  pre-gate artifact digest `d824fa22592fc256b5e8bdbd1ef64089963eb95016924c10478bfe16d2701dcf`. Originates in the decision.
- **Classification:** optional; low; schema detail.
- **Evidence.** The record gains FKs to `threads` and `thread_messages` with no stated
  `ON DELETE` rule, and the record is immutable (deleted only with its calling run). Deleting
  the destination thread or its message would then fail on the FK. Doc 04 B.6 owns retention,
  but the key is added here.
- **Suggested.** State the rule, matching what `workflow_runs.thread_id` does for the child.
- **Resolution history.** 2026-11-22 recorded.

### AWC-P03-D1-F1…F6 — resolution updates (recheck AWC-P03-D1-R2-N1, decision r2 sha256 `516b08b3…1fe3`)

- **F1.** 2026-11-22, fixed in the decision. Step 0 reads only `channel_id` with no lock; a
  missing or malformed one answers `invalid_input`. B checked the argument: a record is
  written only after the full parse, which needs a UUID `channel_id`, and a replay matches by
  argument digest, so no record can exist for arguments step 0 rejects. Replay precedes the
  full parse; R2 gains the past-deadline case; L11 covers step 0. Closed.
- **F2.** 2026-11-22, fixed in the decision. A desk whose settings carry an approval policy
  is declined at the end of D1.1 step 8 with `destination_unavailable`, before any child
  (L10). The placement is behind the write-right check, so it discloses nothing to a caller
  without the right. `ReplyApprovalPolicy` always names at least one reviewer
  (`channel_reply/approval.rs:41-47`), so `Some` means review is required. p04 removes the
  rule (AWC-C004). Closed.
- **F3.** 2026-11-22, fixed in the decision. The continuation limit sits at step 8 (L4 checks
  the order); `MAX_CHANNEL_CALL_DEPTH` is dropped with its reason. Closed.
- **F4.** 2026-11-22, fixed in the decision. L9, L12, R4 (capability withdrawn) and R9 added.
  Closed.
- **F5.** 2026-11-22, fixed in the decision. `response_invalid` goes into p01 D2's list and
  the README, with notice AWC-C004. The consumption reads and their share locks are listed,
  with `check_execution_on`. An UPDATE or DELETE of the desk, grant or binding row conflicts
  with `FOR SHARE` by PostgreSQL's row-lock rules, so the serialization claim holds. Lock
  order between those writers and the company lock is not argued in the decision; R4's
  race cases are the evidence B will look for in the batch. Closed.
- **F6.** 2026-11-22, closed. The keys are `NO ACTION`, the same as
  `workflow_runs_company_id_channel_id_thread_id_fkey`; no production code deletes threads.


## AWC-P03-B1-F1 — R4's grant and desk revocations are raw statements, and TASK lists no stand-in

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P03-B1 (B1+B2 merged), stage batch review; round
  [AWC-P03-B1-R1-N1](agent-review-notes.md#awc-p03-b1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P03; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3` (work uncommitted); artifact digest `b9271b7d9295ac2d77e8ecad0c69424a834eea3261b2ef936c66829c3baf6dbb`. Originates in the batch's tests and TASK mapping.
- **Classification:** actionable; low; verification record (stand-in limit). Not blocking: the
  fix is a TASK correction (outside the digest) or an added race, and the production order
  was checked by inspection.
- **Evidence.** `channel_call_resume_tests.rs:309-320`: `Revocation::Grant` deletes the grant
  row and `Revocation::Desk` runs `UPDATE channels SET enabled=false` directly on the pool.
  A single-statement writer cannot deadlock with the consumption, so
  `channel_call_resume_revocation_races_consumption` proves serialization on the row but not
  the lock order of the production writers. Gate r2 named R4's races as the evidence for
  that order. TASK's R4 row lists "—" under stand-ins. The binding and capability cases do
  use production services. By inspection the production channel edit locks users, then
  company ownership, then the gate, then the channel row `FOR UPDATE`
  (`channel_edit.rs:112-153,199`), which is compatible with the consumption's company-then-
  desk order.
- **Required.** Record the two stand-ins in TASK's R4 row with the inspected order, or race
  the production channel edit and grant sync instead of raw statements.
- **Instruction lesson (candidate).** A race test proves a lock order only when the competing
  writer is the production one; a raw statement is a stand-in and must be listed.
- **Resolution history.** 2026-11-23 open; non-blocking.

## AWC-P03-B1-F2 — Optional: declared gaps and one schema note

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P03-B1 (B1+B2 merged), stage batch review; round
  [AWC-P03-B1-R1-N1](agent-review-notes.md#awc-p03-b1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P03; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3` (work uncommitted); artifact digest `b9271b7d9295ac2d77e8ecad0c69424a834eea3261b2ef936c66829c3baf6dbb`.
- **Classification:** optional; low.
- **Evidence.** L6 does not exercise different resources per desk (doc 03 A names it; p02's
  capture tests cover resource capture generally). The stack "before" measurement was not
  taken (doc 03 asks for before/after; the budget gate at 2048 KiB passes and the frames
  after are recorded). `workflow_message_author_valid` now returns true when either the
  ordinary origins all match or the call arm holds; the call arm needs an immutable record
  whose agent is the message's author, so it cannot rescue another author.
- **Resolution history.** 2026-11-23 recorded; B accepts these as declared.
