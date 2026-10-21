# Persistent review notes

B reads this before its first review. Use `<phase-docs>/agent-review-notes.md`
inside the assigned phase record directory, for example
`plans/business-event-gaps/phases/p07/docs/agent-review-notes.md`. Root supplies the
resolved path. Share it across this phase's batches, tasks, and workflow variants,
never across concurrent phases. Create it on the first review; preserve existing
entries and serialize writes. B saves a
compact entry before returning each architecture gate, batch review, correction
recheck, or final integration review, including reviews with no findings. Chat
messages alone are insufficient. The log is a review artifact excluded from the
frozen implementation snapshot.

Each entry records:

- **Identity:** stable plan/phase/task-prefixed review-round ID, date, workflow
  (`implement-astra-reviewed-2.2`), task/review record links, batch and stage, actual
  reviewer model/effort, phase ID, worktree, review-base commit, HEAD, artifact
  digest, and gated decision IDs.
  Include the authoritative plan coordination path, applicable notice IDs/revisions,
  and retained immutable evidence used for the review; record the freshness check.
- **Coverage and evidence:** requirements, files/contracts and failure paths
  examined, assumptions challenged, checks actually run or inspected, and durable
  evidence links. Distinguish tested behavior from inspection and unverified claims;
  state gaps and unavailable checks explicitly. For large batches, name any part
  reviewed only lightly so later analysis does not read the pass as uniform.
- **Outcome:** pass or changes required, finding IDs, unresolved questions, and
  what a correction recheck covered or reused. Detailed defects and dispositions
  remain in `<phase-docs>/agent-review-findings.md`; link rather than duplicate them.
- **Process and instruction feedback:** whether the batch was too large or too small
  to review thoroughly, whether a gated decision was missing or unnecessary, and
  what in the author's records or repository instructions was missing, misleading,
  or demonstrably useful. Distinguish defects in gated decisions from errors in
  implementation. Record a concise candidate instruction, its intended scope, and
  supporting evidence, or state that no change is supported. Save conclusions and
  observable evidence, not private reasoning transcripts.

Append dated follow-ups for corrections, later-discovered misses, or revised
conclusions; preserve the original review outcome. Link a later defect to the
earlier pass and its finding ID when evidence establishes that relationship. Do not
infer that a pass was comprehensive or that no findings means the process was
effective. Preserve the original text of gated decisions before they are revised,
and keep evidence under `<phase-docs>/`, with check logs in its `logs/`
subdirectory. Root verifies persistence before acceptance and handoff.

When later asked to improve instructions, evaluate relevant notes and linked
findings across relevant phase record directories: distinguish recurring failures,
consequential one-off gaps, and task-specific choices. Propose narrowly scoped
instructions supported by the evidence and record which note/finding IDs motivated
any adopted change. Recording a candidate does not itself authorize editing skills
or `AGENTS.md`. Future workers read applicable adopted guidance and selected
relevant notes, not the entire log on every assignment.
