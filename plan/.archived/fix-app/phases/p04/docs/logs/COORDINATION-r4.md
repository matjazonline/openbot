# Plan coordination: fix-app

- Authority: /Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md (primary checkout, main branch; untracked)
- Writer: manager root session (main conversation), worktree /Users/mac03/dev/openbot
- Revision: 4

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
- Disk is tight: prefix cargo with `CARGO_INCREMENTAL=0`; delete only your own `target/debug/incremental` when needed.
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
| p01 | phase root agent | (removed) | main: plan/fix-app/phases/p01/docs/ | integrated in main 9d68a6b (commit 4d940da) |
| p02 | phase root agent | (removed) | main: plan/fix-app/phases/p02/docs/ | integrated in main fbf56e9 (commit 7ca2fb5) |
| p05 | phase root agent | (removed) | main: plan/fix-app/phases/p05/docs/ | integrated in main 48626a1 (commit 2f16a07) |
| p03 | phase root agent | .worktrees/fix-app-p03 | .worktrees/fix-app-p03/plan/fix-app/phases/p03/docs/ | in progress from 48626a1 |
| p04 | phase root agent | .worktrees/fix-app-p04 | .worktrees/fix-app-p04/plan/fix-app/phases/p04/docs/ | in progress from 48626a1 |
| p06 | — | — | — | awaiting p03 merge |
| p07 | — | — | — | awaiting p04 merge |
| p08 | — | — | — | awaiting p03 merge (p01, p02 integrated) |

## Cross-phase notices
Phases request notices in their final report or by SendMessage to the manager.

### fix-app-C001 / r1
- State: active
- Origin: p02 (TASK.md "Cross-phase notes"), integrated 48626a1
- Targets: p03
- Instruction/change: the one continuation limit is SQL `workflow_job_continuation_limit()` (63), mirrored by Rust
  `WORKFLOW_JOB_CONTINUATION_LIMIT` (DB test asserts equality). The `effect_parked` branch of
  `check_workflow_fenced_completion` now also requires `OLD.continuation_count < limit`. p03's run-state transition
  table and guard-error classification must admit this park and the refusals it produces.
- Verification: p02 filters `message_park_continuation_`, `delivery_late_completion_`, `notification_stale_`,
  `schedule_claim_` still pass after p03.
- Responses: p03 -> pending

### fix-app-C002 / r1
- State: active
- Origin: p01 (N1), integrated 9d68a6b
- Targets: all current and future phases with database fixtures
- Instruction/change: `companies.user_id` and the owner `company_members` row are one fact (deferred
  `companies_owner_member_fk` + constraint triggers). Fixtures must not insert a company without its owner row,
  change `companies.user_id` alone, or demote/delete the owner row. Use `test_support::insert_company_with_owner`,
  `transfer_company_ownership`, `revoke_membership` / `revoke_membership_now`. Injected statements should `expect`
  success so a refusal cannot pass as the expected error. Deleting an agent retires it; three history FKs are NO
  ACTION (`principals_company_agent_fk`, `messages_author_principal_fk`, `message_participants_identity_fk`).
- Responses: p03 -> pending; p04 -> pending

### fix-app-C003 / r1
- State: active
- Origin: p01 (N3), integrated 9d68a6b
- Targets: p04
- Instruction/change: p04's precondition is met: ownership is enforced as one fact. `CompanyInvitePersistence` changed:
  `is_company_owner_address` added; `update_invite(company_id, …)` matches only a pending invite of that company and
  returns `Option`. Invite acceptance never rewrites the owner row; inviting the owner's own address is refused.
- Responses: p04 -> pending

### fix-app-C004 / r1
- State: active
- Origin: p01 (N4), p02, integrated
- Targets: p08
- Instruction/change: identity status `disabled` remains unused (droppable). Stale notification events now
  dead-letter with `last_error_class` NULL (no stale class exists); add one if 08 copies error-class rules onto that
  queue. 05: `message_deliveries`/`message_delivery_parts.company_id` are NOT NULL.
- Responses: p08 -> pending (not started)

### fix-app-C005 / r1
- State: active
- Origin: p05 BR1 review, integrated 48626a1
- Targets: p03, p06 (anyone editing workflow_runs / init_schema FKs)
- Instruction/change: a workflow binding with runs can no longer be hard-deleted (run -> binding revision FK).
  Several FKs in workflow message/thread tables are DEFERRABLE INITIALLY DEFERRED because a non-deferred NO ACTION
  key is checked per cascade round in Postgres (p01's schema comment saying "end of the statement" is inaccurate).
  p06 retention deleting terminal runs must respect these keys.
- Responses: p03 -> pending; p06 -> pending (not started)

## Manager log
- 2026-10-31: wave 1 started. Phase roots p01, p02, p05 spawned (Opus 5.5 / high, general-purpose subagents of the manager session) from main e6b0a51. Each spawns its own Opus 5.5 / xhigh reviewer.
- 2026-10-31: user instruction — do not start further phases (03, 04, 06, 07, 08) after wave 1 completes. Merges to main held pending user confirmation.
- 2026-11-01: revision 3 — status rows for p01 (4d940da) and p02 (7ca2fb5) updated in place under r2 without a bump; no instruction changed (I001–I004 unchanged).
- 2026-11-01: wave 1 complete; p01/p02/p05 accepted in phase, committed on branches, not merged. Merges await user.
- 2026-11-01: revision 4 — wave 1 merged into main (9d68a6b, fbf56e9, 48626a1; conflict in message_publish.rs resolved, two test fixtures adapted). Merged targeted union: 1586 passed, 1 failed (fixed: p02 test vs p05 generated column), then message_park_continuation_ 5/5, fairness 17/17; sqlx prepare --check passes. User resumed next phases: wave 2 p03, p04 started from 48626a1.
