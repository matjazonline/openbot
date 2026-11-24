# AWC-P05 review notes

## AWC-P05-D1-R1-N1

**Round.**
- 2026-11-23; architecture gate AWC-P05-D1-R1 for p05 batch B1 (doc 05 Batch A).
- Reviewer: B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer named for p05
  by U7.
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, HEAD `fe7791d3` (p01–p04 accepted,
  uncommitted).
- Pre-gate digest `b2c2b594bf55c57a4fe653f2525fbab8c1c7038e36b288b7ee8b836228a25976` (exclusions
  `plan/agent-workflow-call/phases/p05/docs`, `.trail/learnings`), confirmed before and after.
- Decision [AWC-P05-D1 r1](logs/AWC-P05-D1-r1-decision.md), sha256 `24c9f9ef…70bd`, verified.
- Coordination r21: plan-wide U1–U7; AWC-C001 r4, AWC-C002 r1 and AWC-C005 r1. Evidence:
  inspection only.

**Coverage.**
- Requirements: doc 05 Batch A (A.1–A.5 and focused checks); p01 r3 D3 "Discovery"
  (lines 234–262); U6/U7; p05 TASK's Identity, Investigation and User decisions sections. I did
  not read p01 addendum A1 again beyond AWC-C002's summary.
- Code checked against D1–D5:
  - `agent.rs:116` (`from_builtin`) and `:506-509` (pairing);
  - `agent_context.rs:75-95` (`declaration`);
  - `channel_call_target.rs:386-462` (`target_on`, `agent_participates_on`);
  - `agent_preparation.rs:150-225` and `agent_history/checkpoint.rs` (version check and
    `frozen_preparation`);
  - `agent_history_views` readers of the frozen spec.
- Assumptions challenged:
  - freezing against U6's per-turn wording (F1);
  - who reads model output and the frozen spec (F2);
  - the fence-escape injection (F3);
  - bounds arithmetic (F4);
  - equality of the listing with launch (F5);
  - Batch A completeness (F6).
- Freezing per step is sound against exactness, replay and history freezing. The spec is
  version-checked and frozen per execution, and resumes read the frozen preparation. Only its
  divergence from U6's wording is open.
- D1, D4 and D5 raise no issues. The no-auto-grant rule, the reserved-alias tests, the template
  accepting `RESPONSE_STEPS` and the operations docs match A.1, A.5 and the p04 hand-over. The
  version bump 8→9 under the no-backward-compatibility rule is fine.

**Outcome: changes required.**
- Actionable: [AWC-P05-D1-F1](agent-review-findings.md) and F2 (medium); F3, F4, F5 and F6 (low).
- Optional: F7.

**Not verified.** No code exists yet. I did not read the browser test harness, nor
`channel_settings.rs` (only the decision's references to it). A.1's "protected-tool approval
selection" is accepted as declared (`approval_required: false`); I did not trace the approval
selector.

**Process feedback.**
- The gate was well scoped and its facts were accurate where I checked them.
- Two of the misses concern how the user's wording and the model's audiences carry through:
  decisions that put data in front of a model should state who can read what the model writes.

## AWC-P05-D1-R2-N1

**Round.**
- 2026-11-23; gate recheck AWC-P05-D1-R2 of F1–F7.
- Reviewer: B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, HEAD `fe7791d3`.
- Pre-gate digest `d9e156a590ee14502f72a4e98173b33d56c00091e8bf464508950899f28b6a2e` (exclusions
  `plan/agent-workflow-call/phases/p05/docs`, `.trail/learnings`), confirmed before and after.
- Decision [AWC-P05-D1 r2](logs/AWC-P05-D1-r2-decision.md), sha256 `541203da…28e2`, verified; a delta
  on r1 `24c9f9ef…70bd`.
- Coordination r22, which adds U8. Evidence: inspection only.

**Coverage.** I read the r2 delta in full against F1–F7 and U8. Checked in code:
- `admission.rs:180-200`, `message_intents.rs:342` and `admission_refusal_claims.rs:17`: the only
  writers of intent bindings;
- `admission_source.rs`: reruns are manual starts, which leads to F9;
- `target_on`, `decide_target` and `agent_participates_on`: their locks, for F10;
- `enclosing_call_source_on`, from p04, for the call-subtree rule.

**Outcome: PASS** for decision revision r2 (sha256 `541203da…28e2`).
- F1–F7 are closed.
- Two new low actionable findings are deferred to batch B1, which B' must check there:
  - [AWC-P05-D1-F8](agent-review-findings.md): the escape strings were lost from the text;
  - [AWC-P05-D1-F9](agent-review-findings.md): reruns and manual runs on a customer thread count as
    internal.
- Optional: F10.
- None of these changes an unwind-costly decision. The spec shape, version, filter and audience
  rule stand, and F9 refines only how "external" is detected.

**Not verified.** No code exists yet. I did not check how a thread association records an external
correspondent; that belongs to F9's batch fix.

**Process feedback.** Asking the user for U8 resolved F2 cleanly. Text containing escape sequences
should be checked after it is written (F8).

## AWC-P05-B1-R1-N1

**Round.**
- 2026-11-23; batch review AWC-P05-B1-R1 (doc 05 Batch A, plus the p04 hand-overs).
- Reviewer: B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, HEAD `fe7791d3` (p01–p05 uncommitted).
- Digest `fb860437a159df93045d7975ed0b3c2a6cc104a9958c21edb4cf9559f5e0fdfd` (exclusions
  `plan/agent-workflow-call/phases/p05/docs`, `.trail/learnings`), confirmed before and after.
- Gate AWC-P05-D1 r1 with the r2 delta, plus F8–F10 as recorded in TASK Status.
- Coordination r22 (U6–U8; AWC-C002 r1, AWC-C005 r1).
- `logs/AWC-P05-B1-R1.evidence.json` validates (PASS 7, FAIL 2; both FAILs are the known
  format-7 test).

**Coverage.**
- `channel_call_listing.rs` in full: bounds, fill, escapes and the C5 unit test.
- `desk_listing_on` and `answers_external_on`: the per-candidate `target_on` plus `decide_target`
  equal launch's target admission; the unjudged count; the call-subtree rule.
- `agent_history/preparation.rs::candidate_on` and its two paths, `frozen.rs::restore_on`, and
  `PreparedAgent::with_desk_listing` / `calls_desks` (`agent.rs:325-350`).
- Version pins: the schema diff (two pins, 8→9) and a search of `src` for the constant and for
  literals. This found AWC-P05-B1-F1.
- The desk card: the `channel_response.rs` diff and its unit tests (escaping; no params; a
  `SECRET_PARAM` fixture).
- `layout.rs::copyTextFrom` and every user of `copy-text`: onboarding, Resend, mailbox dispatch.
- Docs diff: `workflow-operations.md` "Channel calls" and the three corrected files.
- C3, C3a and C4 tests, read where they mattered.
- Deviations:
  - 1, accepted: no built-in has a label model, and A.1 does not require one.
  - 2, accepted.
  - 3, accepted.
  - 4: the note is inaccurate but harmless (F4).
  - 5, accepted: an improvement, and the old code threw without the clipboard API.

**Checks B ran** (serialized, scratch fixture, this digest):
- `cargo test --offline --lib -- channel_call_authoring_ agent_history_preparation
  channel_call_response_hand_off channel_call_resume_scripted`: 22/22
  ([log](logs/AWC-P05-B1-R1-reviewer-focused.txt), sha256 `2485327a…7035`).
- `doop_agent_`: 17 passed, 2 failed (the known one and F1)
  ([log](logs/AWC-P05-B1-R1-reviewer-version-pins.txt), sha256 `45d94630…6455`).
- Inspected only: the author's affected suites, stack budget, browser 2/2, clippy, fmt, CSS, sqlx and
  source_modules.

**Outcome: changes required.**
- Actionable: [AWC-P05-B1-F1](agent-review-findings.md) (medium), F2 and F3 (low).
- Optional: F4.
- Gate F8 and F10 are closed. Gate F9's code is in place, and its test gap is F2.

**Covered lightly or not verified.**
- The browser test and the `AuthoringReads::desk_call_facts` SQL were checked by inspection of the
  diff only.
- The deadline-expiry claim in the docs was not traced (F4).
- Catalogue and choices tests: I read only the C1 mapping.
- The full suite was not run.

**Process feedback.**
- A version bump needs a literal search over the tests (F1).
- A record line that promises a test case should match the test's actual setup (F2).

## AWC-P05-B1-R2-N1

**Round.**
- 2026-11-23; correction recheck AWC-P05-B1-R2 of F1–F4.
- Reviewer: B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, HEAD `fe7791d3`.
- Digest `22fdcacd89aa94c2b7d60e432a336910df017210e970d3eb0ba0abd471ecba13` (exclusions unchanged),
  confirmed before and after.
- Coordination r22.
- `logs/AWC-P05-B1-R2.evidence.json` validates (PASS 7, FAIL 3; all three FAILs are the known
  format-7 test).

**Coverage.** The delta is tests, docs and records only; no production code changed.
- Read: the two version assertions; the new `channel_call_authoring_discovery_external_sources`;
  the C3a comment; the doc line; TASK's F4 rationale.
- Searched `src` again for literal prepared-version pins: none left.
- B's run: `cargo test --offline --lib -- channel_call_authoring_ doop_agent_mcp
  agent_history_preparation`, 23/23 ([log](logs/AWC-P05-B1-R2-reviewer-focused.txt), sha256
  `92c8873e…0825`).
- Inspected only: the author's fmt, clippy, focused 46 + 1 known, and stack budget 17/17.
- R1's review of the unchanged production code is reused.

**Outcome: PASS** for digest `22fdcacd…ba13`. F1, F2 and F3 are fixed, and F4 is closed. No open
findings for p05 batch B1.

**Not covered.** As R1-N1, apart from the items closed here. I did not repeat the mutation check;
I accepted the author's record of it.

## AWC-P05-D2-R1-N1

**Round.**
- 2026-11-23; architecture gate AWC-P05-D2-R1 for p05 batch B2 (doc 05 Batch B).
- Reviewer: B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, HEAD `fe7791d3`.
- Pre-gate digest `22fdcacd89aa94c2b7d60e432a336910df017210e970d3eb0ba0abd471ecba13`, confirmed
  before and after.
- Decision [AWC-P05-D2 r1](logs/AWC-P05-D2-r1-decision.md), sha256 `de0e8612…78ca`, verified.
- Coordination r22, including AWC-C005 r1. Evidence: inspection only.

**Coverage.**
- Doc 05 Batch B (B.1–B.5, focused checks, acceptance) against D1–D5 and V1–V10.
- Checked in code:
  - `workflow_run_events`' wait-shape check and its unique indexes (schema 5949-5968, 8598-8600),
    for D5's argument;
  - the `wait_parked` / `wait_completed` audits on call waits (`workflow_tool_calls.rs:243`,
    `child_waits.rs:293-300`);
  - `consume_on`'s completed-wait precondition, for the phase gaps (F1);
  - `scripts/workflow-sample-boundary.py`'s symbol allowlist, and the imports and port in
    `channel_call.rs` (F4).
- Questions put to the decision:
  - **Capture authorization:** it reveals more than a desk viewer sees (F5).
  - **Phase totality and races:** F1, and F3 on the caller side.
  - **Stalled versus review:** F2. Waiting on `decision` also covers business decisions. The
    stalled predicate itself is sound for the child.
  - **New run-event kinds:** declining them is acceptable (F7).
- D2 (explicit scope wording) and D4 (labels and logs) raise no issues.
- Not read: `run_views.rs` CHILDREN_SQL and `project()`, `run_chip`, `metrics.sql` and
  `sample_capture.rs` in detail. I relied on the decision's descriptions of them.

**Outcome: changes required.**
- Actionable: [AWC-P05-D2-F1](agent-review-findings.md) and F2 (medium); F3, F4, F5 and F6 (low).
- Optional: F7.

**Process feedback.**
- State machines that a view derives from several rows should come with a totality table over
  every row combination, including the ones a race leaves behind (F1).
- "Waiting reason" is coarser than the decision kind (F2).

## AWC-P05-D2-R2-N1

**Round.**
- 2026-11-23; gate recheck AWC-P05-D2-R2 of F1–F7.
- Reviewer: B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, HEAD `fe7791d3`.
- Pre-gate digest `22fdcacd…ba13`, confirmed before and after.
- Decision [AWC-P05-D2 r2](logs/AWC-P05-D2-r2-decision.md), sha256 `4fd2383d…94ae`, verified; a
  delta on r1 `de0e8612…78ca`.
- Coordination r22. Evidence: inspection only.

**Coverage.**
- The r2 delta in full, by finding.
- Checked against code:
  - the resume-time wait completion (`child_waits.rs:293-300`), and `consume_on`'s precondition;
  - live result-field behaviour (`channel_reply_completion::project_on`, `families.rs::result_fields`),
    for F9;
  - the agent checkpoint's invocation states (schema ~12408-12415), for F8;
  - the named `channel_call` symbols (`declared_fields`, `check_intake` and so on exist in
    `channel_call.rs`).

**Outcome: PASS** for decision revision r2 (sha256 `4fd2383d…94ae`).
- F1–F7 are closed.
- Two new low findings, [AWC-P05-D2-F8](agent-review-findings.md) and F9, are deferred to batch B2,
  and B' checks them there. Neither changes an unwind-costly decision: the request contract,
  identity kind, allowlist approach, read models and metric names stand. They refine one rule and
  one validation.

**Not verified.** `metrics.sql`, `run_views.rs` and `sample_capture.rs` were not read in detail.
There is still no code.

## AWC-P05-B2-R1-N1

**Round.**
- 2026-11-23; batch review AWC-P05-B2-R1 (doc 05 Batch B; gate AWC-P05-D2 r2 plus F8 and F9).
- Reviewer: B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, HEAD `fe7791d3`.
- Digest `24b290b23293e66b2e534d4dfbd2546ffd6f9fa180917d8859313890f48e9da4`, confirmed before and
  after.
- Coordination r22.
- `logs/AWC-P05-B2-R1.evidence.json` validates (PASS 9, FAIL 1; the FAIL is run1's gauge-count
  expectation, which run2 corrects).

**Coverage.**
- `channel_call_views.rs`: `call_phase` rules 1–4 with F8, labels, and `outcome_label`.
- `run_view_calls.rs`: `CALLS_SQL` (latest wait, result code, final review through
  `reply_proposal_id`, both stall predicates, the checkpoint-consumed predicate), `channel_call_page`
  with the `is_visible` fix, and `channel_call_origin`.
- The `run_views.rs` wiring; `Ui::call_access` (runs, desks, and responders via
  `channel_reply_settings`); `pages/workflow_runs/calls.rs` (link gating, which gives F1).
- The `metrics.sql` CTEs and the gauge filters.
- `sample_capture::call_targets_on` (`authorize_association_on(inbox)`, `desk_state_on`, the
  writable agents, the frozen responder, intake and result fields).
- The `channel_call_target` split, its locks and the absence of locks in the settings and binding
  reads.
- `run_page_for_tests` gating; the CI and `check.sh` wiring; the docs' "Inspecting calls",
  "Samples" and three gauge rows.
- Deviations 1–8 judged in F2 and the F8/F9 dispositions.

**Checks B ran** (scratch `openbot_awc_mac02_test`, this digest):
- `cargo test --offline --lib -- channel_call_views_ channel_call_sample_ metrics_tests`: 13/13.
- `python3 scripts/workflow-sample-boundary.py`: pass.
- [log](logs/AWC-P05-B2-R1-reviewer-focused.txt), sha256 `9832062b…ac5b`.
- Inspected only: the author's affected suites (400 passed, 1 ignored), stack budget 215, browser
  18/18, clippy, fmt, CSS, sqlx, source_modules and the manual negative boundary run.

**Outcome: changes required (one low).**
- Actionable: [AWC-P05-B2-F1](agent-review-findings.md) (low; can be closed by a record).
- Optional: F2.
- Gate F8 is fixed. Gate F9 is resolved by deviation 1.

**Covered lightly or not verified.**
- I did not run the browser tests, nor `metrics_tests` under its own database: my filter may not
  have selected it, and I accepted the author's run2.
- I read the sample execution in `sample/tools.rs` only through its V6/V7 mapping.
- I did not verify independently that the harness `invocation_id` equals the call id; the V3
  mutation check supports it.
- The full suite was not run.

## AWC-P05-B2-R2-N1

**Round.**
- 2026-11-23; recheck AWC-P05-B2-R2 of B2-F1.
- Reviewer: B', Opus 5.5, the Opus substitute reviewer (U7).
- HEAD `fe7791d3`.
- Digest `905b73f8f75ac938e26cb397b96e61108afd5478c951deb7b393c8d8f23c8d5a`, confirmed before and
  after.
- `logs/AWC-P05-B2-R2.evidence.json` validates (PASS 5).

**F1.**
- `CallAccess.decisions` (`pages/workflow_runs/calls.rs:19`) is the only gate on the review link
  (`calls.rs:36`).
- `Ui::call_access` (`runs/mod.rs:370-394`) fills that set with `decisions().read_decision(company,
  actor, id)`, the same call the decision page route makes (`routes/workflows/decisions.rs:66`):
  - Ok: the link is drawn.
  - NotFound or Forbidden: label only.
  - Any other error propagates.
- `calls_tests.rs:63-70` adds the "child readable, decision not openable" case.
- The doc (`workflow-operations.md:337`) now says "when the viewer may open it".
- I accept that no DB fixture separates run access from decision access. F5 had the same limit.
- Fixed. Closed.

**F2 (optional).** Declined, with the rationale in TASK. Accepted.

**Checks B ran:**
- `cargo test --offline --lib -- channel_call_views` on scratch `openbot_awc_mac02_test`: 10/10
  ([log](logs/AWC-P05-B2-R2-reviewer-focused.txt), sha256 `26f522a9…bbf2`).
- Inspected only: the author's clippy, fmt, 38-test focused run, stack budget and browser run.
- The full suite was not run.

**Outcome: PASS** at digest `905b73f8…8d5a`.
