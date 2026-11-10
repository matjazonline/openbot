# Plan coordination: memory-service

- Authority: /Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/COORDINATION.md (worktree 60a0, detached HEAD on a6bed48a; untracked)
- Writer: root session 6fedfb2e-40aa-486c-bd0b-051b5a3add7b (main conversation), worktree /Users/mac03/.codex/worktrees/60a0/openbot
- Revision: 7 (2026-11-10: p06 accepted, plan complete; pushed under I001 r2)

Phases run sequentially in this one worktree. Phases 01–04 (the standalone service) and the phase-05
architecture gate plus batch P7b were completed under `implement-opus-reviewed`; their records stay at
`tmp/memory-service-run/` (progress.md, reviews/, frozen/, logs/) and are referenced, not migrated.
Phase 05 continues under `implement-opus-reviewed-2_2` from 2026-11-08 ~20:10Z.

## Plan-wide instructions

### memory-service-I001 / r1 — Repository and plan rules
- `AGENTS.md` and scoped guides under `src/` apply; `services/memory/` is an independent project.
- No backward compatibility: edit `migrations/20260817000000_init_schema.sql` and
  `services/memory/migrations/0001_init.sql` in place; reset databases instead of migrating data.
- No commits, pushes, deploys or production resets are authorized.
- r2 (2026-11-10, user): "when complete push to main". After the plan is complete (06C passed and
  B's final pass on the integrated tree), root may commit this plan's work and push it to
  `origin/main`. Deploys and production resets stay unauthorized.
  - `main` does not contain this work's base: the plan commit 971fc627 and the local merge
    a6bed48a were never pushed, and `main` is 24 commits ahead of their merge-base 78a93628.
  - About 45 overlapping files (init migration, `src/main.rs`, CI, workflow code), so
    integration is a merge of `origin/main`, conflicts resolved, then 06C run on the merged tree.
  - The commit holds only this plan's paths. It excludes tmp/ and `.env`, and it excludes the
    other session's untracked organization-plan copies. Those are older versions of files
    `main` now has; they are backed up under tmp/ before the merge.

### memory-service-I002 / r1 — Databases and resources
- Tests use only root's disposable private cluster: PostgreSQL 18.6 + pgvector 0.8.7 at
  `postgres://postgres@127.0.0.1:55432/<own db>` (managed by `tmp/memory-service-run/pg-cluster.sh`).
  Never connect to localhost:5432; never read or source any `.env`.
- Mutation probes build in a separate `CARGO_TARGET_DIR` under `tmp/memory-service-run/`, deleted afterwards.
- Docker and CI runs are unavailable here; record them as unavailable.

### memory-service-I003 / r1 — Product decisions (user, 2026-11-08)
- Binding record: `plan/memory-service/implementation-decisions.md` (D-section for the service; E12 for
  phase 05: U1 opt-in per company; U2 ConversationalInput inexact, AuthoredConfiguration exact 0 for
  agent/user scope and inexact (consumed: unknown) for company scope; U3 recall via SearchMemory binding,
  private-session capture to the actor's user scope, thread capture to agent scope behind a per-agent
  policy off by default). Provider Anthropic claude-sonnet-5-5; any authorized level may be sent; no
  ZDR/region requirement; ops budgets as recorded.
- r2 (2026-11-09): U4 the user scope is any of the actor's own manual roots in an unattached session
  (and same-actor children), whoever authored the workflow: kept and documented. U5 removing
  MEMORY_SERVICE_URL while companies are enabled is a documented operator edge (tools stay listed
  and publishable, calls fail closed). Both in E12; they apply to p05 (tools now, recall/capture in
  B4) and p06 docs.
- r3 (2026-11-09): U6 automatic recall cap 8,192 tokens (E7 said 2,048), fixed 64 KiB byte bound.
- r4 (2026-11-10): U7 nothing is captured while `MEMORY_SERVICE_URL` is unset (capture armed only in
  a preparation frozen by a configured runtime); applies to p05-B5 and p06 docs.
- r5 (2026-11-10): U8 the representative load (06A) is option C. The measurement is reported and
  gates nothing: 4 tenants, each namespace seeded with about 10,000 memories, then concurrent saves
  and searches with the real model, p50/p95 recorded. U9 the paid-provider smoke check (D12) is
  authorized with a 50,000-token budget, enforced by the smoke tenant's daily generation cap. It
  uses a test key the user sets as `MEMORY_SMOKE_ANTHROPIC_API_KEY` in the repo's git-ignored
  `.env`, which only `scripts/memory-paid-smoke.sh` reads, and only that one key. This is the
  user's exception to I002's "never read .env". Root confirms with the user before the paid run.
  Both apply to p06.

## Phase directory
| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01–p04 | root 6fedfb2e (implement-opus-reviewed) | 60a0 | tmp/memory-service-run/progress.md (accepted locally) |
| p05 | root 6fedfb2e (implement-opus-reviewed-2_2 from ~20:10Z); **accepted in phase 2026-11-10** (final digest 88808f5a…b0a1) | 60a0 | plan/memory-service/phases/p05/docs/TASK.md, HANDOFF.md |
| p06 | root 6fedfb2e (implement-opus-reviewed-2_2), started 2026-11-10; **accepted in phase 2026-11-10** (final integrated digest 58acbacb…8e39, merge HEAD 801839fd); the commit carrying this record is pushed to origin/main per I001 r2 | 60a0 (merge in tmp/memory-service-run/merge) | plan/memory-service/phases/p06/docs/TASK.md, HANDOFF.md |

## Cross-phase notices
None.
