# NBV-P03 agent review notes

## NBV-P03-B1-R1 — batch review, round 1

### Identity

| Field | Value |
| --- | --- |
| Date | 2026-10-31 |
| Workflow | `implement-opus-reviewed-2_2` |
| Records | task [TASK.md](TASK.md); review [REVIEW.md](REVIEW.md); findings [agent-review-findings.md](agent-review-findings.md) |
| Batch / stage | B1 (whole phase 03), batch review |
| Reviewer | B, Opus 5.5/xhigh |
| Phase | NBV-P03 |
| Worktree | `/Users/mac02/.codex/worktrees/b109/openbot` |
| HEAD | `92e8c35e67dfb71e73db87895301dee74f380898`, all changes uncommitted |
| Review base | tree `d68a8f1b3e86c4082eef8dd718f7a22edaf470e3` (NBV-P02 accepted at d6d1ff4f) |
| Phase state | tree `b923973964cfed23e37ee025d2c5c34e063179de` |
| Digest | `891cc567ad7f8c950b7726deb555fa2bc63d956ca757c2e697595b39be245b26` |
| Digest exclusions | `plan/new-board-view/phases`, `plan/new-board-view/COORDINATION.md` |
| Gated decisions | none |

The author's decisions NBV-P03-D1..D3 were reviewed. D2 (the refresh protocol) and D3 (the shared shell
handler) were challenged as the assignment asked.

**Digest and tree.** The digest was confirmed at the start and before returning. The working tree equals tree
`b9239739`; I checked this through a temporary index without touching the real index or the worktree.

**Coordination.** `plan/new-board-view/COORDINATION.md` is at r3.

| Notice | Status |
| --- | --- |
| NBV-I1 r1 | Satisfied; nothing persistent changed. |
| NBV-N1 r1 | Addressed by D3, which fixes stream-driven pane reloads. The broader polling case is F3, optional. |
| NBV-N2 r1 | Still satisfied: the leaf includes the whole form. |
| NBV-N3, NBV-N4 | Not in scope (p05). |

### Coverage and evidence

**Requirements read**

- `plan/new-board-view/README.md`
- `03-live-updates.md`
- `src/adapters/http/AGENTS.md`, race-safe section
- `src/adapters/http/pages/AGENTS.md`
- `src/AGENTS.md`

**Code read in full in the phase delta**

- `run_board`'s leaf: the triggers and `data-board-wake`.
- The coalescer in `WORKFLOW_RUNS_SCRIPT`.
- `board_html` / `board_html_at`, where `as_of` is taken per read.
- The `mailbox.rs` beforeSwap guard.
- The three `run_board_view_live_*` tests.
- The new `workflow-runs.mjs` tests.

**Traced**

- htmx 2.0.4 paths in the shipped minified build:
  - `htmx:confirm` fires before the `hx-sync` handling;
  - `issueRequest(true)` re-enters `issueAjaxRequest` and skips confirm;
  - input values are gathered at issue time;
  - `replace` aborts the sync element's in-flight xhr;
  - polling requests carry no triggering event.
- `events.rs`: `resync` first and on lag, `tick` every 30 s.
- The referenced model, `ui_run_counts.rs` `RefreshScheduler`. It is completion-based and per tab, so D2's
  per-tab bound (instead of the plan's "per viewer") is consistent with the model the plan points to, and is
  accepted.
- Every other stream- or poll-triggered swap in `pages/`, to judge D3:
  - attention and notifications lists: outside `.ui-pane-detail`, so unaffected;
  - run list and board leaves: inside `.ui-pane-list`, so unaffected;
  - run body: fixed by D3;
  - schedules' `every 15s` column, inside `.ui-pane-detail`: not covered by D3 (F3).

**Challenges to the D2 guarantee**

| Claim | Result |
| --- | --- |
| ≤1 read per 2 s per tab | Holds for issued reads. `run-selected` deliberately bypasses it. |
| Trailing read after a burst | Holds, except in F2(a). |
| No starvation | Fails when reads are slower than the interval (F1). |
| Interaction with `hx-sync` replace | F1 and F2(b). |
| Filter changes | F2. |
| `run-selected` reads at once and clears the held wake-up | Correct: its read starts after the held event arrived. |

**Checks run**

DB tests ran with `SQLX_OFFLINE=true TEST_DATABASE_URL=postgres://mac02@localhost:5432/mail_agents_run_board … -- --test-threads=4`.
Node tests ran with `CHROME_PATH=…`.

| Check | Result | Log |
| --- | --- | --- |
| `run_board_view_` | 14/14, including 3 `run_board_view_live_*` | `logs/review-b1-run_board_view_.log` |
| `routes::workflows::ui::runs::` | 26/26 | `logs/review-b1-routes__workflows__ui__runs__.log` |
| `pages::` | 175/175, 2 ignored | `logs/review-b1-pages__.log` |
| `node --test scripts/tests/workflow-runs.mjs scripts/tests/sse-navigation.mjs` | 15/15 (10 + 5) | `logs/review-b1-node.log` |
| Scratch harness `board-coalescer-p03.mjs` (shipped htmx, sse ext and run script; real SSE stream) | A demonstrates F1; B demonstrates F2(a) | `logs/review-b1-coalescer-harness.log` |

**Author evidence inspected, not rerun**

- The mutation checks described in TASK.md.
- `logs/other-node.log` and `logs/workflow-decisions.log`.
- clippy.

**Not run**

- A served-page browser pass.
- The full suite.

**Coverage limits**

- F2(b) and F3 come from inspection and htmx's code paths; they were not reproduced.

### Outcome

**Changes required.**

| Finding | Kind | Severity | Summary |
| --- | --- | --- | --- |
| [F1](agent-review-findings.md) | actionable | Medium | Start-based throttle combined with `hx-sync` replace: reads slower than 2 s abort each other under constant change, and the board freezes until the change stops. The referenced model is completion-based. |
| [F2](agent-review-findings.md) | actionable | Low | (a) A held wake-up is dropped when an in-flight filter read replaces its leaf, although that read may predate the change: a missed update for up to 30 s. (b) A released wake-up aborts an in-flight filter read, so the toolbar keeps links for the previous scope. |
| [F3](agent-review-findings.md) | optional, pre-existing | Low | D3 exempts only `sse:*`. Polling inside a pane (schedules `every 15s`) still pulls a phone reader back. |

**What passes**

- The leaf's triggers and the `as_of` recomputed on each read.
- The three live route tests:
  - a change between render and subscribe is reconciled by the first `resync`;
  - a run on a hidden channel produces no wake-up, with a control case that does;
  - the window moves on a later `as_of`.
- The detached-response guard already covers `#run-board`.
- D3's fix for the pane body. Its phone test passes.
- The burst and constant-change bounds, with fast reads.

### Process and instruction feedback

**Batch size.** Right-sized.

**Should D2 have been gated?** D2 is a new concurrency protocol (`src/adapters/http/AGENTS.md`), and SKILL.md
lists concurrency protocols as gate-worthy. A pre-implementation gate on D2 would probably have surfaced F1.
The author's mutation checks were useful evidence. Missing: a test whose answers are slower than the interval.

**Candidate instructions** (none edited)

1. http AGENTS: "client-side refresh coalescers are completion-based, and tests include reads slower than the
   interval".
2. Workflow: "a new client-side refresh protocol counts as a concurrency protocol for the architecture gate".

## NBV-P03-B1-R2 — correction recheck, round 2

### Identity

| Field | Value |
| --- | --- |
| Date | 2026-10-31 |
| Workflow | `implement-opus-reviewed-2_2` |
| Records | [TASK.md](TASK.md) (D2 r2, D3 r2, "Corrections for R1"); [REVIEW.md](REVIEW.md); [agent-review-findings.md](agent-review-findings.md) |
| Stage | B1 correction recheck |
| Reviewer | B, Opus 5.5/xhigh |
| HEAD | `92e8c35e67dfb71e73db87895301dee74f380898` |
| Delta | tree `b9239739` → tree `f07c076b2242a35b0bd97bf400b1bcc854d04603` |
| Digest | `38ee1d9edf0121d05d97eb866c403ef2915da61ede2959c13560bbba39cdef49`, exclusions as R1 |

- **Digest and tree:** the digest was confirmed at the start and before returning. The working tree equals
  tree `f07c076b`, checked through a temporary index.
- **Coordination:** still r3. NBV-N1 is now fully addressed: the guard covers stream-driven reloads and also
  polls.

### Coverage and evidence

**Delta read in full**

- The `WORKFLOW_RUNS_SCRIPT` coalescer.
- The leaf's `board-wake` trigger and its doc comment.
- The `mailbox.rs` guard.
- The `board_view_tests.rs` trigger pin.
- The new and changed `workflow-runs.mjs` tests.

**D2 r2 challenged as a concurrency protocol**

- **Per-xhr tracking.** Registration happens at `htmx:beforeRequest`. A request aborted by `replace` fires
  `afterRequest` synchronously, before the replacing request's `beforeRequest`. The order is therefore delete,
  then add.
- **Settle listeners.** Each is scoped to its element and matched on the xhr. It fires even when the element
  is detached, because htmx dispatches on the element itself. It is removed after it fires.
- **Responses dropped by the detached-response guard.** They still settle: `beforeOnLoad` only cuts short the
  response handler, and `afterRequest` still fires.
- **Clearing `held` at read start.** This is correct because a wake-up is sent after its commit.
- **Replay.** Replay goes to the current leaf; `htmx.process` is idempotent through htmx's init hash.
- **`run-selected` during a read.** It waits for the read, then goes at once.
- **Stale cut-off (30 s).** A hung read freezes the board for up to about 30 s. Recovery needs any
  subsequent event, and a tick arrives at least every 30 s. The replayed read aborts the hung xhr.
- **Residual.** If another handler cancelled a request in `beforeRequest` after this listener ran, that
  request would sit in `inFlight` until the cut-off. No such handler exists today.

**D3 r2 challenged across the shell**

- `pages/` has no `htmx.ajax` caller.
- Pane opens are click, submit, change or custom events, all of which carry an event.
- Polls and `load` now count as refreshes.
- Attention and notifications are outside `.ui-pane-detail`. Their behaviour is unchanged.

**Checks run** (DB checks with `-- --test-threads=4`; node with `CHROME_PATH=…`)

| Check | Result | Log |
| --- | --- | --- |
| `run_board_view_` | 14/14 | `logs/review-r2-run_board_view_.log` |
| `pages::` | 175/175 | `logs/review-r2-pages__.log` |
| `node --test scripts/tests/workflow-runs.mjs scripts/tests/sse-navigation.mjs` | 18/18 (13 + 5) | `logs/review-r2-node.log` |
| Reviewer harness `board-coalescer-p03-r2.mjs`, cases A–D | see below | `logs/review-r2-coalescer-harness.log` |

Harness cases (shipped htmx, the SSE extension and the run script, with a real SSE stream):

- **A, slow reads under constant change:** 3 of 3 reads completed, none aborted.
- **B, change during a filter read:** replayed on the new leaf; the board shows v1.
- **C, `run-selected` during a slow read:** follows at 1518 ms with no abort.
- **D, hung read:** reads resume at 32.1 s and 34.1 s, and the final state is shown.

**Inspected, not rerun:** the author's mutation checks and `logs/*-r1*.log`.

### Outcome

**PASS** for digest `38ee1d9edf0121d05d97eb866c403ef2915da61ede2959c13560bbba39cdef49`. F1 and F2 are fixed;
F3 (optional) was taken. No findings are open.

### Process and instruction feedback

The R1 candidate instructions stand: completion-based coalescers, and treating client refresh protocols as
gate-worthy concurrency protocols. The R2 fix followed the referenced completion-based model, and its tests
include slow answers.
