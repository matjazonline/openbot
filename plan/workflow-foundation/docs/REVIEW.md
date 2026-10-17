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


## 04.9 — implement-astra-reviewed-2.1
### 04.9-R001 — 2026-10-17 architecture revision 04.9-D1
CHANGES REQUIRED. See agent-review-notes.md#04.9-R001 and findings 04.9-F001–F003 in agent-review-findings.md. Original decision retained in logs/04.9/review-D1.txt. Current code inspection establishes company-before-run locks, owner/admin workflow authority and a blanket tool-publication rejection that the gated integration contract must reconcile. No implementation acceptance; no tests/fixture mutations performed.


### 04.9-R002 — 2026-10-17 architecture revision 04.9-D2
**PASS — exact decision revision 04.9-D2**, retained in logs/04.9/review-D2.txt. F001–F003 resolved at decision level; see appended resolution history and round note. Reviewed correction delta and affected publication replay/callers; unchanged R001 evidence reused. Architecture may proceed to implementation; no code/test acceptance claimed.

04.9 user-stop checkpoint (2026-10-17): D2/R002 architecture PASS remains the only
04.9 independent acceptance. Implementation is NOT reviewed or accepted. Current
expanded focused compilation was user-stopped status143; earlier11PASS does not
cover latest source. See HANDOFF-04.9-USER-STOP-20261017.md and TASK; resume requires
new instruction. Checkpoint digest:
`e64f7016555ec87c656b2198e309ecb707ccc47e1915f6c093667500d3154cd9`.


### 04.9-R003 — 2026-10-17 whole-batch code/integration review
**CODE REVIEW PASS** for base `e634a82837d50e7ed4aba4bb003f0a98ec294b10`, digest `7b68a7fcdb1c96d8a1e683feae8bc0008b945573c6f846f72cd30b39aa31ab1a`; full delta from `2729f2caaf58830bea8291bdcc6f9882a3ad03ee` reviewed. See [round note](agent-review-notes.md#049-r003--whole-batch-code-and-integration-review--code-review-pass-final-gates-pending). No new findings; F001–F003 decision corrections are implemented. Current HTTP20/20, credential12/12 and inspected schema/static/build logs pass. Full all-target suite, stock-stack and lease-budget gates remain pending; **no final acceptance**. Same reviewer handle retains context for final evidence follow-up on unchanged code.


### 04.9-R004 — 2026-10-17 final verification/documentation follow-up
**PASS — final combined 04.9 artifact**, base `e634a82837d50e7ed4aba4bb003f0a98ec294b10`, digest `1ad9356a5a29195853d8a5be8e53e4e384465e3365db0ed456ea35f35c76f08b`, same exact exclusions. R003 unchanged code review reused; RESUME/BRIEF status delta reviewed. Final snapshot independently recomputed. Full all-target library2579/0/22 plus bin5/0 under network guard, stock2048KiB2579/0/22, lease89/0 with maximum work35.5% below50%, all schema/build/static gates and graph PASS. The22 existing optional/live/developer ignored tests are not counted as coverage. No new finding or outstanding required check; F001–F003 remain fixed. See R004 in agent-review-notes.md and final finding disposition. Root may accept and shut down its task fixture.


### 04.9 root final acceptance — 2026-10-17
**ACCEPTED.** Root verified R003/R004 PASS, persisted notes/findings and resolved F001–F003, all required gate results, and unchanged final digest `1ad9356a5a29195853d8a5be8e53e4e384465e3365db0ed456ea35f35c76f08b` (base `e634a82837d50e7ed4aba4bb003f0a98ec294b10`, five exact exclusions retained in TASK). Manifest: `logs/04.9/crash-accepted-snapshot.txt`. Full library2579/0/22existingignored, bin5/0, stock2048KiB2579/0/22, lease89/0/max35.5%<50%; all remaining gates PASS. No required verification gap. Task-only PostgreSQL18.6 fixture stopped cleanly, data retained at `/private/tmp/workflow049-crash-20261017`; no active author checks. Scope04.9 complete; later phases require a new instruction. Unrelated edits and the original unavailable external-volume database path preserved.
