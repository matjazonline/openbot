# Plan coordination: 04-root-unit

- Authority: /Users/mac02/orca/workspaces/openbot/plan03-04/plan/04-root-unit/COORDINATION.md
- Writer: /root in /Users/mac02/orca/workspaces/openbot/plan03-04
- Revision: 14

## Plan-wide instructions

- Execute original Plan04 documents in order. Latest user direction (2026-11-17):
  finish Plan04 and required verification, push main, then STOP. Do not start Plans05
  or06. C002/r2 supersedes earlier continuation authorization. No backward compatibility.
- Plan03 prerequisite published to main at `8fdb892b6e279ed0c09150e2336bbdc7462f877d`.
  Accepted material artifact c1ccbc73 / d27557291a01be97a3856d6cf1867e06900df104736a9c69f07754b3000f88fb,
  independent review 03PA-P04-B1-R17/N1. Later publication commits change only records.
- Acknowledge Plan03 C008/r1: remaining Doop execution is deferred to plan/fix-doop.md,
  does not block Plan04, and must remain visible as failed/incomplete. Whole CI is not green.
  Do not blindly defer new affected failures.
- Run focused root/membership/concurrency checks and HTTP authorization suites per batch.
  Run the complete repository/Plan04 final gate once after integration.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 (accepted) | /root; A root; B /root/p04_reviewer | /Users/mac02/orca/workspaces/openbot/plan03-04 | phases/p01/docs/{TASK,HANDOFF}.md |
| integration-main | /root; A root; B /root/p04_reviewer | /Users/mac02/orca/workspaces/openbot/plan03-04 | integration/main-60b9a28b/docs/{TASK,HANDOFF}.md |

| p02 (accepted) | /root; A root; B /root/p04_reviewer | /Users/mac02/orca/workspaces/openbot/plan03-04 | phases/p02/docs/{TASK,HANDOFF}.md |

| p03 | /root; A root; B /root/p04_reviewer | /Users/mac02/orca/workspaces/openbot/plan03-04 | phases/p03/docs/{TASK,HANDOFF}.md |

## Inherited accepted contract

- 01DEP-C002/r1 (explicit user2026-11-10): one-step audience BOTH WAYS — own
  members/role holders, parent Lead, direct child Leads, no further inheritance.
  Carries into Plan04 modes/UI/docs; read source Plan01 COORDINATION.md, not older
  parent-only shorthand. p02 acknowledges this requirement in its TASK.

## Cross-phase notices

### 04RU-C001 / r1
- State: active
- Origin: main integration60b9a28b, root writer; integration/main-60b9a28b/docs/TASK.md
- Targets: integration-main, p02, p03; p01 historical acceptance is retained.
- Instruction/change: imported attribution authority must read canonical root membership,
  retain account/company/root-row authorization fences and current permissions; no old
  company_members table is reintroduced. Owning deletion retains attribution terminal
  lifecycle/outbox evidence alongside global installation reservations, with exact proof.
- Dependencies/order: reviewed p01 commitd4bbde66/source2a5714f5…7ab793 plus imported
  origin/main03f0bda2, merged60b9a28b. Complete affected integration before p02 edits.
- Evidence/review: p01 independent04RU-P01-B1-R2/N1 PASS; published attribution contract
  plan/attribution-service/03-single-sign-on-and-company-access.md. This carries established
  owners/protocols, not a new architectural decision. Integration review remains required.
- Verification: attribution identity/access/lifecycle/competing status cases, real history-rich
  company/account teardown with exact retained terminal records, all21HTTP authorization cases.
- Responses: integration-main -> satisfied r1, source09f2972f…d22c4a8, independent
  04RU-MAIN-B1-R1/N1 PASS,55focused+HTTP21+exact terminal proof; own TASK.
  p02 acknowledged r1 at startup (own TASK); p03 pending.


### 04RU-C002 / r1
- State: superseded by r2 below
- Origin: explicit user steering2026-11-16, root writer.
- Targets: all current/future Plan04 phases and continuation owners for Plans05/06.
- Instruction/change: complete Plan04 and required verification, push to main, then
  start/execute Plans05 and06 in order under the selected Astra review workflow.
  Earlier stop-before05 records are superseded; no authorization beyond06.
- Dependencies/order: Plan04 completion/publication precedes05; honor each plan's
  documented prerequisites and phase acceptance gates.
- Evidence/review: direct user instruction; no architecture change or gate waiver.
- Verification: retain completion/review/check evidence and remote push identity
  before progressing; do not label unexercised checks passed.
- Responses: root -> acknowledged r1 here; p02/p03 acknowledge at startup;
  future05/06 root must carry this authorization into their own records.

### 04RU-C003 / r1
- State: superseded by r2 below; preserved pending-choice history.
- Origin: p02 investigation, phases/p02/docs/TASK.md D1 draft.
- Targets: p02, p03, future Plan05 scopes/tool authorization and documentation.
- Instruction/change: resolve older explicit01DEP-C001/r1 trusted internal-relay
  bypass against accepted Plan03 receiving-agent grant enforcement. Pending options:
  receiving grants everywhere; authenticated-agent bypass on root Members desks;
  authenticated-agent bypass on all Members desks. No answer inferred from time.
  Keep existing policy intact and the affected p02 architecture gate pending.
- Dependencies/order: resolve product choice before dependent authorization/schema
  changes and cohesive D1 acceptance. Unchanged-production scale measurement and
  caller/SQL investigation are independent and continue.
- Evidence/review: Plan01 COORDINATION.md C001/r1; current
  relay_desk_can_participate and relay_identity_tests; async clarification pending.
- Verification: real authenticated versus unproved/system senders, receiving grant
  presence/removal, root versus child desk, tenant boundary, hop/cycle refusal.
- Responses: p02 -> pending user choice; TASK/HANDOFF retain baseline PASS and draft;
  p03/Plan05 -> evaluate once activated, no new policy assumed from this proposal.

### 04RU-C003 / r2
- State: active; explicit user decision2026-11-17, "require grants as recommended".
- Origin: p02 clarification of internal relay access after practical examples.
- Targets: p02, p03, future Plan05 scopes/tool authorization and documentation.
- Instruction/change: every Members desk, root and child alike, requires the actual
  authenticated sending agent's receiving-desk participate grant. No trusted-agent
  bypass. Existing tenant, source-proof/incarnation, hop and cycle checks remain.
  Public intake behavior remains as planned. This supersedes inherited
  01DEP-C001/r1's trusted-relay bypass for this and later implementation.
- Dependencies/order: product choice resolved; incorporate into cohesive p02 D1
  and obtain independent architecture PASS before dependent edits. Existing
  accepted Plan03 source-proof and receiving-grant invariants are retained.
- Evidence/review: explicit user instruction in this session; D1 architecture
  and later batch execution still require normal review and checks.
- Verification: actual authenticated agent with/without/revoked receiving grant on
  root and child Members desks; unproved/system senders, foreign tenant, hop/cycle
  refusal; Public behavior and direct/history privacy unchanged as specified.
- Responses: p02 -> satisfied r2, source6391c0ba…1e660, independent04RU-P02-B1-R2/N1 PASS;
  p03/Plan05 -> acknowledge on startup and carry current rule into docs/tools.

### 04RU-C004 / r1
- State: active; independent D1/r2 PASS04RU-P02-D1-R2/N1.
- Origin: p02 actual unverified-root regression, logs/D1-r2-decision.md.
- Targets: p02,p03 and Plan05 membership-derived scope/access consumers.
- Instruction/change: a signed-in current principal's real department_membership
  grant depends on UUID placement, not email proof. Preserve existing unverified
  team attention/notes; other grant provenances remain withheld without proof.
  No root/relay/old-address/outsider bypass. Keep all current tenant, principal
  incarnation, row locks and revision/qualification fences.
- Dependencies/order: independent D1/r2 PASS before affected semantic edits and
  activation. Existing D1/r1 policy over-withholds root membership grant access.
- Evidence/review: B1-unverified-root-members-investigation FAIL; existing context
  grant_authority documentation; D1/r2 original criterion/proof mapping.
- Verification: unchanged unverified team journey, provenance-specific SQL/domain
  matrix, full affected email-proof/incarnation and real approval/follow races.
- Responses: p02 -> satisfied r1, source6391c0ba…1e660, independent04RU-P02-B1-R2/N1 PASS;
  provenance/domain/SQL and real approval/follow proofs retained. Future p03/Plan05 must acknowledge this active contract.


### 04RU-C002 / r2
- State: active; explicit user2026-11-17, "stop after 04 push".
- Origin: latest user steering, root writer.
- Targets: p02,p03,Plan04 publication owner and any continuation owner.
- Instruction/change: finish Plan04 and its required verification, push the completed
  Plan04 to main, then stop. Do not start Plans05or06. This supersedes C002/r1 and
  earlier continue-through06/stop-before07 authorizations.
- Dependencies/order: normal phase review/acceptance and final integration gates
  precede the authorized main push. This narrows continuation scope, waiving no
  Plan04 checks or policy/security requirement.
- Evidence/review: direct user instruction; no architecture/policy change.
- Verification: retain accepted Plan04/review/check evidence and remote main SHA;
  confirm work stops after publication and Plans05/06 remain unstarted.
- Responses: p02 -> acknowledged r2 in TASK/HANDOFF; p03/publication -> acknowledge
  on startup and stop after normal authorized main push.


## P03 startup acknowledgment (revision12)

P03 ACK C001/r1,C002/r2,C003/r2,C004/r1 and inherited01DEP-C002/r1. Accepted
p02 prerequisite commit4dad1e971e1791f75731da50a7e73e6776177057,
source6391c0ba46d382417a618bdc9238b32c1b8e1b976a457ee42f60b2382971e660,
independent04RU-P02-B1-R2/N1 PASS. Existing p01 and main-integration prerequisites
remain accepted. Finish original03 docs/full required gates, review combined
integration, push main, STOP; no05/06. Doop C008 remains separately explicit.

## P03 final verification entry (revision13)

All original final gates have executed; failed prerequisites/fixtures and successful
rechecks are retained in p03 TASK/logs. Current core stock run4719PASS/1Doop notify
timeout/29ignored, no overflow; isolated same-case stock recheckPASS. This is an
explicit review disposition question, not an automatic extension of C008 or an
unrelatedness claim. Normal full run had passed this Doop case; no Doop code,
assertion or deadline changed. Lease139PASS; all31initial core failures have exact
successful focused corrections; HTTP21, native/browser/service/image gates have
successful current proof under their documented prerequisites. Final independent
whole-p03 integration review and acceptance/publication remain required. No clean
whole-CI claim. C002/r2 still requires main push then STOP, with no05/06 work.

### 04RU-C005 / r1
- State: active; explicit user2026-11-17, "we'll check doop separately".
- Origin: user response to p03 final-review status naming the new full-stock Doop
  notify timeout; root writer.
- Targets: p03 final review, Plan04 acceptance/publication, separate Doop follow-up.
- Instruction/change: Doop checks and remaining Doop failures, including the named
  mock full-stock timeout, belong to separate follow-up. Finish and independently
  review verified Plan04 core, push main, then STOP per C002/r2. Do not investigate
  or fix Doop further as part of Plan04. Keep failures, CI declarations and test
  assertions visible; deferred means neither PASS nor whole-CI-green.
- Dependencies/order: all non-Doop Plan04 gates and current independent source/
  integration review remain required. No runtime/schema/authorization change,
  deadline increase, assertion removal or permission-policy exception.
- Evidence/review: direct user decision after the specific Doop gap was disclosed;
  p03 TASK/HANDOFF and immutable final-review scope addendum pin this revision.
- Verification: retain current core/service/browser/image proof, both failed full
  Doop result and isolated recheck, explicit deferred disposition, accepted source
  identity and normal remote main SHA. No Plans05/06 continuation.
- Responses: p03 root -> acknowledged r1; final B must acknowledge fresh scope
  before acceptance. Publication pending normal final review/acceptance.
