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


## 04.10 — implement-opus-reviewed-2.1

### 04.10-R001 — 2026-10-17 architecture gate 04.10-D1
**CHANGES REQUIRED** for decision 04.10-D1: `logs/04.10/decision-D1.md`, SHA256 `3f10553022b5d631876b8174d57d0ea270ff3c7d150b25d5812fd776e285035e` (verified), base `8ce1fd4`. Workflow implement-opus-reviewed-2.1; Reviewer B Opus 5.5. See the [round note](agent-review-notes.md#0410-r001--architecture-gate-0410-d1--changes-required) and [findings](agent-review-findings.md).

Actionable findings:
- 04.10-F001 (medium): outcome table for pre-marker and non-committed paths, so revocation, approval and crash do not become unreconcilable reconciliation waits.
- 04.10-F002 (low): the `mcp.tool_error` safety label contradicts the shared DB retry-safety predicate.
- 04.10-F003 (medium): the race/recovery matrix and the affected-suite/stock-stack gate are incomplete.

Optional findings: 04.10-F004 to F007.

The core architecture holds up: a single MCP owner, immutable endpoint, operations table with grant-FK cascade, routing through publication/binding/admission, company-first lock order, the eager/lazy possible-dispatch boundary, envelope validation and cleanup. The corrections are decision text and verification scope. This was inspection only: no tests run, no fixture/DB access, no project edits, and no implementation acceptance.

### 04.10-R002 — 2026-10-17 correction recheck 04.10-D2
**CHANGES REQUIRED** for decision 04.10-D2: `logs/04.10/decision-D2.md`, SHA256 `84d532eec3476569a878bca7188ed914ecde7f8ff9f09e4b462679e55330ded5` (verified), base `8ce1fd4`. D1 is unchanged at `3f105530…035e`. See the [round note](agent-review-notes.md#0410-r002--correction-recheck-0410-d2--changes-required) and [findings](agent-review-findings.md).

Status of the R001 findings:
- 04.10-F001, F002 and F003 are resolved at decision level.
- F004's decline is accepted as a documented limitation.
- F005 is resolved.
- F006 and F007 were adopted, but each introduced a new defect.

New actionable findings:
- 04.10-F008 (medium): the accepted 04.9 HTTP handler parks pre-marker failures in a reconciliation wait that nothing can settle. D2's shared classifier makes http.request inconsistent: a crash before the marker retries, while the same refusal returned by the handler waits until the deadline. Recommended: align the HTTP handler in this batch, or root records it as a blocker for 04.11.
- 04.10-F009 (low): order the permits endpoint-first so a waiter never holds a global permit.
- 04.10-F010 (low): make the operation revision ABA-safe across revoke and cascade delete.

Optional: 04.10-F011 (record the classifier's handler-composition invariant).

The shared `marker_owned_effect_kind` change to `recovery::report` and `pending_recovery::unknown_effect` was reviewed for regressions and found sound. This was inspection only: no tests, no fixture/DB access, no project edits, and no implementation acceptance.

### 04.10-R003 — 2026-10-17 correction recheck 04.10-D3
**PASS (architecture gate) for decision 04.10-D3**: `logs/04.10/decision-D3.md`, SHA256 `f89b0a89afa11ec9b789ce8e25fc3370a9678bbb43ff1e60c7dcb3e050e7b32b` (verified). The effective gated decision is **D2 (`84d532ee…`, unchanged) as amended by D3**; D1 is unchanged. Base `8ce1fd4`. See the [round note](agent-review-notes.md#0410-r003--correction-recheck-0410-d3--pass-architecture-gate-f012-carried-to-implementation) and [findings](agent-review-findings.md).

Findings:
- 04.10-F008, F009 and F010 are resolved at decision level; F011 is adopted as documentation.
- The D2 row-4 side effect (MCP pre-marker reservation refusal costs one extra attempt, after which eager preparation rejects without network) is acceptable.
- The shared `action_step_failure` mapping changes accepted 04.9 HTTP behaviour only as intended; its effective decisions match.
- **04.10-F012 (low, actionable)**: slotted `http.request` preparation does not check connection readiness, so a disabled connection or removed secret retries until attempts are exhausted instead of failing as `http.rejected`. It is carried to implementation and must be fixed and tested before batch acceptance.
- Optional: 04.10-F013 (name the changed accepted assertion `http_tests.rs:100`; refine the I9 wording for evidence-conflict waits).

Implementation may proceed. This was inspection only: no tests, no fixture/DB access, no project edits, and no implementation acceptance.

04.10 user-stop checkpoint (2026-10-17): architecture gate D2+D3 accepted via R003 only;
implementation unverified, not frozen, not code-reviewed; F012 open. Checkpoint digest
`6d5b89b7a612505bf209e74b18a3483a5999d5920ecb905bca7f4a8979de0c30`. See
HANDOFF-04.10-USER-STOP-20261017.md. Author proposes an application-level substitute for the
lazy post-not-applied MCP matrix case (no MCP evidence verifier) — reviewer to judge on resume.


### 04.10-R004 — 2026-10-17 whole-batch code/integration review
**CHANGES REQUIRED (verification); code inspection PASS.** Workflow `implement-astra-reviewed-2.1`; independent Reviewer B (requested gpt-6-astra/medium). Base `8ce1fd49b5a961da705fddf0b92cac3182da427b`, digest `cb1b1dba9f6a803d7e35308917786c2863e89e73e7abddd4d2f722f2927a6210`; sole exclusion `plan/workflow-foundation/docs`, independently matched before/after review. See [R004 note](agent-review-notes.md#0410-r004--whole-batch-codeintegration-review) and [findings](agent-review-findings.md).

Whole04.10 implementation and HTTP alignment were inspected against original requirements and D2+D3. No new code defect found. F012 is **FIXED** with real HTTP readiness/one-attempt retirement evidence; previous decision corrections are implemented. The lazy not-applied matrix uses the real DB reconciliation service plus a test-host verifier and real transport, satisfying the planned seam. F013's HTTP effective-state assertion and evidence-conflict wording are adopted.

New blocking finding **04.10-F014 (medium, verification)**: required scoped2048KiB gate failed390/3; the three focused rechecks passed, but the full retry failed391/2 on different existing action-protocol tests. Cause remains unestablished; logs retained. Fresh migration/schema comparison and focused504/0/0 pass; stock and subsequent SQLx/build/static gates do not yet pass as a complete gate. No final batch acceptance. Root may investigate/correct and request a single affected-delta/evidence recheck; reviewer handle retained. No04.11 review performed.


### 04.10-R005 — 2026-10-17 correction/evidence recheck — PASS
**PASS for batch04.10**, base `8ce1fd49b5a961da705fddf0b92cac3182da427b`, digest `2be42e3e5d8b0651ad1d6bc523cf356dd6bb55ac254050b99914bede669ebc4c`; sole exclusion `plan/workflow-foundation/docs`, independently recomputed before/after checks. Same independent Reviewer B, workflow `implement-astra-reviewed-2.1`; R004 unaffected code coverage reused. [R005 note](agent-review-notes.md#0410-r005--f014-correction-and-evidence-recheck), [finding dispositions](agent-review-findings.md).

F014 **RESOLVED**: test synchronization corrections preserve GUIDANCE ordinary-race oracles and negative controls; affected stock12/0/0 and corrected complete scoped/default-parallel stock2048KiB393/0/0 pass. All previously failing tests pass in the complete corrected run. SQLx prepare/check, offline all-target compilation, strict Clippy, fmt/diff, transport/regression, credential CLI and graph gates pass; fresh migration/schema equivalence and focused504/0/0 evidence retained. F012 stays fixed; no outstanding04.10 blocker or new finding. Previous failures are preserved as history.

Root may accept04.10. This is not overall task completion: full combined all-target tests, full stock-stack and lease-budget gates remain the04.11 final gate. No04.11 architecture or implementation reviewed here; same reviewer handle retained.

Root acceptance2026-10-17: independently matched2be42e3e…bc4c after R005 final PASS,
verified persisted notes/dispositions and batch gates. **04.10 ACCEPTED**. Next04.11
architecture gate, then one cohesive implementation review and combined final gates.


### 04.11-R001 — 2026-10-17 user-stop architecture checkpoint — INCOMPLETE
**NO PASS; architecture gate unfinished.** Reviewed portions of `logs/04.11/decision-D1.md`, SHA256 `ebb523dfdfcf5d7b937ddf48b47161bb79b70ad9bc2d890b511f63ef58b8ad83` (independently matched before review and checkpoint). User requested safe handoff and STOP before grouped findings/final verdict. No04.11 implementation exists;04.10 R005 acceptance remains valid.

[Round note](agent-review-notes.md#0411-r001--architecture-review-interrupted-by-user-stop-incomplete) and [provisional findings](agent-review-findings.md) preserve F001 recovery/owner integration, F002 unestablished action bound, F003 subject byte limit and optional F004 cursor synchronization test. Author already identified F002/F003, but proposed corrections are not yet gated. Resume by completing this architecture review and obtaining a verdict before implementation. Inspection only; no DB/cargo checks or implementation edits. Reviewer stopped.


### Final user-stop checkpoint — 2026-10-17

**STOPPED at user request.** See [current handoff](HANDOFF-04.11-USER-STOP-20261017.md).
04.10 R005/root acceptance remains valid; 04.11 R001 is incomplete, NO PASS,
with provisional F001–F004 retained. No04.11 implementation. Reviewer returned
stopped. Root verified fixture system identifier7697669855432439122, stopped only
`/private/tmp/workflow0410-20261017` with pg_ctl fast stop, and confirmed no server
running; data retained. No author checks remain active. Worktree/evidence preserved;
no commit or deployment. Final handoff/RESUME edits are administrative documentation;
full combined tests/stock/lease gates remain pending. Further work requires a new
continuation instruction. Stop-checkpoint verification: `git diff --check`.

### 04.11-R002 — 2026-10-17 architecture gate 04.11-D1 (completes R001) — CHANGES REQUIRED
**No PASS** for `logs/04.11/decision-D1.md` SHA256 `ebb523dfdfcf5d7b937ddf48b47161bb79b70ad9bc2d890b511f63ef58b8ad83` (recomputed at review start; base `8ce1fd49b5a961da705fddf0b92cac3182da427b`). Reviewer B Opus 5.5, xhigh requested, fresh independent context. All D1 sections 1–6 reviewed by code/schema inspection; no checks run (assignment forbids builds/fixtures). No 04.11 implementation exists.

[Round note](agent-review-notes.md#0411-r002--architecture-gate-0411-d1-completed--changes-required). Findings:
- Actionable: [04.11-F001](agent-review-findings.md#0411-f001--r002-disposition-confirmed-revised-medium) medium — delivery-kind recovery/reconciliation outcomes across shared predicates (confirmed, expanded); [04.11-F002](agent-review-findings.md#0411-f002--r002-disposition-confirmed-medium) medium — no enforced action-count/consumption bound (confirmed); [04.11-F003](agent-review-findings.md#0411-f003--r002-disposition-confirmed-revised-low) low — subject 998-byte owner bound and destination count vs association bound (confirmed, revised); [04.11-F005](agent-review-findings.md#0411-f005--canonical-message-classification-and-per-recipient-provider-identity-are-unspecified) medium — message classification and per-recipient Message-ID/threading vs existing thread consumers (new); [04.11-F006](agent-review-findings.md#0411-f006--verification-plan-omits-ci-gates-and-the-tests-f001f005-require) low — final gate lacks network isolation/diff check and required tests (new); [04.11-F007](agent-review-findings.md#0411-f007--commit-time-and-receipt-proofs-are-not-database-derived) low — captured expiry and receipt result not DB-derived (new); [04.11-F008](agent-review-findings.md#0411-f008--effect-wait-deadline-and-resume-semantics-unspecified) low — effect-wait deadline/resume semantics (new).
- Optional: [04.11-F004](agent-review-findings.md#0411-f004--r002-disposition-confirmed-optional) — cursor/event-production invariant and test (confirmed optional; no lost-event defect).
- Author's unreviewed 16-page / 129-probe proposals remain unaccepted; F002 states the conditions a D2 bound must meet. Next: A revises D1 into D2 addressing all actionable findings in one revision; B rechecks by finding ID before implementation.

### 04.11-R003 — 2026-10-17 correction recheck of 04.11-D2 — CHANGES REQUIRED
**No PASS** for `logs/04.11/decision-D2.md` SHA256 `1dcd1a9e99e57cd32e15bf095490961722f185153c66945cd82d29bd4d943a44` (recomputed at start). This is a recheck by finding ID, broadened to the contracts D2 changed, including the reaper and lease-budget changes. Inspection only; no checks run.

[Round note](agent-review-notes.md#0411-r003--correction-recheck-of-0411-d2--changes-required); [dispositions](agent-review-findings.md#r003-dispositions-for-0411-f001-to-f008).
- Resolved at decision level: F001 (except the evidence path, now F010), F002, F003, F004 (optional, adopted), F005, F006, F007, F008.
- New, actionable:
  - [04.11-F010](agent-review-findings.md#0411-f010--message-deliveries-with-unknown-outcome-have-no-authorized-reconciliation-path) **medium**: no verifier-attested reconciliation path for message invocations, which fails item 6 and the reconciliation Acceptance line.
  - [04.11-F011](agent-review-findings.md#0411-f011--lease-budget-measurement-of-new-message-operations-undefined) **low**: the bound and leading-lock boundary for the new lease-budget operations are undefined.
- New, optional: [04.11-F009](agent-review-findings.md#0411-f009--sql-backstops-admission-count-race-and-step-close-identity-optional) low, SQL backstop tightening.
- D2 §5.3 judgement: **(b) fails the criteria.**
  - Item 5 is met.
  - Item 6 and the Acceptance line are not, under the accepted 04.7/04.10 reading. A test-host verifier through the real service is enough, and no production verifier is required (MCP precedent, BRIEF-04.10:60).
  - Only an explicit user scope reduction could accept it; the R003 note states the exact choice. Root has not waived it.
- Reaper change: sound. Lease-budget change: sound in direction (no threshold raised), but incomplete (F011).
- Next: A corrects F010 and F011 (F009 optional) in one revision, then B rechecks those IDs and the F004/§5.1 premises that F010 touches.

### 04.11-R004 — 2026-10-17 correction recheck of 04.11-D3 (delta over D2) — CHANGES REQUIRED
**No PASS** for the effective decision D2 `1dcd1a9e99e57cd32e15bf095490961722f185153c66945cd82d29bd4d943a44` + D3 `c462e4eb56cb76ac816fa107b7b01f7651706796b8723fed8f206c6315858a89` (both recomputed at start). This is a recheck of F009–F011 plus the premises F010 changes, broadened to the 04.7 schema D3 extends. Inspection only.

[Round note](agent-review-notes.md#0411-r004--correction-recheck-of-0411-d3-delta-over-d2--changes-required); [dispositions](agent-review-findings.md#r004-dispositions-for-0411-f009-f010-f011).
- Resolved at decision level: F009 (optional, adopted), F010, F011. The F004 premise and rows 11–13, 15–19 and 21 are consistent with the code. The remote 04.7/04.10 MCP branches are unchanged; the request-digest `target` field is a no-BC encoding change and is retested.
- New, actionable:
  - [04.11-F012](agent-review-findings.md#0411-f012--effect-wait-resume-bypasses-the-finality-breach-continuation-veto) **low**: the effect-wait resume bypasses the accepted finality-breach continuation veto (`completion.rs:106`).
  - [04.11-F013](agent-review-findings.md#0411-f013--message-evidence-shape-and-projection-gaps-against-the-047-schema) **low**: message `applied_request` attribution, the reopen-guard conflict check, effect-state precedence for row 16, and conflict orientation.
- D3 §L deferral (phase 06 reopens `model_tool`-origin agent executions): **acceptable** against phase 04 items 4–6 and Acceptance, under three conditions recorded in the note: BRIEF-04.11 carries the obligation and its trigger; message outcomes stay out of the shared `continue_on`; a port-seam test is added.
- Next: A amends with a small delta for F012 and F013, then B rechecks those two IDs only.

### 04.11-R005 — 2026-10-17 correction recheck of 04.11-D4 (delta over D2+D3) — PASS
**PASS at the architecture gate** for the effective decision:
- D2 `1dcd1a9e99e57cd32e15bf095490961722f185153c66945cd82d29bd4d943a44`;
- D3 `c462e4eb56cb76ac816fa107b7b01f7651706796b8723fed8f206c6315858a89`;
- D4 `6bf5efcb3de54949df516b45725c8c9d4fe8cc1ff968a38671a367862e1fa276`.

All three hashes were recomputed at start and at end. Inspection only.

[Round note](agent-review-notes.md#0411-r005--correction-recheck-of-0411-d4-delta-over-d2d3--pass); [dispositions](agent-review-findings.md#r005-dispositions-for-0411-f012-f013).
- Resolved at decision level:
  - F012: the effect-resume conflict veto, giving the Reconcile park that `complete_io` would.
  - F013 items 1–4.
  - The three R004 §L conditions.
- The renamed `DeliveryReopened` outcome (NULL `scheduled_job_id`) is required by the existing command CHECK and the claim-episode binding.
- No open actionable 04.11 decision findings. Optional F014 (low): clarify the NULL shape alternative, and add conflict refusal at publication/`begin_part`.
- This PASS does not cover implementation, tests, the combined phase 04 gate or the whole-batch review; all remain required. The lightly reviewed areas listed in R003–R005 are carried into the batch review.

### 04.11-R006 — 2026-10-18 whole-batch code, criteria and final phase-04 integration review — CHANGES REQUIRED
**No PASS** for snapshot base `5c1de32ecb3c1a7e59e1a60deb74ab5c42e755df`, digest `8891d74001e78bac6b65e0238c3d73ac25b92f3970178edb08f296417c75cabd` (recomputed). Effective decision D2+D3+D4. Inspection plus digest recomputation; gate results taken from the logs.

[Round note](agent-review-notes.md#0411-r006--whole-batch-code-criteria-and-final-phase-04-integration-review--changes-required).
- Actionable:
  - [04.11-F016](agent-review-findings.md#0411-f016--tests-required-by-the-accepted-decision-are-missing) **medium**: tests required by D2 §8.2 / D3 §K / D4 §S are missing, including the F012 breach-then-all-accepted variant, per-recipient inbound reply resolution, delayed-COMMIT rollback, SQL-only reopen negatives, the competing settle races and the model_tool `DeliveryReopened` seam.
  - [04.11-F015](agent-review-findings.md#0411-f015--unlisted-departures-from-the-decided-outcome-table-and-reply-relay) **low**: a pre-marker authority refusal becomes Retryable `message.unresolved` rather than the decided Terminal `message.rejected`, with no step-level revocation test; replies reset the relay trace against D2 §1.3.
- Optional:
  - [04.11-F017](agent-review-findings.md#0411-f017--expired-proof-path-commits-on-lease-loss-and-inspection-is-not-wired-optional): rollback on the expired-proof `LeaseLost` branch; inspection not wired.
  - [04.11-F018](agent-review-findings.md#0411-f018--timing-oracle-and-baseline-evidence-for-the-first-run-failure-optional): timing oracle and baseline evidence.
- Root judgements:
  1. None of the 18 deviations reopens the gate. Specifically: the fenced-completion guard keeps the original disjunct and admits only the exact park; the ambiguous-COMMIT intent read is a read-only resolution with replays still authorized; 40P01 keeps the at-most-128 invariant, and the decided 54000 interleaving is tested. Deviation 2 understates the reply case (F015).
  2. Most listed gaps are required tests and block (F016). The exceptions are `approval_required` (unreachable), the lock-count assertion (structurally bounded) and remote evidence with a subject (FK-impossible).
  3. The first-run failures were handled acceptably: the fixture now matches CI `pg_stat_statements` settings, and the accepted timing test is unchanged and passed 7 times on rerun (optional F018 caveats).
  4. The gate-deferred areas are sound: `completion_job`, the reconciliation Rust shapes, claim/episode/budget isolation and fair polling. Inspection is not wired (optional F017).
- Next: A fixes F015 and F016 (F017/F018 optional), reruns the affected focused areas plus full suite/stack/lease, refreezes, and B rechecks those IDs.

### 04.11-R007 — 2026-10-18 correction recheck (final 04.11 batch + combined phase-04 integration) — PASS
**PASS** for snapshot base `5c1de32ecb3c1a7e59e1a60deb74ab5c42e755df`, digest `e76ef46c07965bb7b233d21813ac413efb6c5ad763e207ac3bc242168235c2cf` (recomputed). Effective decision: D2 `1dcd1a9e…3a44` + D3 `c462e4eb…8a89` + D4 `6bf5efcb…a276`, with accepted amendments A1 (D4 §S receipted-variant uncertainty assertion) and A2 (drop "and `inspection`" from D3 §E).

[Round note](agent-review-notes.md#0411-r007--correction-recheck-of-the-0411-batch-final-0411--combined-phase-04-integration--pass); [dispositions](agent-review-findings.md#r007-dispositions-for-0411-f015-to-f018).
- F015, F016 and F017: resolved. F018: addressed, with the optional residual accepted.
- The row-2 boundary is correct: Terminal only for refusals with no marker; retryable with a marker or for other errors.
- The race tests use real competing claimants (separate transactions on the production locks, released together).
- Gate per `r006-*` logs: fmt, diff, transport, credential/deploy, offline, clippy, fresh schema, SQLx prepare/check, no-run, full suite 2674/0/22, stack 2669/0/22, lease budget 99 tests within bound, graft. Not run (disclosed): CI frontend/CSS/Playwright/SSE steps, the `queryid` probe, and the F018 stashed baseline.
- No open actionable 04.11 findings. This PASS covers the 04.11 whole-batch review and the combined phase-04 final integration review at this digest.

### 04.11 root acceptance — 2026-10-18
Root accepted 04.11 and the combined phase04 gate on R007 PASS for digest
`e76ef46c…c2cf` (base `5c1de32`), digest independently recomputed. See TASK.md
"04.11 ROOT ACCEPTED".

## Batch S — migration squash (implement-opus-reviewed-2.1)

### S-R001 — 2026-10-18 batch review of the migration squash — CHANGES REQUIRED
**No PASS** for snapshot base `4b54485830495dfeb68d45c61c0d6d8dd684e4c5`, digest `0aeeca933e66244420f6b05b37d645cc79facbc2307890591b187d0f9ac43fa5` (recomputed). Requirements: the user's 2026-10-18 instruction "squash migrations", root AGENTS.md no-backward-compatibility, and phase 05 "Schema and data". Method: inspection plus read-only fixture queries; gate results are taken from the logs.

[Round note](agent-review-notes.md#s-r001--batch-review-of-batch-s-migration-squash--changes-required); independent checks in `logs/S-R001/`.
- Actionable:
  - [S-F001](agent-review-findings.md#s-f001--the-removed-budget-replay-carried-the-only-assertion-of-the-sealed-root-debit-rule) **low**: the deleted budget replay test was the only assertion that a sealed root (a bundle that is not revision 2, or is malformed) has every positive debit `exhausted`. That rule is still live in the init schema. Add a fresh-schema test, or have root/user decide to remove the sealing branch in a reviewed phase-05 edit.
- Optional:
  - [S-F002](agent-review-findings.md#s-f002--init_function_body-reads-past-a-function-without-a-dollar-quoted-body-optional): `init_function_body` reads past functions without a dollar-quoted body. None is pinned, and the failure mode is a mismatch, not a false pass.
- Accepted as reviewed:
  - The equivalence proof. The full dump diff is empty. Both ledgers match the artifacts' checksums. A deparser-independent node-tree comparison shows only one OID-bearing `'{}'::citext[]` datum differing. The BETWEEN rewrite is correct.
  - The stripped dump items.
  - The dependents: CI, Dockerfile, `db.rs`, `.sqlx`, README, `reset-db.sh --all`, the harness script and the plans. No stale references remain.
  - The fresh-ledger test, which also makes an additive migration fail red.
  - The other six test removals.
  - The helper and its six repins, which reproduce `prosrc` exactly.
- Observation for the user: `src/adapters/persistence/AGENTS.md` §1 contradicts the squash instruction. Its lines 30 and 160 (additive and staged migrations) do too, and the proposed replacement does not cover them.
- Next: A adds the S-F001 test (or records the root/user decision), runs focused `workflow_budget_schema` plus fmt/clippy/no-run, refreezes, and B rechecks S-F001 by ID.

### S-R002 — 2026-10-18 correction recheck (S-F001, S-F002) — PASS
**PASS** for snapshot base `4b54485830495dfeb68d45c61c0d6d8dd684e4c5`, digest `33ae9028d28d3047de936b2e9580ffe267bd0187dafe2092601d0cef51514552` (recomputed). The init schema is byte-identical to S-R001's, so the S-R001 equivalence acceptance carries over.

[Round note](agent-review-notes.md#s-r002--correction-recheck-of-batch-s-s-f001-s-f002--pass); [dispositions](agent-review-findings.md#s-r002-dispositions-for-s-f001-and-s-f002); checks in `logs/S-R002/`.
- S-F001: resolved. A fresh-schema test covers both sealing branches, the exhausted debits for every resource with usage unchanged, and a `frozen_v2` control.
- S-F002: resolved. The helper is bounded to its own statement, with a `should_panic` pin.
- The `migrated_own_database` change is an equivalent single-caller inlining with the future still boxed. No stack-budget rerun is needed.
- Gate evidence: the `sr001-*` logs (focused 28 passed / 1 ignored; fmt, Clippy and no-run PASS) plus the S-R001 full gate for the unchanged remainder.
- The network-isolated full-suite rerun is not needed for this PASS. It was still running at review time with no failure so far; a failure in it would reopen the batch.
- The `src/adapters/persistence/AGENTS.md` conflict (§1 plus lines 30 and 160) remains open for the user.
- No open Batch S findings.

### S-R003 — 2026-10-18 delta recheck (user-approved AGENTS.md and startup warning) — PASS
**PASS** for base `feefe3e060a851fd8860e4bacb90c11c7b284888`, digest `03ca0bea615bfc261ce6191b60147d4df20f9bab8115d0ec5b2dbebbaa40cbae` (recomputed). The base reproduces the S-R002 digest `33ae9028…4552`.

[Round note](agent-review-notes.md#s-r003--delta-recheck-of-batch-s-user-approved-agentsmd-and-startup-warning--pass); checks in `logs/S-R003/`.
- `src/adapters/persistence/AGENTS.md`: §1, the composite-FK bullet and the credential paragraph now follow the pre-release in-place init-schema model, with the post-release rule kept. No other rule changed. This resolves the S-R001 AGENTS observation by the user's approval.
- The `serve()` warning is accurate and logged before migrations. Optional nit: say "recorded migrations or checksum". The file is still listed in `NO_BACKWARD_COMPATIBILITY_NOTES`.
- Stack budget: I agree with root's conclusion but not its premise. The code delta since the S-gate pass also includes the S-R001 corrections. This tree's two 2 MiB runs completed every library test with no overflow. Their timing failures are disjoint, so every test completed at 2 MiB at least once, and all five also pass alone. Record the result on that basis, with the command's exit 101 disclosed.
- No open Batch S findings.

## Phase 05 (implement-opus-reviewed-2.1)

### 05-R001 — 2026-10-18 architecture gate of 05-D1 — CHANGES REQUIRED
**No PASS** for `logs/05/decision-D1.md` SHA-256 `4e3a46ebb03137ba5c86e1b3d64e1d3b0abf756c54eaa59efc8cee8301707931` (investigation `484af70e…975e`; both confirmed). The user's answers relayed by root are applied: Q1 a list of 1–16 with first valid wins; Q2 a timeout over the `error` route; Q6 no external reviewers in phase 05; Q8 wiring in phase 08.

[Round note](agent-review-notes.md#05-r001--phase-05-architecture-gate-05-d1--changes-required).
- Medium:
  - [05-F001](agent-review-findings.md#05-f001--the-error-route-carries-no-failure-code-so-a-timeout-is-indistinguishable-in-graph): the error route commits `null` and no binding root carries a code. A timeout is therefore indistinguishable in-graph from invalid input and from `reviewer_unavailable`, and D1's plan-07 `data.map` claim is wrong.
  - [05-F002](agent-review-findings.md#05-f002--q6-removes-external-reviewers-d1-must-drop-the-credential-design-and-re-home-what-depends-on-it): drop the whole credential/external design without seams. Re-home its dependents (REPLACEMENT-MAP:72 "scoped single-use link", the phase 08 legacy-approval removal gate, the plan and EXPANSION text).
  - [05-F003](agent-review-findings.md#05-f003--notice-transport-must-be-re-decided-without-credentials-and-the-notice-audit-collides-per-execution): re-decide notice transport for members only; prefer standalone `notification` deliveries enqueued in the park transaction. Also, the `decision_notice_failed` per-execution audit collides on the unique index once two reviewers' notices fail.
- Low:
  - [05-F004](agent-review-findings.md#05-f004--member-decision-commands-take-the-company-row-for-no-key-update): use `companies FOR SHARE` for member commands.
  - [05-F005](agent-review-findings.md#05-f005--run-scoped-event-inbox-inexact-128-bound-and-unstated-late-event-semantics): the event bound is inexact under concurrency, and the late-event semantics are unstated.
  - [05-F006](agent-review-findings.md#05-f006--05b-is-direction-level-require-a-05b-decision-addendum-before-05b-code): a 05B decision addendum (risks I§5.1–5.3, approver eligibility) before 05B code.
  - [05-F007](agent-review-findings.md#05-f007--clarifications-to-carry-into-the-revision): clarifications.
- Accepted:
  - the two-batch split;
  - the wait row as lifecycle owner, and the decision and comment model;
  - settlement and continuation atomic through `batch_commit::complete(Parked)`;
  - the lock order, which is consistent with controls and the dispatch prelock;
  - the race table and competing-claimant tests;
  - the bounds signals;
  - the verification plan;
  - the questions: Q5 (params are admin binding snapshots; the check must be recursive), Q7 (05B), and Q4 as direction (eligibility decided in the 05B addendum).
- Next: A issues D2 resolving F001–F007; B rechecks by ID.

### 05-R002 — 2026-10-18 correction recheck of 05-D2 (over D1) — CHANGES REQUIRED
**No PASS** for `logs/05/decision-D2.md` SHA-256 `831890270a87273b2cbbccf580a942c4582841cc2c4430b15d542d95754815b3` over D1 `4e3a46ebb03137ba5c86e1b3d64e1d3b0abf756c54eaa59efc8cee8301707931` (both confirmed). This includes the user's decision of 2026-10-18 to drop email-link approvals with the legacy code, which is carried in D2 §2 and §15.

[Round note](agent-review-notes.md#05-r002--correction-recheck-of-the-phase-05-gate-05-d2-over-d1--changes-required); [dispositions](agent-review-findings.md#05-r002-dispositions-for-05-f001-to-05-f007).
- Resolved: 05-F001, F002, F003, F004, F005 and F007.
- 05-F006 is addressed by the §16 addendum. §16.1, §16.3 and §16.4 are accepted.
- Widened and accepted: notices on the delivery engine in the 05A park transaction; the non-routed `ReviewerUnavailable` settle variant; the producer-only advisory event bound.
- Open:
  - [05-F008](agent-review-findings.md#05-f008--the-approval-reopen-reuses-the-parked-attempt-number-and-is-unguarded) **medium**: §16.2 keeps `retry_count` unchanged across the approval park and reopen. The next claim therefore reuses the parked attempt's number, which `UNIQUE (task_id, attempt_number)` refuses. The park precondition is vacuous, and the new completed→pending transition has no database guard.
  - [05-F009](agent-review-findings.md#05-f009--notice_delivery_id-needs-uniqueness-insert-only-writing-and-a-provenance-binding) **low**: the id-only FK is acceptable, since standalone deliveries are tenant-less. It still needs `UNIQUE`, insert-only writing (`NOT NULL`, or SET NULL only) and an idempotency-key provenance check, with a DB test.
- Next: A fixes 05-F008 and 05-F009 in a narrow delta; B rechecks by ID.

### 05-R003 — 2026-10-18 correction recheck of 05-D3 (over D1+D2) — CHANGES REQUIRED
**No PASS** for `logs/05/decision-D3.md` SHA-256 `bd937b8630e48e6c7e2bf0bfeb223cd879004e4c3cfb898bb302b0d432af0b69` over D2 `831890270a87273b2cbbccf580a942c4582841cc2c4430b15d542d95754815b3` and D1 `4e3a46ebb03137ba5c86e1b3d64e1d3b0abf756c54eaa59efc8cee8301707931` (all three confirmed). The user's decision of 2026-10-18 to allow self-approval is recorded in D3 §1, with no rule change and an added test.

[Round note](agent-review-notes.md#05-r003--correction-recheck-of-the-phase-05-gate-05-d3-over-d1d2--changes-required); [dispositions](agent-review-findings.md#05-r003-dispositions-for-05-f008-and-05-f009).
- 05-F008 resolved. The parked attempt is consumed at reopen, and the park requires `k+1 < max_retries`. A deferred completed→pending guard keyed on `submitted_xact = pg_current_xact_id()` allows at most one reopen per approval.
- 05-F009 resolved for integrity: UNIQUE, NOT NULL/NO ACTION, insert-only, a provenance trigger, and a rejection test.
- Widened and accepted: `submitted_xact` with the reopen branch; a composer `BadRequest` settling unrouted as `decision.reviewer_unavailable`; the 16-delivery lease-budget coverage.
- Open: [05-F010](agent-review-findings.md#05-f010--the-notice-key-is-not-proven-unhashed-so-a-long-member-address-loops-the-park) **low**. No member-address bound exists, so a key can be hashed once the address exceeds 391 bytes. The provenance trigger's 23514 then makes the park retry until the run deadline. Fix: a pre-write key check that settles as `reviewer_unavailable`, or bound `users.email`.
- Next: A fixes 05-F010 (small delta); B rechecks by ID.

### 05-R004 — 2026-10-18 recheck of 05-D4 (over D1+D2+D3) — PASS
**PASS** for the effective phase 05 decision: D1 `4e3a46ebb03137ba5c86e1b3d64e1d3b0abf756c54eaa59efc8cee8301707931` + D2 `831890270a87273b2cbbccf580a942c4582841cc2c4430b15d542d95754815b3` + D3 `bd937b8630e48e6c7e2bf0bfeb223cd879004e4c3cfb898bb302b0d432af0b69` + D4 `e8ed88c46e1420e3ededa3da33b282a8e6c9cefc9495e3146a420ed77fe1fc86` (all four confirmed).

[Round note](agent-review-notes.md#05-r004--recheck-of-the-phase-05-gate-05-d4-over-d1d2d3--pass); [disposition](agent-review-findings.md#05-r004-disposition-for-05-f010).
- 05-F010 resolved by option (a): a pre-write unhashed-prefix check settles `decision.reviewer_unavailable` unrouted with no retry. The 05B twin fails Terminal `approval_required` with no park.
- No phase 05 gate findings remain open (05-F001 to F010 resolved).
- 05B code proceeds under the accepted §16 addendum; implementation deviations return to B first.
- Deferred, outside phase 05: the phase 07 timeout-code mapping, the phase 08 routes and wiring, and member-address bounding.


### 05-R005 — 2026-10-18 whole 05A batch — CHANGES REQUIRED
Artifact `e2a59ac010a2b8c819f88a490729a21d97e9dde868ec4a8681ae54277a3e053a`, base `feefe3e060a851fd8860e4bacb90c11c7b284888`, sole exclusion `plan/workflow-foundation/docs`; pre/post snapshot matches. Workflow `implement-astra-reviewed-2.1`, independent Reviewer B.

[Round note](agent-review-notes.md#05-r005--whole-05a-batch--changes-required). Open findings: 05-F011 (medium: actual membership/visibility revocation and authorization acceptance tests), 05-F012 (low: cross-run concurrent event-id reuse yields Database instead of Conflict), 05-F013 (low: outstanding explicit provenance/composite-FK and boundary cases). See [findings](agent-review-findings.md).

The `/params` fixture adjustment conforms to static reviewer assignment; phase07 child parameter propagation and timeout-code obligations remain. Broader gates were incomplete with failures reported by root; no batch acceptance. Author fixes all findings and gate failures before one correction recheck.


### 05-R006 — D5 correction architecture gate — CHANGES REQUIRED
Decision D5 `ec061ebe8a01fedf18b18258337c09dcbd1567394d59d13226374eef8c9a1cdb` (2026-10-18). [Note](agent-review-notes.md#05-r006--d5-correction-architecture-gate--changes-required); open 05-F014 low: account for actual remove_member conversion to external/NULL user before membership deletion and exercise that port in lifecycle/race tests. Local snapshots and runtime disclosure rejection are otherwise accepted direction. No dependent schema approval until the narrow decision correction passes; 05-F011–013 remain open for implementation recheck.


### 05-R007 — D5+D6 architecture correction recheck — PASS
**PASS** for D5 `ec061ebe8a01fedf18b18258337c09dcbd1567394d59d13226374eef8c9a1cdb` + D6 `93aabee933500c4d2f9a02535901be92768f31ff47a2b19493525652d1f54c80` over D1–D4. [Note](agent-review-notes.md#05-r007--d5d6-architecture-correction-recheck--pass); [F014 disposition](agent-review-findings.md#05-r007-disposition-for-05-f014). Actual production conversion, raw deletion, snapshot history and bounded contention are now specified distinctly. Dependent implementation may proceed. 05-F011–013 and corrected 05A artifact/gates still require recheck; this is not batch acceptance.


### 05-R008 — Corrected 05A consolidated recheck — CODE PASS; broader gates pending
**CODE PASS** for exact artifact `2a428cd707ec4427ddb31ca9ea60d875ba829c1bb0c4c743ab7f38929d99bef3`, base `feefe3e060a851fd8860e4bacb90c11c7b284888`, sole exclusion `plan/workflow-foundation/docs`; pre/post snapshots match. [Note](agent-review-notes.md#05-r008--corrected-05a-consolidated-recheck--code-pass); [F011–F013 resolutions](agent-review-findings.md#05-r008-dispositions--05-f011-05-f012-and-05-f013-resolved).

D5+D6 snapshot/history and runtime disclosure amendments implemented and reviewed. Focused correction evidence: 44/0, strict Clippy passes. No open review findings. Root must still complete correction static/schema/SQLx/workflow/delivery/graph gates and reconfirm the artifact before accepting 05A. Earlier broad failures are not waived; final phase05 full/stack/lease gates remain after 05B.


### 05-R009 — D7 one-entry approval architecture gate — PASS
**PASS** for immutable decision D7 SHA-256 `d980ffa58aea5eb8f58b7627e2ba4b68506a8912b47f6dc2db72ec30a13ebcf9` over D1–D6. [Note](agent-review-notes.md#05-r009--d7-one-entry-approval-architecture-gate--pass). Actual marker reuse and deferred entry callers confirm the not-applied-proof alternative bypasses the old replay predicate; D7 closes it while preserving current authorization, receipt replay and truthful reconciliation. Dependent correction may proceed. No new finding; source WIP, 05B batch acceptance and final verification remain outstanding.


### 05-R010 — Whole 05B batch — CHANGES REQUIRED
Artifact `b42023ba2177a20937e6aa0c14b4abf6dc6e41fcbcf2a89a6fa711370f6fd630`, base `feefe3e060a851fd8860e4bacb90c11c7b284888`, sole exclusion `plan/workflow-foundation/docs`; pre/post snapshot matches. [Note](agent-review-notes.md#05-r010--whole-05b-batch--changes-required). One open finding: [05-F015](agent-review-findings.md#05-f015--05b-omits-explicit-accepted-reopen-guard-and-notice-failure-cases), low, missing explicit D3 stale/reject/accounting reopen tests and D4 unprovable-notice approval twin. No implementation bypass identified. Batch gate evidence passes; final phase05 full/stock gates remain pending. Complete focused corrections before one recheck; 05B is not yet accepted.


### 05-R011 — 05B F015 correction recheck — PASS
**PASS** for `ac6e12f99bf53762a9056a6fcea62ac4a90513d4315dc47e5e3b087d21d4a84a`, base `feefe3e060a851fd8860e4bacb90c11c7b284888`, sole exclusion `plan/workflow-foundation/docs`. [Note](agent-review-notes.md#05-r011--05b-f015-consolidated-correction-recheck--pass); [F015 resolution](agent-review-findings.md#05-r011-disposition--05-f015-resolved). Test-only correction supplies all requested reopen variants with positive control and exact guard evidence, plus unprovable-notice approval refusal. Focused19/0 and static/SQLx gates pass; prior broad production evidence reused. No open review findings. Root snapshot confirmation precedes batch acceptance; final phase05 full/stock checks remain pending.


### 05-R012 — Final phase05 delta and combined integration evidence — PASS
**PASS** for `59fde0cfbc2a70abdd01614b15dedc6d5929c4e40aeec74d66fb41120983edba`, base `feefe3e060a851fd8860e4bacb90c11c7b284888`, sole exclusion `plan/workflow-foundation/docs`; pre/post snapshots match. [Round note](agent-review-notes.md#05-r012--final-phase05-delta-and-combined-integration-evidence--pass).

The final test fixture preserves every timing/resource bound and strengthens the genuine lease-first negative control to require its specific causal rejection. Prior accepted production/integration review remains applicable. Corrected full all-target suite at 2 MiB: 2727 library + 5 binary passed, zero failed, 22 existing ignored; focused deadline2/0; lease118/0, maximum measured code-work35.7% against50%. Static/offline/Clippy/SQLx/graph gates pass; unchanged-schema migration/equality and transport/credential/deploy evidence reused. Earlier failed evidence remains recorded. No open findings. Root may accept phase05 after snapshot confirmation; deferred phase07/08 work remains outside acceptance, with no commits, deployment or phase06+ authorized.


### 06-R001 — 06A capability contract architecture gate — PASS
**PASS** for immutable [06-D1](logs/06/decision-D1.md), SHA-256 `c89927735e1f2d1c0fe23bfb898070f88bbd285cc5703defcb482be31aefba36`, base `5b4989ae0038a82f431db60a338b7f59d9bcf25c`. [Round note](agent-review-notes.md#06-r001--06a-capability-contract-architecture-gate--pass). No findings. Ordered frozen defaults/profile replacement, explicit ceiling, requirements and pure revocation validation fit the existing publication contracts. Dependent 06A implementation may proceed; runtime policy, durable execution and legacy removal require later gates/reviews. No implementation or database verification claimed.


### 06-R002 — Whole 06A capability contract batch — PASS
**PASS** for `bf8b89c3a741c530a557ee3c7fd92ce4bb2c0628ce055ad0ba2e6e2d079faa6e`, base `5b4989ae0038a82f431db60a338b7f59d9bcf25c`, sole exclusion `plan/workflow-foundation/docs`. [Round note](agent-review-notes.md#06-r002--whole-06a-capability-contract-batch--pass). Entire source/deliverable diff and new files inspected; no findings. Focused publication30/0, compiler32/0, registry34/0 and static gates inspected. No database verification claimed. Pure ordered frozen capability contract accepted by review; root snapshot confirmation precedes batch acceptance. 06B/06C and full phase06 verification remain outstanding.


### 06-R003 — 06A resume checkpoint delta — PASS
**PASS** for `0e62290c896c341c9e6700bcef8cf3293fbd47e5cf3cc76fed020326c5b26f54`, same base, sole exclusion `plan/workflow-foundation/docs`. [Round note](agent-review-notes.md#06-r003--06a-resume-checkpoint-delta--pass). Only RESUME status changed; diagnostic snapshot reproduces06-R002 for all other artifacts. Earlier technical review/evidence retained unchanged. No findings or new tests;06B/06C and full phase06 gate remain unfinished pending the monetary-budget choice.


### 06-R004 — 06B1 usage/pricing architecture gate — PASS
**PASS** for [06-D2](logs/06/decision-D2.md), SHA-256 `97861d07e9f19f275193575a7fb4981317c4ed593ca776e1d60e3687c42734c7`, base `5b4989ae0038a82f431db60a338b7f59d9bcf25c`. [Round note](agent-review-notes.md#06-r004--06b1-usagepricing-architecture-gate--pass). No findings. Durable checkpoint projection, immutable settlement, exact provenance, authorized bounded analytics and optional current-rate estimates fit actual contracts and revised user requirements. Dependent06B1 implementation may proceed. Workflow token approval/checkpoint runtime requires separate06B2 gate; no source/test/DB verification claimed here.


### 06-R005 — Whole 06B1 usage/pricing batch — CHANGES REQUIRED
Artifact `7c438b4fe32fbff52d6d97428577af7551e0c33f74b600452ffae78e4c574a2f`, base `5b4989ae0038a82f431db60a338b7f59d9bcf25c`, sole exclusion `plan/workflow-foundation/docs`; pre/post snapshot matches. [Round note](agent-review-notes.md#06-r005--whole-06b1-usagepricing-batch--changes-required). One open low finding [06-F001](agent-review-findings.md#06-f001--06b1-lacks-promised-production-usage-and-revocation-race-proof): missing promised production Rig→usage/repair/partial-response composition and usage-operation membership race proof. No implementation bypass identified; source/schema/UI and reported focused/static/browser gates otherwise reviewed.06B1 not yet accepted.


### 06-R006 — 06B1 F001 correction recheck — PASS
**PASS** for `4eabf1ca4939d6446eeb92baca22a82696496ecb0cca2beac1ddf3fb2eb44222`, base `5b4989ae0038a82f431db60a338b7f59d9bcf25c`, sole exclusion `plan/workflow-foundation/docs`. [Round note](agent-review-notes.md#06-r006--06b1-f001-correction-recheck--pass); [F001 resolution](agent-review-findings.md#06-r006-disposition--06-f001-resolved). Production repair/partial-response/approval/deadline usage assertions and controlled report/price membership races close the missing verification. Usage13/0, final production9/0 and affected static/SQLx gates pass. Unchanged06-R005 technical/browser/schema evidence reused. No open findings; root snapshot confirmation precedes06B1 acceptance. Later06B2/B3/C and full phase06 verification remain pending.


### 06-R007 — 06B2a successful approval continuation architecture gate — PASS
**PASS** for [06-D3 revision1](logs/06/decision-D3.md), SHA-256 `c2ba112b23bf2839d34fab0b68d36101c3c2b87ac2c36351fc940d2a3f5debcc`. [Round note](agent-review-notes.md#06-r007--06b2a-successful-approval-continuation-architecture-gate--pass). No findings. Generated attempt identity with separate failure/continuation counters and exact decision/wait/parked-attempt/audit associations address actual ownership/history contracts. Existing implementation remains accepted06B1 digest `4eabf1ca4939d6446eeb92baca22a82696496ecb0cca2beac1ddf3fb2eb44222`; no dependent edits yet.06B2a may proceed; model runtime/token allowance and06B2b remain outside this gate.


### 06-R008 — Whole06B2a successful continuation protocol — CHANGES REQUIRED
Artifact `1d7e5545bb22655b22c40e14aac9f49e67f41af5683699a983b7e23deb4ce4d5`, same base/sole exclusion `plan/workflow-foundation/docs`; pre/post snapshot matches. [Round note](agent-review-notes.md#06-r008--whole06b2a-successful-continuation-protocol--changes-required). Open:06-F003 medium audit discriminator/uniqueness,06-F004 low approval replay result,06-F005 low promised operation identity and post-continuation reconciliation proofs. [Findings](agent-review-findings.md#06-f003--approval-audit-exemption-weakens-unrelated-event-kind-constraints). Earlier compiler/SQL admission defect06-F002 independently resolved with current-revision activation regression. Workflow765/0 and static/schema gates inspected;06B2a not accepted until corrections/recheck.


### 06-R009 — 06B2a consolidated correction recheck — CODE PASS; broader gate pending
**CODE PASS** for `92cde7ae8804f314f0eb6308a845de4c2d8efdf02be70432949a9ee6e051df46`, same base and sole exclusion `plan/workflow-foundation/docs`. [Round note](agent-review-notes.md#06-r009--06b2a-consolidated-correction-recheck--code-pass-broader-gate-pending). F003–F005 resolved; author-discovered shared capability duplication recorded resolved asF006. Focused approval32/0 and final static/schema/SQLx evidence pass. Both broad correction sweeps remain769/1 despite passing unchanged focused reruns, so required broad verification blocks batch acceptance. Root is running a frozen-source four-thread sweep preserving internal contention and all bounds; evidence-only recheck may close this gate if clean. No D4 review or phase06 completion claim.


### 06-R010 — 06B2a broad-gate evidence-only recheck — PASS
**PASS for the entire06B2a batch** at `92cde7ae8804f314f0eb6308a845de4c2d8efdf02be70432949a9ee6e051df46`, base `5b4989ae0038a82f431db60a338b7f59d9bcf25c`, sole exclusion `plan/workflow-foundation/docs`. [Round note](agent-review-notes.md#06-r010--06b2a-broad-gate-evidence-only-recheck--pass). Source unchanged from06-R009; clean complete workflow770/0 in392.30s at stock2MiB/four parallel libtest threads closes its broad gate. Actual contention, assertions and bounds unchanged; earlier failures preserved. No open findings. Root may accept after digest confirmation; D4 and final phase06 completion remain outside this pass.


### 06-R011 — 06B2b D4 architecture gate — CHANGES REQUIRED
**CHANGES REQUIRED** for06-D4 revision1, SHA-256 `ed282ae6c42f539546d1bd1fca0ce1557852a29ae41629aa70cd6933a2ae360b`. [Round note](agent-review-notes.md#06-r011--06b2b-d4-architecture-gate--changes-required). One medium open finding [06-F007](agent-review-findings.md#06-f007--d4-approval-time-reservation-conflicts-with-native-fence-and-recovery-ownership): define approval versus fresh worker reservation boundaries and distinguish new admission from replay/unresolved requests before provider entry. No dependent06B2b implementation approval. Accepted06B2a artifact remains unchanged; corrected decision requires recheck.


### 06-R012 — D4 F007 architecture correction recheck — PASS
**PASS** for06-D4 revision2, SHA-256 `ffb176f2446a21cefd438c289aa357f6144f3f3921747df2a3b44248afc848b2`. [Round note](agent-review-notes.md#06-r012--d4-f007-architecture-correction-recheck--pass); [F007 resolution](agent-review-findings.md#06-r012-disposition--06-f007-resolved). Approval only authorizes/reopens; fresh fenced admission owns charges and the sole provider-entry permit. Replay/uncertainty and retired-generation recovery preserve charges without re-entering old IDs. No remaining architecture findings. Dependent06B2b may proceed after root acceptance; required implementation/crash/race proofs remain outstanding. Accepted source digest unchanged.


### 06-R013 — D4 revision3 root allowance scheduling architecture gate — PASS
**PASS** for [06-D4 revision3 addendum](logs/06/decision-D4-root-pause-r3.md), SHA-256 `ab2be4fe23c66e31d53296a5d40629cb31a45b5516b3b80e208ac71f774fd3c7`. [Round note](agent-review-notes.md#06-r013--d4-revision3-root-allowance-scheduling-architecture-gate--pass); [finding disposition](agent-review-findings.md#06-r013-architecture-disposition--no-new-findings). B inspected actual pending/pure/I/O scheduling, historical effect safety, retirement debit/hard bounds and SQL progression/reopen seams. No findings. Five-second pending deferral plus bounded genuine processing retirement fits the existing owners while preserving exact native approval/reproposal behavior. Digest matches before/after. This accepts only the architecture extension; unfinished06B2b source, its concurrency/recovery proofs and final gates remain unaccepted.
