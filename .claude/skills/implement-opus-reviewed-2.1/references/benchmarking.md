# Optional workflow benchmarking

Read only when comparing workflow usage, cost, or quality, or when detailed
accounting is requested. Measurement is not a prerequisite for implementation.

- Use runtime-provided usage and authoritative session identities. Include root,
  A, B, retries, corrections, compactions, replacements, and resumed work belonging
  to the task. Do not infer session IDs from agent names.
- Count unique requests or task-bounded cumulative deltas. Deduplicate replayed
  snapshots, not real requests that resend history: repeated input is part of usage.
  Cached input is already part of input; reasoning tokens are already part of
  output. Report input, cached input, and output by role and model without
  double-counting subsets.
- Prefer automatic collection already supplied by the host. Do not construct a
  telemetry system or inspect unrelated sessions merely to fill missing fields.
  Missing usage is unknown, not zero. Context occupancy is not total spend. Convert
  to money only with verified applicable rates and cache treatment.
- Record batch count and size, review rounds, gates held, correction rounds,
  compactions, required checks, review outcome, and defects discovered afterward
  alongside usage. Fewer findings or fewer reviews alone do not establish higher
  quality; check notes and later findings for misses attributable to larger batches.
- Compare with `implement-claude-reviewed` or the Codex variants on representative
  tasks with similar scope, starting artifacts, acceptance criteria, and
  environments. Record each workflow's model/effort settings and review cadence.
  To isolate one change (batch size, effort, or compaction threshold), hold the
  others stable. Report sample size and limitations; do not promise savings or
  equal quality from one run.
- If the user requires strict context measurements or thresholds, record and honor
  them. Disclose unavailable measurements rather than claiming compliance.

Append a compact result to the existing task record with coverage and unknowns so
partial accounting cannot be mistaken for the whole task's cost.
