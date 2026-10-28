# Plan coordination: jev-evaluate

- Authority: /Users/mac02/dev/openbot/plan/jev-evaluate/COORDINATION.md
- Writer: /root, /Users/mac02/dev/openbot (initial file established by /root/phase_01; ownership handed to /root)
- Revision: 16

## Plan-wide instructions
- JEV-I001 / r1: Implement phases 01–05 sequentially in this checkout. No deployment or backward compatibility. Preserve pre-existing src/main.rs, plan files, and implement-plan-phases skill.
- JEV-I002 / r1: Each phase runs focused checks and independent Astra review. Coordinator runs the full formatting, offline compilation, migrations, and database-backed suite after all phases.

## Phase directory
| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | /root/phase_01 | /Users/mac02/dev/openbot | phases/p01/docs/TASK.md, phases/p01/docs/HANDOFF.md |
| p02 | /root/phase_02 | /Users/mac02/dev/openbot | phases/p02/docs/TASK.md, phases/p02/docs/HANDOFF.md |
| p03 | /root/phase_03 | /Users/mac02/dev/openbot | phases/p03/docs/TASK.md, phases/p03/docs/HANDOFF.md |
| p04 | /root/phase_04 | /Users/mac02/dev/openbot | phases/p04/docs/TASK.md, phases/p04/docs/HANDOFF.md |
| p05 | /root/phase_05 | /Users/mac02/dev/openbot | phases/p05/docs/TASK.md, phases/p05/docs/HANDOFF.md |

## Cross-phase notices

### JEV-C001 / r1
- State: active
- Origin: p01, /root/phase_01, phases/p01/docs/TASK.md, "Implementation decisions and coordination request"
- Targets: p02, p03, p04, p05
- Instruction/change: Use the p01 `registry::jev` contract and catalogue v1.6. Keep Jev separate from agent-only `ModelStep`. Preserve the 64 KiB serialized request/response bounds and exact compiled completion validation. p04 extends the existing question contract for Box and authorized images. p05 removes `jev.evaluate` from `infra::workflow::UNHANDLED_STEP_KINDS` and updates temporary help/language text only when the full production handler exists. Keep compiler semantic revision 6 unless a later change to existing semantics updates its database budget classifier together.
- Dependencies/order: Effective for consumers after p01 acceptance; p02–p05 start only after their preceding phase is accepted. Partial runtime must remain refused in production.
- Evidence/review: p01 architecture PASS JEV-P01-D1-R1 and implementation PASS JEV-P01-B1-R1 for digest `16398c0978ae78f217256a05a4310ea88c5dfe71fa5d160aee9446c8a66e7157`; see phases/p01/docs/TASK.md and REVIEW.md. Producer accepted in phase.
- Verification: Each consumer checks its affected contract and the handled-or-refused I/O-kind invariant; p05 checks final production activation. Coordinator runs the complete suite after all phases.
- Responses: p02 satisfied JEV-C001/r1 in phases/p02/docs/TASK.md and REVIEW.md, PASS JEV-P02-B1-R1; p03 satisfied in phases/p03/docs/TASK.md and REVIEW.md, PASS JEV-P03-B1-R2; p04 satisfied in phases/p04/docs/TASK.md and REVIEW.md, PASS JEV-P04-B1-R2; p05 satisfied in phases/p05/docs/TASK.md and REVIEW.md, PASS JEV-P05-B1-R3.

### JEV-C002 / r1
- State: active
- Origin: p02, /root/phase_02, phases/p02/docs/TASK.md, "Coordination request JEV-C002/r1"
- Targets: p03, p04, p05
- Instruction/change: Use `application::workflow::jev::{JevModel,JevConnections,JevAuthorization,JevCredential}` and `company_typesafe_connections`; credentials use enc:v2 context `typesafe_credential(company,id)`. Bindings and runs store selected connection ID only; required-model checks traverse frozen bundles and children. p03 calls fenced `authorize_jev(&ClaimedWorkflow)` at final provider use, treats revoked/missing/model-disallowed selections as terminal refusal, never searches a replacement connection, never checkpoints an ephemeral credential grant, and preserves authority-before-local-lock order in reservation/accounting. p03 uses fixed `https://api.typesafe.ai/v1/systemone` with a loopback-only test constructor; management accepts no origin field. p05 uses secret-free management views in authoring.
- Dependencies/order: Effective for consumers after p02 acceptance; p03–p05 wait for preceding phase acceptance.
- Evidence/review: p02 architecture PASS JEV-P02-D1-R1 and implementation PASS JEV-P02-B1-R1 for digest `b8cecd31a6d8ece8a31a0340f6eb82b353e8e9bbcc3c6a5e5d6741e0ab8b7bae`; see phases/p02/docs/TASK.md and REVIEW.md. Producer accepted in phase.
- Verification: p03 tests revoked/model-removed/current-key credential use and competing claims with reservation; p05 checks no secret in authoring or run artifacts and enables the production handler only when complete.
- Responses: p03 satisfied JEV-C002/r1 in phases/p03/docs/TASK.md and REVIEW.md, PASS JEV-P03-B1-R2; p04 satisfied in phases/p04/docs/TASK.md and REVIEW.md, PASS JEV-P04-B1-R2; p05 satisfied in phases/p05/docs/TASK.md and REVIEW.md, PASS JEV-P05-B1-R3.

### JEV-C003 / r1
- State: active
- Origin: p03, /root/phase_03, phases/p03/docs/TASK.md, "Coordination request JEV-C003/r1"
- Targets: p04, p05
- Instruction/change: Extend p03 `application::workflow::jev` prepared request/response and persistence ports plus `adapters::workflow_jev` for images/Box. Preserve exact request digest, current-use `authorize_jev`, fenced settlement, fresh reservation for every provider entry, and root accounting. Keep the 64 KiB local limits unless a separately reviewed change raises them. `UsageSample.agent` and `model_usage_calls.agent_id` are optional for TypeSafe because Jev is not an agent. p04 adapts request estimates/validation for assets; p05 displays absent agent correctly and removes temporary production refusal only when all phases are complete.
- Dependencies/order: Effective for consumers after p03 acceptance; p04/p05 wait for preceding phase acceptance.
- Evidence/review: p03 architecture PASS JEV-P03-D1-R1 and corrected implementation PASS JEV-P03-B1-R2 for digest `33e9963bb9ed70b8f2a183367fd54d6a4f4d44031b54d2549bd7d743b730e5d8`; see phases/p03/docs/TASK.md and REVIEW.md. Producer accepted in phase; F1/F2 fixed.
- Verification: Affected HTTP/runtime/accounting tests, production handled-or-refused invariant, and final mixed/image end-to-end cases.
- Responses: p04 satisfied JEV-C003/r1 in phases/p04/docs/TASK.md and REVIEW.md, PASS JEV-P04-B1-R2; p05 satisfied in phases/p05/docs/TASK.md and REVIEW.md, PASS JEV-P05-B1-R3.

### JEV-C004 / r1
- State: active
- Origin: p04, /root/phase_04, phases/p04/docs/TASK.md, "Coordination request JEV-C004/r1"
- Targets: p05
- Instruction/change: Image source uses literal `with.assets` private channel/thread/SHA256 references. Image wire state exposes original state under `content` plus `assets.NAME` paths; Box instructions identify declared `assets.NAME`. Show the experimental images/Box status and limits in authoring. Samples remain scripted with inert metadata and never read storage or call the provider. Preserve exact request digest and metadata-only persistence; keep production handler refused until p05 completes it.
- Dependencies/order: Effective for p05 after p04 acceptance; p05 waits for accepted p04 artifact.
- Evidence/review: p04 architecture PASS JEV-P04-D1-R1 and corrected implementation PASS JEV-P04-B1-R2 for digest `6f2cb93e22e3a4c9892a2fee8d44706666efea95074c4abb6fc14225ad03f45c`; see phases/p04/docs/TASK.md and REVIEW.md. Producer accepted in phase; F1 fixed.
- Verification: p05 mixed authoring sample, image-capable model selection, secret/byte-free run views, and final production activation.
- Responses: p05 satisfied in phases/p05/docs/TASK.md and REVIEW.md, PASS JEV-P05-B1-R3.

### JEV-C005 / r1
- State: active
- Origin: p04, /root/phase_04, phases/p04/docs/TASK.md, "Coordination request JEV-C005/r1"
- Targets: p05
- Instruction/change: Final integration must correct the inherited p02 `clippy::collapsible_if` in `src/application/workflow/binding/mod.rs:136-147` with a mechanical nested-if collapse. p04's own `nonminimal_bool` warning has been fixed; p04 preserves the accepted p02 binding source. This is a check gap, not a changed authorization or binding contract.
- Dependencies/order: Effective for p05 after p04 acceptance; p05 includes the correction in its reviewed batch.
- Evidence/review: p04 `phases/p04/docs/logs/f1-clippy-final.log`, accepted p04 PASS JEV-P04-B1-R2; inherited lint remains pending p05 correction.
- Verification: Independently review the affected binding check and pass `cargo clippy --locked --offline --all-targets -- -D warnings` in the final full gate.
- Responses: p05 satisfied in phases/p05/docs/TASK.md and REVIEW.md, PASS JEV-P05-B1-R3.

## Overall integration
All five phases are accepted in their own records. p05's final reviewed artifact is JEV-P05-B1-R3 at digest `ca4075dfee53c125bcf1824822a68d92465080db3a109c15261f5daf742db067`; the earlier R2 artifact was superseded by the CSS reproducibility correction. The coordinator's [final verification record](phases/p05/docs/COORDINATOR-VERIFICATION.md) reports all runnable post-R3 plan and affected CI checks passing, with `graft build` unavailable (exit 127). Source digest matched after the gate. No deployment or unqualified full-verification claim.
