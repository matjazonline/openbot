# f01 — fix-01-run-agent-query

## Identity

- Phase ID: `f01` (the whole unphased plan `plan/fix-01-run-agent-query/`; IDs `F01-B<n>-R<n>` /
  `F01-B<n>-F<n>`)
- Workflow: `implement-opus-reviewed-2_2`, skill dir
  `/Users/mac02/.codex/worktrees/b109/openbot/.claude/skills/implement-opus-reviewed-2_2`
- Record root: `/Users/mac02/.codex/worktrees/b109/openbot/plan/fix-01-run-agent-query/phases/f01/docs/`
- Coordination: `/Users/mac02/.codex/worktrees/b109/openbot/plan/fix-01-run-agent-query/COORDINATION.md`,
  revision 1 checked at startup; no notices (none applicable).
- Root = Author (A): this Claude Code session, Opus 5.5 at `xhigh` (user-confirmed 2026-10-30 via
  AskUserQuestion; `~/.claude/settings.json` `effortLevel: xhigh`). Reviewer (B): spawned at first
  review, `general-purpose`, `model: opus`, `effort: xhigh`; agent ID `ad2d28ffa854b444f`
  (spawned 2026-10-30 for F01-B1-R1, continued with SendMessage for rechecks).
- Worktree: `/Users/mac02/.codex/worktrees/b109/openbot`, detached HEAD (pushes go to `main` via
  `HEAD:main`, user-authorized per accepted plan).
- Starting commit: `a6dc3bec0f40b3d7200e45198b21e707cc2de8be` (= `origin/main` at start). Preserved
  pre-existing changes: none (worktree clean at start).
- Snapshot exclusions (exact, reused for A, B and root):
  `plan/fix-01-run-agent-query/phases/f01/docs` `plan/fix-01-run-agent-query/COORDINATION.md`
- Context: `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75` verified in the session environment.
- Authorizations: commit and push this plan to `main` once accepted (user, 2026-10-30). No
  development/production database reset is authorized; checks use disposable databases.

## Original requirements

- `plan/fix-01-run-agent-query/01-agent-run-filter.md`
- `plan/fix-01-run-agent-query/02-query-plan-evidence-and-index.md`
- `plan/fix-01-run-agent-query/03-slow-read-tripwire-and-release.md`
- Repository rules: `AGENTS.md`, `src/AGENTS.md`, `src/adapters/persistence/AGENTS.md`,
  `src/adapters/http/AGENTS.md`, `src/application/AGENTS.md`.

## Acceptance criteria (from the plan)

1. `RunFilter.agent` selects runs of versions whose `agent_ids` contain the agent, in one
   company-scoped statement; meaning ("uses, not ran") documented on the field.
2. A caller-supplied agent id is resolved company-scoped first: another company's or a deleted
   agent is not found, never an empty list. JSON runs route accepts `agent_id`.
3. Display-index contract comments name the second reader; nothing decides/authorizes on it.
4. `run_agent_filter_*` DB-backed tests: version split; re-pointed binding; agent only in a pinned
   child matches the parent's runs; combined with binding/state/cursor; cross-tenant 404 and empty
   page for an unused agent.
5. `scripts/tests/run-list-query-plan.py` + recorded evidence (`docs/performance/run-list.md`,
   `run-list-query-plan.txt`); index decision by access path per the plan's ladder.
6. `warn_if_run_list_is_slow` tripwire past `RUN_LIST_QUERY_WARN_THRESHOLD` (500 ms) with a pure,
   unit-tested decision; "Revisit when" docs; `agent_id` documented on the route.
7. Focused checks, then `scripts/check.sh` once; results recorded.

## Batches

One batch, `B1`: the whole plan (01–03). No architecture gate: the decisions are display-only
read filters on established patterns; an index (if the evidence warrants one) is a cheap,
reversible init-schema edit reviewed in the batch review. A link-table outcome would stop the
batch for a separate plan (per 02).

## Decisions (A)

- D1 — Statement text for measurement: `list_runs` builds its statement with `QueryBuilder`; the
  builder moves into a synchronous `run_list_query` so an ignored test can dump each shipped shape
  (`dump_run_list_query_shapes`), and the Python script measures that exact text. This replaces the
  plan's "named const per fragment" wording with the stronger guarantee the plan asked for (shipped
  text, not a copy).
- D2 — (superseded 2026-10-30 by F01-B1-F3) Originally left the schema `COMMENT ON COLUMN
  workflow_versions.agent_ids` unchanged to avoid a checksum change; void because B1 already edits
  the init schema (D4 index). The COMMENT and `application/workflow/reads.rs` `agent_usage` doc now
  name the run list's agent filter as the second reader, with the other two comments.
- D3 — Agent existence check reuses `AuthoringReads::agent_tool_grants` (documented "`None` when
  the company has no such agent"), as the Used-by route already does, through a new
  `bindings::company_agent` helper; the route authorizes the company before looking at the agent.
- D4 — Index decision (plan 02 ladder), from evidence in `docs/performance/run-list.md`:
  - Option 1 (no change) fails: every agent shape read the heavy tenant's history; the generic
    plan, which production gets after five executions of a cached prepared statement, took 234 ms
    (dense) and walked 239,520 runs (sparse); dense at depth 200 took 3.9 s with a custom plan.
  - Option 2 measured two ways (`logs/option2-experiment.txt`). The index
    `(company_id, version_id, created_at DESC, id DESC)` with the plan's `IN (...)` still read
    and sorted the dense agent's whole history (58–63 ms, 127k buffers) and was rejected. The same
    index read per version with `CROSS JOIN LATERAL ... LIMIT`, then merged, bounds a page to
    versions × page size (dense 10–15 ms, sparse 0.15 ms, same in both plan modes) and is adopted
    as `workflow_runs_version_newest` in the init schema. This changes the agent shape from the
    plan's "appended `IN` clause" to a lateral read: same semantics, recorded deviation.
  - Option 3 (link table) — revised 2026-10-30 after F01-B1-F1. The original note said it was not
    triggered; that was measured at 50-row pages only. At 200-row pages (`MAX_LISTED`, the JSON
    default and the UI's grown-list refresh) the lateral read takes 80,400 of the dense agent's
    ~125k runs (41 ms generic), and with a state filter all of it. So option 2 still reads run
    history, which is 02's trigger. 02 says "stop and write its plan rather than growing this
    one": the link-table plan is written as `plan/db_improve/run-agent-link-table.md`, **deferred**
    to the tripwire (persistence `AGENTS.md`: change performance only from production evidence).
    B1 ships the lateral read as an order-of-magnitude improvement at both page sizes, with its
    bound stated in `docs/performance/run-list.md`, the `run_list_query` doc, and the deferred plan.
  - Agent + rare state (`queued`/`running`) reads the agent's whole history under every option,
    including a link table, the same class as the pre-existing company + rare-state shape;
    documented as known and left to the tripwire.

## Corrections after F01-B1-R1 (2026-10-30)

- F01-B1-F1: script measures both page sizes (`LIMITS`); `docs/performance/run-list.md` rewritten
  with 50- and 200-row tables and the stated bound; `run_list_query` doc corrected; D4 revised;
  deferred plan `plan/db_improve/run-agent-link-table.md` written (note + `src/main.rs` list).
- F01-B1-F2: HOT claim removed (state is indexed elsewhere, so state changes are never HOT and
  each writes the new index); size restated as 135 MB for 1,096,000 runs (~123 MB per million).
- F01-B1-F3: schema COMMENT and `application/workflow/reads.rs` `agent_usage` doc updated.
- F01-B1-F4 (optional, taken): Used-by route (`ui/authoring/tools.rs`) calls
  `bindings::company_agent` instead of repeating the check.
- F01-B1-F5 (optional, taken): log field renamed `returned` → `rows_read` and documented as the
  page plus its look-ahead row.

## Corrections after F01-B1-R2 (2026-10-30)

- F01-B1-F6: the slow-read log now names the state asked for (`logged_state`: a run state,
  `none`, or `other` — the JSON route passes caller text through unchecked, so it is never logged
  as given) and reports `page_rows` (page + look-ahead when full, fewer when matches ran out)
  instead of `rows_read`; new unit test
  `the_slow_read_log_names_a_run_state_and_never_the_callers_text`. `docs/performance/run-list.md`
  "Revisit when" rewritten around `state` and full vs short pages, distinguishing the link-table
  case (agent, no/terminal state, full page) from the rare-state shape. The deferred plan's
  "Build it when" uses the same reading, so agent + a common state counts.
- F01-B1-F7: `plan/db_improve/run-agent-link-table.md` Verification now names per-batch focused
  checks (`run_agent_links_*` tests, existing suites, evidence script, lease budget) and a final
  `scripts/check.sh` gate.
- Record issue noted in R2: the check-results table is joined into one table.

## Progress

- Status: **accepted in phase** 2026-10-30 — B PASS F01-B1-R3 for digest
  `3cd0d212c470888aa55b3ede7edf83d1ac5e5f4de7e5019df20d4ed769563f3b`, recomputed by root at
  acceptance; all actionable findings (F1, F2, F3, F6, F7) fixed, optional F4/F5 taken; coordination
  rev 1, no notices. Integrated: committed and pushed to `main` (commit recorded in HANDOFF).
- F01-B1-F8 (optional, one shared list of run-state names for `logged_state` and the UI's
  `state_filter`): declined for this artifact to avoid reopening review; carried to
  `fix-03-run-board-finished-columns`, whose board read handles run states again.
- Changed: `src/application/workflow/reads.rs` (`RunFilter.agent`),
  `src/adapters/persistence/workflow/reads.rs` (`run_list_query`/`push_run_page`, tripwire),
  `reads_tests.rs` (new), `src/adapters/http/routes/workflows/bindings.rs` (`agent_id`,
  `company_agent`), `run_filter_tests.rs` (new) + `tests.rs` (module), `ui/runs/mod.rs`
  (`agent: None`), comments in `authoring_reads.rs` and `publication/mod.rs`, init schema index,
  `scripts/tests/run-list-query-plan.py` (new), `docs/performance/run-list.md` + both plan
  outputs (new), `docs/workflow-operations.md` (listing filters).

## Check results

All with `SQLX_OFFLINE=true RUST_TEST_THREADS=4` against disposable `mail_agents_f01`
(`TEST_DATABASE_URL` and `DATABASE_URL`); route fixtures create their own databases.

| Command | Result | Log |
| --- | --- | --- |
| `cargo test --lib run_agent_filter_` (before D4) | 4 passed | `logs/run_agent_filter-1.log` |
| `cargo test --lib workflow::reads::tests` | 2 passed, 1 ignored (dump) | inline |
| `cargo test --lib dump_run_list_query_shapes -- --ignored` | writes 6 shapes | via script |
| `python3 scripts/tests/run-list-query-plan.py …` (before D4) | exit 0 | `logs/run-list-query-plan-before.txt` |
| option-2 experiment (scratch script) | exit 0 | `logs/option2-experiment.txt` |
| `python3 scripts/tests/run-list-query-plan.py …` (after D4, schema re-migrated) | exit 0 | `docs/performance/run-list-query-plan.txt` |
| `cargo test --lib routes::workflows::` (after D4) | 83 passed (incl. 4 `run_agent_filter_`) | `logs/routes-workflows-1.log` |
| `cargo fmt --all -- --check` | ok | inline |
| `RUST_TEST_THREADS=4 DATABASE_URL=…/mail_agents_f01 ./scripts/check.sh` | exit 0; 15/15 gates PASS; 2,461 lib tests passed (6 ignored) in the suite and again at 2 MiB (stack budget) | `logs/check-1.log` |
| `scripts/lease-budget.sh --offline` (not in check.sh; CI runs it; affected because the new index adds work to `workflow_runs` inserts) | exit 0; 130 passed; highest shares `budget.reserve` 359/1000 ms, `lease.renew` 313/2000 ms (calibration 357, 310) | `logs/lease-budget-1.log` |
| After corrections: DB re-migrated (COMMENT checksum), `python3 scripts/tests/run-list-query-plan.py …` | exit 0, both page sizes | `docs/performance/run-list-query-plan.txt` |
| `cargo test --lib -- routes::workflows:: workflow::reads::tests` | 85 passed, 1 ignored (incl. `used_by_lists_…`, 4 `run_agent_filter_`) | `logs/routes-workflows-2.log` |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | exit 0 | `logs/clippy-2.log` |
| `cargo sqlx prepare --check -- --all-targets` (fresh migrated `mail_agents_f01`) | exit 0 | `logs/sqlx-prepare-2.log` |
| `cargo fmt --all -- --check`, `git diff --check` | ok | inline |
| After R2 corrections: `cargo test --lib workflow::reads::tests` | 3 passed, 1 ignored | inline |
| After R2 corrections: `cargo clippy --locked --offline --all-targets -- -D warnings` | exit 0 | `logs/clippy-3.log` |

`scripts/check.sh` was not repeated after corrections: the changes are a schema COMMENT, docs, a
log field name, a script, a plan file and one route's switch to the shared helper, all covered by
the rows above.

R1 artifact frozen for review: digest `6d32795ab478c5a08c6d0bdfe1d139f77cb0fe6b34dde44dd5ce9a538860fae9`
(base `a6dc3be`, exclusions as in Identity), re-confirmed after `check.sh`.
