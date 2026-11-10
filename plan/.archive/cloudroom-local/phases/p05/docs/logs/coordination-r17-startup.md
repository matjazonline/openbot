# Plan coordination: cloudroom-local

- Authority: `/Users/mac03/dev/openbot/plan/cloudroom-local/COORDINATION.md`
- Writer: `/root`, main worktree `/Users/mac03/dev/openbot`
- Revision: 17
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

| p05 | /root (author, architecture preparation) | /Users/mac03/dev/openbot | phases/p05/docs/{TASK,HANDOFF}.md |

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
