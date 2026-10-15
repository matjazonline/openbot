# Assignment D-004 — correct S findings F-001..F-003 (R-004)

Same role and settings (claude-code / claude-opus-5-5 / high).

S reviewed D-003 artifact `e2a41cc3…` and required changes; read R-004 in
`docs/agent-review-notes.md` and F-001, F-002, F-003 in
`docs/agent-review-findings.md`. Implement each correction criterion exactly:

- F-001: restore the separate all-relation entry no-write oracle.
- F-002: run the affected prior-bounds negative controls on the current artifact.
- F-003: rerun SQLx prepare/check (all targets, live task URL) after the query changes.

Rerun affected checks: corrected tests in isolation, the full `workflow_action_`
filter at stock 2 MiB/default parallelism (at least once more on the final
artifact; more if F-001 changes timing), fmt/diff-check/offline check/strict Clippy.
Record a D-004 section in IMPLEMENTATION.md referencing finding IDs, logs under
docs/logs/groupA/, and a new full manifest `logs/groupA/source-manifest-D004.json`
(all files changed since `8533a79`). Do not edit findings/notes. Stop after;
final response: per-finding evidence, checks, manifest SHA256.
