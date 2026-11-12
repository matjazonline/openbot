# Plan coordination: 01-departments

- Authority: /Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/COORDINATION.md
- Writer: root session 68933786-cbe7-4b6b-9387-b5dcb29689a4, worktree
  /Users/mac02/.codex/worktrees/2050/openbot (branch codex/departments-addressing-plans)
- Revision: 1

## Plan-wide instructions

- 01DEP-I001 / r1: No backward compatibility (plan README, root AGENTS.md). Edit the init schema
  in place; reset databases instead of backfills, dual reads, shims or staged rollouts.
- 01DEP-I002 / r1: Phases run in order 01 -> 02 -> 03 in this one worktree, each committed
  locally once accepted (user authorization, 2026-11-09). 02C (the word "desk") is its own
  commit. Phase 04 (UI and docs) and phase 05 are out of scope for this run.
- 01DEP-I003 / r1: Verification for this run is per-phase focused checks only (user decision,
  2026-11-09). The repository's full gate (`scripts/check.sh` plus CI-only gates) is not run and
  must be reported as not run; the task is not claimed fully verified.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | root 68933786 (A), B per TASK | /Users/mac02/.codex/worktrees/2050/openbot | plan/01-departments/phases/p01/docs/{TASK,HANDOFF}.md |
| p02 | root 68933786 (A), B per TASK | /Users/mac02/.codex/worktrees/2050/openbot | plan/01-departments/phases/p02/docs/{TASK,HANDOFF}.md (pending) |
| p03 | root 68933786 (A), B per TASK | /Users/mac02/.codex/worktrees/2050/openbot | plan/01-departments/phases/p03/docs/{TASK,HANDOFF}.md (pending) |

## Cross-phase notices

None.
