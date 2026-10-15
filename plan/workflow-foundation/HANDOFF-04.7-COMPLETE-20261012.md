# 04.7 complete — final handoff, 2026-10-12

**Current user-authorized 04.7 is VERIFIED and root-accepted. STOP here.**
No 04.8, migration squash, staging, commit or deployment was performed. Later work
requires a new user instruction. This closes 04.7, not all of phase04 or phase10.

## Acceptance and verification

Independent Astra review gives explicit actual-code/criteria/integration PASS,
with no findings: [final closure](/private/tmp/workflow-foundation-20261012-root69/reviewer-closure/CLOSURE.md).
Original requirements remain [BRIEF-04.7](BRIEF-04.7.md) and execution item6 in
[04 actions](04-actions-http-and-delivery.md), with the established user-authorized
scope dispositions below. Root accepts on that review plus the completed gates.

Final evidence: [author handoff](/private/tmp/workflow-foundation-20261012-root69/author-final-instrumented/HANDOFF.md).
- Full library at stock2048KiB/default parallelism: **2532 passed,0 failed,22 unchanged
  existing ignored,0 filtered**,592.69s. All2554 registered outcomes accounted for.
- All28 strict race sites mapped to20 tests, and all30 worker/fairness/polling tests passed.
- Unchanged lease-budget gate:87 passed; maximum measured work ratio35.7%, below50%.
- Formatting, whitespace, locked offline all-target compilation, strict Clippy,
  SQLx prepare/check, fresh53 migrations with exactSHA384 checksums, complete
  fresh/retained schema equality and graft build passed.
- Source/WIP preservation, exact database identity and resource cleanup verified.

This continuation finished deferred coverage D1–D3 (8cells/6rollback witnesses/
2ordinary commits,65affected tests and scoped gates), corrected two documentation
nits, and completed final integration. Independent code review corrected diagnostic
line attribution and row/witness comparison assertions without weakening criteria.
Earlier accepted row3d/row3e and native-owner checkpoint details are retained in
Git history for `HANDOFF-20261012-cont.md` and `CONTINUATION-20261012-root69.md`;
these superseded logs were removed during the 2026-10-15 planning cleanup.

## Retained limitations and history

The first final full run FAILED2531/1/22 at the proof-expiry test's final dispatch
with a generic lifecycle conflict. Its cause remains unknown. Reviewed read-only
diagnostics preserved all behavior, order, assertions and limits. One targeted
instrumented run and one full original-workload instrumented run passed. This is
current green execution, **not a causal fix or resolved-flake claim**. Failed logs
remain in `author-final/stock.log` under the evidence root.

No new criterion waiver was made. The existing authorized SQL A/C amendment remains:
A is layered witness-pair/public-attachment evidence; the exact otherwise-valid
public historical-witness diagnostic remains mandatory Phase10 reassessment.
C uses the accepted committed adversarial topology. See [A/C evidence](EVIDENCE-04.7-SQL-AC.md).
Row3f remains user-dropped. Selected-owner probes are not natural-COMMIT negatives;
the redundant attribution grammar CHECK has catalog/redundancy evidence only.
Future production-verifier, confidentiality/note-canonical/U+0000/direct4096 and
TRUNCATE followups remain outside this completion. Phase04.8–11 and later phases
remain outstanding; none started here.

## Exact artifact and resources

HEAD `6436ecb70463065d69577e9a80ad51386b9fe8b1`; index empty, all inherited WIP preserved.
Final1011-file checksum inventory (not1011changed files):
[FINAL-FROZEN.json](/private/tmp/workflow-foundation-20261012-root69/author-final-instrumented/FINAL-FROZEN.json),
SHA256 `d22bfa240f04f130ff51b7d8dbcf033ab1f27ceb5016392e136a1b6ef88ebd8a`.
It matches the tested artifact, original31 accepted-source hashes and2 diagnostic
hashes. Subsequent root changes are acceptance documentation only.

Retained authorized PostgreSQL18.6/system7693072113847127473/mac03/workflow_admission/
55440/max200/data `/Volumes/ssd1/dev/workflow-recovery-20261005/test-pg` is left running.
Both test URLs were `postgres://mac03@127.0.0.1:55440/workflow_admission`.
Original4databases and zero other clients verified; task-created schema database
removed, no owned processes/fixtures/pending approvals. Exact command provenance
is in final evidence RESOURCE-PROVENANCE.md. Damaged55439 and5432 remain untouched.
No production/schema/dependency/cache/resource-bound changes in this continuation.
All six workers quiescent; exclusive build/DB ownership released.

## Workflow and usage

Used implement-astra-reviewed: root actual runtime gpt-6-astra/medium unchanged,
three fresh author/reviewer pairs, each Astra/medium. Role UUIDs, context samples,
findings/rechecks and accepted gates are in the continuation and evidence records.
Worker50%/root60% thresholds retained. Two grouped code correction/recheck rounds
for deferred fixtures; later diagnostic review added observations, not a behavior fix.

[Usage snapshot](/private/tmp/workflow-foundation-20261012-root69/USAGE-FINAL.json)
deduplicates527 response IDs across this root and six workers:37,096,031input
(including36,349,440cached),101,649output tokens, all gpt-6-astra. It includes retries
and corrections, excludes later documentation calls; earlier linked continuations
are not fully reconciled, so whole-task usage/cost is unknown. No price inferred.
Elapsed continuation approximately09:20–10:40UTC, including waits and failed checks.
Root graft estimate19,208tokens/1call; worker estimates are in their records.
