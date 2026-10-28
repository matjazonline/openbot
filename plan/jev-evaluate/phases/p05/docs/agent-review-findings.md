# p05 reviewer findings

## JEV-P05-B1-F1 — readiness text claims a check the list does not perform

2026-10-28; implement-astra-reviewed-2.2; phase p05, batch B1, whole-batch implementation review. Reviewer /root/phase_05/reviewer, requested Astra/medium, independent empty-context assignment (host does not expose additional model/compaction telemetry). Worktree /Users/mac02/dev/openbot, branch main. Start/review-base/HEAD 5a12ebec76c76c97e50b2a51ebacd66712db5820. Artifact `8d2e5cead1e9498700ca2c74b53e91573861b2dc0c28858cdfb86bcab8ab6939`. No new architecture gate. [TASK](TASK.md).

Round [JEV-P05-B1-R1](agent-review-notes.md). Classification: actionable defect; medium; documentation/UI correctness. Status: OPEN.

New `src/adapters/http/pages/company_typesafe.rs:26` says “Ready means enabled with a decryptable credential”; `docs/workflow-operations.md:113` likewise promises local decryptability. Actual `src/adapters/persistence/workflow/jev.rs:25-39` maps `ready` directly from `enabled`; list/read projections do not select the envelope or call the cipher. `src/adapters/persistence/workflow/jev_authorization.rs:135-143` decrypts only at current provider authorization. An enabled row with an unavailable encryption version or damaged envelope is displayed as ready despite the promised check and will fail at execution. This can mislead an operator diagnosing key rotation/configuration failures.

Correct the new UI and runbook to describe the existing metadata accurately (enabled; credential decryption and current authority are checked at use, no remote probe), or deliberately implement/test local decryptability if that is the intended management contract. Preserving p02 semantics by correcting text is sufficient. No new runtime architecture change is required.

Instruction lesson: root documentation rules and infra validation-claim rule already require claims to match actual checks. Candidate process guidance: when adding a readiness explanation, trace its projection producer and distinguish metadata from probes. One observed case; no additional repository instruction change warranted.

Resolution history: 2026-10-28 R1 confirmed by source inspection against the frozen digest; author alerted while source remained frozen. Await correction and affected recheck.

### F1 resolution — 2026-10-28, JEV-P05-B1-R2

FIXED on artifact `f5142763c1a9414a0f84635d56bb974b7c2719d1f124036c5bdfd182c0d4952c`. Independently inspected corrected `company_typesafe.rs:26` and `workflow-operations.md:113-115`: readiness now means enabled metadata, while credential decryption/current authorization happen at provider use; no remote probe is promised. Existing runtime/persistence contract retained. Author focused `workflow_authoring_jev` rerun PASS 2 under external-egress guard, [log](logs/f1-tests.log); formatting/diff checks PASS. R1 evidence remains applicable. No remaining actionable findings.

## JEV-P05-B1-F2 — final CSS reproducibility gate exposed a Rust scanner token

2026-10-28; implement-astra-reviewed-2.2; p05/B1, final-gate correction recheck JEV-P05-B1-R3; same independent reviewer /root/phase_05/reviewer (requested Astra/medium), worktree /Users/mac02/dev/openbot/main, review-base/HEAD 5a12ebec76c76c97e50b2a51ebacd66712db5820. Affected prior accepted artifact f5142763c1a9414a0f84635d56bb974b7c2719d1f124036c5bdfd182c0d4952c. [TASK](TASK.md), [REVIEW](REVIEW.md). Classification: actionable defect, medium, build reproducibility.

Coordinator final `npm run check:css` failed with stale generated CSS; independently inspected [failure](logs/final/css.log). C005's collapsed condition put bare `!status` in binding/mod.rs, and assets/app.css.in scans ../src/**/*.rs, so Tailwind discovers an unused important daisyUI status utility. [Rebuilt CSS delta](logs/css-readable.diff) contains only that utility (+1011 bytes), not a product styling change. This was not checked in R1/R2 focused review; the explicitly outstanding coordinator final gate found it. Prior passes remain historical phase acceptance, not final-gate claims.

Correction: coordinator chose logically equivalent `all(ne)` instead of `!any(eq)` in the inner supported-contract predicate; [exact delta](logs/css-correction.diff). This avoids the false utility discovery without changing the scanner or checking in unused CSS. No authority, schema, concurrency or bound change.

Instruction lesson: existing full CSS gate successfully catches Rust-to-CSS scanner coupling. Candidate narrow guidance: run check:css when a Rust edit introduces a utility-like token in scanned source, even without HTML changes. This is one observed coupling; no instruction edits authorized or made.

Resolution 2026-10-28 R3: FIXED at ca4075dfee53c125bcf1824822a68d92465080db3a109c15261f5daf742db067. Independent predicate review confirms missing model still rejects, including empty supported-contract sets; outer required-model any and resource/actor/error checks unchanged. Independent `npm run check:css` PASS ([log](logs/reviewer-r3-css.log)); assets/app.css has no diff. Author focused binding4 and database jev_binding5 PASS, clippy PASS. Final R3 review note records completion and remaining verification scope.
