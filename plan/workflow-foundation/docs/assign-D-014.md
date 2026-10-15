# Assignment D-014 — F-009 test fix, then final C2 gates

Same role and settings (claude-code / claude-opus-5-5 / high).

S3 (R-016): F-007 resolved. New F-009 (agent-review-findings.md): the stop-task
test `stop_task_serializes_with_final_dispatch_without_duplicate_customer_reply`
wrongly expects `Completed` after a direct `commit_agent_dispatch` (worker
closeout calls `mark_task_completed` separately) — an inherited race-order test
defect, not load. Quiet reruns cannot fix it. S3 also ruled that a clean
`stack-budget.sh` run (same binary, full suite, 2 MiB, default parallelism, real
DB) counts as the full-suite gate evidence.

1. Stop your gate orchestrator now (your own processes only; let nothing retry),
   keep its logs, record attempt a1 as superseded by F-009.
2. Fix F-009 per its correction criterion (test-only; keep the serialization and
   no-duplicate-reply oracle). Show the test deterministic: isolated ×5 plus the
   task filter, with a negative check if the criterion asks for one.
3. fmt, diff-check, offline check, strict Clippy.
4. Final gates on the final artifact, preferring a quiet host (same quiet rule;
   wait at most ~1 h, then run anyway and record load): `stack-budget.sh --offline`
   (counts as full suite), bin tests, `lease-budget.sh --offline`, live SQLx check.
   If a failure occurs with foreign load present, rerun once when quiet; a failure
   on a quiet host stops and is reported.
5. `graft build` (F-008), manifest `logs/groupC/source-manifest-C2.json`, and the
   IMPLEMENTATION.md "C1 correction (F-007/F-008/F-009)" section.
Final response: gate table with load, manifest SHA256.
