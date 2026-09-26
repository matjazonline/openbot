# Workflow foundation execution progress

Original plans remain authoritative. Each point carries the full named section/item, README rules, and applicable phase acceptance criteria. Execute phases 01–10 in order. Each phase depends on the preceding phases. No backward compatibility is required: do not add legacy shims.

## User stop boundary

User instruction received during 01.5 corrections: save progress and the resume location, and STOP after phase01 is complete. Finish01.5–01.7 and phase01 verification, then do not start02. Resume later at02 Definition and context contract after checking the actual tree/evidence. No commit was requested.

User model preference: when next recreating an implementation sub-agent, use `gpt-6-sol` with `reasoning_effort: high` and `fork_turns: none`. Applies to future replacements and resume. Existing Sol04 may finish while context-suitable. Astra expansion/review lifecycle is unchanged.

## Baseline

Preserve existing user edits to 01, 02, 04, 09, 10, NOT_PLANNED, README. No implementation changes existed at startup. No database reset is authorized. Production cutover requires an identified target and applicable execution authorization.

Parent session: 01a0ddce-d419-7523-96cb-931ec5bfa2b2. Startup context: 10.24%, 26,453 / 258,400 tokens; timestamp 2026-09-26T13:03:01.871Z; runtime-reported capacity.

## Ordered queue

Phase01 and all seven of its points are VERIFIED. Phases02–10 remain queued and were not started. Numbered items remain separate points; descriptive sections are separate top-level work units. Acceptance sections are gates attached to their phase, not substitute implementation tasks.

1. 01-architecture-and-domain.md: Implementation items 1–7, individually.
2. 02-workflow-language-and-publication.md: Definition and context contract; Registered step types; Publication and binding items 1–6, individually.
3. 03-durable-runtime-and-persistence.md: Admission and independent contexts; Execution and commits items 1–7, individually; Failure and recovery.
4. 04-actions-http-and-delivery.md: Action contract; Execution and uncertainty items 1–7, individually; HTTP and registered tools; MCP tool calls over HTTP; Provider-neutral messaging.
5. 05-human-decisions-and-waits.md: Decision contract; Comments versus submission; Assignment, authorization, and notification; Other waits.
6. 06-agent-context-memory-and-capabilities.md: Rig integration; Explicit preparation steps; Skills and capability profiles (nested resolution rules remain part of this contract).
7. 07-child-workflows-and-review-revisions.md: Child calls and workflow tools; Retry versus revision; Reusable draft-and-review round; Feedback to a different next step.
8. 08-triggers-and-application-replacement.md: Channels and ingress; Other entry points; Public interfaces; Removal work.
9. 09-authoring-and-operations-ui.md: Authoring and setup; Runs and conversations; Human work queue; Sample execution.
10. 10-verification-and-cutover.md: End-to-end acceptance matrix; CI and reproducibility; Operations; Cutover items 1–7, individually, subject to target and execution authorization.

## Current point

STOPPED AT USER-REQUESTED BOUNDARY: phase01 complete, all seven points VERIFIED. No phase02 work started. Resume at **02-workflow-language-and-publication.md — Definition and context contract** only upon a later explicit resume. See [RESUME.md](RESUME.md). No commit/deployment/database reset performed.

| Point | Verified deliverable |
| --- | --- |
| 01.1 | Checked workflow IDs, definitions, bounded graph/context validation and definition-owned routing |
| 01.2 | Cohesive internal application ports and atomic admission/cancellation contracts |
| 01.3 | Typed completed/waiting/failed outcomes and engine-owned retry/route disposition |
| 01.4 | Six run states and waiting reasons, pure transitions, claim/cancel race coverage |
| 01.5 | Current lifecycle authorization, independent channel/thread visibility, replay reauthorization |
| 01.6 | Eight-family replacement map with callers, UI projections, single owners and deletion gates |
| 01.7 | Scoped trigger/run/execution/action/child/message causality; correlation excluded from authority/dedup/fences |

## Final phase01 verification

Same point07 Astra explicitly passed the combined phase01 acceptance gate after inspecting actual code and all corrections. Original plan requirements are unchanged. This foundation establishes ownership/contracts; it does not install a production workflow runtime. Production persistence/effect protocols, YAML compiler/publication, trigger wiring and UI remain in later phases.

- `cargo fmt --all -- --check`: PASS.
- `git diff --check`: PASS.
- `SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow`: PASS,53/53. Parent independently reran final tree.
- `SQLX_OFFLINE=true cargo check --locked --offline --all-targets`: PASS.
- `SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings`: PASS. Parent independently reran final tree.
- Graft refreshed after final code changes. Replacement-map source references and docs checked.
- No SQL/migrations/providers changed. Full database-backed suite and whole-suite stack-budget script were not run in this foundation-only phase; isolated persistence races and broader runtime gates remain assigned to their owning later phases.
- No temporary databases/services/workers/resources to clean up. All implementation and progress changes remain uncommitted in the shared working tree.

## Evidence and context checkpoints

Per-point briefs, agent tool IDs and verified session UUIDs, context samples, changed files, acceptance evidence, findings, cleanup, and next actions will be recorded here or in linked files beside this record.

- 01.1 Astra tool ID `/root/astra_01_1`, session `01a0ddd1-bf31-74c2-a4bd-44b6e17e92f6`; after expansion 18.00%, 46,519 / 258,400 tokens at 2026-09-26T13:07:07.783Z, runtime capacity. Suitable for same-point review. Graft savings 229,858 tokens.
- User's original plan edits were committed externally as `a3f6633 plan` during expansion. Preserve that HEAD; no agent commit occurred.
- 01.1 Sol tool ID `/root/sol_01`, session `01a0ddd4-a3b8-74a3-b30b-d12b21717857`; after implementation 27.75%, 71,708 / 258,400 tokens at 2026-09-26T13:24:15.857Z, runtime capacity. Suitable for corrections/next point. Graft savings 2,426,007 tokens.
- 01.1 changed `src/domain/mod.rs` and new `src/domain/workflow/{mod,ids,definition,graph,context,transition}.rs`. Sol reports final fmt, whitespace, focused offline tests (10/10), 2 MiB stack tests (10/10), locked offline all-target check passing. No DB changes, temporary resources, or commits. Graft refreshed. Same Astra resumed for actual-code acceptance review.
- 01.1 review 1 requires fixes: type errors vs missing defaults, bounds before auxiliary allocation, missing definition fields, checked execution limits, context reference newtype, route acceptance cases. Same Astra after review: 27.38%, 70,745 / 258,400 tokens at 2026-09-26T13:25:43.130Z. Review graft savings 28,614 tokens. Parent confirmed pre-correction 10/10 at 2 MiB. Corrections and new evidence pending.
- 01.1 corrections complete: Sol reports all six findings fixed, focused and 2 MiB tests 15/15, fmt/whitespace/locked offline all-target check pass. No cleanup needed. After corrections Sol 37.17%, 96,053 / 258,400 tokens at 2026-09-26T13:32:28.411Z. Correction graft savings 135,449. Same Astra resumed for review 2.
- 01.1 VERIFIED: Astra review 2 explicitly passed all criteria and independently ran 15/15 at 2 MiB; parent independently confirmed 15/15 at 2 MiB too. No outstanding point checks. Astra boundary sample 33.08%, 85,467 / 258,400 tokens at 2026-09-26T13:33:12.583Z; final review graft savings 65,172. Sol last correction sample recorded above; eligible for reuse below 50%. No temporary resources. Entire phase 01 is not yet complete.
- 01.1 final Sol depth-0 sample: 37.33%, 96,473 / 258,400 tokens at 2026-09-26T13:32:35.083Z. Reuse for 01.2 permitted.
- 01.2 Astra tool `/root/astra_01_2`, session `01a0ddec-10b4-7f30-bd63-943c987e3107`, expanded; 23.40%, 60,475 / 258,400 tokens at 2026-09-26T13:37:02.946Z; graft savings 470,619. Same reviewer reserved for 01.2 review.
- 01.2 Sol implementation: application/workflow contracts/ports/service/tests, application module registration, explicit domain workflow root re-export in lib.rs to resolve ambiguity. Prior domain files preserved. Reported fmt/whitespace/offline all-target check pass; application 8/8 normal and 2 MiB, domain 15/15 at 2 MiB. No DB/resource cleanup. Graft refreshed; savings 85,884. Depth-0 sample 49.72%, 128,486 / 258,400 at 2026-09-26T13:49:06.026Z. Remeasure before reassignment; same Astra reviewing actual code.
- 01.2 review 1: two required corrections recorded in brief. Parent and Astra independently confirmed application 8/8 at 2 MiB. Astra 30.92%, 79,900 / 258,400 at 2026-09-26T13:50:20.093Z, savings 23,030. Sol latest 49.98%, 129,152 / 258,400 at 2026-09-26T13:49:11.835Z; eligible for this one correction assignment, remeasure and retire at >=50 before further work.
- 01.2 correction checks pass: named AdmissionSnapshots + complete public port contracts; application 8/8 normal/2 MiB, domain 15/15 at 2 MiB, fmt/whitespace/offline all-target compile. Sol 53.67%, 138,672 / 258,400 at 2026-09-26T13:53:31.130Z. RETIRED `/root/sol_01`; interrupt called after completed edits, no process termination claimed. No future assignments. Graft savings 4,406. Same Astra review 2 pending.
- 01.2 VERIFIED: same Astra review 2 passed actual code, independently confirmed 8/8 at 2 MiB + whitespace. Parent confirmed final 8/8 at 2 MiB. No required checks outstanding for this point. Astra final depth-0 33.59%, 86,797 / 258,400 at 2026-09-26T13:54:14.408Z; review savings 16,929. Sol boundary sample above; retired. No temporary resources.
- 01.3 expanded by fresh `/root/astra_01_3`, session `01a0ddff-124d-78e2-9c02-4a7c952a0b50`, 20.03%, 51,745 / 258,400 at 2026-09-26T13:56:31.211Z; graft savings 172,790. Same reviewer reserved. Fresh Sol `/root/sol_02` assigned implementation; session UUID pending verified report.
- 01.3 Sol `/root/sol_02` session `01a0de01-446b-7792-8909-0a2acec40499`: 24.85%, 64,201 / 258,400 at 2026-09-26T14:05:23.396Z. Changed domain workflow context/ids/mod + new outcome.rs. Final reported fmt/diff, domain22 normal/2MiB, application8 at2MiB, locked offline all-target pass. Graft refreshed, savings2,383,345. No SQL/resources/cleanup/commits. Same Astra reviewing.
- 01.3 review1 behavior passed; public semantic/atomicity docs missing, correction in brief. Parent and Astra independently confirmed22/22 at2MiB. Astra25.64%,66,252/258,400 at2026-09-26T14:06:07.466Z; review savings46,943. Sol latest24.91%,64,374/258,400 at2026-09-26T14:05:30.121Z, resumed docs-only correction; fmt/diff+rereview sufficient.
- 01.3 VERIFIED: docs-only correction passed same Astra review2, fmt/diff pass; prior behavior checks remain valid. No resources/cleanup. Sol correction savings54,202; final depth0 26.96%,69,658/258,400 at2026-09-26T14:06:58.346Z. Astra final27.11%,70,050/258,400 at2026-09-26T14:07:09.577Z, rereview savings82,173. No outstanding point criteria/checks.
- 01.4 expansion `/root/astra_01_4`, session `01a0de0a-e0ba-7f63-9887-3e14227e9eb2`:20.58%,53,168/258,400 at2026-09-26T14:09:23.837Z, savings49,312. Same reviewer reserved; Sol02 assigned. Explicit decisions in brief: Running active logical work; End error route handled success; terminal cancellation preserves state.
- 01.4 Sol changes: domain state/outcome/mod; application contracts/ports/test fixtures/state_cases. Reported final fmt/diff/domain26 normal+2MiB/application11 at2MiB/offlinealltargets pass, graph refreshed; savings507,461. No resources/SQL/commits. Sol depth0 42.43%,109,634/258,400 at2026-09-26T14:17:35.878Z, eligible corrections. Same Astra reviewing actual code.
- 01.4 review1: stale waiting-reason docs and deterministic coverage of both claim/cancel orderings required. Parent confirmed11 app tests2MiB; Astra37 workflow tests2MiB. Astra26.46%,68,383/258,400 at2026-09-26T14:18:22.865Z,savings152,282. Sol42.53%,109,903/258,400 at2026-09-26T14:17:41.119Z resumed corrections.
- 01.4 correction: controlled claim-first/cancel-first/stale-revision cases added, real barrier test retained; stale docs fixed. fmt/diff/app13+domain26 at2MiB/offlinealltargets pass, graph refreshed. Sol46.45%,120,038/258,400 at2026-09-26T14:20:58.657Z,savings100,445. Same Astra rereview pending. No cleanup.
- 01.4 VERIFIED same Astra review2; parent and Astra independently confirmed final13 app tests2MiB. All point criteria/checks pass. Final Astra28.20%,72,879/258,400 at2026-09-26T14:21:21.928Z,savings56,085. Final Sol46.54%,120,269/258,400 at2026-09-26T14:21:04.146Z. No cleanup.
- 01.5 expansion `/root/astra_01_5`,session `01a0de17-e043-7f52-8ef2-c0ff3079122a`:31.60%,81,654/258,400 at2026-09-26T14:24:21.980Z,savings4,017,052. Same reviewer reserved. Sol02 assigned (46.54%); remeasure/retire if >=50 after implementation. Current management policy owner/admin; later trigger/manual policy explicitly deferred, not broad grant.
- 01.5 implementation: application authorization and tests added; service/contracts/ports/factories updated. Reported46 workflow tests2MiB,offlinealltargets,fmt/diff pass, graph refreshed; savings117,740. No resources/SQL/commits. Sol02 depth0 61.75%,159,569/258,400 at2026-09-26T14:36:24.252Z; RETIRED, interrupt called after completion. Same Astra reviewing; replacement Sol for any corrections.
- 01.5 review1: behavior sound; public docs incomplete and610line auth test module/long functions need split. Parent independently confirmed46/46 at2MiB. Astra39.40%,101,811/258,400 at2026-09-26T14:37:45.735Z,savings49,137. Fresh `/root/sol_03` assigned corrections; UUID pending. Same Astra remains reviewer below50.
- 01.5 correction Sol03 session `01a0de27-229a-7743-8db2-99b5054c22e8`:28.11%,72,632/258,400 at2026-09-26T14:42:43.494Z,savings112,960. Public docs completed, tests split into8 behavioral files, largest function69lines. Final46 workflow2MiB/fmt/diff/offlinealltargets pass; graph refreshed, no cleanup. Same Astra rereview pending.
- 01.5 VERIFIED same Astra rereview, assertions preserved/doc obligations satisfied; no outstanding point checks. Review savings108,202. No cleanup. 01.6/01.7 remain before requested stop.
- 01.5 boundary Astra44.32%,114,523/258,400 at2026-09-26T14:43:36.452Z; Sol03 28.63%,73,977/258,400 at2026-09-26T14:42:59.226Z.
- 01.6 Astra `/root/astra_01_6`,session `01a0de2c-5e72-71b1-a9d3-0859bcf6af93`:26.57%,68,662/258,400 at2026-09-26T14:45:46.898Z,savings1,612,980. Brief recorded, Sol03 assigned doc inventory; same reviewer reserved.
- 01.6 implemented REPLACEMENT-MAP.md+README link,111source refs checked,8families/owners/callers/UI/gates; docs-only verification/diff pass. Sol03 60.12%,155,355/258,400 at2026-09-26T14:52:37.279Z RETIRED,interrupt called after completed edits. Savings at least1,871,068 (one tool output truncated). No resources/cleanup. Same Astra review pending.
- 01.6 review1 corrections recorded: owner precision, missing memory/preparation, wrong capability/publication spans, simulation can send, UI deletion gates. Astra36.28%,93,735/258,400 at2026-09-26T14:53:42.846Z,savings24,459. FreshSol04 assigned docs correction; same Astra reviewer.
- 01.6 corrections by `/root/sol_04`,verified session `01a0de35-a472-71c0-919f-37bf8d1bce79`:34.91%,90,200/258,400 at2026-09-26T14:57:49.428Z. All5 map findings addressed,exact spans/whitespace checked,161lines,docs only. Savings3,122,837. Same Astra rereview pending,no cleanup.
- 01.6 VERIFIED same Astra rereview all5 findings resolved,eightfamilies/callers/projections/gates retained,independent whitespace pass. Review savings13,425. No behavioral checks/cleanup required.
- 01.6 final Astra41.04%,106,047/258,400 at2026-09-26T14:58:27.823Z; Sol04 boundary34.91% above.
- 01.7 Astra `/root/astra_01_7`,session `01a0de39-ecf0-7c22-9c15-9bab659da4b8`:22.17%,57,285/258,400 at2026-09-26T15:00:38.156Z,savings2,511,960. Brief recorded,Sol04 assigned final point; same reviewer reserved for point+combined01gate.
- 01.7 implemented domaincausality and appadmission/head/claim provenance+seededsourcevalidation. Final53workflow2MiB/fmt/diff/offlinealltargets pass,graftrefreshed,savings94,711. Sol04 65.31%,168,754/258,400 at2026-09-26T15:17:13.602Z RETIRED after completion. No resources/cleanup. Same Astra point+phasegate review, parentclippy session22588. Replacement uses Sol/high per user.
- Finalreview: no behavior gap; clippy exit101(newfiles conditionals/borrows/testlock scopes),longcausaltests,Conflict docs corrections in BRIEF01.7. Astra34.54%,89,244/258,400 at2026-09-26T15:18:36.718Z,savings123,190. FreshSol/high05 assigned bounded cleanup+finalchecks. No new phase scope.
- Final correction `/root/sol_high_05` (`gpt-6-sol`,high),session `01a0de4c-7b1a-77c1-9b66-907099d0fcb8`:22.76%,58,806/258,400 at2026-09-26T15:23:28.892Z,savings95,493. All final gates PASS,53tests,graphrefreshed,no cleanup.
- 01.7 AND PHASE01 VERIFIED by same Astra final review. Final reviewer37.89%,97,909/258,400 at2026-09-26T15:24:00.416Z,savings37,959. Parent final53tests/Clippy/whitespace independently PASS. No outstanding phase01 findings. Stop boundary honored; next02 Definition and context contract.
- Parent graft tally2,321,572tokens across7calls; subagent savings recorded separately above. No pricing supplied.
