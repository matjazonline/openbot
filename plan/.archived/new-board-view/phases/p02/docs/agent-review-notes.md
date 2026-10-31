# NBV-P02 agent review notes

## NBV-P02-B1-R1 — batch review, round 1

### Identity

| Field | Value |
| --- | --- |
| Date | 2026-10-31 |
| Workflow | `implement-opus-reviewed-2_2` |
| Records | task [TASK.md](TASK.md); review [REVIEW.md](REVIEW.md); findings [agent-review-findings.md](agent-review-findings.md) |
| Batch / stage | B1 (whole phase 02), batch review |
| Reviewer | B, Opus 5.5/xhigh |
| Phase | NBV-P02 |
| Worktree | `/Users/mac02/.codex/worktrees/b109/openbot` |
| HEAD | `92e8c35e67dfb71e73db87895301dee74f380898`, all uncommitted |
| Review base | tree `85b05025ef161cd04a7e07081f168bb51f649a75` (NBV-P01 accepted at 158ecfe9…) |
| Phase state | tree `ed59ca7f2423bb3c5407fdf61335d9800f0afa2c` |
| Digest | `68e88e1d5fbbcc572cd7bd85b67c416eb7d2cc6e88c9db5e1f52c5df460926f1` |
| Exclusions | `plan/new-board-view/phases`, `plan/new-board-view/COORDINATION.md` |
| Gated decisions | none; author decisions NBV-P02-D1..D6 reviewed |

- **Artifact checks.** The digest was confirmed at the start and before returning. The working tree was
  confirmed equal to tree `ed59ca7f` by writing it through a temporary index in the scratchpad, without
  touching the real index or the worktree.
- **Coordination.** `plan/new-board-view/COORDINATION.md` is at r2.
  - Applies: NBV-I1 r1, satisfied (no migration or compatibility path; no schema change in this batch).
  - Applies: NBV-N2 r1. Satisfied for the board. It was not applied to the list's analogous leaf, which is F1.
  - Not in scope: NBV-N1 and NBV-N3.

### Coverage and evidence

**Requirements read**

- `plan/new-board-view/README.md`, `02-filters-and-entry-points.md`, and the fix-01 02 index ladder.
- The AGENTS files: root, `src`, `http`, `pages`, `persistence`. From `persistence`: the performance-evidence
  rule, the SQL style rule, and the sqlx-cache note that runtime queries in `workflow/` generate no `.sqlx/`
  entries.

**Code read in full in the phase delta**

- Application: `RunFilter.workflow`, `RunBoardScope.workflow`.
- Persistence: list `push_run_page` workflow predicate; board `push_scope` and the working-set column; shape
  dumps and the placeholder-order tests.
- JSON routes: `RunsQuery` and `BoardQuery` `workflow_id`, `company_workflow`, slow-line fields.
- UI routes: `ScopeIds`, `Scope`, `Ui::scope`, `Ui::scope_options`, `board_html`, `workspace`, `list`,
  `run_filter`.
- Pages: `RunFilterView` query builder, `ScopeOptions`, `scope_controls`, `run_list`, `run_board`, the
  toggle's move.
- Entry points: Workflows "Runs", workflow pane "Runs board", agent usage card.
- Tests: `board_view_tests.rs` (new `run_board_view_filter_*`), `run_filter_tests.rs` JSON parity,
  `reads_tests.rs`, and `workflow-runs.mjs`.
- Docs and scripts: the `docs/performance/run-list.md` diff, the `run-list-query-plan.py` diff, and the
  workflow sections of both regenerated plan outputs. List workflow shapes were read in both plan modes;
  board workflow shapes only enough to confirm real rows were measured.

**Assumptions challenged**

| Item | Result |
| --- | --- |
| D2: ids resolved company-scoped, list binding now validated | tested |
| D3: one query builder; leaf includes the whole form | tested for the board; list leaf → F1 |
| D4: option bounds and cut notes | the board leaf's options read → F3 |
| D5: entry point placement | accepted; links untested → F4 |
| D6: index deferral | evidence and access path re-read → F2 |
| Escaping of workflow titles, agent names and short ids in option and `aria` sinks | inspected, plus the unit test with a hostile label |

**Checks run by the reviewer.** DB-backed runs used
`SQLX_OFFLINE=true TEST_DATABASE_URL=postgres://mac02@localhost:5432/mail_agents_run_board … -- --test-threads=4`.

| Check | Result | Log |
| --- | --- | --- |
| `run_board_view_` | 10/10 | `logs/review-b1-run_board_view_.log` |
| `run_agent_filter_` | 4/4 | `logs/review-b1-run_agent_filter_.log` |
| `run_board_read_` | 10/10 | `logs/review-b1-run_board_read_.log` |
| `run_top_level_` | 7/7 | `logs/review-b1-run_top_level_.log` |
| `persistence::workflow::reads` | 5/5, 1 ignored dump | `logs/review-b1-persistence__workflow__reads.log` |
| `routes::workflows::ui::runs::` | 23/23 | `logs/review-b1-routes__workflows__ui__runs__.log` |
| `node --test scripts/tests/workflow-runs.mjs` | 5/5 | `logs/review-b1-workflow-runs-node.log` |
| `npm run check:css` | exit 0 | `logs/review-b1-check-css.log` |
| Scratch harness `list-race-p02.mjs` (shipped htmx and run script, list markup as rendered) | demonstrates F1 | `logs/review-b1-list-race-harness.log` |

**Author evidence inspected, not rerun**

- The query-plan run: the `logs/qp-*` files and the regenerated `docs/performance/*.txt`. The script was not
  rerun.
- Clippy: `logs/clippy.log` shows only a cached "Finished" line.
- The broader suites in TASK.md: `pages::` 175, `routes::workflows::ui::` 68, `routes::workflows::tests::` 31.

**Not run**

- A served-page browser pass.
- The full suite.
- The query-plan script itself, including a dense or sparse workflow case. That is part of F2's remedy.

**Reviewed lightly**

- The 20k-line regenerated `run-board-query-plan.txt`, beyond the workflow sections.
- `assets/app.css`, beyond `check:css`.

### Outcome

**Changes required.**

| Finding | Kind | Severity | Summary |
| --- | --- | --- | --- |
| [F1](agent-review-findings.md) | actionable | Medium | List scope controls lose to wake-ups and stay lost |
| [F2](agent-review-findings.md) | actionable | Medium | The D6 "no index" decision rests on latency and a misdescribed access path; the generic plan scans the tenant's whole `(company_id, id, workflow_id)` range per page; no dense or sparse workflow case |
| [F3](agent-review-findings.md) | actionable | Low, may be routed to 03 | Options re-read and discarded on every board leaf reload; agent read unbounded in SQL |
| [F4](agent-review-findings.md) | optional | Low | Workflow entry links untested |

**Unresolved question for root:** F2's remedy choice. Either add the index under the ladder, or defer
with dense and sparse access-path evidence.

**What passes**

- Filters on both views, alone and combined.
- Foreign-id refusal on every view, part and list, and JSON parity.
- The board leaf satisfying NBV-N2.
- Toggle and link scope.
- The agent card link.
- Cut option notes.
- Escaping.
- The board's workflow scope evidence.
- The tripwire fields.

### Process and instruction feedback

**Batch size.** Right-sized: one feature across persistence, routes, pages, tests and docs. Gating D6 as an
architecture decision before implementation would have caught F2 earlier. It is an index decision, which
SKILL.md lists among gate-worthy schema decisions only when costly to unwind. An index added in place is cheap
to unwind, so no gate was strictly required. Its evidence still needed challenge.

**Author records.** Useful: decisions D1–D6 named every judgment call. D6 cited only the persistence rule and
not the ladder plan 02 points at.

**Candidate instructions** (no instruction was edited):

1. **NBV-N2-style notices** (F1). A notice that names one view should be applied to every view with the same
   pattern.
2. **Index decision ladders** (F2). Name the rung by access path, and measure each new filter dimension dense
   and sparse.

### Follow-up to R1 (2026-10-31, recorded in R2)

The R1 evidence for F3 was partly wrong. It called the agent option read "unbounded", but
`list_by_company_id` already had `LIMIT 200` ordered by creation time. The real defect was that this cap made
the cut impossible to detect, so agents past the newest 200 were silently dropped. The R2 fix resolves both;
see F3's resolution history. The R1 outcome stands.

## NBV-P02-B1-R2 — correction recheck, round 2

### Identity

| Field | Value |
| --- | --- |
| Date | 2026-10-31 |
| Workflow | `implement-opus-reviewed-2_2` |
| Records | task [TASK.md](TASK.md) (NBV-P02-D6 r2, "Corrections for R1", Checks); review [REVIEW.md](REVIEW.md); findings [agent-review-findings.md](agent-review-findings.md) |
| Batch / stage | B1, correction recheck |
| Reviewer | B, Opus 5.5/xhigh |
| Phase | NBV-P02 |
| HEAD | `92e8c35e67dfb71e73db87895301dee74f380898` |
| Correction delta | tree `ed59ca7f` → tree `fac0b467e0b7ce01bcfb769617a81f1995649314` |
| Digest | `c5538eb58dccc02187953dbf1ad7351919b6eefc3092b9028744f132c1858142`, exclusions as R1 |

Artifact checks:
- The digest was confirmed at the start and before returning.
- The working tree was confirmed equal to tree `fac0b467` through a temporary index; the real index and the
  worktree were untouched.

Coordination:
- Still r2.
- NBV-N2 is now applied to the list as well (F1).
- NBV-I1: the index is added in place in the init schema, with no migration.

### Coverage and evidence

**Correction delta read in full.** The parts I read:
- the migration line (`init_schema.sql:7322`);
- `AuthoringReads::agent_options` (trait and Postgres implementation);
- `scope_options`, the `reload` field, the `board_html` change and both call sites;
- the list leaf, "Load more" and the form id;
- the board leaf's `hx-vals`;
- route test pins (`board_view_tests.rs`, `runs_tests.rs`) and the authoring links test;
- the new list browser test;
- the `run-list-query-plan.py` workflow cases;
- the "Workflow filter" section of `run-list.md`.

The regenerated `.txt` outputs were not read line by line. They were parsed for index and time changes, below.

**Checks run.** DB-backed checks used `-- --test-threads=4`.

| Check | Result | Log |
| --- | --- | --- |
| `run_board_view_` | 11/11 | `logs/review-r2-run_board_view_.log` |
| `routes::workflows::ui::runs::` | 23/23 | `logs/review-r2-routes__workflows__ui__runs__.log` |
| `persistence::workflow::reads` | 5/5, 1 ignored | `logs/review-r2-persistence__workflow__reads.log` |
| `run_agent_filter_` | 4/4 | `logs/review-r2-run_agent_filter_.log` |
| `node --test scripts/tests/workflow-runs.mjs` | 6/6 | `logs/review-r2-workflow-runs-node.log` |
| `npm run check:css` | exit 0 | `logs/review-r2-check-css.log` |
| Reviewer harness `list-race-p02-r2.mjs` | F1 fixed | `logs/review-r2-list-race-harness.log` |
| Reviewer parse of `logs/qp-list-before.txt` / `qp-list-after.txt` | see below | — |
| Reviewer parse of `qp-board-before.txt` / `qp-board-after.txt` | see below | — |

The query-plan parse covered the index set and execution time per shape. It shows:
- every workflow list shape moves to `workflow_runs_workflow_newest`;
- no non-workflow list shape changes its index set;
- 0 of 198 board shapes change their index set or slow more than 2×.

**Environment observation.** This is a verification gap for later gates, not an artifact defect. The shared test
database `mail_agents_run_board` still records the pre-index checksum of the init migration (`_sqlx_migrations`,
installed 08:30; the stored checksum matches the p01 file). Any DB-backed test that goes through
`test_support::test_pool()` will panic with a version mismatch until that database is reset or recreated, as
NBV-I1 allows. The focused filters above use per-test databases and pass. The full-suite gate in 05 must reset
it first.

**Inspected, not rerun:**
- the author's `r1-*.log`, including `persistence::workflow::` 1011 and `routes::workflows::` 101;
- `clippy-r1.log`;
- the author's query-plan run;
- the author's mutation claim for the list browser test.

### Outcome

**Changes required: one Low actionable item remains.**

| Finding | R2 result |
| --- | --- |
| F1 | fixed and verified |
| F2 | fixed and verified (D6 r2 index, evidence parsed) |
| F3 | fixed and verified; R1 evidence corrected, see the follow-up above |
| F4 | fixed |
| **F6** (new) | **Low, actionable:** the doc says a state change does not write the new index. It does: state changes are never HOT, as the same doc says. Fix the sentence, and run or point to the lease-budget check |
| F5 (new) | optional: `with_options: bool` flag parameter |
| F7 (new) | optional: agent option labels unpinned |

Once F6's sentence is corrected, a recheck needs only that doc line. If root counts F6 as non-blocking
because it is a documentation error with no behaviour change, everything else in this artifact passes.

### Process and instruction feedback

**The correction round was thorough:**
- the access-path evidence was redone before and after with a sparse case;
- the list browser test mirrors the board's.

**One process improvement.** Write the index's write-cost paragraph from the doc's existing HOT analysis rather
than anew. No instruction change is supported beyond that.

## NBV-P02-B1-R3 — correction recheck, round 3

### Identity

| Field | Value |
| --- | --- |
| Date | 2026-10-31 |
| Workflow | `implement-opus-reviewed-2_2` |
| Records | task [TASK.md](TASK.md) ("Corrections for R2"); review [REVIEW.md](REVIEW.md); findings [agent-review-findings.md](agent-review-findings.md) |
| Batch / stage | B1, correction recheck |
| Reviewer | B, Opus 5.5/xhigh |
| Phase | NBV-P02 |
| HEAD | `92e8c35e67dfb71e73db87895301dee74f380898` |
| Delta | tree `fac0b467` → tree `d68a8f1b3e86c4082eef8dd718f7a22edaf470e3`: 3 files, +42/−9 |
| Digest | `d6d1ff4f6c031002d47015c92a1bd859f872ee80f84e813c84e2a1d7d8b7ccb0`, exclusions as R1 |

- **Artifact checks:** the digest was confirmed at the start and before returning. The working tree equals tree
  `d68a8f1b`; I checked this through a temporary index.
- **Coordination:** still r2, with nothing new.

### Coverage and evidence

**Delta read in full**

- The doc sentence in `run-list.md` "Workflow filter".
- `BoardRead` and its two call sites in `routes/workflows/ui/runs/mod.rs`.
- The new agent-option order assertion in `board_view_tests.rs`.

**Checks run**

| Check | Result | Log |
| --- | --- | --- |
| `run_board_view_` (`-- --test-threads=4`) | 11/11 | `logs/review-r3-run_board_view_.log` |
| `routes::workflows::ui::runs::` | 23/23 | `logs/review-r3-routes__workflows__ui__runs__.log` |
| `cargo fmt --check` | exit 0 | `logs/review-r3-fmt.log` |

**Inspected, not rerun:** `logs/lease-budget.log` (130 passed; all work-phase shares under 50%).

**Reused from R2, unchanged by this delta:**

- the F1 list race harness and node tests (6/6);
- the F2 query-plan parse;
- `check:css`;
- the F3 agent options SQL.

**Environment gap, still open for 05:** the shared `mail_agents_run_board` database predates the init-schema
edit and must be reset before any `test_pool()` or full-suite gate. Root recorded this in TASK.md.

### Outcome

**PASS** for digest `d6d1ff4f6c031002d47015c92a1bd859f872ee80f84e813c84e2a1d7d8b7ccb0`.

- F1, F2, F3, F4 and F6 are fixed.
- The optional F5 and F7 were taken.
- No findings are open.

### Process and instruction feedback

No new candidate instructions. The candidates already recorded in R1 and R2 stand.
