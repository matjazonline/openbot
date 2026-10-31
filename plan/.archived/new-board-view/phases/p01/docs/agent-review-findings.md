# NBV-P01 agent review findings

Common context for every finding below unless stated otherwise: workflow `implement-opus-reviewed-2_2`;
task record [TASK.md](TASK.md), review record [REVIEW.md](REVIEW.md); batch B1 (whole phase 01);
stage batch review; review round [NBV-P01-B1-R1](agent-review-notes.md#nbv-p01-b1-r1--batch-review-round-1);
reviewer B Opus 5.5/xhigh; phase NBV-P01; worktree `/Users/mac02/.codex/worktrees/b109/openbot`;
reviewed HEAD `92e8c35e67dfb71e73db87895301dee74f380898` plus working tree, digest
`67d63ee9d7b1483f1ee27c0c2d41d826452625f302789cef86aefa7b979de666` (exclusions
`plan/new-board-view/phases/p01/docs`, `plan/new-board-view/COORDINATION.md`); date 2026-10-31.

Scratch harnesses cited below live in the reviewer's scratchpad (not in the repository):
`/private/tmp/claude-501/-Users-mac02--codex-worktrees-b109-openbot/147507f8-dc5b-4026-9dc4-af253abbbdfc/scratchpad/harness/`
(`board-layout.mjs`, `board-race.mjs`, `board-busy-visual.mjs`, `board-scroll.mjs`). Each one rebuilds the board's markup
from the format strings in `src/adapters/http/pages/workflow_runs/board.rs` with the shipped `assets/app.css`,
`assets/htmx-2.0.4.min.js`, `WORKFLOW_RUNS_SCRIPT`, and the shell's style consts and phone-pane script
copied out of `src/adapters/http/pages/mailbox.rs`. Their output is in `logs/`.

---

## NBV-P01-B1-F1 — On a phone, the first card click opens nothing, and the pane never gets the screen

- **Classification:** actionable defect · **Medium** · requirements/correctness (layout)
- **Evidence:**
  - `runs_board_page` (`src/adapters/http/pages/workflow_runs/board.rs:302-319`) renders the board without the
    shell's compact-layout column classes and, with no run open, an unclassed placeholder
    `<div id="run-pane"></div>` (`board.rs:310`) inside a wrapper that shows only once it holds a `<section>`
    (`board.rs:307`).
  - The shell's phone layout (`src/adapters/http/pages/mailbox.rs:1300-1312`, `:1366-1374`) shows
    `.ui-pane-detail` only while `body[data-pane="detail"]`. It sets that attribute in the `htmx:beforeSwap` handler
    only when the swap *target* is inside `.ui-pane-detail` (`mailbox.rs:410-418`), and `syncMobilePane`
    (`mailbox.rs:398-401`) sets `list` on a board load because the page has no `.ui-pane-detail`.
  - So the first card click swaps the run pane (`<section id="run-pane" class="ui-pane-detail …">`, `detail.rs:139`)
    into a target outside `.ui-pane-detail`, `data-pane` stays `list`, and the new pane is `display:none`. The
    wrapper still becomes `flex` and takes `max-w-[50%]`, so the board shrinks to half the screen beside an empty
    column. Only the second click flips `data-pane` to `detail`, and then the pane gets 50% of a 390 px screen
    beside the board, not the screen. The board is not `ui-pane-list`, so the shell's back control
    (`.ui-mobile-back`) hides the pane but leaves the empty half-width wrapper.
  - Reproduced (`logs/review-b1-board-layout-harness.log`, `board-layout.mjs`): at 390×844, after the first click
    the pane reports `display:"none"`, wrapper 195 px, board 195 px, `data-pane:"list"`. After the second click the
    pane is `flex`, 194 px. At 1400×900 the same markup works: 575 px pane, 824 px board.
  - Required: plan 01 step 3, a board whose columns scroll horizontally "when narrow", with the pane beside it when
    a card is chosen. The shell's contract is that a workspace "opts in by naming its columns" (`mailbox.rs:1309`).
    The list workspace does this with `ui-pane-list`/`ui-pane-detail`.
- **Impact:** the board is unusable for opening runs on phone-width windows. A click gives no visible result and
  no error.
- **Suggested direction (not prescriptive):** take part in the compact layout. For example, mark the board column
  `ui-pane-list` and the pane wrapper or placeholder `ui-pane-detail` with `data-pane-empty` when empty. Keep the
  desktop "takes no width when empty" behaviour. Add a phone-width browser check with the fix.
- **Instruction lesson:** observation: the route tests in plan 01's acceptance list check server markup only, and
  `pages/AGENTS.md` asks only for theme checks. Neither mentions the compact layout's opt-in classes for a new
  workspace shape. Candidate (pages AGENTS, scoped to new workspaces/panes): "A new workspace or pane placement must
  opt into the compact layout (`ui-pane-list` / `ui-pane-detail` / `data-pane-empty`) and be checked at a phone
  width (<48rem)."
- **Resolution history:**
  - 2026-10-31 R1: open.
  - 2026-10-31 R2 ([NBV-P01-B1-R2](agent-review-notes.md), digest `158ecfe951d14a9b901725d24e1427e0fc4a2341985e84d2cdf5cb1d0be171da`, HEAD `92e8c35`): **fixed**.
    - Changes: `#run-board` carries `ui-pane-list` (`board.rs:269`). The empty pane is
      `<div id="run-pane" class="ui-pane-detail" data-pane-empty>` (`board.rs:292-294`). The wrapper is
      `hidden … max-md:contents md:w-[36rem] md:max-w-[50%] md:has-[>section]:flex` (`board.rs:314`). All the new
      utilities are compiled in `assets/app.css`, and `max-md:contents` comes after `.hidden`.
    - Reviewer's own harness, rebuilt from the new format strings (`board-layout-r2.mjs`,
      `logs/review-b1-r2-board-layout-harness.log`):
      - phone 390×844: the first click shows the pane at 390 px with `data-pane:"detail"` and the board hidden;
        pane-back shows the board at 390 px with the pane hidden; the second click shows the pane again;
      - desktop 1400×900: pane 575 px beside an 824 px board; with no run open, the board takes the full width;
      - no page errors.
    - Route test pins the new classes (`board_view_tests.rs:119-123`). A related pre-existing phone behaviour is
      logged separately as F9.

## NBV-P01-B1-F2 — The board's busy feedback cannot be seen, and a stale busy mark survives in history

- **Classification:** actionable defect · **Medium** · requirements (pages AGENTS "visible progress")
- **Evidence:**
  - Plan 01 step 7 ("Visible progress (pages AGENTS)") and `src/adapters/http/pages/AGENTS.md` ("Every user action
    needs visible progress feedback … the UI must visibly acknowledge it") require visible feedback. In the AGENTS
    rule, `aria-busy` is one part of that feedback, not all of it.
  - The new handler (`src/adapters/http/pages/workflow_runs/mod.rs:54-64`, decision NBV-P01-D4) only sets and
    clears `aria-busy="true"` on `#run-board` or `#run-pane`. Nothing styles `[aria-busy]`: there is no match in
    `assets/app.css` or in the shell style consts. `BUTTON_BUSY_SCRIPT` (`mailbox.rs:710-776`) covers only buttons,
    and the window form has none. The card is an `<a>`. Neither region uses the shell's `data-skeleton` placeholders
    (`skeleton.rs:242-310`), which already handle region busy state.
  - Reproduced (`logs/review-b1-busy-visual-harness.log`, `board-busy-visual.mjs`): screenshots of `#run-board` and
    `#run-pane` with and without `aria-busy="true"` are pixel-identical, and opacity, cursor and filter do not change.
    On the first card click the marked element is the placeholder inside a `display:none` wrapper. That is hidden
    from assistive technology as well, so the click gives no feedback of any kind.
  - Second defect from the same mechanism: htmx saves the history snapshot before the swap, while the pane is still
    marked (`if(u.type){zt()}` in `htmx-2.0.4.min.js`), and its snapshot cleaner strips only htmx's own classes. A
    card click therefore stores `aria-busy="true"` in the previous entry's snapshot, and Back restores a pane marked
    busy that nothing clears. Reproduced in `logs/review-b1-board-layout-harness.log`
    ("history: after back … busy: true").
- **Impact:** a slow board read or pane read looks like an ignored click or select change. After Back, assistive
  technology may treat the restored pane as permanently busy. Any visible style added later on `[aria-busy]` would
  also show up stuck.
- **Suggested direction:** make the in-flight state visible, for example with a skeleton via `data-skeleton`, which
  already handles `ui-pane-*` identity, or a local spinner or dim on the region. Clear it, or leave it out of the
  snapshot (for example by clearing it on `htmx:beforeHistorySave`), so a restored page is never marked busy. Check
  it in a browser.
- **Instruction lesson:** the plan wording ("marks … `aria-busy`") can be met literally without anything visible.
  Candidate (pages AGENTS, progress section): "`aria-busy` alone is not visible; pair it with a visible indicator,
  and keep busy marks out of htmx history snapshots."
- **Resolution history:**
  - 2026-10-31 R1: open.
  - 2026-10-31 R2 ([NBV-P01-B1-R2](agent-review-notes.md), digest `158ecfe951d14a9b901725d24e1427e0fc4a2341985e84d2cdf5cb1d0be171da`, HEAD `92e8c35`): **fixed**.
    - Changes: the handler marks the requester and its region and clears both on `afterRequest`, aborts and
      failures included. It also clears every mark on `htmx:beforeHistorySave` (`pages/workflow_runs/mod.rs:54-77`).
      Cards get `aria-busy:opacity-60 aria-busy:cursor-progress` (`board.rs:140`) and `#run-board` gets
      `transition-opacity aria-busy:opacity-60` (`board.rs:269`); both are compiled.
    - Reviewer harness: while a card's answer is held, card opacity is `0.6` and cursor `progress`; marks are gone
      after the answer; after Back the restored pane has no `aria-busy`.
    - Node test (`workflow-runs.mjs`, "opening a run while a window read is in flight…") asserts the busy marks and
      that none remain after Back.
    - Residual note, not a defect: clearing all marks on a history save also removes the dim of another request
      still in flight, for example a window read when a card push happens. That is cosmetic and brief.

## NBV-P01-B1-F3 — The reload leaf re-reads the scope fixed at render time and can override a window change

- **Classification:** actionable defect · **Medium** · correctness (partial-page race), verification
- **Evidence:**
  - `run_board` (`board.rs:267-285`) has two readers in one replace group on `#run-board`: the window form
    (`hx-sync="#run-board:replace"`, current `window`) and the `run-selected` leaf. The leaf's
    `hx-get="{href}&part=columns"` bakes the scope and window from render time (`RunBoardView::href`,
    `board.rs:72-78`). Under `replace`, the later request aborts the earlier one: htmx triggers `htmx:abort` on the
    sync element and the body listener aborts that element's `xhr`.
  - Sequence: the reader changes the window, then opens a card before the board read answers. The pane's
    `HX-Trigger-After-Settle: run-selected` (`routes/workflows/ui/runs/mod.rs:514-520`) fires the leaf, which aborts
    the window read and swaps in columns for the *old* window, while the select shows the new one. The busy mark is
    cleared by the aborted request's `afterRequest`, so nothing signals a problem.
  - Reproduced (`logs/review-b1-board-race-harness.log`, `board-race.mjs`, shipped htmx and the run script):
    `{"reads":["day","week"],"aborts":["/ui/workflow-runs"],"select":"day","columns":"week"}`.
  - Required: `src/adapters/http/AGENTS.md` ("Make partial-page updates race-safe"): replace groups so "the last
    user intent wins", and tests covering "two reads completing out of order whenever a new live or partial-page
    update protocol is introduced". This batch introduces `part=columns` with the leaf. No such test was added; the
    existing `workflow-runs.mjs` does not exercise `#run-board`.
  - Forward impact: plan 03 step 2 puts `sse:runs`, `sse:resync` and `sse:tick` on this same leaf "with the current
    filters". With the render-time URL, any wake-up during a window change, and later during a 02 filter change,
    restores the old scope. That makes the race routine in 03.
  - Context, out of scope here: the list's reload leaf has the same fixed-at-render shape
    (`pages/workflow_runs/mod.rs:255-259`). It is pre-existing and not counted against this batch.
- **Suggested direction:** have the leaf take the toolbar's current values instead of render-time ones, for example
  with `hx-include` on the toolbar form plus `#run-selected` and a URL without scope. Whichever read wins then reads
  what the select shows. Add the browser out-of-order test the AGENTS rule asks for. Alternatively, record an
  explicit agreed deferral to 03, naming this exact sequence among 03's browser checks; 03 currently lists only the
  opposite order, "a filter change while a wake-up read is in flight".
- **Instruction lesson:** candidate (http AGENTS, race-safe section): "A reload leaf in a replace group shared with
  a filter control must read the control's current values, not a URL fixed at render." Evidence: this finding.
  Cause of the miss is otherwise unknown; the author copied the list's established pattern.
- **Resolution history:**
  - 2026-10-31 R1: open.
  - 2026-10-31 R2 ([NBV-P01-B1-R2](agent-review-notes.md), digest `158ecfe951d14a9b901725d24e1427e0fc4a2341985e84d2cdf5cb1d0be171da`, HEAD `92e8c35`): **fixed**.
    - Change: the leaf's URL is `/ui/workflow-runs?{scope}&view=board&part=columns` with no window, and it adds
      `#run-board-window`, the select, to `hx-include` (`board.rs:270-281`). The route test pins those attributes
      and that the leaf names no window (`board_view_tests.rs:335-349`).
    - Reviewer's race harness on the new leaf (`board-race-r2.mjs`, `logs/review-b1-r2-board-race-harness.log`):
      `{"reads":["day","day"],…,"select":"day","columns":"day"}`. In R1 the columns showed `week`.
    - New node tests pass 5/5 (`logs/review-b1-r2-workflow-runs-node.log`): "two window reads completing out of
      order show the later choice" and "opening a run while a window read is in flight keeps the chosen window".
    - Forward note for 02: the leaf still bakes the binding scope into its URL. That is fine while binding has no
      toolbar control, but any scope control 02 adds to the toolbar must also be in the leaf's `hx-include`, or
      the leaf should include the form.

## NBV-P01-B1-F4 — Card clicks are outside the detached-response guard

- **Classification:** actionable defect · **Low** · correctness (partial-page race)
- **Evidence:**
  - The guard in `WORKFLOW_RUNS_SCRIPT` (`pages/workflow_runs/mod.rs:65-72`) attaches `discardDetachedRunResponse`
    only to requesters inside `#run-pane, #run-list, #workflow-decision`. Board cards are new requesters that
    target `#run-pane` (`board.rs:83-90`), but they sit in `#run-board`, so they are not guarded. List links that do
    the same job are guarded because they sit in `#run-list`.
  - Pane writes target `#run-pane` with no replace group (`detail.rs:328`, `:690`). If a write answer replaces the
    pane while a card read is in flight, the late card answer goes to the detached old pane. That is the case the
    guard's own comment describes (`mod.rs:18-21`): htmx's preserve pass can pull `hx-preserve` elements such as
    step disclosures and the reconcile note (`detail.rs:545-548`, `:689`) out of the live pane when the card is the
    same run.
  - This finding is from inspection plus the documented guard rationale. The existing node test
    "a late body read cannot remove preserved steps…" exercises the guard only for in-pane requesters.
- **Impact:** a narrow race: write in flight, then a card click on the same run. The result is lost disclosures or
  note state in the pane and a dropped click.
- **Suggested direction:** add `#run-board` to the guard's `closest()` selector now. Plan 03 step 4 schedules the
  same selector change for wake-ups. If root defers to 03, record that disposition here.
- **Instruction lesson:** no instruction change supported; the guard's scope is documented, and the miss is a
  new requester location.
- **Resolution history:**
  - 2026-10-31 R1: open.
  - 2026-10-31 R2 ([NBV-P01-B1-R2](agent-review-notes.md), digest `158ecfe951d14a9b901725d24e1427e0fc4a2341985e84d2cdf5cb1d0be171da`, HEAD `92e8c35`): **fixed**.
    - Change: the guard selector is now `#run-pane, #run-list, #run-board, #workflow-decision`
      (`pages/workflow_runs/mod.rs:83`).
    - On a dropped answer, `afterRequest` still fires (`beforeOnLoad` only cuts short the response handler), so
      busy marks still clear.
    - Node tests 5/5 with no page errors.

## NBV-P01-B1-F5 — The light/dark computed-style check required by plan 01 step 8 was deferred without an agreed disposition

- **Classification:** actionable verification gap · **Low** · verification
- **Evidence:** plan 01 step 8 says "Check the light and dark themes on computed values (pages AGENTS)". TASK.md
  "Checks" defers it to the phase 05 browser pass. The reviewer's fixture harness (`board-layout.mjs`,
  `logs/review-b1-board-layout-harness.log`) read computed values under `data-theme="dark"` and `"light"` with the
  shipped CSS and shell token overrides. Column `base-200`, card `base-100`, borders `base-300` and the selected edge
  `primary` all resolve per theme: dark primary takes the `DARK_THEME_BLUES` override, light gets daisyUI's. This
  lowers the risk; no override is added, only token utilities. It does not run the real shell's theme-controller
  `:has()` arm on a served page.
- **Impact:** low; mostly a record-keeping gap. A required check is reported as deferred, not done.
- **Suggested direction:** either run the check on a served page, both themes via the toggle, or have root record
  the deferral to 05 as an agreed disposition, citing this harness evidence.
- **Instruction lesson:** none beyond the existing rule; the author did report the gap honestly.
- **Resolution history:**
  - 2026-10-31 R1: open.
  - 2026-10-31 R2 ([NBV-P01-B1-R2](agent-review-notes.md), digest `158ecfe951d14a9b901725d24e1427e0fc4a2341985e84d2cdf5cb1d0be171da`, HEAD `92e8c35`): **deferred; disposition accepted**. NBV-P01-D7 moves the served-page theme-toggle check to the 05
    browser pass, which already lists "both themes".
    - Reason it is acceptable: the batch adds no theme-token override, only `base-*` and `primary` token utilities
      and theme-neutral `opacity`/`cursor`/`transition`. The `:has()` light-theme arm that `pages/AGENTS.md` warns
      about matters only for overrides.
    - Fixture evidence: computed values resolve per theme in R1 and again in R2 (dark and light lines in
      `logs/review-b1-r2-board-layout-harness.log`).
    - It stays an **unverified** item. P01 acceptance must not claim it, and 05 must record it.

## NBV-P01-B1-F6 — (optional) Links inside the pane push the list URL while on the board

- **Classification:** optional suggestion · Low · UX consistency
- **Evidence:** lineage and child-run links in the pane use `run_pane_attrs` (`detail.rs:62-70`), which pushes
  `/ui/workflow-runs?company_id=…&run_id=…` with no `view=board`. TASK.md notes this as intended. After following one
  on the board, reloading, sharing the URL or opening it in a new tab lands on the list. Plan 01 step 5 covers card
  clicks only, so this is not a violation.
- **Suggested direction:** let the pane know its view, or push the current view's URL.
- **Resolution history:**
  - 2026-10-31 R1: open (optional).
  - 2026-10-31 R2 ([NBV-P01-B1-R2](agent-review-notes.md), digest `158ecfe951d14a9b901725d24e1427e0fc4a2341985e84d2cdf5cb1d0be171da`, HEAD `92e8c35`): **not taken** (optional). Recorded in TASK.md Decisions; no further action required for P01.

## NBV-P01-B1-F7 — (optional) Card age and child rollup are not pinned by route tests; "started" for a queued run

- **Classification:** optional suggestion · Low · verification/wording
- **Evidence:** `board_view_tests.rs` asserts no card's age label (`board.rs:118-122`) and no child rollup
  (`board.rs:92-109`). No fixture has a parent with children or a run with `finished_at`, though the read-level
  `run_board_read_*` tests cover top-level selection. The rollup shows "N child runs · X open · Y failed", while the
  plan's example reads "· 1 waiting". That follows from `ChildCounts` (`reads.rs:273-278`), which has no waiting
  count, and is reasonable. A queued run reads "started 5m ago" although it has not started.
- **Suggested direction:** one route test with a parent and a waiting child, plus a finished run, asserting the
  rollup and "finished … ago". Consider "queued/created … ago" for queued runs.
- **Resolution history:**
  - 2026-10-31 R1: open (optional).
  - 2026-10-31 R2 ([NBV-P01-B1-R2](agent-review-notes.md), digest `158ecfe951d14a9b901725d24e1427e0fc4a2341985e84d2cdf5cb1d0be171da`, HEAD `92e8c35`): **taken**. Open cards read "created …" (`board.rs:124-127`). The route test asserts "· created just now"
    and "· finished just now" (`board_view_tests.rs:126-127`), and that a parent card shows "1 child run" while the
    child has no card of its own (`board_view_tests.rs:351-372`). 19/19 pass (`logs/review-b1-r2-runs-suite.log`).

## NBV-P01-B1-F8 — (optional) The List/Board switch uses ARIA tab roles for navigation links

- **Classification:** optional suggestion · Low · accessibility
- **Evidence:** `runs_view_toggle` (`board.rs:203-230`) renders `<a role="tab" … aria-selected="true">` inside
  `role="tablist"`. These are page links with no tabpanels and no arrow-key handling, and the ARIA tabs pattern
  implies both.
- **Suggested direction:** keep the daisyUI `tabs` look, drop `role="tab"`/`tablist`, and mark the current view with
  `aria-current="page"`.
- **Resolution history:**
  - 2026-10-31 R1: open (optional).
  - 2026-10-31 R2 ([NBV-P01-B1-R2](agent-review-notes.md), digest `158ecfe951d14a9b901725d24e1427e0fc4a2341985e84d2cdf5cb1d0be171da`, HEAD `92e8c35`): **taken**. The switch is a `<nav aria-label="Runs view">` of plain links with `aria-current="page"` on
    the current view, and no tab roles (`board.rs:203-230`).

## NBV-P01-B1-F9 — (optional, pre-existing) On a phone, a pane wake-up drags the reader from the board back to the pane

- **Context:** found in round [NBV-P01-B1-R2](agent-review-notes.md), digest
  `158ecfe951d14a9b901725d24e1427e0fc4a2341985e84d2cdf5cb1d0be171da`. Otherwise as in the common context above.
- **Classification:** optional suggestion · Low · UX/layout. Pre-existing: the list workspace has the same
  behaviour. It is not caused by this batch, but F1's fix brings the board into it.
- **Evidence:**
  - The shell's `htmx:beforeSwap` handler (`mailbox.rs:410-418`) treats any xhr swap whose target is inside
    `.ui-pane-detail` as navigation and calls `setMobilePane('detail')`.
  - Its comment assumes live updates "carry no `xhr`", which holds only for `sse-swap`. The pane's body leaf is an
    `hx-get` triggered by `sse:run-<id>, sse:resync`, plus `sse:tick` every 30 s while running (`detail.rs:162`),
    so its swaps have an xhr.
  - Reviewer harness (`board-layout-r2.mjs`, "phone back" lines in `logs/review-b1-r2-board-layout-harness.log`):
    after pane-back on a phone, one pane wake-up sets `data-pane:"detail"` again, hides the board (0 px) and shows
    the pane (390 px).
  - The list workspace uses the same `run_pane` inside `.ui-pane-detail` beside a `.ui-pane-list`, so it behaves
    the same way. Inferred from identical markup and handler; the reviewer did not reproduce it there.
- **Impact:** with a running run's pane open, a phone reader who returns to the board is pulled back to the pane
  within 30 s or on any wake-up for that run.
- **Suggested direction:** count only reader-initiated requests as navigation in the shell handler, for example by
  skipping swaps whose triggering event is an SSE event or whose target is `#run-body`. Route it to 03 (live updates)
  or a shell fix. Phase 03's board leaf targets `#run-board-columns`, inside `.ui-pane-list`, so it will not add to
  this.
- **Instruction lesson:** candidate: the compact-layout comment (`mailbox.rs:405-409`) should say that
  `hx-trigger="sse:…"` reloads do carry an xhr. Observation, not yet verified on the list page.
- **Resolution history:**
  - 2026-10-31 R2: open (optional, pre-existing; not blocking P01).
