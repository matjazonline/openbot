# Plan coordination: cloudroom-local

- Authority: `/Users/mac03/dev/openbot/plan/cloudroom-local/COORDINATION.md`
- Writer: `/root`, main worktree `/Users/mac03/dev/openbot`
- Revision: 11
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
| p03 | /root (author) | /Users/mac03/dev/openbot | phases/p03/docs/{TASK,HANDOFF}.md |

## Cross-phase notices

### CL-C001 / r1
- State: active
- Origin: p01, /root, CL-P01-D01/r1 (phases/p01/docs/logs/CL-P01-D01-r1.md)
- Targets: p01, p04, p06, p07
- Instruction/change: identify integration Cloudroom builds by the fixed source base plus the reviewed local-only optional-onboarding patch; protected local APIs and hosted cloud entitlements retain authentication.
- Dependencies/order: CL-P01-D01/r1 passed; effective before patch application/build-helper edits in p01 and every later integration source build.
- Evidence/review: CL-P01-D01/r1, PASS in reviewer note CL-P01-D01-R1; exact source/patch hashes retained in phase decision.
- Verification: source guards, native permission/UI recovery without hosted account, bounded reproducible source build.
- Responses: p01 -> satisfied active r1, independent acceptance CL-P01-B1-R3; exact source export and evidence in p01 TASK. p04/p06/p07 must acknowledge when started.


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
- Responses: p02 -> satisfied active r2, accepted digest ee02338b474bb7b234a8a8978669f952df0b36aabf125d191ae1bb2fc547aad7, PASS CL-P02-B1-R2; real SQL and forged matching-digest/leap proof retained. p03 -> acknowledged r2 in p03 TASK; preserve exact deadline wire bytes. Future targets must acknowledge at startup.


### CL-C003 / r1
- State: active
- Origin: p03 /root, CL-P03-D01/r2 (phases/p03/docs/logs/CL-P03-D01-r2.md)
- Targets: p03, p04, p05, p06, p07
- Instruction/change: current authenticated connection identity lives outside immutable journal-payload deduplication. Every operational envelope is fenced before inbox lookup/ACK; reconnect replay rewraps the unchanged canonical typed observation payload under the new identity. Transport chunk hashes cover current envelope bytes only. Bounded chunk assembly is unacknowledged until the full logical observation/output commits; no partial chunk acknowledgment.
- Dependencies/order: D01/r2 architecture gate PASS CL-P03-D01-R2; applies before p03 dependent edits and downstream protocol integration. Preserve accepted p02 operation/deadline digest under C002/r2.
- Evidence/review: D01-F1 resolved at gate, source unchanged from accepted p02; decision and reviewer records in p03 docs. Runtime proof pending B1.
- Verification: lost-ACK N→N+1 replay preserves original receipt/state; stale N is rejected before dedup; changed content conflicts; bounded complete assembly commits before ACK.
- Responses: p03 -> acknowledged active r1, implementation/proof pending; p04/p05/p06/p07 acknowledge at startup.


### CL-C004 / r1
- State: active
- Origin: p03 /root, B1 capacity/late-evidence audit in phases/p03/docs/TASK.md
- Targets: p03, p04, p05, p06, p07
- Instruction/change: unknown native outcomes remain unsettled and consume effect capacity until matching native completed/failed/stopped evidence is accepted; missing is not terminal proof. Count accepted control work toward the bounded outstanding queue until its wait settles. Preserve the expired server wait outcome while accepting late native audit evidence; positive known native terminal state may free physical capacity without reviving any workflow. Disabled machines permit explicitly authorized existing-thread controls, while refusing new effects.
- Dependencies/order: implements original separate-state/capacity/disable invariants under D01/r2; effective before dependent client/workflow/retention code. p03 B1 review/proof and acceptance remain required before p04 starts.
- Evidence/review: shared SQL capacity helper with six-command/outcome/native-state equivalence matrix, actual uncertain-to-positive-native path and late-completion test; final execution/review pending.
- Verification: eight uncertain effects block a ninth; explicit controls retain headroom; known matching terminal evidence frees a slot; late output cannot settle an expired workflow; accepted controls cannot create an unbounded backlog.
- Responses: p03 -> acknowledged active r1, proof/review pending; downstream phases acknowledge at startup.
