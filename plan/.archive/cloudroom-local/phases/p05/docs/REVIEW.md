# p05 independent review

## CL-P05-D01-R1 — combined architecture gate

**PASS for CL-P05-D01/r1 only**, SHA-256 **`4a5173ccf88cc6725553c98e895a14097559c5348d71424cad4812b3602abfe4`**. **No actionable findings.** [Decision](logs/CL-P05-D01-r1.md), [review note](agent-review-notes.md#cl-p05-d01-r1--combined-management-and-durable-workflow-architecture), [findings disposition](agent-review-findings.md#cl-p05-d01-r1--disposition).

Reviewed original05/README and actual shared action/approval, immutable intent, publication/tool/sample, lock/fence, message-specific park/wake/recovery and local machine/session consumers. The proposed separate deferred marker, execution-scoped preparation and atomic park/ready/fresh-claim ownership address their existing constraints. All planned B1 behavioral/concurrency/browser/SQL proof remains required; this is inspection-only design acceptance.

- Base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; source unchanged at accepted p04 **`512b6e90ecf87aad7f0dc140e0afeb45532d67e2b4fedf5c15952685342cabda`**.
- Authority r18 pin, decision hash and exact8-exclusion entry/final source identities match. [Input checks](logs/reviewer-d01-r1-input-checks.json), [final snapshot](logs/reviewer-d01-r1-final-snapshot.txt).
- Root may activate proposed C006/r1 and implement B1. No p05 code/runtime/platform/final acceptance claimed. Native Linux waiver remains; p07 macOS/full-repository gate remains outstanding.

## CL-P05-D01-R2 — explicit operation origin architecture gate

**PASS for CL-P05-D01/r2 only**, SHA-256 **`6c3c67bf78cb1cdf288f2ab271d24691d29fece1ceb2d71e923064ef2c40618d`**. **No actionable findings.** [Decision](logs/CL-P05-D01-r2.md), [review note](agent-review-notes.md#cl-p05-d01-r2--direct-follow-up-and-immutable-operation-origin), [findings disposition](agent-review-findings.md#cl-p05-d01-r2--disposition).

The immutable server-owned Direct/Workflow discriminator, origin-scoped call FK and reverse deferred admission proof separate authorized manual follow-up from parked workflow delivery without treating an absent call as direct authority. Current original-association visibility, immutable selection/request replay, first-delivery revocation and attempted-operation reconciliation remain required.

- Base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; unchanged **UNACCEPTED p05 progress** digest **`e967b0adb919a51c689b99350b2cac39544e88b470952a22ac1365767eb558a1`**. This is not the accepted p04 artifact or a p05 batch acceptance.
- Authority r21 pin and decision hash match [input checks](logs/reviewer-d01-r2-input-checks.json); exact eight-exclusion [entry](logs/reviewer-d01-r2-entry-snapshot.txt) and [final](logs/reviewer-d01-r2-final-snapshot.txt) snapshots are byte-equal.
- Root may activate C006/r2 and implement the dependent changes. No code/runtime/platform acceptance; B1 behavioral and concurrency proof and p07 final gate remain required. Native Linux waiver unchanged.

## CL-P05-B1-R1 — whole implementation review

**CHANGES REQUIRED** for artifact **`5e3cb39493d6068a837125789a1cb5f6165c45b77af547ecf90ae3e0ed91c505`**. Base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; authority r23, C006/r2. No p05 acceptance.

- **P1 F1:** actual fair-poll worker discovery omits ready local-agent results; tests call a standalone sweeper instead.
- **P1 F2:** late positive native evidence can replace an already-ready uncertain workflow disposition before receipt consumption.
- **P1 F3:** workflow expiry marks attempted running work Expired and prematurely frees physical capacity.
- **P2 F4:** profile paging drops the conversation needed to start with a later-page profile.
- **P2 F5:** required Workflow continuation and first-delivery authority/negative-link proofs remain unexercised.

[All findings and required corrections](agent-review-findings.md#cl-p05-b1-r1--whole-batch-findings), [review note](agent-review-notes.md#cl-p05-b1-r1--complete-management-and-workflow-implementation), [coverage](logs/reviewer-b1-r1-coverage.md). Independent focused run:103 PASS/0 FAIL/1 ignored. Exact production-query reproduction confirms F1/F3; passing tests and retained manifest do not establish the missing behavior. Full repository/platform gate remains p07.

[Entry snapshot](logs/reviewer-b1-r1-entry-snapshot.txt), [final snapshot](logs/reviewer-b1-r1-final-snapshot.txt), [input checks](logs/reviewer-b1-r1-input-checks.json). Correct all findings together, run affected checks and return one frozen correction artifact for recheck.

## CL-P05-B1-R2 — combined correction recheck

**PASS** for exact artifact **`c7dfb6034c5f3f5001e107ac94a008da7fc55b88054af8208cfa1b79a6685664`**. Base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; authoritative coordination r25/C006-r2. **F1–F5 resolved; no remaining/new actionable findings.** Prior R1 rejection remains historical and is superseded for this corrected artifact only.

Rechecked production fair-poll discovery, immutable Workflow disposition versus late native success, shared physical-uncertainty expiry, later-page authorized profile navigation, and explicit Workflow continuation/approval/current first-delivery authority. Reused unchanged whole-batch R1 coverage and inspected all14 correction files and affected consumers.

Independent verification: **112 tests passed,0 failed,1 ignored fixture generator; browser1 passed**. Author evidence separately exercises the generator and records shared polling/wait/message, migration, SQLx/offline all-target Clippy, formatting and CSS checks. Historical test/Clippy failures remain retained with their corrections. B used the parent-built test binary and retained browser fixtures; no independent rebuild or fixture regeneration claimed.

[Review note](agent-review-notes.md#cl-p05-b1-r2--combined-correction-recheck), [all finding resolutions](agent-review-findings.md#cl-p05-b1-r2--combined-resolution-history), [independent PG run](logs/reviewer-b1-r2-cloudroom-tests.txt), [browser run](logs/reviewer-b1-r2-browser-tests.txt). Exact-eight-exclusion entry/final snapshots and authority pin match; [final checks](logs/reviewer-b1-r2-final-checks.json).

Root may record this p05 batch acceptance. Full combined repository/macOS/native acceptance remains p07; Linux native remains explicitly NOT EXERCISED. No source edits, provider activity or final full-suite claim by B.
