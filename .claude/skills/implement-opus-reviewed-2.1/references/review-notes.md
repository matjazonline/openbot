# Persistent review notes

B reads this before its first review. Use `<plan-dir>/docs/agent-review-notes.md`
inside the selected plan directory, for example
`plans/business-event-gaps/docs/agent-review-notes.md`. Root supplies the resolved
path. Share it across that plan's batches, tasks, and workflow variants. Create it
on the first review; preserve existing entries and serialize writes. B saves a
compact entry before returning each architecture gate, batch review, correction
recheck, or final integration review, including reviews with no findings. Chat
messages alone are insufficient. The log is a review artifact excluded from the
frozen implementation snapshot.

Each entry records:

- **Identity:** stable task-prefixed review-round ID, date, workflow
  (`implement-opus-reviewed-2.1`), task/review record links, batch and stage, actual
  reviewer model/effort, artifact base commit and digest, and gated decision IDs.
- **Coverage and evidence:** requirements, files/contracts and failure paths
  examined, assumptions challenged, checks actually run or inspected, and durable
  evidence links. Distinguish tested behavior from inspection and unverified claims;
  state gaps and unavailable checks explicitly. For large batches, name any part
  reviewed only lightly so later analysis does not read the pass as uniform.
- **Outcome:** pass or changes required, finding IDs, unresolved questions, and
  what a correction recheck covered or reused. Detailed defects and dispositions
  remain in `<plan-dir>/docs/agent-review-findings.md`; link rather than duplicate them.
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
and keep evidence under `<plan-dir>/docs/`, with check logs in its `logs/`
subdirectory. Root verifies persistence before acceptance and handoff.

When later asked to improve instructions, evaluate relevant notes and linked
findings across plan-local docs directories: distinguish recurring failures,
consequential one-off gaps, and task-specific choices. Propose narrowly scoped
instructions supported by the evidence and record which note/finding IDs motivated
any adopted change. Recording a candidate does not itself authorize editing skills
or `AGENTS.md`. Future workers read applicable adopted guidance and selected
relevant notes, not the entire log on every assignment.
