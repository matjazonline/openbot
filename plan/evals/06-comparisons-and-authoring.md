# 06 — Comparisons and authoring

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on 04–05. Let workflow authors compare the same reviewed suite against frozen baseline
and candidate subjects, inspect failures, and carry honest evidence into their existing publish
and binding-selection decisions. An eval is not an alternative workflow activation path.

## Batch 06A — Matched comparisons and release evidence

Pair results by immutable case ID/revision, repeat index and subject. Include the captured
configuration hash and explicit agent/workflow selections in the comparison identity; equal root
workflow content hashes do not imply the same independently published agent revisions. Require
compatible dataset,
fixtures, execution mode and grading identity; record deliberate differences such as model, prompt
or agent revision. Changed reference data or fixture environment requires new evidence rather than
an unlabeled improvement claim. Freeze state/time/random inputs where controllable and record
remaining provider nondeterminism.

Report expected/completed/failed/cancelled/unavailable counts, case-level changes, hard violations,
quality by criterion/workflow category, per-class routing errors, run-to-run variation, actual
subject/judge usage, and latency distributions with their sample counts. Expose candidate losses
and critical slices; a global mean cannot conceal a failed class or missing attempt. Show price
estimates only with their basis; synthetic/unknown cost remains labeled. Use uncertainty intervals
where sample sizes and methods support them, not spurious precision on a tiny suite.

Thresholds are reviewed suite policy, not invented universal quality numbers. A passing report
requires all mandatory checks/attempts plus the declared quality criteria; incomplete runs cannot
be the only release evidence. CI can block code release on the selected suite's failed hard gates.
Workflow/agent publication and binding activation retain their current explicit authorization and
revision checks. Initial integration shows current evidence; it does not silently activate on
pass or introduce an automatic binding-selection policy.

**Focused checks:** `eval_comparison_` and `eval_release_evidence_` for matched identity, changed
dataset/grader/mode, partial attempts, repeated runs, hard-gate regressions, rare-class loss and
misleading missing-cost/latency values. Prove report completion cannot publish or move a binding.

## Batch 06B — Workflow authoring surface

Extend the workflow authoring area with suites, immutable revisions, execution mode, selected
subject(s), repeats and an explicit paid budget for live inference/grading. Display the distinction
between scripted correctness, live-model quality and disposable integration evidence. Ordinary
sample forms continue to run offline with no hidden provider fallback or new paid behavior.

Provide run admission, progress/cancellation, paginated case results, per-check evidence and a
baseline/candidate comparison. Show actionable failure location, hard-gate status, expected vs
observed facts, diagnostic class and incomplete/unavailable evidence. Keep raw sensitive content
behind the same case/run access checks; do not place it in public URLs, logs or notifications.

Use current forms, CSRF, revision handling, escaping, accessibility, SSE navigation/lifecycle and
pagination patterns. Do not allow repeated submit/navigation to admit duplicate paid runs. A stale
result or case edited during execution must remain labeled with the evaluated immutable revision.
Failure-to-case capture goes through 02's explicit reviewed draft flow, not automatic data export.

**Focused checks:** `eval_authoring_` HTTP authorization/form tests and a new browser protocol
fixture covering duplicate submissions, stale revisions, navigation/cancellation, report paging,
escaping, keyboard/error states, restricted readers and deliberate paid-run intent. Run affected
existing workflow authoring, sample and SSE tests.

## Exit criteria

Authors can identify exactly what was compared, where a candidate regressed and what remains
unverified. Existing publication/activation remains an explicit separately authorized operation.
