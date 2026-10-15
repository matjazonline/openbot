# Assignment D-003 — reconciliation test corrections under load (S guidance DEC-009..011)

Same role and settings (claude-code / claude-opus-5-5 / high). Run after D-002.

S diagnosed the `workflow_action_` full-filter failures (R-002 in
`docs/agent-review-notes.md`, DEC-009, DEC-010, DEC-011 in `docs/GUIDANCE.md`).
No production defect established; corrections are test-side. Read those
decisions and implement them exactly:

- DEC-010: keep existing limits; renew genuine ownership at quiescent boundaries,
  move expensive diagnostics outside critical intervals, require actual supported
  entry/completion, make lease lapses fail distinctly; keep CI budget guards.
- DEC-011: measure verifier release/return and settlement timing for the
  prior-bounds timeout; preserve prompt-release negative controls, inspect
  durable outcomes before retry, complete proof within the existing
  three-attempt ceiling; timeout alone never passes.

If a decision cannot be implemented reliably, or measurements contradict DEC-009,
stop the affected edits and return evidence to root (do not change limits or
weaken oracles on your own).

Verify: the corrected tests in isolation and the full `workflow_action_` filter
at stock 2 MiB/default parallelism on the task fixture, repeated at least 3 times
(all must pass; record every run), plus fmt/diff-check/offline check/strict Clippy.
Record in IMPLEMENTATION.md, logs under docs/logs/groupA/, and the combined
group A artifact manifest (all files changed since `8533a79`, SHA256 each) in
`docs/logs/groupA/source-manifest-D003.json`. Stop after; final response with
results, changed files, and manifest SHA256.
