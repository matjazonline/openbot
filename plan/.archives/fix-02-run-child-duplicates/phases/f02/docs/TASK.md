# f02 — fix-02-run-child-duplicates

## Identity

- Phase ID: `f02` (whole unphased plan `plan/fix-02-run-child-duplicates/`; IDs `F02-B<n>-R<n>` /
  `F02-B<n>-F<n>`). Workflow `implement-opus-reviewed-2_2`, skill dir
  `/Users/mac02/.codex/worktrees/b109/openbot/.claude/skills/implement-opus-reviewed-2_2`.
- Record root: `/Users/mac02/.codex/worktrees/b109/openbot/plan/fix-02-run-child-duplicates/phases/f02/docs/`.
- Coordination: `plan/fix-02-run-child-duplicates/COORDINATION.md` revision 1, checked at startup;
  no notices.
- Root = A: this Claude Code session, Opus 5.5 / xhigh (user-confirmed 2026-10-30). B: a fresh
  reviewer for this phase (`general-purpose`, `model: opus`, `effort: xhigh`): agent
  `aefe55d63c37d2fe5`, spawned 2026-10-31 for F02-B1-R1.
- Worktree `/Users/mac02/.codex/worktrees/b109/openbot`, detached HEAD; starting commit and review
  base `0b6cec8` (= `origin/main`, the accepted f01 artifact). No preserved pre-existing changes.
- Snapshot exclusions: `plan/fix-02-run-child-duplicates/phases/f02/docs`
  `plan/fix-02-run-child-duplicates/COORDINATION.md`.
- Prerequisite: f01 accepted (B PASS F01-B1-R3, digest `3cd0d212…`), pushed as `0b6cec8`.
- Context: `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75`. Authorizations: commit and push to `main` once
  accepted. No dev/production database reset; checks use disposable databases.

## Original requirements

`plan/fix-02-run-child-duplicates/01-top-level-filter.md`, `02-child-rollup-on-parent.md`,
`03-verification-and-release.md`; repository `AGENTS.md` files (root, `src/`, persistence, http,
application).

## Acceptance criteria (from the plan)

1. `RunLineage::{Any, TopLevel}` on `RunFilter` (named enum, default `Any`); `TopLevel` filters
   in SQL (`parent_run_id IS NULL`); JSON route takes `lineage=top_level`; the board-vs-list
   decision is recorded on the enum; TopLevel shapes measured and recorded.
2. `run_top_level_*` tests: agent only in a child → TopLevel+agent gives the parent once, Any both;
   a rerun is returned; pages full with children interleaved and the cursor continues; default
   unchanged for existing callers.
3. `child_rollup(company, parents)` → direct children by state (total, open, failed), one grouped
   statement over `workflow_run_parents` (no per-card query), bounded input refused above the bound.
4. Child visibility settled and pinned by a test (children inherit the parent's association, or
   counts are restricted to visible channels).
5. `run_top_level_rollup_*` tests: none → absent; mixed states; foreign ids → nothing; grandchild
   not counted on the root; oversized input refused.
6. Existing list test unchanged and passing; doc comments carry the decisions; focused checks then
   `scripts/check.sh` once.

## Batches

One batch `B1` (the whole plan). No architecture gate: read-only filters and a grouped read on
established patterns, no schema change expected.

## Decisions (A)

- D1 — Child visibility: children inherit the parent's association. Evidence: every child is
  written by `children::create_on` (`src/adapters/persistence/workflow/children.rs:219`, "the
  single runtime child writer"), which copies the parent run's `channel_id`/`thread_id`; its three
  callers (`children.rs`, `child_waits.rs`, `workflow_tool_calls.rs`) all go through it, and
  `workflow_runs_preserve_association` makes associations immutable. Not enforced by a database
  constraint, so a test pins it on a channel-associated parent. The rollup therefore needs no
  per-child visibility: it is only ever read for parents the caller may see.
- D2 — The rollup bound is `MAX_ROLLUP_PARENTS = 300`, six board columns of 50 cards. fix-03 defines
  `BOARD_COLUMN_CAP` and must keep the two in step (a compile-time assertion there).

- D3 — The rollup reads `workflow_run_parents` (direct links, indexed by
  `workflow_run_parents_by_parent`) joined to the child runs, grouped by parent and state; it
  refuses more than `MAX_ROLLUP_PARENTS` with `BadRequest`, as `publication_options` refuses an
  oversized name list.
- D4 — The cross-check test (plan 03 step 1) lives with f01's route-level agent tests because the
  persistence publish fixture stubs agent capture. Its child freezes the agent on a
  `decision.rule` branch never taken, so engine workers run the tree without a model call (test
  engines call real providers).
- D5 — `scripts/tests/run-list-query-plan.py` now strips psql's header padding: modifying the
  tracked evidence file otherwise fails `check.sh`'s `git diff HEAD --check` gate. The committed
  "before" output got the same whitespace-only cleanup, and `docs/performance/run-list.md` figures
  were refreshed to the regenerated output (buffers and row counts identical; times within a few
  percent).

## Progress

- Status: **accepted in phase** 2026-10-31 — B PASS F02-B1-R3 for digest
  `5ca40e1fa804f8e92b7627e127c453342fde8b1c59b5aaa0d413e2ebaf0bf101`, recomputed by root at
  acceptance; actionable F1 and F5 fixed, optional F2–F4 taken; coordination rev 1, no notices.
  Integrated: committed with these records and pushed to `main` (hash in f03's TASK).
- F02-B1-F6 (optional: the deferred `plan/db_improve/run-agent-link-table.md` needs lineage in the
  link design) is carried into fix-03 rather than reopening this artifact.

## Corrections after F02-B1-R2 (2026-10-31)

- F02-B1-F5: D6 — `workflow_runs_version_top_level` partial index in the init schema; the
  `run_list_query` doc names it. `scripts/tests/run-list-query-plan.py` gains a child-only history
  section (100,000 children on one dense-agent version, measured with the partial index and with it
  dropped from the temp clone). `docs/performance/run-list.md` "Top-level runs only" rewritten
  (child-only versions, partial index, both tables, size 122 MB), its wrong "does not change the
  shape" claim removed, the `lineage=top_level, agent=true` "Revisit when" row corrected, and all
  figures refreshed to the regenerated output.
- F02-B1-F4 nit (`agent_id=` line could use `is_err()`): left as is; optional, not reopened.

- D6 — (after F02-B1-F5, 2026-10-31) Schema change: `workflow_runs_version_top_level`, a partial
  copy of `workflow_runs_version_newest` `WHERE parent_run_id IS NULL`. Without it, an agent +
  top-level read takes every child-only version's range to its end on every page (the shape a
  workflow only ever called as a child produces), growing with that workflow's history; the
  board will always read top-level. Evidence: the script's new child-only history section (100,000
  children on one dense-agent version: 19,630 buffers / 13.4 ms with the partial index, 37,584 /
  40.9 ms without it). Cost: 122 MB for 983,770 top-level runs; written on insert and every state
  change. This reverses 01 step 5's expected "no index" with the recorded evidence; it is a cheap,
  reversible init-schema index, reviewed in the correction recheck rather than a separate gate.

## Corrections after F02-B1-R1 (2026-10-31)

- F02-B1-F1: the slow-read log names the lineage (`logged_lineage`: `any` / `top_level`); unit
  test `the_slow_read_log_names_the_lineage`; `docs/performance/run-list.md` lists the field and
  adds two "Revisit when" rows for top-level reads (company-wide: a `WHERE parent_run_id IS NULL`
  partial twin of the index used, measured first; with an agent: read as the agent rows).
- F02-B1-F2 (optional, taken): `child_rollup` names `succeeded | cancelled` and returns
  `invalid()` for any other stored state.
- F02-B1-F3 (optional, taken): the rollup SQL is back on `\` continuations. Cause: a Python edit
  script's `\`-newline was read as a Python line continuation; later edits with escapes use the
  Edit tool.
- F02-B1-F4 (optional, taken): `run_top_level_query_decodes_the_documented_lineage_values` decodes
  `RunsQuery` with `Query::try_from_uri`: `top_level`, `any`, absent accepted; other values
  refused.
- Changed: `src/application/workflow/reads.rs` (`RunLineage`, `RunFilter.lineage`,
  `MAX_ROLLUP_PARENTS`, `ChildCounts`, `child_rollup` port), `src/adapters/persistence/workflow/reads.rs`
  (`AND parent_run_id IS NULL`, `child_rollup`), `reads_tests.rs` (two shapes + clause assertion),
  `run_lineage_tests.rs` (new, under `admission_tests.rs`), `bindings.rs` (`RunsQuery.lineage`),
  `ui/runs/mod.rs` (`RunLineage::Any`), `run_filter_tests.rs` (route test),
  `scripts/tests/run-list-query-plan.py` (padding), `docs/performance/*` (top-level section,
  refreshed figures, regenerated evidence), `docs/workflow-operations.md` (listing filters).

## Check results

All with `SQLX_OFFLINE=true RUST_TEST_THREADS=4` against disposable `mail_agents_f02`.

| Command | Result | Log |
| --- | --- | --- |
| `cargo test --lib workflow::reads::tests` | 3 passed, 1 ignored | inline |
| `cargo test --lib run_top_level_lists_a_parent_once` | 1 passed | `logs/route-top-level-1.log` |
| `cargo test --lib run_top_level_` | 6 passed | `logs/run_top_level-1.log` |
| `python3 scripts/tests/run-list-query-plan.py …` (migrated `mail_agents_f02`; rerun after D5) | exit 0 | `docs/performance/run-list-query-plan.txt` |
| `cargo test --lib -- routes::workflows:: workflow::reads::tests run_lineage_tests` | 92 passed, 1 ignored (incl. unchanged `the_list_pages_over_runs_an_admin_cannot_see_and_labels_reruns_and_children`) | `logs/focused-1.log` |
| `git diff HEAD --check` | ok after D5 | inline |
| `RUST_TEST_THREADS=4 DATABASE_URL=…/mail_agents_f02 ./scripts/check.sh` | exit 0; 15/15 PASS; 2,468 lib tests passed (6 ignored), again at 2 MiB | `logs/check-1.log` |

| After R1 corrections: `cargo test --lib -- workflow::reads::tests run_top_level_` | 11 passed, 1 ignored | `logs/recheck-1.log` |
| After R1 corrections: `cargo clippy --locked --offline --all-targets -- -D warnings` | exit 0 | `logs/clippy-2.log` |
| After R1 corrections: `cargo fmt --all -- --check`, `git diff HEAD --check` | ok | inline |

| After R2 corrections (schema change): DB re-created and migrated; `python3 scripts/tests/run-list-query-plan.py …` | exit 0 | `docs/performance/run-list-query-plan.txt` |
| After R2 corrections: `RUST_TEST_THREADS=4 DATABASE_URL=…/mail_agents_f02 ./scripts/check.sh` | exit 0; 15/15 PASS; 2,470 lib tests passed (6 ignored), again at 2 MiB | `logs/check-2.log` |
| After R2 corrections: `scripts/lease-budget.sh --offline` | exit 0; 130 passed; `budget.reserve` 358/1000, `lease.renew` 309/2000, `completion` 215/2000 ms (f01: 359, 313, 215) | `logs/lease-budget-1.log` |

Before R2 (no schema change), `scripts/lease-budget.sh` was not run: no schema change and no leased operation touched (read paths
only). Artifact frozen for review: digest
`0fad5fee42ad183834c77ddc76250703ccf00f16456c89aacb7f3d87a9a63829` (base `0b6cec8`).
