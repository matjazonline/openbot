# NBV-P01 review record

| Field | Value |
| --- | --- |
| Phase | NBV-P01 (plan `new-board-view` 01, board view and layout) |
| Workflow | `implement-opus-reviewed-2_2` |
| Task record | [TASK.md](TASK.md) |
| Worktree | `/Users/mac02/.codex/worktrees/b109/openbot` |
| Coordination | `plan/new-board-view/COORDINATION.md` r1, NBV-I1 r1 |
| Review notes | [agent-review-notes.md](agent-review-notes.md) |
| Findings | [agent-review-findings.md](agent-review-findings.md) |

## Rounds

| Round | Stage | Reviewer | Commit | Digest | Outcome |
| --- | --- | --- | --- | --- | --- |
| [NBV-P01-B1-R1](agent-review-notes.md) | B1 batch review | B, Opus 5.5 / xhigh | `92e8c35` (start = base = HEAD) | `67d63ee9d7b1483f1ee27c0c2d41d826452625f302789cef86aefa7b979de666` | **Changes required** |
| [NBV-P01-B1-R2](agent-review-notes.md) | B1 correction recheck | B, Opus 5.5 / xhigh | `92e8c35` (base = HEAD) | `158ecfe951d14a9b901725d24e1427e0fc4a2341985e84d2cdf5cb1d0be171da` | **PASS** |

Both digests exclude `plan/new-board-view/phases/p01/docs` and `plan/new-board-view/COORDINATION.md`.

## Findings (latest disposition)

| ID | Kind | Severity | Summary | Status |
| --- | --- | --- | --- | --- |
| [NBV-P01-B1-F1](agent-review-findings.md) | actionable | Medium | Phone width: the first card click shows nothing (pane `display:none`), and the pane is never more than half the screen | fixed (R2) |
| [NBV-P01-B1-F2](agent-review-findings.md) | actionable | Medium | Busy feedback is `aria-busy` only (no visible change); Back restores a pane stuck at `aria-busy="true"` | fixed (R2) |
| [NBV-P01-B1-F3](agent-review-findings.md) | actionable | Medium | The `run-selected` leaf re-reads the render-time window and aborts an in-flight window change (select and columns disagree); no out-of-order test; 03 makes it routine | fixed (R2) |
| [NBV-P01-B1-F4](agent-review-findings.md) | actionable | Low | Board cards are outside the detached-response guard (`#run-board` not in the guard's selector) | fixed (R2) |
| [NBV-P01-B1-F5](agent-review-findings.md) | actionable (verification) | Low | Plan 01 step 8's light/dark computed check was deferred without an agreed disposition (reviewer fixture evidence attached) | deferred to 05, disposition NBV-P01-D7 accepted (R2); unverified until 05 |
| [NBV-P01-B1-F6](agent-review-findings.md) | optional | Low | Pane-internal run links push the list URL while on the board | not taken (optional) |
| [NBV-P01-B1-F7](agent-review-findings.md) | optional | Low | Card age and child rollup not pinned by route tests; "started" for queued runs | taken (R2) |
| [NBV-P01-B1-F8](agent-review-findings.md) | optional | Low | List/Board switch uses ARIA tab roles on navigation links | taken (R2) |
| [NBV-P01-B1-F9](agent-review-findings.md) | optional, pre-existing | Low | Phone: a pane wake-up (sse-triggered `hx-get`, which has an xhr) drags the reader from the board back to the pane; the list has the same behaviour | open (optional; route to 03 or a shell fix) |
