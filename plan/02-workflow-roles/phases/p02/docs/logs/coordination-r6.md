# Plan coordination: 02-workflow-roles

- Authority: /Users/mac02/.codex/worktrees/2050/openbot/plan/02-workflow-roles/COORDINATION.md
- Writer: `/root` in `/Users/mac02/.codex/worktrees/2050/openbot`
- Revision: 6

## Plan-wide instructions

### I001 / r1
No backward compatibility; edit the init schema and use fresh disposable databases.

### I002 / r1
Root/A is Sol 6.1/xhigh, confirmed by the user earlier in this session. Independent reviewer B is Astra/high with no inherited conversation. Execute phases sequentially, accepting each before dependent work. Existing authorization covers implementation; no new publishing or deployment request is inferred.

### I003 / r1
Run focused checks in implementation phases and the full repository/CI gate once at this plan's final integration phase. Retain failed checks. Do not raise limits or weaken assertions.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | /root; accepted in phase; integration pending p05 | /Users/mac02/.codex/worktrees/2050/openbot | phases/p01/docs/TASK.md; HANDOFF.md |
| p02 | /root; B1 implemented in part and verifying; relay contract unresolved | /Users/mac02/.codex/worktrees/2050/openbot | phases/p02/docs/TASK.md; HANDOFF.md |
| p03 | /root; queued | /Users/mac02/.codex/worktrees/2050/openbot | phases/p03/docs/TASK.md; HANDOFF.md |
| p04 | /root; queued | /Users/mac02/.codex/worktrees/2050/openbot | phases/p04/docs/TASK.md; HANDOFF.md |
| p05 | /root; queued | /Users/mac02/.codex/worktrees/2050/openbot | phases/p05/docs/TASK.md; HANDOFF.md |

## Cross-phase notices

### 02WR-C001 / r1
- State: active
- Origin: user scope clarification, 2026-11-11
- Targets: p01–p05
- Instruction/change: Defer only the denied `channel.call` escalation trigger until `channel.call` exists. Implement all escalation triggers available today. The user explicitly selected this disposition. Do not introduce `channel.call` or revive deferred automated `message.send` dispatch.
- Dependencies/order: Applies throughout this plan and final completeness report.
- Evidence/review: User reply in this session; p04 must record the deferred criterion explicitly.
- Verification: Existing triggers exercised; unavailable child-call trigger clearly deferred.
- Responses: p01 r1 satisfied for its scope (phases/p01/docs/TASK.md, B1-A1); p02 acknowledges r1 at startup; p03–p05 evaluate at startup.

## Accepted prerequisite handoff (revision 2)

p01 B1-A1 accepted artifact91741e066ae90284ca17ba5163cbc89561eef3ac1fb728daec3645392d1f473d at HEAD103004a8ac2fd643cf14bde7e09022ae79f72053. Source archive/byte map retained in p01 logs; no commit required to recover it. B PASS02WR-P01-B1-R1/N1. Explicit p02 runtime carry-forward is in p01 HANDOFF/TASK; publication input preparation still needs its role branch. Full-plan final acceptance remains p05.

### 02WR-C002 / r1
- State: superseded
- Origin: p02 /root, phases/p02/docs/TASK.md D1/r1
- Targets: p02, p03, p04, p05
- Instruction/change: Root admission captures role holders for each reachable workflow version and step under the existing dependency closure. Frozen role rows include root-run/version identity and resolution_revision0, not an ambiguous step-only key. Agent execution never consults live roles and always reads0; future human follow-role may append later resolutions. Typed role refusals retain the64-byte static reason-code bound and carry bounded step/role details through message/schedule/manual consumers.
- Dependencies/order: Blocks affected p02 schema/runtime edits until B passes D1/r1 and root activates this notice. p03/p04 must reuse accepted schema/contracts and independently gate any required change.
- Evidence/review: Proposed decision in p02 TASK; independent gate pending.
- Verification: p02 resolver/admission/runtime/immutability/child/refusal/race/view tests; p03/p04 add their own behavior evidence; p05 combined final gate.
- Responses: p02 acknowledges r2 and implements under D1/r2; p03–p05 evaluate at startup.

### 02WR-C003 / r1
- State: proposed
- Origin: independent p02 clarification02WR-P02-C2-R1/N1, finding C2-R1/F1
- Targets: p02, p03, p04, p05
- Instruction/change: Resolve02A.3's literal relay-refusal clause before accepting p02. Existing ingestion returns successful delivery before workflow admission. Definite named refusal on the intent is implemented, but does not itself change that delivery into the disabled-desk terminal-refusal outcome. A pending user question offers preserving asynchronous delivery with workflow refusal, or requiring relay delivery to fail too. No narrower semantics or changed ingestion boundary is authorized by silence.
- Dependencies/order: p02 acceptance and dependent phases wait for this disposition. If literal relay failure remains required, gate the changed delivery/admission transaction contract before implementation. Other focused verification may continue.
- Evidence/review: phases/p02/docs/logs/02WR-P02-C2-R1-disposition.md and saved finding/notes; user question pending in the session. No whole B1 review/PASS or p02 acceptance.
- Verification: Record the selected semantics, affected production path and decisive tests before activation; retain the unresolved criterion meanwhile.
- Responses: p02 acknowledges r1 as unresolved at coordination revision6; p03–p05 remain queued.

### 02WR-C002 / r2
- State: active
- Origin: p02 /root, D1/r2 correction of architecture findings F1–F3
- Targets: p02, p03, p04, p05
- Instruction/change: Capture each root/version/step role requirement through the existing bounded dependency closure. Canonical immutable manifest content seals the complete admission requirement/principal sets; normalized role rows must match exactly, including inserts. Only resolution0 is supported in p02; future human reassignment requires a separate immutable canonical resolution and exact membership before enabling later revisions. Company-scoped agent deletion takes company NO KEY UPDATE before agent UPDATE, serializing demotion/channel cleanup with admission and department writers; preserve library behavior and all retained-history guards. Keep the64-byte static refusal-code bound and separate bounded typed step/role details through message/schedule/manual consumers.
- Dependencies/order: Blocks affected p02 schema/runtime/retirement edits until B passes D1/r2 and root activates. p03/p04 reuse the accepted contract and independently gate any required extension.
- Evidence/review: D1/r1 retained; F1–F3 closed at decision level by independent PASS02WR-P02-D1-R2/N1 for D1/r2, source91741e066ae90284ca17ba5163cbc89561eef3ac1fb728daec3645392d1f473d. Root activates at revision5 before implementation.
- Verification: deterministic both-order admission/delete schedules with exact business refusal, a committing role/revision race, canonical set mutation/refusal tests, runtime/child/view checks and affected memory/lifecycle concurrency regressions. p03 adds published People-set cases; p04 adds later-resolution sealing and empty-set behavior; p05 final combined gates.
- Responses: p02 acknowledges r2 and implements under D1/r2; p03–p05 evaluate at startup.
