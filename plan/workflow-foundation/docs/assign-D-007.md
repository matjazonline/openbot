# Assignment D-007 — group B2: expand and implement

Same role and settings (claude-code / claude-opus-5-5 / high).

B1 is root-accepted (REVIEW.md, manifest `ef2e0ecd…`). Start sub-batch **B2**
per `docs/GUIDANCE.md` DEC-007: both receipt/cancel orders, observed-result
token case, ordinary cancelled duplicate/restart, and reuse of the 04.7 conflict
cases (DEC-002, DEC-004; DEC-001 coverage mapping; DEC-008 reopen rule). S notes
in R-006: B2 requires actual run blockers, both receipt/cancel orders, and the
observed-result settlement seam. Originals: BRIEF-04.8 §B table rows
"Result/receipt/cancel" and "Duplicate/restart"; phase 04 item 7 and acceptance.

Same procedure as B1: expand in a "Group B2" section of IMPLEMENTATION.md,
implement, verify at stock 2 MiB on the task fixture (new tests isolated and ×3,
full `workflow_action_` filter, affected receipt/control filters, fmt/diff-check/
offline check/strict Clippy, SQLx check if queries change), and write
`logs/groupB/source-manifest-B2.json` (all files changed since `8533a79`,
including untracked). Stop at review-ready B2 with summary, checks, manifest
SHA256, and open questions.
