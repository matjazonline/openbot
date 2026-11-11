# p05 Integration and final verification — TASK

## Identity

- Plan / phase: `plan/01-departments/`, phase `p05` (document `05-integration-and-final-verification.md`)
- Workflow: `implement-opus-reviewed-2_2` (skill dir `/Users/mac02/.claude/skills/implement-opus-reviewed-2_2`)
- Record root: `/Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/phases/p05/docs/`
- Root = Author (A): session 68933786, Opus 5.5 / xhigh (user confirmed earlier in the session).
  Reviewer (B): agent `ad409b2d3d149c2c6` (Opus 5.5 / xhigh), resumed from p01–p04.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`.
  Phase start c42fc15d (p04 B3 accepted and committed; p01–p04 accepted in phase).
- Snapshot exclusions: `plan/01-departments/phases/p05/docs` `plan/01-departments/COORDINATION.md`
- Authorizations: user 2026-11-10 "pull main and continue" (I002 r2): phase 05 after 04. Local
  commit once accepted; no push.
- Coordination: authoritative `plan/01-departments/COORDINATION.md`, revision 9 at start. I001 r1
  (no backward compatibility), I002 r2, I003 r2 (the full gate runs once, here). C001 and C004 r1
  keep plan 05 open: p05 checks the shipped docs and the trusted-relay/deferred-dispatch state hold
  in the integrated tree.
- Resources: gate database `openbot_dept_p05_gate` (DATABASE_URL for `scripts/check.sh`; dropped at
  phase close); `check.sh` creates and drops its own `_check_<pid>` test database. Root
  `node_modules` is a gitignored symlink to `/Users/mac02/dev/openbot/node_modules` (lockfile
  identical). `integrations/cloudroom-plugin/node_modules` via locked `npm ci` (gitignored).
  Environment: `RUST_TEST_THREADS=4` (local Postgres lock memory), `CHROME_PATH` = system Chrome
  for node browser tests.

## Requirements

- Plan 05: final gate once after integration (05A items 1–6) and the completion record.
- Plan 05 preamble: the `workflow_provisioning_commands` table comment lists `agent:{agent_id}`
  and `department:{id}` (present since p03; p05 corrects its button label to "Create additional
  desk", renamed in p04 B1).
- AGENTS.md: full verification suite once the task is implemented (formatting, offline
  compilation, migrations, database-backed tests); record unavailable checks; do not claim full
  verification when a required check could not run. Raised bounds keep a failing check.

## Batch

- **B1** = 05A gate plus any fixes it forces, and the completion record. No architecture gate
  (no new decision; fixes are reviewed in the batch review).

## Gate plan (05A)

1. Fresh disposable DB `openbot_dept_p05_gate`; `SQLX_OFFLINE=false cargo sqlx prepare --
   --all-targets` against it; `.sqlx` must not change.
2. `DATABASE_URL=…/openbot_dept_p05_gate RUST_TEST_THREADS=4 scripts/check.sh` (all gates, no
   fail-fast), log `logs/B1-check.txt`.
3. CI gates check.sh omits: lease budget (`scripts/test-network-isolation.sh scripts/lease-budget.sh
   --offline`), SSE/node suites CI adds (`workflow-runs.mjs`, `workflow-conversations.mjs`,
   `workflow-samples.mjs`), cloudroom plugin build/typecheck/tests/guards, run-counts and browser
   protocols, integration conformance families, knowledge/doop/memory-service gates. Each is
   recorded as PASS, FAIL or UNAVAILABLE with its reason.
4. `graft build`.

## Gate results (partial — paused 2026-11-10 at the user's "stop at safe checkpoint")

| Gate | Result | Evidence |
| --- | --- | --- |
| SQLx metadata regenerated on fresh DB (`cargo sqlx prepare -- --all-targets`) | PASS, `.sqlx` unchanged | `logs/B1-sqlx-prepare.txt` |
| cloudroom plugin `npm ci`, typecheck, build, test, test:guards | PASS (npm 11.17.0; package.json pins 11.19.0) | `logs/B1-cloudroom-npm-ci.txt`, `logs/B1-cloudroom-plugin.txt` |
| cloudroom reproducible package + locked runtime install | UNAVAILABLE: `package.mjs` asserts Node v24.21.0; only v26.5.0 here (brew `node@24` links to 26.5.0). Branch leaves `integrations/` untouched. | `logs/B1-cloudroom-package.txt` |
| CI-only node suites (`workflow-runs`, `workflow-conversations`, `workflow-samples`) | PASS 17/17 | `logs/B1-node-extra.txt` |
| Run counts browser protocol | PASS 2/2 | `logs/B1-run-counts.txt` |
| knowledge-model-prepare.py, doop-test.py, doop-probe.mjs | PASS | `logs/B1-py-node-gates.txt` |
| Knowledge production image, doop-integration job | UNAVAILABLE: no Docker | — |
| memory-integration job | UNAVAILABLE: local Postgres has no pgvector; pinned model not downloaded. Branch leaves `services/` untouched. | — |
| `scripts/check.sh` (DATABASE_URL=openbot_dept_p05_gate, RUST_TEST_THREADS=4) | STOPPED during "Compile tests" (user pause); no summary printed | `logs/B1-check.txt` |

Read from `logs/B1-check.txt` before the stop (no summary line, so from gate output):
- Generated CSS, SSE navigation (8 pass), deployment scripts, formatting, patch whitespace,
  transport boundary, evidence validator fixtures, fetch, offline compilation, clippy, migrations,
  SQLx metadata check: no error output.
- **Retained workflow evidence: FAIL** — `evidence hash mismatch: logs/B1-css-check.txt` in
  `phases/p04/docs/logs/01DEP-P04-B1-R1.evidence.json`. The log was rewritten for B1-R2 (whose
  manifest validates) after R1 pinned it; only the R2 form was committed (399efe2c). A p04 record
  defect found by the gate (01DEP-P05-B1 gate finding G1), not a code regression. Fix next
  session without touching R2's evidence: e.g. give the R1 manifest's CSS check a retained file
  of its own, or mark that entry superseded in the way `scripts/check-workflow-evidence.py`
  accepts — read that script first. Then rerun the gate.
- Not reached: compile tests, source-module reachability, test database, DB-backed suite
  (isolated network), stack budget.
- Not yet run (CI-only, need check.sh's build first): integration conformance families, vault /
  html-injection / knowledge / memory-settings / agent-authoring / local-agents /
  workflow-agent-picker / session-revocation / decision browser protocols, lease budget.
