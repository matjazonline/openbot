# Agent history p01 independent review

## AH-P01-AG1-R1 — CHANGES REQUIRED (architecture only)

2026-11-07. Reviewed exact [AH-P01-D1/r1](logs/AH-P01-D1-r1.md), SHA `5f45027f1c475f0cf43620915e89efd1ad3aa1c992ecd09cf51e63833b532732`, source `1110c5fc00a39784aab21d8f94b47d1ec5e7cdc458f72ecfabb371c1b0f9d268`, base/HEAD `4ab45555f8316ec7a8b1b64e8c05b77ff7d9a29d`.

- **AH-P01-AG1-F1 — medium, architecture/authorization:** the proposed execution-authority rule uses binding run-as when it differs from the starter. Actual manual admission uses the authenticated starter as persisted execution actor and ceiling source. Preserve each producer's actual actor; add a differing-starter/run-as regression before implementing history.

[Finding/evidence/correction requirement](agent-review-findings.md#ah-p01-ag1-f1--preserve-the-actual-manual-execution-principal), [whole-decision coverage and limitations](agent-review-notes.md#ah-p01-ag1-r1--architecture-gate-changes-required), [entry](logs/AH-P01-AG1-R1-reviewer-entry.json), [exit](logs/AH-P01-AG1-R1-reviewer-exit.json).

Source, decision, coordination1 authority/pin and accepted channel coordination28 match at exit. No implementation/runtime acceptance; dependent implementation awaits a revised decision and explicit gate PASS.


## AH-P01-AG1-R2 — PASS (architecture decision only)

2026-11-07. **PASS** for exact [AH-P01-D1/r2](logs/AH-P01-D1-r2.md), SHA `2a2001b0889b9d586a077ebd69ea5aa4728ccfdc62005ceef3c69add824fdc3a`, with unchanged source digest `1110c5fc00a39784aab21d8f94b47d1ec5e7cdc458f72ecfabb371c1b0f9d268`, base/HEAD `4ab45555f8316ec7a8b1b64e8c05b77ff7d9a29d`.

F1 is FIXED at architecture level: manual/automatic/child execution principals and existing ceiling sources remain producer-specific; session reuse grants no actor substitution. The differing manual-starter/binding-run-as regression is explicitly required. Unchanged R1 architecture coverage reused; no remaining actionable findings.

[Recheck coverage/limits](agent-review-notes.md#ah-p01-ag1-r2--architecture-correction-recheck-pass), [F1 disposition](agent-review-findings.md#ah-p01-ag1-r2-resolution--f1-fixed-at-architecture-level), [entry](logs/AH-P01-AG1-R2-reviewer-entry.json), [exit](logs/AH-P01-AG1-R2-reviewer-exit.json). Source/decision/coordination pins match. This is not implementation or runtime acceptance; whole-phase code review and targeted checks remain required.


## AH-P01-AG1-R3 — PASS (principal-lifecycle architecture only)

2026-11-07. **PASS** for exact [AH-P01-D1/r3](logs/AH-P01-D1-r3.md), SHA `ccdc36a566d74d5703c7c6e6d506b11167185a2c4a4882810ea018fb36a4a170`, reviewed against frozen source `fd81283df217c677856979928e35b5f2a31a7805b7547a24d95f75fa3efa006a`, base/HEAD `4ab45555f8316ec7a8b1b64e8c05b77ff7d9a29d`.

The historical creator snapshot plus nullable live principal FK safely preserves audit identity without pinning revocation, subject to the explicit native clearing guard and unchanged current authority checks. No additional defect in the proposed correction. [F2](agent-review-findings.md#ah-p01-ag1-f2--retained-session-fk-prevents-native-principal-revocation) records the confirmed current FK regression: corrected architecture accepted, **implementation still OPEN**. F1 remains fixed.

[Coverage/limitations](agent-review-notes.md#ah-p01-ag1-r3--principal-lifecycle-architecture-reopen-pass), [failure evidence](logs/AH-P01-AG1-R3-reviewer-failure-evidence.json), [entry](logs/AH-P01-AG1-R3-reviewer-entry.json), [exit](logs/AH-P01-AG1-R3-reviewer-exit.json). Source/decision/coordination2 remain exact. Current164-case evidence includes three failures; no code/runtime acceptance is issued. Implement the correction, rerun affected checks, then request whole-phase code review.


## AH-P01-B1-R1 — PASS (whole phase01 code)

2026-11-07. **PASS** for exact source `e62ebc6c7dac3ec2e34ca3ddce727df87b3d70f780154eb5b581b710a394aa91`, branch `codex/autonomous-agent-history-h_82_5pw`, start/review-base/HEAD `4ab45555f8316ec7a8b1b64e8c05b77ff7d9a29d`.

Reviewed the entire59-path owned phase01 delta and affected admission, actor/authority, thread/private/child provenance, native lifecycle, prepared/checkpoint and channel/sample interfaces against original01A/01B. F1 and F2 are **FIXED in implementation**; no remaining actionable finding. Exact source/owned archive/all owned and external hashes, D1/r2+r3 and coordination3 authority/local/pin match at entry and exit.

Independently audited author-run evidence:165 distinct targetedPASS, zero failed/ignored, all29 history cases and all three original revocation regressions; fresh migration pair, actual SQLx prepare/check, offline all-target compile, Clippy, fmt/diff/GraftPASS. B inspected source and receipts and did not rerun runtime checks. No full suite/live-provider or later history-phase behavior is claimed.

[Complete coverage and limitations](agent-review-notes.md#ah-p01-b1-r1--whole-phase01-code-review-pass), [F1/F2 disposition](agent-review-findings.md#ah-p01-b1-r1-implementation-dispositions--f1-and-f2-fixed), [per-path coverage](logs/AH-P01-B1-R1-reviewer-coverage.json), [evidence audit](logs/AH-P01-B1-R1-reviewer-evidence-audit.json), [entry](logs/AH-P01-B1-R1-reviewer-entry.json), [exit](logs/AH-P01-B1-R1-reviewer-exit.json).
