# JEV-P01-D1 / r1 — reviewed decision

Implement one reusable application-owned Jev contract module, with a strict tagged Question enum for Noul, Choice, Score. All literal validation and exact output schema derivation share this enum. Question/option IDs bounded nonblank strings; instructions/descriptions <=16 KiB UTF-8; 1–32 questions; 1–64 Choice options; 2–10 Score levels. Existing source decoder rejects duplicate keys before Value conversion. Box remains unsupported. Pin model syntax to jev-<major>.<minor>.<patch>, no moving aliases.

Resource selection uses existing declared-resource validation with kind typesafe; additionally reject unused typesafe declarations. Registration freezes connection, model and questions via const schemas and literal_fields, included in compiled hash. Existing ModelStep enum stays agent-only. Metadata classifies Jev as ModelInvocation / ReuseCommittedResult, with ModelInvoke and ResourceUse; I/O completion and effect-free retry classification include it without entering action semantics.

Exact schemas reject extra/missing answer keys, tags, option/level keys, malformed legend and wrong model. Additional completion validation checks sum(probabilities) within absolute 1e-6 of 1, with finite [0,1] fields enforced by JSON/schema. Noul remains probability only. No new persistence contract or SQL is needed.

Bound borrowed input before clones/serialization using existing context structural checks and a no-buffer JSON byte counter. Add fixed Jev request/state/output bounds at 64 KiB (also subject to stricter workflow context limits); request accounting covers model/state/questions envelope, and output accounting covers the entire response. Registry preflight charges future descriptor copies before generation; compiled fact budget remains 1 MiB. Runtime prepare validates dynamic state string/object/array and aggregate budget before provider dispatch. Provider adapter and token/deadline/spend handling belong to phase 03.

Failure behavior: compiler diagnostics for literals/declarations, runtime input diagnostics for dynamic invalid/oversized state, runtime output diagnostics before completion for malformed responses. No provider can be called by this phase.

Verification: list then execute jev_contract_, workflow_registry_, workflow_compiler_, workflow_publication_ filters. Include mixed requests, exact output paths, unknown/duplicate fields, bounds at/over limits, mismatched responses, probability sum tolerance, static hashes, runtime state bounds, and failure classification. Coordinator owns final full-suite gate. No architecture schema/persistence/concurrency changes beyond the application contract.


# Retained coordination revision 1
# Plan coordination: jev-evaluate

- Authority: /Users/mac02/dev/openbot/plan/jev-evaluate/COORDINATION.md
- Writer: /root, /Users/mac02/dev/openbot (initial file established by /root/phase_01; ownership handed to /root)
- Revision: 1

## Plan-wide instructions
- JEV-I001 / r1: Implement phases 01–05 sequentially in this checkout. No deployment or backward compatibility. Preserve pre-existing src/main.rs, plan files, and implement-plan-phases skill.
- JEV-I002 / r1: Each phase runs focused checks and independent Astra review. Coordinator runs the full formatting, offline compilation, migrations, and database-backed suite after all phases.

## Phase directory
| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | /root/phase_01 | /Users/mac02/dev/openbot | phases/p01/docs/TASK.md, phases/p01/docs/HANDOFF.md |

## Cross-phase notices
None.
