# CHS p02 — Terminal outcomes and channel.no_reply

## Identity and state

- Original requirements: `../../../02-terminal-outcomes-and-no-reply.md`; entire plan03 and subsequent plan04 authorized.
- Workflow `implement-astra-reviewed-2-2`; phase p02; whole-phase batch CHS-P02-B1.
- Root/A `/root`, user-confirmed Sol6.1/xhigh; B `/root/channel_reply_reviewer`, requested Astra/high, independent context, reuse after p01. Runtime/context/compaction metadata unexposed; durable checkpoints apply.
- Worktree `/Users/mac02/dev/openbot`, branch main, starting/review-base/HEAD `769a23aa6ef423e964f8221a8ee6b0743325c6ba`.
- Preserved pre-existing changes: all16 accepted p01 source files at digest `0995151ed93e67f2e5feee8092cdc08687580b79f1d988d40d5d072d5e39decf`; source archive/hash manifest in p01 logs. No other initial changes. No commits/push/deployment authorized.
- Prerequisite p01 accepted by root, B PASS CHS-P01-B1-R2, latest architecture D1/r3 PASS AG2-R1. No later phase implementation started.
- Authority `/Users/mac02/dev/openbot/plan/03-channel-handles-send/COORDINATION.md`, root writer, revision3 checked. I001–I003/r1,I004/r2,I005–I006/r1 and CHS-C001/r1 acknowledged. C001 constrains run-local disposition and sample/no-obligation behavior; publication/final intents remain later phases.
- Record root `/Users/mac02/dev/openbot/plan/03-channel-handles-send/phases/p02/docs`.
- Status: p02 accepted by root after B PASS CHS-P02-B1-R2; whole plan integration pending.

## Scope, ownership and checks

Add a pure terminal `channel.no_reply` with explicit typed business value, terminal-success route restrictions, preserved workflow-output validation, and trusted typed response disposition. Persist with committed completion and return/replay exact metadata; a child cannot suppress its parent. Existing explicit messages survive. No mutable intermediate run/channel switch or result-derived flags. Update shared registry/compiler identity, metadata/help/examples and all affected exhaustive consumers.

Trace shared `batch::validate_completion` and `batch_commit::complete` through pure/I/O/decisions/waits/children/recovery/samples and exact receipt readers. New persistence/semantic decisions require a narrow architecture gate before dependent edits. No durable final intent or publisher is introduced in p02.

- Focused new prefixes `channel_reply_terminal_contract_` and `channel_reply_terminal_`; list each and require intended nonzero counts.
- Relevant registry/compiler/catalogue, completion/pure batch, child/human/reply/recovery and sample regressions from traced interfaces; avoid unrelated/full suites.
- Fresh task-owned migrations for SQL/schema changes; SQLx prepare/check and offline compiler/lint, fmt/diff, graph refresh. Record exact commands/results/counts and unavailable checks. Confirmed unrelated p01 onboarding failures stay untouched and disclosed.
- Final combined gate remains targeted affected scope only, per explicit user instruction. p03–p07 follow accepted dependencies; plan04 follows accepted plan03.

## Resources and artifact

- Adopt existing root-owned sequential PostgreSQL18.6 cluster `/private/tmp/openbot-chs-xb34mvy9/pgdata`, PID85060, port65037; main/test `openbot_chs`/`openbot_chs_test`. Reset only these task-owned databases as schema changes; preserve baseline pair and all unrelated resources.
- Resource/command runner receipts in p01 logs remain authoritative; p02 will use phase-local check logs and explicit URLs. Primary target cache serialized. B's original sandbox cannot execute the nested test guard; independent source/evidence review continues, root can execute an exact reviewer-chosen diagnostic if needed.
- Source identity/exclusions: exact same9 workflow exclusions as p01, no deliverable/source exclusions. Current starting source is accepted p01; new review digest pending. Snapshot uses installed script plus p01 logs/tools realpath adapter.
- Next: inspect execution-owned completion metadata/receipt guards and registry pure-value contracts; record CHS-P02-D1/r1, obtain explicit B architecture PASS, implement whole phase, run focused checks and request one whole-phase review.

## CHS-P02-D1 / r1 — proposed architecture gate

1. **Trusted control and value.** Register `channel.no_reply` as a pure built-in with the existing data.map-style `with.value` binding and explicit declared output schema. It returns that business value unchanged. The shared compiler/runtime still validates the final value against the workflow's output schema. Require its success route to `$end` and no choice routes; ordinary error routing stays available. Reserved registry type, metadata/help/example, publication identity, catalogue revision and compiler semantic revision change together; no legacy decoding.
2. **Typed disposition.** Add a small domain `ResponseDisposition` enum (`automatic`, `suppressed`) to shared prepared/committed completion. Only successful terminal completion of the trusted compiled `channel.no_reply` step derives Suppressed; output keys, earlier messages and child metadata cannot select it. Other completions carry Automatic. A pure batch exposes terminal/replayed disposition through its result; fenced I/O/wait receipts preserve the exact committed execution disposition. No intermediate flag command or channel/run switch is introduced.
3. **Execution-owned persistence.** Store disposition on `workflow_executions` beside committed output/route. New executions default to Automatic; SQL admits Suppressed only together with completed success routing to `$end`, and completed metadata becomes immutable with output. Shared `batch_commit::complete` commits output, disposition and run progression atomically under existing ownership/deadline/transaction bounds. Do not create a separately mutable run-level status. A run's terminal disposition is the metadata of its existing terminal execution; later status views can join that identity. Change the sole init schema and use fresh owned databases.
4. **Replay and failure.** Pure, fenced I/O, durable wait/decision, child and recovery paths use the shared completion factory/writer. Update receipt readers/factories and the recovery final-error constructor explicitly. Replays read stored disposition, never infer it from current mutable configuration or output. Failed/cancelled/deadline/invalid-output paths remain failure, never suppressed success; rollback must roll back metadata too. Disposition on an ordinary/nonterminal execution remains Automatic; only terminal success exposes a run outcome.
5. **Run-local propagation.** Child/repeat result consumption returns only its business value to the parent; parent completion derives its own disposition from its own terminal step. Manual/scheduled/child/sample runs record a disposition but gain no channel response authority. p01 passive snapshots still confer none. Explicit messages/receipts remain intact. Sample pure dispatch recognizes the new pure node; it may carry typed local terminal metadata but cannot write production ownership/intent/delivery rows. Bounded authorized HTTP/status projection changes remain phase06, not an unguarded new reader here.
6. **Verification.** New `channel_reply_terminal_contract_` cases cover valid source/roundtrip, rejected successor/choice routes, declared and workflow output validation, normal-versus-forged flags, catalogue identity/example and business-value preservation. New `channel_reply_terminal_` database cases cover pure completion/replay/reconstruction, I/O predecessor, human/child continuation, separate parent/child dispositions, preserved explicit messages, immutable/invalid SQL metadata, rollback and real completion/cancellation competitors. Select existing registry/compiler/batch/completion/wait/child/recovery/sample interfaces from the traced callers, plus the p01 producer binding cases because publication identity changes. Fresh migration, SQLx prepare/check, current offline alltarget compile/lint, fmt/diff and graph checks; targeted only.

**Failure and integration invariants:** no business output is replaced with a control envelope; no failed run is mislabeled silent success; an empty/invalid business value follows existing failure/error routing; no child controls parent suppression; no completion succeeds with partially committed metadata. No automatic final decision/publisher exists until p03/p04. This phase changes shared semantic identity and resets disposable databases rather than supporting old bundles.

**Discovery:** `PureCompletion` is built in `batch::validate_completion` and recovery final-error handling; common consumers include pure advancement, I/O, decisions, waits, child completion and samples. `batch_commit::complete` owns progression, and IO/wait replay read execution-owned committed output/route. Pure batch has its own replay reader. Original execution fact/route guards are at init schema lines1227ff/1340ff. These owners, not post-commit callbacks, carry the metadata.

**Resources:** phase-local runner/resources in logs; same owned PG/serialized target. Starting p02 uses CARGO_INCREMENTAL=1 for local verification (p01 used0); durations will be recorded and savings not assumed. Test stack remains2MiB, no bound raised. Original baseline failure evidence retained; no unrelated fixes/full suite.

## CHS-P02-D1 / r2 — precise business-value validity

Preserve D1/r1 and its original immutable text. This r2 replaces the ambiguous empty/invalid phrase in r1 with missing or schema-invalid; all other decisions are unchanged. Exact revised decision: logs/CHS-P02-D1-r2.md.


**Business-value clarification:** missing bindings or values invalid under the declared/workflow schema fail through existing error handling. An explicitly bound empty string, null, empty object or array is valid when the business schema permits it. Suppressed completion still validates business output but skips automatic-response projection; the nonempty canonical message-body requirement applies only when a later automatic response is actually projected. No new blanket nonempty business-value rule.

## Architecture acceptance

Root checked B's saved CHS-P02-AG1-R1 PASS for D1/r2 and recomputed source digest0995151e (full identity above) before p02 edits. Coordinationr3/C001r1 remains current. Implementation proceeds under this decision; whole-phase review and focused checks remain pending.

## Whole-phase implementation and checks in progress

Implemented channel.no_reply using the shared typed value contract, terminal-success diagnostic, catalogue v1.7/compiler8 identity and pure worker dispatch. New ResponseDisposition domain enum flows through shared pure completion, fenced/wait receipts, recovery and execution-owned SQL result/immutability; parent/child/repeat decisions remain local. Sample uses the production pure dispatcher. p01 authority code is preserved; the only producer modification is the additive execution metadata schema.

New tests cover schema-valid empty/null output, runtime workflow-schema rejection/error routes, forged flags, replay/reconstruction, actual competing completions/cancellation, transaction rollback and immutable SQL facts, worker dispatch, human/IO continuations, parent/call/repeat isolation, explicit message preservation and isolated sample behavior. Phase-local author-p02-delta.diff and author-p02-paths.json compare against the accepted p01 archive rather than conflating the preserved prerequisite with p02 changes.

- author-fresh-migrate-01 and author-fresh-test-migrate-01: PASS fresh sole migration on both adopted disposable databases.
- author-clippy-01: PASS initial implementation, before final fixture additions.
- author-terminal-tests-01/02: compile failures in new fixture module imports (private sibling helpers/AdmissionFixture); corrected by owning the message fixture under message_tests and using its real publication/binding/admission helpers. No production visibility widened and assertions retained.
- author-clippy-02: unused test import rejected; removed.
- author-clippy-03: PASS current source, offline all-target Clippy with -Dwarnings; stock2MiB test stack, CARGO_INCREMENTAL1.
- fmt/diff and graph refresh: PASS; final logged commands and source digest pending checks.
- run-focused.py serializes explicit nonzero inventories and guarded runtime selections for the terminal feature, catalogue, publication-storage revision, sample, batch, IO/wait/decision/call/repeat/recovery and p01 producer binding. Its current first listing is compiling the test binary. SQLx prepare/check and whole-phase independent review pending.

No full suite or plan-level completion claimed. Earlier two clean-HEAD onboarding failures remain out of scope and untouched.

### Focused runtime corrections

- focused-terminal-tests-01:18PASS/1FAIL; the new immutability assertion exposed that UPDATE OF on the existing fact trigger omitted response_disposition. Added that column to the trigger's event list, preserving the assertion and D1's invariant. Both disposable databases reset/migrated again (author-fresh*-02 PASS).
- focused-terminal-tests-02: all19 new cases PASS, 0ignored, real PG and network guard/stock2MiB. Includes14 DB cases,4 compiler-contract cases and1 isolated sample case.
- focused-registry-tests-02:55PASS/1FAIL; existing exhaustive per-type dynamic-input test treated the new type as the wildcard wait/deadline family. Added channel.no_reply to the existing typed-value/data.map case so its schema-invalid dynamic value is actually checked; no skip or weakened assertion. Rerun registry and continue remaining focused caller selections. The new terminal feature's unchanged passed evidence is reused.

### Focused caller verification

All targeted selections now PASS: terminal19, registry56, publication-storage5, sample15, batch15, fencedcompletion14, waits20, decisions38, child calls9, repeat4, poison/recovery12 and producer bindings19. Inventories are nonzero and retained. These are226 selections/220 unique tests after overlaps,0ignored, all guarded runtime execution under stock2MiB. See logs/author-focused-checks.json. The first publication-storage inventory used the source filename rather than its actual storage::tests module; zero-count guard correctly stopped it. Corrected exact module inventory is5 and all5 PASS; no empty selection counted as verification.

Named the new multi-field SQL fixture records (Effects/Pending) in accordance with src/AGENTS, preserving the existing assertions; rechecking those two affected terminal cases in final-terminal-* logs. No other production behavior changed after the full passing feature selection. Final SQLx prepare/check, current offline Clippy, fmt/diff/graph are executing serially before source freeze.

Final-terminal-business/invalid tests both PASS after naming SQL fixture records. SQLx prepare and prepare --check PASS against the freshly migrated schema; regenerated .sqlx bytes are unchanged. Current-source final offline Clippy/fmt/diff/graph are the remaining author checks; source freeze then B whole-phase review. Context telemetry unexposed; durable root checkpoint maintained. Graft reported about265,685 tokens saved since this continuation (no dollar estimate supplied).

## CHS-P02-B1-R1 whole-phase review freeze

All final author checks PASS: both affected fixture rechecks, SQLx prepare/check (no metadata byte changes), offline all-target Clippy -Dwarnings, fmt, diff check, graph refresh. Fresh migrations PASS. Author-focused-checks.json records220 distinct affected-scope tests/226 selections and0ignored, not a full suite. Original failed checks are retained and corrected above; earlier unrelated baseline onboarding failures remain untouched.

Frozen cumulative digest `96e1d8ff39c45d5668d737f458897304b3d26b067e84f5c5fdc2411447d9add0`, base/start/HEAD `769a23aa6ef423e964f8221a8ee6b0743325c6ba`, same exact9 exclusions from the immutable manifest. 48 cumulative source files; 33 p02 files relative to accepted p01. Exact source archive, content hashes, p02 diff and coordinationr3 pin saved under CHS-P02-B1-R1-* logs. No implementation edits while B reviews. p02 implemented and author-verified; independent review/acceptance and plan integration pending.

### Independent review diagnostic (source still frozen)

B identified actions::failure::effect_free_kind as an omitted pure-type consumer and asked root to execute its exact existing exhaustive regression. reviewer-failure-kind-list-01 selected1; guarded reviewer-failure-kind-tests-01 failed at failure.rs311 for channel.no_reply (false vs expectedtrue),0ignored. This is a p02 regression, not one of the inherited onboarding failures. B also identified an undercharged borrowed-preflight schema-copy count in registry/budget.rs for the new data.map-style contract; later charging remains bounded. Both are implementation omissions under D1/r2; wait for all saved findings and final review identity before unfreezing and correcting together.

## Combined F1/F2 correction (architecture unchanged)

B saved complete review CHS-P02-B1-R1 with two actionable findings and final identity check; root read both and released source freeze only after that final result. R1 manifest/archive/diff remain immutable. F1: add no_reply to effect_free_kind and check pending-exhaustion behavior plus the exhaustive classifier. F2: include its third schema copy in borrowed preflight and check a discriminating boundary before construction. No resource bound raised and no architecture amendment required.

Author added a real competing-claimant no_reply exhausted-pending regression and a focused borrowed-preflight budget boundary regression. The initial budget fixture exceeded the unchanged256KiB wire-source cap (correction-budget-before-01); redesigned it to reuse external tool-schema facts through16 compact tool steps, keeping the actual source under the cap while filling the same budget. Positive and data.map controls retained. correction-budget-before-02 and correction-exhaustion-before-01 are running against unchanged production classifiers before the combined fixes, to establish intended failures rather than accepting an unrelated fixture rejection. No production correction yet.

Before-fix reproduction complete: correction-budget-before-03 fails specifically because no_reply preflight incorrectly returnsOk after the data.map control rejects; correction-exhaustion-before-02 fails with actual durable run state waiting rather than failed under two real retirement claimants. Earlier -02/-01 attempts had a missing test trait import; corrected to WorkflowLeaseRecovery without changing assertions and retained logs. Both production matcher omissions now fixed together. Executing21 terminal cases, the whole existing failure-classification module, all registry cases and poison/recovery selections, then current offline Clippy/fmt/diff/graph. No SQL, query macro, migration or metadata changes in this correction, so earlier fresh-migration/SQLx evidence is unchanged and reused.

## CHS-P02-B1-R2 combined correction freeze

Both regressions reproduced before the production fixes, then all correction selections PASS:21 terminal,4 failure-classification,57 registry,12 poison/recovery cases (94 selections,89 distinct). Current offline all-target Clippy -Dwarnings, fmt/diff/graph PASS. Final combined selected scope is226 distinct tests,0ignored, with unchanged prior evidence reused; not a full suite. See CHS-P02-B1-R2-author-checks.json and author-correction-checks.json. SQL/schema/query bytes are unchanged from R1, so fresh migrations and SQLx prepare/check remain applicable. No bound raised; the new boundary test keeps both a data.map rejection control and a below-limit no_reply control.

Frozen corrected digest `aa7cd3e8ddd572c6dd7a551cc69a8119769b09b8ace32b567744f4d1fc11ed5e`, same base/HEAD and9 exclusions. Four correction files (two matchers/two regression files) are listed and diffed in CHS-P02-B1-R1-to-R2.diff; all50 cumulative source hashes/archive and35 p02 changes pinned in R2 manifest. D1/r2, coordinationr3/C001r1 unchanged. B must close F1/F2 and PASS exactR2 before root acceptance/p03.

### R2 verification-record correction

B confirms both F1/F2 code fixes and89 distinct correction tests resolve the findings, but correctly notes that the new exhaustion regression adds a dynamic SQL UPDATE. Earlier claims that all query bytes were unchanged were too broad: production SQL/schema/query macros are unchanged, while the test query is new. Per src/AGENTS, run fresh current-source SQLx prepare/check and verify no metadata/digest changes; no fresh migration is needed. Those checks are executing before B final PASS. Source remains frozen at R2.

## Root phase acceptance

Root verified B PASS CHS-P02-B1-R2, all50 source hashes, unchanged HEAD and coordinationr3, fresh current SQLx prepare/check with no metadata diff, and all final correction check receipts. Accepted exact aa7cd3e8ddd572c6dd7a551cc69a8119769b09b8ace32b567744f4d1fc11ed5e, D1/r2. See logs/CHS-P02-root-acceptance.json and R2 verification addendum; original overly broad query-reuse wording is superseded. All226 distinct selected tests pass with0ignored; not a full suite. No remaining p02 finding. Continue p03 under new coordination notice; p03–p07 and plan04 still required.
