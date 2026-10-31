# f03 review record — fix-03-run-board-finished-columns

Workflow `implement-opus-reviewed-2_2`; task [TASK.md](TASK.md); notes
[agent-review-notes.md](agent-review-notes.md); findings
[agent-review-findings.md](agent-review-findings.md).

## Review inputs (pinned)

- Coordination: `plan/fix-03-run-board-finished-columns/COORDINATION.md` revision 1, no notices.
- Original requirements: `plan/fix-03-run-board-finished-columns/01-run-finished-at.md`,
  `02-bounded-board-read.md`, `03-truncation-signal-and-release.md`; f01 and f02 HANDOFF items carried
  into fix-03; `AGENTS.md` (root, `src/`, persistence, http, application). Consumer context:
  `plan/new-board-view/README.md`, `01`–`03`.
- Prerequisites: f01 `0b6cec8`, f02 `3793244` (review base and HEAD).
- B1 artifact: base = HEAD `3793244`, whole working-tree delta, digest
  `9b583f28bb6f22a813e513dcf017077e71fc24ecf5ca4a0f445813d465ee63fc`, exclusions
  `plan/fix-03-run-board-finished-columns/phases/f03/docs` and
  `plan/fix-03-run-board-finished-columns/COORDINATION.md`. After corrections (R2): digest
  `42c1472d862a198d1830c1949de8878bb85c5674f4cf4559e3cdae1ec41fa484`, same base and exclusions.
- Gate G1 decision revision 1: `logs/G1-r1.md`, sha256
  `a808388d73a19216c11a01c582c0e6967f27222d191143000e1069e8e6f87613` (identical to TASK.md's section
  at R1).
- Gate G1 decision revision 2: `logs/G1-r2.md`, sha256
  `5b7724e16980e3220a03fc87abbda00df852fb86d2dc956924942e229f833654` (identical to TASK.md's section
  at R2).

## Rounds

| Round | Date | Stage | Base / HEAD | Artifact | Outcome | Findings |
| --- | --- | --- | --- | --- | --- | --- |
| [F03-G1-R1](agent-review-notes.md#f03-g1-r1--architecture-gate-g1-decision-revision-1) | 2026-10-31 | Architecture gate G1 (before B1) | `3793244` / `3793244`, clean source tree | G1-D1 … D5 revision 1 (`logs/G1-r1.md`) | Changes required | F03-G1-F1 (actionable, Medium); F2, F3, F4, F5 (actionable, Low); F6, F7 (optional, Low) |
| [F03-G1-R2](agent-review-notes.md#f03-g1-r2--gate-recheck-decision-revision-2) | 2026-10-31 | Gate G1 recheck (before B1) | `3793244` / `3793244`, clean source tree | G1-D1 … D5 revision 2 (`logs/G1-r2.md`) | **PASS for gate G1 decision revision 2** | F1–F5 fixed in the decision; F6, F7 taken; new F03-G1-F8 (optional, Low) |
| [F03-B1-R1](agent-review-notes.md#f03-b1-r1--batch-review-of-b1-whole-plan) | 2026-10-31 | B1 batch review (whole plan) | `3793244` / `3793244` + working tree | `9b583f28bb6f22a813e513dcf017077e71fc24ecf5ca4a0f445813d465ee63fc` | Changes required | F03-B1-F1, F2 (actionable, Low); F3, F4 (optional, Low); F03-G1-F8 resolved (A1) |
| [F03-B1-R2](agent-review-notes.md#f03-b1-r2--correction-recheck-of-b1) | 2026-10-31 | B1 correction recheck (F1–F4, A3 revision 2) | `3793244` / `3793244` + working tree | `42c1472d862a198d1830c1949de8878bb85c5674f4cf4559e3cdae1ec41fa484` | **PASS** | F1, F2, F4 fixed; F3 dismissed; no new findings |

## Findings index

| ID | Class | Severity | Summary | Status |
| --- | --- | --- | --- | --- |
| [F03-G1-F1](agent-review-findings.md#f03-g1-f1--d1s-trigger-inventory-omits-the-run-snapshot-guard-and-its-order-does-not-matter-claim-is-false-for-the-revision-trigger) | actionable | Medium | D1 omits `workflow_run_snapshot_immutable` (whose exemption list lacks `finished_at`; reproduced rejection); its "order does not matter" claim is false for the revision trigger; `capture_catalog`'s pinned trigger list not named | fixed in decision r2 (R2) |
| [F03-G1-F2](agent-review-findings.md#f03-g1-f2--d4s-evidence-plan-cannot-show-the-two-working-set-dimensions-that-decide-the-index-lineage-and-channel-visibility) | actionable | Low | D4 evidence leaves out lineage (children in the window, no top-level candidate) and channel visibility (seed has no channels); windows and selection criterion unstated | fixed in decision r2 (R2) |
| [F03-G1-F3](agent-review-findings.md#f03-g1-f3--d5-folds-the-child-rollup-into-the-one-elapsed-time-plan-03-reads-as-working-set-vs-pool-or-visibility) | actionable | Low | D5's single `elapsed_ms` includes the rollup, so plan 03's reading would misattribute a slow rollup; `returned_cards` undefined | fixed in decision r2 (R2) |
| [F03-G1-F4](agent-review-findings.md#f03-g1-f4--d3s-json-route-resolves-binding_id-only-company-scoped-and-has-no-route-test-plan) | actionable | Low | Board JSON route resolves `binding_id` without visibility (`visible_binding`), and names no cross-tenant or hidden-channel route tests | fixed in decision r2 (R2) |
| [F03-G1-F5](agent-review-findings.md#f03-g1-f5--d2-puts-the-openterminal-split-in-rust-beside-the-sql-one-with-no-equivalence-check) | actionable | Low | `RunStateName::is_open` and the SQL terminal set have no DB-backed equivalence test | fixed in decision r2 (R2) |
| [F03-G1-F6](agent-review-findings.md#f03-g1-f6--optional-maxtotal-shown-can-report-a-cut-column-as-50-of-50) | optional | Low | `max(total, shown)` allows a cut column reading "50 of 50"; use rows read; pure assembly as the race-test seam | taken in decision r2 (R2) |
| [F03-G1-F7](agent-review-findings.md#f03-g1-f7--optional-read-side-shape-notes-for-b1) | optional | Low | `RunRow` tuple → `FromRow` struct; `finished_at` in `run_json`; board module beside `bindings.rs`; constant card `LIMIT` | taken in decision r2 (R2) |
| [F03-G1-F8](agent-review-findings.md#f03-g1-f8--optional-revision-2-drops-revision-1-text-without-marking-it-including-d1s-failure-modes) | optional | Low | r2 drops r1 text without (r2) marks: D1's failure modes (direct `finished_at` writes allowed) and D4's "record the adopted index in run-list.md" | resolved outside the gate text (A1, B1-R1) |
| [F03-B1-F1](agent-review-findings.md#f03-b1-f1--the-index-record-misstates-the-evidence-a-reads-only-the-window-in-auto-mode-and-b-does-not-stop-scoped-finished-columns-at-51) | actionable | Low | A4 and run-list.md say (A) reads history for scoped cards in both plan modes (only generic) and that (B) stops scoped finished columns at 51 (they read their window); dataset omits that measured scopes have no finished runs in any window | fixed (R2) |
| [F03-B1-F2](agent-review-findings.md#f03-b1-f2--plan-02s-db-backed-race-case-a-run-inserted-between-the-count-and-card-reads-is-covered-only-by-a-pure-unit-test) | actionable | Low | Plan 02's DB-backed `run_board_read_*` case "a run inserted between the count and card reads" exists only as a pure unit test (cut column only) | fixed (R2) |
| [F03-B1-F3](agent-review-findings.md#f03-b1-f3--optional-the-child-rollups-match-keeps-a-catch-all-so-a-new-run-state-would-be-counted-as-neither-open-nor-failed) | optional | Low | `child_rollup`'s `Some(_) => {}` absorbs a future `RunStateName` variant | dismissed (R2, A14) |
| [F03-B1-F4](agent-review-findings.md#f03-b1-f4--optional-the-evidence-logs-taskmd-cites-are-gitignored-and-will-not-be-committed) | optional | Low | Cited `logs/*.log` evidence is gitignored (`.gitignore:3`) and will not be committed; recurring f01–f03 | fixed (R2, A15) |
