# f03 review notes — fix-03-run-board-finished-columns

Workflow `implement-opus-reviewed-2_2`. Task [TASK.md](TASK.md); review record [REVIEW.md](REVIEW.md);
findings [agent-review-findings.md](agent-review-findings.md).

## F03-G1-R1 — architecture gate G1, decision revision 1

### Identity

- Date 2026-10-31; workflow `implement-opus-reviewed-2_2`; batch B1, stage **architecture gate G1**
  (before any dependent edit); reviewer Opus 5.5 (`claude-opus-5-5`), spawned as the phase's B with
  effort xhigh per TASK.md (the runtime effort setting is not independently observable by the
  reviewer); phase `f03`; worktree `/Users/mac02/.codex/worktrees/b109/openbot`.
- Review base = HEAD `379324420bb494e1acc69b70c217ddb00172826f` (detached, = f02's pushed artifact).
  Source tree clean; only workflow records untracked (`COORDINATION.md`, `phases/`). No artifact
  digest: nothing is implemented; the reviewed artifact is the decision text.
- Decisions G1-D1 … G1-D5 **revision 1**: immutable copy `logs/G1-r1.md`, sha256
  `a808388d73a19216c11a01c582c0e6967f27222d191143000e1069e8e6f87613`, byte-identical to TASK.md's
  section "Architecture gate G1 — decisions, revision 1" (extracted and diffed, rc 0).
- Coordination `plan/fix-03-run-board-finished-columns/COORDINATION.md` revision 1; plan-wide
  instructions: none beyond plans 01–03 and AGENTS files; no notices.
- Freshness rechecked before return: HEAD unchanged, `git status` shows only the records,
  coordination still revision 1 with no notices, `G1-r1.md` hash unchanged and still equal to
  TASK.md's section.

### Coverage and evidence

- **Requirements read in full:** plan 01, 02, 03; f01 and f02 HANDOFF "Carry into fix-03" items (and
  F02-B1-F5/F6 text in f02's findings for the child-only-version and link-table items);
  new-board-view README and 01–03 (consumption only); AGENTS: root, `src/`, persistence (incl. "Change
  performance only from production evidence", "1. Migration", "Encode queue state machines as
  constraints", "Carry the discriminator", §3), http, application.
- **Schema (read):** `workflow_runs` table and constraints (init schema 5533-5578); indexes
  (7317-7330); every trigger on the table — the 8300-8901 block **and** `guard_workflow_token_reopen`
  (11220) and `workflow_run_snapshot_immutable` (11245, function 11236); every BEFORE trigger
  function body (`workflow_action_capture_claim_retirement`, `…_state_witness`,
  `allocate_workflow_wakeup_sequence`, `advance_workflow_run_revision`, parent/association/rerun/budget
  guards) for writes to `NEW.state` or whole-row comparisons; the SQL-side state writer
  `workflow_action_conflict_park_pending` (2168); `notify_workflow_run_changed`.
- **Writers (grep + read):** all 26 production `UPDATE workflow_runs SET state` statements in
  `src/adapters/persistence` (count matches D1); the admission `INSERT` (default `queued`); retry
  (`control_retry.rs:40`); the cancel cascade (`controls.rs` `cancel_tree_on`/`cancel_run_on`: parent
  then each live child, one statement each, same transaction); tests that disable triggers or use
  `session_replication_role = replica` (none touch run state).
- **Reads/HTTP (read):** `reads.rs` (`RunRow`, `RUN_COLUMNS`, `run_summary`, `run_list_query`,
  `logged_state`, `warn_if_run_list_is_slow`, `child_rollup`); application `reads.rs` (`RunSummary`,
  `RunFilter`, `RunLineage`, `MAX_ROLLUP_PARENTS`, `WorkflowReads`); `bindings.rs` (`Visibility`,
  `visible_to`, `visible_binding`, `run_json`, `RunsQuery`, `company_agent`, `runs`, `run`);
  `ui/runs/mod.rs` (`run_page`, `state_filter`); `LifecycleAuthorizer::authorize` (Company → allowed
  for managers; channel → `viewer_access`); `run_counts.rs` and `attention.rs` (the
  `channel_id = ANY($n) OR (channel_id IS NULL AND $b)` precedent); router (axum 0.8.4 / matchit 0.8.4:
  static `/board` wins over `/{run_id}`); `scripts/tests/run-list-query-plan.py` (TEMP `LIKE … INCLUDING
  ALL` clones copy CHECKs, not triggers; seed has `channel_id` NULL; `CHILD_HISTORY` 30 days back);
  Postgres 18.6 locally and in CI (`.github/workflows/ci.yml`), so btree skip scan exists.
- **Checks run (tested, not inspected):** disposable database `mail_agents_f03_b`, freshly migrated
  from the init schema at `3793244` with `psql -f` (rc 0), D1 applied as written, exercised on a clone
  carrying the same-named row-reading BEFORE UPDATE triggers —
  `logs/review-g1-r1-trigger-experiment.sql` / `.txt`. Results: E1 stamp on entering `failed`; E2
  same-terminal `finished_at = NULL` keeps the stamp and the revision; E3 retry clears it; E5 a
  `finished_at`-only write is allowed on a terminal row (revision bumps), E5b rejected by the CHECK on
  an open row; E6 INSERT into a terminal state stamps over a supplied value; **B0/E4** the snapshot
  guard now rejects a state change that also names a snapshot column at its own value; **E0b** the
  `capture_catalog` query gains `workflow_run_finished`; **E7** a stamp trigger sorting after
  `workflow_run_revision` makes E2 bump the revision. Database dropped afterwards. No cargo or
  DB-backed Rust tests run (nothing implemented).
- **Assumptions challenged:**
  - Concurrent and replayed transitions: row locks plus READ COMMITTED re-evaluation give the BEFORE
    trigger the latest OLD, so a second terminal write keeps the first finish. Inspection only; no
    competing-claimant test is needed for a derived column.
  - Retry clears and re-stamps (E3); the cancel cascade stamps each child separately, and children
    are off the board anyway.
  - Generic plans: the literal open set equals `workflow_runs_open_by_channel`'s predicate, and
    `finished_at >= $cutoff` (a strict operator) implies `finished_at IS NOT NULL`, so the partial
    candidates match in generic plans. Selectivity under generic plans is left to the evidence, which
    measures both modes.
  - Counts/cards authorization: totals are summed only over channels `Visibility` allows; cards are
    restricted to that set; a channel that first appears between the reads is excluded, never shown.
    `Company` visibility follows from the route's own company authorization. Children inherit their
    parent's association (f02 D1). No leak found.
  - Read consistency: one cutoff for both reads, same scope and lineage; the `max(total, shown)`
    race is accepted by the plan (see F6 for a cut/total edge).
  - JSON route: judged justified (it keeps `visible_board` from being dead code under clippy, and the
    same read serves new-board-view 01), with the scoping and test gaps in F4.
- **Reviewed lightly:** D2's call-site list (checked that `logged_state`, `child_rollup`,
  `state_filter`, `STATES` exist as named; did not enumerate every other string match of state names,
  such as `inspection.rs` and `wakeups.rs` domain decodes, which D2 deliberately leaves). D5's
  link-table item (F02-B1-F6) checked only for presence.

### Outcome

- **Changes required** for decision revision 1. Actionable:
  [F03-G1-F1](agent-review-findings.md#f03-g1-f1--d1s-trigger-inventory-omits-the-run-snapshot-guard-and-its-order-does-not-matter-claim-is-false-for-the-revision-trigger)
  (Medium),
  [F03-G1-F2](agent-review-findings.md#f03-g1-f2--d4s-evidence-plan-cannot-show-the-two-working-set-dimensions-that-decide-the-index-lineage-and-channel-visibility)
  (Low),
  [F03-G1-F3](agent-review-findings.md#f03-g1-f3--d5-folds-the-child-rollup-into-the-one-elapsed-time-plan-03-reads-as-working-set-vs-pool-or-visibility)
  (Low),
  [F03-G1-F4](agent-review-findings.md#f03-g1-f4--d3s-json-route-resolves-binding_id-only-company-scoped-and-has-no-route-test-plan)
  (Low),
  [F03-G1-F5](agent-review-findings.md#f03-g1-f5--d2-puts-the-openterminal-split-in-rust-beside-the-sql-one-with-no-equivalence-check)
  (Low). Optional:
  [F03-G1-F6](agent-review-findings.md#f03-g1-f6--optional-maxtotal-shown-can-report-a-cut-column-as-50-of-50),
  [F03-G1-F7](agent-review-findings.md#f03-g1-f7--optional-read-side-shape-notes-for-b1).
- Sound as written: the column/CHECK/trigger semantics (stamp on entering terminal, keep while
  terminal, clear on leaving) and the choice of a trigger over editing 26 writers; the counts → visibility
  → cards split with no spanning transaction; literal states for partial-index matching; the
  `MAX_ROLLUP_PARENTS >= 6 × BOARD_COLUMN_CAP` assertion; carrying all f01/f02 items.
- The recheck should cover the revised D1 (snapshot-guard exemption, ordering statement and its pin,
  catalog contract), D4's evidence plan, D5's log fields, D3's binding resolution and route tests, and
  the D1/D2 equivalence test; the rest of this review can be reused unless those revisions change it.
- No unresolved questions for root beyond the findings.

### Process and instruction feedback

- Gate scope was right: D1 (a schema invariant written by every transition) and D3/D4 (a read
  contract spanning persistence and HTTP, plus an index) are costly to unwind; D2 and D5 are routine
  and could have waited for the batch review, but listing them cost little.
- What helped: D1 named the triggers it relied on and counted the writers, which made the claim
  checkable in minutes; the immutable `G1-r1.md` copy made the decision identity unambiguous.
- What misled: D1's inventory came from the contiguous trigger block, not from the catalog; the
  snapshot guard is defined ~2,400 lines later. The decision's verification sections are uneven: D1
  has one, D2–D5 rely on the plan's tests, which do not cover the new route (F4).
- Candidate instructions (not adopted; evidence above): (1) persistence AGENTS "1. Migration" — list
  a table's triggers from `pg_trigger` on a migrated database before changing the table, and check
  every whole-row comparison (`to_jsonb(NEW)`) for an exemption list the new column belongs in (F1;
  one occurrence). (2) persistence AGENTS performance section — measure a fixed filter (lineage,
  visibility) on data where it drops most rows inside the range read (F2; recurrence of F02-B1-F5).
  (3) Workflow skill, architecture gate — every gated decision states its own verification, including
  route tests for any new route (F4; one occurrence).

## F03-G1-R2 — gate recheck, decision revision 2

### Identity

- Date 2026-10-31; workflow `implement-opus-reviewed-2_2`; batch B1, stage **architecture gate G1
  recheck** (still before any dependent edit); reviewer Opus 5.5 (`claude-opus-5-5`), same B as R1
  (effort xhigh per TASK.md); phase `f03`; worktree `/Users/mac02/.codex/worktrees/b109/openbot`.
- Review base = HEAD `379324420bb494e1acc69b70c217ddb00172826f`; source tree clean, only records
  untracked. No artifact digest (nothing implemented).
- Decisions G1-D1 … D5 **revision 2**: `logs/G1-r2.md`, sha256
  `5b7724e16980e3220a03fc87abbda00df852fb86d2dc956924942e229f833654`, byte-identical to TASK.md's
  section "Architecture gate G1 — decisions, revision 2" (extracted and diffed). Revision 1 unchanged
  (`logs/G1-r1.md`, `a808388d…87613`).
- Coordination revision 1, no notices. Freshness rechecked before return: HEAD, status, coordination
  revision, both gate-copy hashes and the TASK/G1-r2 equality are unchanged.

### Coverage and evidence

- **By finding ID:** F1–F5, plus F6 and F7, which root took. The recorded outcome for each is in
  [agent-review-findings.md](agent-review-findings.md).
- **Whole-revision diff:** I diffed r2 against r1 rather than reading only the **(r2)** marks. That
  found the unmarked removals logged as optional F8.
- **Tested.** On a fresh disposable database (`mail_agents_f03_b`, init schema at `3793244`, dropped
  afterwards) I applied D1 r2 as written. That means the column, the CHECK, `workflow_run_finished`,
  and the snapshot guard with `finished_at` exempted on both sides. Script and output:
  `logs/review-g1-r2-trigger-experiment.sql` / `.txt`.
  - R2-E4: F1's reproduction succeeds and stamps.
  - R2-E4b: a real snapshot change beside a state change is still refused.
  - R2-E2: a same-terminal write with `finished_at = NULL` keeps both the stamp and the revision.
  - R2-E0: in catalog order, `workflow_run_finished` sorts before `workflow_run_revision` and
    `workflow_run_snapshot_immutable`.
- **Inspected:**
  - `preserve_workflow_parent` and `preserve_workflow_run_association` only raise, so r2's inventory
    is complete.
  - Catalog pin tests compare either against the init schema text (`init_function_body`) or a
    before/after snapshot within one test, so editing the guard's body breaks no fixed text. Only
    `capture_catalog`'s name list needs the edit r2 names.
  - `cookie_for_test` exists (`session_test_support.rs:72`), and router-level `oneshot` tests already
    exist, so the `/board` routing test is feasible.
  - `visible_binding` is private to `bindings.rs`; `board.rs` will need it widened (noted under F7).
  - D4's selection rule is decidable from `EXPLAIN (ANALYZE, BUFFERS)` output in both plan modes.
- **Corrected from R1:** R1 asked F2 to measure `$company_visible = false`. That is not a production
  shape: the route authorizes the company association first, so `Visibility` always allows it. R2
  rightly omits it.
- **Reused from R1 (unaffected by r2):** the concurrency, retry, cancel-cascade, visibility-leak,
  read-consistency and generic-plan conclusions.
- **Gaps:** none at gate level. The implementation, the new tests and the index measurements belong
  to the B1 batch review. If no candidate meets D4's rule in both plan modes, that contradicts a gated
  assumption, and the author should pause for B, as the skill requires.

### Outcome

- **PASS for gate G1 decision revision 2.**
- F1–F5 are fixed in the decision. F6 and F7 are taken.
- New optional finding:
  [F03-G1-F8](agent-review-findings.md#f03-g1-f8--optional-revision-2-drops-revision-1-text-without-marking-it-including-d1s-failure-modes)
  (Low). Unmarked removals from r1: D1's failure modes and D4's "record the adopted index in
  run-list.md". It does not block acceptance, because the plan and TASK still require the record and
  the semantics are unchanged.
- B1 batch review must verify:
  - the snapshot-guard exemption and the trigger name/comment in the init schema;
  - the two r2 pin tests and the equivalence test over `RunStateName::ALL`;
  - the `capture_catalog` list;
  - the route tests, including `/board` through the router;
  - `assemble_columns`;
  - the per-phase tripwire fields;
  - the measured index choice against D4's rule;
  - `lease-budget.sh`.

### Process and instruction feedback

- The **(r2)** marks plus the preserved `G1-r1.md` made the recheck quick. The marks alone would have
  hidden the removals (F8). The candidate instruction is recorded in F8.
- R1 over-asked once (the `$company_visible = false` shape). Root's revision correctly did not act on
  it. The evidence: the route-level company authorization makes the company association always
  visible. No instruction change is supported.

### Follow-up (2026-10-31, found at F03-B1-R1)

- G1-D1 r2 claimed a complete inventory of what the new column touches. Its affected contracts named
  `capture_catalog` but missed a caller class: tests that pin whole `workflow_runs` rows and allow a
  transition to change only listed fields. B1's `check-1` failed four of them on `finished_at`
  (TASK.md A10: `completion_race_assertions::completed`,
  `eligibility_invariant_tests::retirement_delta`, `runtime_mixed_truth_assertions::completed_owners`).
  The reviewer's R1 and R2 inventories missed them too: they listed triggers and catalog pins, but
  not row snapshots in assertions. The author's fix was reviewed at F03-B1-R1. It adds `finished_at`
  only to the allowlists of transitions into a terminal state and asserts it is set; the waiting and
  revision-only allowlists are unchanged.
- Candidate instruction (low confidence, one occurrence): when a column is added to a table that
  tests snapshot as JSON (`to_jsonb(row)` / `all_tables`), grep the row-pinning assertions as part of
  the gate's affected contracts.

## F03-B1-R1 — batch review of B1 (whole plan)

### Identity

- Date 2026-10-31; workflow `implement-opus-reviewed-2_2`; batch B1 (the whole plan), stage **batch
  review**; reviewer Opus 5.5 (`claude-opus-5-5`), the phase's B (effort xhigh per TASK.md); phase
  `f03`; worktree `/Users/mac02/.codex/worktrees/b109/openbot`.
- Phase start = review base = HEAD `379324420bb494e1acc69b70c217ddb00172826f`; nothing committed, so
  the batch is the whole working-tree delta: 25 modified and 9 new files. No preserved pre-existing
  work.
- Artifact digest `9b583f28bb6f22a813e513dcf017077e71fc24ecf5ca4a0f445813d465ee63fc` (snapshot.sh;
  exclusions `plan/fix-03-run-board-finished-columns/phases/f03/docs`,
  `plan/fix-03-run-board-finished-columns/COORDINATION.md`). Verified at the start and again before
  return.
- Gate G1 decision revision 2 (`logs/G1-r2.md`, sha256 `5b7724e1…3654`), passed at F03-G1-R2.
  Coordination revision 1, no notices. Freshness rechecked before return.

### Coverage and evidence

- **Requirements, from the originals:**
  - plan 01: column, CHECK, trigger, read side, tests, reset;
  - plan 02: contract (scope, window, order, cap), steps 1–6, the `run_board_read_*` list;
  - plan 03: truncation, tripwire fields and threshold, the "Revisit when" readings, final gate;
  - the f01/f02 carried items;
  - gate r2 D1–D5;
  - new-board-view 01–03 for consumption: `visible_board` is reachable from `routes::workflows::ui`.
- **Code read in full (every changed source file):**
  - init schema diff: column, CHECK, stamp function with the load-bearing-name comment,
    `workflow_runs_finished_by_state`, and the snapshot-guard exemption on both sides;
  - application: `board.rs`, `reads.rs` (`RunStateName`, `RunSummary.finished_at`, port methods);
  - persistence: `board_reads.rs` (fences, literal states, `LIMIT 51` in the text), `reads.rs` (named
    `RunRow` struct, `RUN_COLUMNS`, `logged_state`, `child_rollup`, port impls), `mod.rs`;
  - HTTP: `board.rs` (four-phase trace, visible channels from the counts, `visible_binding`,
    `company_agent`, JSON), `bindings.rs` (`pub(super)`, `run_json.finished_at`), `mod.rs`
    (`run_routes`), UI `state_filter`, pages `STATES`;
  - all new tests and the edited tests (A8, A10), `capture_catalog`;
  - the script diff, run-list.md, workflow-operations.md, and the link-table plan diff.
- **Evidence inspected:**
  - `docs/performance/run-board-query-plan.txt`: (B) and (A) plans for counts, unscoped cards and
    scoped cards in both modes, against A3/A4 and the run-list.md table;
  - the clone index sizes in `run-list-query-plan.txt`;
  - `logs/B1-unfenced-board.md` and `logs/B1-mutation-trigger-name.md`;
  - `check-2.log`: 15/15 PASS, 2,494 passed, run after the last source edit (edit 02:42, log ended
    03:01 after about 18 minutes of suites);
  - `lease-budget-1.log`: exit 0, figures at the f02 levels.
- **Tested by the reviewer:**
  - Disposable `mail_agents_f03_b`, `SQLX_OFFLINE=true RUST_TEST_THREADS=4`, filters `run_finished_at
    run_board board_tests application::workflow::board routes::workflows::board capture_catalog
    action_reconciliation_claim_sql_candidate completion_race eligibility_exhausted runtime_mixed
    workflow_routes_ run_top_level run_agent_filter routes::workflows::ui::runs::`: **60 passed, 0
    failed** (`logs/review-b1-r1-focused.txt`). This covers all `run_finished_at_*`,
    `run_board_read_*` and `run_board_read_route_*` tests, the A10 tests, UI runs, run_top_level and
    run_agent_filter.
  - `claim_sql_multiple_applicable action_reconciliation_claim_sql` (the `capture_catalog` user): **14
    passed** (`logs/review-b1-r1-catalog.txt`).
  - The init schema applied with `psql -f`, rc 0. `\d workflow_runs` shows the column, CHECK, trigger
    and index as written, and the BEFORE trigger order puts `workflow_run_finished` before
    `workflow_run_revision` and `workflow_run_snapshot_immutable`.
  - The database was dropped afterwards.
- **Assessed explicitly (as asked):**
  - **A3 — departs from r2's statement text, satisfies r2's decision.** r2 sketched the scope inside
    each part. The decision it serves is D4's invariant: "the board reads only the working set, never
    per-version ranges", plus plan 02's bound. Measurement showed the sketch broke that invariant: the
    unfenced dense agent read 230,378 / 408,108 buffers through per-version ranges. The `OFFSET 0`
    fence restores it.
    - Every scoped read is now bounded by the company's working set: open runs plus the window. I
      checked the scoped plans in both modes.
    - The cost, a sparse scope paying the company's working set (702 → 12,652 buffers), is
      recorded.
    - The fence is pinned in text (`run_board_queries_bind_…` counts `OFFSET 0) AS working`) and in
      behaviour only by the evidence and the tripwire.
    - Process: this changed gated statement text mid-batch without pausing for B, which the skill asks
      for. It is accepted here on the evidence, so no rework is needed.
  - **A4 — (B) correctly adopted by D4's rule.** (B) reads only the window for counts and stops
    unscoped finished columns at 51 in both modes. (A) reads history for scoped cards in generic mode.
    The record overstates both points (F03-B1-F1); the choice stands.
  - **A5 — satisfies D3 r2.** `run_routes` is the same route table merged into `router()`, with the
    same paths and handlers and `DefaultBodyLimit` still layered over the merge. The test mounts the
    shipped run routes with `require_auth` and a session, and `/board` beats `/{run_id}` there.
    `workflow_routes_mount_in_the_app_router` builds the full router, and a conflict would panic at
    build.
  - **A10 — correct.** Only the three allowlists for transitions into a terminal state gain
    `finished_at`, and each now asserts it is set (one asserts null before). Waiting and
    revision-only pins are unchanged, so they still catch a stamp on a non-terminal write.
- **Also confirmed:**
  - plan 01 stamp/keep/clear, with the production fail → retry → cancel path tested;
  - the round-trip through `run_detail`;
  - the equivalence over `RunStateName::ALL`;
  - the load-bearing name, with a mutation run;
  - the snapshot-guard test, run with the guard enabled on its own database;
  - plan 02: counts by (state, channel) with hidden channels included, visibility once per counted
    channel (company included), cards only from visible channels, `LIMIT 51`, order per contract,
    `total = max(sum, read)`, no transaction, child rollup over shown cards (≤ 300), the
    `MAX_ROLLUP_PARENTS` assertion;
  - route tests: stranger and member 404; foreign binding and agent 404; hidden binding 404; hidden
    runs absent from totals and cards; agent through a child; the window 400s; JSON `finished_at`;
  - plan 03: per-phase timings, `cards_read` / `cards_shown`, the pure threshold unit test, and the
    per-phase "Revisit when" rows;
  - carried items: one state list (`logged_state`, `state_filter`, `STATES`, rollup, board), the
    link-table plan's lineage, `NO_BACKWARD_COMPATIBILITY_NOTES` listing the fix-03 plans;
  - AGENTS rules: no default port methods (one impl); authorization errors propagate; bound params
    plus literals named from `RunStateName` with the reason in the module doc; `RunRow` is a named
    struct.
- **Reviewed lightly:**
  - the refreshed list figures in run-list.md (spot-checked index sizes only);
  - the full suite (relied on `check-2`, not rerun).
- **Gaps:** none blocking. No full-suite rerun by the reviewer.

### Outcome

- **Changes required.** The decision and the code are sound. The remaining defects are the record and
  one test.
- Actionable:
  - [F03-B1-F1](agent-review-findings.md#f03-b1-f1--the-index-record-misstates-the-evidence-a-reads-only-the-window-in-auto-mode-and-b-does-not-stop-scoped-finished-columns-at-51)
    (Low): A4 and run-list.md claim (A) reads history "in both plan modes" (only in generic), and that
    (B) stops scoped finished columns at 51 (it reads their window). The dataset leaves out that the
    measured scopes have no finished runs in any window.
  - [F03-B1-F2](agent-review-findings.md#f03-b1-f2--plan-02s-db-backed-race-case-a-run-inserted-between-the-count-and-card-reads-is-covered-only-by-a-pure-unit-test)
    (Low): plan 02's DB-backed race case is missing.
- Optional:
  - [F03-B1-F3](agent-review-findings.md#f03-b1-f3--optional-the-child-rollups-match-keeps-a-catch-all-so-a-new-run-state-would-be-counted-as-neither-open-nor-failed):
    the rollup's `Some(_)` catch-all.
  - [F03-B1-F4](agent-review-findings.md#f03-b1-f4--optional-the-evidence-logs-taskmd-cites-are-gitignored-and-will-not-be-committed):
    the cited `*.log` evidence is gitignored.
- F03-G1-F8 is resolved via A1.
- **Recheck scope:**
  - F1 is a documentation and TASK edit; its digest change needs only a record check, no tests.
  - F2 adds one DB test; rerun the `run_board_read_` filter.
  - Everything else is reused unless those edits touch code beyond them.

### Process and instruction feedback

- **Batch size:** right. The whole plan was reviewable in one pass. The evidence file (11,875 lines)
  was the heaviest part, and I read it by shape and index rather than in full.
- **Gate value:** G1 caught the snapshot guard and lineage before code. Two misses surfaced only in
  B1:
  - the row-pinning tests (A10; follow-up appended to F03-G1-R2);
  - the unfenced statement shape (A3).

  A3 was handled by a recorded decision outside the gate rather than a mid-batch pause. That worked
  out because the evidence was kept (`B1-unfenced-board.md`). The skill asks for a pause when gated
  text must change. Candidate (low confidence): allow the author to proceed with a recorded,
  evidence-backed amendment when it restores a gated invariant, and flag it for explicit batch-review
  assessment, as root did here.
- **What helped:**
  - the author's A-decisions named exactly what to assess;
  - the unfenced and mutation logs;
  - the board section split into its own evidence file.
- **What misled:** the A4 and run-list.md summaries were written from the generic-mode table only
  (F1).
- **Instruction candidates:** in F1 and F4 (F4 is recurring across f01–f03).

## F03-B1-R2 — correction recheck of B1

### Identity

- Date 2026-10-31; workflow `implement-opus-reviewed-2_2`; batch B1, stage **correction recheck**
  (F03-B1-F1..F4 and the revised A3); reviewer Opus 5.5 (`claude-opus-5-5`), the phase's B; phase
  `f03`; worktree `/Users/mac02/.codex/worktrees/b109/openbot`.
- Base = HEAD `379324420bb494e1acc69b70c217ddb00172826f`, nothing committed. Digest
  `42c1472d862a198d1830c1949de8878bb85c5674f4cf4559e3cdae1ec41fa484` with the same exclusions as R1.
  Verified at the start and before return.
- Gate G1 revision 2 unchanged. Coordination revision 1, no notices; rechecked before return.

### Coverage and evidence

- **By finding:**
  - F1: A4, run-list.md "Run board" (rewritten) and the regenerated
    `docs/performance/run-board-query-plan.txt`. I spot-checked the evidence:
    - (A) auto against generic for scoped cards;
    - the recent agent's (A) auto history read;
    - workflow 1's finished cards (52 in the week);
    - the sparse agent's counts at 8,575 buffers.
  - F2: the new test, read and run.
  - F3: A14's rationale; I accept it (see the finding).
  - F4: `.txt` logs, `git check-ignore`, the TASK.md citations.
- **A3 revision 2, reviewed as a material change:**
  - **Read in full:** `board_reads.rs`. Counts always `WITH working AS MATERIALIZED (open UNION ALL
    window)` with the scope outside. Scoped cards use six `<state> AS MATERIALIZED` column CTEs, then
    per column `WHERE scope ORDER BY … LIMIT 51`. Unscoped cards keep the plain per-state form.
  - **Text pins:** the changed `board_parameters` (all six column CTEs first, then each column's
    scope), and the text test now counting ` AS MATERIALIZED (`.
  - **Evidence:**
    - `logs/B1-offset0-rescan.md`: the `OFFSET 0` subquery re-ran 13 times under a nested loop,
      111,055 buffers.
    - (B) plans for counts and scoped cards in both modes: a single CTE Scan of `working`; the column
      CTEs scanned once each; the agent semi-join as a `workflow_versions_pkey` probe per CTE row.
      Every read is bounded by the company's working set.
  - **Judgement:** same invariant as gate r2 D4 ("reads only the working set, never per-version
    ranges"), with a stronger fence. It satisfies the gate.
- **Tested by the reviewer** on disposable `mail_agents_f03_b` (dropped afterwards):
  - Filters `run_finished_at run_board board_tests application::workflow::board
    routes::workflows::board run_top_level run_agent_filter run_list_query`: **37 passed, 1 ignored**
    (`dump_run_list_query_shapes`). Log: `logs/review-b1-r2-focused.txt`.
  - That run covers every `run_board_read_*`, `run_board_read_route_*` and `run_finished_at_*` test,
    the new race test, and the text test. The scoped card form runs in the DB through
    `run_board_read_leaves_out_children_…` (binding) and `run_board_read_route_scopes_to_an_agent_…`.
  - `cargo fmt --all -- --check`: rc 0.
  - `cargo clippy --locked --offline --all-targets -- -D warnings`: rc 0
    (`logs/review-b1-r2-clippy.txt`).
- **Reused from R1 (code unchanged since):** schema, application, HTTP, route tests, A5, A10, and the
  rest of the R1 coverage. The changed files this round are `board_reads.rs`, `reads_tests.rs`,
  `run_board_tests.rs` (persistence), the script, run-list.md and the board evidence file.
- **Not run by the reviewer:** the full suite. The author's `check-3` for this digest was still
  running and is to be reported at acceptance.

### Outcome

- **PASS for digest `42c1472d862a198d1830c1949de8878bb85c5674f4cf4559e3cdae1ec41fa484`.**
- F03-B1-F1, F2 and F4 are fixed. F3 is dismissed on the author's correct rationale. No new findings.
- Acceptance still needs root to confirm `check-3` (full `scripts/check.sh`) passes on this digest.
  `lease-budget-1` (R1 digest) covers the unchanged trigger path.
- Residual nit inside F1's resolution (no action): "about 60 a week" against a measured 52.

### Process and instruction feedback

- The correction round improved on the record. Measuring scopes that hold matching rows (A12) found
  a real plan defect, the `OFFSET 0` rescan, that my R1 never pointed at. My R1 finding F1 asked only
  for the dataset to be stated, not re-measured. The author's re-measurement is what surfaced it.
  This supports the author's candidate ("measure each scope with rows that match it, so a degenerate
  dataset cannot hide a plan"); evidence: `logs/B1-offset0-rescan.md`.
- My R1 F3 suggestion did not compile in my head: guarded arms are not exhaustive. Lesson: check
  that a suggested code change builds before recording it.
