# P10 reviewer findings

> **No backward compatibility.** Fresh database only; no backfills, dual reads/writes, compatibility shims or staged legacy rollout.

WF-P10-R001: no actionable defects or optional suggestions recorded. This does not convert the documented exact-public-A diagnostic limitation into tested coverage. Implementation and final phase acceptance remain pending.


## WF-P10-F001 — Promised lost-response scenario returns a complete HTTP 500

- Identity: 2026-10-25; implement-astra-reviewed-2.2; independent Reviewer B (requested Astra/medium; runtime identity not independently exposed); p10 B1 whole-batch review WF-P10-R002; /Users/mac02/dev/openbot workflows; starting/base/HEAD5d4aab1d44e209fbb46bae275be77c86cd3fef22; artifact `7abd12267b7119e99006720eeaa5ba3a4455d04bf50461945134f6d7bca223ad`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); accepted P10-D1r1.
- Classification: actionable verification/documentation defect; **Low**.
- Evidence: `http_reconciliation_integration_support.rs:46–52` counts the effect and always returns `StatusCode::INTERNAL_SERVER_ERROR` through Axum. No connection is dropped, response withheld or body truncated. The real HTTP adapter therefore receives a complete status response and fails its non-2xx branch (`src/adapters/workflow_http/mod.rs:188–196`), not its transport/read failure branch (`:180` or chunk handling). Yet the test describes the second request as a lost answer and `docs/workflow-reconciliation-safety.md` says it loses a response after one effect. P10-D1r1 explicitly requires lost provider response with restart/replay safety; the current test establishes useful ambiguous-server-error behavior but leaves that promised failure mode unexercised.
- Required correction: add or change the applied-request scenario so the counted remote effect happens and the client genuinely loses a usable response (for example a controlled disconnect or incomplete response), then retain duplicate/stale/competing-claimant/evidence/receipt-only completion assertions. Ensure the fixture distinguishes transport loss from the current complete500 outcome, and describe the exact exercised mechanism in the docs. No production schema/behavior change is requested.
- Verification: independent new journey1/1 and full focused43/43 pass at stock2MiB on ownedPG55500 (logs/r002-integration.log and r002-focused.log); this is missing scenario coverage, not a failing existing test or a demonstrated unsafe production repeat.
- Instruction lesson: name fault injection by the actual provider/transport behavior, not just the uncertainty it produces; compare promised failure modes to the exact adapter branch exercised. Cause otherwise unknown.
- Resolution history: open2026-10-25 atR002. R001 decision still stands; omission is in implementation of its verification promise.

- Resolution update2026-10-25 / WF-P10-R003 at `e8e20cf25f93466f1e09168fde58e2cc7d1c42dc6ab68d0fccefe5bc29077a43`: **F001 fixed**. Applied provider branch counts its effect, builds200 with declared11-byte body, then the actual streamed body yields ConnectionReset. The client cannot receive a complete response; first no-effect500 remains separate. Independent full recovery journey1/1 passed with stock2MiB and external-network denial (logs/r003-integration.log), retaining two calls/one effect, one-use evidence and receipt-only successful continuation assertions. No production changes.


## WF-P10-F002 — Shutdown integration test still expects three owned tasks

- Identity: 2026-10-25; implement-astra-reviewed-2.2; Reviewer B (requested Astra/medium, runtime identity not independently exposed); p10 B2/final integration WF-P10-R004; /Users/mac02/dev/openbot workflows; start/base/HEAD `5d4aab1d44e209fbb46bae275be77c86cd3fef22`; artifact `dc41913be23197f8910048d9d6e6cf9da69dbfbf1c99f17627548b049b8e47a5`; [TASK](TASK.md), [REVIEW](REVIEW.md).
- Classification: actionable verification defect; **Medium**.
- Evidence: `src/infra/workflow.rs:441–458` adds the metrics sampler to the returned owned handles; the one-slot fixture therefore returns four handles (slot, metrics, intent, schedule). `production_workers_stop_on_the_shutdown_token` at `:748–755` still asserts three and names only the former three owners. This deterministic assertion fails before cancellation/join assertions run, breaking the mandatory full-suite gate and leaving the combined sampler lifecycle assertion unexercised. Source inspection proves the mismatch; the author full suite is still running at this review return, so this entry does not claim its failure log has completed.
- Required correction: update the lifecycle expectation to include the metrics owner while preserving cancellation and joining of every returned handle; run that production composition test and finish the required full gates.
- Instruction lesson: when adding an owned background task, include the existing production composition/shutdown test among the focused affected-interface checks. Cause beyond the stale assertion is unknown.
- Resolution history: open at R004. No gated-design defect; sampler ownership itself is consistent with the host.


## WF-P10-F003 — Historical notice splits the compatibility policy sentence

- Identity: 2026-10-25; implement-astra-reviewed-2.2; Reviewer B (requested Astra/medium, runtime identity not independently exposed); p10 B2/final integration WF-P10-R004; /Users/mac02/dev/openbot workflows; start/base/HEAD `5d4aab1d44e209fbb46bae275be77c86cd3fef22`; artifact `dc41913be23197f8910048d9d6e6cf9da69dbfbf1c99f17627548b049b8e47a5`; [TASK](TASK.md), [REVIEW](REVIEW.md).
- Classification: actionable documentation defect; **Low**.
- Evidence: `plan/workflows/post-approval-workflow-execution.md:1–5` inserts the new historical notice between “reset the database” and “when needed”. Markdown consequently attaches the latter continuation to the historical-proposal block instead of the no-backward-compatibility notice. The other historical notices do not split their existing policy.
- Required correction: place the historical notice before or after the complete existing compatibility block.
- Verification: direct diff/render-structure inspection; no Rust check needed for this text correction.
- Instruction lesson: preserve the full surrounding paragraph when inserting a document notice; no broader rule change justified.
- Resolution history: open at R004.


### R005 resolution updates — 2026-10-25

Artifact `37915f1301f66f04c5fff3f2a2ea28bc1fb98649eeb7997f39e00b61b4661ecb`, unchanged start/base/HEAD5d4aab1d44e209fbb46bae275be77c86cd3fef22.

- **F002 fixed**: production shutdown test captures four tasks, cancels and joins every handle before asserting the count. Corrected focused test1/1 and complete stock2MiB library2311/2311 passed (logs/final-corrected-shutdown.log, final-stack.log). Original full-suite failure retained in final-tests.log confirms precisely the reviewed4-versus3 assertion; no other failures. No production behavior changed.
- **F003 fixed**: post-approval-workflow-execution.md now preserves the full original no-backward-compatibility paragraph and places the historical notice after it. Independent whitespace check passes.

All p10 findings are closed; no new actionable finding in R005.
