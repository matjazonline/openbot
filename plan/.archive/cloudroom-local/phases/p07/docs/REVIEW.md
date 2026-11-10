# p07 independent review

## CL-P07-D01-R1 — sparse native replay architecture gate

**PASS for CL-P07-D01/r1 only**, SHA-256 **`dc0193397f4e0435210249647dd3504199f0cc937f3a3b73c8eff84cbdf40dd0`**. No actionable architecture findings.

Pinned upstream source confirms resolved deltas are pruned without renumbering, so native SDK replay cursors cannot require adjacency. The proposed correction preserves exact positive request/accepted-turn/terminal binding, honest snapshot-only output/truncation, bounds, no blind retry and contiguous Openbot observation acknowledgments.

[Complete review note](agent-review-notes.md#cl-p07-d01-r1--sparse-native-replay-architecture-gate-2026-11-08); [disposition](agent-review-findings.md#cl-p07-d01-r1--architecture-gate-2026-11-08). Frozen source **`94295971c7786ed757d259d0a6a7dbaf57056dcbae9e65ca5543f1d4bad325cf`**, authority r34 and decision hash rechecked unchanged. Root may activate C008/r1 and implement.

Architecture inspection only: no p07 source/provider/full-suite acceptance. Separate Claude stop uncertainty and all original integration checks remain outstanding; Linux native NOT EXERCISED. No runtime checks or provider tasks were run by B for this gate.


## CL-P07-B1-R1 — whole batch and combined integration

**CHANGES REQUIRED** for artifact **`02c3f5520a7bc34e29b260adc6cda9b1d5c1f94a7e2e937f1a8811482a3f776f`**. All actionable findings recorded together:

- **F1 / P2:** add composed admission/pre-socket crash recovery and SDK-count assertions.
- **F2 / P2:** complete actual native human/permission interaction and login-failure acceptance.
- **F3 / P2:** prove output recovery through restarted native observation, beyond retained request/turn IDs.

[Complete review note](agent-review-notes.md#cl-p07-b1-r1--whole-batch-and-combined-integration-review-2026-11-08); [findings and required assertions](agent-review-findings.md). Sparse-cursor production correction matches accepted D01; no new production correctness defect identified. Existing89 plugin tests,2 deterministic e2e cases and3 actual macOS provider runs cover their asserted scopes, not the omitted requirements. Root requested this complete findings round before the still-running final gate finishes. B did not run PG/build/provider checks while root held shared resources.

Final full-suite evidence remains pending. Docker image/knowledge-memory and user-waived headless Linux native acceptance remain separately NOT EXERCISED. No p07/combined acceptance or full-verification claim. Source/authority/decision rechecked unchanged at return; correct every actionable finding before one affected recheck.

- **Return-time gate update:** root reports the ongoing full suite has an actual failure in `workflow_control_revision_overflow_refuses_without_partial_writes` (`control_revision_tests.rs:80–107`). Investigation is pending; relationship to this task is unresolved. Do not classify it as unrelated or claim a passing full suite. This is an additional unresolved final-verification gap, not an independently diagnosed fourth finding.


## CL-P07-B1-R2 — corrected batch and final integration

**PASS** for artifact **`bd92212d24d49b9d937eac2c52723eae45085cdb81abc36d12c262dcdf33cbe9`**, authority36, unchanged D01/r1. **F1, F2 and F3 FIXED; no remaining actionable current-acceptance findings.** This dated outcome supersedes R1 for the corrected artifact; prior findings/results remain historical.

[Complete review and scope](agent-review-notes.md#cl-p07-b1-r2--combined-correction-recheck-and-final-integration-review-2026-11-08); [finding dispositions](agent-review-findings.md#cl-p07-b1-r2--resolution-histories-2026-11-08); [validated immutable evidence](logs/CL-P07-B1-R2.final-v2.evidence.json); [independent final audit](logs/reviewer-b1-r2-final-audit.json).

B independently ran90 plugin tests,5 composed PG/WSS cases and the Linux ARM64 production-image/memory check. B inspected successful final regular3319-test and stock2MiB3314-test suites, actual three-provider macOS native matrix, fresh migrations/offline compilation/Clippy/SQLx, lease/browser/static/package gates and affected final checks. Current manifest validates65PASS/0FAIL/2NOTEXERCISED; source, exact exclusions, authority and D01 rechecked unchanged. Production image runs non-root/read-only/network-none within unchanged2048MiB memory/1536MiB RSS guard.

**Limits:** native Linux provider acceptance remains user-waived/unverified. One historical R1 install log was overwritten and unrecoverable; its manifest entry remains invalid, while fresh uniquely named current installation proof passes. Earlier Codexr18/Clauder19 native failures remain incompletely diagnosed; later passes do not prove them unrelated. No aggregate historical-validation or unexercised-platform pass is claimed. Root may accept the corrected current artifact and exercised combined scope with these explicit dispositions.
