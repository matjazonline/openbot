# AWC-P03 review notes

## AWC-P03-D1-R1-N1

**Round.** 2026-11-22; architecture gate AWC-P03-D1 r1 (sha256 `0e239c5336296c0d6d2a531572379a4b8104b6fae0b4ee6e5f21ace25835445c`); reviewer B, Fable 5.2
(effort as assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`,
branch `plan08_ph1`, HEAD `fe7791d3`; pre-gate artifact digest `d824fa22592fc256b5e8bdbd1ef64089963eb95016924c10478bfe16d2701dcf` (exclusions
`plan/agent-workflow-call/phases/p03/docs`, `.trail/learnings`), confirmed before and after.
Coordination agent-workflow-call r11 (AWC-C001 r4, AWC-C002 r1, AWC-C003 r2).

**Coverage and evidence.** Inspection only; no tests run. Read the decision, doc 03, doc 04,
p01 gate r3 D1.4/D1.5/D2, AWC-C003 r2 and the accepted p02 `start_on`.

Facts checked against code:
- Fact 2: `workflow_tool_calls::call_on` order is authorize, `lock_fence`, replay, limit,
  `create_on`, park (`workflow_tool_calls.rs:154-204`). Holds.
- Fact 5: `write_destination_on` creates the thread and calls `insert_message_on` with a
  Workflow stamp, delivery only when planned (`message_writer.rs:15-50`); the producer seal
  requires a destination row or a published decision, then author validity and origin
  (init schema, `check_workflow_message_producer_seal`). Holds.
- A completing run finds its reply context through `channel_reply_runs` for itself
  (`channel_reply_completion.rs:37-47`), so a call child with no intent is `Unowned` and the
  existing reply path publishes nothing for it.
- Fact 8: `workflow_run_budgets` has no depth column; no dynamic depth bound was found.
- `message_internal_relays` is written only by `inbound.rs`, so a message stored through
  `insert_message_on` is not an internal relay row and the relay-target guard returns early.

Judgements:
- D2 seal and author arms: sound. The record is immutable, unique per source message, tied
  to thread and message by FK and to the child by the snapshot check, and inserted in the
  same transaction as the message. Storing through `insert_message_on` writes no intent, so
  no reply context, responder admission or delivery follows; L1 and L5 assert it.
- D3: sound; the trigger ties record, wait and tool-call id without a shared FK.
- D5 matches p01 D1.4. D6 covers nested call children through the cancel cascade.
- D1, D4: see findings.

**Not verified.** Facts 1, 3, 4, 6, 7, 9, 10, 11 beyond the citations above; triggers on
`threads`/`thread_messages` other than those named (notifications, direct notifications) for
side effects on a desk thread; that every writer of grants, desk enablement and binding
activation takes the company lock.

**Outcome.** Changes required. Actionable: AWC-P03-D1-F1, F2 (medium), F3, F4, F5 (low).
Optional: F6. See [agent-review-findings.md](agent-review-findings.md).

**Process and instruction feedback.** The Facts section with locations made the check fast.
F1 comes from composing accepted functions in a new order without re-reading their inputs.


## AWC-P03-D1-R2-N1

**Round.** 2026-11-22; gate recheck AWC-P03-D1 r2 (sha256 `516b08b3940d8ccbeca6622dd68b5147bf1309f476b66e69e58e0da1f0b61fe3`, a delta on r1 `0e239c53…445c`);
reviewer B, Fable 5.2 (effort as assigned: medium); worktree
`/Users/mac02/orca/workspaces/openbot/plan08_ph1`, HEAD `fe7791d3`; pre-gate artifact digest
`d824fa22592fc256b5e8bdbd1ef64089963eb95016924c10478bfe16d2701dcf`, confirmed before and after. Coordination agent-workflow-call r11.

**Coverage and evidence.** Inspection only. Read r2 in full and rechecked F1–F6 by ID. Checked
in code: the thread FK of `workflow_runs` is `NO ACTION` (init schema line 11355) and no
production statement deletes threads; `ReplyApprovalPolicy::new` refuses an empty reviewer
list. The step-0 argument and the share-lock conflict claim were checked by reasoning (see
the resolution updates).

**Not verified.** The lock modes inside `agent_origin::authority_on`; the lock order of the
desk-enablement, grant and binding-deactivation writers relative to the company lock (a
deadlock there would show in R4's race cases); that `saved_call_on` still finds the call
pending on a resumed attempt (R1 exercises it).

**Outcome.** PASS for decision AWC-P03-D1 r2 (sha256 `516b08b3940d8ccbeca6622dd68b5147bf1309f476b66e69e58e0da1f0b61fe3`, with r1's unreplaced text), on
pre-gate digest `d824fa22592fc256b5e8bdbd1ef64089963eb95016924c10478bfe16d2701dcf`. F1–F6 closed. No new findings.

**Process and instruction feedback.** Two rounds. The delta format named exactly what it
replaced, which kept the recheck short.


## AWC-P03-B1-R1-N1

**Round.** 2026-11-23; batch review AWC-P03-B1-R1 (B1+B2 as one batch, per TASK "Batches");
reviewer B, Fable 5.2 (effort as assigned: medium); worktree
`/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, HEAD `fe7791d3` (work
uncommitted); artifact digest `b9271b7d9295ac2d77e8ecad0c69424a834eea3261b2ef936c66829c3baf6dbb` (exclusions `plan/agent-workflow-call/phases/p03/docs`,
`.trail/learnings`), confirmed before and after. Governing decision AWC-P03-D1 r2 on r1;
coordination agent-workflow-call r12.

**Coverage and evidence.** Read in full: `channel_call_dispatch.rs`, `channel_call_source.rs`,
the revised `start_on`, `write_call_on`, `continue_call`, `agent_origin::channel_call_on`,
the archive and `knowledge.rs` diffs, the launch guard and the producer-seal and
author-valid arms in the init schema, the runtime routing diff in `workflow_agent/mod.rs`,
and the R3/R4 tests; listed every test and raw statement in the launch, resume and support
files. Read TASK's implementation, corrections, mapping and checks. The p03 delta cannot be
diffed alone (p02 is uncommitted), so files were read as they stand.

- D1: `start_on` follows r2's order (desk alone, prelock, fence, replay, parse, origin,
  steps 4–8 with the limit and the review rule at the end of 8, then the savepoint).
- Stale retry: only `CallStart::Stale` loops; a refused park is an `Err` that leaves
  `call_channel` at once and drops the transaction. Sound.
- D4: `still_authorized_on` re-reads origin and execution authority, then the desk row
  (`FOR SHARE`, unit `FOR KEY SHARE`), the grant, then the binding (`FOR SHARE`), the same
  order as the launch's `target_on`. Revocation is judged before any child fact is read; the
  size check precedes absorption, so nothing is absorbed without a disclosed result.
- `continue_call` skips `call_result` only when a record exists for the wait's child; the
  record is unique per child.
- D5: the call case lowers by human readers and the child's ceiling; any other thread is
  unchanged. A grandchild of a call child gets the ordinary rule, which is stricter.
- D6: the selection adds recorded call children on the subtree's desks whose caller's desk
  is outside it (or absent); a caller inside is reached through its own ancestor. Roots are
  ordered first; one bound covers both.
- Validated the evidence manifest (9 PASS, 3 FAIL, 2 NOT EXERCISED). Ran
  `cargo test --locked --offline --lib channel_call_ -- --test-threads 4` → 61 passed
  ([log](logs/AWC-P03-B1-R1-reviewer-focused.txt)).

**Not verified.** `admission_history::capture_for`, `infra/workflow.rs` wiring,
`channel_call.rs` (application) changes and `workflow_tool_support.rs` were not read. The
affected and caller suites, stack budget, sqlx check, fmt and clippy were not rerun by B.
The production grant-sync writer's lock order was not read (see F1). The full suite was not
run; that gate remains the author's at the end of the task.

**Outcome.** PASS for digest `b9271b7d9295ac2d77e8ecad0c69424a834eea3261b2ef936c66829c3baf6dbb`, with one non-blocking low finding, AWC-P03-B1-F1
(actionable, settled by a TASK correction or an added race), and optional AWC-P03-B1-F2.
See [agent-review-findings.md](agent-review-findings.md).

**Process and instruction feedback.** One round. The stand-in column in TASK made the raw
updates easy to judge; the one row that missed its stand-ins is F1.
