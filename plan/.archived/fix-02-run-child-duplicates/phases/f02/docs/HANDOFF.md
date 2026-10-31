# f02 handoff — fix-02-run-child-duplicates

- Phase `f02`, workflow `implement-opus-reviewed-2_2`; records here; coordination
  `plan/fix-02-run-child-duplicates/COORDINATION.md` revision 1 (no notices).
- Root = A: Claude Code session, Opus 5.5 / xhigh (user-confirmed). B: agent `aefe55d63c37d2fe5`
  (Opus, xhigh): R1 changes (F1 Low; optional F2–F4), R2 changes (F5 Medium), R3 PASS. Finished.
- Worktree `/Users/mac02/.codex/worktrees/b109/openbot`, start/review base `0b6cec8`.
- **Accepted in phase** for digest `5ca40e1fa804f8e92b7627e127c453342fde8b1c59b5aaa0d413e2ebaf0bf101`
  (exclusions `plan/fix-02-run-child-duplicates/phases/f02/docs`
  `plan/fix-02-run-child-duplicates/COORDINATION.md`); committed with these records and pushed.
- Delivered: `RunLineage` (+ JSON `lineage`), `child_rollup` + `ChildCounts` +
  `MAX_ROLLUP_PARENTS = 300`, partial index `workflow_runs_version_top_level`, slow-read log fields
  `lineage`, child-only history evidence, docs.
- Contract for fix-03: board reads use `RunLineage::TopLevel`; pass `child_rollup` only the shown
  cards; `RunFilter` + `push_run_page` / `run_list_query` builder; extend
  `dump_run_list_query_shapes` and `parameters()` for any new statement shape; the evidence
  script regenerates every figure, so refresh `docs/performance/run-list.md` after a re-run.
- Carry into fix-03:
  - F01-B1-F8 / F02-B1-F2: one shared list of run-state names (`logged_state`, UI `state_filter`,
    rollup and board state matches);
  - the board must pass `child_rollup` the shown cards, not cap + 1 (6 × 51 = 306 >
    `MAX_ROLLUP_PARENTS`); add a compile-time `MAX_ROLLUP_PARENTS >= 6 * BOARD_COLUMN_CAP`;
  - slow-read logging for the board statements naming their shape;
  - F02-B1-F6: the deferred link-table plan's design must carry lineage (e.g. a `top_level` column
    with a partial index) or a top-level page steps over child links again;
  - the board's agent scope reads top-level, so it must keep child-only versions out
    (`workflow_runs_version_top_level` or equivalent).
- Dev database `mail_agents` untouched; reset before `serve` (init schema changed again).
