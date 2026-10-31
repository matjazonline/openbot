# NBV-P01 agent review notes

## NBV-P01-B1-R1 — batch review, round 1

### Identity

| Item | Value |
| --- | --- |
| Date | 2026-10-31 |
| Workflow | `implement-opus-reviewed-2_2` |
| Records | task [TASK.md](TASK.md); review [REVIEW.md](REVIEW.md); findings [agent-review-findings.md](agent-review-findings.md) |
| Batch / stage | B1 (whole phase 01), batch review |
| Reviewer | B, Opus 5.5 / xhigh |
| Phase | NBV-P01 |
| Worktree | `/Users/mac02/.codex/worktrees/b109/openbot`, detached HEAD |
| Commits | phase start `92e8c35`; review base `92e8c35`; HEAD `92e8c35e67dfb71e73db87895301dee74f380898`; all changes are uncommitted working-tree changes |
| Digest | `67d63ee9d7b1483f1ee27c0c2d41d826452625f302789cef86aefa7b979de666` |
| Digest exclusions | `plan/new-board-view/phases/p01/docs`, `plan/new-board-view/COORDINATION.md` |
| Gated decisions | none (no architecture gate) |
| Author decisions reviewed | NBV-P01-D1..D6 (TASK.md "Decisions") |

The digest was confirmed with `snapshot.sh` before the review and again after the findings were written, with the
same exclusions. It was unchanged.

**Coordination.** Source is `plan/new-board-view/COORDINATION.md` revision 1, with NBV-I1 r1 (no backward
compatibility) and no cross-phase notices. Rechecked before returning: still revision 1, NBV-I1 r1 satisfied. The
batch adds no data migration or compatibility path. The plan files are already listed in
`NO_BACKWARD_COMPATIBILITY_NOTES` (`src/main.rs:175-180`).

**Requirements read.** Read in full:
- `plan/new-board-view/README.md`, `01-board-view-and-layout.md`, and
  `plan/.archives/fix-03-run-board-finished-columns/02-bounded-board-read.md`.
- Phases 02–05, only to check that 01 does not block them.
- `AGENTS.md`, `src/AGENTS.md`, `src/adapters/http/AGENTS.md`, `src/adapters/http/pages/AGENTS.md`.

### Coverage and evidence

**Code read in full:**
- New `pages/workflow_runs/board.rs` and `routes/workflows/ui/runs/board_view_tests.rs`.
- The whole diff of `routes/workflows/ui/runs/mod.rs`, `pages/workflow_runs/mod.rs`, `pages/dashboard.rs`,
  `runs_tests.rs` and `assets/app.css`. The CSS was checked for the new utilities and with `check:css`.

**Callers and contracts traced:**
- `visible_board`, `visible_channels` and the board JSON route (`routes/workflows/board.rs`).
- `assemble_columns`, `BoardWindow`, `RunBoardScope` and `BOARD_COLUMN_CAP` (`application/workflow/board.rs`).
- `visible_binding` (`bindings.rs:153`).
- `Ui::company`, `Ui::frame` and `load_managed_company` (`routes/ui.rs:182`).
- The pane route's `HX-Trigger-After-Settle`, `run_pane` / `run_pane_attrs` / `#run-selected` (`detail.rs`).
- The list's leaf and form protocol.
- `WORKFLOW_RUNS_SCRIPT`'s guard and its consumers (`workflow-runs.mjs`, `workflow-decisions.mjs`).
- The shell's compact layout and phone-pane script (`mailbox.rs:398-418`, `:1300-1386`), `BUTTON_BUSY_SCRIPT`,
  the skeleton behaviour, and the htmx 2.0.4 sync, abort, select and history-snapshot paths in the shipped
  minified build.

**Requirements checked against code.** Plan 01 steps 1–8 and the acceptance list:

| Plan 01 item | Status |
| --- | --- |
| Step 1: `view` parse / list fallback; `part=columns` region; manager-only authorization | met |
| Step 2: board read with per-request `as_of`; titles | met |
| Step 3: SSE `<main>` mounted, columns side by side, desktop pane placement | met; phone layout broken (F1) |
| Step 4: column header, window line, cut line and its scope link | met |
| Step 5: card | met, except rollup wording follows `ChildCounts` (F7) |
| Step 6: escaping | met; titles escaped in text and attribute, no binding or agent labels rendered in 01 |
| Step 7: visible progress | not met (F2) |
| Step 8: CSS regenerated | met |
| Step 8: theme computed check | deferred (F5) |

**Assumptions challenged:**
- D1: the region that swaps, and the leaf's render-time URL. Leads to F3.
- D2: the empty placeholder versus the phone layout. Leads to F1.
- D4: `aria-busy` alone counted as visible progress. Leads to F2.
- D3: authorization ordering. The full page uses `frame` (managers); `part=columns` uses `company`, so a foreign
  company or a plain member gets NotFound; the binding goes through `visible_binding`; channel visibility is
  inside `visible_board`. Fine.
- D5/D6: list unchanged except for the toggle; fallbacks for unknown `view`, `window` and `part`. Fine.

**Failure paths examined:**
- A foreign binding on both the full page and the part (NotFound, tested).
- A plain member or stranger on both the full page and the part (tested).
- An aborted, failed, or superseded request clearing the busy mark: htmx fires `afterRequest` on abort and error.
- A Back restore. Leads to F2's second defect.
- A late card response into a detached pane. Leads to F4.

**Checks run by the reviewer (tested behaviour):**

1. `SQLX_OFFLINE=true TEST_DATABASE_URL=postgres://mac02@localhost:5432/mail_agents_run_board cargo test --locked --offline --lib routes::workflows::ui::runs:: -- --test-threads=4`
   - 19 passed, including all 5 `run_board_view_*` tests, exit 0, DB-backed. Log: `logs/review-b1-runs-suite.log`.
2. `npm run check:css`
   - exit 0. Log: `logs/review-b1-check-css.log`.
3. `CHROME_PATH=… node --test scripts/tests/workflow-runs.mjs scripts/tests/workflow-decisions.mjs`
   - `workflow-runs.mjs` passed 3/3.
   - `workflow-decisions.mjs` failed only because `WORKFLOW_DECISION_BROWSER_FIXTURES` was unset. That is a harness
     precondition, not a regression. Log: `logs/review-b1-node.log`.
   - Rerun as in CI: generated the fixtures with `cargo test --lib decision_browser_markup_and_closed_draft_recovery`
     and `source_message_outside_window_is_exact_and_authorized` (both passed), then the node test passed 1/1. Log:
     `logs/review-b1-decisions-node.log`.
   - Reason for running it: `WORKFLOW_RUNS_SCRIPT` changed, and the author ran only `workflow-runs.mjs`.
4. Reviewer scratch browser harnesses (shipped CSS, htmx, run script and shell consts; markup rebuilt from the
   `board.rs` format strings; scripts kept in the reviewer scratchpad, not the repository):
   - `logs/review-b1-board-layout-harness.log`:
     - desktop pane and board widths: OK;
     - phone first-click defect: F1;
     - busy mark set and cleared on a card click;
     - busy mark stuck after Back: F2;
     - theme computed values in dark and light: F5 mitigation.
   - `logs/review-b1-board-race-harness.log`: a leaf aborting a window change (F3).
   - `logs/review-b1-busy-visual-harness.log`: `aria-busy` is pixel-invisible (F2).
   - `logs/review-b1-board-scroll-harness.log`:
     - desktop: a 50-card column scrolls inside itself and the six columns scroll sideways. OK.
     - phone: the column overflows the viewport. The harness's reconstruction of the *list* workspace overflows the
       same way, so the harness probably lacks part of the shell. Not attributed to this batch and not a finding;
       it remains an unverified point for the 05 browser pass.

**Inspection only, not tested:**
- F4 (guard scope).
- Copying the stale-scope pattern into the list leaf (pre-existing, out of scope).
- Toggle and sidebar header fit in the 18 rem list sidebar.
- The real served page in both themes through the toggle (F5).
- The age and rollup labels (F7).
- That a hostile title is not reflected anywhere else: `<title>` uses the company name as the list does.

**Unavailable or not run:**
- A served-page browser pass (no server run; the harnesses stand in).
- The full suite and `scripts/check.sh`: out of scope per SKILL.md and the assignment.
- The `run_board_read_*` suite: the read is unchanged in this batch.

**Lightly reviewed:**
- `assets/app.css`: minified output, checked only by `check:css` and the presence of the new utilities.
- `dashboard.rs`: a one-line visibility change.

### Outcome

**Changes required.**
- Actionable: [NBV-P01-B1-F1](agent-review-findings.md) (Medium, phone layout),
  [F2](agent-review-findings.md) (Medium, invisible and stale busy mark),
  [F3](agent-review-findings.md) (Medium, render-time leaf scope overrides a window change; no out-of-order test),
  [F4](agent-review-findings.md) (Low, guard misses card requesters),
  [F5](agent-review-findings.md) (Low, deferred theme check needs a disposition).
- Optional: [F6](agent-review-findings.md) (pane links push the list URL),
  [F7](agent-review-findings.md) (age and rollup untested; "started" for queued runs),
  [F8](agent-review-findings.md) (ARIA tab roles on links).

**Unresolved questions for root:**
- Whether F3 and F4 are fixed now or carried into 03 with an explicit disposition. F3 has to be addressed
  before 03 adds SSE triggers to the leaf.
- Whether F5 is satisfied by the deferral to 05.

**Blocking later phases:**
- F3 would make 03's live reloads override 02's filter and window changes.
- Nothing in 01 blocks 02 (`RunFilterView::query` is already the single scope builder used by the toggle, the cut
  link, the card links and the leaf) or 04 (both views set `UiSection` in one place each).

### Process and instruction feedback

**Batch size:** right for one thorough review (about 750 changed lines in 7 files, one subsystem). No architecture
gate was needed: no schema, transaction or authorization-model change.

**Author records:** TASK.md decisions D1–D6 were concise and useful for targeting the review. The "Checks" section
was honest about the deferred theme check and the environment notes.

**What was missing:** any client-side verification of the new partial-page protocol and layout. All three Medium
findings (F1, F2, F3) were invisible to the server-render route tests the plan prescribes, and each was found only
by a browser harness built from the shipped assets.

**Candidate instructions** (evidence F1–F3; no instruction edited):

1. `pages/AGENTS.md`: "A new workspace or pane placement opts into the compact layout (`ui-pane-list` /
   `ui-pane-detail` / `data-pane-empty`) and is checked at a phone width."
2. `pages/AGENTS.md` progress section: "`aria-busy` is not visible on its own; pair it with a visible indicator
   (skeleton or spinner) and keep busy marks out of htmx history snapshots."
3. `http/AGENTS.md` race-safe section: "A reload leaf sharing a replace group with a filter control reads the
   control's current values, not a URL fixed at render; cover the wake-up-during-filter-change order in a browser
   test."
4. Plan template for UI phases: "Acceptance lists for a new htmx region include a browser protocol check (shipped
   htmx and scripts), not only route tests."

These are defects in the implementation and its prescribed verification, not in a gated decision.

## NBV-P01-B1-R2 — correction recheck, round 2

### Identity

| Item | Value |
| --- | --- |
| Date | 2026-10-31 |
| Workflow | `implement-opus-reviewed-2_2` |
| Records | task [TASK.md](TASK.md) ("Corrections for R1", "Checks", NBV-P01-D7); review [REVIEW.md](REVIEW.md); findings [agent-review-findings.md](agent-review-findings.md) |
| Batch / stage | B1, correction recheck |
| Reviewer | B, Opus 5.5 / xhigh |
| Phase | NBV-P01 |
| Worktree | `/Users/mac02/.codex/worktrees/b109/openbot` |
| Commits | review base `92e8c35`; HEAD `92e8c35e67dfb71e73db87895301dee74f380898`; all changes uncommitted |
| Digest | `158ecfe951d14a9b901725d24e1427e0fc4a2341985e84d2cdf5cb1d0be171da` |
| Digest exclusions | as in R1 |
| Gated decisions | none |
| Author decisions | new: NBV-P01-D7 (F5 disposition) |

The digest was confirmed at the start and again before returning; it was unchanged. Coordination was rechecked:
still r1, NBV-I1 r1, no notices.

### Coverage and evidence

**Correction delta read in full:**
- `pages/workflow_runs/board.rs`: layout classes, empty pane, leaf URL and include, busy utilities, "created",
  `<nav>` switch.
- `pages/workflow_runs/mod.rs`: busy handler, `beforeHistorySave` clearing, guard selector.
- New and changed assertions in `board_view_tests.rs`.
- The two board tests appended to `scripts/tests/workflow-runs.mjs`.
- `assets/app.css`: confirmed `max-md:contents`, `md:*` widths and `has`, `aria-busy:*` and `transition-opacity`
  are compiled, and that `max-md:contents` comes after `.hidden`.
- `routes/workflows/ui/runs/mod.rs`, `dashboard.rs` and `runs_tests.rs`: semantically unchanged since R1. That
  R1 evidence is reused.

**Checks run by the reviewer:**

1. `SQLX_OFFLINE=true TEST_DATABASE_URL=postgres://mac02@localhost:5432/mail_agents_run_board cargo test --locked --offline --lib routes::workflows::ui::runs:: -- --test-threads=4`
   - 19 passed, including 5 `run_board_view_*`, exit 0. Log: `logs/review-b1-r2-runs-suite.log`.
2. `CHROME_PATH=… node --test scripts/tests/workflow-runs.mjs`
   - 5/5 passed, including the two new board protocol tests. Log: `logs/review-b1-r2-workflow-runs-node.log`.
3. `cargo fmt --check` and `npm run check:css`
   - both exit 0. Log: `logs/review-b1-r2-fmt-css.log`.
4. Reviewer harnesses rebuilt from the new format strings (scripts in the scratchpad, as in R1):
   - `board-layout-r2.mjs` → `logs/review-b1-r2-board-layout-harness.log`:
     - F1 on phone and desktop;
     - F2: visible busy state and a clean history snapshot;
     - both themes;
     - the F9 discovery.
   - `board-race-r2.mjs` → `logs/review-b1-r2-board-race-harness.log` (F3).
   - `board-scroll-r2.mjs` → `logs/review-b1-r2-board-scroll-harness.log`: the R1 phone vertical-overflow note
     is unchanged, and the list reconstruction overflows identically. It stays unattributed to this batch and
     unverified for 05.

**Author evidence inspected, not rerun:**
- `logs/a-layout-fixed.log`: consistent with my harness.
- `logs/workflow-decisions.log`: 1/1 with fixtures, consistent with my R1 rerun. The guard and busy handler are
  inert for decision forms.
- The author's claimed mutation check (the race test fails against the old leaf). Not rerun. The Rust test pins
  the leaf attributes that the node test's markup mirrors.

### Outcome

**PASS** for digest `158ecfe951d14a9b901725d24e1427e0fc4a2341985e84d2cdf5cb1d0be171da`.

| Finding | Outcome |
| --- | --- |
| F1, F2, F3, F4 | fixed and verified |
| F5 | NBV-P01-D7 deferral accepted; stays an unverified item for the 05 browser pass, not claimed for P01 |
| F6 | optional, not taken |
| F7, F8 | optional, taken |
| F9 (new) | optional and pre-existing; recorded, not blocking |

**Forward notes:**
- 02: any scope control added to the toolbar must also be in the leaf's `hx-include`, or the leaf should include
  the form.
- 03: F9.
- 05: the served-page theme check (F5) and the phone vertical overflow.

**Reused or not covered:** R1 coverage of authorization, the board read, escaping and the route handler is reused
unchanged. No served-page browser pass was run.

### Process and instruction feedback

**The correction round was efficient.** All actionable findings were fixed in one round, with focused browser
tests added to the CI-run `workflow-runs.mjs`. The reviewer's scratch harness was reused by the author, which was
useful.

**New candidate instruction (F9):** document that `hx-trigger="sse:…"` reloads carry an xhr in the shell's
phone-pane handler comment, and treat only reader-initiated swaps as navigation. Scope: the `mailbox.rs`
compact-layout comment and the pages AGENTS compact-layout guidance.
