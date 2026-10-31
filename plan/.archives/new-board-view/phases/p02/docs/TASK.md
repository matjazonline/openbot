# NBV-P02 — Filters and the ways in

- Workflow: implement-opus-reviewed-2_2 (skill dir /Users/mac02/.codex/worktrees/b109/openbot/.claude/skills/implement-opus-reviewed-2_2)
- Requirements: plan/new-board-view/README.md, plan/new-board-view/02-filters-and-entry-points.md
- Record root: plan/new-board-view/phases/p02/docs/
- Root = Author (A): Opus 5.5, user-confirmed effort medium or above. Reviewer (B): Opus 5.5/xhigh, same agent as p01 (a02b779b149185911).
- Worktree: /Users/mac02/.codex/worktrees/b109/openbot, detached HEAD 92e8c35. Uncommitted.
- Prerequisite: NBV-P01 accepted at digest 158ecfe951d14a9b901725d24e1427e0fc4a2341985e84d2cdf5cb1d0be171da, saved as git tree 85b05025ef161cd04a7e07081f168bb51f649a75 (review base for this phase: compare that tree with the current state).
- Coordination: plan/new-board-view/COORDINATION.md r2. NBV-I1 r1 acknowledged; NBV-N2 r1 (scope controls reach the leaf) applies: satisfied by the leaf including the whole toolbar form (`#run-board-filters`). NBV-N1 (p03), NBV-N3 (p05) not applicable here.
- Environment: DB tests `-- --test-threads=4`; node tests with CHROME_PATH=Google Chrome (see p01 TASK).

## Acceptance criteria
02 "Focused checks and acceptance".

## Batches
- B1: whole phase 02.

## Decisions
- NBV-P02-D1: `RunFilter.workflow` and `RunBoardScope.workflow` (`WorkflowId`): list `AND workflow_id = $n`; board filters the materialized working set (which now selects `workflow_id`). Resolved company-scoped by `bindings::company_workflow` (definition read; another tenant's → NotFound), used by the JSON list, the JSON board and both UI views.
- NBV-P02-D2: UI scope ids (binding, workflow, agent) are resolved in `Ui::scope` for both views: a binding through `visible_binding` (the list now validates it too; before it passed the raw id), workflow and agent as above. Unknown/foreign id → NotFound. `workflow_id`/`agent_id` accept "" as none (the select's "any").
- NBV-P02-D3: `RunFilterView` holds binding/workflow/agent/state and is the one query builder (switch, open in list, Load more, list wake-up). The board's leaf now GETs `/ui/workflow-runs` including `#run-selected, #run-board-filters` (the whole toolbar form) so every control reaches it (NBV-N2).
- NBV-P02-D4: Controls: workflow and agent selects in the board toolbar and the list form (`scope_controls`), options from `list_definitions` (200, `more` → cut) and `list_company_agents` (sorted by name, cut at 200); a cut list says "Only the first N …s are offered."; a chosen id past the bound stays selected. Binding scope remains a hidden field with an "any binding" link. The list region now holds the List/Board switch (moved from the sidebar header) so the switch follows list filter changes. Options are read only for whole-region answers of the list; the board region reads them on each part=columns read (03 bounds the read rate).
- NBV-P02-D5: Entry points: agent "Used by" card → "Runs board" (`view=board&agent_id`); workflow pane header → "Runs board" (`view=board&workflow_id`) — the library row itself is a link to the pane, so the link sits in the pane header; Workflows page "Runs" → `view=board`; binding "runs" link unchanged (list by binding).
- NBV-P02-D6 r2 (index; replaces r1 "defer" after finding NBV-P02-B1-F2): add `workflow_runs_workflow_newest (company_id, workflow_id, created_at DESC, id DESC)` to the init schema in place (NBV-I1). Evidence: script run on a freshly migrated scratch DB with the index dropped (logs/qp-list-before.txt, qp-board-before.txt) and re-created (qp-list-after.txt, qp-board-after.txt; copied to docs/performance/run-*-query-plan.txt). A sparse "no runs" workflow case was added to the list shapes. Generic plans before: every workflow-filtered page walked the tenant's whole `(company_id, id, workflow_id)` unique index (2,594 buffers for a workflow with no runs, ~8,844 for a fortieth); after: a page of the new index (4 / 54 buffers). Remaining: workflow + a state matching few of its runs reads the workflow's range (6,268 buffers, 10.5 ms), the rare-state shape. Index 143 MB / 1.1M runs. Recorded in docs/performance/run-list.md "Workflow filter (new-board-view 02)" with revisit rows.

## Corrections for R1 (NBV-P02-B1-R1)
- F1: list leaf `hx-get="/ui/workflow-runs/list?part=entries"` and "Load more" `list?after=…&part=more` now include `#run-list-filters` (the form, which holds company_id, state, binding, workflow, agent) and name no scope themselves. Browser test `list: a wake-up during a filter read reads the new filter instead of undoing it` (mutation without the form include fails). Route test pins updated (board_view_tests, runs_tests paging test).
- F2: NBV-P02-D6 r2 (index added, before/after evidence).
- F3: the board leaf sends `reload=columns` (hx-vals) and `board_html` then skips `scope_options`; agent options come from new `AuthoringReads::agent_options` (`ORDER BY name, id LIMIT 201` in SQL), no longer `list_company_agents`.
- F4: `run_board_view_filter_workflow_pages_link_to_the_board` (authoring route test) checks the Workflows page "Runs" → board and the pane header "Runs board" → `workflow_id` board.

## Corrections for R2 (NBV-P02-B1-R2)
- F6: docs/performance/run-list.md "Workflow filter": a state change does write the new index (never a HOT update). `scripts/lease-budget.sh` run: exit 0, 130 passed; highest shares budget.reserve 358/1000 ms, lease.renew 311/2000 (calibration 357 / 310) — unchanged (logs/lease-budget.log).
- F5 (optional, taken): `board_html`'s bool replaced by `enum BoardRead { Region, Columns }`.
- F7 (optional, taken): the filter test asserts the agent select offers `assistant` before `idle` by name, before any is chosen.
- Known for 05: the shared test DB mail_agents_run_board predates the init-schema edit; reset it before any `test_pool()` suite / the full gate.

## Checks
- R2 corrections: fmt clean, clippy -D warnings clean, `run_board_view_` 11/11, lease-budget pass.
- R1 corrections: fmt clean; clippy -D warnings exit 0; build:css + check:css exit 0; DB (`--test-threads=4`): `run_board_view_` 11/11, `run_agent_filter_` 4/4, `run_board_read_` 10/10, `run_top_level_` 7/7, `persistence::workflow::` 1011/1011 (+1 ignored), `routes::workflows::` 101/101, `pages::` 175/175 (logs/r1-*.log); workflow-runs.mjs 6/6 (logs/workflow-runs-node-r1.log); fresh `sqlx migrate run` of the edited init schema succeeded (used for the measurement).
- `cargo fmt --check` clean; `cargo clippy --locked --offline --lib --tests -- -D warnings` exit 0 (logs/clippy.log); `npm run build:css` + `check:css` exit 0.
- DB-backed, `-- --test-threads=4`: `run_board_view_` 10/10 (5 new `run_board_view_filter_*`: scopes alone/combined on both views, foreign ids refused, switch/links keep scope + agent pane link, cut option list; plus JSON parity `run_board_view_filter_json_routes_take_a_workflow_of_the_company`), `run_agent_filter_` 4/4, `run_board_read_` 10/10, `run_top_level_` 7/7, `persistence::workflow::reads` 5/5 (+1 ignored dump), `routes::workflows::ui::` 68/68, `routes::workflows::tests::` 31/31, `pages::` 175/175 (logs/b1-*.log).
- `node --test scripts/tests/workflow-runs.mjs` 5/5 (board fixture now uses the form-include leaf and an agent select).

## Status
Accepted in phase: digest d6d1ff4f6c031002d47015c92a1bd859f872ee80f84e813c84e2a1d7d8b7ccb0 (tree d68a8f1b3e86c4082eef8dd718f7a22edaf470e3), B PASS NBV-P02-B1-R3. Not integrated (uncommitted).
