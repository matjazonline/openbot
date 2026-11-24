# AWC-P04 — Response ownership and review

## Identity

- Plan `plan/agent-workflow-call`, phase `p04`, doc [04-response-ownership-and-review.md](../../../04-response-ownership-and-review.md).
- Workflow implement-opus-reviewed-2_4; records at
  `/Users/mac02/orca/workspaces/openbot/plan08_ph1/plan/agent-workflow-call/phases/p04/docs`.
- Root/A: Claude Code `1e84a7af`, Opus 5.5/xhigh (user-confirmed). B: Fable/medium `a8d0fa1fbd0b8b88e` (continued)
  through R1. For R2, B hit the Fable usage limit. With user authorization ("Fresh Opus reviewer now"), a
  substitute B' (Opus 5.5/medium, fresh context) does R2 as a full batch review.
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, HEAD `fe7791d3`.
  Preserved pre-existing work: p01, p02 and p03 accepted in phase, uncommitted.
- Prerequisites: p03 accepted in phase at `b9271b7d…6dbb` (B PASS AWC-P03-B1-R1-N1).
- Coordination `plan/agent-workflow-call/COORDINATION.md` r13. Applicable:
  - AWC-C001 r4: the authority model. D1 replaces the plan text on the child's actor; D2 sets the result envelope
    (the closure is the result kind); D3 replaces 01 C.5's `reply` step (final output `/fields`; `channel.deny`,
    `channel.hand_off`). p04 refuses a `channel.hand_off` whose nested call did not close `done`.
  - AWC-C002 r1: pending for p04.
  - AWC-C004 r1 (from p03):
    - `channel_call.response_invalid` is reused for invalid projections;
    - p04 removes p03's interim rule declining approval-policy desks when it adds review;
    - p04 inherits the `NO ACTION` retention keys (B.6).
- Resources: scratch DBs `openbot_awc_mac02`, `openbot_awc_mac02_test`.
- Snapshot exclusions: `plan/agent-workflow-call/phases/p04/docs`, `.trail/learnings`.

## Status

- **2026-11-23: B1 implementation progress** (not yet frozen or reviewed). Done:
  - D1 ownership: `channel_reply_runs` owner kinds; `owner_id` across proposals, decisions, guards and readers.
    The reader table had one miss, found by the regression: `notification/projection.rs:117`, now renamed. Ingress
    `link_run` writes `owner_kind='ingress'`.
  - D2 steps: registry (`channel.deny`, `channel.hand_off`; TYPES 25; help; examples; exactness Pure); pure
    evaluation; the workflow output-schema check skipped for the two at `$end`.
  - D2 closures: `channel_reply::ReplyClosure` (`closure.rs`) follows the terminal step. The proposal stores
    `closure`, `handed_off_run_id` and `fields`, all bound into the integrity document. A DB guard
    (`channel_reply_proposal_hand_off`) requires a consumed `done` fact for a hand-off.
  - D2a and D7 refusals happen in `pending_recovery::prepare_pure` before commit (`ResponseStepUnowned`,
    `HandOffInvalid`; both routable).
  - D3/D7 consumption (`channel_call_dispatch::consume_on`):
    - live authority first;
    - then the stored fact (`channel_call_results.rs`), if there is one;
    - otherwise the decision and proposal give the closure, a failure maps through the terminal execution's
      proposal decision (`message.approval_rejected` / `_expired` events) to `review_rejected` / `review_expired`,
      and any other failure is `child_failed`;
    - absorption only for a disclosed answer; then the fact is inserted.
  - D4:
    - p03's approval-policy decline is removed;
    - the reviewer's proposal view carries `response {returns_to: calling_agent, caller_run_id, closure, fields,
      note}` (`decision_approvals::view_proposal`, `CALL_REVIEW_NOTE`).
  - D8: `policy_current_on` is true for call owners. `message.ask` is refused in `message_step_request` with
    `AppError::InteractiveUnsupported` → step failure `channel_call.interactive_unsupported`.
  - Decision (in scope): result `fields` are stored only for call owners, since mail never renders them. Over
    128 KiB compact, the terminal refuses them (child fails → `child_failed`). The `fields` CHECK is 256 KiB of
    jsonb text, to allow spacing; the integrity-document CHECK goes 1.2 → 1.5 MB to admit them. Both CHECKs
    back the Rust guard and are not the limit themselves. `workflow_channel_call_results.content_level` uses the
    `access_level` domain (0–9).
  - Fixes found by the new tests (in scope, all within D1–D8):
    - **Publication of call proposals.** `channel_reply_publish_authority::lock_run_on` required a root run,
      so the publisher refused a call child's proposal. It now joins `channel_reply_runs` and requires
      `(run.parent_run_id IS NULL) = (owner.owner_kind = 'ingress')`: an ingress owner is a root and a call
      owner is a child. This is U5 ("reuse the publisher").
    - **Personal scope across the call boundary.** `memory_tool_checkpoint::user_scope_owner_on` climbed
      parents through a call child. A destination run as the caller's own user then inherited the caller's
      private-session user scope, and the scope seal refused it ("fresh scope seal requires valid live owners
      and base root"). The walk now stops at a `workflow_channel_calls.child_run_id`. A call child is a fresh
      scope (AWC-P02-D1 D1.2); A9 asserts that no personal scope is captured. This tightens p02's contract
      and is recorded for coordination.
    - **Where `message.ask` is refused.** The refusal moved to the start of `message_step_request`, so it
      fires before recipient resolution (which had reported `message.rejected`).
    - **Test fixture.** `CallSetup` publishes destinations with built-in step snapshots
      (`Fixture::publish_with_builtins`), as production publication does, so `message.*` steps run.
  - Tests (`channel_call_response_tests.rs` unless noted). Verification mapping:

    | Criterion | Test | Decisive assertion |
    | --- | --- | --- |
    | A1 owner is the call | `channel_call_response_owner_is_the_call` | `channel_reply_runs` row `(call, intent NULL, call source message, owner_id = that message)`; a forged owner for a non-child run and a row with both owner keys are refused by the guards |
    | A2 automatic done; D7 replay | `channel_call_response_automatic_done` | the result equals `done(child)`; a second consumption returns the stored fact, with no second absorption |
    | A2/D6 desk trail | `channel_call_response_reply_note_published` | the approved reply is filed as an internal note in the call's desk thread |
    | A3 no reply | `channel_call_response_no_reply` | closure `no_reply`, no content |
    | A4 deny; hand-off invalid | `channel_call_response_deny_and_hand_off` | `denied` with code and reason; a hand-off without a `done` fact fails the step (`channel_call.hand_off_invalid`) |
    | A4 hand-off positive | `channel_call_response_hand_off_names_the_nested_run` | the destination's agent calls a second desk (`done`), then `channel.hand_off` gives `{closure: handed_off, child_run_id, run_id: nested, content: nested answer}` |
    | A5 projection refused | `channel_call_response_projection_refused` | invalid contract output → `child_failed`; control passes; over-bound fields → `child_failed` |
    | A6 independent effect | `channel_call_response_independent_effect` | a `message.send` to another desk is one message action and writes no proposal |
    | A7 reply to source | `channel_call_response_reply_to_source` | `message.reply` lands in the call's desk thread |
    | A8 owner required | `channel_call_response_steps_require_owner` | in a workflow-tool child and a manual run, response steps fail with `channel.response_step_unowned` |
    | A9 agent destination | `channel_call_response_agent_destination_prepares` | the destination agent prepares, answers and closes `done`; no personal scope |
    | A10 captured policy | `channel_call_review_approve_releases`; p03 L10 → `channel_call_launch_review_desk_captures_policy` | the policy changes after launch; the view's reviewers are the captured ones |
    | A11 / D8 ask refused | `channel_call_response_ask_refused` | step failure `channel_call.interactive_unsupported`; no wait |
    | B1 approve releases; D4 view | `channel_call_review_approve_releases` | the caller stays `waiting` during review; the view shows `returns_to: calling_agent`; after approval the result is `done(child)` |
    | B2 reject / expire | `channel_call_review_reject_and_expire` | `review_rejected` / `review_expired`, no content |
    | B2a routed rejection | `channel_call_review_rejected_then_routed` | a routed rejection whose later step fails → `child_failed` |
    | B3 races | `channel_call_review_races` | approve vs reject concurrently: exactly one applies, and the caller reads the winner; the winner's replay is `Replayed`; a late submission → `Conflict`; approve vs caller cancel: tree `cancelled`, 0 stored facts; approve vs revocation: release → `revoked`, no content, 0 facts |
    | B4 no raw leak | `channel_call_privacy_no_raw_leak` | a child-only marker in the destination's output is absent from the result, the stored fact, the caller's executions, notifications and messages |
    | B5 retention | `channel_call_privacy_retention` | a finished tree retires only when aged; a tree cancelled mid-review retires whole; the desk thread keeps its 2 messages |
    | D7/F8 consumed once | `channel_call_response_consumed_once` | fact stored, desk disabled, replay → `revoked` |
    | D2 mail deny | `channel_reply_publication_tests::channel_reply_publication_deny_publishes_its_reason` | closure `denied`, fields NULL; body = reason; subject = `denial(source subject)` |
    | D2 closure unit | `channel_reply_closure_*` | `Re:` once, char-boundary safe, bounded, empty body refused |

    Not proved by stand-ins:
    - B3 approval vs revocation: the probe showed that approval of a revoked desk's proposal applies, and the
      release (`consume_on` live authority) is what refuses. The test asserts that refusal, not an
      approval-time refusal.
    - The hand-off test completes the destination's agent step with a fixed output (the nested run id)
      instead of an LLM turn. The nested call itself runs through the production `call_channel` port.
    - Other test changes: R3's invalid case corrupts the stored proposal consistently (the `response_invalid`
      defect guard); shared helpers moved to `channel_call_support.rs`; `publish_agent_workflow` added to
      `workflow_tool_support.rs`.
  - Checks:
    - `channel_call_`: 80/80 (`logs/AWC-P04-B1-channel-call-try4.txt`).
    - `channel_call_review_races` repeated 3× more: all pass.
    - Affected suites (`channel_call_ channel_reply_ memory_scope agent_history run_scope workflow_decision`):
      670 passed, 1 failed, 1 ignored (`logs/AWC-P04-B1-affected-try1.txt`). The failure is
      `doop_agent_current_preparation_reaches_ready_before_model_io` (expects preparation format 7, got 8).
      It also fails in isolation and is recorded as unrelated by p02 and p03 (`PREPARED_AGENT_VERSION` is 8
      since `2edd0a73`). This run predates B3, B4 and the hand-off test (tests only), and the
      `publish_agent_workflow` extraction (test support only).
    - Registry, exactness, authoring, templates and provisioning unit tests: 149/149.
    - Earlier `channel_reply_` regression: 315/315 (try3).
  - Environment: the disk filled (`No space left on device`). With the user's approval, `cargo clean` of this
    worktree removed 242 GiB; nothing else was deleted.
  - Hand-over to p05 (authoring): editor labels for the new steps (`graph.rs:219`, `catalogue.rs:54`),
    `BUILDER_STEP_TYPES`, and the native template's response-step note.
  - Final B1 checks on the frozen digest (`logs/AWC-P04-B1-R1.evidence.json`; the validator reports PASS 5, FAIL 2):
    - Stack budget at 2 MiB, `channel_call_ channel_reply_`: 396/396 (`logs/AWC-P04-B1-stack-budget-try1.txt`).
    - `memory_scope agent_history run_scope workflow_decision message_`: 893 passed, 1 failed, 1 ignored
      (`logs/AWC-P04-B1-affected-try2.txt`). The failure is the known unrelated format 7/8 test above.
    - source_modules: 8/8.
    - fmt `--check`: clean.
    - `cargo sqlx prepare --check -- --all-targets`: clean.
    - Clippy 1.95.0 (CI pin), all targets: 10 findings, all in files this branch did not touch (committed in
      `73afd5a7`). With those two lints allowed, the run exits 0. Local 1.99.1 clippy reports the same ones.
      One finding of mine (B3's `unwrap` after `is_ok`) was fixed before the freeze, by matching the pair of
      results.
- **2026-11-23: batch B2 accepted in phase; p04 accepted in phase** at digest
  `46e25d956441f5ef6901f1294b044abfa81842d199f39a64c36243111dfa995c` (base/HEAD `fe7791d3`, uncommitted). Basis:
  - B' gave an explicit PASS (AWC-P04-B2-R1-N1). Root recomputed the digest and it matches. B1 was accepted at
    `7b3f300c…8a13`; the B2 delta builds on it.
  - Coordination r18 is checked. The known unrelated failure and the pre-existing clippy findings are recorded
    separately.
  - The plan docs' "Implementation evidence" lines stay `pending` until p06's final gate, as for p01–p03.
  - Dispositions of AWC-P04-B2-F1 (optional, low):
    - **Writer.** The note is a platform-authored internal message, filed as a `Note` through `insert_message_on`,
      instead of doc 04 C.2's `add_internal_note` / `internal_notes`. That path records a human or API command
      author with UI/API/integration provenance (`internal_notes_provenance_check`). A platform note has no such
      author, and B1's desk reply already uses this message shape. This is a recorded deviation from C.2's named
      writer; the requirement (one internal note, system author, agent named in the body) holds.
    - **Error-closure note and two concurrent consumers.** Not tested. The note format for an error closure is
      `closure (code)`. Concurrent consumption is serialized by the caller's fence and claim, and the note is
      keyed on the fact insert's primary key in the same transaction.
    - **A note failure blocks the result.** This was chosen at the gate (one transaction) and is accepted.
- **2026-11-23: batch B2 (D6 caller note; doc 04 C) implemented and frozen** at `46e25d956441f5ef6901f1294b044abfa81842d199f39a64c36243111dfa995c` (`logs/AWC-P04-B2-R1-snapshot.txt`;
  manifest `logs/AWC-P04-B2-R1.evidence.json`). The stack run gave 551 passed and 1 failed (the known unrelated test); the
  lints, sqlx and source_modules were clean. Review AWC-P04-B2-R1 requested from B'.
  - **Caller note** (gate r2 D6, doc 04 C.2): new `channel_call_note::file_on`.
    - `consume_on` calls it only when `channel_call_results::insert_on` reports the first insert, in the same
      transaction. Idempotence comes from the fact's primary key; no notes table.
    - The note goes only on the caller run's `thread_id`; a threadless caller records nothing.
    - Shape: a `MessageWrite::internal` with author `Platform`, role `System`, entry kind `Note`, audience
      `internal_only`, stamped `MessageStampSource::Public`, so `access_level` is 0.
    - Subject "Call to {desk}". Body "{calling agent} asked {desk}. The desk's run {child} closed: {closure}[ ({code})]."
      It names the agent, desk, child run and closure (plus the error code for an error closure) and never quotes
      content. The calling agent's label is the call source's author principal.
    - Written with `insert_message_on`, so it creates no intent, no delivery and no ingress; nothing reads it back.
  - **Destination reply** (C.1): unchanged from B1, the existing publisher (U5). `denied` publishes its reason and
    `no_reply` publishes nothing. C.1's `MessageRole::Agent` holds only for agent destinations; otherwise the
    final author is the system (gate r1 D6).
  - Tests:
    - `channel_call_trail_reply_and_note` (C1), in four parts:
      - **Replied call.** One `note`/`system`/`internal_only` platform note on the caller thread, with level 0, no
        intent or delivery, naming the desk, child and `done`, and without the reply text. The desk thread reads
        exactly [question, answer], filed only on the destination desk. The caller thread holds no desk content.
        A replay returns the same fact, and the note count stays 1.
      - **`no_reply`.** The note only; the desk thread holds the request alone.
      - **`denied`.** The note names the closure but not the reason; the desk reply is the reason.
      - **Threadless caller.** No note anywhere.
    - `channel_call_trail_agent_caller_continues` (in `channel_call_resume_tests.rs`): a scripted agent caller on
      a thread parks, then resumes and finishes; one note lands mid-turn without disturbing its history.
    - Support: `CallSetup::claim_in_thread` admits the caller with a `RelatedAssociation::Thread` on its own front
      desk.
  - Stand-in: "a reader of one desk sees nothing of the other" is asserted structurally. Each write is filed only on
    its own desk's thread, neither quotes the other, and the note is Public with no content. Read access is the
    existing per-thread desk grants (C.3, unchanged); no actor-level read test was added.
  - Checks (`logs/AWC-P04-B2-*.txt`): stack budget at 2 MiB `channel_call_ channel_reply_ agent_history`; clippy
    1.95.0 (the two pre-existing lints allowed); fmt; sqlx; source_modules.
- **2026-11-23: batch B1 accepted in phase** at digest `7b3f300c9f6f1de7a926f8b000d3a106beae939a26bd276470d74708b2f88a13`
  (base/HEAD `fe7791d3`, uncommitted). Basis:
  - B' gave an explicit PASS (AWC-P04-B1-R3-N1), building on its full R2 review (AWC-P04-B1-R2-N1). Root recomputed
    the digest and it matches.
  - F1–F6 are all closed. Coordination r17 is unchanged; AWC-C005 r1 is effective.
  - Known unrelated failure: `doop_agent_current_preparation_reaches_ready_before_model_io`.
  - Pre-existing clippy findings (from 73afd5a7) are recorded separately.
  - Next: batch B2 (D6, the caller note on the first fact insert; doc 04 C1).
- **2026-11-23: B1 correction for AWC-P04-B1-R2** (B', the Opus substitute: changes required, note AWC-P04-B1-R2-N1;
  F1–F3 closed, two new low findings).
  - Frozen at `7b3f300c9f6f1de7a926f8b000d3a106beae939a26bd276470d74708b2f88a13` (`logs/AWC-P04-B1-R3-snapshot.txt`).
    Checks:
    - `channel_call_ message_ workflow_decision decision_ reviewed_delivery`: 798 passed, 1 failed (the known
      unrelated test);
    - clippy 1.95.0 (the two pre-existing lints allowed), fmt and sqlx `--check`: clean.
    Manifest: `logs/AWC-P04-B1-R3.evidence.json`.
  - **F4 (low): fixed.** The review page's source link for a call subtree used the caller tree root's ingress
    message (`decisions/read.rs`), on another desk from the thread the page opens.
    - New `channel_call_source::enclosing_call_source_on`: the source message of the nearest call enclosing a run
      (the run itself or an ancestor, depth ≤ 64). The decision view uses it first and falls back to the root's
      ingress outside calls.
    - `message.rs`'s ask refusal (F3) now uses the same helper, so there is one walk instead of a second copy.
    - `channel_call_review_approve_releases` asserts `view.context.thread` = the call's desk thread and
      `trigger_message` = the call's source message. Before the fix this read `None` for a company-admitted
      caller; for an ingress-admitted caller it named the caller's message.
    - Equivalent surface: `run_views::trigger_message` reads the run's own ingress binding, so a call child shows
      no source link rather than a wrong one. Left unchanged; recorded here.
  - **F5 (low): dispositions** for doc 04's leak surfaces and A3. Each basis is checked in code:
    - **Run view.** The caller's committed executions hold only the released result. B4 asserts this over
      `committed_output` and `frozen_inputs`. The child's own run is a desk run, viewed under the existing
      run-inspection authority for the destination.
    - **Sample preview.** A sample never runs a destination: `sample/tools.rs:126` requires a fixture output for
      `ToolKind::ChannelCall` and has no production fallback.
    - **Logging.** The call path logs ids and states only (`channel_call_dispatch.rs:72`,
      `channel_reply_completion.rs:390,485`), never content.
    - **A3, the parent's own response.** `channel_reply_decisions` is keyed `PRIMARY KEY (company_id, run_id)` and
      `UNIQUE (company_id, owner_id)`. A child's `suppressed` decision is its own row and is never the parent's.
  - **F6 (optional):**
    - The static-child ask test now asserts that no wait exists (taken).
    - The stored-result retention cascade is left to its FK (`ON DELETE CASCADE` with the record). Exercising it
      needs a claimed caller continuation, which is what blocked eligibility earlier.
    - An "open review is not eligible" assertion would be vacuous: a live caller has no `finished_at` to age.
    - A1's "both keys" check by error code is unchanged.
    - The crate-wide clippy allows are limited to the two lints; the R2 run without them shows only the 10 files
      that predate this branch.
- **2026-11-23: B1 correction for AWC-P04-B1-R1** (B: changes required, note AWC-P04-B1-R1-N1). Frozen at digest
  `7af1aa2ed955dfc0599bfa4bd4c0501c3ebacefc9d143b2d4ed84f3bb84cc83b` (`logs/AWC-P04-B1-R2-snapshot.txt`). It differs from the first freeze `6aea3eb4…f902`
  only by clippy's `needless_borrow` fix in `hand_off_through`; the hand-off tests were rerun after it (2/2).
  - **F1 (medium): fixed (test defect, not production).** `channel_call_resume_tests::cancel` retried a
    `RevisionConflict` with the same command key. The run control (`controls.rs::control`) stores a conflicting
    attempt under its key with that expected revision, and `replay` refuses the same key with another revision
    as `Conflict("Workflow lifecycle revision or command conflict")`. The idempotency rule is correct, so a cancel
    racing a call's launch or completion is not meant to raise it. Each attempt now takes `{key}-{attempt}`.
    Evidence: `channel_call_resume_cancel_races` 25/25 (`logs/AWC-P04-B1-F1-repeats.txt`), plus the group run
    below. `ToolSetup::cancel` (used by B3 and B5) has no retry: in B3 the approval only parks the child for
    finalization, so the caller's revision does not move during that race.
  - **F2 (low): fixed.**
    - A4: `hand_off_through` drives a real nested call from the destination's agent in three cases. `done` → the
      positive test. A stored `denied` fact, and a nested desk disabled before consumption (`revoked`, no fact) →
      new `channel_call_response_hand_off_needs_a_done_call`: the destination fails with `hand_off_invalid`, has no
      proposal, and the caller reads `child_failed`. Each case asserts the stored fact's closure (`denied` / none).
    - A8: new `channel_call_response_steps_require_owner_in_static_child`. A `workflow.call` child of a call
      child ending in `channel.deny` fails with `channel.response_step_unowned` and commits no output.
  - **F3 (optional): taken.** `message.ask` is now refused anywhere in a call subtree. `message_step_request`
    walks the run's ancestry (depth ≤ 64) to a `workflow_channel_calls.child_run_id`. A static child would
    otherwise ask the tree's root correspondent across the call boundary.
    - New `channel_call_response_ask_refused_in_static_child`.
    - Mutation check: with the walk disabled (depth < 0), the new test fails and A11 still passes; the fix is
      restored.
  - Checks:
    - stack budget at 2 MiB `channel_call_ message_` on `6aea3eb4`: 716 passed, 1 failed (the known unrelated
      format 7/8 test);
    - clippy 1.95.0 all targets, with the two pre-existing lints allowed, on `7af1aa2e`: exit 0;
    - fmt and sqlx `--check`: clean.
    Manifest: `logs/AWC-P04-B1-R2.evidence.json`.
- **2026-11-23: B1 frozen** at digest `555771ee2d824f5a556e08cf1b7e1746528a754fadb9e8daeb1020911b2b0a7c`, base
  `fe7791d3` (`logs/AWC-P04-B1-R1-snapshot.txt`). Coordination r17 (AWC-C005 r1 published). Review round
  AWC-P04-B1-R1 requested from B.
- **2026-11-23: gate AWC-P04-D1 r2 PASSED** (AWC-P04-D1-R2-N1; r2 sha256 `b1c7ff9a…4927`, a delta on r1
  `56d9c53b…b9c5`; pre-gate digest `f759143d…5a03`). Carried into the batch: **F8** (low). D7's replay still runs
  p03's live-authority check before returning a stored fact, tested as fact stored, then revoked, then replay
  answers `revoked`. The facts table's FK cascades with the tree delete. Batch B1 (D1–D5, D7, D8) is in
  implementation.
- 2026-11-23: gate AWC-P04-D1 r1 changes required (AWC-P04-D1-R1-N1, F1–F7). r2 (`logs/AWC-P04-D1-r2-decision.md`)
  answers them. User decision U5 ("reuse the publisher") settles F6: doc 04 C amended; coordination r15.
- 2026-11-23: gate AWC-P04-D1 r1 (`logs/AWC-P04-D1-r1-decision.md`) sent to B; pre-gate digest in
  `logs/AWC-P04-D1-r1-snapshot.txt`. Coordination r14 acknowledges AWC-C002 and AWC-C004 for p04.
- 2026-11-23: phase started. Investigation of the channel-reply response machinery is in progress; gate
  AWC-P04-D1 is to follow (response ownership, review reuse, the result path and the thread trail are persistent
  ownership and cross-subsystem contracts).
- AWC-C002 r1 for p04: p04 owns the `channel.deny` and `channel.hand_off` steps (compile and runtime), result-field
  validation and projection, and the refusal of a `hand_off` whose nested call did not close `done`. Acknowledged
  at startup 2026-11-23.

## Investigation notes (2026-11-23)

The existing response machinery was mapped by a read-only helper; root/A verified the proposal table. In this
list, `schema` means `migrations/20260817000000_init_schema.sql` and `wf/` means
`src/adapters/persistence/workflow/`.

**Ownership is keyed on ingress.**
- `workflow_message_intents` → trigger `capture_channel_reply_context` → `channel_reply_contexts`
  (schema:13006-13160): immutable, and requires an admissible intent.
- `wf/channel_reply.rs::link_run` (:422-447) → `channel_reply_runs` (schema:13036-13215). Its guard requires a root
  run (`parent_run_id IS NULL`), `trigger_id = intent_id`, and a message `source_key`.
- Downstream, every table carries `intent_id` with FKs to contexts and/or runs:
  - `channel_reply_proposals` (schema:13217-13282; guards :13393-13430 and :13509-13555);
  - `channel_reply_decisions` (:13325-13472; `UNIQUE(intent_id)`);
  - `require_channel_reply_decision` (:13474);
  - `channel_reply_publication_matches` (:13596-13645).
- A call child can never own a context: it has no intent, its trigger is `Child`, and it is not a root.

**Terminal handling.**
- `batch.rs:166-177` gives `Suppressed` only for `channel.no_reply` → `$end`.
- `wf/channel_reply_completion.rs::prepare_on` (:147-183) returns `Unowned` without `root_on`, otherwise
  authorizes and stages a proposal (`stage_on` :243-318, `project_on` :191-241).
- `commit_on` (:368-408) writes the decision as `pending` or `suppressed`.
- A final error writes no decision.

**Review.**
- `channel_reply_approvals::{prepare_on, park_on}` (wf/channel_reply_approvals.rs:17-144) set the deadline from the
  policy, check reviewer clearance, and write the decision wait plus `workflow_decisions(action_approval,
  reply_proposal_id)`.
- Submission (`decisions.rs:647-724`, `decision_approvals.rs:178-277`) accepts only approve/reject with
  `data == {}`. **Channel finals have no edited content**: approved content is the immutable `proposal.content`.
- Reject and expiry give `message.approval_rejected` / `message.approval_expired`, routed or failing the root
  (`wait_commit.rs:268-278`, `decision_approvals.rs:201-232`).
- Approve → `resume_finalization` (`channel_reply_resume.rs`).

**Publication.**
- The proposal reserves a `message.reply` action intent with origin `channel_final` (schema:13293-13303).
- `publisher.rs::publish_claim` → `channel_reply_publish.rs::publish_final_reply` (re-validates, then
  `write_destination_on`) sets the decision to `published`.
- The route is `reply_email` or `reply_internal`. **A source with `audience='internal_only'` gives
  `ReplyInternal`**: a note filed in the source thread with no delivery (`message_planner.rs:358-392`,
  `wf/message.rs:510-580`). A call's source message is `MessageWrite::internal`, so a call child's final reply
  would be exactly that: an internal reply in the call's fresh desk thread (Batch C.1's trail).

**Gaps.**
- `channel.deny` and `channel.hand_off` do not exist; only the result types do (`channel_call.rs`).
- **p03 interim gap:** `channel_call_dispatch::consume_on` ignores `response_disposition`, so a child ending in
  `channel.no_reply` closes `done`. p03 D4 assumed it would read `response_invalid`. p04 replaces this arm with
  the `no_reply` closure, so no p03 correction is opened. Recorded for p04's R-row and B's attention.
