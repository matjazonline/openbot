

## CL-P04-D01-R1 — combined architecture gate

**CHANGES REQUIRED for CL-P04-D01/r1.** [Decision](logs/CL-P04-D01-r1.md), [review note](agent-review-notes.md#cl-p04-d01-r1--combined-architecture-gate), [CL-P04-D01-F1/medium](agent-review-findings.md#cl-p04-d01-f1--persist-the-pending-enrollment-identity-before-its-first-request): the stated post-receipt installation-metadata write cannot recover a process crash after enrollment commits, because the existing status API requires the original installation ID as well as the saved claim secret.

- Decision SHA-256 `cf4f35a691e11bdd2f1c84f4aad69cce97c594bcb7599639840ae34381edac8b`; source remains accepted p03 `1e4f5b2148970677a7f1c388fa5a38cc52b1e7685061a4fac15857ea6b1325d9`.
- Base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`, main. Coordination r14 pin and source entry/final identities match. [Input checks](logs/reviewer-d01-r1-input-checks.json), [final snapshot](logs/reviewer-d01-r1-final-snapshot.txt).
- Whole combined design reviewed against actual public SDK/server consumers. Proposed SDK patch apply-check PASS, source unapplied. No runtime/DB/native execution by B. C001/r2 and C005/r1 remain proposed; dependent implementation waits revised gate PASS.


## CL-P04-D01-R2 — enrollment and queue-mode recheck

**PASS for CL-P04-D01/r2 only.** [Decision](logs/CL-P04-D01-r2.md), [review note](agent-review-notes.md#cl-p04-d01-r2--enrollment-and-queue-mode-recheck). **CL-P04-D01-F1 resolved at architecture gate; no remaining findings.** Pre-request Pending secret state preserves the full enrollment identity across process crashes; Paired+journal loss stays fail-closed. The explicit queue-if-active send mode matches the actual native dispatch/queue contract.

- Decision SHA-256 `ac2213e981379e881a11df17c41e9a090564dc66f52da48dce3f7e6769c3baa0`; source remains `1e4f5b2148970677a7f1c388fa5a38cc52b1e7685061a4fac15857ea6b1325d9`.
- Base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; coordination r14 pin and entry/final source identities match. [Input checks](logs/reviewer-d01-r2-input-checks.json), [final snapshot](logs/reviewer-d01-r2-final-snapshot.txt).
- Inspection-only architecture pass, reusing unaffected R1 coverage. Actual crash/reopen, SDK cancellation, queue races and other B1 proof remain required. Root may activate proposed notices and implement; no p04 batch/native/final acceptance claimed.


## CL-P04-B1-R1 — whole plugin and Submission batch

**CHANGES REQUIRED** for artifact **`0ee58168040121f43177343a8c3efe358fd0f02c0ea2051056ca1f99df1a9661`**. [Review note](agent-review-notes.md#cl-p04-b1-r1--whole-plugin-and-submission-batch), [all findings](agent-review-findings.md#cl-p04-b1-f1--settings-withdrawal-during-startup-leaves-stale-project-authority).

- **F1/medium:** project withdrawal during startup is missed; stale selection still permits a mutation.
- **F2/low:** resolved matching permission prompts remain waiting_for_input until a later item event.
- **F3/medium:** definitive enrollment rejection retains transient input/frozen rejected request and ignores replacement input on retry.
- **F4/medium:** required idle/active-human/active-Openbot/status-race queue proof is absent from B1's fixtures.

Whole source/test/contract batch reviewed against original04 and approved D01/r2; no additional actionable Rust Submission issue identified. B independently reran49 plugin tests (all PASS), validated evidence/hashes, and reproduced F1–F3 in review-only production-boundary fixtures. Author's82 Rust/5 focused Submission, SDK HTTP cancellation and build/lint evidence inspected. [Reproductions](logs/reviewer-b1-r1-boundaries.txt), [test log](logs/reviewer-b1-r1-plugin-tests.txt), [input checks](logs/reviewer-b1-r1-input-checks.json).

Base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; r15 pin and exact8 exclusions unchanged; entry/final source identities match. Correct all findings before one recheck. No p04/native/final acceptance; macOS actual-plugin and repository full suite remain p07, native Linux user-waived only, p05 producer still disabled.


## CL-P04-B1-R2 — combined F1–F4 correction recheck

**PASS** for artifact **`512b6e90ecf87aad7f0dc140e0afeb45532d67e2b4fedf5c15952685342cabda`**. **CL-P04-B1-F1–F4 all RESOLVED; no remaining actionable findings.** [Recheck note](agent-review-notes.md#cl-p04-b1-r2--combined-f1f4-correction-recheck), [finding dispositions](agent-review-findings.md#cl-p04-b1-f1-resolution--2026-11-08-cl-p04-b1-r2).

Reviewed all9 changed paths and affected startup, permission-state, definitive-enrollment-cleanup and native-queue fixture contracts; reused unaffected R1 whole-batch coverage. B independently ran **72 plugin tests, all PASS**, verified all49 prior cases retained+23 added, validated retained evidence, and checked current compiled inputs/source exports. F3 implements approved D01/r2 without a new gate decision. [Tests](logs/reviewer-b1-r2-plugin-tests.txt), [delta](logs/reviewer-b1-r2-correction-delta.patch), [input checks](logs/reviewer-b1-r2-input-checks.json).

Base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; unchanged r15 authority and exact8 exclusions; entry/final digest matches. Root may accept p04 after its own boundary checks. Actual macOS plugin/native proof and final repository suite remain p07; native Linux remains user-waived, and p05 producer remains disabled.
