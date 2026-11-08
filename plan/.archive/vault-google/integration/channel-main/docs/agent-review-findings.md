# Independent integration findings

Both entries belong to VG-INT-MAIN-R1,2026-11-07, implement-astra-reviewed-2-2, independent B /root/reviewer requested Astra/high (actual runtime settings not independently exposed), worktree `/Users/mac02/.codex/worktrees/1133/openbot`, review HEAD4ab45555f8316ec7a8b1b64e8c05b77ff7d9a29d and digestf2bd7d430aa1f4eff0289a6006fa64deeb9fa189def2e9ae74fa427f232b7926. [Round notes](agent-review-notes.md#vg-int-main-r1), [TASK](TASK.md), [REVIEW](REVIEW.md). No newly introduced actionable integration defect found; confirmed unrelated failures are preserved under the scoped pull/check instructions.

## VG-INT-MAIN-F1 — inherited markup scanner failure

**Classification:** low, verification, inherited upstream fixture/source-scanner mismatch; unresolved outside this integration's authorized correction scope.

`src/adapters/http/pages/workflow_authoring/sample.rs:152` contains deliberate hostile script text inside cfg(test). Existing `pages/tests.rs:4613–4669` scanner exempts tests.rs filenames, so it reports this test input as inline production markup. Actual renderer uses escaping; its targeted escaping test passes. Both offending source and scanner are byte-identical to canonical pulled main. The exact existing guard/helper compiled std-only against a canonical-main HTTP-source export reproduces the same failure. B inspected bodies/hashes and actual diagnostic; no vault/schema merge change causes it. [Failure](logs/markup-01.log), [baseline receipt](logs/baseline-markup.json), [baseline output](logs/baseline-markup-run.log).

**Disposition:** CONFIRMED INHERITED / UNRESOLVED. Leave source/checker unchanged and report the check as failed. This integration PASS does not waive or silently repair it. Candidate instruction lesson: deliberate hostile HTTP fixtures need consistent test-only classification in existing source checks; validate exact export artifacts. No new instruction or implementation authored by B.

## VG-INT-MAIN-F2 — inherited transport scanner failure

**Classification:** low, verification, inherited upstream test-helper/source-scanner mismatch; unresolved outside this integration's authorized correction scope.

`src/adapters/persistence/workflow/channel_reply_continuation_views_support.rs:59` builds a synthetic inbound RFC Message-ID for its test fixture; imported from continuation_views_tests. Existing transport-boundary script reports provider input construction outside the adapter without recognizing that helper as test-only. Offending helper and script are byte-identical to canonical main. Running the unchanged script against complete canonical-main src/migrations export reproduces identical diagnostic output. [Failure](logs/transport-01.log), [baseline receipt](logs/baseline-transport.json). This is not evidence of new production provider coupling from the vault merge.

**Disposition:** CONFIRMED INHERITED / UNRESOLVED. Leave source/checker unchanged; boundary check remains failed. No current-task correction is required for the scoped integration PASS, and no all-checks-green claim is allowed. Instruction lesson mirrors F1's exact-artifact/test-fixture classification; earlier upstream acceptance is historical evidence, not a substitute for this check.
