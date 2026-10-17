# Durable reviewer findings

B reads this reference before recording its first finding. Use
`<plan-dir>/docs/agent-review-findings.md`, inside the selected plan directory, as
the shared log across that plan's batches, tasks, and workflow variants. Root gives
workers its resolved path. Create it on the first finding; preserve existing
entries. B owns entries; serialize writes across reviewers. This is a review
artifact, excluded from the frozen implementation snapshot, so recording findings
does not invalidate the artifact under review.

Log every concrete issue from architecture gates, batch reviews, correction
rechecks, and final integration review, including optional suggestions labeled as
such. Chat-only findings are insufficient. Record findings against the artifact or
decision actually examined, not a later correction. Ordinary open design questions
are not defects by default. Use one Markdown section per finding with:

- **ID and context:** stable task-prefixed ID, date, workflow
  (`implement-opus-reviewed-2.1`), task/review record links, batch, review stage,
  review-round note ID, reviewer model/effort, and reviewed commit/digest or
  decision revision.
- **Classification:** actionable defect or optional suggestion, severity, and category
  such as requirements, architecture, correctness, security, or verification.
- **Evidence:** file/line or decision references, observed versus required behavior,
  impact, and reproduction or missing verification; link supporting logs. Identify
  when the defect originated in a gated decision rather than its implementation.
- **Instruction lesson:** violated or missing guidance and a concrete candidate
  improvement when evidence supports one; otherwise mark cause unknown. Distinguish
  observation from hypothesis. This does not authorize editing agent instructions.
- **Resolution history:** initial open status, then dated correction and recheck
  evidence with artifact identity. B records fixed, deferred, or dismissed outcomes
  with reasons. A's claimed fix alone does not close a finding, and deferring an
  acceptance-blocking defect does not make the batch acceptable.

Preserve original findings and append resolution updates, including dismissals;
never delete resolved issues. Reuse IDs for the same issue's rechecks and link
recurrences in later tasks. Link IDs from `TASK.md`/`REVIEW.md` instead of
duplicating findings. Root checks before acceptance or handoff that every reported
issue and its latest disposition is saved; there is no need to load unrelated
historical entries.

Link findings to their review-round IDs in `<plan-dir>/docs/agent-review-notes.md`.
When a later finding exposes a gap in an earlier review or gated decision, append a
linked follow-up to that review note so later evaluation can trace the missed
assumption and correction without rewriting the original conclusion.
