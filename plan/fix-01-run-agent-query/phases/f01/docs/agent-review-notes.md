# f01 review notes

## F01-B1-R1 — batch review of B1 (whole plan fix-01-run-agent-query)

### Identity

- Date: 2026-10-30. Workflow `implement-opus-reviewed-2_2`. Stage: batch review (B1 = plan 01–03,
  no architecture gate). Reviewer: B, Opus 5.5 (`claude-opus-5-5`), effort as configured by root
  for this agent (not independently observable from inside the run).
- Records: task [TASK.md](TASK.md), review [REVIEW.md](REVIEW.md), findings
  [agent-review-findings.md](agent-review-findings.md) (this directory).
- Phase `f01`, worktree `/Users/mac02/.codex/worktrees/b109/openbot`, detached HEAD.
- Phase start = review base = HEAD = `a6dc3bec0f40b3d7200e45198b21e707cc2de8be`; no commits in the
  batch; reviewed `git diff HEAD` plus untracked files.
- Artifact digest `6d32795ab478c5a08c6d0bdfe1d139f77cb0fe6b34dde44dd5ce9a538860fae9`, exclusions
  `plan/fix-01-run-agent-query/phases/f01/docs` `plan/fix-01-run-agent-query/COORDINATION.md`;
  confirmed with `scripts/snapshot.sh` at review start and again before returning (see Outcome).
- Coordination: `plan/fix-01-run-agent-query/COORDINATION.md` revision 1, no cross-phase notices,
  no plan-wide instructions beyond the plan files and `AGENTS.md`; re-read before returning,
  unchanged. Gated decision IDs: none. Author decisions examined: TASK D1–D4.
- Retained evidence used: TASK "Check results" and `logs/` (`run_agent_filter-1.log`,
  `routes-workflows-1.log`, `check-1.log` summary, `lease-budget-1.log` cited only,
  `run-list-query-plan-before.txt`, `option2-experiment.txt`), `docs/performance/*` in the artifact.

### Coverage and evidence

- Requirements read from the originals: plan `01-agent-run-filter.md`,
  `02-query-plan-evidence-and-index.md`, `03-slow-read-tripwire-and-release.md`; `AGENTS.md`,
  `src/AGENTS.md`, `src/adapters/persistence/AGENTS.md`, `src/adapters/http/AGENTS.md`,
  `src/application/AGENTS.md`.
- Files read in full (diff + new files): `application/workflow/reads.rs` (RunFilter.agent),
  `persistence/workflow/reads.rs` (`run_list_query`, `push_run_page`, tripwire, `list_runs`),
  `reads_tests.rs`, `http/routes/workflows/bindings.rs` (`RunsQuery.agent_id`, `company_agent`,
  `runs`), `run_filter_tests.rs`, `tests.rs` registration, `ui/runs/mod.rs`, the two comment edits,
  the init-schema index, `scripts/tests/run-list-query-plan.py`, `docs/performance/run-list.md`,
  both plan outputs (section timings and the agent plans in both modes), `docs/workflow-operations.md`.
- Callers/contracts traced: every `RunFilter` constructor (JSON route, UI `run_filter`, tests);
  every `WorkflowReads::list_runs` caller (JSON route, UI `run_page`); `PageRequest` bounds
  (default `MAX_LISTED` = 200); UI paging (`LIST_PAGE` 50, `Entries` refresh up to 200);
  `agent_tool_grants` SQL (company-scoped) and `AgentPersistence::delete` (hard delete); the Used-by
  route; all `workflow_runs` indexes and triggers; `NO_BACKWARD_COMPATIBILITY_NOTES` already lists
  the three plan files.
- Behaviour checked by inspection: SQL of all six shapes (dumped from the shipped builder); keyset
  correctness of per-version cut + merge (global top-L = top-L of per-group top-L; cursor applied
  in each range; one version per run so no duplicates); both statement halves company-scoped;
  cursor lookup company-scoped; authorization before agent lookup (no existence oracle);
  `version` interpolation is a `&'static str` correlation reference, all values bound; tripwire
  logs only kinds and identifiers; decision `is_slow` pure.
- Assumptions challenged: D1 (dump test instead of named consts) — accepted, stronger guarantee;
  D2 — rationale void (F3); D3 — accepted, `agent_tool_grants` is `WHERE company_id AND id`;
  D4 — the lateral read is a strict improvement over 02's option 2 as written, but its bound was
  measured only at 50-row pages and is overstated (F1); agent + rare state "no option fixes it"
  — accepted for the realistic regime (fewer matches than a page); HOT claim — false (F2).
- Checks run by B (disposable `mail_agents_f01_b`, `SQLX_OFFLINE=true RUST_TEST_THREADS=4`,
  `TEST_DATABASE_URL`=`DATABASE_URL`; route fixtures use `own_database`):
  - `cargo test --locked --offline --lib run_agent_filter_` → 4 passed
    ([logs/B-R1-run_agent_filter.log](logs/B-R1-run_agent_filter.log)).
  - `cargo test --locked --offline --lib workflow::reads::tests` → 2 passed, 1 ignored (dump)
    ([logs/B-R1-reads-tests.log](logs/B-R1-reads-tests.log)).
  - `cargo test --locked --offline --lib routes::workflows::` → 83 passed
    ([logs/B-R1-routes-workflows.log](logs/B-R1-routes-workflows.log)).
  - `DATABASE_URL=… SQLX_OFFLINE=false cargo sqlx migrate run` on `mail_agents_f01_b`; `\d
    workflow_runs` shows `workflow_runs_version_newest`.
  - Shipped script with only `limit` 51→201 (scratchpad copy, shapes from
    `dump_run_list_query_shapes`) → exit 0
    ([logs/B-R1-run-list-query-plan-limit201.txt](logs/B-R1-run-list-query-plan-limit201.txt)).
  - HOT check on a `LIKE public.workflow_runs INCLUDING ALL` temp table
    ([logs/B-R1-hot-update-check.txt](logs/B-R1-hot-update-check.txt)).
- Not run by B (per assignment or not needed): `scripts/check.sh` (root's `logs/check-1.log`
  summary inspected: 15/15 PASS), `scripts/lease-budget.sh` (A's log cited, not re-run), the
  shipped script at `LIMIT 51` (A's committed output inspected instead), clippy/fmt.
- Lightly reviewed: `docs/workflow-operations.md` wording only; the `before` output beyond the
  timing table; the option-2 experiment beyond buffer totals (its scratch script is not retained in
  the artifact, only its output).

### Outcome

- **Changes required.** Actionable: [F01-B1-F1](agent-review-findings.md#f01-b1-f1--d4s-bound-is-measured-only-at-50-row-pages-at-the-default-200-row-page-the-adopted-read-approaches-the-rejected-one-and-the-not-with-run-history-claim-is-false)
  (Medium), [F01-B1-F2](agent-review-findings.md) (Low), [F01-B1-F3](agent-review-findings.md)
  (Low). Optional: F01-B1-F4, F01-B1-F5.
- Functional behaviour of the filter, route scoping, tests and tripwire: no defects found.
- Unresolved question for root/user (F1): whether "bounded by the agent's version history × page
  size" satisfies 02's ladder, or 02's stop-and-plan rule for the link table applies.
- Digest/coordination freshness rechecked before returning: unchanged.

### Process and instruction feedback

- Batch size was reviewable in one round (≈180 changed lines of Rust, two new test files, one
  script, docs). No architecture gate was needed for the filter itself; in hindsight the index
  *shape* decision (D4) changed the plan's prescribed query and its ladder outcome — a cheap
  pre-implementation gate would not have helped because D4 came from measurement, but a
  mid-batch pause ("assumption contradicted") would have been the place to ask root/user about
  deviating from 02's ladder.
- Author records were useful: D1–D4 stated deviations explicitly and the before/option-2/after
  evidence was retained, which made F1 verifiable in minutes.
- Candidate instruction (benchmarking reference / plan template, evidence F1): "When a measured
  decision changes the query shape, re-derive what bounds its work (page size, fan-out, depth) and
  measure each at the maximum callers can request, including route defaults."
- Observation (cause unknown, not attributed to this batch): 16 `mail_agents_own_*` databases
  were present at review start; `own_database` drops on handle drop, so these are likely from
  aborted runs or other sessions. Not investigated.
- Freshness recheck before returning (2026-10-30): HEAD `a6dc3be`, digest
  `6d32795ab478c5a08c6d0bdfe1d139f77cb0fe6b34dde44dd5ce9a538860fae9` unchanged; COORDINATION.md
  revision 1, no notices. Reviewer database `mail_agents_f01_b` dropped; `mail_agents` and
  `mail_agents_f01` not touched.

## F01-B1-R2 — correction recheck of B1 (F01-B1-F1 … F5)

### Identity

- Date: 2026-10-30. Workflow `implement-opus-reviewed-2_2`. Stage: correction recheck of batch B1.
  Reviewer: B, Opus 5.5 (`claude-opus-5-5`), same agent as R1. Records: [TASK.md](TASK.md)
  ("Corrections after F01-B1-R1", revised D2/D4), [REVIEW.md](REVIEW.md),
  [agent-review-findings.md](agent-review-findings.md).
- Phase `f01`, worktree `/Users/mac02/.codex/worktrees/b109/openbot`, detached HEAD; base = HEAD =
  `a6dc3bec0f40b3d7200e45198b21e707cc2de8be`; no commits.
- Artifact digest `029f6b315d9ca85fc5667d872514f714bd9e804edcf67f500cf98e47df3fb540`, same
  exclusions as R1; confirmed with `scripts/snapshot.sh` at start and before returning.
- Coordination: COORDINATION.md revision 1, no notices (checked at start and end). No gated
  decisions. Previous round: F01-B1-R1 (digest `6d32795a…`).

### Coverage and evidence

- Correction delta read in full: `reads.rs` (`run_list_query` doc, `rows_read`), application
  `reads.rs` (`agent_usage` doc), init-schema COMMENT, `ui/authoring/tools.rs`, script `LIMITS`,
  rewritten `docs/performance/run-list.md`, re-measured `run-list-query-plan.txt` (every After-table
  figure cross-checked), new `plan/db_improve/run-agent-link-table.md`, `src/main.rs` list entry,
  TASK revised D2/D4 and corrections section.
- Broadened where the corrections touched contracts: tripwire readings vs. what the line and
  request spans carry (`src/infra/app.rs:57-63`); deferred plan vs. root `AGENTS.md` plan rules and
  persistence `AGENTS.md` (composite FK, production-evidence deferral, threshold log);
  `insert_run` reference exists (`admission_write.rs:94`); Used-by cross-tenant tests
  (`tools_tests.rs:293-324`).
- Challenged: F1 disposition (02's "stop and write its plan rather than growing this one" vs.
  shipping the lateral read and deferring the link table) — accepted, reasoning in F1's R2 entry.
- Checks run by B on disposable `mail_agents_f01_b` (`SQLX_OFFLINE=true RUST_TEST_THREADS=4`):
  - `cargo test --locked --offline --lib -- routes::workflows:: workflow::reads::tests` → 85
    passed, 1 ignored, incl. 4 `run_agent_filter_` and `used_by_lists_…`
    ([logs/B-R2-routes-workflows.log](logs/B-R2-routes-workflows.log)).
  - `cargo fmt --all -- --check` → exit 0.
- Inspected, not re-run: A's `routes-workflows-2.log`, `clippy-2.log`, `sqlx-prepare-2.log`; the
  re-measured plan output (B did not re-run the script this round; R1's independent `LIMIT 201`
  run agrees with it: 80,400 rows, ~83k buffers, ~40 ms). `scripts/check.sh` not repeated by A;
  its justification (COMMENT, docs, log field, script, plan file, one route switched to the
  shared helper, all covered by the focused rows plus clippy and sqlx check) is reasonable.
- Reused from R1 without re-review: unchanged filter SQL, route scoping, tests, index.

### Outcome

- F01-B1-F1, F2, F3: fixed. F4, F5 (optional): taken and verified.
- **Changes required** for two new Low findings in the correction delta:
  [F01-B1-F6](agent-review-findings.md) (agent + state reading names a field the line lacks; plan
  03's `returned` reading dropped; `rows_read` collides with the table's "Rows") and
  [F01-B1-F7](agent-review-findings.md) (deferred plan lacks per-batch checks and final gate).
  Both are documentation/plan text; no code behaviour is affected.
- Digest/coordination freshness rechecked before returning: unchanged.

### Process and instruction feedback

- Root's corrections were complete and well evidenced (both page sizes, re-measured on a fresh
  schema, explicit superseded D2, revised D4 keeping the original text). The new findings come
  from rewritten prose, which suggests rewritten operator runbooks deserve the same "does the line
  carry this?" check as code.
- Records note: TASK.md "Check results" has a blank line splitting the table, so the post-
  correction rows render without a header (workflow record only, not the artifact).
- R2 freshness recheck before returning (2026-10-30): HEAD `a6dc3be`, digest
  `029f6b315d9ca85fc5667d872514f714bd9e804edcf67f500cf98e47df3fb540` unchanged; COORDINATION.md
  revision 1, no notices. `mail_agents_f01_b` dropped; `mail_agents` and `mail_agents_f01` untouched.

## F01-B1-R3 — correction recheck of B1 (F01-B1-F6, F7)

### Identity

- Date: 2026-10-30. Workflow `implement-opus-reviewed-2_2`. Stage: correction recheck of batch B1.
  Reviewer: B, Opus 5.5 (`claude-opus-5-5`), same agent as R1/R2. Records: [TASK.md](TASK.md)
  ("Corrections after F01-B1-R2"), [REVIEW.md](REVIEW.md),
  [agent-review-findings.md](agent-review-findings.md).
- Phase `f01`, worktree `/Users/mac02/.codex/worktrees/b109/openbot`, detached HEAD; base = HEAD =
  `a6dc3bec0f40b3d7200e45198b21e707cc2de8be`; no commits.
- Artifact digest `3cd0d212c470888aa55b3ede7edf83d1ac5e5f4de7e5019df20d4ed769563f3b`, same
  exclusions; confirmed with `scripts/snapshot.sh` at start and before returning. Coordination
  revision 1, no notices (start and end). No gated decisions. Previous round F01-B1-R2
  (digest `029f6b31…`).

### Coverage and evidence

- Correction delta read: `reads.rs` tripwire (`logged_state`, `page_rows`, doc) and its call site;
  `reads_tests.rs` new test; run-list.md "Revisit when"; deferred plan "Build it when" and
  "Verification"; TASK corrections. Confirmed the run-list SQL builder is unchanged
  (`reads.rs:164-236`) and diff stats of the other tracked files match R2.
- Checked `logged_state` against the `workflow_run_state` CHECK (same six states) and against the
  existing UI `state_filter` (same list; optional F8). Checked the Revisit readings against what
  the line now carries and against plan 03's wording (deviation accepted, reasoning in F6's R3
  entry).
- Checks run by B on disposable `mail_agents_f01_b` (dropped afterwards):
  `SQLX_OFFLINE=true RUST_TEST_THREADS=4 cargo test --locked --offline --lib --
  workflow::reads::tests run_agent_filter_` → 7 passed, 1 ignored
  ([logs/B-R3-reads-and-filter.log](logs/B-R3-reads-and-filter.log)); `cargo fmt --all -- --check`
  → exit 0. Inspected A's `logs/clippy-3.log` (finished, no warnings).
- Reused from R1/R2 without re-review: filter SQL, route scoping and tests, index, plan evidence,
  COMMENT/doc corrections, Used-by helper (unchanged since R2).

### Outcome

- F01-B1-F6 and F01-B1-F7: fixed. All actionable findings F1–F3, F6, F7 fixed; optional F4, F5
  taken.
- **PASS** for digest `3cd0d212c470888aa55b3ede7edf83d1ac5e5f4de7e5019df20d4ed769563f3b`. One
  new optional suggestion, [F01-B1-F8](agent-review-findings.md) (shared run-state list), does not
  block acceptance.

### Process and instruction feedback

- Correction round was tight and complete; the unit test for log-injection-safe state naming is a
  good pattern for threshold logs that echo caller filters.
- R2's lesson held: rewritten runbook text needed the same "does the line carry it?" check; it
  now does. No further instruction change proposed.
- R3 freshness recheck before returning (2026-10-30): HEAD `a6dc3be`, digest
  `3cd0d212c470888aa55b3ede7edf83d1ac5e5f4de7e5019df20d4ed769563f3b` unchanged; COORDINATION.md
  revision 1, no notices. `mail_agents_f01_b` dropped; `mail_agents` and `mail_agents_f01` untouched.
