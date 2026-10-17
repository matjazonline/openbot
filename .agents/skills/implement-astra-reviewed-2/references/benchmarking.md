# Optional workflow benchmarking

Read only when comparing workflow usage, cost, or quality, or when detailed
accounting is requested. Measurement is not a prerequisite for implementation.

- Use runtime-provided usage and authoritative session identities. Include root
  and every author/reviewer session, retries, corrections, rotations, and resumed
  work belonging to the task. Do not infer session UUIDs from collaboration handles.
- Count unique requests or task-bounded cumulative deltas. Deduplicate replayed
  snapshots and inherited history. Cached input is already part of input; reasoning
  tokens are already part of output. Report input, cached input, and output by model.
- Prefer automatic collection already supplied by the host. Do not construct a
  telemetry system, inspect unrelated sessions, or copy the original skill's
  context-measurement machinery into this workflow merely to fill missing fields.
- Missing usage is unknown, not zero. Context occupancy is not total spend.
  Convert to money only with verified applicable rates and cache treatment.
- Record elapsed time, correction rounds, required checks and review outcome, and
  defects found afterward separately from token usage. Do not treat fewer review
  findings as proof of higher quality.
- Compare representative tasks with similar scope, starting artifacts, acceptance
  criteria, and environments. Keep author/reviewer model and effort stable while
  testing process changes. Report sample size and limitations; do not promise a
  savings percentage or equal quality from one run.
- If the user explicitly requires strict context measurements or thresholds,
  record and honor those additional constraints. Disclose unavailable measurements
  and resolve that requirement instead of silently claiming compliance.

Append a compact result to the existing task record. Include coverage and unknowns
so partial accounting cannot be mistaken for the whole task's cost.
