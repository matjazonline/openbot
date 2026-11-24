# 08G-P04: integration and final verification (Plan08 p04)

> **No backward compatibility.** We don't need backward compatibility and will reset the database when needed.

## Phase identity

- **Phase:** p04 of `plan/08-goals/04-integration-and-final-verification.md`, run once after part 03.
- **Workflow:** implement-opus-reviewed-2_4.
- **Root/author A:** Claude Code session `1e84a7af` (Opus 5.5).
- **Reviewer B:** Fable agent `a6ec66ca8201b8d62`, continued from p03.
- **Worktree:** `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`.
- **Starting commit:** `f1eb82dc` (p03 B2). There were no preserved pre-existing changes; the tree was clean.
- **Records:** `plan/08-goals/phases/p04/docs/`.
- **Coordination:** `/Users/mac02/orca/workspaces/openbot/plan08_ph1/plan/08-goals/COORDINATION.md`, checked at
  r10. 08G-C001 and 08G-C002 are satisfied by p03; p04 adds no notice.
- **Prerequisites:** p01 and p02 complete; p03 B1 `481b14cd` and B2 `f1eb82dc`, accepted in phase.
- **Fixtures:** scratch DBs `openbot_p08p03_mac02` (dev/SQLx) and `openbot_p08p03_mac02_test`, recreated from the
  init migration. A fresh disposable DB is used for `scripts/check.sh`.
- **Authorization:** the user's "continue" (2026-11-24) after p03; commit when accepted, as for p03.

## Requirements (04 for parts 02 and 03)

- **04A docs:**
  - `docs/departments.md` gains "Archive" and "Goals" (done in p02/p03).
  - `docs/workflow-operations.md` gains goal budgets and the goals page (budgets done in p03; the goals page
    is added here).
  - `docs/custom_tools.md` and `docs/agent-builder-context.md` describe `goal.status` and the kind argument on
    department creation.
  - Name only behavior the code has.
- **04B final gate:**
  - fresh DBs and `.sqlx` regenerated;
  - `scripts/check.sh` with a disposable `DATABASE_URL`, every gate recorded;
  - compare with `.github/workflows/ci.yml` and run what the script omits;
  - the evidence list: formatting, patch whitespace, locked offline compilation, clippy, fresh migrations, SQLx
    metadata, reachability, the DB suite, network isolation, stack and lease budgets;
  - graph refresh.
- **Carried from p03:**
  - `goal.status` through the agent runtime (the stand-in);
  - the full suite;
  - browser checks;
  - rollup cost at 10,000 runs.

## Batches

One batch, 08G-P04-B1: the docs, the `goal.status` runtime test, the final gate and the completion record. No
architecture gate: nothing here is a new persistent, concurrency or authorization decision.

## Implementation (2026-11-24)

- **Runtime test, and the defect it found.** New test `goal_rollup_status_tool_through_the_agent_runtime`
  (`goal_budget_tests.rs`). An agent holding `channel.call` and `goal.status`, and the goal's lead role, runs a
  real agent step on the goal's desk with a scripted model:
  - the first request offers `goal__status`;
  - the tool result in the second request is `{"status": …}` for this goal (name, 1 run counted, running 1).

  It found a p03 defect: an agent could not be given `goal.status` at all. The `agent_tool_bindings` CHECKs
  (`agent_binding_target_shape`, `agent_binding_reserved_alias`) listed every built-in except `goal_status`,
  so saving the binding failed with "Select a valid tool target belonging to this agent's company". Fixed in
  the init migration: `goal_status` is a builtin target, `goal.status` a reserved alias mapped from it. This is
  the same class as p03's learning (sibling literal `channel_call`). The sweep of `respond_to_escalation` /
  `channel_call` found no other stored copy; the Rust `AgentBuiltinTool::{name,parse}` already had it.
- **Fixtures:** `ToolSetup::with_builtins` and `CallSetup::with_tools` grant any set of built-ins;
  `with_channel_call` / `with` delegate to them.
- **04A docs:**
  - `docs/workflow-operations.md`: the Goals tab and "Runs for this goal" (`goal_id`, viewer's goals only); the
    JSON run listing has no goal filter.
  - `docs/custom_tools.md`: the opt-in built-ins `channel.call` and `goal.status`.
  - `docs/agent-builder-context.md`:
    - a goal is a department, so the Builder's `department` slug may name one, and `role: "lead"` applies
      (checked: `provisioning.rs` looks departments up by slug with no kind filter);
    - the Builder has no department-creation tool, so a person creates goals (Departments tab or JSON with
      `kind: "goal"`);
    - a standing department vs a goal;
    - `goal.status` must be granted.
  - "The kind argument on department creation" exists only on the JSON/UI create, which `departments.md`
    already documents. No Builder argument is invented.
- **Final gate started:** `CARGO_BUILD_JOBS=6 RUST_TEST_THREADS=8 DATABASE_URL=…/postgres ./scripts/check.sh`, log
  `logs/08G-P04-final-check.txt`. An unrelated `cargo test` from another project was running on the machine.

## Final gate (2026-11-24)

### `scripts/check.sh`

- **Command:** `CARGO_BUILD_JOBS=6 RUST_TEST_THREADS=8 DATABASE_URL=postgres://mac02@localhost:5432/postgres ./scripts/check.sh`.
  It makes and drops its own `openbot_check_*` database.
- **Log:** `logs/08G-P04-final-check.txt`, exit 1. Each gate's result, with its disposition, is below.

**PASS:**
- Runner test coverage; local-agent plugin tests and guards;
- generated CSS; frontend protocols (includes run counts and SSE navigation);
- deployment script tests; knowledge artifact preparation;
- Doop runner boundaries; Doop probe protocols;
- formatting; patch whitespace (`git diff HEAD --check`);
- workflow sample boundary, both its tests and the check;
- integration boundary, both its tests and the check; integration documentation;
- workflow evidence validator fixtures; retained workflow evidence;
- fetch locked Rust dependencies; offline compilation (`--locked --offline --all-targets`);
- test database; fresh migrations; SQLx metadata matches the migrated schema; compile tests;
- integration conformance families; ignored browser fixture producers;
- browser protocols using native markup (every CI browser step's script is in this gate or in the frontend
  protocols);
- lease budget (139 tests, 335 s);
- memory model preparation; memory backup and restore; OpenBot and memory integration.

**FAIL, with dispositions:**
- **Clippy.** 9 `redundant_field_names` in test files from merge `73afd5a7`, the same 9 as p02's gate. Plus
  one regression of p04's: `ToolSetup::with_channel_call` became unused once `CallSetup::with` went through
  `with_tools`.
  - Fixed: the helper is removed, since `with_builtins` replaces it.
  - Rerun (`cargo clippy --locked --offline --all-targets -- -D warnings`, `logs/08G-P04-clippy-after-fix.txt`):
    exactly the 9 pre-existing lints remain.
- **Database-backed test suite (isolated network).** 5195 passed, 1 failed, 30 ignored (2595 s). The failure is
  the known pre-existing `doop_agent_current_preparation_reaches_ready_before_model_io`: it expects prepared
  version 7, while `PREPARED_AGENT_VERSION` is 9 (log `left: Number(9)`, `right: 7`), set by `358e662a`
  (agent-workflow-call, before Plan08 p03; p03 TASK records the same). No Plan08 file is involved.
- **Stack budget** (stock 2 MiB). 5195 passed, 1 failed: the same doop test, with no overflow.
- **Transport abstraction boundary.** Environment: `rg` exists only as a Claude Code shell function that scripts
  cannot see.
  - Rerun with Claude Code's bundled ripgrep 14.1.1 on PATH (a scratchpad shim): PASS, both the regression
    tests and the check (`logs/08G-P04-transport-boundary.txt`).
- **Query statistics prerequisites.** Environment: the shared server at 5432 does not preload
  `pg_stat_statements` (`pg_stat_statements.track` unrecognized), and its configuration is not changed.
  - Rerun on a disposable private PostgreSQL 18.7 cluster in the scratchpad (127.0.0.1:15482, which preloads
    `pg_stat_statements` with the settings the gate requires): PASS.
  - The 11 `query_health` tests also PASS on that cluster (`logs/08G-P04-query-statistics.txt`). The cluster is
    stopped.
- **Memory extracted service suite; memory standalone journey.** Environment: system Python 3.9 lacks
  `tomllib`.
  - Rerun with Python 3.13.18 first on PATH and `check.sh`'s database URLs: PASS. The memory suite passes 178
    tests ("verify: all gates passed"), and the journey runs 13 steps on the stand-in provider
    (`logs/08G-P04-python313-gates.txt`).
- **Attribution extracted service and contracts.** With Python 3.13 the extracted build and 120 tests pass. The
  contract check then needs the Python package `jsonschema`, which is not installed (not installed here,
  without asking). **UNAVAILABLE.**
- **Attribution browser integration.** `attribution_sso_browser_real_openbot_password_and_existing_session`
  fails at "independent secure cookies". This is pre-existing: the same failure was in p02's gate, and Plan08
  does not touch `services/` or attribution.
- **Reproducible plugin package.** "use the pinned Node runtime": it needs Node 24.21.0, and the machine has
  v26 and Homebrew `node@24` v24.22.3. **UNAVAILABLE.**
- **Knowledge, memory and attribution production images.** No Docker daemon. **UNAVAILABLE.**
- **Managed Doop service and private integration.** `DOOP_TEST_UI_CHROME` is unset. **UNAVAILABLE.**
- **Linux network-isolation scope** (`network-isolation-scope.py`). The runner prints "NOT EXERCISED locally;
  required in CI". On macOS the DB suite, stack budget and lease budget ran under `sandbox-exec` network
  isolation. **UNAVAILABLE** here.

### Compared with `.github/workflows/ci.yml`

Every CI step has a `check.sh` gate except two that CI runs on Linux:
- the network-isolation scope (unavailable here, above);
- query-statistics activation (exercised above on the private cluster).

### Other checks

- **Reachability.** All 51 `src/**/*.rs` files added since merge base `a1cdb2d5` are declared modules
  (`mod` or `#[path]`), and offline compilation of all targets compiles them.
- **Rollup cost at the limit** (carried from p03). New ignored measurement `goal_rollup_cost_at_the_run_limit`
  (`goal_rollup_cost_tests.rs`):
  - it clones one admitted goal run into 10,001 runs, with their root budgets;
  - three whole `goal_rollup_on` calls at `GOAL_ROLLUP_RUN_LIMIT` take 31, 21 and 21 ms (truncated, 10,000
    counted);
  - `EXPLAIN (ANALYZE, BUFFERS)` of the base statement: 9.7 ms, 30,460 shared hits, an index scan with LIMIT and
    one index probe per run for the scope and the root (`logs/08G-P04-rollup-cost.txt`).
  - Left unproved: the clones have no decisions, calls or escalations, so those joins probe and find nothing;
    and every run in the company is the goal's, so the planner chose `workflow_runs_newest` with a goal filter.
    A company with many runs outside the goal, which would use `workflow_runs_goal_idx`, is not measured.
  - The scratch test DB was recreated afterwards.
- **Browser checks** (carried from p03). The browser protocols all pass, above. No browser script covers the
  department form or the goals pages; those are covered by the HTTP route tests only. This is a stand-in, as
  recorded in p03.
- **Focused, after the post-gate edits** (the helper removed, the measurement added):
  - `cargo test --offline --locked --lib -- goal_ channel_call_ builtin exactness catalog` on the recreated
    scratch DB: 234 passed, 1 ignored (the measurement) (`logs/08G-P04-focused.txt`);
  - `cargo fmt --check` and `git diff --check` are clean.
- **SQLx.** The p04 schema edit is CHECK-only. The "SQLx metadata matches" gate passed on the fresh check DB,
  so `.sqlx` is unchanged.
- **Graph.** `graft build`: exit 0 (`logs/08G-P04-graph-build.txt`).

## Verification mapping

| Criterion | Evidence | Decisive assertion or result |
| --- | --- | --- |
| 04A docs name only behavior the code has | `docs/workflow-operations.md`, `docs/custom_tools.md`, `docs/agent-builder-context.md` | Each claim was checked against the code: Goals tab, `goal_id` filter, no JSON goal filter, opt-in built-ins, slug lookup with no kind filter (`provisioning.rs`) |
| `goal.status` through the agent runtime (p03 carry) | `goal_rollup_status_tool_through_the_agent_runtime` | The model is offered `goal__status`; the tool message is `{"status":…}` for this goal, with `runs_counted==1` and `running==1` |
| Production wiring of `goal.status` (`src/infra/workflow.rs`) | compilation only | The runtime test wires the dispatcher in `workflow_tool_support.rs`; the production `with_goal_status` line is compile-only, as `channel.call`'s is (08G-P04-B1-R1-S1) |
| An agent can be granted `goal.status` (defect) | the same test (the grant is saved through `ToolSetup::with_builtins`) | The bind succeeds; before the CHECK fix it failed with 23514 |
| Rollup cost at the limit (p03 carry) | `goal_rollup_cost_at_the_run_limit` (ignored, measurement) | 10,001 runs: `truncated`, 10,000 counted, 21–31 ms per rollup |
| 04B gate items | `logs/08G-P04-final-check.txt` plus the reruns above | See "Final gate" |

## Review 08G-P04-B1-R1 (2026-11-24)

B: changes required, record-only (N1).
- **F1** (fixed): the doop failure is version 9 vs 7, from `358e662a`, not 8 from `2edd0a73`. Corrected here and in
  the 04 completion record.
- **F2** (fixed): HANDOFF refreshed (coordination r12, next step).
- **S1** (taken): the production `goal.status` wiring is compile-only; the limit is now stated in the mapping.
- **S2** (not taken): the measurement's EXPLAIN copies the rollup's base statement by hand. It is an ignored
  measurement, and the copy matched when it was reviewed; moving `BASE` into shared test support is not worth it.

## Acceptance (2026-11-24)

- **Pass:** B PASS 08G-P04-B1-R2-N1 at digest `c081c405b668c034641caeac45ada2af93e98c1a88d6874c355f2634f4c3fc47`
  (base `f1eb82dc`, exclusions `plan/08-goals/phases/p04/docs` and `.trail/learnings`). Root recomputed the
  same digest. `logs/08G-P04-B1-R2.evidence.json` validates (5 PASS, 3 FAIL with dispositions, 2 NOT EXERCISED).
- **Status:** p04 is accepted in phase. Parts 02 and 03 of Plan08 are not declared complete, because the gates
  listed in the 04 completion record are unavailable here.
- **`trail check`** (`logs/08G-P04-trail-check.txt`): exit 1, 8 conflicts, all keyword matches with no
  conflicting change.
  - Seven are the schema comments p03's commit explained (lines 12667, 16222, 17996, 20987, 21063, 21159),
    shifted 2 lines by p04's CHECK edit.
  - One is `docs/departments.md:397`, the goals-office prose from `481b14cd`, matched against a learning about
    holding a document during traversal.
