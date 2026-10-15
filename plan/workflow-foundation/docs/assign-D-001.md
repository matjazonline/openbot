# Assignment D-001 — developer: complete group A verification for 04.8

Role: Developer (D) in the bb-implement-astra-opus workflow. Fixed settings:
provider claude-code, model claude-opus-5-5, reasoning high. Parent/root thread
`thr_8m3bvxma9d`. Perform only the developer role; never spawn agents or threads;
report via your final response (no `bb thread tell` to the parent).

Skill (read the D-relevant parts and `references/review-records.md`):
`/tmp/bb-provider-bridge-provider-claude-code-ILWLzX/skills/bb-claude-skill-plugins-SpplXt/9aa755def426bba4/skills/bb-implement-astra-opus/SKILL.md`

## Read first

- `/home/mh/dev/openbot/AGENTS.md` and applicable `src/**/AGENTS.md`; use graft per AGENTS.md.
- `/home/mh/dev/openbot/plan/workflow-foundation/docs/TASK.md` (root record).
- `/home/mh/dev/openbot/plan/workflow-foundation/HANDOFF-20261014-04.8.md`
- `/home/mh/dev/openbot/plan/workflow-foundation/BRIEF-04.8.md` (accepted design; §A and §C).
- `/tmp/workflow-foundation-20261013-04.8/author-impl/AUTHOR-HANDOFF.md` and `EVIDENCE.md`
  (exact prior commands, fixture credentials, run-focused.py, logs).

## Context

Group A (company-before-run action authority lock order + both-order tests) is
committed in HEAD and matches the independently reviewed manifest (code PASS).
It is NOT accepted: runtime verification is incomplete. Completed already at
stock 2 MiB: `workflow_action_cancellation_` 2 PASS, `workflow_action_authority_`
3 PASS. Everything else is outstanding.

## Task (no source edits expected)

1. Restart ONLY the task fixture container `workflow-foundation-048-20261013`
   via an escalated `newgrp docker <<'EOF' … EOF` shell (the session's groups
   are stale). Verify container ID, image digest, system identifier, readiness,
   and that only task connections exist. Inventory leftover disposable databases
   from interrupted tests before cleaning only task-created ones.
2. Confirm the prebuilt test binary matches current source (rebuild with the
   repo's normal compiler stack if in doubt; stock 2 MiB applies to running tests
   via RUST_MIN_STACK=2097152, not to rustc — see handoff for why).
3. Run at stock 2 MiB with DATABASE_URL and TEST_DATABASE_URL pointing to the
   task fixture: the interrupted `workflow_action_dispatch_` filter, receipt
   filters (operational errors, refused return, and all `workflow_action_receipt`
   tests), and the affected action/control/reconciliation filters listed in the
   handoff/brief. Zero-test filters are failures of the runner, not passes.
4. SQLx: `cargo sqlx prepare --check` (with all targets as CI does) against the
   live task URL; record any cache delta. Then `cargo fmt --all -- --check`,
   `git diff --check`, `SQLX_OFFLINE=true cargo check --locked --offline --all-targets`,
   and strict Clippy as CI defines it (read `.github/workflows/ci.yml`).
5. If any check fails, diagnose. Do not change consequential design (BRIEF-04.8
   decisions) — return evidence to root instead. Routine fixes (e.g. lint) are
   yours, but any source edit creates a new artifact needing S recheck; record it.

## Records you own

Write `/home/mh/dev/openbot/plan/workflow-foundation/docs/IMPLEMENTATION.md`
(group A verification section: exact commands, env, fixture identity, binary
SHA256, results, log paths) and put logs under
`/home/mh/dev/openbot/plan/workflow-foundation/docs/logs/groupA/`.
Do not edit TASK.md, REVIEW.md, or review notes/findings.

## Stop boundary

Stop after group A verification is complete (or blocked). Leave the fixture
running (root will reuse it for group B). Final response: per-check PASS/FAIL
with log paths, any source edits (with new hashes), and blockers. Not authorized:
commits, 04.9, migration squash, deployment, touching non-task databases.
