# AWC-P04 review notes

## AWC-P04-D1-R1-N1

**Round.** 2026-11-23; architecture gate AWC-P04-D1 r1 (sha256 `56d9c53b8151ebc6cff9d5aabb3d9a1d3fca36c97df17dc919c0cb9806d4b9c5`, matches the request);
reviewer B, Fable 5.2 (effort as assigned: medium); worktree
`/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, HEAD `fe7791d3`.
Coordination agent-workflow-call r14 as the file stands.

**Digest.** The request named pre-gate digest `d6486c88…22d8`. B computed
`6ef4d6b4d0931236fddff460183e68de80be56264a907e45732506473fa45b53` with the same exclusions,
before and after. The file list equals the author's `logs/AWC-P04-D1-r1-snapshot.txt`; the
only tracked-or-untracked project file newer than that snapshot is
`plan/agent-workflow-call/COORDINATION.md` (no file under `src/` or `migrations/`). B
continued because this round reviews a decision whose own hash matches, and reports the
mismatch for the author to re-freeze.

**Coverage and evidence.** Inspection only; no tests run. Read the decision, doc 04, p01 D2,
and in code: every schema use of `channel_reply_runs/proposals/decisions/contexts` (listed by
search), `guard_channel_reply_run`, `guard_channel_reply_proposal`,
`guard_channel_reply_decision`, `require_channel_reply_decision`,
`channel_reply_publication_matches`; `channel_reply_completion.rs:37-150`;
`approval_accounts.rs:20-45`; `agent_preparation.rs:40-112`; `batch.rs:135-180`; the
`validate_output` call sites; `storage_retention.rs:30-60`.

Facts confirmed: ownership is keyed on ingress and root-only; `root_on` reads runs ⋈
contexts; only `channel.no_reply` gives `Suppressed`; terminal outputs are validated at
`batch.rs:144`.

Judged sound: reading the result from the child's decision row rather than its publication
state (D3); removing the interim review rule once the existing review path owns the call
(D4); no edit path; the caller note keyed by call (D6).

**Not verified.** TASK's investigation map beyond the lines above; `message_planner.rs` and
`wf/message.rs` on `ReplyInternal`; `validate_on`; the retention eligibility SQL; the
notification and view readers' behaviour for a row with a null intent.

**Outcome.** Changes required. Actionable: AWC-P04-D1-F1, F2, F3 (medium), F4, F5, F6, F7
(low). See [agent-review-findings.md](agent-review-findings.md).

**Process and instruction feedback.** The decision's fact map came from a helper and a spot
check; F1 and F5 are what a search over the keyed column and one read of the retention
statement would have shown. A decision that makes a key nullable should attach the search
result.


## AWC-P04-D1-R2-N1

**Round.** 2026-11-23; gate recheck AWC-P04-D1 r2 (sha256 `b1c7ff9a1f0601f6509bd52621d4df92d7eace814362143ccf6c3077f5df4927`, a delta on r1
`56d9c53b…b9c5`); reviewer B, Fable 5.2 (effort as assigned: medium); worktree
`/Users/mac02/orca/workspaces/openbot/plan08_ph1`, HEAD `fe7791d3`; pre-gate digest `f759143dc771fca7b3a3d87f3c492465fb480e70d3c7f941b9feb1c3530f5a03`
(exclusions `plan/agent-workflow-call/phases/p04/docs`, `.trail/learnings`), confirmed before
and after. Coordination agent-workflow-call r15.

**Coverage and evidence.** Inspection only. Read r2 in full and rechecked F1–F7 by ID.
Repeated the reader search over `src` (files naming a reply table; `intent_id` counts per
file; root-run conditions in the remaining readers) and read
`storage_retention_eligible.sql:15-25`. Confirmed the doc 04 C amendment in the working tree
and the U5 line in coordination r15.

- Reader table: complete against B's search (see the F1 update).
- D7: sound. `revoked` writes no fact; the hand-off reads only this run's `done` fact, which
  exists only after authority and absorption. Two details are F8.
- D2a: the refusal is at the terminal before any output is committed, in every unowned run
  (static child, workflow-tool child, manual run, observer, rerun), so the schema exception
  is never visible to a consumer that assumes the schema.
- D8: for a call owner the policy compared is the captured one, so a desk edit cannot fail
  or replace it; reviewer membership and clearance stay live at approval. `message.ask` is
  refused before a wait exists.

**Not verified.** The schema objects' behaviour once rewritten (the batch shows it); that
`message.ask` can only target the requester; user decision U5 beyond the author's record.

**Outcome.** PASS for decision AWC-P04-D1 r2 (sha256 `b1c7ff9a1f0601f6509bd52621d4df92d7eace814362143ccf6c3077f5df4927`, with r1's unreplaced text) on
pre-gate digest `f759143dc771fca7b3a3d87f3c492465fb480e70d3c7f941b9feb1c3530f5a03`. F1–F7 closed. New AWC-P04-D1-F8 (low, deferred to the batch, not
blocking). See [agent-review-findings.md](agent-review-findings.md).

**Process and instruction feedback.** Two rounds. Attaching the search that built the reader
table let B repeat it in a few commands.


## AWC-P04-B1-R1-N1

**Round.** 2026-11-23; batch review AWC-P04-B1-R1 (D1–D5, D7, D8); reviewer B, Fable 5.2
(effort as assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`,
branch `plan08_ph1`, HEAD `fe7791d3` (work uncommitted); artifact digest `555771ee2d824f5a556e08cf1b7e1746528a754fadb9e8daeb1020911b2b0a7c` (exclusions
`plan/agent-workflow-call/phases/p04/docs`, `.trail/learnings`), confirmed before and after.
Governing decision AWC-P04-D1 r2 on r1; coordination agent-workflow-call r17.

**Coverage and evidence.** The snapshot files list names only, so the p04 delta inside files
p03 already changed was read as the files stand. Read: `consume_on`, `settled_outcome_on`,
`review_failure_on`, `channel_call_results.rs`; in the init schema `channel_reply_runs`, its
guard's call arm, the `channel_reply_owners` view, `workflow_channel_call_results`, the
hand-off clause of `guard_channel_reply_proposal`; the diffs of `batch.rs`,
`pending_recovery.rs`, `channel_reply_publish_authority.rs`, `agent_preparation.rs`,
`approval_accounts.rs`; `user_scope_owner_on`; `policy_current_on`; the `message.ask`
refusal; and the owner, deny/hand-off, owner-required, consumed-once and hand-off-positive
tests. Read TASK's B1 entry and mapping.

- D1: the owner key is non-null and generated; the call arm ties the owner to the record's
  child, desk, thread, binding and `run_as`; the call FK is added after the calls table.
- D2a/D7: the refusals run in `prepare_pure` before the completion is committed; the
  hand-off reads only this run's `done` fact, and the schema guard repeats it.
- D3: authority, then the stored fact, then the decision and proposal; absorption only for a
  disclosed answer; the review mapping keys on the terminal execution's proposal decision.
- D8: a call owner's policy is the captured one; `message.ask` is refused before any route.
- AWC-C005: correct as a tightening. The scope walk stops at a call child and then finds no
  root row, so the child and its descendants capture no personal scope; that is p02 D1.2's
  fresh scope, and no p02 evidence depended on the old walk. The publisher change is U5.
- Evidence manifest validated (5 PASS, 2 FAIL). B's run: `channel_call_` group 79 passed,
  1 failed; `channel_call_resume_cancel_races` alone 4 of 5 passed
  ([log](logs/AWC-P04-B1-R1-reviewer-focused.txt)).

**Not verified.** `channel_reply/closure.rs`, the registry and exactness changes,
`decision_approvals::view_proposal`, the `channel_reply_completion.rs` and
`channel_reply_proposals.rs` writers, `notification/projection.rs`, and the review, privacy
and retention tests were not read. The affected suites, stack budget, sqlx check, fmt and
clippy were not rerun. The cause of F1 was not found. The full suite was not run.

**Outcome.** Changes required. Actionable: AWC-P04-B1-F1 (medium), F2 (low). Optional: F3.
Gate finding AWC-P04-D1-F8 closed. See [agent-review-findings.md](agent-review-findings.md).

**Process and instruction feedback.** The declared-gaps list was accurate for what it
named. The snapshot would be easier to use across phases if it listed a hash per file, so a
later phase's delta inside an earlier phase's uncommitted file could be isolated.

## AWC-P04-B1-R2-N1

**Round.**
- 2026-11-23; full batch review and F1–F3 recheck, AWC-P04-B1-R2 (D1–D5, D7, D8; D6 and C1 are B2).
- Reviewer: B', Opus 5.5 (effort as assigned: medium). **I am the Opus substitute reviewer**,
  standing in for the Fable reviewer, which hit its usage limit. The user authorized the substitute.
  Fresh context: I did not see R1's working, only its records.
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`. Phase start,
  review base and HEAD are all `fe7791d3`; the work is uncommitted.
- Artifact digest `7af1aa2ed955dfc0599bfa4bd4c0501c3ebacefc9d143b2d4ed84f3bb84cc83b`, exclusions
  `plan/agent-workflow-call/phases/p04/docs` and `.trail/learnings`. Confirmed before the review and
  again before returning.
- Gate AWC-P04-D1 r2 (sha256 `b1c7ff9a…4927`) on r1.
- Coordination `plan/agent-workflow-call/COORDINATION.md` r17. Applicable: r1, r2 and r15 (U1–U5),
  AWC-C001 r4, AWC-C002 r1, AWC-C004 r2 and AWC-C005 r1. Freshness: I read r17 at the start of the
  round, and it is the revision named in the assignment.
- Evidence manifests `logs/AWC-P04-B1-R1.evidence.json` (PASS 5, FAIL 2) and
  `logs/AWC-P04-B1-R2.evidence.json` (PASS 4, FAIL 1) both validate with `check-evidence.py`.
  R2 was checked against the digest above.

**Coverage, read against doc 04 A/B, doc 01 as the gate references it, and gate r1+r2.**
- Schema:
  - `channel_reply_runs` with its owner kind, generated `owner_id` and the guard's call arm;
  - the deferred call FK and the `channel_reply_owners` view;
  - the proposal's `closure`, `handed_off_run_id` and `fields`, with the integrity CHECKs;
  - the hand-off guard in `guard_channel_reply_proposal`;
  - the decision immutability list, `require_channel_reply_decision`,
    `require_channel_reply_proposal_links` and `channel_reply_publication_matches`;
  - `workflow_channel_call_results`: CHECKs, cascade FK and immutability trigger;
  - the retention guard on runs.
- Rust, read as each file stands:
  - `closure.rs` (all of it, with its unit tests);
  - registry `families.rs`, `mod.rs` (`TYPES`, `RESPONSE_STEPS`, the terminal-route rule), `help.rs`,
    `examples.rs` and `actions.rs`;
  - `batch.rs` (`is_pure_kind`, `execute_pure`, and the output-schema exemption in
    `validate_completion`) and `exactness.rs`;
  - `pending_recovery::response_step_refusal`, and the `prepare_pure` callers (worker
    `advance_pure`, and sample `dispatch.rs::step`);
  - `channel_reply_completion.rs`: `root_on`, `policy_current_on`, `project_on`, `stage_on` and
    `commit_on`;
  - `channel_reply_proposals.rs::insert_on`, with its bind order checked against the column list;
  - `channel_call_dispatch.rs`: `consume_on`, `settled_outcome_on`, `review_failure_on` and
    `still_authorized_on`;
  - `channel_call_results.rs`; `channel_call.rs` (outcome and `to_value`);
  - `decision_approvals::view_proposal`, with its reader `decisions/read.rs`, the decision page,
    visibility and history;
  - `channel_reply_publish_authority`, `publish_read`, views `.rs` and `.sql`, `approval_accounts`,
    `agent_preparation`, `memory_tool_checkpoint` (AWC-C005) and `children::write_call_on`'s owner
    insert;
  - `message.rs` (the F3 walk), `app_error.rs`, `app_error_impl.rs`, `actions/failure.rs`,
    `notification/projection.rs:117` and `channel_reply.rs::link_run`;
  - the interim-decline removal in `channel_call_target.rs:266-271`.
- Every non-test reader of `channel_reply_runs` / `channel_reply_contexts` in `src` (by grep) has a
  disposition that matches gate r2's reader table.
- Tests read: every function in `channel_call_response_tests.rs` (A1–A11, B1–B5, D7, and the
  hand-off pair); `channel_call_resume_tests::cancel`; the L10 replacement in launch tests;
  `publish_agent_workflow` and `bind_child_with` in `workflow_tool_support.rs`; and
  `Fixture::publish_with_builtins`.
- Removed-test check: B compared the `channel_call_` names in p03's logs with p04's. The only name
  removed is `channel_call_launch_review_desk_declines`, replaced by
  `channel_call_launch_review_desk_captures_policy`. That is intended, because p04 removes the
  interim rule (AWC-C004).
- AWC-C005 checked again: the climb in `user_scope_owner_on` now stops at a call child, and the
  publisher accepts `(parent IS NULL) = (owner_kind='ingress')`. I agree with R1 that this is a
  tightening.
- Clippy claim verified: the 10 findings (`clippy-1.95-try2`) are in 9 files that are unmodified in
  the worktree. Each was last changed by a commit before the base (`73afd5a7`, `3d7148b1`,
  `2edd0a73`). See F6 for the masking caveat. B's `cargo fmt --check` exits 0.

**Checks B ran.** All were serialized, on fixture `openbot_awc_mac02` / `_test` at localhost:5432,
against this digest.
- `cargo test --offline --lib -- channel_call_response_ channel_call_review_ channel_call_privacy_
  channel_reply_closure channel_call_resume_`: 34 passed, 0 failed
  ([log](logs/AWC-P04-B1-R2-reviewer-focused.txt), sha256 `470d14d2…6346`).
- `channel_call_review_races` and `channel_call_resume_cancel_races`, interleaved 10 times each:
  20 of 20 passed ([log](logs/AWC-P04-B1-R2-reviewer-repeats.txt), sha256 `1ee87f35…4e04`).
- The ingress ask regression for the F3 walk, `channel_reply_hitl channel_question_ message_ask`:
  35 passed, 0 failed ([log](logs/AWC-P04-B1-R2-reviewer-ask-regression.txt), sha256 `2bf12713…0ff1`).
- Inspected only: the author's stack budget (`channel_call_ message_`, 716 passed and 1 failed; the
  failure is the known unrelated `doop_agent_current_preparation_reaches_ready_before_model_io`,
  format 7 vs 8), sqlx `--check`, clippy, and the R1-round `channel_reply_` 315/315 and 396/396 runs.
  I did not rerun `channel_reply_` at this digest: the only production change since R1 is the
  `message.ask` walk, which my ask regression covers.

**Outcome: changes required (low).**
- F1, F2 and F3 are closed (see the resolution updates).
- New actionable findings:
  - [AWC-P04-B1-F4](agent-review-findings.md) (low): the review page's "Source message" for a
    call-owned review resolves to the caller tree's ingress message;
  - [AWC-P04-B1-F5](agent-review-findings.md) (low, closable by record): run view, sample preview,
    logging and A3's suppression isolation have neither a test nor a recorded disposition. B
    supplies the inspection basis.
- Optional: [AWC-P04-B1-F6](agent-review-findings.md).
- No acceptance-blocking correctness, security or concurrency defect was found in D1–D5, D7 or D8.

**Covered lightly or not verified.**
- Registry help and examples text was skimmed.
- The sample path for the two new steps was checked by inspection only: no owner check runs there.
  That is acceptable for a simulation, and the steps' output is still contract-validated.
- Not read: p03-owned diffs in `knowledge.rs`, `workflow_tool_calls.rs`, `workflow_agent/mod.rs`,
  `agent.rs` and `channel.rs`.
- Of `channel_call_launch_tests.rs`, only the L10 replacement was read.
- `channel_reply_*_tests.rs` renames: I checked only that they compile and that the R1 regression
  passed. I did not read them line by line.
- The DB hand-off guard and the facts cascade are verified by inspection only (F2, F6).
- Not run: the full suite, `message_` / `memory_scope` / `agent_history` / `run_scope` /
  `workflow_decision` at this digest, the stack budget, and sqlx.

**Process and instruction feedback.**
- The batch could be reviewed in one round, but only with a narrow reading of the shared files.
  The per-file hash list R1 asked for would still have helped.
- The author's records were accurate where I checked them. The F1 root cause is confirmed by an
  existing production test.
- The gate's reader-table method (searching for `channel_reply_*` and `intent_id`) missed readers
  keyed on the tree root (F4). The candidate instruction is in F4.

## AWC-P04-B1-R3-N1

**Round.**
- 2026-11-23; correction recheck AWC-P04-B1-R3 of F4–F6.
- Reviewer: B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer.
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, HEAD `fe7791d3` (uncommitted).
- Digest `7b3f300c9f6f1de7a926f8b000d3a106beae939a26bd276470d74708b2f88a13` (exclusions
  `plan/agent-workflow-call/phases/p04/docs`, `.trail/learnings`), confirmed before and after.
- Coordination r17, with the notices as in R2-N1.
- `logs/AWC-P04-B1-R3.evidence.json` validates against this digest (PASS 3, FAIL 1; the FAIL is
  the known unrelated format 7/8 test).

**Coverage.**
- Read the correction delta:
  - `enclosing_call_source_on`, and its two callers (`decisions/read.rs`, `message.rs`);
  - the new assertions in `channel_call_review_approve_releases` and in
    `channel_call_response_ask_refused_in_static_child`;
  - `run_views::trigger_message`, an equivalent surface;
  - TASK's "B1 correction for AWC-P04-B1-R2" dispositions.
- Run by B: `cargo test --offline --lib -- channel_call_review_ channel_call_response_ask_refused
  reviewed_delivery channel_reply_hitl`, 42/42 ([log](logs/AWC-P04-B1-R3-reviewer-focused.txt),
  sha256 `28a71794…6b76`).
- Inspected only: the author's 798-pass affected run, plus clippy, fmt and sqlx.
- Everything else from R2-N1 is reused: R2 found no defect there, and the delta touches only these
  readers and tests.

**Outcome: PASS** for digest `7b3f300c…8a13`. F4 is fixed, F5 is closed by record, and F6 is
closed. No open findings for batch B1.

**Not covered:** as R2-N1's list. No new gaps.

**Process feedback:** the correction stayed within the findings and reused one walk instead of
adding a second copy. No change to the instructions is supported.

## AWC-P04-B2-R1-N1

**Round.**
- 2026-11-23; batch review AWC-P04-B2-R1 of the thread trail: doc 04 C, gate r2 D6 "Caller note"
  (replaced), gate r1 D6 and its verification row C1.
- Reviewer: B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer.
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, HEAD `fe7791d3` (uncommitted).
- Digest `46e25d956441f5ef6901f1294b044abfa81842d199f39a64c36243111dfa995c` (exclusions
  `plan/agent-workflow-call/phases/p04/docs`, `.trail/learnings`), confirmed before and after.
- B1 was accepted at `7b3f300c…8a13`. Coordination r18 (B1 acceptance only); notices as in R2-N1.
- `logs/AWC-P04-B2-R1.evidence.json` validates against this digest (PASS 4, FAIL 1; the FAIL is
  the known unrelated format 7/8 test).

**Coverage.**
- Code read:
  - `channel_call_note.rs` in full;
  - the `consume_on` call site, which files the note only on the first `insert_on`, after
    absorption, in the same transaction;
  - the shape of `tool_error` / `ChannelCallError::to_value`, for the `["error"]["code"]` read;
  - `create_internal_note` and `add_internal_note`, for comparison with doc C.2.
- Tests read: `call_notes`, `filed`, `trail_call`, `channel_call_trail_reply_and_note` (replied,
  `no_reply`, `denied`, threadless) and `channel_call_trail_agent_caller_continues`.
- Requirements, checked against the code and the assertions:
  - C.1: one reply on the desk thread (the B1 publisher). `no_reply` writes nothing; `denied` has
    the reason as its body.
  - C.2: one note on the caller's thread; a threadless caller gets none. It names the desk, child
    and closure, is authored by the system with the agent named, and is Public at level 0.
  - C.3: neither side quotes the other (asserted both ways). Classification is absorbed before the
    note.
  - C.4: nothing reads the note back.
  - Replay writes nothing twice (asserted). No intent, delivery or ingress (asserted).
- I accept the author's stand-in: "a reader of one desk sees nothing of the other" is shown
  structurally (separate threads, no quoting, no content), not by an actor-level read test.
- B's run: `cargo test --offline --lib -- channel_call_trail_ channel_call_resume_competing
  channel_call_response_automatic_done channel_call_response_consumed_once`: 5/5
  ([log](logs/AWC-P04-B2-R1-reviewer-focused.txt), sha256 `792e980c…06c4`).
- Inspected only: the author's stack budget (551 passed, 1 known unrelated failure), clippy, fmt,
  sqlx and source_modules.

**Outcome: PASS** for digest `46e25d95…995c`. Optional: [AWC-P04-B2-F1](agent-review-findings.md).
No actionable findings.

**Not covered.** No actor-level read test (the declared stand-in). No error-closure note test.
No concurrent-consumer note test; correctness there rests on the fact's primary key, by
inspection. The full suite was not run.

**Process feedback.** A small, well-scoped batch, with an accurate record. When a plan names a
specific writer (`add_internal_note`) and the implementation uses another, a one-line note of the
departure in TASK would save the reviewer the comparison.
