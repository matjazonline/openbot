# NBV-P03 — Keep the board current

- Workflow: implement-opus-reviewed-2_2 (skill dir /Users/mac02/.codex/worktrees/b109/openbot/.claude/skills/implement-opus-reviewed-2_2)
- Requirements: plan/new-board-view/README.md, plan/new-board-view/03-live-updates.md
- Root = A (Opus 5.5, user-confirmed medium+); B = a02b779b149185911 (Opus 5.5/xhigh). Worktree /Users/mac02/.codex/worktrees/b109/openbot, HEAD 92e8c35, uncommitted.
- Prerequisites: NBV-P01 accepted (tree 85b05025…), NBV-P02 accepted at digest d6d1ff4f6c031002d47015c92a1bd859f872ee80f84e813c84e2a1d7d8b7ccb0 (tree d68a8f1b3e86c4082eef8dd718f7a22edaf470e3 = review base). This phase's state: tree b923973964cfed23e37ee025d2c5c34e063179de.
- Coordination: COORDINATION.md r3. NBV-I1 r1 ack. NBV-N1 r1 (phone pull-back on stream-driven pane reloads) applies: fixed here (D3). NBV-N2 stays satisfied (leaf includes the whole toolbar form). NBV-N3/N4 are for p05.
- Environment: DB tests `-- --test-threads=4`; node tests with CHROME_PATH=Google Chrome. Shared test DB reset at p02 acceptance.

## Batches
- B1: whole phase 03.

## Decisions
- NBV-P03-D1: No new stream. The board leaf triggers on `sse:runs, sse:resync, sse:tick, run-selected from:body` (`data-board-wake`), still `hx-sync="#run-board:replace"`, `reload=columns` (no option reads), swapping only the columns. Every read recomputes `as_of` and the cutoff (`board_html` → `board_html_at(…, Utc::now())`).
- NBV-P03-D2 (refresh bound): client-side coalescer in WORKFLOW_RUNS_SCRIPT on `htmx:confirm` for `[data-board-wake]`: a wake-up passes when none was issued in the last 2 s and none is held; otherwise it is held (later ones replace it) and the held one is issued (`issueRequest(true)`) when the 2 s interval ends — so at most one read per 2 s per tab and always one after a burst's last event; a held read whose leaf was replaced (filter change) is dropped (the filter read is fresh, the new leaf takes later events). `run-selected` (reader's action) reads at once and clears the held one. Guarantee stated in the script comment. Per-tab, not per-viewer: a viewer with several tabs gets one bound per tab (as the list's leaf and the open-run snapshot are per tab).
- NBV-P03-D3 (NBV-N1): the shell's phone handler (mailbox.rs htmx:beforeSwap) ignores requests whose triggering event is `sse:*`, matching its own comment that a live stream writing into the pane must not drag the reader to it. Affects every workspace's stream-triggered pane reloads equally (they were the bug).
- Detached-response guard already covers #run-board (p01 F4). The pane keeps its own `run-<id>` wake-ups.

- NBV-P03-D2 r2 (replaces r1 after NBV-P03-B1-F1/F2): completion-based coalescer. Board reads (requests whose target is inside #run-board: the leaf and the filter form; not card pane reads) are tracked per xhr from htmx:beforeRequest to that request's own htmx:afterRequest (listener on the element, matched by xhr — a replaced form's events no longer bubble, and an element with two requests sees the first settle twice); entries older than 30 s stop counting (BOARD_READ_STALE_MS). A wake-up passes only when no board read is in flight, none is held, and 2 s have passed since the last read settled; otherwise it is held. A starting board read clears what is held (it reads after those events' commits). When the last read settles, a held wake-up is replayed after the rest of the interval as `board-wake` on whichever leaf is current (htmx.process first: a just-swapped leaf may be unprocessed). `run-selected` reads at once when nothing is in flight, else right after it settles (no 2 s wait) — so it never aborts a filter read (F2b). Guarantees: no two board reads overlap; ≤1 wake read starts per 2 s after the previous settles; a wake-up after a read began is always followed by a read.
- NBV-P03-D3 r2 (+F3): the shell's phone handler treats a swap as navigation only when a real triggering event exists and it is not `sse:*`. htmx 2.0.4 issues polls (`every …`) and `load` reads with no triggering event (verified in the shipped min.js: the poll handler is called with the element only), so those refreshes no longer pull a phone reader into the pane (e.g. the Schedules runs column's 15 s poll). No page calls htmx.ajax.

## Corrections for R1 (NBV-P03-B1-R1)
- F1, F2(a), F2(b): D2 r2. New browser tests: reads answered after 2.5 s under 8 s of change every 100 ms — none aborted, starts ≥4.4 s apart, the board moves during the change and ends on the final state; a run change during a filter read is read again after it on the new leaf; two quick filter changes do not leave the coalescer waiting. The p01 card-click test now expects the filter read to stay uncancelled and the leaf to read once after it. Mutation: in-flight tracking disabled → the slow-read and filter-snapshot tests fail 2/2.
- F3 (optional, taken): D3 r2; the phone test adds a pane that polls every 300 ms — data-pane stays list (fails with the r1 guard).

## Checks
- R1 corrections: fmt clean, clippy exit 0; workflow-runs.mjs 13/13; sse-navigation + agent-runtime + workflow-conversations + workflow-samples + workflow-decisions 12/12; DB `routes::workflows::ui::` 72/72 (incl. `run_board_view_` 14), `pages::` 175/175 (logs/*-r1*.log, r1-*.log).
- fmt clean; clippy -D warnings exit 0; check:css exit 0.
- DB (`--test-threads=4`): `run_board_view_` 14/14 incl. 3 new `run_board_view_live_*` (change before subscribe reconciled by first resync; hidden channel sends no wake-up + visible control; finished run leaves the week window on a read 8 days later, month keeps it "finished 8d ago"); `routes::workflows::ui::` 72/72; `pages::` 175/175 (logs/b1-*.log).
- Browser (shipped htmx, sse ext, run script): workflow-runs.mjs 10/10 — new: filter change while a wake-up read is in flight wins (late wake answer discarded); a 30-event burst in 1.5 s reads ≤2 times and shows the final state with the last read after the last event; 6 s of constant change reads 3–5 times, ≥1.9 s apart, ending on the final state; phone: a stream-driven pane reload leaves data-pane=list, a click opens. Mutation checks: leaf without data-board-wake → burst/constant tests fail 2/2; coalescer without the trailing issue → fail 2/2; shell without the sse: guard → phone test fails.
- sse-navigation.mjs 5/5, agent-runtime + workflow-conversations + workflow-samples 6/6, workflow-decisions.mjs 1/1 (logs/*.log).

## Status
Accepted in phase: digest 38ee1d9edf0121d05d97eb866c403ef2915da61ede2959c13560bbba39cdef49 (tree f07c076b2242a35b0bd97bf400b1bcc854d04603), B PASS NBV-P03-B1-R2. NBV-N1 resolved. Not integrated (uncommitted).
