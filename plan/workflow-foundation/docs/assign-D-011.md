# Assignment D-011 — batch C1: precision fixes, F-006, timing stabilization

Same role and settings (claude-code / claude-opus-5-5 / high).

User decisions (TASK.md decisions log): fix the 5 Linux precision failures in
04.8; include the F-006 production fix; for timing, make verification
load-independent where S says feasible, otherwise rerun on a quieter host.

Read S2's R-012, DEC-013 and DEC-014 in GUIDANCE.md and F-006 in
agent-review-findings.md, and implement exactly:
1. DEC-013 test-only corrections for the four test-only cases (attention ×2,
   task outreach, schedule slot) — keep their oracles.
2. F-006 production fix: initial wait park returns the persisted deadline
   (preferably `INSERT … RETURNING`) in `src/adapters/persistence/workflow/waits.rs`;
   trace callers (graft callers) first; add/adjust a test with deliberately
   sub-microsecond input asserting response == storage == replay. Re-run
   `cargo sqlx prepare` if queries change (commit `.sqlx` delta to worktree, not git).
3. DEC-014: make `workflow_action_dispatch_delayed_first_poll_checks_monotonic_window`
   deterministic (wait until captured deadline; zero provider polls; no guessed
   300/400 ms race) and add named-mode diagnostics / early-fail to
   `..._pending_actual_provider_dropped_on_loss_cancel_deadline`. No bound changes.

Verify: changed tests isolated ×3; affected filters (attention, task, schedule,
wait, `workflow_action_dispatch_`); fmt, diff-check, offline check, strict Clippy,
SQLx check. Then the full library suite at stock 2 MiB plus `lease-budget.sh` and
`stack-budget.sh` — check `uptime` first; if load average > 8 (other projects on
host, e.g. bizi44 — never touch them), wait (poll at most every 10 min, up to
~1 h) for a quieter host and record the load at start/end of each run. If still
loaded, run anyway and report the load alongside results.
Record "Batch C1" in IMPLEMENTATION.md, logs in logs/groupC/, manifest
`logs/groupC/source-manifest-C1.json` (all files changed since `8533a79` incl.
untracked and `.sqlx` changes). Stop after; final response: gate table, load
figures, manifest SHA256.
