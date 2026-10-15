# Assignment S-004 — review D-003 delta (reconciliation test corrections)

Same role and settings (codex / gpt-6-astra / high). Read-only. D is idle; no
project edits occur during this review.

Artifact: HEAD `8533a79` + worktree changes identified by
`logs/groupA/source-manifest-D003.json` (SHA256
`e2a41cc3b06aabaaefb0939de2916f94f8f657aee91d5ec403d6401643cc93cb`, 15 files;
the six D-002 files are unchanged from your R-003 PASS). Verify the worktree
matches it (no other tracked changes, nothing staged) before reviewing.

Review the nine D-003 test files against DEC-009..011 and the AGENTS.md
"raised limit must leave something that fails" rule: no weakened oracle, no
raised bound, renewals via genuine production path, lease lapse fails distinctly,
the single-statement `all_tables` is equivalent (and its equivalence test is
meaningful), prior-bounds pause/digest still proves what it proved before, and
the fourth test (`..._live_return_conflict_vetoes_genuine_new_result`) disposition
outside DEC-009. Evidence: D-003 section of IMPLEMENTATION.md and logs.

Also decide group A acceptance readiness: with R-001 (code), R-003 (D-002) and
this review, plus D's verification logs (D-001/D-002/D-003), is every group A
gate from BRIEF-04.8 §A/TASK.md satisfied on this artifact? Record R-004 and
findings. Final response: pass/changes-required naming the manifest hash, and
group A readiness with any gaps.
