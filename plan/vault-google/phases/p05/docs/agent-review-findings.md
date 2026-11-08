# Phase05 independent findings

## VG-P05-AG1-R1 — no actionable architecture findings

2026-11-07. Whole05A–05D architecture review of D1/r1 SHA256 `a0df7f11553ef95ea538942ee44a3277eb7e97a8934c9cd44b1fd09ae2dc1259`. Baseline `b1dde903b71fda9e8f03eb2338114d72c03418f39b8bceaa54ea308e0d484960`, HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`.

Architecture PASS; proposed VG-C004/r1 reviewed and approved for activation. No defect or optional suggestion warrants a finding ID. Implementation remains unreviewed. Role, original criteria, evidence and limitations: [review notes](agent-review-notes.md#vg-p05-ag1-r1). Preserve this entry when appending later findings/corrections.


## VG-P05-B1-R1 — no actionable code findings

2026-11-07. Whole05A–05D code review against original05 and relevant01–04 contracts, accepted D1/r1. Exact digest `2b114131e1b9a47fa1db8557baf6b913c392f88163500245b719801b1c60d982`, detached HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`, coordination revision9.

**PASS.** All52 changed paths reviewed against accepted04;45 new renewal tests independently listed/executed successfully. No actionable defect or optional suggestion warrants a finding ID. [Detailed scope, role, evidence, limitations and instruction feedback](agent-review-notes.md#vg-p05-b1-r1); [identity receipt](logs/reviewer-B1-identity.json). Full task verification remains pending and this finding-free phase review does not imply live-provider or06/07 verification. Preserve architecture history above.


## VG-P05-VF1 — hostile test fixture misclassified by source guard

- **Context:** 2026-11-07, implement-astra-reviewed-2-2; discovered by root's final full gate after VG-P05-B1-R1, independently rechecked in VG-P05-B1-R2. Original affected artifact `2b114131e1b9a47fa1db8557baf6b913c392f88163500245b719801b1c60d982`, HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; worktree1133; independent B requested Astra/high (runtime settings not independently exposed). [Round notes](agent-review-notes.md#vg-p05-b1-r2), [TASK](TASK.md).
- **Classification:** actionable verification/integration defect, medium. This is a test-file placement issue, not a defect in D1 or emitted production markup.
- **Evidence:** existing source guard `src/adapters/http/pages/tests.rs:4613–4669` walks HTTP Rust files and deliberately exempts conventional `tests.rs`. Deliberate hostile script markup in the accepted earlier vault management fixture at old `routes/vault_management_tests.rs:157,162` was therefore flagged. Both final full library runs failed that guard; [exact diagnostic](logs/full-style-diagnostic-01.log). R1's focused renewal/management tests passed but did not execute this source guard. The R1 phase-code PASS explicitly left full verification pending; this later result exposes that focused coverage gap.
- **Resolution:** root moved the byte-identical fixture to `routes/ui_vault/tests.rs` and removed only its old `#[path]` attribute, preserving cfg(test), namespace and all escaping assertions. Scanner/runtime implementation unchanged. Reviewer independently compared both manifests, archived R1 source and current bytes: exactly3changed paths, identical fixture SHA `2c1e74b32da10c85b5d86c184c0077e8c1c57d25742a53ba6aa1cbd8b089792e`. At corrected digest `9d561a9cd7b6e4662d859bca78e70b0739b985b1dad6d8e75e5f30b68311088b`, fresh root scanner1/UI3 tests pass; reviewer direct current-binary exact scanner test also passes. **FIXED / independently verified VG-P05-B1-R2.** [Relocation proof](logs/fixture-relocation-R2.json), [reviewer test](logs/reviewer-R2-style.json), [current checks](logs/checks-R2.json).
- **Instruction lesson:** existing source-scanning convention was missed by earlier fixture placement. Candidate scoped guidance: keep deliberate hostile HTTP markup fixtures in conventional tests.rs modules and include the existing source guard when adding such fixtures. No instruction/source edits were made by B; a new broad testing requirement is not justified.

## VG-P05-VF2 — inherited overflow fixture setup fails before tested operations

- **Context:** same workflow/role/worktree/HEAD and R1/R2 identities as VF1, final integration review VG-P05-B1-R2. [Coverage](agent-review-notes.md#vg-p05-b1-r2).
- **Classification:** confirmed inherited verification limitation, not an introduced phase05 regression or an actionable correction required for this bounded artifact review. Remains unresolved outside this correction batch.
- **Evidence:** `workflow/control_revision_tests.rs:80–107` issues DISABLE TRIGGER, UPDATE and ENABLE TRIGGER in one raw SQL setup; PostgreSQL returns SQLSTATE55006 (pending trigger events) at line85 before either cancel/claim operation under test. Same failure occurs in both full library runs and root's clean exact baseline HEAD97e5d60d reproduction on a fresh exclusively owned database. Reviewer inspected reproduction/ownership/cleanup receipts and independently verified current test bytes equal baseline HEAD and recorded SHA `d055ee09a0573a40f6898e97912c8a84ae4f48283cc4a397a17c530c903180d6`. [Baseline receipt](logs/baseline-overflow-01.json), [baseline log](logs/baseline-overflow-01.log), [resources](logs/baseline-resource-01.json), [full coverage](logs/full-coverage-R1.json).
- **Disposition:** **CONFIRMED BASELINE / UNRESOLVED**, no unrelated workflow/schema edits authorized by this recheck. The overflow behavior itself remains unverified by this fixture because setup aborts. Phase05 correction may pass with this explicit limitation; `scripts/check.sh` remains failed and no full-suite-green claim is permitted. Normal and stock2MiB library runs each recorded3337passed/2failed/9ignored; VF1 is now fixed through focused checks, VF2 remains.
- **Instruction lesson:** existing rule to establish a clean baseline before calling a failure pre-existing was followed. No new source guidance inferred from this review; a separately scoped fixture repair needs its own verification.
