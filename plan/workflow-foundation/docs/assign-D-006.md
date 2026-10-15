# Assignment D-006 — correct F-004 (R-006) in B1

Same role and settings (claude-code / claude-opus-5-5 / high).

S reviewed B1 artifact `822434bc…`: changes required. Read R-006 in
`docs/agent-review-notes.md` and F-004 in `docs/agent-review-findings.md`
(durable-pending test can pass via 3 s deadline expiry inside its 8 s timeout
without proving cancellation-driven ownership interruption; pending checkpoint
too late). Implement the correction criterion; do not raise or relax bounds.
S answered your three questions: all "yes" (see R-006).

Rerun: the corrected test isolated and repeated (×3), the B1 filter, the full
`workflow_action_` filter, fmt/diff-check/offline check/strict Clippy. Add a
"B1 correction (F-004)" subsection to IMPLEMENTATION.md, logs in logs/groupB/,
and a new manifest `logs/groupB/source-manifest-B1r1.json` (all files changed
since `8533a79`). Stop after; final response with evidence and manifest SHA256.
