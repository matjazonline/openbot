# Plan coordination: fix-app

- Authority: /Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md (primary checkout, main branch; untracked)
- Writer: manager root session (main conversation), worktree /Users/mac03/dev/openbot
- Revision: 2

## Plan-wide instructions

### fix-app-I001 / r1 — Repository and plan rules
- `AGENTS.md` (repo root) and scoped guides under `src/` apply. No backward compatibility (reset DB instead).
- `plan/fix-app/README.md` "Rules for running plans concurrently" and "Verification" apply:
  edit only the named statements of `migrations/20260817000000_init_schema.sql` in place; per-plan final
  gate is `./scripts/reset-db.sh --all` then `scripts/check.sh`.

### fix-app-I002 / r1 — Isolation and resources
- Each phase runs in its own worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-pNN`, branch `fix-app/pNN`.
- Each phase has a private Postgres 18 cluster (trust auth) at `127.0.0.1:554NN`, data dir
  `/Users/mac03/dev/openbot/.worktrees/pg-pNN` (owned by the manager; do not stop/delete it).
  Worktree `.env`: `DATABASE_URL=postgres://mac03@127.0.0.1:554NN/mail_agents` (tests derive `mail_agents_test`).
  psql/createdb: `/opt/homebrew/opt/postgresql@18/bin` (not on PATH).
- Use `CARGO_TARGET_DIR` inside the worktree (default `target/`); never share another phase's target dir.
- Never touch another phase's worktree, cluster, branch or records.

### fix-app-I003 / r1 — Commits and merges
- User authorized: each phase root commits its accepted phase on its own branch (`fix-app/pNN`), including
  code, migrations, `.sqlx/` and its own records under `plan/fix-app/phases/pNN/docs/`.
  Do not commit `plan/fix-app/COORDINATION.md`. No push.
- Only the manager merges phase branches into local `main`, then resets DB and reruns focused tests.
  Dependent phases start from the merged `main` commit.

### fix-app-I004 / r1 — Verification scope (user instruction, supersedes the README per-plan final gate)
- Phases do NOT run the full test suite or `scripts/check.sh`. Run only targeted tests for the code the
  phase affects (changed behavior, callers, shared contracts), with nonzero test counts per filter, after
  `./scripts/reset-db.sh --all` when the schema changed. `cargo fmt --check` and compiling the affected
  targets are fine. Regenerate `.sqlx/` if macro-checked queries changed. Record what ran.

## Phase directory
| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths | State |
| --- | --- | --- | --- | --- |
| p01 | phase root agent (see manager) | .worktrees/fix-app-p01 | .worktrees/fix-app-p01/plan/fix-app/phases/p01/docs/ | in progress |
| p02 | phase root agent | .worktrees/fix-app-p02 | .worktrees/fix-app-p02/plan/fix-app/phases/p02/docs/ | in progress |
| p05 | phase root agent | .worktrees/fix-app-p05 | .worktrees/fix-app-p05/plan/fix-app/phases/p05/docs/ | in progress |
| p03 | — | — | — | awaiting p02 merge |
| p04 | — | — | — | awaiting p01 merge |
| p06 | — | — | — | awaiting p03 merge |
| p07 | — | — | — | awaiting p04 merge |
| p08 | — | — | — | awaiting p01, p02, p03 merge |

## Cross-phase notices
None yet. Phases request notices by stating them in their final report / SendMessage to the manager ("main").

## Manager log
- 2026-10-31: wave 1 started. Phase roots p01, p02, p05 spawned (Opus 5.5 / high, general-purpose subagents of the manager session) from main e6b0a51. Each spawns its own Opus 5.5 / xhigh reviewer.
