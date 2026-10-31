# NBV-P05 agent review notes

## NBV-P05-B1-R1 — final integration review, round 1

### Identity

| Field | Value |
| --- | --- |
| Date | 2026-10-31 |
| Workflow | `implement-opus-reviewed-2_2` |
| Records | task [TASK.md](TASK.md); review [REVIEW.md](REVIEW.md); findings [agent-review-findings.md](agent-review-findings.md) |
| Batch / stage | phase 05 plus the combined integration of P01–P05; final integration review |
| Reviewer | B, Opus 5.5/xhigh |
| Phase | NBV-P05 |
| HEAD | `92e8c35e67dfb71e73db87895301dee74f380898`, all uncommitted |
| Review base | tree `7a120c2817ecca236772a033beb3e9d69a6f735c` (P04 accepted at fc4955c7) |
| Final tree | `07e777a8ecfb42daa3d073dcb8d886b9a3db6609` |
| Digest | `f8948ad46ad063fb311640c7544181ac7c52d260c40dd154873c8b705977cb99` |
| Exclusions | `plan/.archives/new-board-view/phases`, `plan/.archives/new-board-view/COORDINATION.md` |
| Gated decisions | none |
| Author decisions | NBV-P05-D1..D3, reviewed |

- **Freshness:** the digest was confirmed at the start and before returning. The working tree equals tree
  `07e777a8`, checked through a temporary index. No stray files: no `.nbv-*` dot-file is left in
  `scripts/tests`.
- **Coordination:** r6, at its new path.
  - NBV-I1: satisfied.
  - NBV-I2 (user: archive as 601440f did, delete nothing): satisfied; see "Archive" below.
  - NBV-N3: the theme part is satisfied. The phone-overflow part is **not** satisfied and is misreported (F1).
  - NBV-N4: the gate and the browser pass used fresh scratch databases. The developer database `mail_agents`
    was deliberately left as is and is reported for the user to reset. That is an acceptable, recorded
    disposition. The shared test database was reset at p02.

### Coverage and evidence

#### Archive (NBV-I2, 601440f)

- The plan and all phase records moved to `plan/.archives/new-board-view`; `plan/new-board-view` no longer
  exists.
- Phase records p01–p04 are complete: TASK, HANDOFF, REVIEW, notes, findings and logs.
- The six `NO_BACKWARD_COMPATIBILITY_NOTES` entries are repointed and kept with the other archives, as
  601440f did for the fix plans.
- The plan's links to the fix plans are rewritten to `../fix-…`. The fix plans' `../new-board-view/…` links
  now resolve.
- Check script output (`logs/review-b1-archive-check.log`):
  - 47 entries, all present and all carrying the note;
  - 0 broken relative links in the archived plan files and the three fix plans.
- Records keep the paths they were written with, as in 601440f.
- 05 step 3's "remove their entries" is superseded by the user's NBV-I2 ("repoint …; delete nothing").

#### Browser pass (05 step 1)

- **Read:** `browser-pass.mjs`, the seed script and the results JSON.
- **Real checks:** theme toggled with the real switch (`label:has(#theme-toggle)`), admin channel
  visibility, live cancel without a reload, phone pane open and back.
- **Not exercised, reported honestly in TASK.md:**
  - Retry: the engine refused it correctly. 05 lists "a card moving live after cancel or retry", and cancel
    satisfies that.
  - A Dashboard job opening its run: no job was outstanding; covered by p04 page and DB tests.
- **Misrecorded in the JSON:** the retry item is marked `ok: true` (F2).
- **Phone overflow:** wrong (F1). I probed the served app read-only at 390×844 and 1400×900, plus the list.

#### Gate (05 final verification)

- `check.log`: every `scripts/check.sh` gate PASS, including the DB-backed suite 2511/0/6 and the stack
  budget.
- `gate-node.log`: 18/18.
- `gate-*.log`: focused suites.
- **Timing gap, closed:** `check.log` ends at 11:40:35, and the gate runs about 19 minutes. Its fmt and
  whitespace gates may have run before the last `src/main.rs` edit (11:22:20). I re-ran them on the final
  tree (`logs/review-b1-quick-gates.log`):
  - `cargo fmt --all -- --check`: exit 0
  - `git diff HEAD --check`: exit 0
  - `cargo clippy --all-targets -D warnings`: finished clean
- **Lease budget:** 05 says `check.sh` includes it. It does not (no lease gate in `scripts/check.sh`). The
  last run was at p02 R2, and no lease-path SQL changed since. Accepted.

#### Step 2 (tripwires)

`warn_if_run_list_is_slow` and `warn_if_run_board_is_slow` are both present with `workflow=`, and
`docs/performance/run-list.md` "Revisit when" reads them. Confirmed by inspection.

#### Combined integration of P01–P05

Exercised together on the served page:

- the board with p02 filters, p03 live movement and p04 rail lighting;
- the D3 shell change and the board's phone layout;
- the list's form-include leaf;
- the Dashboard link.

Optional findings carried from earlier phases remain as recorded, for example p01 F6 (pane links push the
list URL), which was not taken.

#### Checks run by the reviewer on the final tree

DB-backed checks used `-- --test-threads=4`.

| Check | Result | Log |
| --- | --- | --- |
| `run_board_view_` | 14/14 | `logs/review-b1-run_board_view_.log` |
| `icon_rail_` | 2/2 | `logs/review-b1-icon_rail_.log` |
| `node --test scripts/tests/workflow-runs.mjs` | 13/13 | `logs/review-b1-workflow-runs-node.log` |
| Quick gates (fmt, whitespace, clippy) | as above | `logs/review-b1-quick-gates.log` |
| Archive and notes-list check | as above | `logs/review-b1-archive-check.log` |
| Served-page phone probe | F1 evidence | `logs/review-b1-phone-overflow.log` |

- **Inspected, not rerun:** the full `check.sh`, the author's browser pass and its seed, the gate logs.
- **Not done:** re-running `browser-pass.mjs`. My probe measured the disputed item directly.

### Outcome

**Changes required.**

| Finding | Kind | Severity | Summary |
| --- | --- | --- | --- |
| [F1](agent-review-findings.md) | actionable | Medium | On a phone, a long board column cannot be scrolled (the runs list likewise); the pass recorded the failing signal as a pass, and TASK.md says the column scrolls. Likely `<main>` without `min-h-0` in both runs pages. |
| [F2](agent-review-findings.md) | actionable | Low | The results JSON marks the unexercised retry item `ok: true`; the totals quoted in TASK.md come from a different round than the JSON. |
| [F3](agent-review-findings.md) | optional, pre-existing | Low | The list opens on its empty pane on a phone: `run_empty_pane` lacks `data-pane-empty`. |

Everything else passes: the archive, the gate, step 2, both themes on the served page, visibility, live
cancel, the rail and the Dashboard link.

### Process and instruction feedback

The 05 browser pass was thorough and mostly well-instrumented. The one miss was a proxy assertion that could
not fail in the failing case.

Candidate instruction: phone checks assert reachability, and a pass's machine-readable record never marks an
unexercised item `ok`.

## NBV-P05-B1-R2 — final integration correction recheck, round 2

### Identity

| Field | Value |
| --- | --- |
| Date | 2026-10-31 |
| Workflow | `implement-opus-reviewed-2_2` |
| Records | [TASK.md](TASK.md) ("Browser pass", round 3; "Corrections for R1"); [REVIEW.md](REVIEW.md); [agent-review-findings.md](agent-review-findings.md) |
| Reviewer | B, Opus 5.5/xhigh |
| HEAD | `92e8c35e67dfb71e73db87895301dee74f380898` |
| Delta | tree `07e777a8` → tree `b122c8feb95ccf6ab1a8cf0e58b580e727056d9b`: 2 files, +7/−4 |
| Digest | `f7da3b6c896de2515d48ff553804929220e6d512210f4d0f26b8f06d832c612c`, exclusions as R1 |

- **Digest and tree:** the digest was confirmed at the start and before returning. The working tree equals
  tree `b122c8fe`, checked through a temporary index.
- **Coordination:** r6. NBV-N3 is now fully satisfied: the served-page theme check was done in R1, and the
  phone overflow is fixed and verified.

### Coverage and evidence

**Delta read in full.**
- `min-h-0` on both runs pages' `<main>`.
- `data-pane-empty` on `run_empty_pane`, whose only caller is the list with no run open.
- The doc comments.
- The `min-h-0` utility already exists in `assets/app.css`, and `check:css` exits 0.

**Served-page re-probe.** I re-ran `p05-phone-overflow-r2.mjs` read-only against the rebuilt app on :3001. The
script now scrolls the board's columns sideways to bring the failed column into view first. Log:
`logs/review-r2-phone-overflow.log`.

| View | Page height | Column / list (client/scroll) | Last card |
| --- | --- | --- | --- |
| Board, phone 390×844 | 844 | `ul` 510/4493 | reached at y=742 |
| Board, desktop | — | 686/4493 | reached |
| List, phone | 844 | 540/2923 | — |

The list on a phone also opens on the list (`data-pane=list`), as F3 intended.

**Browser pass round 3, inspected.** `logs/browser-pass-results-round-3.json` and `browser-pass-round-3.txt`:
- 13 items PASS and 2 NOT EXERCISED, each with a reason.
- The phone check now asserts the page height and that the last card can be reached.

**Checks run on the final tree** (DB-backed checks with `-- --test-threads=4`):

| Check | Result | Log |
| --- | --- | --- |
| `run_board_view_` | 14/14 | `logs/review-r2-run_board_view_.log` |
| `routes::workflows::ui::runs::` | 26/26 | `logs/review-r2-routes__workflows__ui__runs__.log` |
| `workflow-runs.mjs` | 13/13 | `logs/review-r2-workflow-runs-node.log` |
| `check:css` | exit 0 | `logs/review-r2-check-css.log` |
| `cargo fmt --check` | exit 0 | `logs/review-r2-fmt.log` |

**Full suite.** `check.sh` was not rerun after this markup-only correction. Its last full run, on tree
`07e777a8`, passed every gate. The correction changes two class lists and one attribute, and the focused
suites that render them pass. This is accepted under SKILL.md: "rerun affected checks; repeat the full suite
when the impact warrants it".

**Inspected, not rerun:** the author's `logs/r1-*.log`, including `routes::workflows::ui::` 72 and `pages::` 176.

### Outcome

**PASS** for digest `f7da3b6c896de2515d48ff553804929220e6d512210f4d0f26b8f06d832c612c`.

| Item | Result |
| --- | --- |
| F1 | fixed |
| F2 | fixed |
| F3 (optional) | taken |
| Open findings | none |
| Combined integration of P01–P05 | passes |

R1 coverage is reused for the archive (NBV-I2), the gate and step 2.

**Outstanding for the user, recorded in TASK.md (not artifact defects):**
- reset the developer database `mail_agents` before serving (NBV-N4);
- drop the scratch databases and the :3001 server at completion.

### Process and instruction feedback

The candidate from R1 stands: phone checks must assert reachability, and machine-readable pass records
distinguish NOT EXERCISED from PASS. The correction adopted both.
