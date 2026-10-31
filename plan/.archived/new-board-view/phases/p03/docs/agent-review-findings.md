# NBV-P03 agent review findings

## Common context

These fields apply to every finding below unless a finding says otherwise.

| Field | Value |
| --- | --- |
| Date | 2026-10-31 |
| Workflow | `implement-opus-reviewed-2_2` |
| Task record | [TASK.md](TASK.md) |
| Review record | [REVIEW.md](REVIEW.md) |
| Batch | B1 (whole phase 03) |
| Stage | batch review |
| Review round | [NBV-P03-B1-R1](agent-review-notes.md) |
| Reviewer | B, Opus 5.5/xhigh |
| Phase | NBV-P03 |
| Worktree | `/Users/mac02/.codex/worktrees/b109/openbot` |
| HEAD | `92e8c35e67dfb71e73db87895301dee74f380898` |
| Phase delta | tree `d68a8f1b3e86c4082eef8dd718f7a22edaf470e3` (NBV-P02 accepted) → tree `b923973964cfed23e37ee025d2c5c34e063179de` |
| Digest | `891cc567ad7f8c950b7726deb555fa2bc63d956ca757c2e697595b39be245b26` |
| Digest exclusions | `plan/new-board-view/phases`, `plan/new-board-view/COORDINATION.md` |

**Harness.** `board-coalescer-p03.mjs` is in the reviewer's scratchpad, not in the repository:
`/private/tmp/claude-501/-Users-mac02--codex-worktrees-b109-openbot/147507f8-dc5b-4026-9dc4-af253abbbdfc/scratchpad/harness/`.
It uses:

- the board leaf and the window form exactly as `run_board` renders them in this artifact;
- the shipped `htmx-2.0.4.min.js`, `htmx-ext-sse-2.2.3.js` and `WORKFLOW_RUNS_SCRIPT`, which includes the
  D2 coalescer;
- a real SSE stream.

Each board read snapshots the state when it arrives, as a database read would.

Output: `logs/review-b1-coalescer-harness.log`.

---

## NBV-P03-B1-F1 — Under constant change, board reads slower than 2 s cancel each other and the board freezes

- **Classification:** actionable defect · **Medium** · correctness/liveness of the D2 refresh protocol.

### Evidence

**How the coalescer paces reads.** D2 (`WORKFLOW_RUNS_SCRIPT`, the `htmx:confirm` listener and
`releaseBoardWake`, `src/adapters/http/pages/workflow_runs/mod.rs`) spaces reads by when they *start*:

- `boardWake.last` is set when a read is issued;
- a held wake-up is released `BOARD_WAKE_MS` after that;
- nothing checks whether the previous leaf read has finished.

**Why that cancels reads.** The leaf keeps `hx-sync="#run-board:replace"`. A released read therefore aborts a
leaf read that is still in flight. Once a board read takes 2 s or more, every read is aborted by the next one
for as long as the change continues.

**Reproduced.** Case A of the harness: board reads take 2.5 s, and a run changes every 100 ms for 8 s.

```text
A {"leafReads":6,"aborted":5,"completed":1 (the trailing read, after the change stopped),"shownAtEndOfChange":"0","shownAfterSettling":"80"}
```

Nothing landed during 8 s of change. Only the trailing read after the change stopped landed.

**Required.**

- Plan 03 step 3 says "Coalesce wake-ups … and always read after the last event of a burst", and warns that
  "`delay` can starve under constant change". It also says to follow
  `src/adapters/http/routes/ui_run_counts.rs` "or an equivalent". That model is explicitly a "Completion-based
  throttle, not a debounce": `RefreshScheduler::finished` sets `eligible = now + interval` after the read
  completes.
- D2's own stated guarantee ("never starves", in the script comment and the "constant change" node test)
  holds only while reads finish within the interval.
- The node tests answer instantly, so they cannot see this.

### Impact

Board reads take more than 2 s when the database is loaded, which is when the slow-board tripwire would fire.
Under those conditions:

- the board stops updating for as long as runs keep changing, which can be indefinitely on a busy company;
- each started read is cancelled, wasting server work.

### Suggested direction

Make the interval completion-based:

- while a leaf read is in flight, hold wake-ups;
- start the 2 s interval from that read's `htmx:afterRequest`;
- then issue the held one.

Add a browser case with answers slower than the interval under constant change.

### Instruction lesson

Candidate (http AGENTS, race-safe section): "A client-side refresh coalescer is completion-based, and its tests
include reads slower than its interval. `hx-sync` replace plus a start-based throttle starves."
Evidence: this finding.

### Resolution history

- 2026-10-31 R1: open.

---
- 2026-10-31 R2 ([NBV-P03-B1-R2](agent-review-notes.md); digest `38ee1d9edf0121d05d97eb866c403ef2915da61ede2959c13560bbba39cdef49`; tree `f07c076b`): **fixed** (D2 r2, a completion-based coalescer).
  - **How it now works**
    - Every read whose target is inside `#run-board`, from the leaf or the filter form, is tracked per xhr
      from `htmx:beforeRequest`.
    - A settle listener is attached to that request's own element and matched on the xhr. It records
      `settled` and schedules any held wake-up `BOARD_WAKE_MS` later.
    - While any board read is in flight, wake-ups are held, so reads never overlap.
  - **Reviewer harness** (`board-coalescer-p03-r2.mjs`, `logs/review-r2-coalescer-harness.log`), case A:
    reads take 2.5 s and a change arrives every 100 ms for 8 s. All 3 reads completed and none was aborted;
    the board showed v45 mid-change and v80 at the end. In R1 it was 5 aborted and nothing landed during
    the change.
  - **Author's tests:** the new browser test "reads slower than the interval never overlap…" and the
    mutation check that disables the tracking.

## NBV-P03-B1-F2 — A wake-up held during a filter change is dropped or overrides it

The held wake-up can be dropped while the filter read is in flight, and a released one can cancel the filter
read.

- **Classification:** actionable defect · **Low** · correctness of the D2 guarantee.

### Evidence

**(a) A held wake-up can be dropped although the filter read predates its change.**

D2 drops a held wake-up whose leaf has been replaced (`releaseBoardWake`: `if (!held || !held.elt.isConnected) return;`).
Its rationale is "the filter read is fresh". That holds only when the filter read was *issued after* the held
event arrived.

The failing order:

1. A filter read is issued.
2. The server takes its snapshot.
3. A run changes, and its wake-up arrives inside the 2 s interval, so it is held.
4. The filter read lands and replaces the leaf.
5. The timer fires and drops the held wake-up.

Nothing reads the change until the next event or `tick`, up to 30 s later.

Reproduced as harness case B:

```text
B {"reads":["leaf:v0:week","form:v0:day"],"shown":"0","window":"day","finalState":1}
```

No read followed the held wake-up, and the board stays on v0 while the state is v1. This breaks plan 03 step 3's
"always read after the last event of a burst".

**(b) A released wake-up can cancel a filter read in flight.**

The form shares `#run-board:replace`, so a released wake-up aborts a filter read that is still in flight. The
leaf reads the form's current values (NBV-N2), so the columns are right. The toolbar is not replaced, though,
and keeps links rendered for the previous scope:

- the List/Board switch hrefs;
- "any binding".

The busy dim also clears early. The pattern dates from p01. 03's wake-ups make the overlap routine.
This part was found by inspection only; it was not reproduced.

### Suggested direction

Treat a region (form) read as a board read for the coalescer:

- hold wake-ups while one is in flight;
- when releasing, issue on the current leaf (`document.querySelector('[data-board-wake]')`) instead of dropping
  the held one because its leaf was replaced.

The F1 fix can cover this in the same change.

### Resolution history

- 2026-10-31 R1: open.

---
- 2026-10-31 R2 ([NBV-P03-B1-R2](agent-review-notes.md); digest `38ee1d9edf0121d05d97eb866c403ef2915da61ede2959c13560bbba39cdef49`; tree `f07c076b`): **fixed**.
  - **(a) Held wake-up no longer dropped.** A held wake-up is replayed as `board-wake` on whichever leaf is
    current (`document.querySelector('[data-board-wake]')`, after an idempotent `htmx.process`). It is no
    longer dropped when its leaf was replaced. Harness case B: reads `leaf:v0, form:v0, leaf:v1`, and the
    board shows v1. In R1 it stayed on v0.
  - **(b) Filter reads no longer cancelled.** A filter read counts as a board read, so wake-ups and
    `run-selected` wait for it instead of cancelling it. Harness case C: `run-selected` arrives during a
    1.5 s read; its read starts at 1518 ms, immediately after the first settles, and nothing is aborted.
    The p01 card-click test was updated to match.
  - **Why clearing `held` at `beforeRequest` is safe.** An SSE event is sent after its change commits. Any
    wake-up received before a read starts is therefore covered by that read's snapshot.
  - **Stale cut-off checked.** Harness case D: a board read that never answers holds wake-ups, and
    30–32 s later they resume (reads at 32.1 s and 34.1 s, final state shown). The replayed read's `replace`
    aborts the hung xhr. While the read hangs, the board is frozen for up to about 30 s, by design.

## NBV-P03-B1-F3 — (optional, pre-existing) Polling reloads inside a pane still pull a phone reader back to it

- **Classification:** optional suggestion · Low · UX/layout.
- **Scope:** pre-existing, in another workspace; not caused by this batch.

### Evidence

D3 (`mailbox.rs:414-417`) exempts only requests whose triggering event is `sse:*`. The handler's principle is
"only a request the reader made is navigation". Polling requests break it the same way.

The Schedules runs column is an example:

- it polls with `hx-trigger="every 15s"`, targeting `#schedule-runs-column`;
- that column is inside `#schedules-workspace.ui-pane-detail` (`schedules.rs:55`, `:205`).

htmx issues polling requests with no triggering event, so they are not exempt. A phone reader who goes back to
the schedules list while a schedule is open would be pulled back to it within 15 s.

This is inferred from the handler and htmx's polling path, not reproduced.

### Suggested direction

Generalize the guard to reader-initiated requests: for example, ignore swaps whose triggering event is absent
(polling or `load`) as well as `sse:*`. Or raise a notice for the Schedules owner.

### Resolution history

- 2026-10-31 R1: open (optional).
- 2026-10-31 R2 ([NBV-P03-B1-R2](agent-review-notes.md); digest `38ee1d9edf0121d05d97eb866c403ef2915da61ede2959c13560bbba39cdef49`; tree `f07c076b`): **taken** (D3 r2).
  - The shell counts a swap as navigation only when its triggering event exists and is not `sse:*`.
    htmx 2.0.4 issues polls (`every`) and `load` with no triggering event.
  - Reviewer search: `pages/` has no `htmx.ajax` caller. Every pane-opening request is a click, submit,
    change or custom event, all of which carry an event. JS-initiated opens are not affected.
  - The phone browser test now also polls every 300 ms and keeps `data-pane=list`.
