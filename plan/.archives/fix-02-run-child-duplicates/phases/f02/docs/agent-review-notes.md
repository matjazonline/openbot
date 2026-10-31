# f02 agent review notes

## F02-B1-R1 — batch review of B1 (whole plan)

### Identity

- Date: 2026-10-31. Workflow `implement-opus-reviewed-2_2`. Stage: batch review, B1 = the whole
  plan `plan/fix-02-run-child-duplicates/` (01–03); no architecture gate, so no gated decision IDs.
- Reviewer: B, Claude Opus 5.5 (`claude-opus-5-5`), effort as configured by root (xhigh per
  TASK.md).
- Phase `f02`; worktree `/Users/mac02/.codex/worktrees/b109/openbot`, detached HEAD.
- Phase start = review base = HEAD = `0b6cec8acfda492b321fff817a8b6aad140fbe62`. No commits in the
  batch; reviewed `git diff HEAD` (12 tracked files) plus untracked
  `src/adapters/persistence/workflow/run_lineage_tests.rs`. No preserved pre-existing work.
- Artifact digest `0fad5fee42ad183834c77ddc76250703ccf00f16456c89aacb7f3d87a9a63829`, exclusions
  `plan/fix-02-run-child-duplicates/phases/f02/docs` `plan/fix-02-run-child-duplicates/COORDINATION.md`.
  Confirmed with `scripts/snapshot.sh` at review start and again before returning (see Freshness).
- Records: [TASK.md](TASK.md), [HANDOFF.md](HANDOFF.md), [REVIEW.md](REVIEW.md); findings in
  [agent-review-findings.md](agent-review-findings.md).
- Coordination: `/Users/mac02/.codex/worktrees/b109/openbot/plan/fix-02-run-child-duplicates/COORDINATION.md`
  revision 1, no cross-phase notices, no plan-wide instructions beyond plans 01–03 and the
  `AGENTS.md` files; prerequisite f01 accepted as `0b6cec8` (read its findings F01-B1-F6/F8 for
  the tripwire and run-state precedents).
- Author decisions considered: TASK.md D1–D5. Author evidence inspected:
  `logs/route-top-level-1.log`, `logs/run_top_level-1.log`, `logs/focused-1.log` (via TASK
  table), `logs/check-1.log` (summary: 15/15 PASS incl. DB suite 2,468 passed and 2 MiB stack
  budget).

### Coverage and evidence

Requirements traced from the plan files, not TASK.md's restatement:

- 01.1 named enum, default `Any`: `RunLineage` (`src/application/workflow/reads.rs:187-199`),
  `RunFilter.lineage`; default asserted in `run_top_level_leaves_out_children_and_keeps_reruns`.
  Every `RunFilter` construction found by grep (JSON route, UI list, reads_tests, lineage tests).
  The only `WorkflowReads` impl is Postgres; no mocks to update. **Met.**
- 01.2 SQL `AND parent_run_id IS NULL`: `push_run_page` (`reads.rs:231-233`), so it lands inside
  each per-version LATERAL range when an agent is present (f01 contract unchanged otherwise).
  Clause count asserted per shape in `run_list_query_binds_each_present_filter_once_in_placeholder_order`
  (count only, not position; position follows from the single builder). **Met.**
- 01.3 JSON route `lineage=top_level`: `RunsQuery.lineage` + `unwrap_or_default()`
  (`bindings.rs:454,494`). Wire decoding verified by reviewer outside the tree
  (`logs/review-b1-lineage-wire.log`; axum 0.8.4 `Query` uses serde_urlencoded,
  `extract/query.rs:91`). No in-tree test decodes the text → optional F02-B1-F4. **Met.**
- 01.4 decision on the enum: both variants' doc comments state board = TopLevel, list = Any
  (forensic feed, labels children); UI list sets `RunLineage::Any` explicitly with a comment. **Met.**
- 01.5 shapes + record: `top_level`, `agent_top_level` added to `shapes()`; run-list.md
  "Top-level runs only". Reviewer reproduced the script on its own migrated DB
  (`logs/review-b1-query-plan.txt`); normalized against the committed evidence the plan structure
  and actual row counts are identical, differing only in stats-dependent choice between two
  equivalent `workflow_versions` bitmap indexes, cursor timestamps (seeded from `now()`) and
  index-size stats (`logs/review-b1-query-plan-structure.diff`). Every figure in run-list.md's
  After table and the new section was checked against `docs/performance/run-list-query-plan.txt`
  (e.g. dense top-level 50: 27,956+4,390 = 32,346 buffers, 17.45 ms; without filter 21,718).
  "About 10% children" matches the seed (`n % 10 = 5`). `run-list-query-plan-before.txt` diff is
  empty under `-w` and `--ignore-space-at-eol`: whitespace only. Index name
  `workflow_runs_version_newest` verified in the init schema. **Met**; but the deferral is not
  covered by the tripwire → F02-B1-F1.
- 01 tests `run_top_level_*`: agent-only-in-child (route level, D4), rerun kept, interleaved
  children with page size 1 (would fail under post-read filtering) and cursor continuation,
  default unchanged. **Met.**
- 02.1 `child_rollup(company, &[RunId]) -> HashMap<RunId, ChildCounts>`: one grouped statement,
  `workflow_run_parents` joined on `(company_id, child_run_id)`, grouped by parent and state;
  `MAX_ROLLUP_PARENTS` refusal with `BadRequest` before any I/O (D3's precedent
  `authoring_reads.rs:417-419` verified). Reviewer EXPLAIN on an empty migrated schema with
  seqscan off shows `workflow_run_parents_by_parent` with `parent_run_id = ANY($2)` as index
  condition (`logs/review-b1-rollup-explain.log`; index usability only, not representative cost).
  `workflow_runs_pkey` is `(company_id, id)`. Empty input and duplicate ids are harmless under
  `ANY`. **Met.**
- 02.2 direct children only, stated on `ChildCounts`: the link table holds direct links only
  (PK `(company_id, child_run_id)`, rows written by trigger `insert_workflow_parent_link`). **Met.**
- 02.3 visibility (D1): verified independently — `insert_run` is called only by admission
  `create` (`parent: None`) and `children::create_on` (`children.rs:219-291`), which copies the
  parent's `channel_id`/`thread_id`; its callers are `children.rs:98`, `child_waits.rs:231`,
  `workflow_tool_calls.rs:130` (+ test support); no other non-test `INSERT INTO workflow_runs`;
  trigger `workflow_runs_preserve_association` blocks association updates. Pinned by the route
  test on a channel-bound parent (root `(Some(channel), None)`, child equal). **Met.**
- 02.4 rendering contract total/open/failed: `ChildCounts`; open = queued|running|waiting matches
  the `workflow_run_state` CHECK. Classification style → optional F02-B1-F2. **Met.**
- 02 tests `run_top_level_rollup_*`: none → absent (`idle`, `leaf`, `len()==3`); mixed states
  (queued/running/failed → 3/2/1; succeeded covered in the route test); foreign ids both
  directions; grandchild not on root (chain of 2); 301 refused, 300 accepted. **Met.**
- 03.1 cross-check: route test lists `[child, root]` by agent, `[root]` top-level, root rollup
  `{1,0,0}`, child absent. **Met.**
- 03.2 `the_list_pages_over_runs_an_admin_cannot_see_and_labels_reruns_and_children` unchanged
  (`git diff HEAD -- …/runs_tests.rs` empty) and passing in reviewer run. **Met.**
- 03.3 doc comments: `RunLineage` (board/list), `child_rollup` (direct children, bound),
  `ChildCounts` (grandchildren, visibility settlement). The visibility reasoning sits on the
  returned type rather than the method; acceptable. **Met.**
- 03 final gate: root's `check.sh` 15/15 (not rerun, per assignment); focused gates rerun below.
- AGENTS.md checks: port in application, no default method; `AS` table aliases and named columns;
  bound enforced at the boundary; no schema/SQL-macro change so no sqlx cache change; no new
  concurrency protocol; doc keys match code (`lineage`, `top_level`, `any`).

Assumptions challenged: whether a child can have a different association (no — single writer +
immutability trigger); whether TopLevel could short pages (no — SQL, tested at page size 1);
whether the 300 bound fits fix-03 (yes for shown cards, see handoff note); whether the
regenerated evidence is reproducible (yes, structurally); whether docs figures match evidence
(yes).

Checks actually run by the reviewer (own DB `mail_agents_f02_b`, `SQLX_OFFLINE=true
RUST_TEST_THREADS=4`, serialized; test binary `mail_agents-e5e831b0f75261b3`, not rebuilt — same
fingerprint as the author's runs):

| Command | Result | Log |
| --- | --- | --- |
| `cargo test --locked --offline --lib run_top_level_` | 6 passed | `logs/review-b1-run_top_level.log` |
| `cargo test --locked --offline --lib -- run_agent_filter_ routes::workflows:: workflow::reads::tests run_lineage_tests` | 92 passed, 1 ignored (shape dump) | `logs/review-b1-focused.log` |
| `cargo sqlx migrate run` then `python3 -I scripts/tests/run-list-query-plan.py … --out` | exit 0; structure identical to committed evidence | `logs/review-b1-query-plan.txt`, `logs/review-b1-query-plan-structure.diff` |
| `EXPLAIN` of the rollup statement (generic plan, seqscan off, empty schema) | parent index used with `ANY` | `logs/review-b1-rollup-explain.log` |
| scratch crate: `RunLineage` under serde_urlencoded 0.7.1 | `top_level`/`any`/absent decode; others rejected | `logs/review-b1-lineage-wire.log` |
| `git diff HEAD --check` | clean | inline |

Gaps (stated, not passed silently): no representative-volume EXPLAIN of `child_rollup` (plan does
not require one; cost bounded by 300 parents × children per parent, itself bounded by the root
activation budget); `scripts/check.sh` not rerun (root's log relied on); no persistence-level
agent+TopLevel test (covered at route level per D4); no mutation testing. No part of the batch was
reviewed only lightly: the 1,390-line evidence regeneration was checked by reproduction plus
figure-by-figure comparison rather than line reading.

No unrelated failures observed.

### Outcome

**Changes required** (one Low actionable defect):

- [F02-B1-F1](agent-review-findings.md#f02-b1-f1--the-slow-read-tripwire-cannot-tell-a-top-level-read-though-the-batch-defers-an-index-for-its-filter)
  — actionable, Low: slow-read log and "Revisit when" do not carry lineage although the batch
  defers an index for the top-level filter.
- [F02-B1-F2](agent-review-findings.md#f02-b1-f2--optional-the-rollups-state-classification-ignores-an-unknown-state-silently-and-re-spells-the-run-states)
  — optional, Low: `_ => {}` in the rollup state match; recurrence of F01-B1-F8.
- [F02-B1-F3](agent-review-findings.md#f02-b1-f3--optional-the-rollups-sql-literal-is-one-379-character-line-with-whitespace-runs)
  — optional, Low: 379-char single-line SQL literal.
- [F02-B1-F4](agent-review-findings.md#f02-b1-f4--optional-nothing-pins-the-documented-wire-value-lineagetop_level)
  — optional, Low: no in-tree test decodes `lineage=top_level`.

Recheck scope for F1: the slow-log fields and their unit test, run-list.md "Revisit when", and
`workflow::reads::tests` + `run_top_level_`; all other evidence reusable while code outside that
delta is unchanged.

Handoff notes for fix-03 (not defects of this artifact): (1) pass only the shown cards to
`child_rollup`: reading `BOARD_COLUMN_CAP + 1` per column gives 6 × 51 = 306 > 300 and the rollup
refuses it; the planned compile-time assertion should be `MAX_ROLLUP_PARENTS >= 6 *
BOARD_COLUMN_CAP` with the look-ahead card dropped before the rollup. (2) fix-03's own board
statements will need the same tripwire discipline as F02-B1-F1. (3) F02-B1-F2 can fold into the
F01-B1-F8 consolidation.

### Process and instruction feedback

- Batch size: right — the whole plan was reviewable in one pass; no gate was needed (read-only
  filter and grouped read on established patterns; agree with TASK.md).
- Author records: D1's citations (single writer, callers, immutability trigger) made the
  visibility claim quick to verify; TASK check table matched reruns. D4/D5 rationale was accurate.
  Nothing misleading found.
- Instruction candidate (from F02-B1-F1, scope `src/adapters/persistence/AGENTS.md` performance
  section; evidence F01-B1-F6 + F02-B1-F1, two occurrences on the same tripwire): "When adding a
  filter to a read that has a threshold log, add the filter to the log and its runbook in the same
  change."
- No change supported for F3/F4.

### Freshness

- Start: snapshot digest `0fad5fee…3829`, HEAD `0b6cec8`, coordination revision 1 with no notices.
- Before return: see the dated line appended below.
- 2026-10-31, before return: `snapshot.sh` with the same exclusions → base `0b6cec8`, digest
  `0fad5fee42ad183834c77ddc76250703ccf00f16456c89aacb7f3d87a9a63829` (unchanged); HEAD `0b6cec8`;
  coordination still revision 1, "Cross-phase notices: None". Reviewer database
  `mail_agents_f02_b` dropped; no `mail_agents_own_*` left by reviewer runs (all existing ones
  predate it by OID).

### Follow-up (2026-10-31, from F02-B1-R2)

R1 missed [F02-B1-F5](agent-review-findings.md#f02-b1-f5--a-top-level-agent-read-reads-every-child-only-version-to-its-end-the-record-says-the-filter-does-not-change-the-shape-of-the-work).
R1 checked every run-list.md figure against the evidence and accepted "It does not change the
shape of the work". It did not ask what `Rows Removed by Filter: 31` per loop said about the
seed: 40 child-only versions read to their end. The R1 "Coverage" line for 01.5 ("**Met**") is
therefore superseded for the performance record. The R1 outcome (changes required) stands.

## F02-B1-R2 — correction recheck of B1

### Identity

- Date: 2026-10-31. Workflow `implement-opus-reviewed-2_2`. Stage: correction recheck of B1 (whole
  plan) after F02-B1-R1; no architecture gate.
- Reviewer: B, Claude Opus 5.5 (`claude-opus-5-5`), effort as configured by root (xhigh).
- Phase `f02`; worktree `/Users/mac02/.codex/worktrees/b109/openbot`, detached HEAD.
- Base = HEAD = `0b6cec8acfda492b321fff817a8b6aad140fbe62`; working-tree artifact digest
  `45f033919e017e3f9db1c3852e569bc5b8f587c56a7f41a839199fc242be395c`, exclusions
  `plan/fix-02-run-child-duplicates/phases/f02/docs` `plan/fix-02-run-child-duplicates/COORDINATION.md`.
  Confirmed at start and before return.
- Coordination revision 1, no notices; plan-wide inputs unchanged since R1.
- Inputs: TASK.md "Corrections after F02-B1-R1"; `logs/recheck-1.log`, `logs/clippy-2.log`.

### Coverage and evidence

- Delta identity: file mtimes show only `reads.rs`, `reads_tests.rs`, `run_filter_tests.rs` and
  `run-list.md` changed after R1 (others unchanged since 2026-10-30 23:52 or earlier); the diff
  stat changed only for those four. That matches root's list. Unchanged R1 evidence is reused for
  the rest of the batch.
- F1: `logged_lineage` + warn field + doc (`reads.rs:271-300`), unit test, run-list.md field list and
  two readings, read in full. Fixed as asked; the agent reading is wrong → F5.
- F2: exhaustive arms, fail-closed default (`reads.rs:500-505`). Fixed.
- F3: `\` continuations; checked each continued line keeps a separating space. Fixed.
- F4: `Query::try_from_uri` test read; passes. Fixed (one readability nit noted in the finding).
- New: re-derived the seed's child distribution and the evidence's `Rows Removed by Filter`, then
  measured a child-only version directly (`logs/review-b1-r2-child-only-version.log`) → F5.

Checks run by reviewer (own DB `mail_agents_f02_b`, created, migrated and dropped;
`SQLX_OFFLINE=true RUST_TEST_THREADS=4`):

| Command | Result | Log |
| --- | --- | --- |
| `cargo test --locked --offline --lib -- workflow::reads::tests run_top_level_ run_agent_filter_ routes::workflows::ui::runs::` | 29 passed, 1 ignored (shape dump), including the unchanged list test, both new tests, all `run_top_level_*`, all `run_agent_filter_*` | `logs/review-b1-r2-focused.log` |
| psql experiment: agent page 50, lineage any vs top_level, child-only version of 100,000 runs | 12 vs 5,248 buffers; 0.1 vs 23.9 ms; 100,000 rows removed by filter | `logs/review-b1-r2-child-only-version.log` |
| `cargo fmt --all -- --check`; `git diff HEAD --check` | clean | inline |
| `logs/clippy-2.log` (author's) | inspected: finished, exit 0 per TASK | — |

Gaps: `check.sh` not rerun (not requested). The experiment uses a reduced projection (noted in
the log). It shows the mechanism and how it grows; it is not a production latency figure.

### Outcome

**Changes required.**
- F02-B1-F1: fixed. F02-B1-F2, F3, F4 (optional, taken): fixed.
- [F02-B1-F5](agent-review-findings.md#f02-b1-f5--a-top-level-agent-read-reads-every-child-only-version-to-its-end-the-record-says-the-filter-does-not-change-the-shape-of-the-work)
  — actionable, **Medium**, open. With an agent, `top_level` reads every child-only version to its
  end on every page. run-list.md says the filter does not change the shape of the work, and its
  new runbook row reads it as the plain agent case.
- Recheck scope for F5: run-list.md "Top-level runs only" and the `lineage=top_level, agent=true`
  reading (plus any shape added to the script and regenerated evidence, if the author measures).
  If the author adds an index instead, that is a schema change: migrations, sqlx metadata,
  regenerated evidence and an affected review.

### Process and instruction feedback

- The correction round was well scoped and recorded; root's delta list was exact.
- Reviewer miss: R1 verified numbers but not the mechanism behind a filter's "rows removed"
  count. This is recorded as the F5 instruction candidate. Distinguish the cause: the
  implementation is per plan, and the defect is the evidence interpretation in the record.
- Handoff for fix-03 (root): the board's agent scope + `TopLevel` inherits F5's shape if it uses
  per-version ranges.

### Freshness

- Start: digest `45f03391…395c`, HEAD `0b6cec8`, coordination revision 1, no notices.
- 2026-10-31, before return: `snapshot.sh` with the same exclusions → base `0b6cec8`, digest
  `45f033919e017e3f9db1c3852e569bc5b8f587c56a7f41a839199fc242be395c` (unchanged); HEAD `0b6cec8`;
  coordination revision 1, no notices. Reviewer database `mail_agents_f02_b` dropped. The
  reviewer's test runs left no `mail_agents_own_*` databases: none has an OID above the old
  reviewer database's.

## F02-B1-R3 — correction recheck of B1 (F5, schema change)

### Identity

- Date: 2026-10-31. Workflow `implement-opus-reviewed-2_2`. Stage: correction recheck of B1 after
  F02-B1-R2. It includes a schema change (TASK.md D6), reviewed here as one. There is no separate
  architecture gate; root chose to review the index in this recheck, and I agree for a cheap,
  reversible pre-release init-schema index.
- Reviewer: B, Claude Opus 5.5 (`claude-opus-5-5`), effort as configured by root (xhigh).
- Phase `f02`; worktree `/Users/mac02/.codex/worktrees/b109/openbot`, detached HEAD.
- Base = HEAD = `0b6cec8acfda492b321fff817a8b6aad140fbe62`; artifact digest
  `5ca40e1fa804f8e92b7627e127c453342fde8b1c59b5aaa0d413e2ebaf0bf101`, exclusions
  `plan/fix-02-run-child-duplicates/phases/f02/docs` `plan/fix-02-run-child-duplicates/COORDINATION.md`.
  Confirmed at start and before return.
- Coordination revision 1, no notices. Inputs: TASK.md D6 and "Corrections after F02-B1-R2";
  `logs/check-2.log`, `logs/lease-budget-1.log`, regenerated `docs/performance/run-list-query-plan.txt`.

### Coverage and evidence

- Delta identity: mtimes and `snapshot.sh` show the migration, `reads.rs` (doc comment only),
  `scripts/tests/run-list-query-plan.py`, `run-list-query-plan.txt` and `run-list.md` changed
  after R2. That matches root's list. The other files are unchanged since R2/R1, so their evidence
  is reused.
- Schema change: init schema edited in place with a comment, per persistence `AGENTS.md`
  "1. Migration". Verified on a fresh database (`logs/review-b1-r3-schema.log`). The predicate in
  the query is a literal, so the partial index is matchable in generic plans, and a unit test pins
  the literal. Write cost is documented: 122 MB, plus one write per insert and per state change.
  The lease budget passes. No constraint, FK, concurrency or recovery change. No sqlx cache impact
  (runtime query strings, and check-2's "SQLx metadata matches migrated schema" passes).
- Other ranged reads: a child keeps its parent's binding id (`children.rs:170-174`), so binding
  ranges are never child-only. Company and binding top-level reads interleave children like the
  documented company case. Only version ranges can be child-only, and the index covers those.
- Script: the child-only section's seed matches the main seed's naming (tenant 1, workflow 5,
  version 18, a dense-agent version). The `\gexec` drop matches only the one temp index whose
  definition has `WHERE (parent_run_id IS NULL)`; the shipped partial indexes were checked. The
  second plan uses the full index, which confirms the drop.
- Evidence and docs: every new or refreshed figure was checked against the committed output. The
  whole script was reproduced on my own fresh database (`logs/review-b1-r3-query-plan.txt`,
  `logs/review-b1-r3-query-plan-structure.diff`): plan structure, index choices and actual rows are
  identical. Child-only buffers are identical too (19,630 / 37,584). Timings differ (19.8 vs 13.4 ms
  with the index; 42.8 vs 40.9 without).
- Code doc: the `run_list_query` comment names the partial index for top-level ranges. That is a
  planner choice rather than a guarantee, and run-list.md's runbook covers it ("if the plan has
  stopped using that index, re-measure").

Checks run by reviewer (own DB `mail_agents_f02_b`, fresh, migrated, dropped afterwards;
`SQLX_OFFLINE=true RUST_TEST_THREADS=4`):

| Command | Result | Log |
| --- | --- | --- |
| `cargo sqlx migrate run`; `psql -c '\d workflow_runs'` | both version indexes as written | `logs/review-b1-r3-schema.log` |
| `cargo test --locked --offline --lib -- workflow::reads::tests run_top_level_ run_agent_filter_ routes::workflows::ui::runs::` | 29 passed, 1 ignored | `logs/review-b1-r3-focused.log` |
| `python3 -I scripts/tests/run-list-query-plan.py … --out` | exit 0; structure matches committed evidence | `logs/review-b1-r3-query-plan.txt`, `logs/review-b1-r3-query-plan-structure.diff` |
| `logs/check-2.log`, `logs/lease-budget-1.log` (author's, after last edit) | inspected: 15/15 PASS, 2,470 tests; lease-budget exit 0 | — |

Gaps: `check.sh` and `lease-budget.sh` were not rerun by the reviewer; the author's logs postdate
the last source edit. The agent + state + top-level shape is not measured on its own. It is the
rare-state shape already documented, with the partial index in place of the full one.

### Outcome

- **F02-B1-F5: fixed.** All actionable findings (F1, F5) are fixed, and the optional F2–F4 are
  fixed. The R2 F4 readability nit was left as is by choice, which is fine.
- New [F02-B1-F6](agent-review-findings.md#f02-b1-f6--optional-the-deferred-link-table-plan-does-not-carry-lineage-so-building-it-would-bring-back-f5s-shape-for-top-level-reads)
  — optional, Low: `plan/db_improve/run-agent-link-table.md` does not carry lineage. Not
  acceptance-blocking.
- **PASS** for digest `5ca40e1fa804f8e92b7627e127c453342fde8b1c59b5aaa0d413e2ebaf0bf101`. It covers
  the whole B1 batch: R1 full review, R2 recheck, and R3 schema-change and evidence review, with
  evidence reused only for files unchanged since it was gathered.

### Process and instruction feedback

- D6 recorded the decision, the evidence, the cost and the plan deviation in one place, which made
  the schema recheck quick. The reproducible child-only section in the script is a useful pattern:
  evidence for the index now lives in the measured output, not in the reviewer's side experiment.
- No new instruction candidate beyond F5's.

### Freshness

- Start: digest `5ca40e1f…f101`, HEAD `0b6cec8`, coordination revision 1, no notices.
- 2026-10-31, before return: `snapshot.sh` with the same exclusions → base `0b6cec8`, digest
  `5ca40e1fa804f8e92b7627e127c453342fde8b1c59b5aaa0d413e2ebaf0bf101` (unchanged); HEAD `0b6cec8`;
  coordination revision 1, no notices. Reviewer database `mail_agents_f02_b` dropped. No
  `mail_agents_own_*` database was created after it, so the reviewer's runs left none behind.
