# JEV P01 — step contract

- Workflow: implement-astra-reviewed-2.2; phase p01; batch B1 (entire phase's two plan batches, one cohesive contract).
- Root/author: /root/phase_01, confirmed Astra/high by coordinator assignment. Independent reviewer: pending Astra/medium, no inherited context.
- Worktree: /Users/mac02/dev/openbot. Starting/review-base HEAD: 5a12ebec76c76c97e50b2a51ebacd66712db5820. Branch recorded in logs.
- Original requirements: ../../../../README.md and ../../../../01-step-contract.md (plan/jev-evaluate paths); applicable AGENTS.md at repository root, src/, src/application/.
- Preserved pre-existing changes: src/main.rs warning plan list; untracked plan/jev-evaluate and .agents/skills/implement-plan-phases.
- Coordination authority: /Users/mac02/dev/openbot/plan/jev-evaluate/COORDINATION.md, writer /root, revision 1; JEV-I001/r1 and JEV-I002/r1 acknowledged. No notices.
- Owns registry Jev contract, compiler completion semantics, failure classification, language documentation and focused tests. No SQL, HTTP transport, production activation, credentials, or agent provider integration.
- Resources: shared checkout/build cache, serialized checks; no database fixture required for compiler tests. DATABASE_URL unset; coordinator reports local PostgreSQL reachable.
- Discovery limitation: graft binary/index unavailable (coordinator preflight); source discovery uses rg. Graph refresh unavailable. Context occupancy/compaction target not exposed; durable records used.
- Status: architecture gate pending. Implementation not started; not accepted or integrated.

## Architecture gate JEV-P01-D1 / r1

Implement one reusable application-owned Jev contract module, with a strict tagged Question enum for Noul, Choice, Score. All literal validation and exact output schema derivation share this enum. Question/option IDs bounded nonblank strings; instructions/descriptions <=16 KiB UTF-8; 1–32 questions; 1–64 Choice options; 2–10 Score levels. Existing source decoder rejects duplicate keys before Value conversion. Box remains unsupported. Pin model syntax to jev-<major>.<minor>.<patch>, no moving aliases.

Resource selection uses existing declared-resource validation with kind typesafe; additionally reject unused typesafe declarations. Registration freezes connection, model and questions via const schemas and literal_fields, included in compiled hash. Existing ModelStep enum stays agent-only. Metadata classifies Jev as ModelInvocation / ReuseCommittedResult, with ModelInvoke and ResourceUse; I/O completion and effect-free retry classification include it without entering action semantics.

Exact schemas reject extra/missing answer keys, tags, option/level keys, malformed legend and wrong model. Additional completion validation checks sum(probabilities) within absolute 1e-6 of 1, with finite [0,1] fields enforced by JSON/schema. Noul remains probability only. No new persistence contract or SQL is needed.

Bound borrowed input before clones/serialization using existing context structural checks and a no-buffer JSON byte counter. Add fixed Jev request/state/output bounds at 64 KiB (also subject to stricter workflow context limits); request accounting covers model/state/questions envelope, and output accounting covers the entire response. Registry preflight charges future descriptor copies before generation; compiled fact budget remains 1 MiB. Runtime prepare validates dynamic state string/object/array and aggregate budget before provider dispatch. Provider adapter and token/deadline/spend handling belong to phase 03.

Failure behavior: compiler diagnostics for literals/declarations, runtime input diagnostics for dynamic invalid/oversized state, runtime output diagnostics before completion for malformed responses. No provider can be called by this phase.

Verification: list then execute jev_contract_, workflow_registry_, workflow_compiler_, workflow_publication_ filters. Include mixed requests, exact output paths, unknown/duplicate fields, bounds at/over limits, mismatched responses, probability sum tolerance, static hashes, runtime state bounds, and failure classification. Coordinator owns final full-suite gate. No architecture schema/persistence/concurrency changes beyond the application contract.

## Upstream verification (2026-10-28)
Read https://docs.typesafe.ai/api/system-one, /primitives, /models, /primitives/choice, /primitives/noul, /primitives/score. Confirmed model/state/questions request; answers/model/usage envelope; Noul only noul; Choice choice/probabilities/confidence; Score zero-based score, numeric-string level probabilities and legend descriptions, confidence. Exact published version jev-1.14.0; aliases move. Platform text-string-only instruction/criteria and 64-choice bounds intentionally narrower than upstream. Probability sum tolerance is application policy.

## Implementation decisions and coordination request

- Architecture JEV-P01-D1/r1 passed by independent B, note JEV-P01-D1-R1; decision SHA-256 e5440f963054fd723b915e54f67d4c2e2dc74efb4bf5d00f01b1de0af2c939db.
- Branch main; no commits authorized or made. Catalogue revision changes to workflow-catalogue-v1.6. Compiler semantic revision remains 6: existing registered-kind evaluation semantics are unchanged, and the newly introduced kind cannot appear in older compiled publications. Its exact specialization is hashed under the new catalogue revision. Existing database frozen_v6 budget classifier remains untouched.
- Production composition refuses jev.evaluate through UNHANDLED_STEP_KINDS until runtime exists; this preserves the existing every-I/O-kind-handled-or-refused invariant. Added src/infra/AGENTS.md to applicable instructions.

### Notice request JEV-C001 / r1 (to coordinator /root)
- Targets: p02, p03, p04, p05; effective after p01 acceptance.
- Cause/contracts: phase 01 adds catalogue v1.6, pure application registry::jev contract module and an intentional production publication refusal.
- Required action: p02 adds TypeSafe connection/publication authority; p03 uses frozen literal questions and exact compiled completion validation, preserves 64 KiB request/response bounds (including serialized envelope), and does not add Jev to agent-only ModelStep. p04 extends the one Question enum and schema derivation for Box with authorized image inputs. p05 removes jev.evaluate from infra::workflow::UNHANDLED_STEP_KINDS only after the complete handler supports it, and updates temporary help/language text. Leave compiler semantic revision 6 unless a later phase changes existing semantics and updates the database budget classifier in the same accepted change.
- Verification: every I/O kind is handled XOR refused; mixed questions compile and validate; adapter honors bounded preparation/completion; production publication refuses partial runtime; final full suite after phases 01–05. Producer evidence is p01 final reviewed artifact and JEV-P01-D1-R1, not this mutable summary.
- Status: request pending publication by sole coordination writer; p01 can continue unaffected checks.

## Focused verification in progress

- Coordination rechecked at revision 2; JEV-I001/r1 and JEV-I002/r1 acknowledged. JEV-C001/r1 published active, targets consumers p02–p05; p01 producer acknowledgment recorded, activation awaits this phase acceptance.
- `cargo test --locked --offline --lib jev_contract_ -- --list`: PASS, initially 13 tests listed (logs/jev-list.log).
- Initial `cargo test --locked --offline --lib jev_contract_`: 12 passed, 1 failed (logs/jev-tests.log). The failed new test incorrectly expected existing compiler reference availability analysis to prove cross-schema type compatibility. Existing generic compiler contract checks destination value types during resolved preparation. Removed that incorrect static expectation and added a separate preparation test proving probability-to-Boolean rejection; malformed reference paths still fail compilation. No production validation was weakened.
- `cargo test --locked --offline --lib workflow_registry_ -- --list`: PASS, 28 tests listed (logs/registry-list.log).
- Added explicit response/aggregate-question ceiling tests; final Jev list/run pending with 15 tests expected.

## B1 final author verification and frozen artifact

Status: implementation complete; awaiting independent batch review (not accepted).

All commands ran in /Users/mac02/dev/openbot. Final focused tests: **79 executed, 79 passed**, none skipped. List commands confirmed nonzero matches before execution:

| Command | Result / actual count | Log |
| --- | --- | --- |
| `cargo test --locked --offline --lib jev_contract_ -- --list` | 15 listed | logs/jev-final-list.log |
| `cargo test --locked --offline --lib jev_contract_` | PASS, 15 tests | logs/jev-final-tests.log |
| `cargo test --locked --offline --lib workflow_registry_ -- --list` | 28 listed | logs/registry-list.log |
| `cargo test --locked --offline --lib workflow_registry_` | PASS, 28 tests | logs/registry-tests.log |
| `cargo test --locked --offline --lib workflow_compiler_ -- --list` | 16 listed | logs/compiler-list.log |
| `cargo test --locked --offline --lib workflow_compiler_` | PASS, 16 tests | logs/compiler-tests.log |
| `cargo test --locked --offline --lib workflow_publication_ -- --list` | 17 listed | logs/publication-list.log |
| `cargo test --locked --offline --lib workflow_publication_` | PASS, 17 tests | logs/publication-tests.log |
| `cargo test --locked --offline --lib production_handler_covers_every_io_kind_or_refuses_it -- --list` | 1 listed | logs/handler-list.log |
| `cargo test --locked --offline --lib production_handler_covers_every_io_kind_or_refuses_it` | PASS, 1 test | logs/handler-tests.log |
| `cargo test --locked --offline --lib workflow_effect_free_kinds_ -- --list` | 1 listed | logs/effects-list.log |
| `cargo test --locked --offline --lib workflow_effect_free_kinds_` | PASS, 1 test | logs/effects-tests.log |
| `cargo test --locked --offline --lib production_publication_refuses_memory_steps_and_captures_children -- --list` | 1 listed | logs/production-publication-list.log |
| `TEST_DATABASE_URL="postgres://$(whoami)@localhost:5432/openbot_jev_p01_test" cargo test --locked --offline --lib production_publication_refuses_memory_steps_and_captures_children` | PASS, 1 actual database-backed test, fresh migrations included | logs/production-publication-tests.log |
| `cargo fmt --all -- --check` | PASS | logs/fmt.log |
| `git diff --check` | PASS | no output |

DB fixture: explicit TEST_DATABASE_URL only for the named test; DATABASE_URL remained unset. `own_database` created, freshly migrated and dropped a UUID-named `mail_agents_own_*` local database using that endpoint; no development/production rows modified. No separate migration/SQL changes. Full suite remains coordinator-owned after all phases.

Snapshot script initially unavailable directly on macOS (`realpath -m` unsupported). Preserved workflow script unchanged and used a minimal Python `realpath` CLI shim in excluded logs/snapshot-bin. Exact reproducible invocation:

`PATH="$PWD/plan/jev-evaluate/phases/p01/docs/logs/snapshot-bin:$PATH" bash .agents/skills/implement-astra-reviewed-2.2/scripts/snapshot.sh plan/jev-evaluate/phases/p01/docs plan/jev-evaluate/COORDINATION.md`

Exact exclusions: `plan/jev-evaluate/phases/p01/docs`, `plan/jev-evaluate/COORDINATION.md` (workflow-only records). Start, review base, current HEAD: `5a12ebec76c76c97e50b2a51ebacd66712db5820`.
Frozen B1 digest: `16398c0978ae78f217256a05a4310ea88c5dfe71fa5d160aee9446c8a66e7157` (logs/b1-snapshot.txt).
Pinned coordination revision 2: logs/b1-coordination-r2.md. JEV-I001/r1 and JEV-I002/r1 satisfied for phase scope; JEV-C001/r1 producer request published, consumer activation awaits acceptance. Review covers the complete phase delta plus preserved pre-existing work identified above; no exclusions hide source or deliverables.

Limitations: graft unavailable; context compaction target unavailable. Neither is reported as verified. No provider/network execution, credential or image behavior claimed for p01.

## Acceptance — JEV-P01-B1

**Accepted in phase; ready for coordinator integration/phase-02 dependency use.** Independent reviewer explicit PASS JEV-P01-B1-R1 for digest `16398c0978ae78f217256a05a4310ea88c5dfe71fa5d160aee9446c8a66e7157`; no findings or unresolved defects. Reviewer reran 15 Jev tests and inspected retained author evidence for the other 64 tests. Author/root reread saved REVIEW/notes/findings, rechecked coordination revision 2 and recomputed the same digest (logs/b1-acceptance-snapshot.txt). Architecture JEV-P01-D1-R1 retained. JEV-C001/r1 producer contract is now ready for consumers; coordinator owns notice update and overall integration status.

All required focused checks passed; full task verification awaits later phases. Graft and strict compaction-target control remain unavailable tooling limitations. No active worker/check processes or retained disposable databases. Existing source behavior preserved; no production deployment or commits performed. Next owner /root schedules p02 with this accepted artifact and JEV-C001/r1.
