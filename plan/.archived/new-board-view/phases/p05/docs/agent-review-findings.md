# NBV-P05 agent review findings

## Common context

Applies to every finding below unless stated otherwise.

| Field | Value |
| --- | --- |
| Date | 2026-10-31 |
| Workflow | `implement-opus-reviewed-2_2` |
| Task record | [TASK.md](TASK.md) |
| Review record | [REVIEW.md](REVIEW.md) |
| Batch | B1 (phase 05) plus the combined integration of P01–P05 |
| Stage | final integration review |
| Review round | [NBV-P05-B1-R1](agent-review-notes.md) |
| Reviewer | B, Opus 5.5/xhigh |
| Phase | NBV-P05 |
| Worktree | `/Users/mac02/.codex/worktrees/b109/openbot` |
| HEAD | `92e8c35e67dfb71e73db87895301dee74f380898` |
| Final tree | `07e777a8ecfb42daa3d073dcb8d886b9a3db6609` |
| Digest | `f8948ad46ad063fb311640c7544181ac7c52d260c40dd154873c8b705977cb99` |
| Exclusions | `plan/.archives/new-board-view/phases`, `plan/.archives/new-board-view/COORDINATION.md` |

**Served-page probe.** `p05-phone-overflow.mjs` lives in the reviewer's scratchpad, not the repository:
`/private/tmp/claude-501/-Users-mac02--codex-worktrees-b109-openbot/147507f8-dc5b-4026-9dc4-af253abbbdfc/scratchpad/harness/`.
It drives the app served on `http://localhost:3001` (scratch database `mail_agents_nbv_pass`), read-only, with
the owner's session from the scratchpad. Output: `logs/review-b1-phone-overflow.log`.

---

## NBV-P05-B1-F1 — On a phone, a long column cannot be scrolled, and the pass reported the opposite

- **Classification:** actionable defect · **Medium** · requirements (NBV-N3) and correctness (layout) ·
  verification reporting.

### Evidence

**The defect, measured on the served board at 390×844.** The 50-card failed column does not scroll inside
itself. The page grows instead, and the shell's `body` is `overflow-hidden`.

| Measure | Value |
| --- | --- |
| `document` client / scroll height | 844 / 4827 |
| `body` overflow | hidden |
| `#run-board-columns` client / scroll height | 4594 / 4594 |
| failed column `ul` client / scroll height | 4493 / 4493 |
| last card's top | 4725 px |
| the only scrollable ancestor | `body`, overflow hidden |

Twenty mouse-wheel steps over the column leave the last card at 4725 px. A reader can reach only the cards on
the first screen, about the first eight.

**Desktop is fine.** At 1400×900 the `ul` is 686/4493 and the last card scrolls into view.

**The runs list has the same problem.** After pane-back on a phone, the list's `document` is 844/3228 and its
`ul` 2923/2923, so it does not scroll.

**Likely cause.** Both runs pages render `<main class="flex min-w-0 flex-1">`:

- `workflow_runs/board.rs:310`
- `workflow_runs/mod.rs:497`

There is no `min-h-0`. Under the shell's phone layout (`.app-workspace { flex-direction: column }`,
`COMPACT_LAYOUT_STYLES`), `<main>` therefore grows to its content height, so nothing below it can scroll. The
p01 reviewer harness showed the same shape (NBV-P01-B1-R1/R2 notes), and NBV-N3 routed it to 05.

**What NBV-N3 required, and what the pass reported.** NBV-N3 says "the 05 browser pass … must look at phone
vertical overflow of a 50-card column". The pass's check (`logs/browser-pass.mjs`, phone check) returns
"column fits" when `ul.scrollHeight <= ul.clientHeight`. That is exactly the signal of this overflow. Yet
`logs/browser-pass-results.json` records it as a pass ("column fits"). TASK.md "Browser pass" says "the failed
column scrolls inside itself (NBV-N3 overflow: no page overflow observed)", which is contrary to the pass's own
data and to the served page.

### Impact

On phones, both runs views hide most of any long column or list with no way to scroll to it. The 05 record
claims NBV-N3 is satisfied.

### Suggested direction

- Constrain `<main>` on both runs pages, for example with `min-h-0`. Check that `#run-board-columns` or each
  column then scrolls inside itself, and that the list does too.
- Check other workspaces whose `<main>` follows the same pattern.
- Make the pass measure reachability: page scroll height against the viewport, or the last card reachable by
  wheel or touch. A column-fits test cannot tell these apart.

### Instruction lesson

A check should assert the property the requirement names, not a proxy that can pass in the failing case.
Candidate for the pages AGENTS phone-layout guidance: "a phone check asserts the last item of a long list is
reachable by scrolling".

### Resolution history

- 2026-10-31 R1: open.

---
- 2026-10-31 R2 ([NBV-P05-B1-R2](agent-review-notes.md); digest `f7da3b6c896de2515d48ff553804929220e6d512210f4d0f26b8f06d832c612c`; tree `b122c8fe`): **fixed**.
  - **Change:** both runs pages' `<main>` gained `min-h-0` (`board.rs:310`, `mod.rs:497`). The reviewer
    confirmed the served build carries the class.
  - **Board at 390×844**, reviewer re-probe of the served app (`logs/review-r2-phone-overflow.log`):
    - page 844/844;
    - failed column `ul` 510/4493 and scrollable;
    - after scrolling the columns sideways to the fifth column and wheeling over it, the last card is at
      y=742 and visible.
  - **Desktop:** unchanged, 686/4493, last card reached.
  - **Runs list on a phone:** `ul` 540/2923, scrollable; the page stays 844.
  - **The pass's phone check** now fails when the page outgrows the viewport or the last card cannot be wheeled
    into view. Round 3: "page 844px; last of 50 failed cards reached at y=742".
  - **TASK.md** now reports this correctly.

## NBV-P05-B1-F2 — The machine-readable browser-pass record marks an unexercised check as passed

- **Classification:** actionable defect · **Low** · verification record.

### Evidence

`logs/browser-pass-results.json` has
`{"name": "live: retry from the pane moves a failed card out of failed", "ok": true, …}`. Its detail says
the retry was refused ("Nothing was done: This run cannot be retried…") and the card is "now in failed".

The script returns a string instead of throwing when nothing moved (`browser-pass.mjs`, retry check), so the
item is recorded as passed. TASK.md correctly reports it as NOT EXERCISED.

The JSON also comes from a later round than the totals TASK.md quotes:

| Source | queued | running | waiting | succeeded | failed | cancelled |
| --- | --- | --- | --- | --- | --- | --- |
| TASK.md | 1 | 2 | 3 | 5 | 57 | 2 |
| JSON | 1 | 1 | 3 | 5 | 57 | 3 |

The difference is one run that an earlier round's cancel moved. Neither record says which round produced which.

The dashboard row ("outstanding job rows linking a run: 0") also reads as a pass for an item that was not
exercised. That one is honestly qualified in TASK.md.

### Impact

A later reader of the JSON alone would believe retry-driven live movement was verified.

### Suggested direction

- Record the retry item, and the dashboard job-opening item, as not exercised in the results: the script
  throws or emits `ok: null` with a reason.
- Name the round (`NBV_ROUND`) that produced the JSON next to the totals quoted.

### Resolution history

- 2026-10-31 R1: open.

---
- 2026-10-31 R2 ([NBV-P05-B1-R2](agent-review-notes.md); digest `f7da3b6c896de2515d48ff553804929220e6d512210f4d0f26b8f06d832c612c`; tree `b122c8fe`): **fixed**.
  - The pass records PASS / FAIL / NOT EXERCISED.
  - `logs/browser-pass-results-round-3.json` (`"round": 3`) marks retry and the Dashboard job link
    NOT EXERCISED, with reasons.
  - Earlier rounds are kept and named by round (round-1, round-2).
  - TASK.md cites round 3 and its totals, and explains that earlier rounds' cancels moved runs.

## NBV-P05-B1-F3 — (optional, pre-existing) The runs list opens on its empty pane on a phone

- **Classification:** optional suggestion · Low · UX/layout. Pre-existing; not caused by this plan.

### Evidence

On the served list page at 390×844 with no run open, `body[data-pane]` is `detail`, the list column is
`display: none`, and the visible pane says "Choose a run…" (`logs/review-b1-phone-overflow.log`, line
`list …`).

`run_empty_pane` (`workflow_runs/mod.rs:524-529`) has no `data-pane-empty`, so the shell's `syncMobilePane`
treats it as an open run. The board's empty pane gained `data-pane-empty` in p01. The list's never had it.

### Suggested direction

Add `data-pane-empty` to `run_empty_pane`. The shell's handler then also returns the reader to the list
whenever a control puts an empty pane back. Pair this with F1's fix.

### Resolution history

- 2026-10-31 R1: open (optional).
- 2026-10-31 R2 ([NBV-P05-B1-R2](agent-review-notes.md); digest `f7da3b6c896de2515d48ff553804929220e6d512210f4d0f26b8f06d832c612c`; tree `b122c8fe`): **taken**.
  - `run_empty_pane` carries `data-pane-empty` (`mod.rs:524-529`). Its only caller is the list page with no
    run open.
  - Served probe: the list opens with `data-pane=list`, the list column displayed, and the pane marked empty.
