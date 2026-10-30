# f01 review findings

Shared context for every finding below unless stated otherwise: workflow `implement-opus-reviewed-2_2`;
task record [TASK.md](TASK.md), review record [REVIEW.md](REVIEW.md); batch B1 (whole plan
`fix-01-run-agent-query`, 01–03); stage batch review; review round
[F01-B1-R1](agent-review-notes.md#f01-b1-r1--batch-review-of-b1-whole-plan-fix-01-run-agent-query);
reviewer B, Opus 5.5 (`claude-opus-5-5`); phase `f01`; worktree
`/Users/mac02/.codex/worktrees/b109/openbot` (detached HEAD); reviewed HEAD
`a6dc3bec0f40b3d7200e45198b21e707cc2de8be` + working tree, digest
`6d32795ab478c5a08c6d0bdfe1d139f77cb0fe6b34dde44dd5ce9a538860fae9` (exclusions
`plan/fix-01-run-agent-query/phases/f01/docs` `plan/fix-01-run-agent-query/COORDINATION.md`).
Date 2026-10-30.

## F01-B1-F1 — D4's bound is measured only at 50-row pages; at the default 200-row page the adopted read approaches the rejected one, and the "not with run history" claim is false

- **Classification:** actionable defect, **Medium**; category requirements / verification (decision
  evidence). Originates in author decision D4 (not a gated decision; there was no gate).
- **Evidence:**
  - The adopted agent shape (`run_list_query`, `src/adapters/persistence/workflow/reads.rs:155-195`)
    reads, per page, `Σ over versions using the agent of min(L, runs of that version after the
    cursor)` with `L = page.size + 1`. That is `min(versions × L, the agent's run history)`, not
    "versions × page" independent of history.
  - The default page of the JSON route is `MAX_LISTED` = 200 (`PageRequest::new`,
    `src/application/workflow/reads.rs:33-40`: `size.unwrap_or(MAX_LISTED)`), so `GET
    …/workflow-runs?agent_id=…` without `limit` issues `LIMIT 201`. The UI's `Entries` refresh
    also reads up to 200 rows from the top in one read (`ui/runs/mod.rs:297-303`), which is where
    new-board-view will put the agent filter.
  - Every figure in `docs/performance/run-list.md` and every plan in
    `docs/performance/run-list-query-plan.txt` uses `LIMIT 51` (script `VALUES["limit"] = "51"`).
    Plan 02 step 3's "agent at the deepest page the UI reaches (`MAX_LISTED` = 200)" was taken as a
    50-row page after a cursor at depth 200 — the dimension the old `IN` shape was sensitive to —
    not the page-size dimension the lateral shape is sensitive to.
  - Reproduced with the shipped statements (dump from `dump_run_list_query_shapes`) and the shipped
    script with only `limit` changed to 201, on disposable `mail_agents_f01_b`
    ([logs/B-R1-run-list-query-plan-limit201.txt](logs/B-R1-run-list-query-plan-limit201.txt)):
    dense agent, first page, generic plan: **80,400 rows read** in the nested loop, **82,843
    buffers**, 39.8 ms (custom plan 77 ms); cursor page 41 ms; dense + `failed` 41 ms. The dense
    agent's whole history in that tenant is ~125,000 runs, and the rejected option (same index,
    `IN`) read 127,000 buffers (`logs/option2-experiment.txt`, as summarised in run-list.md). At the
    route's default page the adopted read covers ~64% of the agent's history in the plan's own
    dataset (one version per 36 days, ~312 runs per version).
  - Whenever a version holds no more runs than a page (frequent publication: e.g. ~60 runs per
    version), the read is the agent's **entire** history — exactly the property D4 used to reject
    the `IN` variant ("That cost grows with the agent's runs, so it was rejected").
  - `docs/performance/run-list.md` ("After" section) says "A page now reads at most a page per
    version that uses the agent. That grows with how often those workflows are published, not with
    run history." and TASK D4 says option 3 is "not triggered: no shape that a link table would fix
    still reads run history". Neither holds at `L = 201`, and the plain agent shape (no state) is
    one a `(company_id, agent_id, created_at DESC, run_id DESC)` link table would read in one page.
  - Plan 02 step 4: the link table applies "only when both options above leave a shape reading
    unbounded history … stop and write its plan rather than growing this one". The lateral variant
    is a strict improvement over option 2 as written (never reads more than `IN` + index), and is a
    reasonable engineering choice, but whether "bounded by the agent's version history × page size"
    counts as bounded is the ladder decision 02 asked for, and it was made on evidence that omits
    the default page size.
- **Required:** (1) measure the agent shapes at `LIMIT 201` (the script's `limit` could become a
  per-run list such as 51 and 201) and record them in run-list.md / run-list-query-plan.txt;
  (2) correct the bound statement in run-list.md, the `run_list_query` doc comment
  (`reads.rs:155-163`, "a page reads at most `limit` runs per such version, however long the
  agent's history" is true but should say it reaches the whole history when versions are small)
  and D4; (3) give 02's ladder an explicit disposition on that evidence. If the disposition is
  "the lateral read is enough for now despite the 200-row cost", that is a deviation from 02's
  stop-and-plan rule that root should confirm with the user (or record why 02's condition is not
  met); otherwise stop for the link-table plan as 02 directs. Either way the Revisit-when table
  should mention page size (the log already carries `page_size`).
- **Instruction lesson (observation):** the author re-measured the dimension the *old* shape was
  sensitive to (cursor depth) after changing the shape, not the one the *new* shape is sensitive to
  (page size). Candidate guidance for plans/skill (benchmarking reference): "When a measured
  decision changes the query shape, re-derive which inputs bound its work (page size, fan-out,
  depth) and measure each at the maximum the callers can request, including route defaults."
- **Resolution history:**
  - 2026-10-30 (F01-B1-R1): open.
  - 2026-10-30 (F01-B1-R2, digest `029f6b315d9ca85fc5667d872514f714bd9e804edcf67f500cf98e47df3fb540`): **fixed.** Script measures `LIMIT 51` and `LIMIT 201`
    (`LIMITS`); `docs/performance/run-list-query-plan.txt` re-measured on a freshly migrated
    schema. I checked every figure in run-list.md's After table against it (dense 11.5 / 41.3 ms,
    20,400 / 80,400 rows; depth 200 11.0 / 43.1 ms; sparse 0.27 / 0.43 and 0.13 / 0.15 ms, 201 /
    113 rows; dense + failed 66.1k / 128.0k buffers; dense + queued 124.7k buffers); all match.
    The bound is now stated as "the smaller of (versions × page size) and the agent's whole
    history" in run-list.md, the `run_list_query` doc (`reads.rs:155-165`), revised D4 and the
    deferred plan; no "not with run history"/"however long" text remains (grep). Disposition:
    02's trigger is acknowledged as met, the link-table plan is written
    (`plan/db_improve/run-agent-link-table.md`, carries the no-compat note, listed in
    `src/main.rs` `NO_BACKWARD_COMPATIBILITY_NOTES`) and the batch was not grown with it, which is
    what 02 says ("stop and write its plan rather than growing this one"); building it is deferred
    under persistence `AGENTS.md`'s production-evidence rule, with the threshold log that rule
    requires as its trigger. I accept this disposition. The tripwire's reading of agent + state
    lines has a gap, tracked separately as F01-B1-F6.

## F01-B1-F2 — run-list.md claims the new index is written on insert only and state updates stay HOT; state updates are not HOT, so every state transition adds an entry to it

- **Classification:** actionable defect, **Low**; category documentation correctness / performance
  evidence.
- **Evidence:**
  - `docs/performance/run-list.md` ("After" section): "It is written on insert only: `version_id`
    and `created_at` never change, so state updates stay HOT."
  - `state` is a key column and predicate column of `workflow_runs_attention` and a key/predicate
    column of `workflow_runs_open_by_channel` (`migrations/20260817000000_init_schema.sql:7323,
    7325`). PostgreSQL allows HOT only when no column referenced by a non-summarizing index (keys,
    expressions or predicates) changes, and a non-HOT update inserts a new entry into every
    B-tree index — including `workflow_runs_version_newest`.
  - Reproduced on PostgreSQL 18.6 with a `LIKE public.workflow_runs INCLUDING ALL` temp table
    ([logs/B-R1-hot-update-check.txt](logs/B-R1-hot-update-check.txt)): 5 revision-only updates →
    5 HOT; 5 `queued → running` updates → 0 HOT.
  - Impact: the write cost the new index adds is one index insertion per run insert **and per
    state transition** (several per run lifetime), not per insert only. A's
    `scripts/lease-budget.sh` run (`logs/lease-budget-1.log`) passed, so this is a documentation
    error, not a measured regression; but run-list.md is the record a later index decision reads.
    (Minor in the same paragraph: "about 135 MB per million runs" is 135 MB for 1.096 M runs, ≈123 MB
    per million.)
- **Required:** correct the sentence (state transitions are already non-HOT because of the two
  state indexes, so the new index is written on insert and on every state change), and keep the
  cost statement consistent with the lease-budget evidence.
- **Instruction lesson:** cause unknown (plausible assumption that HOT depends only on the new
  index's own columns). No instruction change proposed from one occurrence.
- **Resolution history:**
  - 2026-10-30 (F01-B1-R1): open.
  - 2026-10-30 (F01-B1-R2, digest `029f6b315d9ca85fc5667d872514f714bd9e804edcf67f500cf98e47df3fb540`): **fixed.** run-list.md now says a state change is never HOT
    (state is in `workflow_runs_attention` and `workflow_runs_open_by_channel`) and every insert and
    state change writes the new index; size restated as 135 MB for 1,096,000 runs (~123 MB per
    million). "`scripts/lease-budget.sh` showed no change" rests on A's R1 `lease-budget-1.log`
    (shares at calibration); not re-run by B.

## F01-B1-F3 — Plan 01 step 5's display-index contract sites are not all updated; D2's reason for skipping the column COMMENT does not hold

- **Classification:** actionable defect, **Low**; category requirements.
- **Evidence:**
  - Plan 01 step 5: "Name the second reader wherever the display-index contract is written:
    `authoring_reads.rs:289`, `src/application/workflow/publication/mod.rs:60`,
    `src/application/workflow/reads.rs:372`, and the column `COMMENT` in the init schema."
  - Updated: `authoring_reads.rs:289-290`, `publication/mod.rs:60-61`. Not updated:
    `src/application/workflow/reads.rs:376-378` (`agent_usage` doc, "Reads the version's
    `agent_ids` display index (p09 gate P09-B1b-D1)") and the column comment
    (`migrations/20260817000000_init_schema.sql:5616`).
  - TASK D2 leaves the COMMENT unchanged because "editing it would change the init-migration
    checksum and force a database reset for no behavioral reason". The same batch already edits
    that file (new index at line 7321), so the checksum changes and the reset is required anyway;
    the stated cost is zero. D2 is silent on `application/workflow/reads.rs:372`.
  - The COMMENT's current text remains true (no execution/admission/authorization path reads it),
    so impact is limited to the plan's explicit requirement and the contract's discoverability.
- **Required:** name the run list's agent filter in the column COMMENT and the `agent_usage` trait
  doc, or restate D2 with a reason that holds and have it accepted as a plan deviation.
- **Instruction lesson:** cause: a deviation rationale that was true in isolation but not for this
  batch. Candidate (low confidence, single occurrence): "When recording a deviation justified by a
  cost, check that the batch does not already incur that cost."
- **Resolution history:**
  - 2026-10-30 (F01-B1-R1): open.
  - 2026-10-30 (F01-B1-R2, digest `029f6b315d9ca85fc5667d872514f714bd9e804edcf67f500cf98e47df3fb540`): **fixed.** Column COMMENT (`init_schema.sql:5616`) now reads
    "Read only by the Used-by listing and the run list's agent filter; no execution, admission or
    authorization path reads it."; `AuthoringReads::agent_usage` doc names `RunFilter::agent`;
    D2 superseded in TASK. Fresh-migration fixtures pass (B-R2 route run) and A's
    `sqlx-prepare-2.log` shows `cargo sqlx prepare --check` passing.

## F01-B1-F4 — Optional: the Used-by route still inlines the check `company_agent` now names

- **Classification:** optional suggestion, Low; category maintainability ("One decision, one place",
  `src/AGENTS.md`).
- **Evidence:** `bindings::company_agent` (`src/adapters/http/routes/workflows/bindings.rs:458-470`)
  is the company-scoped agent existence rule; `ui/authoring/tools.rs:403-408` (`agent_usage`)
  repeats it inline with the same error text. `company_agent` is `pub(super)` in `bindings`, so it
  is visible to `workflows::ui::authoring::tools`.
- **Suggested:** call `bindings::company_agent` from `agent_usage`. Not acceptance-blocking.
- **Resolution history:**
  - 2026-10-30 (F01-B1-R1): open (optional).
  - 2026-10-30 (F01-B1-R2, digest `029f6b315d9ca85fc5667d872514f714bd9e804edcf67f500cf98e47df3fb540`): **fixed (optional taken).** `ui/authoring/tools.rs`
    `agent_usage` calls `bindings::company_agent`; the cross-tenant/member 404 assertions in
    `tools_tests.rs:293-324` and `used_by_lists_…` pass in B's R2 run.

## F01-B1-F5 — Optional: the slow-read line's `returned` counts the look-ahead row

- **Classification:** optional suggestion, Low; category observability.
- **Evidence:** `list_runs` logs `returned = rows.len()` before `paged(rows, page.size)`
  (`reads.rs:410-416`), so a full page with more rows logs `returned = page_size + 1`. run-list.md's
  Revisit-when table reads `returned` against `page_size`.
- **Suggested:** log the page's item count, or say in run-list.md that `returned` includes the
  look-ahead row. Not acceptance-blocking.
- **Resolution history:**
  - 2026-10-30 (F01-B1-R1): open (optional).
  - 2026-10-30 (F01-B1-R2, digest `029f6b315d9ca85fc5667d872514f714bd9e804edcf67f500cf98e47df3fb540`): **fixed (optional taken)**, field renamed `rows_read` and
    documented as "the page plus its look-ahead row". Side effect: run-list.md's After table uses
    "Rows" for version-index entries read (e.g. 80,400), so the log's `rows_read` (≤ 201) and the
    table's "rows read" now mean different things in one document; folded into F01-B1-F6.

## F01-B1-F6 — The slow-read reading for agent + state asks for the state value, which neither the line nor request spans carry; plan 03's `returned` reading was dropped

- **ID and context:** found in correction recheck
  [F01-B1-R2](agent-review-notes.md) against digest
  `029f6b315d9ca85fc5667d872514f714bd9e804edcf67f500cf98e47df3fb540` (introduced by the F1/F5
  corrections; the R1 text used "small `returned`" for this row). Other context as at the top of
  this file.
- **Classification:** actionable defect, **Low**; category requirements / observability.
- **Evidence:**
  - Plan 03 step 2: the Revisit-when section says "slow under an agent filter with a small
    `returned` moves 02's ladder up a step". Plan 03 outcome: the line "carries what is needed to
    reopen 02's decision".
  - `docs/performance/run-list.md:129`: "`agent=true`, `state=true` | The bound above, or the
    rare-state shape; check which state was asked for." No reading uses `rows_read` any more.
  - `plan/db_improve/run-agent-link-table.md:28-31` ("Build it when") counts only `state=false`
    lines and excludes "a line with `state=true` for a rare state such as `queued`".
  - The line carries `state` only as a boolean (`reads.rs:253-273`), and request spans record the
    matched route template, not the query string (`src/infra/app.rs:57-63`, "do not fall back to
    the raw URI"). So an operator cannot "check which state was asked for".
  - The two cases need different answers: dense agent + `failed` at a 200-row page reads its whole
    history (128k buffers, run-list.md After table) and a link table would read about a page;
    agent + `queued` is the rare-state shape no option fixes. As written, the trigger in the
    deferred plan ignores the first case and the reading cannot separate them.
  - `rows_read` can separate them in the common case (a full page = page_size + 1 means enough
    matches, so the per-version bound; fewer than a page means a rare state or the list's end), but
    run-list.md's After table also calls version-index entries "Rows" (`run-list.md:74`), so
    `rows_read` reads as database work in the same document (see F5's R2 note).
- **Required (any one that makes the reading actionable):** restore the `rows_read` reading (full
  page vs fewer than a page) for `state=true` lines in run-list.md and in the deferred plan's
  "Build it when", or log the state when it is one of the known run states (a closed set, not an
  identifier or title; arbitrary caller strings would need mapping to "other"). Consider naming the
  field so it does not collide with the table's "rows read" (e.g. `rows_returned`), or rename the
  table's column.
- **Instruction lesson:** observation: a correction rewrote the reading table and replaced a signal
  the line carries with one it does not. Candidate (low confidence, single occurrence): "Every
  reading in a threshold-log runbook must name only fields the log line carries."
- **Resolution history:**
  - 2026-10-30 (F01-B1-R2): open.
  - 2026-10-30 (F01-B1-R3, digest `3cd0d212c470888aa55b3ede7edf83d1ac5e5f4de7e5019df20d4ed769563f3b`): **fixed.** The line now logs `state` as one of the six run
    states (matching the `workflow_run_state` CHECK), `none`, or `other` (`logged_state`,
    `reads.rs:249-262`), so caller text is never logged; unit test
    `the_slow_read_log_names_a_run_state_and_never_the_callers_text` covers all six, `none`, a
    newline-injection string and case. `rows_read` → `page_rows`, documented in code and in
    run-list.md as rows returned (page + 1 when full, fewer when matches ran out) and explicitly not
    the index entries the tables count; no `rows_read` remains (grep). Revisit-when now separates
    agent + none/terminal state + full page (link table) from agent + open state + short page
    (rare-state shape), and the deferred plan's "Build it when" uses the same reading, so agent +
    a common state counts. Plan 03's literal "small `returned` moves the ladder up" is now "full
    page moves it up": accepted, because under the adopted design the small-returned case
    (sparse agent walking history) was the step the version index already took, and a short page
    means the matches ran out, which no remaining option fixes.

## F01-B1-F7 — The new deferred plan has no per-batch checks or final full-suite gate

- **ID and context:** found in correction recheck [F01-B1-R2](agent-review-notes.md) against digest
  `029f6b315d9ca85fc5667d872514f714bd9e804edcf67f500cf98e47df3fb540`. Other context as at the top.
- **Classification:** actionable defect, **Low**; category requirements (repository rule).
- **Evidence:** root `AGENTS.md`, "Keep verification focused during implementation": "Implementation
  plans must specify per-batch focused checks and a final full-suite gate." 
  `plan/db_improve/run-agent-link-table.md` "Verification" (lines 49-55) lists an equal-pages oracle
  test, the query-plan script and `scripts/lease-budget.sh`, but no final gate (`scripts/check.sh`:
  formatting, offline compilation, migrations, sqlx metadata, database-backed suite). It adds a
  table, an admission-path write and a composite FK, so the full suite is the relevant gate.
  (The older deferred notes in `plan/db_improve/` do not carry one either; this is the first one
  written as a buildable design.)
- **Required:** add the focused checks (e.g. the `run_agent_filter_` tests and the equal-pages
  oracle) and a final `scripts/check.sh` gate with "record commands and results" to the plan's
  Verification section.
- **Instruction lesson:** cause unknown; the rule applies to plans but deferred plans were written
  as sketches. No change proposed from one occurrence.
- **Resolution history:**
  - 2026-10-30 (F01-B1-R2): open.
  - 2026-10-30 (F01-B1-R3, digest `3cd0d212c470888aa55b3ede7edf83d1ac5e5f4de7e5019df20d4ed769563f3b`): **fixed.** `plan/db_improve/run-agent-link-table.md`
    Verification names per-batch focused checks (schema/write `run_agent_links_*` tests incl. a
    cross-tenant composite-key rejection, equal-pages oracle against the current read, the existing
    `run_agent_filter_` and `routes::workflows::` suites, evidence script, lease budget) and a final
    `scripts/check.sh` gate with "record commands and results, and name any gate that could not
    run".

## F01-B1-F8 — Optional: the six run-state names are now listed twice in Rust adapters

- **ID and context:** found in correction recheck [F01-B1-R3](agent-review-notes.md) against digest
  `3cd0d212c470888aa55b3ede7edf83d1ac5e5f4de7e5019df20d4ed769563f3b`. Other context as at the top.
- **Classification:** optional suggestion, Low; category maintainability ("One decision, one
  place", `src/AGENTS.md`).
- **Evidence:** `logged_state` (`src/adapters/persistence/workflow/reads.rs:249-262`) and
  `state_filter` (`src/adapters/http/routes/workflows/ui/runs/mod.rs:95-104`) each spell
  `queued | running | waiting | succeeded | failed | cancelled`; the database CHECK
  `workflow_run_state` is the authority. A state added later would be logged as `other` (harmless)
  and silently rejected by the UI filter.
- **Suggested:** one shared list (e.g. a `RUN_STATES` const beside `RunFilter`) used by both. Not
  acceptance-blocking.
- **Resolution history:**
  - 2026-10-30 (F01-B1-R3): open (optional).
