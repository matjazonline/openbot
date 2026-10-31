# NBV-P04 — Put Runs in the rail

- Workflow: implement-opus-reviewed-2_2 (skill dir /Users/mac02/.codex/worktrees/b109/openbot/.claude/skills/implement-opus-reviewed-2_2)
- Requirements: plan/new-board-view/README.md, plan/new-board-view/04-runs-in-navigation.md
- Root = A (Opus 5.5, user-confirmed medium+); B = a02b779b149185911 (Opus 5.5/xhigh). Worktree /Users/mac02/.codex/worktrees/b109/openbot, HEAD 92e8c35, uncommitted.
- Prerequisites: P01–P03 accepted; P03 at digest 38ee1d9edf0121d05d97eb866c403ef2915da61ede2959c13560bbba39cdef49 = tree f07c076b2242a35b0bd97bf400b1bcc854d04603 (review base). This phase's state: tree 7a120c2817ecca236772a033beb3e9d69a6f735c.
- Coordination: COORDINATION.md r4. NBV-I1 r1 ack; no notice targets p04 (N3/N4 → p05).
- Environment: DB tests `-- --test-threads=4`.

## Batches
- B1: whole phase 04.

## Decisions
- NBV-P04-D1: `icon_rail` destinations are `RailDestination { section, path, query, glyph, title }`; `RailDestination::href` is the one builder: form-urlencodes the query pairs, appends `company_id`, escapes the result for the attribute. Existing destinations have no query, so their href is byte-identical (`{path}?company_id={id}`).
- NBV-P04-D2: `UiSection::Runs` after Workflows → `/ui/workflow-runs?view=board&amp;company_id=…`, visible with `manages_company_operations()` (the runs routes' `ui.company`). Both runs views render with `UiSection::Runs`.
- NBV-P04-D3: `Icon::Project`, the Octicons 16px `project` path, fetched from unpkg @primer/octicons@19.15.0 build/svg/project-16.svg (the icon module's other glyphs come from the same 16px set).
- NBV-P04-D4: Dashboard: the job panel is "Workflow step jobs" with "Each job is one step of a run." and, on a company-scoped page for a company the caller can open (the existing `linked_company` rule), a "Runs by state" link to the board; "Outstanding workflow step jobs" likewise. `OutstandingTask.run_id` comes from `LEFT JOIN workflow_executions ON (company_id, id) = (task.company_id, task.workflow_execution_id)` (PK, ≤20 rows). `outstanding_row` links a thread when the job has one, else its run via `run_href`, only where `openable`; the run page re-checks company and channel access. The stale "no page until the run page (p09)" comment is replaced.

## Checks
- fmt clean; clippy -D warnings exit 0; build:css + check:css exit 0.
- `icon_rail_` 2/2 (extended: exact hrefs of every existing destination for owner and admin; Runs with view=board and company_id for owner/admin, absent for a member; Runs lit with aria-current on the list and board pages, Workflows unlit; Runs unlit on Workflows).
- `dashboard` 49/49 DB-backed incl. new: `the_job_panel_counts_step_jobs_and_links_the_board`, `a_job_with_no_thread_links_to_its_run` (replaces the p09-era "not a link" test; an execution-less job stays plain), the cannot-open test also asserts no run link, and `workflow_job_projection_dashboard_outstanding_jobs_name_their_run` (persistence, company and global scope).
- `pages::` 176/176; `adapters::http::routes::` 232/232 (logs/b1-*.log, dashboard.log).

## Status
Accepted in phase: digest fc4955c759c6af2f6b146644847630ab7b43416ea8806ba0bbbfa2a88f61cf57 (tree 7a120c2817ecca236772a033beb3e9d69a6f735c), B PASS NBV-P04-B1-R1. Optional F1 (record the glyph's source in code) carried into p05. Not integrated (uncommitted).
