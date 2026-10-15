# Assignment D-008 — correct F-005 (R-008) in B2

Same role and settings (claude-code / claude-opus-5-5 / high).

S reviewed B2 artifact `19abba9a…`: changes required. Read R-008 in
`docs/agent-review-notes.md`, F-005 in `docs/agent-review-findings.md`, and new
DEC-012 in `docs/GUIDANCE.md`. Receipt-first must prove timely gate release and
ordinary attempt completion; its oracle must reject ambiguous-COMMIT recovery.
Answers to your questions: (1) yes; (2) 6 s fixture lease acceptable under DEC-012
once F-005 is corrected; (3) no third blocker required, but do not describe
`Interrupted` as proving a literal return-authorization wait (fix wording in
records/comments if it does).

Rerun the corrected test isolated ×3 (plus a negative control if the criterion
calls for one), B1+B2 filter, full `workflow_action_` filter, fmt/diff-check/
offline check/strict Clippy. Add a "B2 correction (F-005)" subsection to
IMPLEMENTATION.md, logs in logs/groupB/, manifest
`logs/groupB/source-manifest-B2r1.json`. Stop after with evidence + SHA256.
