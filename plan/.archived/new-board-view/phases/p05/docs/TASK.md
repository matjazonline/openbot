# NBV-P05 — Verify and release

- Workflow: implement-opus-reviewed-2_2 (skill dir /Users/mac02/.codex/worktrees/b109/openbot/.claude/skills/implement-opus-reviewed-2_2)
- Requirements: plan README + 05-verification-and-release.md (now under plan/.archives/new-board-view/).
- Root = A (Opus 5.5, user-confirmed medium+); B = a02b779b149185911 (Opus 5.5/xhigh). Worktree /Users/mac02/.codex/worktrees/b109/openbot, HEAD 92e8c35, uncommitted.
- Prerequisite: NBV-P04 accepted at fc4955c7… (tree 7a120c2817ecca236772a033beb3e9d69a6f735c = review base).
- Coordination r6 (COORDINATION.md moved with the plan). Applicable: NBV-I1, NBV-I2 (user: archive as 601440f), NBV-N3 (05 owes the served-page theme check and the phone overflow look), NBV-N4 (reset databases: the gate and the pass used fresh scratch databases; the developer DB `mail_agents` was NOT reset — see Status).

## Decisions
- NBV-P05-D1 (step 3, NBV-I2): the plan directory, with all phase records, moved to plan/.archives/new-board-view (`mv`; nothing deleted); its six NO_BACKWARD_COMPATIBILITY_NOTES entries repointed and kept with the other archives; its links to the fix plans changed from `../.archives/fix-…` to `../fix-…` (the fix plans' `../new-board-view/…` links now resolve to the archive). Every listed note file exists and carries the note (checked by script). Records keep the paths they were written with.
- NBV-P05-D2: p04 optional F1 taken: `Icon::Project` names its source file.
- NBV-P05-D3 (step 1): served-page browser pass on a scratch database `mail_agents_nbv_pass` (fresh migrate of the edited init schema): the app served by `target/debug/mail_agents` on :3001 with throwaway JWT secret and credential key, SMTP ingress off; seeded through the app (register/login, onboarding company with an encrypted model connection, `/ui/channels` for the private channel then SQL `access_mode = 'allowlist'`, JSON workflow/binding/run routes) plus SQL for the admin's membership/principal, two agents and moving runs across states. Driven headless (Playwright + Google Chrome) as owner, admin and at 390×844. Scripts and results: logs/browser-pass*.

## Browser pass (step 1) — round 3, logs/browser-pass-results-round-3.json (browser-pass-round-3.txt)
Superseded rounds kept: round 1 (logs/browser-pass-results-round-1.json, the R0 assertion set and the faulty phone check), round 2 (harness count error). Round n uses `seed.running[n]` / `seed.assistantRuns[n]` (logs/browser-pass-seed.json). Totals below are round 3's, after earlier rounds' cancels.
- PASS six columns in order with totals (queued 1, running 1, waiting 3, succeeded 3, failed 57, cancelled 5).
- PASS a column with >50 runs: "Showing 50 of 57"; "open in list" opens the list filtered to failed.
- PASS List/Board toggle keeps the workflow scope both ways.
- PASS workflow, agent and window filters each narrow the board (2 runs for the assistant workflow / agent; finished columns say "the last 24 hours").
- PASS a parent waiting on its child shows "1 child run · 1 open"; the child has no card; a hostile workflow title renders as text.
- PASS the rail: Runs lit on board and list, its href `view=board&company_id`; every other destination's href unchanged.
- PASS single-agent board from the agent pane's "Runs board".
- PASS a card moves live to cancelled after Cancel from the pane (hx-confirm accepted), no page reload.
- NOT EXERCISED a card moving live after Retry: the engine refused every available failed run ("only a failed run whose failed step is safe to repeat"), correctly. Cancel satisfies 05's "cancel or retry"; the wake-up path is covered by the p03 tests.
- PASS Dashboard shows "Workflow step jobs" and "Runs by state" → board.
- NOT EXERCISED a Dashboard job opening its run: no workflow job was outstanding during the pass (the workers drain the queue at once); covered by the p04 page and DB tests.
- PASS themes on computed values (NBV-N3; p01 D7): light column oklch(0.98 0 0) / card oklch(1 0 0), dark oklch(0.2326 0.014 253.1) / oklch(0.2533 0.016 252.42); text and borders differ, none transparent; toggled with the real theme switch.
- PASS admin: runs on the private channel are in neither cards nor totals (succeeded 1 vs owner 3).
- PASS phone 390×844 (NBV-N3 overflow, after the R1 fix): the page stays 844 px; a card opens the pane at 390 px with the board hidden; the back control returns to the 390 px board; the failed column (scrolled into view) scrolls inside itself and its 50th card is reached by the mouse wheel. B's probe (logs/a-phone-overflow-r1.log) also shows the runs list scrolling (540/2923) and opening on the list.
- No page script errors.

## Corrections for R1 (NBV-P05-B1-R1)
- F1: both runs pages' `<main>` gain `min-h-0` (under the shell's phone column it grew to its content, and the body does not scroll). The pass's phone check now fails when the page grows past the viewport or the last failed card cannot be wheeled into view (the R0 check read "scrollHeight not above clientHeight" as fitting — the failing signal).
- F2: the pass records PASS / FAIL / NOT EXERCISED; retry and the Dashboard job link are NOT EXERCISED; every results file is named by its round.
- F3 (optional, taken): `run_empty_pane` carries `data-pane-empty`, so the list opens on the list on a phone.
- Checks: fmt, `git diff HEAD --check`, clippy --all-targets -D warnings clean; check:css exit 0; `routes::workflows::ui::` 72/72, `pages::` 176/176, `run_board_view_` 14/14; workflow-runs + sse-navigation 18/18 (logs/r1-*.log). The changes are two CSS utility classes and one attribute; the full check.sh was not rerun (its last run, on the pre-fix tree, passed every gate).

## Step 2 — tripwires
- `warn_if_run_list_is_slow` (src/adapters/persistence/workflow/reads.rs) and `warn_if_run_board_is_slow` (src/adapters/http/routes/workflows/board.rs) in place, both now with `workflow=`; docs/performance/run-list.md "Revisit when" reads both, including the workflow rows.

## Final gate
- Focused, DB (`--test-threads=4`): run_agent_filter_ 4, run_top_level_ 7, run_finished_at_ 8, run_board_read_ 10, run_board_view_ 14, icon_rail_ 2, dashboard 49, routes::workflows::ui::runs:: 26 — all pass (logs/gate-*.log).
- scripts/check.sh, once, with DATABASE_URL=postgres://mac02@localhost:5432/mail_agents_nbv_check (a fresh scratch DB: the developer DB predates the init-schema edit, NBV-N4), RUST_TEST_THREADS=4 (this machine's Postgres runs out of lock memory at default parallelism), CHROME_PATH=Google Chrome: exit 0 — every gate PASS: generated CSS, SSE navigation lifecycle (sse-navigation + agent-runtime, 7), deployment script tests, formatting, patch whitespace, transport boundary, locked fetch, offline compilation, clippy, migrations, SQLx metadata, compile tests, database-backed suite 2511 passed / 0 failed / 6 ignored, stack budget (logs/check.log).
- CI's browser step also runs workflow-runs, workflow-conversations, workflow-samples (+ workflow-decisions in its own CI step), which check.sh does not: 18/18 on the final tree (logs/gate-node.log).
- Lease budget: last run at p02 R2 (pass, unchanged shares); p03–p05 changed no lease-path SQL (p04 changed only the Dashboard read).
- Query-plan script: run-list and board SQL unchanged since the p02 r2 evidence (only the dashboard outstanding read changed), so not rerun.

## Status
Accepted in phase and as the combined P01–P05 result: digest f7da3b6c896de2515d48ff553804929220e6d512210f4d0f26b8f06d832c612c (tree b122c8feb95ccf6ab1a8cf0e58b580e727056d9b), B PASS NBV-P05-B1-R2. Not integrated: uncommitted (no commit authorization). Cleanup done at completion: the :3001 server stopped; scratch databases mail_agents_nbv_pass, mail_agents_nbv_check, mail_agents_run_board dropped. Left for the user: reset the developer DB `mail_agents` (scripts/reset-db.sh --all) before serving it (NBV-N4).
