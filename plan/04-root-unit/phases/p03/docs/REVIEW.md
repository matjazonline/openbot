# Plan04 p03 independent review

## 04RU-P03-B1-R1 — CHANGES REQUIRED (whole B1 and final integration)

2026-11-17; retained independent B `/root/p04_reviewer`, requested Astra/high; actual runtime settings are not exposed. Workflow `implement-astra-reviewed-2-3`. [04RU-P03-B1-R1/N1](agent-review-notes.md) reviews artifact **9b394b31e98b0ede9d281ea7e977a5966ce683692500dc0ba2642369b43e2f0e**, phase start/review base/HEAD **4dad1e971e1791f75731da50a7e73e6776177057**, branch `feature/plan03_04`. Exact exclusions: `plan/04-root-unit/phases/p03/docs` and `plan/04-root-unit/COORDINATION.md`.

**One actionable finding: [04RU-P03-B1-F1](agent-review-findings.md), P2/medium, acceptance-blocking verification gap.** The current full stock run fails the ordinary Doop mock error-observation test at `notify`. The isolated pass, historical occurrence and healthy lease metrics do not establish its cause or separation from Plan04's changed membership/visibility/notification paths. Existing private Doop deferral does not waive this new failure. Diagnose the affected path and establish meaningful integrated proof or evidence-backed unrelatedness; preserve the original failure, deadlines and assertions.

All p03 source/documentation changes and final-gate mapping reviewed. All 31 original core failures have exact passing focused and current stock results. Manifest retention validates: 24 PASS / 4 FAIL / 1 NOT EXERCISED; this is not acceptance. No additional source defect found. Prior accepted p01, main integration and p02 coverage is reused for unchanged source. External/browser/image/Linux replacement evidence is inspected, not independently rerun. No reviewer DB or runtime resources were created.

Coordination13 pins C001/r1, C002/r2, C003/r2, C004/r1 and inherited 01DEP-C002/r1; C002/r2 requires finish04/checks/pushmain then STOP, no05/06. Private Doop remains explicitly user-deferred under 03PA-C008/r1; no whole-CI-green claim. Trail's migration/localStorage match is a scope false positive, with its original failure retained. [Independent result audit](logs/B1-R1-reviewer-evidence-audit.json), [validator](logs/B1-R1-reviewer-evidence-validation.txt), [final pins](logs/B1-R1-reviewer-final-checks.json).

Final freshness check found revision14/C005/r1, an explicit new user instruction to check Doop separately including this named mock failure. Source identity is unchanged. R1 above is preserved under its revision13 scope; a fresh scope-addendum disposition is required before acceptance. B informed root and stopped Doop investigation.

## 04RU-P03-B1-R1-S1 — PASS under explicit updated user scope

2026-11-17; [04RU-P03-B1-R1-S1/N1](agent-review-notes.md), same retained independent B. **PASS** artifact **9b394b31e98b0ede9d281ea7e977a5966ce683692500dc0ba2642369b43e2f0e**, base/HEAD/exclusions unchanged. B explicitly acknowledges **coordination14 / 04RU-C005/r1**: after disclosure of the named full-stock mock timeout, the user directed “we'll check doop separately”. [Immutable scope addendum](logs/B1-R1-scope-addendum.md), [coordination14 pin](logs/B1-R1-scope-addendum-coordination14.md), [manifest](logs/B1-R1-scope-addendum.evidence.json).

**F1 is USER-DEFERRED, not fixed or confirmed unrelated.** The explicit user scope change removes further Doop investigation/proof from Plan04 acceptance; original failed result, unresolved cause and isolated pass remain intact. All other source/integration and gate review from R1 is reused unchanged, with no remaining in-scope findings. Both evidence manifests validate. Full suite/CI is not claimed green. No further Doop checks were run.

Root may accept the verified Plan04 scope after its normal final pin check, commit/push main and confirm remote identity, then **STOP; no05/06**. Publication is root-owned and is not performed or claimed by this review. [Fresh source/authority checks](logs/B1-R1-S1-reviewer-final-checks.json).
