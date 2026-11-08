# Paused fix-app recovery checkpoint — 2026-11-04

This is a recovery artifact for the existing fix-app plan. It stores unfinished p03 and p04 work as patches, new files, handoffs, reviews and test logs. Neither phase code batch is accepted. Application implementation remains paused.

The Git base is already in repository history, so this versioned checkpoint omits the standalone ZIP's redundant Git bundle. Use a normal clone with full history (or fetch the base commit if your clone is shallow).

## Restore on another computer

After pulling main, run from the repository root (Git and Python 3 required):

```sh
python3 plan/fix-app/recovery/2026-11-04/restore.py ~/dev/openbot-recovery
```

The destination must be NEW. Restoration clones the local repository into it, creates `.worktrees/fix-app-p03` and `.worktrees/fix-app-p04` at their original base, restores source changes and evidence, and verifies every captured source file. It never overwrites an existing destination, starts databases, runs application tests, commits, pushes or resumes implementation. The new clone's remote is `checkpoint_source`, pointing to the source clone; configure the real upstream as `origin` before future integration.

Read this checkpoint's `RESUME.md` before assigning work. No `.env`, credentials, PostgreSQL data or build caches are included. Recreate isolated test databases on the new host.

## Verification and unresolved work

A fresh offline restoration of the standalone package verified all2937 source files and reproduced both original workflow snapshot digests exactly. `RESTORE-VERIFICATION.txt` records that evidence; Git-based restoration is also exercised before publishing this checkpoint.

- p03: architecture gate G1r4 PASS; core24/24 PASS; affected union1071 passed,31 failed,1 ignored. All31 failures remain unresolved.
- p04: architecture gate G1r6 PASS; all-target compilation and authorization15/15 PASS. Broader checks and code review pending; WB2 not implemented.
- p06/p07/p08 have not started. User chose90-day eligible terminal-tree retention with usage/cost reporting preserved.

No backward compatibility is required by the underlying fix-app plan; recreate disposable fixtures when schemas change. Restoring this checkpoint does not accept or merge its unfinished application changes.
