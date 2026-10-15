# Assignment S-015 — recheck F-007 now; pre-judge C2 gate evidence

Same role and settings (codex / gpt-6-astra / high). Read-only. D2's gate
orchestrator is running tests on the fixture; source is frozen (no edits until it
finishes). Manifest C2 is not yet written: identify the artifact as current HEAD
+ worktree, verify that only `src/adapters/persistence/workflow/action_dispatch_remote_tests.rs`
differs from the C1 manifest `logs/groupC/source-manifest-C1.json` (record its SHA256).

1. Recheck F-007 against the actual diff and D2's evidence (IMPLEMENTATION.md
   C1 correction section if written, logs/groupC/C2-isolated-*, quiet-rerun-C2-stdout.log,
   load-C2.log): run-limited witness and lease-limited negative control.
   Update its disposition; record R-016.
2. Evidence judgment (no verdict on F-008 yet; graft build pending): attempt a1
   on this artifact had the direct full-suite run 2558/1/22 under bizi44 load
   (failure `stop_task_serializes_with_final_dispatch_without_duplicate_customer_reply`,
   task still Processing, outside 04.8 code), while `stack-budget.sh` re-ran the
   full suite at 2 MiB on the same binary with 2559/0/22. Is that failing test
   load-sensitive (inspect it and its prior pass history in groupC/C1 logs) or a
   concern? Is a clean full-suite run within stack-budget acceptable as the
   full-suite gate evidence, or must the direct run be clean on a quiet host?
Final response: F-007 disposition, answers to 2, anything blocking final PASS
besides F-008.
