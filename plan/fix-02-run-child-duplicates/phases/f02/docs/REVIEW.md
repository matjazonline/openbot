# f02 review record — fix-02-run-child-duplicates

Workflow `implement-opus-reviewed-2_2`; task [TASK.md](TASK.md); notes
[agent-review-notes.md](agent-review-notes.md); findings
[agent-review-findings.md](agent-review-findings.md).

## Review inputs (pinned)

- Coordination: `plan/fix-02-run-child-duplicates/COORDINATION.md` revision 1, no notices.
- Original requirements: `plan/fix-02-run-child-duplicates/01-top-level-filter.md`,
  `02-child-rollup-on-parent.md`, `03-verification-and-release.md`; `AGENTS.md` (root, `src/`,
  persistence, http, application).
- Prerequisite: f01 accepted artifact `0b6cec8`.

## Rounds

| Round | Date | Stage | Base / HEAD | Digest | Outcome | Findings |
| --- | --- | --- | --- | --- | --- | --- |
| [F02-B1-R1](agent-review-notes.md#f02-b1-r1--batch-review-of-b1-whole-plan) | 2026-10-31 | B1 batch review (whole plan) | `0b6cec8` / `0b6cec8` + working tree | `0fad5fee42ad183834c77ddc76250703ccf00f16456c89aacb7f3d87a9a63829` | Changes required | F02-B1-F1 (actionable, Low); F02-B1-F2, F3, F4 (optional, Low) |
| [F02-B1-R2](agent-review-notes.md#f02-b1-r2--correction-recheck-of-b1) | 2026-10-31 | B1 correction recheck | `0b6cec8` / `0b6cec8` + working tree | `45f033919e017e3f9db1c3852e569bc5b8f587c56a7f41a839199fc242be395c` | Changes required | F1–F4 fixed; new F02-B1-F5 (actionable, Medium) |
| [F02-B1-R3](agent-review-notes.md#f02-b1-r3--correction-recheck-of-b1-f5-schema-change) | 2026-10-31 | B1 correction recheck (F5; schema change D6) | `0b6cec8` / `0b6cec8` + working tree | `5ca40e1fa804f8e92b7627e127c453342fde8b1c59b5aaa0d413e2ebaf0bf101` | **PASS** | F5 fixed; new F02-B1-F6 (optional, Low) |

## Findings index

| ID | Class | Severity | Summary | Status |
| --- | --- | --- | --- | --- |
| [F02-B1-F1](agent-review-findings.md#f02-b1-f1--the-slow-read-tripwire-cannot-tell-a-top-level-read-though-the-batch-defers-an-index-for-its-filter) | actionable | Low | Slow-read log and "Revisit when" do not carry lineage, though the top-level filter's index is deferred | fixed (R2) |
| [F02-B1-F2](agent-review-findings.md#f02-b1-f2--optional-the-rollups-state-classification-ignores-an-unknown-state-silently-and-re-spells-the-run-states) | optional | Low | Rollup state match has a silent `_ => {}`; recurrence of F01-B1-F8 | fixed (R2, taken) |
| [F02-B1-F3](agent-review-findings.md#f02-b1-f3--optional-the-rollups-sql-literal-is-one-379-character-line-with-whitespace-runs) | optional | Low | Rollup SQL literal is one 379-character line | fixed (R2, taken) |
| [F02-B1-F4](agent-review-findings.md#f02-b1-f4--optional-nothing-pins-the-documented-wire-value-lineagetop_level) | optional | Low | No in-tree test decodes `lineage=top_level` | fixed (R2, taken) |
| [F02-B1-F5](agent-review-findings.md#f02-b1-f5--a-top-level-agent-read-reads-every-child-only-version-to-its-end-the-record-says-the-filter-does-not-change-the-shape-of-the-work) | actionable | Medium | With an agent, `top_level` reads every child-only version to its end on every page; run-list.md says the shape is unchanged and its runbook row misreads it (missed at R1) | fixed (R3, D6 partial index) |
| [F02-B1-F6](agent-review-findings.md#f02-b1-f6--optional-the-deferred-link-table-plan-does-not-carry-lineage-so-building-it-would-bring-back-f5s-shape-for-top-level-reads) | optional | Low | Deferred link-table plan does not carry lineage; building it would bring back F5's shape for top-level reads | open (optional) |
