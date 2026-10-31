# f02 agent review findings

Shared context for every finding below unless stated otherwise: workflow
`implement-opus-reviewed-2_2`; task record [TASK.md](TASK.md); review record [REVIEW.md](REVIEW.md);
batch B1 (the whole plan `plan/fix-02-run-child-duplicates/`), stage batch review, review round
[F02-B1-R1](agent-review-notes.md); reviewer B, Claude Opus 5.5 (`claude-opus-5-5`); phase `f02`;
worktree `/Users/mac02/.codex/worktrees/b109/openbot` (detached HEAD); reviewed HEAD/base
`0b6cec8acfda492b321fff817a8b6aad140fbe62` plus working tree, artifact digest
`0fad5fee42ad183834c77ddc76250703ccf00f16456c89aacb7f3d87a9a63829` (exclusions
`plan/fix-02-run-child-duplicates/phases/f02/docs` `plan/fix-02-run-child-duplicates/COORDINATION.md`).
No architecture gate; no finding originates in a gated decision.

## F02-B1-F1 — The slow-read tripwire cannot tell a top-level read, though the batch defers an index for its filter

- **ID and context:** F02-B1-F1, 2026-10-31, found in F02-B1-R1. Context as above.
- **Classification:** actionable defect, **Low**; category observability / repository instructions
  (performance evidence).
- **Evidence:**
  - `docs/performance/run-list.md:95-109` ("Top-level runs only") measures the new filter on
    synthetic data where 10% of runs are children, finds the dense agent reads up to ~50% more
    buffers at a 50-row page because "`parent_run_id` is not in `workflow_runs_version_newest`, so
    each candidate is checked on the heap", and concludes "no index is added for the filter". That
    is a deliberately deferred optimization resting on an assumed child fraction (plan 01 step 5:
    "Children are a small fraction, so no index is expected — record that rather than assume it").
    The company-only top-level read has the same property: `workflow_runs_newest` does not carry
    `parent_run_id`, so a tenant whose recent runs are mostly children walks them on the heap
    (evidence: "Rows Removed by Filter" in `docs/performance/run-list-query-plan.txt` top_level
    shapes).
  - `src/adapters/persistence/AGENTS.md`, "Change performance only from production evidence":
    "When an optimization is deliberately deferred pending production evidence, add a named
    duration or working-set metric, or a threshold log, that can supply the evidence needed to
    revisit it."
  - The only threshold log, `warn_if_run_list_is_slow` (`src/adapters/persistence/workflow/reads.rs:276-296`),
    logs `binding`, `state`, `agent`, `page_size`, `page_rows` — not lineage. The runbook
    `docs/performance/run-list.md:136-157` ("Revisit when") has no reading for top-level reads; a
    slow `agent=true, state=none, page_rows=page_size+1` top-level line would be read as "build
    the deferred link table", and a slow `agent=false, state=none` line has no reading at all.
    Request spans do not carry the query string (f01 F01-B1-F6 evidence, `src/infra/app.rs`), so
    nothing else recovers it.
  - Precedent: F01-B1-F6 (Low, fixed) — the same tripwire could not separate two shapes needing
    different answers.
- **Impact:** if child-heavy tenants make top-level reads slow, the line fires but cannot be
  attributed to the filter whose index this batch deferred, so the decision cannot be revisited
  from the evidence the document says it relies on.
- **Required:** log the lineage on the slow-read line (a closed set such as `lineage = "any" |
  "top_level"`, never caller text), and add a reading for top-level lines to "Revisit when" (e.g.
  slow only with `lineage=top_level` → measure the child fraction / consider `parent_run_id` in the
  keyset index). Keep a unit test over whatever names the field, as
  `the_slow_read_log_names_a_run_state_and_never_the_callers_text` does for `state`.
- **Instruction lesson:** observation: the rule above was applied when the index was deferred in
  f01 but not re-applied when a later plan added a filter to the same read. Candidate (low
  confidence, second occurrence on this tripwire after F01-B1-F6): "When adding a filter to a read
  that has a threshold log, add the filter to the log and its runbook in the same change."
- **Resolution history:**
  - 2026-10-31 (F02-B1-R1): open.
  - 2026-10-31 (F02-B1-R2, digest `45f033919e017e3f9db1c3852e569bc5b8f587c56a7f41a839199fc242be395c`): **fixed** as to what this finding asked. The line now logs
    `lineage = logged_lineage(filter.lineage)` (`reads.rs:271-277,300`), a closed set `any` /
    `top_level` from the enum, never caller text; unit test `the_slow_read_log_names_the_lineage`
    passes (`logs/review-b1-r2-focused.log`); run-list.md lists the field and adds two
    `lineage=top_level` readings. The company-wide reading is sound. The agent reading
    ("read it as the agent rows above … does not change which case applies") is wrong; that is
    tracked separately as [F02-B1-F5](#f02-b1-f5--a-top-level-agent-read-reads-every-child-only-version-to-its-end-the-record-says-the-filter-does-not-change-the-shape-of-the-work),
    because the cause lies in the R1-era "Top-level runs only" conclusion, not in the logging.

## F02-B1-F2 — Optional: the rollup's state classification ignores an unknown state silently and re-spells the run states

- **ID and context:** F02-B1-F2, 2026-10-31, found in F02-B1-R1. Context as above. Recurrence of
  f01's optional F01-B1-F8 (`plan/fix-01-run-agent-query/phases/f01/docs/agent-review-findings.md`),
  which root is carrying into `fix-03-run-board-finished-columns`.
- **Classification:** optional suggestion, Low; category maintainability (`src/AGENTS.md`,
  "Newtypes over bare `String`" last paragraph — statuses parsed once so "a new variant is a
  compile error rather than a silently-passing check" — and "One decision, one place").
- **Evidence:** `child_rollup` (`src/adapters/persistence/workflow/reads.rs:483-490`) matches
  `"queued" | "running" | "waiting" => open`, `"failed" => failed`, `_ => {}`. Today the
  `workflow_run_state` CHECK makes the catch-all reachable only by `succeeded`/`cancelled`, so the
  counts are correct (tested: `run_top_level_rollup_counts_direct_children_by_state`, and the
  route test's succeeded child). A state added later would be counted in `total` but silently in
  neither `open` nor `failed`. The open-state set is now spelled in the rollup, in
  `message_receipts.rs:471`, and in the `workflow_runs_open_by_channel` predicate; the six names
  in `logged_state` (`reads.rs:258`) and `state_filter` (`ui/runs/mod.rs:97`). The adapter already
  has a fail-closed parser, `inspection::run_state` (`src/adapters/persistence/workflow/inspection.rs:66`).
- **Suggested:** name the remaining arms (`"succeeded" | "cancelled" => {}`) and return
  `Err(invalid())` for anything else, as `inspection::run_state` does, or fold the open/failed
  classification into the shared run-state list F01-B1-F8 proposes for fix-03. Not
  acceptance-blocking.
- **Instruction lesson:** none beyond F01-B1-F8; cause: established string-matching pattern in this
  directory.
- **Resolution history:**
  - 2026-10-31 (F02-B1-R1): open (optional).
  - 2026-10-31 (F02-B1-R2, digest `45f033919e017e3f9db1c3852e569bc5b8f587c56a7f41a839199fc242be395c`): **fixed** (suggestion taken). The match now names
    `"succeeded" | "cancelled" => {}` and returns `Err(invalid())` for any other stored state
    (`reads.rs:500-505`, the arm at 503), failing closed like `inspection::run_state`. Behaviour for the six CHECK
    values unchanged; `run_top_level_rollup_*` and the route rollup test pass.

## F02-B1-F3 — Optional: the rollup's SQL literal is one 379-character line with whitespace runs

- **ID and context:** F02-B1-F3, 2026-10-31, found in F02-B1-R1. Context as above.
- **Classification:** optional suggestion, Low; category readability (`src/adapters/persistence/AGENTS.md`,
  "Write SQL for the reader, not for the character count").
- **Evidence:** `src/adapters/persistence/workflow/reads.rs:475` is a single 379-character string
  literal in which each clause is separated by a run of 15–17 spaces (byte dump in review), i.e.
  a multi-line layout whose line breaks were lost. Every other query in the file uses `\`
  continuations (e.g. `list_bindings`, `reads.rs:385-388`). rustfmt does not reflow string
  literals, so `check.sh` formatting passes. The SQL is correct (aliases use `AS`, columns named).
- **Suggested:** restore `\`-continued lines, one clause per line, as the neighbouring queries do.
- **Instruction lesson:** cause unknown (likely an editing-tool artifact); no instruction change
  supported.
- **Resolution history:**
  - 2026-10-31 (F02-B1-R1): open (optional).
  - 2026-10-31 (F02-B1-R2, digest `45f033919e017e3f9db1c3852e569bc5b8f587c56a7f41a839199fc242be395c`): **fixed** (suggestion taken). The literal is back on `\`
    continuations, one clause per line (`reads.rs:484-489`); each continued line keeps its
    separating space before the `\`, so the statement text is unchanged in meaning. Author's
    cause (TASK.md): a Python edit script read the `\`-newline as a Python line continuation.

## F02-B1-F4 — Optional: nothing pins the documented wire value `lineage=top_level`

- **ID and context:** F02-B1-F4, 2026-10-31, found in F02-B1-R1. Context as above.
- **Classification:** optional suggestion, Low; category verification.
- **Evidence:** plan 01 step 3 ("The JSON runs route accepts `lineage=top_level`") and
  `docs/workflow-operations.md` document the query value. The route test
  `run_top_level_lists_a_parent_once_and_rolls_up_its_child` builds `RunsQuery { lineage:
  Some(RunLineage::TopLevel), .. }` and calls the handler with `Query(query)`
  (`run_filter_tests.rs:69-70`), bypassing query-string decoding; no test decodes the text. The
  reviewer confirmed the behaviour outside the tree (`logs/review-b1-lineage-wire.log`: the enum
  and an equivalent `deny_unknown_fields` struct under serde_urlencoded 0.7.1, which axum 0.8.4's
  `Query` uses): `top_level` → `TopLevel`, `any` → `Any`, absent → default, `TopLevel`/empty/other
  → rejected. So behaviour is correct today; a variant rename would change the public value
  silently.
- **Suggested:** one unit test decoding `lineage=top_level` (and rejecting `lineage=bogus`) through
  `axum::extract::Query::<RunsQuery>::try_from_uri`. Not acceptance-blocking.
- **Instruction lesson:** none supported (the f01 route tests share the same direct-call pattern).
- **Resolution history:**
  - 2026-10-31 (F02-B1-R1): open (optional).
  - 2026-10-31 (F02-B1-R2, digest `45f033919e017e3f9db1c3852e569bc5b8f587c56a7f41a839199fc242be395c`): **fixed** (suggestion taken).
    `run_top_level_query_decodes_the_documented_lineage_values` (`run_filter_tests.rs:461-479`)
    decodes through `Query::<RunsQuery>::try_from_uri`: `top_level`, `any`, absent accepted;
    `children` and `TopLevel` refused; passes. Nit (no action needed): its `agent_id=` line
    asserts on `agent_id`, not lineage, and expresses "refused" as `.ok() == None` over an
    `Option<Option<_>>`; `is_err()` would read more plainly.

## F02-B1-F5 — A top-level agent read reads every child-only version to its end; the record says the filter "does not change the shape of the work"

- **ID and context:** F02-B1-F5, 2026-10-31, found in correction recheck
  [F02-B1-R2](agent-review-notes.md) against digest
  `45f033919e017e3f9db1c3852e569bc5b8f587c56a7f41a839199fc242be395c`. The conclusion was already
  present, and missed, at R1 (digest `0fad5fee…3829`, see the R1 follow-up in the notes); the R2
  correction for F02-B1-F1 extended it into the runbook. Other context as at the top of this file.
- **Classification:** actionable defect, **Medium**; category requirements (plan 01 step 5,
  "record that rather than assume it") / performance evidence (`src/adapters/persistence/AGENTS.md`,
  "Change performance only from production evidence": "A bounded result set does not imply
  bounded database work").
- **Evidence:**
  - Mechanism: with an agent, each version that uses it is one range of
    `workflow_runs_version_newest` cut at `limit`; `parent_run_id IS NULL` is a heap filter inside
    that range. A version whose runs are all children never fills `limit`, so it is read to its
    end on every page. A workflow that is only ever called as a child has only such versions, and
    the agent filter matches them by design (plan 01 Context: the child's own version uses the
    agent, and the parent's `agent_ids` includes it). This is plan 01's own acceptance scenario.
  - The committed evidence already shows it: the seed makes every run of workflows 5, 15, 25, 35
    a child (`n % 10 = 5` and workflow `n % 40`, `scripts/tests/run-list-query-plan.py:57-59`),
    i.e. 40 of the dense agent's 400 versions hold only children, about 312 runs each.
    `docs/performance/run-list-query-plan.txt` agent_top_level dense: `Rows Removed by Filter: 31`
    × 400 loops ≈ 12,500 = 40 × 312 — those versions are read in full, at both page sizes. The
    cost looks small only because the synthetic child versions are short.
  - Reviewer experiment (`logs/review-b1-r2-child-only-version.log`; temp clones of the migrated
    tables, one agent on a parent version with 2,000 top-level runs and a child version with
    100,000 child runs; projection reduced to `id, created_at`): page of 50 with lineage `any`
    reads 102 entries, 12 buffers, 0.1 ms; with `top_level` it removes 100,000 rows by filter,
    5,248 buffers, 23.9 ms — growing with the child version's whole history, on every page.
  - The record says otherwise: `docs/performance/run-list.md:107-109` "It does not change the
    shape of the work, so no index is added for the filter", and the new "Revisit when" row
    (`run-list.md:156`): "`lineage=top_level`, `agent=true` | Read it as the agent rows above … it
    does not change which case applies." A slow top-level agent read with a full page (the parent
    versions fill it) would be read as "build the deferred link table". A run-to-agent link table
    does not remove this cost unless it carries lineage; a top-level partial twin of
    `workflow_runs_version_newest` (`WHERE parent_run_id IS NULL`) makes a child-only version's
    range empty.
- **Impact:** the "no index" decision and the runbook rest on a characterization the evidence
  contradicts. `GET …/workflow-runs?agent_id=…&lineage=top_level` reads the full history of every
  child-only version of that agent on every page. fix-03's board always reads `TopLevel` and takes
  an agent scope, so the shape is a design input there too.
- **Required:** correct the record. In "Top-level runs only", state that with an agent every
  version whose runs are all children (any workflow run only as a child) is read to its end on
  every page — the rare-state shape, with lineage in place of state — citing the measured 40 ×
  ~312 rows (and, if wanted, a measured shape with a long child-only version). Rewrite the
  `lineage=top_level`, `agent=true` reading so a slow full page points at child-only versions and
  names the remedy that addresses it (top-level partial twin of the version index; the link
  table only if it carries lineage), or record why the shape is accepted. Whether to add the
  index now is the author's/root's decision under the persistence rules; the record must not
  claim the shape is unchanged.
- **Instruction lesson:** observation: both author and reviewer (R1) checked the figures against
  the evidence but not what the evidence's `Rows Removed by Filter` implied about the seed's
  structure. Candidate (low confidence, one occurrence): "When a filter is added inside a
  per-key ranged read, check what happens to a key whose rows all fail the filter, and measure
  it."
- **Resolution history:**
  - 2026-10-31 (F02-B1-R2): open.
  - 2026-10-31 (F02-B1-R3, digest `5ca40e1fa804f8e92b7627e127c453342fde8b1c59b5aaa0d413e2ebaf0bf101`):
    **fixed**, via TASK.md D6 (schema change, reviewed as one).
    - Index: `workflow_runs_version_top_level ON workflow_runs(company_id, version_id, created_at
      DESC, id DESC) WHERE parent_run_id IS NULL`, edited into the init schema in place
      (`migrations/20260817000000_init_schema.sql:7322-7325`, with a comment), per
      `src/adapters/persistence/AGENTS.md` "1. Migration" (pre-release: edit in place, reset, verify on
      a fresh schema). The reviewer migrated a fresh database and found the index as written
      (`logs/review-b1-r3-schema.log`). The SQL is unchanged. The predicate is the literal
      ` AND parent_run_id IS NULL`, whose presence `run_list_query_binds_each_present_filter_once_in_placeholder_order`
      already asserts, so the partial index stays matchable in generic plans.
    - Evidence: the regenerated output uses the partial index for every `agent_top_level` shape in
      both plan modes and no other shape. Company top-level still uses `workflow_runs_newest`. The
      new child-only section adds 100,000 children to dense version 5-18 and measures:
      19,630 buffers (identical to the plain top-level page) with the index, and 37,584 buffers with
      281 × 400 ≈ 112,400 rows removed by the filter without it. The reviewer reproduced the whole
      script on its own freshly migrated database (`logs/review-b1-r3-query-plan.txt`). Plan
      structure, index choices and actual rows match the committed output, and the buffer counts in
      the child-only section are identical. Only timings, statistics estimates and cursor timestamps
      differ (`logs/review-b1-r3-query-plan-structure.diff`). Every figure in the rewritten
      run-list.md sections and the refreshed After table was checked against
      `docs/performance/run-list-query-plan.txt`.
    - Record: the wrong "does not change the shape" sentence is gone. "Top-level runs only" now
      explains child-only versions, the partial index, and its cost (122 MB for 983,770 top-level
      runs, written on insert and every state change). The `lineage=top_level, agent=true` reading
      now says children are not in the ranges read, and to re-measure if the plan stops using the
      index.
    - Cost and other gates: `logs/lease-budget-1.log` exit 0 at calibration; `logs/check-2.log`
      15/15 PASS with 2,470 tests, including migrations, sqlx metadata and the 2 MiB stack
      budget, run after the last source edit. Reviewer focused rerun on the fresh schema: 29
      passed, 1 ignored (`logs/review-b1-r3-focused.log`).
    - Adding an index on synthetic evidence follows the same precedent as f01's
      `workflow_runs_version_newest`. Without the index the work grows with history rather than
      with the page (persistence `AGENTS.md`: "A bounded result set does not imply bounded
      database work"). The EXPLAIN (ANALYZE, BUFFERS) output and index sizes that rule asks for
      are recorded. The reversal of plan 01 step 5's expected "no index" is recorded as D6.
      Accepted.

## F02-B1-F6 — Optional: the deferred link-table plan does not carry lineage, so building it would bring back F5's shape for top-level reads

- **ID and context:** F02-B1-F6, 2026-10-31, found in correction recheck
  [F02-B1-R3](agent-review-notes.md) against digest
  `5ca40e1fa804f8e92b7627e127c453342fde8b1c59b5aaa0d413e2ebaf0bf101`. Other context as at the top.
- **Classification:** optional suggestion, Low; category requirements (consistency of a deferred
  plan with the contract this batch adds).
- **Evidence:** `docs/performance/run-list.md:174` tells an operator to read a slow
  `lineage=top_level, agent=true` line "as the agent rows above", and one of those rows leads to
  "build the deferred link table". `plan/db_improve/run-agent-link-table.md` ("Design sketch") walks
  `(company_id, agent_id, created_at DESC, run_id DESC)` over every run of the agent's versions and
  joins `workflow_runs` "for binding and state". Nothing there accounts for lineage. A top-level
  page would then step over every child entry of the agent, in proportion to how many children
  each parent starts. `workflow_runs_version_top_level` avoids exactly this today. The file is
  outside this batch's changed files, and its trigger is still unmet.
- **Suggested:** add one line to the deferred plan's design: top-level reads need lineage in the
  link (for example a `top_level` column with a partial index, mirroring
  `workflow_runs_version_top_level`), plus a top-level shape among its measurements. Alternatively,
  root can carry this into fix-03's handoff. Not acceptance-blocking.
- **Instruction lesson:** none supported (one occurrence).
- **Resolution history:**
  - 2026-10-31 (F02-B1-R3): open (optional).
