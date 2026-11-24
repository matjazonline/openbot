# AWC-P06 reviewer notes

## AWC-P06-A-R1-N1 — Batch A review (2026-11-23)

**Identity**

- Workflow implement-opus-reviewed-2_4; plan `plan/agent-workflow-call`, phase p06, Batch A (test-only), stage
  batch review. Task [TASK.md](TASK.md), review [REVIEW.md](REVIEW.md).
- Reviewer B: Fable 5.2, model ID `claude-fable-5-2`, effort medium.
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`; phase start, review base and
  HEAD `fe7791d3`; digest `d3bee2a425e9d3508223741ca231608e8c9336715dbece7ca497f5d061c8576f` with exclusions
  `plan/agent-workflow-call/phases/p06/docs`, `.trail/learnings`. Confirmed at start and again before
  returning (unchanged).
- Gated decisions: none in this batch.
- Coordination `plan/agent-workflow-call/COORDINATION.md` revision 25 (read at start; still 25 at return).
  Applicable: plan-wide r1, r2, r15, r21, r22, r25; AWC-C005 r1 (p06 response pending, planned for Batch B's
  final gate; nothing in Batch A changes it). No applicable requirement changed during the review.
- Requirements derived from `06-integration-and-final-verification.md` section A (scenarios 1–9), docs 01–05
  where a scenario points at them, and `phases/p01/docs/logs/AWC-P01-D1-r3-decision.md` (U1–U4).

**Coverage and evidence**

- Read in full: `channel_call_integration_tests.rs` (all 8 tests and helpers), `channel_call_support.rs`, the
  `committed_step` helper and `channel_call_response_reply_to_source`, the `pub(super)` helpers (`call_rows`,
  `age`, `retire`, `publish_all`), `channel_call_resume_restart_at_each_boundary`, and the module mount.
- Read to judge dispositions and scenario coverage (existing accepted tests): `channel_call_resume_outcomes`,
  `_revocation_races_consumption`, `_competing_resumers`, `_cancel_races`, `_archive_cancels_call_child`;
  `channel_call_launch_competing_claimants_converge`, `_lease_expired_at_commit`, `_continuation_limit`,
  `_shorter_deadline_fails_child`, `_two_targets_differ`; `channel_call_response_owner_is_the_call`,
  `_automatic_done`, `_no_reply`, `_agent_destination_prepares`, `hand_off_through`; `channel_call_privacy_retention`;
  `channel_call_manifest_pins_survive_publication`; `fairness_mixed_tests.rs`; `wait_guard_tests.rs`;
  `polling.rs` (`poll_work`); `channel_call_target.rs` (limits, ancestry); `decision_approvals.rs`
  (`record_choice` accepts only approve/reject with `{}`, confirming the author's finding).
- Executed by me: `cargo test --offline --locked --lib -- channel_call_integration_
  channel_call_resume_restart_at_each_boundary channel_call_response_reply_to_source` on
  `openbot_awc_mac02_test`: 10 passed, exit 0 (`logs/AWC-P06-A-R1-reviewer-focused.txt`).
  `check-evidence.py` on `logs/AWC-P06-A-R1.evidence.json` with the digest: valid, 3 PASS.
- Inspected, not rerun: the author's `logs/a-channel-call-tests.txt` (119 passed), `a-stack-budget.txt`
  (119 passed at 2 MiB), `a-clippy.txt`. Test inventory against p05's last `channel_call_` run: 111 names
  before, 119 now, none removed.
- Scenario-to-assertion check (tested unless marked):
  - 1: `…two_desks_in_one_execution`: same execution across both resumes, three model requests, B's edited
    body and C's body in the later requests and the draft absent, two `done` results on distinct bindings.
    Defect: the registration check names the wrong workflow (F2).
  - 2: `…pinned_while_waiting_later_call_current` (binding, settings revision, agent revision, version and
    contract pinned; later call current; static `workflow.call` grandchild settles the caller). Agent
    destination: `…cycles_and_nested_budget` and p04's A9. Skills/resources at a call scope: not exercised (F3).
  - 3: reviewed, rejection, expiry, no-reply, failure, timeout and source reply are in the p03/p04 tests read
    above; edited approval and human decision in the new G1 test. Parent suppression independence: not
    exercised (F1). No fan-out or external reply: `desk_rows` zero assertions (p03/p04).
  - 4: launchers, resumers, cancellation, revocation at consumption, review races (name and header read only),
    lease expiry mid-commit; new consumption and launch-vs-revocation races (see F4.1).
  - 5: boundaries in `…restart_at_each_boundary` plus `…restart_after_review_settlement`; `retry_count` 1 and 0.
  - 6: self-call and A → B → A with zero rows and the refusal seen by B's model; budget root shared.
    Depth bound and poison wakeup: not exercised (F3).
  - 7: foreign/missing/members-only byte-identical with no rows; demoted caller refused before the model;
    positive control. Forged IDs, audience and hidden history rest on p01–p03 tests whose bodies I did not
    reread (names and p02/p03 mappings only).
  - 8: retention with a stored result, waits and manifests; company teardown with a waiting call.
  - 9: deferred to Batch B's full suite; no production code changed in Batch A, so acceptable here.
- Assumptions challenged: that "edited approval" may be a `decision.human` edit (accepted: the final review has
  no edit path, and the model seeing only the approved text is what doc 04 B asks for); that a budget-root
  equality shows a nested call cannot reset the tree budget (accepted: the link is written by trigger from the
  parent's root, and usage is keyed by root).
- Reviewed lightly: the bodies of `channel_call_review_races`, the snapshot/audience/history tests, and
  `channel_call_views_*`; the uncommitted p01–p05 production code beyond the call target, polling and
  approval sites named above. No production defect found in what I read.

**Outcome**

- Changes required. Actionable: [AWC-P06-A-F1](agent-review-findings.md) (medium),
  [AWC-P06-A-F2](agent-review-findings.md) (low), [AWC-P06-A-F3](agent-review-findings.md) (low). Optional:
  [AWC-P06-A-F4](agent-review-findings.md).
- Dispositions accepted as offered: G4 lease loss (`launch_competing_claimants_converge`,
  `launch_lease_expired_at_commit`); G7 audience and hidden history; the "edited approval" reading.
  Accepted in substance but not as recorded: G3 poison wakeup, G6 skills/resources, G7 resource revocation
  (F3). Not accepted: G2 parent response suppression (F1).
- No unrelated failures observed.

**Process and instruction feedback**

- Batch size was reviewable: one new file and three small edits. The cost was in judging dispositions against
  about 110 earlier tests, not the diff.
- No gate was missing.
- Useful: the gap table G1–G9 and the per-test progress notes. Misleading: one manifest entry with status PASS
  for criteria that include dispositions. Candidate instruction (scope: the workflow's evidence manifest
  guidance): a criterion met by a disposition instead of an exercised check gets its own entry with
  NOT EXERCISED and the reason. Evidence: F1, F3. One batch only, so not yet a recurring pattern.

## AWC-P06-A-R2-N1 — Batch A correction recheck (2026-11-23)

**Identity**

- Workflow implement-opus-reviewed-2_4; plan `plan/agent-workflow-call`, phase p06, Batch A, stage correction
  recheck of [AWC-P06-A-R1-N1](#awc-p06-a-r1-n1--batch-a-review-2026-11-23). Task [TASK.md](TASK.md) section
  "AWC-P06-A-R1 corrections", review [REVIEW.md](REVIEW.md).
- Reviewer B: Fable 5.2, model ID `claude-fable-5-2`, effort medium.
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, base/HEAD `fe7791d3`; digest
  `c7fba3400cd27117204379ea11848105f5d96846018aa35d3b27a3bef9d97431` (same exclusions), confirmed at start and
  before returning. Gated decisions: none.
- Coordination `COORDINATION.md` revision 25 at start and at return; applicable notices unchanged
  (AWC-C005 r1, p06 response pending for Batch B).

**Coverage and evidence**

- Rechecked by finding ID: F1 (new test read in full), F2 (query and control read), F3 (manifest, TASK.md
  dispositions), F4. Correction delta read: the new test, the registration query, and `no_reply_destination` and
  `settled` becoming `pub(super)` (visibility only).
- Executed by me: `cargo test --offline --locked --lib -- channel_call_integration_
  channel_call_response_no_reply channel_call_review_` on `openbot_awc_mac02_test`: 14 passed, exit 0
  (`logs/AWC-P06-A-R2-reviewer-focused.txt`). This covers all 9 integration tests and the existing users of the
  two helpers. `check-evidence.py` on `logs/AWC-P06-A-R2.evidence.json` with the digest: valid, 3 PASS,
  4 NOT EXERCISED.
- Inspected, not rerun: `logs/a2-channel-call-tests.txt` (120 passed), `a2-stack-budget.txt` (120 at 2 MiB),
  `a2-clippy.txt`, `a2-fmt-check.txt`.
- Reused from R1 (unchanged code): the other seven integration tests, `reply_to_source`,
  `restart_at_each_boundary`, and the scenario mapping.
- Not exercised, by recorded disposition: G3 poison wakeup, G3 depth bound, G6 skill/resource capture at a
  call scope, G7 resource revocation; and an ingress-owned parent for the suppression case (F1 resolution).

**Outcome**

- PASS for digest `c7fba3400cd27117204379ea11848105f5d96846018aa35d3b27a3bef9d97431`. F1–F3 fixed, F4 closed;
  no new findings. See the resolution updates in [agent-review-findings.md](agent-review-findings.md).

**Process and instruction feedback**

- One correction round sufficed. The split manifest (PASS separate from NOT EXERCISED) made the recheck quick;
  this supports the candidate instruction recorded in R1. No further change supported.

## AWC-P06-B-R1-N1 — Batch B final integration review (2026-11-24)

**Identity**

- Workflow implement-opus-reviewed-2_4; plan `plan/agent-workflow-call`, phase p06, Batch B (final gate), stage
  final integration review. Task [TASK.md](TASK.md) "Batch B — final gate", review [REVIEW.md](REVIEW.md).
- Reviewer B: Fable 5.2, model ID `claude-fable-5-2`, effort medium.
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, base/HEAD `fe7791d3`; digest
  `bc8b9841d9f9346152acdbd78e3a093597f554357b61c900e5639ed81486764f` (exclusions
  `plan/agent-workflow-call/phases/p06/docs`, `.trail/learnings`), confirmed at start and before returning.
  Gated decisions: none.
- Coordination `COORDINATION.md` revision 25 at start and return. Applicable: plan-wide r1, r2, r15, r21, r22,
  r25; AWC-C005 r1 (p06 response pending).
- Requirements: doc 06 "Batch B" steps 1–6 and "Completion record"; AGENTS.md "Keep verification focused";
  `desk_wording_tests.rs`; `.github/workflows/ci.yml` and `agent-history.yml`.

**Coverage and evidence**

- **Wording fix (question 1): correct and complete.** Read `calls.rs:96` and `channel_response.rs:132-138`:
  only text nodes changed; `id`, `for`, `aria-labelledby` and `data-*` are as before. `channel_id` in the help
  line is not a word-bounded "channel". Repository-wide search for the old and new strings: the two browser
  tests and `docs/workflow-operations.md:331` follow the new names; the doc's section title and anchors
  (`#channel-calls`, linked from three docs) are unchanged and consistent. Executed by me:
  `desk_wording` (7 tests incl. `…page_sources_keep_channel_out_of_text_nodes`), `channel_call_authoring_desk_card`,
  `channel_call_views_section_draws_only_authorized_references` and the `channel_response` tests pass
  (`logs/AWC-P06-B-R1-reviewer-focused.txt`: 15 passed, 1 failed = the version-7 test). Browser checks
  inspected, not rerun (`logs/b2-browser-wording.txt`, 3/3 after regenerating the fixture).
  Not page text and outside the rule: two error strings beginning "Channel calls …"
  (`workflow_agent/mod.rs:257`, `agent_contract.rs:86`).
- **Failure classifications (question 2):**
  - *desk_wording, regression fixed:* holds (log `:6051-6056` names exactly the two fixed lines).
  - *version-7, pre-existing:* holds. `git grep` at `fe7791d3`: `PREPARED_AGENT_VERSION: u8 = 8`; the test
    (`agent_history_freeze_guard_tests.rs:435`, unmodified, last changed in `cb46c6fd`) asserts 7. It failed
    at base; this plan's 9 changes only the left side.
  - *two Doop tests, load timing:* plausible and consistent with the logs (a document timeout and "scenario
    lifetime exceeded" in the 57-minute run only; passed in the second full run and the isolated rerun).
    Inspection of logs only; I did not rerun them.
  - *attribution SSO, unrelated by code path:* holds by inspection. The failing stage asserts attributes of
    the attribution service's own `__Host-attribution` cookie (`sso.mjs:30-36`); `services/` and
    `src/adapters/identity` are unmodified, and neither the tracked diff nor the untracked files of this plan
    touch cookie, session or SSO code. No base-commit run establishes it; it is an inspection claim.
  - *agent_history crash test, unresolved gap:* I agree it must stay a gap, not "unrelated". Executed by me:
    5 more passes alone at 2 MiB, and the `agent-history.yml` job command under network isolation
    (6 passed, `logs/AWC-P06-B-R1-reviewer-agent-history.txt`). Not reproduced in 7 isolated runs at 2 MiB;
    the one failure was in the 69-minute loaded run. See F6.3 for the captured panic.
  - *clippy, pre-existing:* holds (nine unmodified files); status mis-recorded (F6.2).
- **CI gates (question 3):** compared `ci.yml` steps with `check.sh`'s summary. Not run and not recorded:
  the Linux-only network-isolation scope self-test (F6.1). Compiled but never executed, and declared PASS:
  `tests/source_modules.rs` and the binary's unit tests (F5); I ran them: 8 and 6 passed
  (`logs/AWC-P06-B-R1-reviewer-source-modules.txt`). Recorded as unavailable and consistent with the log:
  plugin package (Node 24.21.0), query statistics, the three images, managed Doop, attribution extracted
  (`jsonschema`). `agent-history.yml`'s test step is covered by the full suite and my run; its browser step by
  check.sh's browser protocols (19/19). `memory-service.yml` and `attribution-service.yml` concern
  `services/`, which this plan does not change; I did not review them further.
- **AWC-C005 for p06 (question 4): satisfied, with the gap disclosed.** Counted in `logs/b-check-sh.txt`,
  main run: `memory_scope` 80/80, `run_scope` 4/4, `agent_history` 158 passed and 1 failed (version-7,
  pre-existing), `channel_call_` 120/120. Stack-budget run: the same, plus the crash test failing once. The
  notice's verification clause (the final gate covers those suites) is met by the main run; the crash-test
  gap must be stated with the response.
- **Full re-run (question 5): not warranted.** The delta since the full runs is two text nodes, two browser
  tests and one doc line; the focused Rust and browser checks cover every consumer of those strings.
- Manifest `logs/AWC-P06-B-R1.evidence.json`: validates (5 PASS, 3 FAIL, 4 NOT EXERCISED).
- Reviewed lightly: the 12k-line log was searched, not read; the Doop timing failures and the image, Doop,
  plugin and query-statistics gates were judged from their log excerpts only.

**Outcome**

- Changes required, record only: [AWC-P06-B-F5](agent-review-findings.md) (medium),
  [AWC-P06-B-F6](agent-review-findings.md) (low). No source change is asked for, so the digest can stay.
- No new production or test defect found. The wording fix and every failure classification hold.
- For root and the user, not a finding: doc 06 says not to declare the implementation complete while a
  required check is unavailable. Unavailable here: plugin package, query statistics, three images, managed
  Doop, attribution extracted, the Linux isolation self-test. Failing and unrelated: version-7 test,
  attribution SSO, stable clippy. Unresolved gap: the agent-history crash test under load. The completion
  record must state these as such.

**Process and instruction feedback**

- Batch size fine. No gate missing.
- Useful: the per-gate table with classifications. Misleading: mapping gates onto an all-targets run that
  stopped at its first failing target (F5). Candidate instruction recorded in F5.

## AWC-P06-B-R2-N1 — Batch B correction recheck (2026-11-24)

**Identity**

- Workflow implement-opus-reviewed-2_4; plan `plan/agent-workflow-call`, phase p06, Batch B, stage correction
  recheck of AWC-P06-B-R1-N1. Task [TASK.md](TASK.md) "AWC-P06-B-R1 corrections" and B2.4; review
  [REVIEW.md](REVIEW.md).
- Reviewer B: Fable 5.2, model ID `claude-fable-5-2`, effort medium.
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, base/HEAD `fe7791d3`; digest
  `bc8b9841d9f9346152acdbd78e3a093597f554357b61c900e5639ed81486764f`, recomputed with the same exclusions at
  start and before returning: unchanged from R1, so no source, test or deliverable-doc change. Gated
  decisions: none. Coordination `COORDINATION.md` revision 25 at start and return; AWC-C005 r1 p06 response
  still pending (root's to record).

**Coverage and evidence**

- Rechecked by finding ID: F5 and F6 (three parts), against TASK.md and the R2 manifest. See the resolution
  updates in [agent-review-findings.md](agent-review-findings.md).
- Executed: `check-evidence.py` on `logs/AWC-P06-B-R2.evidence.json` with the digest: valid, 6 PASS, 4 FAIL,
  5 NOT EXERCISED. No test rerun was needed: nothing under the digest changed, and R1's reviewer runs stand.
- Inspected: `logs/b3-source-modules-bins.txt` (8 and 6 passed, exit 0).
- R1 manifest: the author reports rewriting it by accident and regenerating it. I did not record its hash in
  R1, so I cannot prove byte identity. What I can state: it has the same size (7462 bytes), the same 12
  entries with the statuses, criteria, commands and file lists I printed during R1, and it still validates
  (5 PASS, 3 FAIL, 4 NOT EXERCISED). It still carries the claim F5 was raised against, as a reviewed
  historical record should.
- Reused from R1: the wording-fix review, all failure classifications, the CI comparison and the AWC-C005
  counts.

**Outcome**

- PASS for digest `bc8b9841d9f9346152acdbd78e3a093597f554357b61c900e5639ed81486764f`. F5 and F6 fixed; no new
  findings.
- The pass is for the review of the final gate's record and integration effects. It does not turn the gate
  green: 4 FAIL and 5 NOT EXERCISED entries remain and must appear as such in doc 06's completion record
  (unavailable: plugin package, query statistics, images and managed Doop, attribution extracted, Linux
  isolation self-test; failing and unrelated: version-7 test, attribution SSO, stable clippy; unresolved gap:
  the agent-history crash test under load).

**Process and instruction feedback**

- Candidate (scope: this workflow's evidence guidance): the reviewer records the sha256 of each manifest it
  reviews in its note, so a later rewrite can be checked. Evidence: this round's R1 manifest regeneration.
  One occurrence.
