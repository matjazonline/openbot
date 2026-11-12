# Plan coordination: 02-workflow-roles

- Authority: /Users/mac02/.codex/worktrees/2050/openbot/plan/02-workflow-roles/COORDINATION.md
- Writer: `/root` in `/Users/mac02/.codex/worktrees/2050/openbot`
- Revision: 29

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
| p02 | /root; accepted in phase; integration pending p05 | /Users/mac02/.codex/worktrees/2050/openbot | phases/p02/docs/TASK.md; HANDOFF.md |
| p03 | /root; accepted in phase; integration pending p05 | /Users/mac02/.codex/worktrees/2050/openbot | phases/p03/docs/TASK.md; HANDOFF.md |
| p04 | /root; implementing B1, architecture accepted | /Users/mac02/.codex/worktrees/2050/openbot | phases/p04/docs/TASK.md; HANDOFF.md |
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
- State: superseded
- Origin: independent p02 clarification02WR-P02-C2-R1/N1, finding C2-R1/F1
- Targets: p02, p03, p04, p05
- Instruction/change: Resolve02A.3's literal relay-refusal clause before accepting p02. Existing ingestion returns successful delivery before workflow admission. Definite named refusal on the intent is implemented, but does not itself change that delivery into the disabled-desk terminal-refusal outcome. A pending user question offers preserving asynchronous delivery with workflow refusal, or requiring relay delivery to fail too. No narrower semantics or changed ingestion boundary is authorized by silence.
- Dependencies/order: p02 acceptance and dependent phases wait for this disposition. If literal relay failure remains required, gate the changed delivery/admission transaction contract before implementation. Other focused verification may continue.
- Evidence/review: phases/p02/docs/logs/02WR-P02-C2-R1-disposition.md and saved finding/notes; user question pending in the session. No whole B1 review/PASS or p02 acceptance.
- Verification: Record the selected semantics, affected production path and decisive tests before activation; retain the unresolved criterion meanwhile.
- Responses: p02 acknowledges r1 as unresolved at coordination revision6; p03–p05 remain queued.

### 02WR-C003 / r2
- State: active
- Origin: user decision, 2026-11-12, replying "do as recommended" after the explanation of missing role holders and the two delivery outcomes
- Targets: p02, p03, p04, p05
- Instruction/change: Preserve successful message delivery when a role-dependent workflow cannot start. Keep the accepted message on its desk and record a definite, named workflow refusal on its admission intent; the refusal identifies the step and role and admits no run. Workflow admission failure does not turn the completed relay delivery into a terminal delivery refusal. This explicit user disposition replaces02A.3's literal same-relay-refusal-path clause. Existing ingress refusals (disabled desk, access policy, spent hop budget) retain their transport behavior.
- Dependencies/order: Effective before p02 B1 review. Preserve existing asynchronous ingestion/admission boundaries; no new delivery/admission protocol is required. Later phases reuse this outcome and may escalate the workflow refusal under their own accepted contracts.
- Evidence/review: User's explanation/decision in this session; originating ambiguity and open finding02WR-P02-C2-R1/F1 retained in p02 records. B must independently assess closure under this authorized disposition in whole B1 review.
- Verification: Successful ingest retains the message, one definite typed intent refusal names the step/role, no run/capture/provider call, no retry; message outcome reader/display carries the refusal. Existing relay ingress refusal tests remain applicable.
- Responses: p02 acknowledges r2 at revision7 and updates its deliverable docs and acceptance mapping; p03–p05 evaluate at startup.

### 02WR-C002 / r2
- State: active
- Origin: p02 /root, D1/r2 correction of architecture findings F1–F3
- Targets: p02, p03, p04, p05
- Instruction/change: Capture each root/version/step role requirement through the existing bounded dependency closure. Canonical immutable manifest content seals the complete admission requirement/principal sets; normalized role rows must match exactly, including inserts. Only resolution0 is supported in p02; future human reassignment requires a separate immutable canonical resolution and exact membership before enabling later revisions. Company-scoped agent deletion takes company NO KEY UPDATE before agent UPDATE, serializing demotion/channel cleanup with admission and department writers; preserve library behavior and all retained-history guards. Keep the64-byte static refusal-code bound and separate bounded typed step/role details through message/schedule/manual consumers.
- Dependencies/order: Blocks affected p02 schema/runtime/retirement edits until B passes D1/r2 and root activates. p03/p04 reuse the accepted contract and independently gate any required extension.
- Evidence/review: D1/r1 retained; F1–F3 closed at decision level by independent PASS02WR-P02-D1-R2/N1 for D1/r2, source91741e066ae90284ca17ba5163cbc89561eef3ac1fb728daec3645392d1f473d. Root activates at revision5 before implementation.
- Verification: deterministic both-order admission/delete schedules with exact business refusal, a committing role/revision race, canonical set mutation/refusal tests, runtime/child/view checks and affected memory/lifecycle concurrency regressions. p03 adds published People-set cases; p04 adds later-resolution sealing and empty-set behavior; p05 final combined gates.
- Responses: p02 acknowledges r2 and implements under D1/r2; p03–p05 evaluate at startup.

## Accepted p02 prerequisite (revision8)

p02 accepted02WR-P02-B1-A1, artifactbe7fd9d4c0c6e45c67ad288ed4d1a9ccfb221f6a816a7dbaf8a7c24fe44de6ba at HEAD103004a8ac2fd643cf14bde7e09022ae79f72053, B PASS02WR-P02-B1-R1/N1. Complete source archive/byte map and author/independent evidence retained in p02 logs. D1 findings verified implemented; C2/F1 closed under user-authorized active C003/r2. p03 may consume this artifact and independently gate its extension to People resolutions. C001/r1, C002/r2 and C003/r2 remain active.

## p03 startup (revision9)

p03 root acknowledges I001–I003/r1 and active C001/r1, C002/r2, C003/r2; accepted p02 source consumed via pinned archive/byte map. Its phase-local TASK/HANDOFF hold discovery and the upcoming People/reviewer contract gate. No cross-phase extension proposed or implemented yet.

### 02WR-C004 / r1
- State: superseded
- Origin: p03 /root, phases/p03/docs/TASK.md 02WR-P03-D1/r1
- Targets: p03, p04, p05
- Instruction/change: Reviewer identities use person principals, with literal role selectors frozen as People requirements at root admission. Exact canonical membership sealing extends to multiple person holders and multiple roles per version/step. Agent and People requirements remain distinct; model selectors ignore People. Park merges/deduplicates pinned principals and sealed role members under effective16 bound. User-account lookup is notice/auth enrichment only; no stored reviewer user_id. Future followRole must preserve this principal-only contract and gate later immutable resolutions.
- Dependencies/order: Blocks affected p03 source edits until B explicitly passes D1 current revision and root activates. Original D1 defines literal-role and parameter-principal behavior; no new user scope is inferred.
- Evidence/review: Proposed D1/r1; architecture review pending.
- Verification: p03 TASK AC1–AC5, canonical-set SQL mutations, role/park/reassignment/notice/submit/UI tests; p04 extends sealing/followRole; p05 combined full gate.
- Responses: p03 acknowledges proposed r1 for gate; p04/p05 evaluate at startup.

### 02WR-C004 / r2
- State: superseded
- Origin: p03 /root, phases/p03/docs/TASK.md D1/r2, correction of02WR-P03-D1-R1/F1–F3
- Targets: p03, p04, p05
- Instruction/change: Reuse C004/r1 principal-only reviewer/People sealing contract. Preserve recipient principal identity across lifecycle demotion for notice cancellation. Identity resolution does not relax subject-specific admin/owner approval or account-based payload clearance, including publication-time revalidation. Principal-valued reply policies discover account mappings without locks, lock sorted accounts before company authorization, then revalidate exact mappings/policy under company serialization; no principal-before-account lock edge. Future p04 respects these authority/retirement contracts.
- Dependencies/order: Blocks affected edits until B passes D1/r2 and root activates.
- Evidence/review: D1/r1 findings retained; revised D1/r2 immutable at p03 logs/D1-r2.md; recheck pending.
- Verification: p03 AC1–AC5 plus production pending/in-flight cancellation for both lifecycle callers, subject-specific authority/clearance revocation and deterministic both-order retirement/prelock races. p04 extends sealed reassignment; p05 combined final gates.
- Responses: p03 acknowledges proposed r2 for gate; p04/p05 evaluate at startup.

## p03 D1 activation (revision12)

Root activates C004/r2 after independent PASS02WR-P03-D1-R2/N1 for D1/r2 and unchangedbe7fd9d4c0c6e45c67ad288ed4d1a9ccfb221f6a816a7dbaf8a7c24fe44de6ba. FindingsF1–F3 fixed at decision level; implementation/behavioral checks remain B1 obligations. p03 acknowledges active C004/r2 with I001–3/r1,C001/r1,C002/r2,C003/r2 before dependent edits.

### 02WR-C004 / r3
- State: active
- Origin: p03 root observed inherited history regression; TASK D1/r3 and logs/B1-inherited-decision-tests1.txt
- Targets: p03,p04,p05
- Instruction/change: Extend principal-only identity with a bounded immutable reviewer-label display snapshot derived from the already-locked person principal on insert. Read labels from historical reviewer rows so principal deletion preserves the existing whole decision view. No account/user_id snapshot, authority relaxation or additional account lock. Future p04 replacements derive labels for new rows while retaining older facts. D1/r2 F1–F3 protocols remain unchanged.
- Dependencies/order: Only label schema/projection edits wait for B PASS D1/r3 and activation; current C004/r2 remains active until then. Fresh owned schema DBs, no backfill.
- Evidence/review: D1/r3 proposed; original whole-view preservation assertion retained, gate pending.
- Verification: existing stable-history equality plus actual label projection, deletion/demotion retention, forged/updated label rejection, bounded multibyte labels and escaped rendering; all original p03 obligations remain.
- Responses: p03 acknowledges proposedr3 for reopened gate; p04/p05 evaluate at startup.

## D1/r3 activation (revision14)

Root activates C004/r3 after independent PASS02WR-P03-D1-R3/N1 for D1/r3 and unchangedpartial-source63df976b49daad3df412d828c3086d92a519fdf92e19d1a5c1df1bb5a7e3829e. C004/r3 carries forward r2 identity/sealing and F1–F3 authority/lock/cancellation contracts, adding the bounded immutable label. Known regressionD1-R3/F1 remains implementation/verification open until B1. p03 acknowledges activeI001–3/r1,C001/r1,C002/r2,C003/r2,C004/r3 before affected edits. No B1 implementation acceptance.

## p03 verification clarification pending (revision15)

Root requests independent02WR-P03-C1/r1 assessment of a confirmed inherited owned-tenant action-identity FK failure and a bounded F3 coverage disposition. Currentactive notices unchanged. Decision/evidence in p03 TASK/logs; no product/schema contract change or unrelated fix proposed. Owning-run-company retirement commit stays a reported gap; alternative full account-retirement schedules and an owner-company lock-prefix schedule are proposed. Future p05 must consume the resulting reviewed disposition explicitly.

## p03 verification clarification accepted (revision16)

Independent PASS02WR-P03-C1-R1/N1 accepts C1/r1 coverage/disposition for F3. Root implements full retirement races with reviewer owning another empty company, plus explicitly prefix-only same-tenant-owner ordering case. Confirmed inherited failure02WR-P03-C1-R1/F1 remains unresolved/outside changed namespace protocol and must be reported atp05; prefix rollback does not prove owner teardown commit. ActiveC004/r3 and all original scope/authority contracts unchanged. Principal label source remains255bytebounded; no valid oversized-source truncation claimed.


## Accepted p03 prerequisite (revision17)

Root accepts02WR-P03-B1-A1, artifact5bd5645a4256002c4202f11f1bdfdea9814c98188e1d6e2dcbd181cd503ba1db at HEAD103004a8ac2fd643cf14bde7e09022ae79f72053 after B PASS02WR-P03-B1-R2/N1, exact snapshot and evidence validation. Complete206path archive/map and correction/independent evidence retained in p03 logs. Active C004/r3 principal/People/label/account-authority protocols now implemented; p04 may consume this accepted source and must separately gate later sealed resolutions/escalation. C001/r1,C002/r2,C003/r2,C004/r3 remain active. C1-R1/F1 inherited same-workflow-tenant owner teardown failure remains separately reportablep05, without an affected-protocol gap being hidden. Full-plan gate remainsp05. No contract change in this revision; historical p03 records immutable.


## p04 startup (revision18)

p04 root acknowledges active I001–I003/r1,C001/r1,C002/r2,C003/r2,C004/r3 and acceptedp03 prerequisite5bd5645a4256002c4202f11f1bdfdea9814c98188e1d6e2dcbd181cd503ba1db via complete archive/map. Phase-local TASK/HANDOFF hold original AC1–7 and initial discovery. Only channel.call-denied trigger deferred. No p04 schema/ownership/authority/concurrency extension proposed or implemented yet; whole D1 gate remains before affected edits. Historicalp03records immutable, full-plan verificationp05.


### 02WR-C005 / r1
- State: superseded
- Origin: p04 /root, phases/p04/docs/logs/D1-r1.md (02WR-P04-D1/r1)
- Targets: p04,p05; plan03 all phases when consuming plan02
- Instruction/change: Gate the complete durable no-run/terminal escalation subjects, two-hop idempotent existing-event ladder, principal-specific person/agent notice adapter, audited administrative decisions in shared UI, static human followRole with frozen root department context/separate canonical later and empty resolutions, exact current/history reviewer assignment and pending event notice proof, synchronous bounded role/topology/lifecycle refresh and account-before-company protocol. Agent model selections remain0. Plan03 replaces only agent notice routing adapter with principal bindings after reset. Includes narrow deferred action-identity FK repair needed for full role-holder owner retirement, preserving commit-time identity enforcement and historical p03 gap records.
- Dependencies/order: No affected implementation before B explicit PASS for current whole D1 revision and root activation. Original04 requirements/C001/r1,C002/r2,C003/r2,C004/r3 persist.
- Evidence/review: Immutable D1/r1; independent architecture review pending.
- Verification: D1 impact/decisive-assertion matrix plus original TASK AC1–7; actual production triggers/notice/decisions/role/lifecycle paths, competing workers, both-order retirement/role/vote/projection, canonical SQL negatives; p05 full gate.
- Responses: p04 acknowledges proposedr1 for gate at19; p05/plan03 evaluate at startup.


### 02WR-C005 / r2
- State: superseded
- Origin: p04 /root, phases/p04/docs/logs/D1-r2.md, fixes D1-R1/F1–F3
- Targets: p04,p05; plan03 all phases when consuming plan02
- Instruction/change: Preserve C005/r1 scope and add complete inherited reviewer-account hint/union prelock and terminal company-first protocols (ordinary Business/action/token/final response and legacy notification projection/open included), authenticated notification-origin suppression across root/child/rerun/generated-message causality with original-case handling failure/finite owner attention, and neutral all-holder notification truth table supporting mixed/multiple-agent sets up to16 with explicit over-bound/ineligible dispositions. Retained departments keep existing deletion refusal. Agent model resolution0/People identity/current authority/cancellation/history contracts persist.
- Dependencies/order: Affected source still unchanged/frozen; no implementation before B current whole D1/r2 PASS and root activation.
- Evidence/review: D1/r1 and findings retained; immutable D1/r2/correction delta; independent recheck pending.
- Verification: Original AC1–7 plus D1.8–10 actual both-order inherited-consumer retirement/terminal schedules, finite self/cross-desk/generated-origin cycles and neutral holder truth table. No timeout/deadlock or fabricated test origin accepted as behavioral evidence. Full suite p05.
- Responses: p04 acknowledges proposedr2 for gate at20; p05/plan03 evaluate at startup.


### 02WR-C005 / r3
- State: active
- Origin: p04 /root, phases/p04/docs/logs/D1-r3.md, completes residual D1-R1/F1,F2 after R2
- Targets: p04,p05; plan03 all phases when consuming plan02
- Instruction/change: C005/r2 whole scope retained with operation/state-specific applicable account hints preserving historical reads/comments/replays and unused-policy independence, while live synchronous effects preserve actual reviewer/clearance/notice authority. Agent escalation notice intents target only the authenticated captured default binding/revision, retain current validation and zero-or-one accountable handling root; ordinary user intent fan-out remains unchanged. Stable origin link identifies recipient settlement and keeps recursive case suppression, with honest64 notice-handling root bound. Neutral all-kind recipient truth table retained.
- Dependencies/order: No p04 source edits; whole D1/r3 must pass B before activation/implementation.
- Evidence/review: R1/R2 findings and decisions retained; immutable full D1/r3/delta/pin, B recheck pending. F3 already closed at architecture level, behavioral evidence pendingB1.
- Verification: D1.11 historical/removal/live-authority cases and inherited both-order schedules; D1.12 two/eight-binding divergent outcomes, exact target/refusal/root origin, unchanged ordinary fan-out; D1.9–10 recursion/recipient tests and original AC1–7 persist. Full suitep05.
- Responses: p04 acknowledges proposedr3 at21; p05/plan03 evaluate at startup.


## p04 D1/r3 activation (revision22)

Root activates C005/r3 after independent PASS02WR-P04-D1-R3/N1 for immutableD1/r3 and unchangedsource5bd5645a4256002c4202f11f1bdfdea9814c98188e1d6e2dcbd181cd503ba1db, exactroot snapshot. D1-R1/F1–F3 resolved at architecture level; all promised implementation/behavioral evidence remains B1 work. p04 acknowledges active I001–3/r1,C001/r1,C002/r2,C003/r2,C004/r3,C005/r3 before affected edits. No B1 acceptance. Historicalp01–p03 records immutable; fullsuitep05.


## p04 implementation checkpoint (revision23)

Metadata progress only; active I001–3/r1,C001/r1,C002/r2,C003/r2,C004/r3,C005/r3 unchanged. WholeD1/r3 architecture accepted; p04 partial implementation/focused evidence in phaseTASK/HANDOFF, no B1review or phaseacceptance. p05 and allplan03 remain queued.


### 02WR-C006 / r1
- State: superseded by r2; original proposal retained
- Origin: p04 /root, logs/D2-r1.md; actual generic administration-audit store is absent
- Targets: p04,p05; plan03 consumers
- Instruction/change: Supplement C005/r3 retirement audit ownership with a purpose-specific bounded immutable department administration event, actual principal demotion/delete boundary production, exact company-scoped role clearing proof and historical identity snapshots that preserve physical retirement/otherwise-permitted department deletion. No fabricated transport binding/workflow row. All D1/r3 authority/refresh/cancellation/retention semantics unchanged.
- Dependencies/order: No affected audit implementation before independent D2/r1 PASS and root activation. Other accepted D1/r3 work remains authorized; implementation worktree frozen during gate review.
- Evidence/review: Immutable phases/p04/docs/logs/D2-r1.md and premise/source snapshot/coordination pins; gate pending.
- Verification: D2 exact production/no-op/rollback/tenant/mutation/retention matrix plus all original D1 lifecycle races and full owner teardown; p05 integration.
- Responses: p04 acknowledges proposal r1 at24 before affected edits; p05/plan03 evaluate on startup.


### 02WR-C006 / r2
- State: active after D2/r2 architecture PASS
- Origin: p04 /root, immutable D2-r2.md correction of D2-R1/F1
- Targets: p04,p05; plan03 consumers
- Instruction/change: Retain r1 purpose-specific retirement audit and all C005/r3 contracts; explicitly represent unbounded department names as first128Unicode characters plus exact truncated flag from a single SQL helper shared by producer/guard. Full historical IDs and exact255-byte-bounded principal labels retained, unchanged4KiB event cap, no new source-name bound or retirement refusal for long valid names. Include ASCII/multibyte/encoded boundary production retirement and source-proof negatives. Preserve tenant-cascade exception and later legitimate re-grant history semantics.
- Dependencies/order: No audit extension before independent currentD2/r2PASS and activation. Source remains frozen at existing D2-R1 digest; unchanged main D1/r3 implementation still partial/unaccepted.
- Evidence/review: D2-r1/findings retained; D2-r2.md +exactdelta andnewcoord25pins; independent recheck pending.
- Verification: D2.2 name representation boundaries plus all original D2/D1 real lifecycle/rollback/tenant/mutation/concurrency cases.
- Responses: p04 acknowledges proposedr2 at25, before dependent edits; p05/plan03 evaluate at startup.


## D2/r2 activation (revision26)

Root activates C006/r2 after independent PASS02WR-P04-D2-R2/N1 for completeD2/r2 on frozenfffdbcd9c996956ab18e91ab892767ee33179be9f77c263062a61ddc4b810210. Rootsame6excludes snapshotmatches; F1closedatarchitecturelevel. p04 acknowledges I001–3/r1,C001/r1,C002/r2,C003/r2,C004/r3,C005/r3 andC006/r2 beforeaudit/lifecycle edits. AllD1/D2 actualbehavior/testobligations remainB1work; noimplementationacceptance. Historicalpriorphasesunchanged. Sourcefreeze endsafteractivation; fullcohesiveB1stillrequired.


### 02WR-C007 / r1
- State: active after D3/r1 architecture PASS
- Origin: p04 /root, actual role-writer company→exclusive-gate versus D1 new agent notice shared-gate→account/company conflict; immutable D3-r1.md/premise
- Targets: p04,p05; plan03 agent routing adapter consumers
- Instruction/change: Keep D1/D2 protocols, targeted origin and bounds. For new private agent notice commit, sharedgates first then sortedREALrequiredaccounts SHARE NOWAIT andcompany NOKEY NOWAIT; whole-transaction rollback/releases before separatelyfenced queue backoff/explicitDeferred. Current authority/missing route remain actual typedsourceoutcomes. No blocking account/company wait underagentgate; currentperson/control/legacy path unchanged. Required actualboth-order role/notification andthree-party retirement contention/worker/fence/poison proofs.
- Dependencies/order: No affected agentnotice implementation before currentD3/r1 independentPASS+rootactivation. Existing authorized unrelatedD1/F1 work remains but entireimplementation source isfrozen duringgate.
- Evidence/review: D3-r1.md + exactsourcepremise/graphspans, currentwhole-source identity/coord27pins; independentgatepending.
- Verification: D3 actualproduction contention/deferral/retry/ownership andallunchangedD1/D2obligations.
- Responses: p04 acknowledges proposedr1at27beforedependentedits; p05/plan03evaluateatstartup.


## D3/r1 activation (revision28)

Root activatesC007/r1afterindependentPASS02WR-P04-D3-R1/N1 forcompleteD3/r1 +exact43e2e0dfb959a236ffb0c2aeefa4dbe7d899685d21ddaf5a88d7cacd5a405721, rootsame6excludessnapshotidentical. No architecturefindings; allactualcontention/fence/Deferred/backoff/worker testsremainB1obligations. Rootp04acknowledgesI001–3/r1,C001/r1,C002/r2,C003/r2,C004/r3,C005/r3,C006/r2,C007/r1beforedependentagentnoticeedits. Sourcefreezeendsafteractivation; fullcohesiveB1unaccepted, p05fullgate/plan03queued.


### 02WR-C008 / r1
- State: proposed
- Origin: p04 C1/r1, actual producer inventory contradicts D1.3 pre-preparation role-refusal premise
- Targets: p04,p05; plan03 admission routing consumers
- Instruction/change: Preserve all actual fenced role-refusal/terminal triggers. Treat the currently absent legitimate production pre-PreparedAdmission role-refusal producer as not applicable; scripted samples and binding authoring validation must not mint cases. Re-inventory at p05/plan03; introducing such a producer requires the original fenced atomic outcome path before enabling it. No new feature deferral, API or protocol.
- Dependencies/order: Proposed C1/r1 clarification requires B PASS and root activation before adopting this disposition. Source frozen; whole B1 partial/unaccepted.
- Evidence/review: Immutable p04 logs/C1-r1.md, actual application constructors/sample callers/pure bounded_snapshots/capture owner and unchanged preparation/sample assertion. Independent assessment pending.
- Verification: B independently checks every real producer and caller; all original D1/D2/D3 behavioral evidence remains B1/p05 work.
- Responses: p04 acknowledges proposal r1 at revision29; p05/plan03 evaluate after acceptance.
