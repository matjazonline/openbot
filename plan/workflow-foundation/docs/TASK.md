# 04.8 continuation under bb-implement-astra-opus — task record

Root: thread `thr_8m3bvxma9d` (claude-code, Opus 5.5; effort not authoritatively
confirmed, so root is a thin coordinator and fills no role). Started 2026-10-15.

## Original requirements

- [Phase 04](../04-actions-http-and-delivery.md) execution item 7 and its acceptance
  section (competing dispatchers, stale workers, duplicate callbacks).
- [EXPANSION](../EXPANSION.md) point 04.8.
- Accepted architecture: [BRIEF-04.8](../BRIEF-04.8.md), SHA256
  `3f76f3181278bfff7744a47e6701add420fe22d0090f6145a6a0eac89885cd04`
  (independent PASS `/tmp/workflow-foundation-20261013-04.8/reviewer/ARCHITECTURE.md`).
- Prior state: [handoff](../HANDOFF-20261014-04.8.md),
  [continuation](../CONTINUATION-20261013-04.8.md), author handoff
  `/tmp/workflow-foundation-20261013-04.8/author-impl/AUTHOR-HANDOFF.md`.
- Repository rules: `/home/mh/dev/openbot/AGENTS.md` and `src/**/AGENTS.md`.

Not authorized: 04.9, migration squash, commits, deployment, production data.

## Baseline and preserved state

- Group A source was committed by the user in `67d319b`; current HEAD
  `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006`, worktree clean apart from
  untracked dotfiles/IDE dirs (preserve; not task content).
- Group A artifact = HEAD content of the seven files in reviewed manifest
  `source-manifest.json` SHA256 `9148f4c1…5b57`; all seven re-hashed OK
  2026-10-15 ([log](logs/groupA-source-reconcile.txt)). Base for group A diff: `5b3d7dc`.
- Prebuilt test binary `target/debug/deps/mail_agents-43f563ba6b6c24ac` present
  (recorded SHA256 `f14fb5cd…7e`; D must re-verify before reuse).

## Batch order

1. **Group A verification** (code already PASS by prior independent reviewer):
   dispatch, receipt operational-error/refused-return and affected
   action/authority/control/reconciliation filters at stock 2 MiB; SQLx
   prepare/check; fmt/diff/offline check; strict Clippy. Root acceptance after.
2. **Group B** boundary matrix (BRIEF-04.8 §B) — S guidance, D expansion/impl,
   S whole-batch review. Starts only after group A acceptance.
3. **Group C** final combined gates (BRIEF-04.8 §C), S integration review.

## Workers

| Role | Thread | Provider/model/effort | Assignment |
| --- | --- | --- | --- |
| S (retired 82% ctx, after R-009) | `thr_xku2jgixtv` | codex / gpt-6-astra / high | stopped |
| D (retired 64% ctx, after B2r1) | `thr_6iqmr9bdad` | claude-code / claude-opus-5-5 / high | stopped |
| S2 (retired 83% ctx, after R-012) | `thr_4pqv29gbbd` | codex / gpt-6-astra / high | stopped |
| S3 | `thr_4e8he92jaz` | codex / gpt-6-astra / high | C1 review + group C integration review |
| D2 | `thr_2n9nrfy6ra` | claude-code / claude-opus-5-5 / high | B3, group C, C1 |

## Resources

- D owns builds, tests, and the task PostgreSQL fixture
  `workflow-foundation-048-20261013` (ID `f4e50996…dc40`, 127.0.0.1:55448,
  system id `7696100747480141867`). Docker requires an escalated
  `newgrp docker <<'EOF' … EOF` shell. Never touch other databases/containers.
- S is read-only on project files and fixtures; it may run non-mutating reads.

## Status / next action

- Group A: ACCEPTED 2026-10-15 on manifest `64ce8860…` (see REVIEW.md).
- Group B: B1 ACCEPTED (`ef2e0ecd…`), B2 ACCEPTED (`1b109cc6…`); B3 ACCEPTED (`c3379ee3…`).
- Group C: ACCEPTED 2026-10-16 (manifest `5f818107…`, R-017). **04.8 complete.**
- Next: 04.9 only under a new user instruction. Workers stopped; fixture container
  `workflow-foundation-048-20261013` stopped with data retained.

## Decisions log

- 2026-10-15 user: strict Clippy on Rust 1.99 — add crate-level
  `allow(clippy::double_must_use)` with a reason comment (async-trait expansion)
  and rename the 5 `fetch_update` calls to `try_update`. Assigned D-002.
- 2026-10-15 root: `workflow_action_` full-filter failures under load (inherited,
  reproduced on base) routed to S as S-002 for diagnosis/correction criterion.
- 2026-10-15 user: fix the 5 Linux timestamp-precision test failures within 04.8
  (test-only unless S finds a production precision bug). Assigned S-012 → D.
- 2026-10-15 user: timing-sensitive gates — rerun when host load is lower
  (user's other project bizi44 was running cargo test, load ~19 on 8 cores), or
  change verification so it does not depend on timing. S-012 assesses the latter.
- 2026-10-15 user: include F-006 production fix (wait park returns persisted
  deadline) in 04.8. Batch C1 = DEC-013 precision fixes + F-006 + DEC-014 test
  stabilization; full-suite/budget reruns on a quieter host. Assigned D-011.
- 2026-10-15 21:01 user committed `cfef3d8` "workflow-foundation 4.8" (accepted
  B3 content + records). Manifests keep comparing worktree content against
  `8533a79`; C1 freeze records HEAD `cfef3d8` as well.
- 2026-10-16 root: F-009 (inherited stop-task test expectation defect, test-only)
  fixed within 04.8 as it blocks the required full-suite gate (same policy as the
  user's precision-fix decision). S3 accepts a clean stack-budget full-suite run as
  full-suite evidence. Assigned D-014.
