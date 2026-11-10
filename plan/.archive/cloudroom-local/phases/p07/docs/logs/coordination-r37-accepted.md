# Plan coordination: cloudroom-local

- Authority: `/Users/mac03/dev/openbot/plan/cloudroom-local/COORDINATION.md`
- Writer: `/root`, main worktree `/Users/mac03/dev/openbot`
- Revision: 37
- Workflow record, not an additional implementation plan.

## Plan-wide instructions

- CL-I001/r1: Execute groups p01–p07 in dependency order from the original numbered plan files. Accept each group before dependent implementation. No commits, deployment, production resets, or provider inference outside disposable authorized projects.
- CL-I002/r1: Preserve pre-existing deletions in `.agents/skills/implement-astra-reviewed-2-2/`. All work is serialized in this checkout; no concurrent source writers.
- CL-I003/r1: Follow repository reset policy; no backward compatibility. Source inspection and fixtures never establish real-provider/platform acceptance.
- CL-I004/r1: Focused checks per group; repository full verification once at final integration. Record unavailable proof without claiming completion.

- CL-I005/r1: User explicitly chose “Leave Linux acceptance unverified” on 2026-11-07. Complete implementation and other required verification; report native headless Linux/platform acceptance as NOT EXERCISED. This does not waive Rust/database/plugin/browser checks or macOS proof.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | /root (accepted in phase) | /Users/mac03/dev/openbot | phases/p01/docs/{TASK,HANDOFF}.md |
| p02 | /root (accepted in phase) | /Users/mac03/dev/openbot | phases/p02/docs/{TASK,HANDOFF}.md |
| p03 | /root (accepted in phase) | /Users/mac03/dev/openbot | phases/p03/docs/{TASK,HANDOFF}.md |
| p04 | /root (accepted in phase) | /Users/mac03/dev/openbot | phases/p04/docs/{TASK,HANDOFF}.md |

| p05 | /root (accepted in phase) | /Users/mac03/dev/openbot | phases/p05/docs/{TASK,HANDOFF}.md |

## Cross-phase notices

### CL-C001 / r1
- State: superseded
- Origin: p01, /root, CL-P01-D01/r1 (phases/p01/docs/logs/CL-P01-D01-r1.md)
- Targets: p01, p04, p06, p07
- Instruction/change: identify integration Cloudroom builds by the fixed source base plus the reviewed local-only optional-onboarding patch; protected local APIs and hosted cloud entitlements retain authentication.
- Dependencies/order: CL-P01-D01/r1 passed; effective before patch application/build-helper edits in p01 and every later integration source build.
- Evidence/review: CL-P01-D01/r1, PASS in reviewer note CL-P01-D01-R1; exact source/patch hashes retained in phase decision.
- Verification: source guards, native permission/UI recovery without hosted account, bounded reproducible source build.
- Responses: p01 -> satisfied active r1, independent acceptance CL-P01-B1-R3; exact source export and evidence in p01 TASK. p04 -> acknowledged active r1 in p04 TASK; p06/p07 must acknowledge when started.


### CL-C002 / r1
- State: superseded
- Origin: p02, /root, CL-P02-D02/r1 (phases/p02/docs/logs/CL-P02-D02-r1.md)
- Targets: p02, p03, p04, p05, p07
- Instruction/change: freeze/hash operation deadlines at UTC microsecond resolution; peer hash verification preserves the server's exact canonical deadline string rather than round-tripping through JavaScript Date. Persisted operations must recover with the original immutable digest.
- Dependencies/order: proposed precision correction to accepted p01 types; requires CL-P02-D02/r1 pass before implementation and affected tests/review in p02.
- Evidence/review: PostgreSQL precision repro and production hash/decoder source in p02 decision; independent gate pending.
- Verification: decisive real Postgres operation/hash round trip, typed normalization and refusal of forged nonrepresentable precision; later Rust/Node wire equivalence.
- Responses: p02 -> acknowledged proposed r1; dependent edits await gate. Later targets must acknowledge if activated.

### CL-C002 / r2
- State: active
- Origin: p02, /root, CL-P02-D02/r2 (phases/p02/docs/logs/CL-P02-D02-r2.md), resolves CL-P02-D02-F1
- Targets: p02, p03, p04, p05, p07
- Instruction/change: reject Chrono leap-second deadlines, then freeze/hash operation deadlines at UTC microsecond resolution. Frozen records must already be canonical; peers preserve the exact server deadline string rather than reconstructing through JavaScript Date.
- Dependencies/order: supersedes proposed C002/r1; D02/r2 must pass before dependent production edits. Affected p01 semantics are reviewed in p02; historical p01 pass remains unchanged.
- Evidence/review: real row round-trip FAIL with digest mismatch in p02 logs/deadline-roundtrip-before.txt; SQL precision repro and source/SQLx leap-second evidence from D02-R1. Independent revised gate PASS CL-P02-D02-R2; D02-F1 resolved at gate.
- Verification: actual Postgres digest round trip after correction, constructor normalization/leap rejection and forged matching-digest record refusal; later Rust/Node wire equivalence.
- Responses: p02 -> satisfied active r2, accepted digest ee02338b474bb7b234a8a8978669f952df0b36aabf125d191ae1bb2fc547aad7, PASS CL-P02-B1-R2; real SQL and forged matching-digest/leap proof retained. p03 -> satisfied r2 at accepted1e4f5b2148970677a7f1c388fa5a38cc52b1e7685061a4fac15857ea6b1325d9/PASS CL-P03-B1-R2; actual Rust→Node6-kind microsecond proof retained. p04 -> acknowledged active r2 in p04 TASK; future targets must acknowledge at startup.


### CL-C003 / r1
- State: active
- Origin: p03 /root, CL-P03-D01/r2 (phases/p03/docs/logs/CL-P03-D01-r2.md)
- Targets: p03, p04, p05, p06, p07
- Instruction/change: current authenticated connection identity lives outside immutable journal-payload deduplication. Every operational envelope is fenced before inbox lookup/ACK; reconnect replay rewraps the unchanged canonical typed observation payload under the new identity. Transport chunk hashes cover current envelope bytes only. Bounded chunk assembly is unacknowledged until the full logical observation/output commits; no partial chunk acknowledgment.
- Dependencies/order: D01/r2 architecture gate PASS CL-P03-D01-R2; applies before p03 dependent edits and downstream protocol integration. Preserve accepted p02 operation/deadline digest under C002/r2.
- Evidence/review: D01-F1 resolved at gate, source unchanged from accepted p02; decision and reviewer records in p03 docs. Runtime proof pending B1.
- Verification: lost-ACK N→N+1 replay preserves original receipt/state; stale N is rejected before dedup; changed content conflicts; bounded complete assembly commits before ACK.
- Responses: p03 -> satisfied active r1 at accepted1e4f5b2148970677a7f1c388fa5a38cc52b1e7685061a4fac15857ea6b1325d9/PASS CL-P03-B1-R2, current-fence lost-ACK replay proof retained; p04 -> acknowledged active r1 in p04 TASK; p05/p06/p07 acknowledge at startup.


### CL-C004 / r1
- State: active
- Origin: p03 /root, B1 capacity/late-evidence audit in phases/p03/docs/TASK.md
- Targets: p03, p04, p05, p06, p07
- Instruction/change: unknown native outcomes remain unsettled and consume effect capacity until matching native completed/failed/stopped evidence is accepted; missing is not terminal proof. Count accepted control work toward the bounded outstanding queue until its wait settles. Preserve the expired server wait outcome while accepting late native audit evidence; positive known native terminal state may free physical capacity without reviving any workflow. Disabled machines permit explicitly authorized existing-thread controls, while refusing new effects.
- Dependencies/order: implements original separate-state/capacity/disable invariants under D01/r2; effective before dependent client/workflow/retention code. p03 B1 review/proof and acceptance remain required before p04 starts.
- Evidence/review: shared SQL capacity helper with six-command/outcome/native-state equivalence matrix, actual uncertain-to-positive-native path and late-completion test; final execution/review pending.
- Verification: eight uncertain effects block a ninth; explicit controls retain headroom; known matching terminal evidence frees a slot; late output cannot settle an expired workflow; accepted controls cannot create an unbounded backlog.
- Responses: p03 -> satisfied active r1 at accepted1e4f5b2148970677a7f1c388fa5a38cc52b1e7685061a4fac15857ea6b1325d9/PASS CL-P03-B1-R2; SQL matrix, uncertain reconnect and late terminal proof retained; p04 -> acknowledged active r1 in p04 TASK; downstream phases acknowledge at startup.


### CL-C001 / r2
- State: active
- Origin: p04 /root, combined D01/r1 architecture proposal
- Targets: p04, p06, p07
- Instruction/change: integration Cloudroom build is the same fixed source plus accepted local-onboarding patch AND the narrowly reviewed public SDK mutation AbortSignal forwarding patch. SDK spawn/send/stop/archive(archiveAll alias) must abort the actual HTTP wait during plugin shutdown; cancellation is not proof native work stopped or was not accepted. Keep SDK version0.6.50 with explicit source+patch artifact identity; no fictional registry release.
- Dependencies/order: D01/r2 gate PASS CL-P04-D01-R2; active before SDK patch/source guard changes. Supersedes C001/r1 for later integration builds; historical p01 acceptance remains unchanged.
- Evidence/review: proposed patch phases/p04/docs/logs/CL-P04-D01-sdk-abort.patch and identity, source unchanged at gate PASS CL-P04-D01-R2; original SDK mutators omit signal while default request timeout75s exceeds plugin service stop5s.
- Verification: actual SDK held-fetch cancellation for all4 operations and archiveAll equivalent, no signal serialized in body, plugin service joins within drain, bounded reproducible SDK runtime/declarations and source/patch guards.
- Responses: p04 -> satisfied r2 at accepted 512b6e90ecf87aad7f0dc140e0afeb45532d67e2b4fedf5c15952685342cabda/PASS CL-P04-B1-R2; actual source-built SDK held HTTP abort proof and service drain retained. p06/p07 acknowledge at startup.

### CL-C005 / r1
- State: active
- Origin: p04 /root, combined D01/r1 architecture proposal
- Targets: p04, p05, p06, p07
- Instruction/change: add closed Submission evidence with known Cloudroom thread and accepted/queued/missing state before a native turn ID is known. It binds the existing session only after local intent, cannot settle an effect or release physical capacity, cannot replace a known native turn, and missing stays uncertain. Keep matching-turn terminal completion, exact current fencing and canonical journal payload unchanged.
- Dependencies/order: original phase04 public SDK returns known queued thread before turn acceptance; D01/r2 gate PASS CL-P04-D01-R2 before extending initial unreleased v1 wire/server decoder. No compatibility decoder.
- Evidence/review: public SendMessageResponse has sent/queued with queue entry but no native turn ID; spawn returns thread ID/status. Accepted server DispatchEvidence already permits NativeIdentity turn_id=None but LocalEvidence has no producer for this condition. Accepted decision D01/r2/PASS CL-P04-D01-R2, actual implementation/proof pending; p03 historical acceptance unchanged.
- Verification: real PG+WS accepted/queued thread mapping with no successful outcome, explicit control can target known mapped thread, forged foreign thread/no intent/turn downgrade refusals, late submission remains audit, known matching turn later settles once.
- Responses: p04 -> satisfied r1 at accepted 512b6e90ecf87aad7f0dc140e0afeb45532d67e2b4fedf5c15952685342cabda/PASS CL-P04-B1-R2; realPG/WS Submission proof and production plugin native identity fixtures retained. p05/p06/p07 acknowledge at startup.


## Phase acknowledgments at revision17

- p05: /root checked authoritative revision16 at startup and acknowledges I001–005/r1,C002/r2,C003/r1,C004/r1,C005/r1. C001/r2 targets other phases; accepted p04 SDK artifact is a prerequisite, no SDK change in p05. TASK/HANDOFF p05 carry contract impact/proof mapping. No new or changed notice; dependent edits await p05 D01 architecture PASS.


### CL-C006 / r1
- State: superseded
- Origin: p05 /root, CL-P05-D01/r1 (phases/p05/docs/logs/CL-P05-D01-r1.md)
- Targets: p05,p06,p07
- Instruction/change: workflow local effects use shared frozen action/approval/budgets and one durable execution-scoped call record. Authorized operation+effect wait+attempt closure commit atomically; outcome/expiry commits durable wake readiness with server evidence. A current workflow claim alone accepts bounded result. New deferred local_agent marker is distinct from immediately committed SQL-local effects; cancellation/late evidence cannot revive a run. Retention must preserve prepared/active/unresolved call, wait, action and native operation links.
- Dependencies/order: D01/r1 architecture PASS CL-P05-D01-R1 before dependent schema/approval/parking/wake edits. Existing p04 artifact unchanged.
- Evidence/review: D01/r1 SHA4a5173ccf88cc6725553c98e895a14097559c5348d71424cad4812b3602abfe4, independent PASS CL-P05-D01-R1; actual production consumer review retained in p05 records. Runtime proof pending B1.
- Verification: competing claims/atomic park+lostACK/currentfence, protected approval and live revocation, offline expiry and duplicate/late wake, samples/publication boundary; p06 retained-link races; p07 combined native/worker acceptance.
- Responses: p05 -> acknowledged active r1 in TASK after D01/r1 PASS, B1 implementation begins. p06/p07 acknowledge at startup.


## Backend checkpoint at revision20

- p05: root checked r19 on resume; now r20 status update only, notices unchanged. B1 backend core implemented with partial focused proof; management/API/HTML and failure-path work remain. No B1 review/acceptance. I001–005/r1,C002/r2,C003/r1,C004/r1,C005/r1,C006/r1 acknowledged. See p05 TASK/HANDOFF; p06/p07 remain unstarted.


### CL-C006 / r2
- State: active
- Origin: p05 /root, CL-P05-D01/r2 (phases/p05/docs/logs/CL-P05-D01-r2.md)
- Targets: p05,p06,p07
- Instruction/change: preserve C006/r1 plus immutable server-owned Direct/Workflow native operation origin. Direct explicit session follow-up has current manager/original-association authority and no workflow wake link; Workflow always requires its exact shared-action/call/park proof. Missing calls never downgrade origin.
- Dependencies/order: D01/r2 independent gate required before origin/schema/direct-workflow-send dependent edits. C006/r1 remains active until superseded on pass.
- Evidence/review: original05A follow-up requirement and commands.authorize_session branch; immutable decision SHA 6c3c67bf78cb1cdf288f2ab271d24691d29fece1ceb2d71e923064ef2c40618d. Independent architecture PASS CL-P05-D01-R2 for exact SHA; runtime/B1 review pending.
- Verification: direct follow-up after original run terminal without revival; origin immutable/composite FK/atomic call proof; private/foreign reads and controls; live withdrawal races; later retention and combined native acceptance.
- Responses: p05 -> acknowledged active r2 after gate PASS; begin dependent implementation; p06/p07 acknowledge at startup.


## Phase05 B1 review checkpoint at revision23

- p05: implementation and focused verification complete; source freeze and whole B1 review next. Active I001–005/r1,C002/r2,C003/r1,C004/r1,C005/r1,C006/r2 acknowledged; no contract revision. 103 focused tests plus303 affected regressions and28 registry checks pass; browser/forms and current SQLx/offline Clippy pass. Full integration remains p07. p06/p07 remain unstarted.


## Phase05 B1 corrections at revision24

- p05 whole B1 R1 returned CHANGES REQUIRED, F1–F5. No phase acceptance; source unfrozen for root corrections only. Existing notices/design unchanged. Correct actual fair-poll readiness, preserve first workflow outcome independently of late native audit, reuse physical-expiry classification, retain start association across profile paging, and prove continuation/current delivery authority. p06/p07 remain unstarted.


## Phase05 correction recheck at revision25

- p05: all F1–F5 corrections implemented with decisive production-path coverage,112 local-agent assertions PASS/one explicitly generated browser fixture,20 fair-poll,113 wait and303 shared message assertions PASS. Review freeze/recheck R2 pending final static gate. Existing active notices unchanged; no new decision. p06/p07 remain unstarted. Root acknowledges I001–005/r1,C002/r2,C003/r1,C004/r1,C005/r1,C006/r2.


## Phase05 acceptance / phase06 startup at revision26

- p05 accepted at c7dfb6034c5f3f5001e107ac94a008da7fc55b88054af8208cfa1b79a6685664, independent PASS CL-P05-B1-R2, F1–F5 resolved; identity/evidence/authority revalidated.
- p06 now authorized in dependency order, root author/same serialized checkout. Applicable I001–005/r1,C001/r2,C003/r1,C004/r1,C005/r1,C006/r2 acknowledged; C002/r2 remains inherited immutable protocol prerequisite. Architecture preparation before dependent retention/schema edits. p07 remains unstarted.

| p06 | /root (accepted in phase) | /Users/mac03/dev/openbot | phases/p06/docs/{TASK,HANDOFF}.md |


### CL-C007 / r1
- State: superseded
- Origin: p06 /root,CL-P06-D01/r1 SHA137e653cbf2e4691855dab793d2a4298ac31e1683ada0b79cc92012371c7d1d2
- Targets: p06,p07 (affected p05 integration contracts reviewed in p06)
- Instruction/change: preserve physical settlement separately from wait disposition; bounded90-day native summaries/7-day settled observation detail and coordinated workflow retention; finite UUIDv7 manual-request admission horizon after exact saved replay; plugin ACK/terminal compaction and supervised packaging.
- Dependencies/order: independent D01/r1 gate before dependent schema/replay/retention/source edits. Prior p05 historical acceptance unchanged.
- Evidence/review: proposed decision phases/p06/docs/logs/CL-P06-D01-r1.md; no source edits.
- Verification: decisive competing claimants/reference races, current-fence compacted replay, post-GC stale-request refusal, plugin pressure and packaged lifecycle. Linux native acceptance remains I005 NOT EXERCISED.
- Responses: p06 acknowledges proposal,awaits gate; p07 at startup.


### CL-C007 / r2
- State: superseded
- Origin: p06 /root,CL-P06-D01/r2 SHA80af23ff756f56440f9952ee7e69d757d337e5ecf8f18210597d156266f6215a;resolves D01-F1/F2 at design
- Targets: p06,p07 (affected p05 integration contracts reviewed in p06)
- Instruction/change: r1 physical settlement/replay/retention policy plus bounded guarded terminal call archival for cyclic references and current-fence/nonce server-absence confirmation before plugin mapping deletion. No native command/authority added; no old session resurrection.
- Dependencies/order: independent D01/r2 gate before dependent schema/replay/retention/wire/source edits. No source edits since accepted p05.
- Evidence/review: phases/p06/docs/logs/CL-P06-D01-r2.md; R1 findings retained.
- Verification: old prepared/queued server references preserve plugin mapping across90days; true retirement frees mapping after authenticated absence; two terminal reciprocal roots make bounded progress, active/unresolved edge protected; all earlier r1 proof obligations retained.
- Responses: p06 acknowledges proposedr2,awaits gate; p07 at startup.


### CL-C007 / r3
- State: active
- Origin: p06 /root,CL-P06-D01/r3 SHA89345a33f64e528eefbf70c3877902f6e09921314fbe1bab262f7cb4a10c1e41;author restore-boundary audit
- Targets: p06,p07
- Instruction/change: C007/r2 plus explicit coordinated SQL/profile/journal restore or new installation/re-pair after SQL-only rollback; matching observation cursors alone do not prove metadata/mapping history agreement.
- Dependencies/order: independent D01/r3 gate before dependent edits;source still acceptedp05.
- Evidence/review: phases/p06/docs/logs/CL-P06-D01-r3.md;historicalr1/r2 retained.
- Verification: all r2 proof plus restore of pre-retirement SQL cannot silently reuse newer plugin mapping history;authorized same-point restore retains known mapping,otherwise reset/re-pair.
- Responses: p06 acknowledges proposedr3,awaits gate;p07 at startup.


## Phase06 implementation at revision30

- D01/r3 independent PASS CL-P06-D01-R3 exactSHA89345a33f64e528eefbf70c3877902f6e09921314fbe1bab262f7cb4a10c1e41;F1/F2 resolved at design. ActivateC007/r3;root acknowledges/satisfies gate ordering. Source unfreezes for root-only p06 implementation. All earlier notices unchanged. Runtime and platform acceptance still required;LinuxI005 remainsNOTEXERCISED.


## Phase06 implementation/verification at revision31

- Root resumed the authorized task after the handoff; no user pause/cancellation. p06 B1 now implements backend retention/status/replay, journal compaction/pressure/current-fence mapping retirement, reproducible packaging and one-profile services/docs. p06 remains unaccepted until its focused evidence and whole-batch independent review complete; p07 remains unstarted.
- Root acknowledges active I001–005/r1,C001/r2,C002/r2(inherited),C003/r1,C004/r1,C005/r1,C006/r2,C007/r3. No instruction/contract revision. C007/r3 implementation is undergoing decisive verification.
- Packaged macOS public-CLI install/reload/remove and graceful restart PASS; native task/active-service-shutdown proof is running in a new disposable project under existing user authorization. Linux remains NOT EXERCISED.


## Phase06 B1 review corrections at revision32

- CL-P06-B1-R1 CHANGES REQUIRED at2fdbad30cd3ba21436662b82dfd114ad519e75f52736b322a7b5d51db4d393fb. F1/P2 requires bounded advancing mapping-retirement traversal so a retained oldest page cannot starve later permanently absent mappings. Root unfreezes source for this correction and decisive regressions; existing D01/r3/C007/r3 contract is unchanged. CI typecheck/tests C1 remains explicit p07 work. All notices acknowledged; p06 not accepted and p07 not started.


## Phase06 acceptance / phase07 startup at revision33

- p06 accepted atc52a03aec4aaa4fbc55afcf807c77e8814aa4eedffa05f3d727b3217723d3def, PASS CL-P06-B1-R2; F1 resolved. C1 plugin CI typecheck/build/tests remains requiredp07. Linux NOT EXERCISED waiver unchanged.
- p07 now begins in dependency order, root author in serialized checkout. Applicable I001–005/r1,C001/r2,C002/r2,C003/r1,C004/r1,C005/r1,C006/r2,C007/r3 acknowledged. No shared-contract change proposed; integration/failure tests, actual native matrix and final full-suite gate remain required.

| p07 | /root (accepted and integrated in working tree) | /Users/mac03/dev/openbot | phases/p07/docs/{TASK,HANDOFF}.md |


### CL-C008 / r1
- State: active
- Origin: p07 /root, CL-P07-D01/r1 (phases/p07/docs/logs/CL-P07-D01-r1.md)
- Targets: p07; combined integration of p04–p06
- Instruction/change: sparse native SDK event cursors must not be confused with contiguous Openbot observation acknowledgments; exact positive request/turn binding remains required. Completed item snapshots own terminal text; numeric gaps conservatively mark truncation.
- Dependencies/order: D01/r1 independent architecture gate PASS CL-P07-D01-R1; active before dependent edits; prior accepted artifacts remain historical.
- Evidence/review: native Codex timing-dependent pass/failure and OpenCode correlationConflict, pinned upstream resolved-delta pruning source.
- Verification: sparse replay/missing-binding/foreign-turn/partial-output tests; package/native/provider and final regression gates.
- Responses: p07 acknowledges active r1; implementation and verification in p07 TASK.


## p07 correction checkpoint at revision36
Root acknowledges unchanged active notices including C008/r1. B1/R1 source02c3… requested3verification corrections, p07notaccepted. Old final suite retained3308PASS/3FAIL/13ignored; final verification resumes after cohesive corrections. No contract change, no new permissions requested.


## Phase07 and combined acceptance at revision37

- Root accepts p07 and the combined p01–p07 implementation in the serialized main working tree at artifact **bd92212d24d49b9d937eac2c52723eae45085cdb81abc36d12c262dcdf33cbe9**, base/HEAD97e5d60dc538e26c7cb976a8d63772eed80baff2. Independent **PASS CL-P07-B1-R2**; F1–F3 fixed, no remaining actionable current-acceptance findings. All original active instructions/contracts remain unchanged; this revision records status only.
- Current immutable final-v2 evidence validates65PASS/0FAIL/2NOTEXERCISED. Full regular3319, full stock2MiB3314, finalMacnative3providers, Node90+4guards, migrations/offline/Clippy/SQLx, lease/browser/static/package gates passed. LinuxARM64 production image and memory/integrity/offline/non-root checks passed, including B independent recheck. Native Linux provider acceptance remains explicitly user-waived/unverified perI005/r1.
- Historical evidence loss remains explicit: original R1 installation stdout overwritten/unrecoverable, no reconstruction or aggregate historical-validationPASS. Fresh uniquely named current installation proof passes. Earlier incompletely diagnosed native failures remain retained and are not declared unrelated; current final matrix passes. Detailed dispositions in p07 REVIEW/notes/findings/TASK.
- All phases are integrated in the current working tree; no commit, merge, deployment or production reset performed. Preserve pre-existing skill deletions and unmerged work. Root performs only owned disposable verification-resource cleanup after acceptance.
