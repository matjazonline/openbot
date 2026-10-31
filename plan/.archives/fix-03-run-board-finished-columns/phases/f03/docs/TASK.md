# f03 — fix-03-run-board-finished-columns

## Identity

- Phase ID: `f03` (whole unphased plan `plan/fix-03-run-board-finished-columns/`; IDs
  `F03-G1-R<n>` for the architecture gate, `F03-B1-R<n>` / `F03-B1-F<n>` for the batch).
  Workflow `implement-opus-reviewed-2_2`, skill dir
  `/Users/mac02/.codex/worktrees/b109/openbot/.claude/skills/implement-opus-reviewed-2_2`.
- Record root: `/Users/mac02/.codex/worktrees/b109/openbot/plan/fix-03-run-board-finished-columns/phases/f03/docs/`.
- Coordination: `plan/fix-03-run-board-finished-columns/COORDINATION.md` revision 1, checked at
  startup; no notices.
- Root = A: this Claude Code session, Opus 5.5 / xhigh (user-confirmed 2026-10-30). B: fresh
  reviewer for this phase (`general-purpose`, `model: opus`, `effort: xhigh`): agent
  `aba848d505b69e739`, spawned 2026-10-31 for gate round F03-G1-R1.
- Worktree `/Users/mac02/.codex/worktrees/b109/openbot`, detached HEAD; starting commit and review
  base `3793244` (= `origin/main`, f02's accepted artifact). No preserved pre-existing changes.
- Snapshot exclusions: `plan/fix-03-run-board-finished-columns/phases/f03/docs`
  `plan/fix-03-run-board-finished-columns/COORDINATION.md`.
- Prerequisites: f01 (PASS F01-B1-R3, pushed `0b6cec8`), f02 (PASS F02-B1-R3, pushed `3793244`).
- Context: `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75`. Authorizations: commit and push to `main` once
  accepted. No dev/production database reset; checks use disposable databases.

## Original requirements

`plan/fix-03-run-board-finished-columns/01-run-finished-at.md`, `02-bounded-board-read.md`,
`03-truncation-signal-and-release.md`; repository `AGENTS.md` files. Carried from f01/f02 handoffs:
one shared list of run-state names (F01-B1-F8, F02-B1-F2); `child_rollup` gets the shown cards,
not cap + 1, with `MAX_ROLLUP_PARENTS >= 6 × BOARD_COLUMN_CAP` asserted; board slow-read logging
naming its shape; F02-B1-F6 (the deferred link-table plan must carry lineage); the board's agent
scope must not read child-only versions to their end.

## Acceptance criteria (from the plan)

1. `finished_at` set exactly when a run is terminal (CHECK `workflow_run_finished_shape`), stamped
   by a trigger on entering a terminal state, kept while terminal, cleared on retry; read side
   carries `RunSummary.finished_at`. `run_finished_at_*` tests.
2. Board read: six columns, at most `BOARD_COLUMN_CAP` cards each plus the visible total; finished
   columns windowed by `finished_at` (`BoardWindow`, default week, cutoff from a per-read `as_of`);
   open oldest first, finished newest finished first; counts by `(state, channel_id)`, visibility
   resolved before cards, cards restricted to visible channels, `LIMIT cap + 1`; no transaction
   across the reads, shown total `max(total, shown)`; index by evidence; child counts for shown
   cards. `run_board_read_*` tests.
3. Truncation carried in the read model; `warn_if_run_board_is_slow` (500 ms) with working-set
   fields; the kanban rollup item resolved (done earlier: the plan was removed; f01 recorded where
   its reasoning lives).
4. Focused checks, then `scripts/check.sh` once (and `scripts/lease-budget.sh`: schema change on
   the run state path).

## Batches and gate

- Architecture gate `G1`: the decisions below, revision 1, before any dependent edit — a schema
  invariant on `workflow_runs` written by every state transition, and a read contract spanning
  persistence and HTTP.
- One batch `B1` after the gate passes: the whole plan.

## Architecture gate G1 — decisions, revision 2 (2026-10-31)

Revision 1 is preserved in `logs/G1-r1.md`. Revision 2 answers gate round F03-G1-R1:

- F1–F5 are fixed.
- Optional F6 and F7 are taken.

Changed text is marked **(r2)**.

### G1-D1 `finished_at` (plan 01)

- Column `workflow_runs.finished_at timestamptz` (nullable) in the init schema, and CHECK
  `workflow_run_finished_shape`:
  `(state IN ('succeeded','failed','cancelled')) = (finished_at IS NOT NULL)`.
- Trigger `workflow_run_finished BEFORE INSERT OR UPDATE OF state ON workflow_runs`, function
  `stamp_workflow_run_finished()`:
  - terminal `NEW.state` with a terminal `OLD.state` (UPDATE) → keep `OLD.finished_at`;
  - terminal `NEW.state` otherwise → `clock_timestamp()`;
  - non-terminal → `NULL` (retry clears it).
- **(r2) The trigger name is load-bearing.** Row triggers of one timing fire in name order.
  `workflow_run_revision` advances `revision` whenever the row differs from `OLD`, so the stamp must
  fire before it. Then a write the stamp undoes, such as `SET state = <same terminal>, finished_at
  = NULL`, leaves the revision alone. `workflow_run_finished` sorts before `workflow_run_revision`
  and before `workflow_run_snapshot_immutable`. The schema comment says so, and the test
  `run_finished_at_keeps_the_revision_when_the_stamp_undoes_a_write` pins it.
- **(r2) Snapshot guard.** `guard_workflow_run_snapshot` (`workflow_run_snapshot_immutable`)
  compares the whole row minus its lifecycle columns. `finished_at` is lifecycle state, so it joins
  that exemption list on both sides. Test `run_finished_at_stamps_beside_a_snapshot_column` covers
  `UPDATE … SET state='succeeded', deadline = deadline`, F03-G1-F1's reproduction, which must
  succeed.
- **(r2) Affected contract:** `capture_catalog`
  (`src/adapters/persistence/workflow/action_reconciliation_claim_sql_candidate_support.rs:104-118`)
  pins the BEFORE UPDATE trigger names on `workflow_runs`, and gains `workflow_run_finished`.
- **(r2) Complete trigger inventory.** No BEFORE trigger on `workflow_runs` assigns `NEW.state`:
  - claim retirement, state witness and wakeup allocation read it;
  - the revision trigger assigns only `revision`;
  - the parent, association and snapshot guards only raise;
  - the SQL writer in the init schema (around line 2168) and all 26 persistence `UPDATE … state`
    statements are covered by the trigger without edits.
- Read side moves together (persistence AGENTS §3). **(r2)** `RunRow` becomes a named
  `sqlx::FromRow` struct rather than a 15-element tuple (src/AGENTS.md "Name your tuples").
  `run_summary` maps it, and `RunSummary.finished_at` carries the value. `run_json` gains
  `finished_at`.
- Verification:
  - `run_finished_at_*` DB tests:
    - success, failure and cancel stamp the column;
    - retry clears it and re-stamps later;
    - an UPDATE to terminal that sets `finished_at = NULL` is still stamped;
    - a row violating the CHECK is rejected;
    - the two r2 tests above;
    - **(r2, F5)** for each `RunStateName::ALL`, a run driven into that state has
      `finished_at.is_none() == state.is_open()`. This is the database-backed equivalence of the
      Rust and SQL definitions.
  - The run control suites, the `capture_catalog` users, and `scripts/lease-budget.sh`.

### G1-D2 One list of run-state names (carried F01-B1-F8, F02-B1-F2)

- Application `RunStateName::{Queued, Running, Waiting, Succeeded, Failed, Cancelled}`, in board
  order, with `ALL`, `as_str`, `parse` and `is_open`.
- Used by:
  - the board columns;
  - `child_rollup`'s state match (unknown → `invalid()`);
  - `logged_state`;
  - the UI `state_filter` and the sidebar's `STATES`.
- The SQL terminal set is pinned to it by D1's equivalence test **(r2)**.

### G1-D3 Board read contract (plan 02)

- **Application:**
  - `BoardWindow::{Day, Week, Month}`, default `Week`, with `duration()` and slug `parse`.
  - `RunBoardScope { binding, agent }`: always top-level, no state.
  - `BOARD_COLUMN_CAP = 50`, with `const _: () = assert!(MAX_ROLLUP_PARENTS >= 6 * BOARD_COLUMN_CAP)`.
  - Port methods:
    - `run_board_counts(company, &scope, cutoff) -> Vec<BoardCount { state, channel, runs }>`;
    - `run_board_cards(company, &scope, cutoff, &VisibleChannels { channels, company }) -> Vec<RunSummary>`,
      at most `BOARD_COLUMN_CAP + 1` per state.
- **(r2, F7)** The orchestration lives in its own module, `src/adapters/http/routes/workflows/board.rs`,
  using `bindings::Visibility`. `visible_board(engine, company, actor, scope, window, as_of)` runs:
  1. counts;
  2. visibility once per distinct channel;
  3. cards for the visible channels;
  4. **(r2, F6)** the pure `assemble_columns(counts_visible, cards, cap)`, unit-tested;
  5. `child_rollup` over the shown cards (≤ 300);
  6. the tripwire.
- **(r2, F6) Column totals.** Per column, `cut = cards_read > cap`. `total = max(visible sum,
  cards_read)`, look-ahead included, so a cut column never reads "50 of 50". No transaction spans
  the reads, and the next wake-up reconciles.
- **JSON route** `GET /api/companies/{company_id}/workflow-runs/board?binding_id&agent_id&window`
  is `visible_board`'s production caller and new-board-view's reused read.
  - It authorizes the company first.
  - **(r2, F4)** `binding_id` resolves through `visible_binding`, so a binding on a channel the
    caller cannot see answers 404, as `GET …/workflow-bindings/{id}` does.
  - `agent_id` resolves through `company_agent`.
  - `window` is the enum slug; an unknown value is a 400 query rejection.
  - It returns `{window, cutoff, columns: [{state, total, cut, items: [run_json + children]}]}`.
- **(r2, F4) Route tests,** `run_board_read_route_*`:
  - another tenant's `company_id`, `binding_id` and `agent_id`;
  - a plain member;
  - a binding on a hidden channel (404);
  - a run on a hidden channel, absent from both counts and cards;
  - an unknown `window` (400);
  - `/board` reaching the board, not `/{run_id}`: an authenticated request through the workflows
    router, as the session tests do with `cookie_for_test`.

### G1-D4 Board statements and index (plan 02, step 5)

- **Literal states.** They come from `RunStateName`, so partial indexes apply in generic plans.
  - Counts: the open part `state IN ('queued','running','waiting')` `UNION ALL` the finished part
    `finished_at >= $cutoff`. Both take `parent_run_id IS NULL`, the scope, and
    `GROUP BY state, channel_id`.
  - Cards: six per-state subqueries, each with the visible-channel predicate.
    - Open columns: `ORDER BY created_at, id`.
    - Finished columns: `finished_at >= $cutoff ORDER BY finished_at DESC, id DESC`.
    - **(r2, F7)** The limit is the constant `LIMIT 51` (`BOARD_COLUMN_CAP + 1`), written into the
      text, not bound.
- **Agent scope** is a semi-join over versions. The board reads only the working set, never
  per-version ranges.
- **(r2, F2) Evidence plan.**
  - The dump gains the board statements.
  - Seed changes:
    - terminal runs get `finished_at = created_at + 10 minutes`;
    - runs spread over 32 channels plus company-associated (`channel_id` NULL), so the
      visible-channel predicate is exercised;
    - 10% children stay spread through history, so children fall inside every window.
  - Measured for each of the day, week and month windows, in both plan modes:
    - counts, and cards with all channels visible and with 8 of 32 visible;
    - with no finished index;
    - with (A) `(company_id, finished_at DESC, id DESC) WHERE finished_at IS NOT NULL AND parent_run_id IS NULL`;
    - with (B) `(company_id, state, finished_at DESC, id DESC) WHERE finished_at IS NOT NULL AND parent_run_id IS NULL`.
  - Both candidates carry the top-level predicate, so children in the window are never read.
  - Adopt the candidate under which, in both modes, counts read only the window's top-level
    finished runs (plus open runs through `workflow_runs_open_by_channel`), and each finished
    column's cards stop at 51 matching rows.
  - If both qualify, take the one with fewer buffers summed over the shapes; on a tie, (B), which
    serves cards without a heap state check.
  - The list figures in `docs/performance/run-list.md` are refreshed after the re-run.

### G1-D5 Truncation and tripwire (plan 03)

- `RunBoardColumn { state, cards, total, cut }` as in D3.
- `warn_if_run_board_is_slow` (500 ms on the total) **(r2, F3)** logs:
  - `elapsed_ms` and the per-phase `counts_ms`, `visibility_ms`, `cards_ms`, `rollup_ms`;
  - `company_id`; `binding` and `agent` as booleans; `window`;
  - `eligible_runs` (all counts, hidden channels included);
  - `channels_checked`;
  - `cards_read` (look-ahead included) and `cards_shown`.
- The threshold decision is pure and unit-tested. run-list.md's "Revisit when" reads each phase.
- F02-B1-F6: the deferred link-table plan's design gains lineage (a `top_level` column with a
  partial index).

## Decisions (A, outside the gate)

- A1 — F03-G1-F8 (optional, from F03-G1-R2): two revision-1 lines dropped from revision 2's text
  still apply unchanged, recorded here instead of editing the gated text: (1) a write that sets
  `finished_at` without `state` is allowed while the CHECK holds — only tests do it, to backdate
  runs for window tests; `SET state = <same terminal>` and terminal-to-terminal changes keep the
  first finish (exercised by the r2 revision test); (2) the adopted board index and its plans are
  recorded in `docs/performance/run-list.md` (plan 02 step 5, acceptance criterion 2).
- A2 — `visible_binding` becomes `pub(super)` in `bindings.rs` for `board.rs` (noted by B in R2).
- A3 — **Scope outside an `OFFSET 0` fence (implements D3/D4's "the board reads only the working
  set"; the gated invariant is unchanged, the statement text is not what r2 sketched).** Measured
  with the scope filters in the same `WHERE` as the working set, the planner drove a scoped read
  from `workflow_runs_binding_newest` or from each of the agent's versions
  (`workflow_runs_version_top_level`) and read the scope's history. With (B) in place, generic: the
  dense agent's counts 230,378 buffers, its cards 408,108; the binding's counts all 6,250 of its
  runs (`logs/B1-unfenced-board.md`). Now: counts read `(open UNION ALL window) OFFSET 0` and filter
  by scope outside; scoped card columns read `(column working set ORDER BY … OFFSET 0)` and filter
  outside before `LIMIT 51`; unscoped card columns keep the plain form (stop at 51 index entries).
  Fenced, the dense agent reads 12,652 / 20,480 buffers. Cost: a scope with little history now pays
  the company's working set (sparse agent counts 702 → 12,652 buffers); recorded in run-list.md.
  B to review this explicitly.
- A4 — D4 index selection executed: (B) adopted as `workflow_runs_finished_by_state`
  `(company_id, state, finished_at DESC, id DESC) WHERE finished_at IS NOT NULL AND parent_run_id IS
  NULL`. (A) fails D4's rule under generic plans: scoped card columns read history (750,000+ rows
  filtered, a bitmap over the company's channel key); under auto plans it stays in the window except
  for the recent agent with 8 of 32 channels (170 ms, 299,694 filtered). Corrected after
  F03-B1-F1, which found the first text's "in both plan modes" wrong. (B) meets the rule in both
  modes; "stops at 51" holds for unscoped columns only: a scoped column reads its materialized
  working set (A3). 117 MB for 934,800 top-level finished runs. Evidence
  `docs/performance/run-board-query-plan.txt`.
- A5 — `run_routes<S>()` (`src/adapters/http/routes/workflows/mod.rs`): the `workflow-runs` routes,
  generic over a state holding the engine, merged into `router()`. Lets
  `run_board_read_route_answers_the_board_not_a_run_…` mount the shipped route table over the
  engine with `require_auth` and a `cookie_for_test` session (the full `AppState` is not
  constructible in tests). Same paths and handlers as before.
- A6 — Window decoding: `BoardWindow` is a serde enum (`snake_case`, `#[serde(default)]` week) on
  `BoardQuery` (`deny_unknown_fields`); an unknown slug, a wrong case or an extra key is the
  `Query` rejection, 400. D3's "slug parse" is that decoder.
- A7 — Visibility: the route authorizes the company first, then each distinct counted association
  once through `bindings::Visibility` (the company's runs included, only if any were counted).
  Cards read only channels seen in the counts; a run on a new channel between the reads appears
  on the next read (no transaction, per D3).
- A8 — Test placement and shared helpers: persistence `run_finished_at_tests.rs`,
  `run_board_tests.rs` beside `run_lineage_tests.rs` (`admitted` → `pub(super)`); the production
  failure → retry → cancel path in `control_tests.rs`
  (`run_finished_at_follows_failure_retry_and_cancel`); route tests `run_board_tests.rs` under the
  workflow route tests (`model_agent`, `started`, `agent_source` in `run_filter_tests.rs` →
  `pub(super)`); the tripwire unit test `routes/workflows/board_tests.rs`.
- A3 revision 2 (after F03-B1-R1) — **`WITH … AS MATERIALIZED`, not `OFFSET 0`.** Adding a scope
  with finished runs in the window (F1, A12) showed the `OFFSET 0` fence was not enough: the recent
  agent's counts, auto plan, looped over 13 versions (estimated 1) and re-ran the fenced working
  set for each, 111,055 buffers / 194 ms (`logs/B1-offset0-rescan.md`). A materialized CTE runs at
  most once, so a rescan reads its stored rows. Counts always read `WITH working AS MATERIALIZED
  (open UNION ALL window)` and apply the scope outside; scoped cards materialize each column (six
  CTEs) and filter, sort and cut outside; unscoped cards keep the plain per-state form that stops
  at 51. After: every counts read 8,450–8,880 buffers in both modes, the recent agent 8,575; scoped
  cards read their column working sets (the dense agent's generic cards: 12,677 pages read plus
  57,191 cache hits on the versions key, one probe per working-set row, 43 ms). Text test updated
  (`AS MATERIALIZED (` count, parameter order: scoped cards bind all six columns, then each
  column's scope). B to recheck.
- A12 — Evidence scopes (F1): the board cases now include a binding with finished runs
  (workflow 1, about 60 a week) beside the open-only one (workflow 0), and a "recent" agent
  (`agent1-40`, last versions of workflows 1 and 32) beside the dense and sparse ones, which have no
  runs in any window. run-list.md says which scope holds what; the list section and its evidence
  file are unchanged by these board-only re-runs (`--board-out` only).
- A13 — F03-B1-F2 taken: `run_board_read_counts_a_run_that_arrives_between_its_reads_on_the_next_read`
  (counts → admit → cards → assemble: total 2 = cards read, not cut; the next read counts 2).
- A14 — F03-B1-F3 declined: `RunStateName::is_open` is the one definition of open (an exhaustive
  `const fn`, so a new state is already a compile error there); naming the open states again in
  `child_rollup` would make a second one. The rollup's only other bucket is `failed`, a single
  named state.
- A15 — F03-B1-F4 taken: the focused and lease logs are `.txt`; `check-1.txt` / `check-2.txt` are
  summaries (gate table, totals, failing tests) of the ~1 MB full logs, which stay local
  (`*.log`). A trailing-whitespace strip over `logs/*.txt` also touched B's `review-*.txt`
  (whitespace only) so the commit passes `git diff --check`.
- Candidate instruction improvement (from F03-B1-R1): when evidence changes a gated statement shape
  mid-batch, pause for the reviewer before continuing, even when the gated invariant is unchanged;
  and measure each scope with rows that match it, so a degenerate dataset cannot hide a plan.
- A10 — A caller class the D1 inventory missed: tests that pin whole `workflow_runs` rows and
  list the fields a transition may change. `check-1` failed four of them, each differing only in
  `finished_at` NULL → stamped on a transition into a terminal state the test already allowed
  (`completion_race_completion_first`, `eligibility_exhausted_pending_has_no_retired_attempt`,
  `runtime_mixed_{missing,invalid}_then_same_genuine_result`). Fixed by adding `finished_at` to the
  three terminal allowlists only (`completion_race_assertions::completed`,
  `eligibility_invariant_tests::retirement_delta`, `runtime_mixed_truth_assertions::completed_owners`)
  and asserting it is now set; the waiting and revision-only allowlists are unchanged, so they still
  catch a stamp on a non-terminal write.
- A11 — Plan 03's names: the threshold is `RUN_BOARD_QUERY_WARN_THRESHOLD`; run-list.md's board
  readings add "small working set but slow → pool, contention, visibility" and point a growing
  window at a deferred rollup plan (plan 03 step 3).
- A9 — Evidence script: board shapes in the dump (`family` field), seed `finished_at` and channels,
  board section written to `--board-out` with plans only (`\pset tuples_only`), list figures in
  run-list.md refreshed from the same run.

## Progress

- Gate: F03-G1-R1 changes required (F1 Medium; F2–F5 Low; optional F6, F7); revision 2
  (`logs/G1-r2.md`, sha256 `5b7724e1…3654`) **PASSED** in F03-G1-R2 (optional F8 → A1).
- B1 implemented (2026-10-31), all three plan files:
  - schema: `finished_at`, `workflow_run_finished_shape`, `stamp_workflow_run_finished` /
    `workflow_run_finished` (load-bearing name), snapshot-guard exemption,
    `workflow_runs_finished_by_state`;
  - application: `RunStateName`, `RunSummary.finished_at`, `board.rs` (`BOARD_COLUMN_CAP`,
    `BoardWindow`, `RunBoardScope`, `BoardCount`, `VisibleChannels`, `RunBoardColumn`,
    `assemble_columns`), port methods `run_board_counts` / `run_board_cards`;
  - persistence: `RunRow` named struct, `board_reads.rs`, trait impls, `capture_catalog`;
  - HTTP: `routes/workflows/board.rs` (`visible_board`, `warn_if_run_board_is_slow`,
    `GET …/workflow-runs/board`), `run_routes`, `run_json.finished_at`, UI state filter and sidebar
    on `RunStateName`;
  - docs: run-list.md (board section, refreshed list figures, board "Revisit when"),
    `run-board-query-plan.txt`, `run-list-query-plan.txt`, workflow-operations.md board route,
    deferred link-table plan lineage (F02-B1-F6).
- Status: **accepted in phase** 2026-10-31. B: F03-B1-R1 changes required (F1, F2 Low; F3, F4
  optional), F03-B1-R2 **PASS** for digest
  `42c1472d862a198d1830c1949de8878bb85c5674f4cf4559e3cdae1ec41fa484` (base/HEAD `3793244`,
  exclusions as in Identity). Root recomputed the digest after `check-3` (15/15 PASS): unchanged.
  Coordination revision 1, no notices. Remaining nit (B, no action): run-list.md says "about 60"
  finished runs a week for workflow 1's binding; the evidence shows 52.
- Next: commit and push to `main` (authorized), drop `mail_agents_f03`.

## Check results

Environment: disposable `mail_agents_f03` (dropped and re-migrated after the index was added),
PostgreSQL 18.6, `SQLX_OFFLINE=true RUST_TEST_THREADS=4 TEST_DATABASE_URL=DATABASE_URL=…/mail_agents_f03
cargo test --locked --offline --lib -- <filters>`.

| Command | Result | Log |
| --- | --- | --- |
| `run_finished_at run_board board::tests` (first pass) | 12 new DB tests + unit pass | `logs/B1-focused-1.txt` |
| Route and neighbour suites (`run_board_read_route run_finished_at_follows run_top_level run_agent_filter workflow_listings_hide workflow_routes_refuse workflow_routes_mount`) | 19 passed | `logs/B1-focused-2.txt` |
| Mutation: trigger renamed to sort after `workflow_run_revision` | `run_finished_at_keeps_the_revision…` FAILS (revision 4 → 5); schema restored | `logs/B1-mutation-trigger-name.md` |
| Final schema: `run_finished_at run_board run_top_level run_agent_filter workflow_listings_hide workflow_routes_ workflow_sql_published capture_catalog application::workflow::board routes::workflows::board workflow_control` | 69 passed | `logs/B1-focused-3.txt` |
| `action_reconciliation_claim_sql_candidate routes::workflows::ui::runs control_tests control_acceptance` | 52 passed (UI runs 14, control 38) | `logs/B1-focused-4.txt` |
| `action_reconciliation_claim_sql` (`capture_catalog` user included) | 14 passed | `logs/B1-focused-5.txt` |
| `run_board_queries_bind… run_list_query_binds… application::workflow::board` (no DB) | 7 passed | — |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | clean (after eliding one lifetime in a test helper) | — |
| `scripts/tests/run-list-query-plan.py … --out … --board-out …` | exit 0, no `ERROR` | `docs/performance/*.txt` |
| `RUST_TEST_THREADS=4 DATABASE_URL=…/mail_agents_f03 ./scripts/check.sh` | exit 1: 13/15 PASS; DB suite 2,490 passed, **4 failed** (A10), 6 ignored; stack budget failed on the same 4 | `logs/check-1.txt` (summary; full log local) |
| `scripts/lease-budget.sh --offline` | exit 0; `budget.reserve` 357/1000, `lease.renew` 313/2000, `completion` 214/2000 ms (f02: 358, 309, 215) | `logs/lease-budget-1.txt` |
| After A10: `completion_race eligibility_exhausted runtime_mixed action_reconciliation_eligibility` | 19 passed, the four included | `logs/B1-focused-6.txt` |
| After F03-B1-R1 corrections: `run_board run_finished_at run_board_queries_bind application::workflow::board` | 24 passed (new race test included) | `logs/B1-focused-7.txt` |
| After F03-B1-R1 corrections: `cargo fmt --check`, `cargo clippy … -D warnings` | clean | — |
| After F03-B1-R1 corrections: `run_board run_finished_at run_top_level run_agent_filter application::workflow::board routes::workflows::board workflow_listings_hide routes::workflows::ui::runs` | 51 passed | `logs/B1-focused-8.txt` |
| After F03-B1-R1 corrections: `run-list-query-plan.py … --board-out` (twice: scopes, then materialized statements) | exit 0, no `ERROR` | `docs/performance/run-board-query-plan.txt` |
| F03-B1-R2 digest `42c1472d…a484`: `RUST_TEST_THREADS=4 DATABASE_URL=…/mail_agents_f03 ./scripts/check.sh` | exit 0; 15/15 PASS; 2,495 lib tests passed (6 ignored), again at 2 MiB | `logs/check-3.txt` (summary; full log local) |
| After A10/A11: `RUST_TEST_THREADS=4 DATABASE_URL=…/mail_agents_f03 ./scripts/check.sh` | exit 0; 15/15 PASS; 2,494 lib tests passed (6 ignored), again at 2 MiB | `logs/check-2.txt` (summary; full log local) |
