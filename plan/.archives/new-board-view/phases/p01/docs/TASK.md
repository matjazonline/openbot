# NBV-P01 — Board view and layout

- Workflow: implement-opus-reviewed-2_2 (skill dir /Users/mac02/.codex/worktrees/b109/openbot/.claude/skills/implement-opus-reviewed-2_2)
- Requirements: plan/new-board-view/README.md, plan/new-board-view/01-board-view-and-layout.md
- Record root: plan/new-board-view/phases/p01/docs/
- Root = Author (A): Opus 5.5, user-confirmed effort medium or above. Reviewer (B): Opus 5.5/xhigh (agent ID recorded at spawn).
- Worktree: /Users/mac02/.codex/worktrees/b109/openbot, detached HEAD; starting commit 92e8c35. No pre-existing changes.
- Coordination: plan/new-board-view/COORDINATION.md r1, NBV-I1 r1 acknowledged.
- Compaction: CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75 verified in env.
- No commit authorization; work stays in the working tree.

## Acceptance criteria
See 01 "Focused checks and acceptance".

## Batches
- B1: whole phase 01.

## Decisions
- NBV-P01-D1: `#run-board` holds the toolbar (view switch, window select) and `#run-board-columns`, mirroring `#run-list` which holds its own filter. A filter/window change swaps `#run-board` outerHTML; the reload leaf (run-selected now, SSE in 03) GETs `part=columns` and swaps only `#run-board-columns` content via `hx-select="#run-board-columns > section"`, keeping horizontal scroll.
- NBV-P01-D2: "#run-pane absent otherwise" is realised as an empty `<div id="run-pane"></div>` inside a pane column that is `hidden` unless it holds a `<section>` (`has-[>section]:flex`), so a card has a swap target and the board takes the full width.
- NBV-P01-D3: The board reuses `routes::workflows::board::visible_board` (counts → visibility → cards → rollup, slow-read tripwire) with `as_of = Utc::now()` per request; binding scope resolved through `visible_binding` (foreign/hidden binding → not found).
- NBV-P01-D4: Busy feedback: elements with `data-busy="<selector>"` mark that region `aria-busy="true"` from htmx:beforeRequest until htmx:afterRequest (WORKFLOW_RUNS_SCRIPT). Used by the window form (#run-board) and cards (#run-pane).
- NBV-P01-D5: The list sidebar header gains the List/Board switch (`runs_view_toggle`); list behaviour is otherwise unchanged, and absent/unknown `view` render identically.
- NBV-P01-D6: Unknown `window` falls back to week; `part` other than `columns` renders the full page. Full-page refusal for a non-manager is the existing no-company frame; `part=columns` refuses with NotFound.
- Card links from inside the pane (lineage, child runs) still push the list URL (`run_pane_attrs`, unchanged).

- NBV-P01-D7 (F5 disposition): the served-page light/dark theme-toggle check moves to the 05 browser pass, which already lists "both themes" on a seeded company. Reason: it needs a running server with seeded runs; B's fixture harness already found every token resolves in both themes (logs/review-b1-*). Stays an open verification item until 05 records it.

## Corrections for R1 (NBV-P01-B1-R1)
- F1: board region is `ui-pane-list`; empty pane is `<div id="run-pane" class="ui-pane-detail" data-pane-empty>`; pane column `hidden max-md:contents md:w-[36rem] md:max-w-[50%] md:has-[>section]:flex`. Verified at 1400x900 and 390x844 with B's layout harness updated to the new markup (logs/a-layout-fixed.log): phone first click → pane 390px, data-pane detail; back → board 390px; desktop pane 575/board 824.
- F2: busy marks both the triggering element and its region, visible via `aria-busy:opacity-60` (+ `cursor-progress` on cards, opacity on #run-board); all marks cleared on `htmx:beforeHistorySave`. Harness: card opacity 0.6 while held; no aria-busy after Back. Browser test asserts the busy marks and none after Back.
- F3: the leaf's URL names no window and includes `#run-board-window`. New browser tests in scripts/tests/workflow-runs.mjs: out-of-order window reads (later wins), card click during an in-flight window read keeps the chosen window. Mutation check: the race test fails against the old leaf (pass 0 / fail 1).
- F4: `#run-board` added to the detached-response guard selector.
- F5: NBV-P01-D7 (deferral to 05).
- F6 (optional): not taken; pane-internal links still push the list URL, recorded under Decisions.
- F7 (optional): queued/open card reads "created …"; route tests now assert ages and a parent card's "1 child run" with the child absent.
- F8 (optional): the switch is `<nav>` with `aria-current="page"`, no tab roles.

## Checks
Environment notes: the DB-backed suite must run with `-- --test-threads=4` here (at default parallelism Postgres fails migrations with 53200 "out of shared memory" / max_locks_per_transaction: logs/runs-suite-1.log; environment, not a regression). Playwright's headless shell is not installed; the node tests run with `CHROME_PATH="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"` (Playwright's chromium-1193 crashes on launch here).
- `SQLX_OFFLINE=true TEST_DATABASE_URL=postgres://mac02@localhost:5432/mail_agents_run_board cargo test --locked --offline --lib run_board_view_` → 5 passed (nonzero, DB-backed).
- `... cargo test --locked --offline --lib routes::workflows::ui::runs:: -- --test-threads=4` → 19 passed (logs/runs-suite-2.log).
- `npm run build:css` (assets/app.css regenerated) and `npm run check:css` → exit 0.
- `CHROME_PATH=... node --test scripts/tests/workflow-runs.mjs` → 3/3 pass (logs/workflow-runs-node.log); WORKFLOW_RUNS_SCRIPT changed (busy handler).
- R1 corrections: `run_board_view_` 5/5; `routes::workflows::ui::runs::` 19/19 (logs/runs-suite-3.log); `npm run build:css` + `check:css` exit 0; `cargo fmt --check` clean; workflow-runs.mjs 5/5 (logs/workflow-runs-node-2.log); workflow-decisions.mjs 1/1 with fixtures generated as CI does (logs/workflow-decisions.log).
- Light/dark computed-style check: deferred to the 05 browser pass (only theme tokens used: bg-base-*, border-base-300, border-primary).

## Status
Accepted in phase: digest 158ecfe951d14a9b901725d24e1427e0fc4a2341985e84d2cdf5cb1d0be171da, B PASS NBV-P01-B1-R2. Not integrated (uncommitted). Open verification: served-page light/dark check (D7, owed by 05). F9 (phone wake-up pulls reader back to pane) routed to p03 via coordination notice NBV-N1.
