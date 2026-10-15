# Assignment S-002 — focused guidance: reconciliation tests failing under parallel load

Same role and settings (codex / gpt-6-astra / high). Read-only; no project edits.

D's group A verification (`docs/IMPLEMENTATION.md`, logs in `docs/logs/groupA/`)
found that the full `workflow_action_` filter (309 tests, stock 2 MiB, default
parallelism, 8-core/13 GB Linux host, task PG 18.6 fixture) fails:
`high_bounds_native_owners_and_supported_entry` fails every run with `Conflict`,
plus one other varying test (run2: `workflow_action_reconciliation_bounds_128_prior_consuming_129_total_then_lost_refuses`).
Base `5b3d7dc` + only the five import fixes also fails (305/307); there the
high-bounds test showed `state["live"] == false` (lease expired). Isolated, it
passes. Tests use a 3 s claim lease (`lease_tests.rs:12-14`). 04.7's full-suite
pass was on the earlier macOS host; this is the first Linux run.

Strict Clippy failure (Rust 1.99 `double_must_use` on `#[async_trait]`) is a
separate policy question root is taking to the user; ignore it.

## Task

Using the logs (`runtime-workflow-action.run1.log`, `.run2.log`, the A/B baseline
logs) and the code, decide:

1. Is each failure a test-calibration artifact (lease/time budget too tight under
   load on this host) or evidence of a production defect (e.g. lock/lease
   contention, unbounded work inside the lease, starvation)? Show the evidence.
2. If calibration: the correction criterion that keeps the AGENTS.md rule
   "a raised limit must leave something that fails when it is approached again"
   (e.g. the test must detect/report lease expiry as a distinct outcome rather
   than silently tolerating it, or a measured budget). Name what remains failing early.
3. If a production defect: describe it, and whether it reopens BRIEF-04.8 or is a
   04.7 regression requiring a separate decision.

Record as DEC-009 (and further IDs if needed) in `docs/GUIDANCE.md`, and a review
note R-002 in `docs/agent-review-notes.md` (findings in `agent-review-findings.md`
if a defect). Stop after that; final response: verdict, decision IDs, correction
criteria for D, and anything needing user input.
