# 04.8 acceptance record (root)

## Group A — ACCEPTED 2026-10-15

- Artifact: HEAD `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006` + worktree (unstaged,
  no untracked source) identified by `logs/groupA/source-manifest-D004.json`
  SHA256 `64ce8860703195366dd8c14b838ac81951d040971fa7183079f457c40bd4935f`
  (15 files; root re-hashed all 15, 0 mismatches, `git diff --name-only 8533a79`
  lists the same 15, nothing staged). Group A production/tests from `67d319b`
  unchanged from the prior reviewed manifest `9148f4c1…`.
- Contents: lock-order correction + both-order tests (prior independent review,
  adopted by S in R-001); user-decided Clippy 1.99 remedy (D-002, R-003); test-side
  lease/timing corrections for reconciliation suites (D-003/D-004, DEC-009..011).
- Review notes: R-001, R-002, R-003, R-004 (changes required), R-005 (PASS).
- Findings: F-001, F-002, F-003 — resolved in R-005.
- Checks (D, task fixture PG 18.6, stock 2 MiB): dispatch 20/20; receipts 12/12;
  control 29/37/7; cancellation 2; authority 3; full `workflow_action_` 310/310 ×3
  on final artifact; prior-bounds controls 9/9; fmt, diff-check, offline
  all-targets check, strict Clippy, SQLx prepare --check all PASS. See IMPLEMENTATION.md.
- Remaining for group C (not group A gaps): full stock-stack library suite,
  `lease-budget.sh`, `stack-budget.sh`, fresh migration/schema, `graft build`,
  combined integration review.

## Group B1 — ACCEPTED 2026-10-15

- Artifact: HEAD `8533a79` + worktree per `logs/groupB/source-manifest-B1r1.json`
  SHA256 `ef2e0ecd1ef49a5ebea6c0ddb60f42a92cb92d56b1a3300575b4513c5c9aafdb`
  (19 files = 17 tracked diffs + 2 untracked new test/support files; root re-hashed,
  0 mismatches, nothing staged).
- Notes: R-006 (changes required), R-007 (PASS). Finding F-004 resolved.
- Checks (D): B1 10/10 (corrected test ×3, negative control, oracle unit test),
  full `workflow_action_` 320/320, prior_bounds 9/9, control 29/29, fmt,
  diff-check, offline check, strict Clippy, SQLx check (B1; no SQL change after).
- Deferred by design to B3: applied-then-hung reconciliation recovery (R-006 Q3).

## Group B2 — ACCEPTED 2026-10-15

- Artifact: HEAD `8533a79` + worktree per `logs/groupB/source-manifest-B2r1.json`
  SHA256 `1b109cc661ccf7dd6b692f06f2882e7d8152325355931bd23d1d2184951eeace`
  (21 files = 18 tracked diffs + 3 untracked; root re-hashed, 0 mismatches,
  nothing staged).
- Notes: R-008 (changes required; DEC-012 added), R-009 (PASS). F-005 resolved.
- Checks (D): receipt-first ×3 + late-release negative control ×3 + oracle unit
  test; B1+B2 16/16; full `workflow_action_` 326/326; fmt, diff-check, offline
  check, strict Clippy; SQLx check at B2 (no SQL change after).
- Carry to B3 (R-008): distinguish ordinary completion from ambiguity recovery.

## Group B3 — ACCEPTED 2026-10-15

- Artifact: HEAD `8533a79` + worktree per `logs/groupB/source-manifest-B3.json`
  SHA256 `c3379ee330dd4795f1f78a4758aa80c4a832b001ee28c845b015e5881f115131`
  (24 files = 19 tracked diffs + 5 untracked; root re-hashed, 0 mismatches,
  nothing staged).
- Notes: R-010 (S2 onboarding/guidance confirm), R-011 (PASS). No new findings.
- Checks (D2): new tests isolated ×3 18/18; cancellation 24/24; boundary 21/21;
  reused reconciliation/receipt/lease filters pass; control 29/29; full
  `workflow_action_` 332/332; fmt, diff-check, offline check, strict Clippy, SQLx.

## Group C and point 04.8 — ACCEPTED 2026-10-16
- C1 artifact `e59bb3f0…` (HEAD `94c77e0`): full suite 2558/0/22, bin 5/5,
  stack-budget 0, lease-budget 88/88 on a quiet host. S3 R-014/R-015: F-006
  resolved; F-007 (RunDeadline witness) and F-008 (graft build rerun) open.
- F-007 correction implemented by D2 (test-only, `action_dispatch_remote_tests.rs`);
  targeted runs pass. C2 full gates: first run load-invalidated (bizi44 builds,
  load ~20, aborted; logs `*-loaded-aborted`). Quiet-host rerun pending.
- Next: C2 gates → `graft build` → manifest C2 → S3 recheck F-007/F-008 and
  final integration PASS → root acceptance of 04.8.

### Final acceptance (root, 2026-10-16)

- Final artifact: HEAD `94c77e0` + uncommitted F-007/F-009 test edits
  (`action_dispatch_remote_tests.rs`, `persistence/task/tests.rs`), identified by
  `logs/groupC/source-manifest-C2.json` SHA256
  `5f8181076864e1fe0e1184380bdac8949987b8685faca8333e7bc51c389debf0`
  (30 files vs `8533a79`; root re-hashed, 0 mismatches, nothing staged).
- Reviews: R-014..R-017 (S3 `thr_4e8he92jaz`); R-017 PASS for C2 corrections and
  final combined integration. Findings F-001..F-009 all resolved.
- Gates on final binary `44ada14f…` (attempt f2, quiet start): stack-budget full
  suite 2559/0/22 (S-accepted full-suite evidence), bin 5/5, lease-budget 89/89,
  live SQLx check, fmt/diff-check/offline check/strict Clippy, `graft build`.
  Fresh migration/schema identity from group C remains valid (no migration/query
  cache change since). Load-invalidated runs (first C2, f1) and superseded a1 are
  retained in logs/groupC with their dispositions.
- Production changes in 04.8: group A action-authority lock order (company before
  run); F-006 wait park returns persisted deadline; Clippy 1.99 crate allow +
  `try_update` renames. All else test-side.
- Known gaps (not blockers): CI `scripts/test-network-isolation.sh` (sudo iptables)
  not run locally, so egress denial is unverified here; 22 pre-existing ignored
  tests excluded as in CI; f2 had intermittent foreign load (no failures).
- Not done / not authorized: committing the two uncommitted test edits, 04.9,
  migration squash, deployment.
