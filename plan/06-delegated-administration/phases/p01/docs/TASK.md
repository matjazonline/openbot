# 06DA-P01: Predicate and owners

## Identity and authority

- Workflow: implement-astra-reviewed-2-3. Phase: p01.
- Original request: implement plan/06-delegated-administration, first check the
  codebase has the needed dependencies. User subsequently says Plan05 is underway
  in another worktree and asks which phase is required.
- Original criteria: ../../../README.md and ../../../01-predicate-and-owners.md;
  later documents 02 and 03 remain authorized and planned, not implemented.
- Record root: /Users/mac02/orca/workspaces/openbot/plan06/plan/06-delegated-administration/phases/p01/docs.
- Root/A: /root, session 01a1e945-99f4-76e0-965e-403847e819b2; actual model
  gpt-6.1-sol, effort xhigh confirmed by this session's turn_context.
- B: /root/p06_reviewer, independent no-history child, requested gpt-6-astra/high;
  actual worker settings unexposed. Reuse this reviewer for B1 implementation review.
- Worktree: /Users/mac02/orca/workspaces/openbot/plan06; branch plan06.
- Starting commit, current HEAD and review base: d748c2ff64309903449a3d860377a5e0385377a4.
- Preserved pre-existing changes: none (clean startup git status).
- Current review-candidate digest: f90321fd26058d076596f6a266282ef5a72ff8d4c099acb523594d4af7972d0c.
  Exact snapshot exclusions:
  plan/06-delegated-administration/phases/p01/docs and
  plan/06-delegated-administration/COORDINATION.md. No source/deliverable exclusions.
- Coordination authority: /Users/mac02/orca/workspaces/openbot/plan06/plan/06-delegated-administration/COORDINATION.md;
  last checked revision 2; no applicable notices or pending cross-phase requests.
- Context: host does not expose effective capacity or compaction threshold. Use
  host defaults and durable checkpoints; no global configuration changed.

## Dependency check (2026-11-17)

| Dependency | Observed evidence | Disposition |
| --- | --- | --- |
| Plans01/02/04 | Root department, canonical membership and department-role code present; HEAD includes Plan04 publication d748c2ff and implementation86d225cc | Present; retain inherited one-step desk grants and current identity fences |
| Plan05 phase03 knowledge scopes | KnowledgeScope carries only company/actor (src/application/knowledge/types.rs:17); persistence authorize checks company team membership (src/adapters/persistence/knowledge/mod.rs:23); no workflow_run_scopes in indexed src | Not integrated here; direct dependency of Plan06 01B item6 |
| Plan05 phase03A | Unit-owned documents/chunks and unit-scoped search/read contract | Required before final knowledge integration; phase03B builds on this |
| Plan05 phase03B | Target-unit upload and one-step upload authority | Exact authority Plan06 extends to managing any ancestor |
| Plan05 phase02 (transitive) | run_scopes and immutable workflow_run_scopes consumed by phase03 search | Producer prerequisite of phase03; do not independently recreate it |
| Plan05 phase01 (transitive) | Unit/principal namespace ownership used by phase02 | Producer dependency, not a reason to duplicate memory implementation here |
| Locked root Rust packages | cargo fetch --locked --offline exits0 | Cached; compilation not yet exercised |
| Tools | rustc/cargo1.99.1, sqlx-cli0.8.6, clippy, Node26.13.1, npm11.20.0, Python3.13.18, CMake/cc/curl/rg and sandbox-exec available | Present; no new crate identified as necessary |
| PostgreSQL | Local18.7; mac02 role is superuser/CREATEDB | Available for disposable databases |
| Query-statistics gate | shared_preload_libraries empty; pg_stat_statements available but not loaded | Required verification setup unavailable on current default server configuration; do not alter shared server settings |
| Frontend packages | Root, plugin and attribution-browser node_modules absent | Routine npm installation needed before affected/final checks |
| Docker image gates | docker CLI exists; daemon socket unavailable | Final image checks currently unavailable |
| Per-worktree artifacts | target/einvoice-validator and target/knowledge-model absent | Need gate-specific preparation; absence is not a source defect |

Plan05 authority read at revision1 in /Users/mac02/orca/workspaces/openbot/plan03-04.
That worktree's phase directory reports p01 investigating, p02/p03 planned. No accepted
Plan05 artifact is available at this check. The user's current Plan06 authorization
applies to this worktree; older stop-before06 instructions in Plan04/05 belong to
those producer sessions and are not imported as a prohibition here.

## Scope and batch boundaries

- Trace p01 owners and invariants before editing. Stable predicate/authority work
  can be investigated independently of Plan05. Knowledge authority and complete
  p01 acceptance wait for the accepted Plan05 phase03 contract.
- Own Plan06 records and eventual predicate/owner authorization changes in this
  worktree only. Shared integration surfaces include the init schema, department
  lifecycle, workflow authorization, knowledge authority, root memory settings and
  generated SQLx metadata. Reconcile these against accepted Plan05 source before
  final acceptance; never copy its uncommitted work.
- Architecture gate required before dependent edits: authorization ownership,
  role-change serialization, definition ownership and schema/transaction changes.
  Decision revision is not yet authored or passed.

## Acceptance mapping (planned, not exercised)

| Criterion | Production path/case | Decisive assertion | Planned evidence | Status |
| --- | --- | --- | --- | --- |
| 01A predicate | Person owner/root admin/unit or ancestor lead; sibling/child/member/foreign/agent; depth8 | Exact authority result and acting role; no visibility widening | manages_unit_ matrix with real DB parity | NOT EXERCISED |
| 01B each owner | Department, desk, binding, schedule, definition, agent, knowledge, root settings/invites | Managed target succeeds; sibling is NotFound; direct persistence bypass refuses | delegated_admin_ per owner plus HTTP authorization families after each batch | NOT EXERCISED |
| Role safety | Self/ancestor/sibling role escalation and subtree re-parenting | Parent management/owner-root requirement prevents escalation | DB/use-case/HTTP negative cases | NOT EXERCISED |
| Concurrency | Role removal competes with actual management writer | Serialized write or refusal, atomic audit/effect | delegated_admin_competing_ with actual competing production calls | NOT EXERCISED |
| Final gate | Integrated Plan06 and accepted Plan05 dependency | Required repository/CI gates pass with immutable retained logs | scripts/check.sh, CI comparison, fresh schema/SQLx, offline, source reachability, network, stack/lease, docs/graph | NOT EXERCISED |

## Contract impact (initial trace)

| Contract | Consumers | Obligation/disposition | Checks |
| --- | --- | --- | --- |
| CompanyMembership::manages_company_operations | Trail transitive callers include department configuration/UI, attention/run inspection, company settings, workflow authority and LevelActor | Classify management versus read/classification/root-only decisions; replacing management must not grant conversation reading or raise levels | Focused per-owner and unchanged privacy/levels coverage |
| managed_company/WorkflowAuthorization | Company, desk and associated configuration | Target resolves to owning unit; Company resolves root; agent actors refuse | Auth matrices and direct persistence calls |
| Plan05 knowledge upload | Target-unit authority, library and persistence write | Preserve base one-step entitlement and level ceiling; add subtree management once producer accepted | knowledge_scope_upload_ plus delegated cases |

## Resources and verification

- No application process, test database, port or build process created yet.
- Future DB names/prefix must be Plan06-specific and disjoint from Plan05; use
  local target directories in this worktree. Never use a shared _test database.
- Retain focused commands/results and failed/unavailable evidence per batch.
- Full suite has not run and no implementation/review/verification acceptance is claimed.

## Current status and next action

Dependency inspection complete; p01 investigation underway, no source edits yet.
Direct prerequisite identified to user: Plan05 phase03A/03B; phase02/01 transitive.
Continue tracing independent authorization owners, resolve one cohesive architecture
revision and review it with B before implementation. Recheck producer coordination
and integrate only an accepted immutable Plan05 artifact for dependent knowledge work.


## Authorized continuation and B1 architecture gate — 06DA-P01-D1/r1

Latest user instruction: "proceed and stop when 05/ph03 is necessary". Implement
and independently accept stable non-knowledge batches; STOP before knowledge
integration and any dependent whole-phase/final-plan acceptance. Coordination r2
pins this scope in logs/coordination-r2.md. No producer work or new publication
permission inferred. No source edits have begun.

### Batch B1: shared predicate and department administration

This is a stable authorization contract and one complete management owner, including
application, persistence and competing callers. Later owner batches consume its
accepted contract. Preserve existing root-only operations until each consumer is
explicitly ported; the old company-management helper is removed only after its
consumer inventory has an explicit disposition. B1 is not acceptance of all p01.

**D1/r1 decisions, invariants and failure behavior**

1. One SQL STABLE function `unit_management_role(user, company, unit)` owns the
   predicate. A null unit selects that company's root. It first proves the target
   department belongs to the requested tenant and the actor is a current person
   with canonical root membership. It returns `owner`, `admin` or
   `lead:<department UUID>`: owner precedence, then root admin, then nearest lead
   on the target's bounded ancestor chain. Root is depth1, maximum8 including root,
   matching MAX_DEPARTMENT_DEPTH and department_shape_problem. Unknown/foreign
   target, missing/retired identity, ordinary member, sibling/descendant lead or
   agent UUID returns no authority. There is no widening of grant/read/level policy.
2. Application `use_cases/administration.rs` owns a small required persistence port
   for this lookup, a typed acting-role result and the `manages_unit`/require helper.
   PostgresPersistence implements it and PostgresDepartments forwards to its existing
   persistence handle. DepartmentPersistence includes this authority port so current
   department use cases can preflight without a new service/container dependency.
   Future company/principal consumer ports can inherit the same authority port when
   ported, keeping one SQL decision instead of local recursive implementations.
   The SQL return is decoded fallibly; infrastructure errors propagate.
3. Department persistence keeps the existing bounded transaction protocol. Lock the
   actor account first, then company FOR NO KEY UPDATE, then current principal/root
   membership, then target department/principals in their established order. Reuse
   company_level_authority::{start,lock_team} to establish current person membership
   without imposing root-management permission. Query the same predicate under the
   company lock before each write; direct persistence entry cannot bypass permission.
   Existing serialized role/membership/reparent writes guarantee revocation either
   follows an authorized old-role write or precedes a refused write. No optimistic
   stale preflight can authorize a commit after revocation.
4. Create a child: manage its parent (None means root). Rename/edit/delete an existing
   department: manage its parent; root itself requires root management. Reparent:
   manage both the old parent and the destination parent, checked while locked.
   Existing cycle/depth, owned-desk/history deletion and root shape guards stay.
   Membership add/remove: manage the department itself. Membership and roles are
   distinct witnesses; removing a membership does not silently clear an independent
   held role. Role assign/clear: manage the parent; all root role changes require
   company ownership. The mirrored owner role remains uneditable and admin remains
   root-only. An ordinary unit lead cannot alter its own lead role or gain authority
   over its parent/sibling through a move or role operation.
5. Preserve the existing special owner-only removal of a person's root membership
   and its complete sorted-account teardown protocol in this batch; widening this
   company-member removal path is outside this initial department contract. Root
   agent membership still uses existing root-manager path. Role refusals and foreign
   targets use NotFound before effects; existing invalid role/root-shape requests
   retain their structural validation. Any later change to root membership removal
   requires explicit original-criteria reconciliation in the owning batch.
6. This batch changes no knowledge/memory scope contract. Its migration adds only
   the shared function to the init schema; fresh schemas only, no backfill. Plan05
   integration must reconcile the init migration from accepted producer source.
   No production/shared database reset and no shared server configuration changes.

**Affected contracts and proof mapping**

- Predicate DB matrix: exact owner/admin/unit/ancestor lead role; sibling/child,
  member, stranger, cross-company target, external/system/agent principal refusal;
  deepest legal eight-level chain and unknown target. The production decision is
  SQL only, avoiding a second Rust policy; Rust decodes role data, never derives it.
- Actual DepartmentUseCases and PostgresDepartments matrix for all mutation kinds,
  including direct persistence calls. Assert descendant work succeeds, sibling and
  own-role escalation are NotFound with zero mutation; reparent out refuses;
  parent-manager changes are allowed; root role changes are owner-only.
- Real competing role-removal versus create/update/member management call, observed
  waiting on the company lock; assert either authorized old-role atomic result or
  refusal, never stale-role effect after demotion. Retain existing department races.
- Run `department_`/`manages_unit_`/`delegated_admin_` focused DB families and HTTP
  authorization families after B1, fresh migrations, SQLx prepare, locked offline
  all-target compilation and source-module reachability. New tests are appended;
  no deleted/moved tests planned. Later route/UI batch adds lead rendering/HTTP cases.
- One full repository gate remains required after integrated complete Plan06. At
  the user-requested dependency stop, run appropriate checkpoint verification and
  explicitly retain whole-plan unverified status; do not claim complete Plan06.

B1 architecture status: pending independent review. Decision revision is immutable
in this section; corrections get a new section/revision. Future definitions require
user clarification: active-binding units (README decision3) versus always recorded
managing department (01B.4). An async question is pending; do not build that dependent
contract without the answer.


### Live resource and clarification update

- B handle /root/p06_reviewer, requested gpt-6-astra/high; actual worker settings
  unexposed. Independent no-history child confirmed initial snapshot identity.
- User resolved definitions: managers of active binding units; recorded department
  when unbound. This governs later definition work and supersedes conflicting01B.4
  shorthand. B1 department decision unchanged.
- Producer coordination rechecked r3: C003/r1 promises accepted phase03 publication;
  C004/r1 requires a separately gated opaque-subject memory extension. Both are
  dependency information here; B1 changes neither contract and does not import them.
- Owned isolated PostgreSQL18 cluster at
  /Users/mac02/Library/Caches/openbot/plan06-01a1e945-postgres/data, TCP127.0.0.1:55406,
  socket directory its parent; actor mac02. Only this session owns its lifecycle.
  Default100connections retained, pg_stat_statements preloaded/top/utilityoff and
  parameter logging0. No shared PostgreSQL configuration changed. Test/schema DB
  openbot_p06_b1; set TEST_DATABASE_URL explicitly so other worktrees are disjoint.
- Snapshot initial command failed because BSD realpath lacks -m; same installed
  script succeeds with PATH=/opt/homebrew/opt/coreutils/libexec/gnubin:$PATH.
  Initial architecture snapshot base d748c2ff, digest
  3774130c5113a407f3af1c3c37f8c288742f2365b02c1d5a741a34901719f33d.
  This is unchanged source; both exclusions match the declared list.


## 06DA-P01-D1/r2 — downstream department effects (supersedes r1 scope)

Independent 06DA-P01-D1-R1/N1 raised high F1: root/read guards in department
provisioning and lead-follow side effects invalidate the complete-owner boundary.
This revision retains decisions1–6 but expands B1's implementation and proof scope
instead of bypassing the nested boundaries. F1 remains open until B rechecks r2.

1. Extend the same required administration lookup port with a typed target:
   Root, Unit(DepartmentId), Agent(AgentId). Root/unit use the one SQL predicate.
   Agent resolution proves a current same-company agent principal, unions its real
   department memberships and roles, and applies that predicate to those units;
   only an agent in no department falls back to root. Unknown/foreign agents never
   get the fallback. This is original README decision2, required here by automatic
   initial publication and follow-lead re-publication. Preserve the publication
   account→company→agent locking and revision/skill/tool/receipt fences.
2. Central persistence authority distinguishes locking current person facts from
   requiring root management. Add explicit scoped unit/agent/association authority
   paths over the same predicate; do not make authorize_company mean "any lead".
   Existing root-owned resource/configuration callers keep root permission. Every
   migrated caller supplies its actual department/agent or immutable run association.
   A run-scoped management guard reads its association under the company lock and
   preserves all live actor, run, lease, source and credential checks. NOWAIT callers
   retain their mode and sorted account prefix (l-070f6689); no late blocking helper.
3. Department-owned desk creation first creates the plain desk directly in its
   owning department, then configures its responder through the existing follow-lead
   flow. Set the initial lead role before responder publication so the selected
   agent is already placed in the new unit. This avoids a temporary root-owned desk
   or implicit root permission and consolidates the plain/agent creation paths.
   Existing callers retain unique command keys, exact provisioning receipts,
   grant synchronization, native source compilation, publication and binding state
   validation. Lead replacement writes placement before follow-lead as today.
4. Department follow-lead, binding save/check_run_as, reply authorization/selection,
   native template captures and agent publication must use the owning target's
   management guard. Configuration-only operations prove association existence,
   tenant and exact thread/inbox relationship without requiring or granting a
   conversation View grant. Inspect/manual message/history and actual read routes
   retain their existing grant checks. Do not change shared association visibility
   helpers to silently allow managers. If an existing combined helper serves both
   configuration and reading, split the responsibilities at the owning layer.
5. Native automated reply admission/preparation/continuation consumers of the newly
   valid lead run-as must ask scoped authority for the captured run association,
   rather than root authority. Scope the changed callers through actual producer
   and run/fence traces. Retain actual agent receiving/participation grants,
   classification and consumption checks, current principal/member incarnation,
   frozen publication, source-proof, resource credential and ownership fences.
   A configuration manager gains no human history/desk-read API entitlement.
6. Added proof: real delegated create with no/person/agent lead; transition
   person→agent, agent→agent, agent→person, agent→none; initial and already-published
   selected agent variants; lead run-as retained in exact provisioning/binding
   revisions; department manager lacking desk View can change configuration while
   thread/history/desk reads stay refused; one real automated message admission and
   execution through the delegated department desk, proving the new run-as survives
   runtime guards. Direct binding/reply/publication sibling and foreign target calls
   refuse. Preserve root-only resource checks with a negative lead case. The batch
   is not accepted if any required downstream case is left for a future batch.

No Plan05 scope data is involved. Knowledge stays explicitly untouched. Workflow
fully general definition creation/active-binding ownership and public route/UI
wiring remain later independent batches; user chose active-bound-unit authority.
If actual native template construction needs that definition contract, include its
owning schema/decision in a reopened gate before dependent edits, never substitute
root identity or skip the authorizer. Full final gate remains at the complete-plan
boundary; checkpoint gates and affected interfaces must pass before dependency stop.


### B1 implementation checkpoint (before current compile)

Independent architecture D1/r2 PASS 06DA-P01-D1-R2/N1; F1 resolved at decision
level only. Root began implementation after the final PASS. B remains retained
/root/p06_reviewer and idle; implementation review has not been requested.

Current implemented source: SQL unit_management_role and typed AdministrationPersistence
port; Postgres/department delegation; use-case and locked department parent/unit/root
role guards; initial lead placement before direct-unit plain desk + follow-lead
responder provisioning; scoped agent publication and binding/reply configuration;
separated current-person locking from root-management requirement; scoped immutable
run guards preserving native account prefixes/NOWAIT; manual-versus-durable-message
admission operation and shared LifecycleAuthorizer unit/agent preflight; native final
reply source scope. Four existing in-memory principal directories implement the
required port as explicit root-role stand-ins (no unit topology); live DB matrix
covers delegated roles. General owner APIs/UI/audit/definition ownership and knowledge
remain unfinished. Existing root helper still has unmigrated consumers; no old-helper
removal or complete p01 acceptance claimed.

Proof so far: initial fresh schema migration PASS; initial offline all-target check
PASS with one now-corrected dead-code warning (old desk reassignment helper is now
cfg(test) because production creation no longer temporarily assigns root ownership).
Initial new matrix ran5PASS/0FAIL in1.28s, after7m12test compilation; original4749
inventory retained. This initial binary precedes the flow/runtime additions and
latest principal-port/runtime changes, so it does not verify current combined source.
Current all-target check found two compile mistakes (entity department_id is already
DepartmentId; Thread uses target rather than channel_id); source corrected, recheck
pending. Preserve initial/second/third compile logs and initial matrix output.

Added pending tests: native lead transitions with an unpublished initial capture,
no-human-View configuration/read refusal, real production clear-role helper in an
owned transaction racing actual persistence member writer, and a native Deployment
journey through message ingress/worker/model/final response/delivery with lead run-as.
No result for those additions yet. Full whole-plan verification remains for after
knowledge integration and is outside the dependency-stop completion claim.


### Current integrated focused run input

Source is now frozen for execution at base/HEADd748c2ff and digest
ba5e79b90dad9887fd06a36ef9c2544526d5837b9fe2719ae3e34d48c1bddd48,
exact existing exclusions; logs/B1-focused-input.snapshot.txt retains full file list.
Cargo process48744 builds/runs the integrated focused families under loopback-only
network guard with TEST_DATABASE_URL targeting owned openbot_p06_b1_v2: delegated_admin_,
manages_unit_, department_, root_membership_, root_department_, authorization_tests,
workflow_binding_, native_mcp_ and channel_reply_. No current verdict yet; do not
change source until completion. The check-fourth all-target compilation passed in
1m44s before the last pure helper/test decomposition; final current source still
needs its own checks and retained evidence. cargo fmt completed after that final
source decomposition; git diff whitespace PASS. No test names were removed; test
helpers were split to stay under source style bounds.

Context signal inspected from the session log: a past token-count event timestamp
2026-11-17T12:27:53.619Z reported window475000 and last-request input304738
(cached304512). This is a historical sample, not current live occupancy or proof
that a356250-token75% compaction threshold is enforced. Continue durable checkpoints;
active host settings remain unexposed. Current source freeze and running command
are recorded above; no worker replacement or global config changes.


Additional downstream SQL readers identified during frozen-run inspection:
notification_principal_can_view/root reviewer visibility, escalation command/agent
eligibility and native_mcp live source membership guards contain owner/admin
classifications. These are not automatically management grants: later owner
migration must distinguish read/assigned-reviewer/credential-source contracts from
configuration/root management and record each disposition. B1 leaves these SQL
read/credential conditions unchanged; its actual native basic-reply proof has no
native-MCP/HTTP/MCP resource or escalation side effect. Existing native-MCP focused
family is included to detect accidental prefix/mode/permission regressions.


## 06DA-P01-D1/r3 — model source authority after actual runtime counterexample

Frozen first integrated inputba5e79b9…ddd48 ran515tests:506PASS/8FAIL/1ignored,
216.93s after4m14build. Seven new hierarchy/owner/lead-transition/revocation tests
passed; new native Deployment admitted the run but failed at the draft agent step
with task_attempts.workflow_failure_code=agent.rejected. Read-only bounded repro
retained actual executions/events/attempts in B1-native-repro-* logs; initial state
sampling missed the disposable DB teardown and is retained as unavailable evidence.
No passing runtime assertion is claimed. Existing failures include intended root
role404/private-configuration policy changes and a now-misplaced company-port outage
stand-in; retain/refactor their assertions at the responsible current boundary.

Root cause traced: agent_history/preparation.rs::locked_on and
agent_preparation.rs::current_on still require the *human run-as* conversation View
at the model boundary, even for an owned automatic department reply whose actual
selected agent has its own valid desk grants. Simply dropping those checks would
also let a manual session continue after its person's grant was revoked. That would
violate the preserved manual/history policy; it is explicitly ruled out.

**D1/r3 decision (adds to r2; independent PASS required before affected edits):**

- Configuration remains person-scoped unit management. Actual human desk/thread/
  history reads and manual model sessions retain their current grant requirements.
  The existing human association authorization path still succeeds exactly as before.
- Introduce a narrowly owned model-source authority helper in agent_preparation,
  used by both initial locked agent preparation and the current checkpoint guard.
  Resolve the frozen execution selection and verify current agent definition/use
  authority first. Require current person management of the run association under
  the existing account/company protocol. Attempt the existing human association
  View guard; errors propagate, except an explicit NotFound may enter the following
  narrow automatic-agent proof path. No generic association read helper changes.
- The fallback proves an actual channel_reply_runs owner joined to its immutable
  channel_reply_context and actual workflow_run: exact company/run, actor, binding,
  channel/principal and thread match; top-level native automatic reply ownership.
  Manual, observer, schedule and unrelated runs do not inherit it. A current
  same-company agent principal must match the already verified frozen selected agent.
  For a desk, lock that principal and its actual View and Participate grant rows
  (both required) in deterministic order before accepting the agent's source access.
  A selected agent replying on its own direct principal target uses that exact
  self-inbox identity; other direct principals do not inherit access. Missing,
  foreign, changed or revoked witnesses refuse. Direct grant revocation is fenced
  by row SHARE locks; scoped management is separately fenced by the company lock.
- This is server execution as the authenticated selected agent over a proven owned
  reply, not human conversation visibility. It cannot authorize config on behalf
  of an agent lead, change a person's level, or expose history APIs. The helper
  does not create a View grant or add a new permission vocabulary.
- Keep agent manifest/revision/use-mode, actual role and credential eligibility,
  source proof, classification/consumption, checkpoint/lease and native sorted-account/
  NOWAIT contracts. Initial preparation is reordered only to obtain/validate its
  selected agent before the model-source authority decision; company still precedes
  agent/grant and checkpoint gates. Current model guard reuses the same helper after
  restoring and checking the exact selection identity, avoiding diverging decisions.

**Decisive verification for r3:** the existing real native department journey must
produce/deliver its exact output with the delegated lead run-as while the human
read rule remains false; separately remove the selected agent's View/Participate
witness and assert provider calls/output remain0. A manual private session whose
human grant is revoked must still refuse; an observer/manual/schedule run cannot
manufacture owned reply proof. Retain existing revoked-agent/publication, read/
history and native-MCP current-authority/race suites. Root-only resources remain
separate. Architecture PASS does not waive any of these execution obligations.


## 06DA-P01-D1/r4 — bounded desk-only fallback and competing grant proof

Recheck06DA-P01-D1-R3/N1 raised F2: add actual competing initial/current model
grant-revocation proof, and explicit direct-self/other-principal evidence. No r3
source edits have begun; input remainsba5e79b9…ddd48.

- Remove r3's proposed direct self-inbox fallback. Inspection of the actual existing
  principal_direct_can_view function (init schema16337) proves every current
  same-company person with canonical root membership already reads an agent direct
  target. A manager must have that same current person/root witness. Thus a valid
  managed agent target reaches the unchanged human-View success path; the added
  self exception would be redundant/unreachable. Person/foreign/missing targets do
  not become agent targets. The new fallback handles only a desk and requires both
  exact selected-agent desk View and Participate witnesses, never a principal inbox.
  Preserve existing valid agent-direct tests and other-principal/foreign refusals;
  no invented no-human-View direct-agent fixture or new direct-view permission.
- Strengthen the proof mapping: create a genuine owned automatic reply and obtain
  its claimed agent activation/frozen selection through the existing admission/
  claim/preparation fixtures. Exercise actual AgentExecutionPersistence::prepare_agent
  at the initial boundary and agent_preparation::current_on at the checkpoint boundary.
  For each boundary, hold a real grant-deletion transaction before authorization,
  observe the model caller waiting on that exact grant revoker's backend, commit
  revocation and assert refusal/no provider/output. In the opposite ordering, hold
  the actual successful current-authorization transaction open, start a competing
  direct grant DELETE, observe it waiting on the authorization owner, then commit;
  verify old-grant authorization followed by completed removal and refusal on the
  next check. Actor management and agent definition/revision remain otherwise valid.
  Run View/Participate cases so either lost witness is decisive. Use disjoint owned
  databases and real row-lock wait sampling, never timing-only sleeps or sequential
  booleans. A full native Deployment positive remains independent behavioral proof.
- Add manual/observer/schedule no-owned-proof negative coverage using actual run
  provenance: actor View revoked while management and agent grants remain valid
  must still refuse at initial/current model boundaries. Actual provider call count
  and publication counts remain0. Existing privacy/history readers unchanged.

This supersedes only r3's redundant direct-self branch and incomplete verification
mapping; the desk policy and all r2/r3 fences remain. F2 needs B's explicit r4
recheck; initial/current grant fencing is not accepted on inspection alone.


### D1/r4 implementation and correction proof now pending

Architecture r4 PASS06DA-P01-D1-R4/N1; F2 resolved at decision level only. Model
source helper now checks scoped person management, preserves human View success,
and otherwise requires an exact owned automatic desk/thread reply plus the frozen
selected agent's actual locked View+Participate rows. No direct-inbox exception
was added. Both initial preparation and current checkpoint use it after current
selection verification. A thread argument must match the actual owned run/context;
all current manifest/revision/lease/classification/credential fences remain.

Added four real-fixture protocol tests: initial grant revocation first, current
revocation first, current authority first, and non-owned observer/manual/schedule
provenance exclusion at initial/current model boundaries. Both capabilities are
exercised and PostgreSQL waits name the exact blocking backend. The HistoryThread
fixture drives production admission/claims/preparation/current-check APIs but has
no provider; the separate native Deployment positive and revoked-agent zero-script
negative supply actual worker/provider/delivery proof. These tests have no result
until the current build/run completes.

The seven existing assertion updates retain foreign/member/privacy and structural
refusals while allowing management without human View and hiding root role refusal
as404. An obsolete company-only outage stand-in is replaced with a closed actual
unit-authority pool for write failure, retaining the failing company port for reads.
Root-person removal remains owner-only in this batch. Source test rename:
channel_reply_binding_rechecks_current_run_as_private_channel_visibility ->
channel_reply_binding_separates_management_from_current_run_as_visibility; retain
before4749 inventory and confirm replacement discovery in the after inventory.

Exploratory compile logB1-check-model-correction retains two revision comparison
errors corrected via explicit revision.get()+1 assertions. R4 compile retained a
missing MessageTriggerInput import, now corrected. The first correction build was
cancelled after inspection caught a five-parameter proof query with only four binds;
B1-correction-cancel records verified worktree-owned Cargo/descendant PIDs only.
No other phase was interrupted. Corrected command is running in exec72187, immutable
output B1-corrections-focused-second.txt, source identity retained in
B1-corrections-input-second.snapshot.txt. Source is frozen again for execution.

## Current continuation — corrected B1 execution

The second correction run finished 16PASS/4FAIL. Three new current/provenance
fixtures failed because their app-domain was unset before preparing an agent;
set the fixture persistence's existing app-domain option to agents.test. The
existing direct human grant race was deadlocking its own sequential write against
the revoker's desk lock; restored concurrent configuration/commit ordering and
retained explicit denied-human-read assertion. No authorization assertion removed.

Corrected focused run B1-corrections-focused-third.txt: **20PASS**, including all
new predicate, department, native positive/zero-provider negatives and both grant
lock orderings/provenance tests. Full focused interface regression is running in
logs/B1-focused-integrated-corrected.txt: 543 selected tests, explicit owned v2
database and loopback-only network guard. SQLx preparation/all-target compile,
clippy and source module reachability are queued/running in their B1 logs. No
source edits during these checks; candidate snapshot retained in
logs/B1-review-candidate.snapshot.txt. Implementation acceptance remains pending.

Producer coordination rechecked revision7: p01 accepted locally, p02 architecture,
p03 still planned. No Plan05 artifact imported; C005/r1 remains proposed and does
not change this batch's contracts. Existing producer notices remain external
prerequisites. User definition clarification is unchanged: active-bound-unit
managers, recorded department when unbound.

### B1 executed acceptance mapping for review

| Criterion | Production path/case | Decisive assertion and actual tests | Evidence/status |
| --- | --- | --- | --- |
| Predicate/current identity/tenant/depth | AdministrationPersistence and actual unit_management_role SQL | manages_unit_current_person_hierarchy_and_tenant_matrix, manages_unit_reaches_the_eighth_level_and_rejects_a_ninth, manages_unit_agent_placements_use_any_managed_unit_and_no_unit_uses_root check typed role/unknown/foreign/agent, depth8 and no-unit/any-placement management | third correction log PASS |
| Department mutations/recheck | DepartmentUseCases and direct PostgresDepartments mutations | delegated_admin_department_parent_member_and_role_boundaries and delegated_admin_department_persistence_refuses_every_sibling_write check current/parent/destination management, no self/ancestor/sibling escalation, direct writer refusal | same PASS |
| Competing role removal | Actual department manager mutation and role clear under company lock | delegated_admin_competing_lead_removal_precedes_a_locked_management_write samples the exact blocker and asserts no write after committed revocation | same PASS |
| Nested department provisioning | Real create/follow-lead publication/binding/reply revisions | delegated_admin_agent_lead_transitions_configure_without_conversation_access covers create with none/person/unpublished/published agent, person→agent→agent→person→none, delegated run-as and denied human conversation reads, sibling/foreign/root-resource refusal | same PASS |
| Real runtime/privacy | Actual Deployment/ingress/worker/provider/delivery path | delegated_admin_native_department_reply_uses_the_lead_run_as_without_human_view asserts exact delivered text and person run-as while human View remains false; delegated_admin_native_reply_missing_agent_grant_never_enters_the_provider separately removes each capability and asserts zero provider entries and output | same PASS |
| New grant protocol | Actual prepare_agent/current_on with genuine owned run/frozen selection | delegated_admin_competing_agent_grant_removal_fences_initial_preparation, delegated_admin_competing_agent_grant_removal_fences_current_model_source, delegated_admin_competing_current_model_authority_fences_grant_removal test View and Participate and both orderings with exact pg_blocking_pids backend; actual refused result/next refused check | same PASS; these fixtures do not run a provider, supplied by separate runtime cases |
| No provenance widening | Actual manual/observer/schedule admission/claims/preparation/current_on | delegated_admin_model_source_never_borrows_owned_reply_proof_for_other_runs retains management and actual agent grants, removes human View and asserts each boundary refuses; no provider in this fixture | same PASS |
| Existing interfaces/privacy | Department/root routes and binding/reply UI/persistence | third correction log includes the seven corrected assertions; 543-test broader regression running | targeted PASS; broad pending |
| Fresh schema/SQLx | Owned freshly migrated v2 database and all-target prepare | B1-migration-v2.txt and B1-sqlx-prepare.txt succeeded; no SQLx cache diff expected for runtime queries | PASS |

Review candidate 4139979e…77d3c; exact established exclusions unchanged. Immutable
manifest logs/06DA-P01-B1-R1.evidence.json validates. Full suite/final-plan gate
intentionally pending the user-identified Plan05 dependency and complete Plan06.
Clippy, focused broad regression and source reachability remain pending before B1
acceptance. Source remains frozen. Before/after discovery4749→4762:13new tests,
one recorded rename with discoverable replacement; no lost behavior/test module.

### B1 R1 final candidate / supplemental checks

Clippy initially failed four new test-style expressions. Removed three redundant
explicit PgConnection dereferences and replaced one literal-bool equality with an
equivalent assertion; no production, schema, policy or fixture change. Reviewer's
preliminary read interrupted before this correction, then resumed against frozen
c0c9cc0500609a1dc6d28d4e7b70092481f1331eeb49e34c41afcb64ea3ace00 with exact
unchanged exclusions. No accepted review exists for the earlier candidate.

Broad focused interface run **542PASS/1ignored/0FAIL** (543selected); source modules
**8PASS**. Lint correction's four affected tests **4PASS**, corrected offline clippy,
locked offline all-target compilation, formatting and patch whitespace all **PASS**.
Metadata remains unchanged after successful SQLx regeneration. Existing broad
execution evidence is reused only across those equivalent test-expression edits;
their own affected execution was repeated. Manifest
logs/06DA-P01-B1-R1-S2.evidence.json validates and retains10PASS entries. B1 is still
awaiting independent review; B is inspecting a required-proof mapping mismatch.
Full-plan/final suite remains explicitly unexercised pending the scoped dependency.

## B1 R1 findings / correction in progress

Independent note06DA-P01-B1-R1/N1 returned CHANGES REQUIRED at c0c9cc05…ace00.
B1-F1 found that `admission_claim.is_some()` incorrectly included Schedule in the
message-only View exemption. Both actual boundaries now discriminate the Message
variant; all other admissions retain human View. This restores D1/r2/r4's scope;
no new schedule exemption or architecture widening is proposed. Traced actual
AdmissionClaim producers/consumers; inventory retained in B1-F1-claim-consumers.
Added a real claimed-schedule regression that captures a valid prepared command,
then removes human View while retaining management, checks both application and
direct persistence refusal/no effects, and restores View to prove the same actual
claim/payload succeeds. Existing native owned-message positive remains required.

B1-F2 found the transition-test mapping claimed negatives it does not assert.
That claim is withdrawn: the transition test proves managed creation/lead changes,
publication/run-as and human desk privacy only. New direct nested-writer tests
exercise sibling and foreign binding pause, reply settings and agent publication
with valid revisions and no write/revision/active-publication change on refusal;
the otherwise same commands succeed under actual target authority. A separate
unit-lead actual ResourceDirectory denial retains root-owned resource authority.
New cases are not yet executed. Source-module inventory now includes new test files.
Current compilation runningB1-review-corrections-check.txt; reviewer idle retained.

### B1 correction verification and instruction disposition

Corrected all-target offline compilation and clippy PASS. New schedule/direct
downstream/resource tests have passed in the current405-case focused run; aggregate
result pending. SQLx regeneration, source-module checks, format/whitespace and graph
refresh running under this batch's owned resources. No source edits while they run.

`trail check` returned1 for a lexical conflict in the newly added predicate SQL.
Its referenced learningl-3837ec57 requires deferring bounded native/workflow NO
ACTION peers at teardown rather than relying only on department/principal order.
The B1 migration diff adds only a STABLE management SELECT function: no FK,
cascade, deferral, deletion order or teardown guard changes. The shared teardown
boundary remains intact. This is a false positive on words used by the recursive
unit/role lookup, not a decision to override that learning. Retain the failed log
B1-R2-trail-check.txt and this rationale for reviewer confirmation and any future
commit/PR. No commit or PR is authorized/created here. New learningl-1944bdfc records
the distinct claim-variant cause; it is uncommitted and included in the next source
identity. Do not edit the prior learning.

## Proposed next batch B2 — desks and schedules (D2/r1, NOT GATED)

Investigation only until B1 is accepted and B independently passes this decision.
Original01B.2–3; source/JSON shapes stay the same. General definitions/agents remain
the subsequent B3 before the knowledge prerequisite in01B.6;02/UI/audit/final gates
remain ordered after complete01. No Plan05 source or contract is introduced.

1. Extend the existing ChannelPersistence management writes to require the current
   WorkflowActor and explicit company. ChannelUpdate carries its actor; delete
   carries actor/company/desk; create carries actor/company/write. These are internal
   port changes, not JSON fields. No actor is inferred from created_by or ownership.
   Existing internal `_on` insert/update helpers remain trusted transaction helpers;
   each production caller retains its own scoped management/fixture contract.
2. Desk create requires management of the chosen unit (root when absent); update
   requires the current unit and destination when changing ownership; delete
   requires the current unit. Application preflight reuses participant persistence's
   AdministrationPersistence. Every public persistence write rechecks under account
   then company locking before unit/desk/gate locks and its write. Reuse the current
   bounded edit transaction, namespace checks, exact settings revision/grant refresh,
   reviewer refresh and retained-history deletion refusal. A role removal or desk
   move cannot leave stale preflight authority sufficient to commit.
3. Desk configuration reads use management of the actual desk's unit, preserving
   explicit tenant checks. Actual readable-channel/thread/mailbox/simulation/history
   paths keep their own current grant/level checks. List/UI filtering remains02
   unless required for a migrated public owner path; never replace read policy with
   the management predicate. Root-owned model/resource control stays root-only.
4. Schedule persistence create/edit/delete/enable/configuration reuses the scoped
   association management guard over its immutable owning desk, without human View.
   Remove root-only preflights before resolving an existing scoped schedule: load
   within company/channel scope, then authorize its actual desk. List configuration
   filters by those same managed desks; missing/foreign/sibling targets are NotFound.
   Preserve binding compatibility, revision, cadence, occurrence and lease protocols.
5. Schedule run inspection and run-now are distinct from configuration: keep human
   View at application and persistence boundaries where a person reads or initiates
   a run. Actual scheduled admission retains the B1-F1 correction. Configuration
   alone cannot manufacture View or a message claim. Current run-as/resource/source,
   classification and immutable binding checks remain separate and unchanged.
6. Update every affected port implementation/caller; focused compile drives fixture
   literals. Existing DB fixtures may use a test-only helper that explicitly obtains
   their real company's owner actor and calls the same production checked write;
   never infer an actor in production or retain unchecked public company mutators.

**B2 verification mapping (planned):**

| Criterion | Production path / decisive assertion | Focused checks |
| --- | --- | --- |
| Desk owner | Use-case/API and actual persistence create/update/reassign/delete: managed target succeeds; sibling/foreign and direct bypass refuse; both units needed on move; no effect on refusal | delegated_admin_desk_, channel CRUD/edit/access-mode tests and channel authorization routes |
| Schedule owner | Real create/update/enable/pause/delete under delegated current unit; direct adapter sibling/foreign refusals; immutable channel and compatibility preserved | delegated_admin_schedule_, schedule management/API/UI authorization families |
| Privacy / initiation | Manager without View can configure; cannot inspect history/simulate/run-now/admit a schedule; granted person retains existing behavior | current channel visibility/read/foreign tests, schedule run/history tests and B1-F1 regression |
| Concurrency | Actual role removal competes with migrated desk/schedule writer under company protocol; refused post-revocation with no partial state | delegated_admin_competing_ plus current edit/settings/occurrence competing tests |
| Port/SQL/integration | All implementations and downstream consumers accounted for; current source inventory retained; no unchecked public company writes | locked offline all-target check, focused tests, source_modules, SQLx prepare, format, clippy, whitespace; B1 affected regressions only |

Trace pointers: logs/B2-owner-callers.txt, B2-channel-edit-callers.txt and
B2-root-authority-callers.txt; source ownership and public configuration-vs-read
callers were inspected before this proposal. Exact B2 frozen identity and final
consumer dispositions will be recorded only after implementation. Complete final
repository gate remains at the full-plan boundary and is not claimed here.

### B1 R2 aggregate diagnosis and final correction

The405-case run finished404PASS/1FAIL. All three new findings regressions passed,
but the existing schedule_management_requires_an_owner_or_admin_who_sees_the_channel
test exposed that B1's shared ManageBinding configuration preflight also widened
schedule listing/inspection. Added the correctly named ManageSchedule operation
for the current schedule use case; it retains human View at the same shared scoped
authorizer. Binding configuration's deliberate no-View rule remains separate.
No existing schedule assertion changed. This restores the already-gated B1
invariant rather than starting proposed B2's schedule-owner changes. New enum has
one production consumer; locked all-target compilation/clippy checks every adapter.

SQLx prepare PASS, no cache diff. Its cache rewrite was incorrectly run concurrently
with offline source_modules compilation, producing21 missing-query-cache errors.
This is the author's check-orchestration error; retain B1-R2-source-modules.txt.
Regeneration is now finished and a sequential offline rerun is running in
B1-R2-source-modules-corrected.txt. No cache workaround or metadata changes were
introduced. Corrected schedule/department focused checks and clippy are running;
the404 unaffected passing tests from the prior run remain retained, with this
actual failed schedule expectation rerun after its responsible-layer correction.

### B1 R2 frozen correction artifact and actual evidence

Source c5286ce14e6fb94342627eae937049ce00fe01a3d83b41d587956ef1f2d35442; same
base/HEADd748c2ff and exact two exclusions. B1-R2-final.snapshot.txt retains the
complete manifest, including new learning and three new regression tests.

Current schedule_ + delegated_admin_ run: **59PASS**, including B1-F1's real claim
case, B1-F2's direct writer/root-resource cases, the unchanged schedule authority
assertions, native positive/zero-provider negatives and grant races/provenance.
Locked offline all-target check, offline clippy, source_modules8tests, SQLx prepare,
formatting, whitespace and graph refresh **PASS**. SQLx cache remains unchanged.
Before/after inventory4749→4765:16new tests plus the prior one discoverable rename.

R2 manifest validates9PASS/1FAIL entries; the FAIL is the actual current `trail
check` lexical result described above, pending independent semantic disposition.
It is not represented as a green command. Historical405 run404PASS/1FAIL and the
cache-race failure remain retained with corrective evidence. Unchanged predicate,
department/root/API and native-fence assertions reuse the earlier542PASS/1ignored
log only where the exact production path, fixtures and assertions are unchanged.
Affected schedule/admission/model paths have current59-case evidence. Complete
repository/Plan06 gate is still NOT EXERCISED; this is a B1 correction checkpoint.

Local coordination r2 still byte-matches the retained pin. Producer nowr9: p02
implements accepted architecture, p03 remains planned. C005/r2 is active only for
producer02/03/04 and future scope integration: historical snapshot copies and
native source projections may not widen live scope. B1 and proposed B2 introduce
no producer scope schema/wire contract and import no artifact; not applicable to
their current source acceptance. Retained full producer pin in
logs/06DA-P01-B1-R2-producer-coordination.md. User stop-at05/ph03 remains in force.

B1-F1/F2 corrections are ready for B's single recheck. If B1 passes, the separate
proposed D2/r1 architecture input is pinned in logs/06DA-P01-D2-R1-decision.md;
no B2 edits or acceptance are implied by that proposal or by B1's source digest.

## B2 implementation started after D2/r1 acceptance

Independent06DA-P01-D2-R1/N1 PASS for architecture/proof plan; B1 accepted prerequisite
c5286ce1…35442. Baseline51source files retained as
logs/B1-accepted-source.snapshot.tar, SHA256
4a237a8fee48bd03a2ebf4208e396797ede067b9ebe06603b1e17cc4029dea97; it is nonignored
and is a historical source artifact, never an exclusion of current implementation.
Use it to review the B2 delta while retaining the agreed d748c2ff commit base.

Desk port now carries actual actor/company into create/update/delete; application
preflights current/destination units and persistence rechecks while locked. Existing
fixture creation calls migrated to one test-only helper that obtains their actual
company owner's person actor and uses the same checked public writer. All five
nonproduction trait implementations are updated; source compile now enumerates
remaining explicit actor fields/call sites. No actor inferred in production and no
unchecked public company mutator is retained. Test fixture conversions are semantic
setup preservation, not assertions or authorization stand-ins for the new cases.

B2 isolated database openbot_p06_b2 is being created/migrated on the existing owned
cluster55406; B1 DBs retained. Initial all-target check runningB2-check-initial.txt.
Source is in progress, not frozen/reviewed. Reviewer idle after D2 gate. Scope and
the user STOP at knowledge05/ph03 remain unchanged; no whole-phase acceptance.

### B2 complete source / initial focused execution

Desk actor port and all six implementations now compile offline. Existing raw
creation fixtures use the test-only owner helper; explicit updates carry their
actual fixture actor. Schedule configuration uses ManageSchedule without human
View, and run inspection/initiation use Inspect/Admit plus the persistence
capture-now View recheck. The Message-only admission fence remains unchanged.
One pure WorkflowOperation property owns the two association View decisions.

Collection entry uses server-only ListManaged/ManagementTarget::AnyUnit, backed
by the same unit_management_role over root and the person's held lead units.
This implements the already-gated scoped-list semantics: a member/stranger/agent
cannot enter, and every returned schedule is separately filtered by its actual
desk guard. The collection target is not a write permission, is not accepted for
an individual association and is not part of any JSON shape. No new persistent
ownership, role, permission vocabulary or producer contract is introduced.

Preserved the existing desk gate-before-row-lock order: initial management reads
the current owner under company serialization, then checks current/destination
units with the common predicate. It does not borrow the association helper's
desk SHARE lock ahead of the gate. The real ingress/edit competing test now samples
the exact blocking backend and upgrades the ingress's row lock NOWAIT while the
edit is waiting, making a premature conflicting row lock a decisive failure.

New actual DB cases cover desk use-case/direct CRUD and moves, both role-removal
orderings for edits, scoped collections, schedule CRUD/listing/direct sibling and
foreign denial, human-View-dependent run inspection/initiation with a real granted
positive, and schedule revocation-first contention. New real JSON handler journeys
cover lead desk/schedule success and sibling/foreign404 without human desk View.
Old schedule_management_requires_an_owner_or_admin_who_sees_the_channel moved to
schedule_tests::configuration_authority::schedule_management_separates_configuration_from_human_view;
ordinary-member refusals and private run/history refusal assertions are retained,
with configuration positives now matching the new policy. Before inventory is the
retained B1-R2-after-tests.txt; confirm the replacement in post-build discovery.

All-target offline checks and clippy passed before the final pure HTTP-test helper
decomposition. Current targeted build/run B2-focused-initial.txt selects new
delegated_admin_/manages_unit_, the updated schedule authority contract and the
strengthened real ingress race on owned openbot_p06_b2 with network isolation.
Source is frozen for this author check; collect its actual result before editing.
No B2 implementation acceptance yet; broader affected-interface, metadata and
reachability checks and a complete frozen B2 review remain required.

### Root B1 acceptance

Root independently recomputed the exact c5286ce1…35442 digest with the unchanged
exclusion list and validated R2 retained evidence. B explicit PASS
06DA-P01-B1-R2/N1 resolves B1-F1/F2; current affected checks pass. The raw Trail
lexical failure has an explicit independently confirmed false-positive disposition,
not a green command. Local coordination r2 and producer r9 dispositions remain as
recorded. **B1 accepted in phase** at this identity; not integrated with Plan05,
not complete p01/Plan06 and not final full-suite acceptance. Reviewer is separately
reviewing D2/r1 while the same source remains frozen. No B2 source edits yet.

### B2 resume and affected-interface diagnosis

Recovered B2 source beyond the stale HANDOFF: initial30focused cases passed. A
new binding-service regression was also present and had an incorrect ToolKind
module path; corrected to its real application publication owner (no assertion
change). Broader affected-interface run retained asB2-affected-interfaces-corrected.txt
executed645cases:638PASS/6FAIL/1ignored. Five failures are old application fixtures
whose shared participant fake treated absence of a View principal as absence of
the separate management membership. Its management implementation now reads the
actual TeamFixture membership, preserving Owner/Admin/Member decisions and explicit
outage propagation; it does not model subtree authority. Real DB tests prove that.

The sixth exposed an intentionally changed binding configuration outcome: root
admin may now configure a private desk via get_company_channel without View.
Replaced that old refusal assertion with explicit Saved binding while retaining
all tab/form/card View refusal assertions. New name
binding_configuration_does_not_grant_conversation_view_in_the_tab_form_or_card
replaces channels_an_admin_cannot_see_stay_out_of_the_tab_the_form_and_the_card;
UI configuration list migration still belongs to02. Existing decisive privacy
assertions and tenant tests are unchanged. No production authority fix was needed.

Current correction run covers these cases, the shared fake's address-resolution
consumers, new delegated/predicate cases, schedule contract and real ingress race.
B2-sqlx-prepare passed with unchanged metadata; clippy/offline all-target checks
passed before this test-only correction, and are rerun before freeze. The source
reachability check is still running. No source review/acceptance claimed.

The49-case correction run passed48and exposed a real inherited root-only preflight
in ordinary BindingService activation/pause: load authorized Company before it
resolved the existing scoped binding. Removed that redundant root check, retaining
actual company-scoped binding load, exact revision/identity checks and actual
association authorization; direct persistence already rechecks the same unit.
TraceB2-activation-readiness-callers and current binding load callers retained.
This is the same scoped-existing-target protocol already gated in D2; no new
permission/model decision. Current B2-corrections-final covers binding lifecycle
and real service configure/activate/pause in addition to the affected cases.


### B2 frozen review input / executed mapping

Source5428a2e367cb3f27321be4b9d6547151f3edb0824c853f906216f618468628c3,
same start/base/HEADd748c2ff, exact two established exclusions. Source is FROZEN
for B review. Local coordinationr2and producerr9byte-pinned for this round;
p03stillplanned, no import and no applicable new notice. D3/r1proposal is an
independent next gate, not source acceptance or B3implementation authorization.

Current246testsPASS: desk CRUD/moves, sibling/foreign direct adapter boundaries,
JSON journeys, schedule configuration/listing withoutView and actual run-now/history
View refusals, both desk revocation orderings and schedule contention; binding
configure/activate/pause actual captured Message service and all binding lifecycle
regressions; shared fake's channel/address-resolution consumers; inherited department,
native grant/model/provider boundaries where selected. Exact assertions and tests
are in their production-path test sources and retained B2-corrections-final output.

Locked offline all-target compilation/clippyPASS; SQLx preparePASS/no cache diff;
fresh B2schemaPASS; source_modules8PASS (no module graph changes since); format/
whitespacePASS. Inventory4765->4778with13newtests and two exact discoverable
renames (schedule authority and binding config/View); logs/B2-discovery-delta.txt.
The earlier638unaffected PASS/6FAIL/1ignored run remains FAILin manifest; all6
failed cases corrected and exercised in246currentPASS. Historical48/49failure
also retained and fixed. The ignored Cloudroom fixture remains unexercised.
RawTrailcheckFAILis retained with B1's independently confirmed teardown lexical
false-positive disposition, for B to recheck against the actual new delta.
Manifest06DA-P01-B2-R1.evidence.json retains9PASS/3FAIL/1NOTEXERCISED.
No whole-phase/whole-plan/final-suite acceptance is claimed.


### Next B3 architecture input (not implementation)

D3/r1 was held before B review after tracing initial-publication/placement order
and real Builder requester dispatch. Revised immutable input
logs/06DA-P01-D3-R2-decision.md includes actual intended membership before initial
publication, existing guard reuse, deferred grant/gate finalization, all three
creation callers and per-kind Builder authorization. Both B2acceptance and an
independent D3/r2PASS are prerequisites of B3edits. No new source changes.

### B2 R1 independent findings (source still frozen)

B saved06DA-P01-B2-R1/N1CHANGESREQUIRED, mediumB2-F1/F2. F1: after removing the
root-only load preflight, check_state exposes a stale-revision Conflict before
actual association authorization. Correct by authorizing the loaded actual scope
before revision comparison, with unauthorized current/stale/missing activate/pause
and authorized-stale controls. F2: the D2promised actor-account-before-company
prefix is absent in newly migrated public desk/schedule entry transactions.
Root confirms the contract is intentional; use existing company_level_authority
account-start protocol at those public boundaries (never indiscriminately inside
nested runtime authorization). Add real account-lock versus writer competition,
proving the company lock remains independently acquirable while writer waits on
actor; keep bounded edit timing and gate-before-desk-row order. Collect all B2
findings and the separate D3gate before unfreezing, then one complete correction
recheck. B3remains unimplemented and ungated until B's D3return.

### B2 correction implementation / D3 accepted decision

B independently PASSed D3/r2architecture/proof plan only,06DA-P01-D3-R2/N1;
root accepts that exact decision while B3implementation still waits for B2.
Source unfrozen after both B2R1and D3rounds completed. F1now authorizes the actual
binding association before comparing state; new real activate/pause tests cover
unauthorized matching/stale/missing and authorized stale/no-state-change controls.

F2public desk create/delete and all5schedule writers now reuse existing account
starter via management::begin (a non-async forwarder, no extra async frame).
Desk edit locks actor inside its existing budget before company/ownership/gate;
its deadline/error behavior is preserved. Public binding persistence also uses
that entry prefix because its actor FK could otherwise take an account lock after
the company; nested _on runtime guards remain unchanged. Added actual competing
account-lock tests for all3desk writes,5schedule entries and direct binding write:
observe exact account blocker, independently acquire company NOWAIT, release actor
and require a successful committed writer. All use owned DBs and joined futures,
no detached workers or claimable shared residue. Initial compile caught module
path company_level_authority versus actual company_invite::authority; fixed path,
retained failed output. Current all-target corrected check pending before DB tests.

Current correction188testsPASS, including all four new F1/F2regressions and the
public binding entry prefix addition, full relevant schedule/binding edit and
inbound gate behavior. SQLx regenerationPASS/no cache changes; corrected offline
all-target and clippyPASS. Discovery/source-module checks follow sequentially in
B2-R2-after-tests/source-modules; no offline cache regeneration overlap. No source
edits since these inputs. D3future consumer trace additionally identifies public
publication capture preflights (step_builtins, agent/child/http/mcp publication and
sample capture): disposition each against its actual definition or unchanged root
resource policy during B3. Inventory retained in B3-workflow-root-authority-consumers.
This is impact tracing under accepted D3/r2, not a new authorization exemption.


### B2 R2 frozen correction / actual evidence

Sourcef90321fd26058d076596f6a266282ef5a72ff8d4c099acb523594d4af7972d0c,
same start/base/HEADd748c2ff and exact two established exclusions. SourceFROZEN
for single complete B2-F1/F2correction recheck. Localr2/producerr9byte-match prior
pins; copied into roundR2logs. No Plan05import or new applicable contract.

Current188testsPASSincluding all4new enduring F1/F2regressions, direct actual
activate/pause unauthorized current+stale+unknown404 and authorized-staleConflict,
company availability while real desk CRUD/schedule writes/binding writer block
on actor account, actual ingress/edit timing/gate races, schedule View and binding
lifecycle/native authority cases. SQLxPASS/no cache diff, all-target offline/clippy
PASS, source reachability8PASS, inventory4778->4782only4additions/no removed
tests, format/whitespacePASS. FreshB2schema unchanged fromR1; retained migration
proof reused. R1proof may be reused only for unchanged interfaces/assertions; current
correction paths have current188case evidence. Failed compile/exploratory logs
remain retained; original R1failures remain in their historical manifest.

R2manifest validates9PASS/1FAIL/1NOTEXERCISED. RawTrailcheckFAILremains unchanged
teardown-learning lexical noise (independently dispositioned inB1/R1; B must confirm
the correction delta changes no teardown constraints). Complete phase/plan/final
repository gate remains NOTEXERCISED and unaccepted. D3/r2architecture already
accepted; dependent B3implementation still waits for explicit B2R2sourcePASS and
root matching-digest/evidence acceptance.


### Root B2 acceptance and B3 entry

Root recomputed exactf90321fd26058d076596f6a266282ef5a72ff8d4c099acb523594d4af7972d0c
with unchanged exclusions and validated retained R2evidence. Independent
06DA-P01-B2-R2/N1PASS resolves B2-F1/F2; no unresolved B2blocking findings.
B2is ACCEPTED IN PHASE only, not integrated or complete p01/Plan06. D3/r2
06DA-P01-D3-R2/N1architecture PASS/root acceptance is prerequisite for B3.
B3now starts from accepted B2, with baseline logs/B2-accepted-source.snapshot.tar
for delta review; base/HEADstilld748c2ff. User stop-at05/ph03and unchanged localr2/
producerr9scope remain inforce. Implement full D3/r2and its actual proof mapping,
then one whole-batch review. Knowledge01B.6is next; stop before its edits.

### B3 intermediate source / narrowly reopened D3/r3 gate

Definitions schema/predicate, typed creation versus existing management, scoped
public save/copy/publish and capture preflights are implemented but untested. Fresh
owned openbot_p06_b3migrationPASS; initial definitions and capture all-target checks
PASS. General trusted native insert defaults root; public creation explicitly
stores the canonical sole non-root creating department (root otherwise). SQL tenant
FK is deferred and immutable, no backfill/compatibility. Old B2 DBs no longer match
this migration and must not be used for B3checks. New source does not change05.

Added AgentPersistence explicit library method declarations and test-only raw
mutator declarations, plus scoped delete declaration on AgentDraftPersistence;
implementations are pending, so current intermediate source is not expected to
compile until that authorized batch work is completed. No false compile claim.

Found r2's application preflight dependency assumption wrong: AgentUseCases has
NO participant persistence handle. Before adding/replacing the dependency,
reopened a narrow architecture gate D3/r3, immutable logs/06DA-P01-D3-R3-decision.md.
Proposes reusing its existing CompanyPersistence via required AdministrationPersistence
supertrait, no constructor/service changes. Actual prod already has common SQL;
shared mock must explicitly model root membership/outages, never hierarchy proof.
Source FROZEN for this decision review; no dependent port/preflight edits until PASS.
All other r2 decisions and user stop-at05/ph03remain. Required B3proof unexecuted.

### D3/r3 review and corrected r4 decision

Independent D3/r3CHANGESREQUIRED,06DA-P01-D3-R3/N1, medium06DA-P01-D3-F1.
CompanyPersistence supertrait proposal is acceptable; the identity-free creation/
copy realization is not. It can reveal an existing unmanaged create conflict, and
HTTP copy conflict replay can return an existing revision after current authority
is lost. Root accepts finding. No dependent code/port edits since frozen ca53630f.
Corrected immutable input logs/06DA-P01-D3-R4-decision.md carries destination identity
through create/copy, uses existing authority on collision and creating unit only
for actual absence, and explicitly reauthorizes HTTP conflict replay before read.
Real adversarial/direct/competing and authorized idempotent controls are required.
R4supersedes only r3's rejected typed realization; other r2andCompanyport rules stay.


### D3/r4 acceptance and B3 implementation continuation

Independent06DA-P01-D3-R4/N1PASS resolves D3-F1at decision level. Root reproduced
ca53630f…c022264 before accepting; source then unfrozen. Identity-bearing creation/
copy now selects existing current authority on collisions, true absence only for
creating-unit authority. HTTPcopyConflictfallback explicitly checks existingDef
before read. CompanyPersistence now requires commonAdmin on existinghandle; shared
mock explicitly derives current root membership/outages. Unit traversal proof stillDB.

Agent commands now scoped: apppreflight targets actualplacement/Agent/Root and
PGdraft/delete recheck via existing locked publication scope. Raw company update/
delete are cfg(test)only; production library methods explicitly lock companyNULL
rows. Added scoped managed-agent list through existingAgentPersistence, using one
SQLagent_management_role projection shared with target and directory checks, rather
than per-agent application queries. Bounded authoring and Builder pages filter
before LIMIT. SchemaexistingDef activeagent bindings use that same projection.

Constructor writes actual intended membership before first publication, defers
role/grant/reviewer gates until construction ends. Explicit Unplaced/Complete/
Deferred placement covers all3callers; identity/publication phase boxed at its seam.
Public CompanyAgentService targetchecks before library work. Placement phases moved
to department/placement.rs to keep parent below1000lines; no test moves. Builder
source/run/requester fences retained, entryAnyUnit plus actualCreateAgent target,
scopedListAgents, Root-only defaultCreateDesk. Listing extracted to bounded
provisioning_directory.rs with namedDBrow (old6tuple removed) and short readphases.
Initial constructor check caught an accidentally removed name borrow in extraction;
fixed, failed log retained. Current corrected all-targetcheck passed before final
HTTPconfiguration-read/catalog scope change. New alltargetownercheck pending.

Local coordinationmetadata revision3updates p01status/worker only; planwideinstructions
and notices unchanged. Prior r2reviewpins remain valid requirements history, no
applicable contract change. B3reviews will pinr3. No05artifact import. FreshB3schema
must be recreated under newowned name after finalSQL changes (oldb3isoldchecksum).

### B3 behavioral implementation and checkpoint verification in progress

D3/r4 was collected as explicit PASS (06DA-P01-D3-R4/N1); no additional gate is
needed for routine implementation of its accepted decision. B3 now implements the
entire definitions/agents batch, including identity-bearing creation/copy, current
replay authorization, CompanyPersistence's required Administration supertrait,
scoped agent commands/direct deletion, library-only mutators and actual placement
before initial publication. All three constructors retain last-in-order placement
finalization; Builder independently verifies the genuine requester/per-kind target.

New definition proof: first run 14 PASS/1 FAIL. Unit deletion returned an internal
FK error for retained definition ownership. Extended the existing department guard
to return Conflict and retained the FK as final enforcement. The corrected run
passed all 15 definition cases, including old/current/inactive bindings, ANY active
binding ownership, unbound recorded ownership, immutable/tenant/default ownership,
actual save/publish/archive, direct prepared writers and captures, create collisions,
copy replay after authority loss, and both real role/binding revocation orderings.
The previous failure is retained in B3-definitions-focused-initial.txt.

Broader B3-delegated-corrected-v2 run: 58 PASS/3 FAIL. All definition/predicate,
B1/B2 delegated and agent revocation races passed. Three new fixtures corrected:
JSON creation/update must send required native_permissions="full"; authoring
read fixture explicitly configures agents.test; revoked capability grants remain
retained, so their deletion refusal is asserted and a fresh agent proves successful
managed deletion. These preserve production contracts rather than remove assertions.
Initial unavailable script invocation B3-delegated-corrected.txt is retained; the
actual repository network guard is scripts/test-network-isolation.sh.

Added actual production Builder runtime journey: signed-in ordinary root member
with a unit Lead creates in that unit, lists only managed agents, fails sibling
placement and root-default desk creation, and same verified person's email receives
no administration authority or agent list. Native binding actor/run-as and actual
member placement must equal that requester/unit. Two additional public construction
versus role-removal competitors prove atomic identity/membership/publication/native
binding/grants and stale-role refusal in both orders. The independent grant-fault
rollback test proves early membership/publication are removed on finalization error.
No knowledge/Plan05 contracts touched. Current B3-delegated-final run pending.

B3 delta compared to accepted B2's immutable source archive is retained in
logs/B3-delta-paths.txt (50 files currently, before later routine proof corrections).
No tests moved or deleted; after-discovery comparison remains required. Current
source is NOT frozen/accepted; whole-B3 review follows all mapped author checks.
Local coordination r3 and producer r9 are current, no active notice change/import.
Knowledge01B.6 remains the next numbered owner and mandatory user-requested STOP.

### B3 final verification mapping and consumer dispositions

| Criterion | Actual production path and discriminating assertion | Enduring proof / retained check |
| --- | --- | --- |
| Definition ownership | Actual lifecycle save/validate/publish/archive stores the sole creating unit; root/multiple-unit fallback, immutable/foreign ownership refuse; retained-owner unit deletion Conflict and tenant teardown commit | delegated_definition_tests + integrity; B3 migration-final, focused runs |
| Bound ownership | EACH manager of two active binding units succeeds, recorded-only manager refuses while bound; old revisions and inactive bindings do not confer authority; last pause returns ownership to recorded unit | manages_unit_definition_uses_any_active_binding_and_recorded_unit_only_when_unbound; delegated_admin_definition_old_binding_revisions_confer_no_authority |
| Creation/replay trust boundary (D3-F1) | expected=None collision cannot expose unmanaged state; prepared direct saves/copies and capture use current actual target; same template key after demotion or binding transfer refuses without revision/event changes; authorized conflict/replay controls succeed | delegated_definition_{boundary,replay}_tests; actual JSON/lifecycle/persistence paths, no fake authorization proof |
| Current authority under competition | Public definition publication vs binding pause/role removal, and public agent draft/delete/create vs role removal execute both deterministic orders; either complete authorized write or NotFound with no partial result | delegated_definition_race_tests; delegated_agent_race_tests; exact blocking backends and joined futures |
| Agent ownership and library separation | Actual AgentUseCases/CompanyAgentService and direct scoped draft/publication/deletion, tools/skills/revoke, managed lists/directory; ANY placement, unplaced root fallback, sibling/foreign/member/agent refusals; library-only mutations cannot touch company rows | delegated_agent_management_tests and delegated_agent_api_tests; retained grants still block deletion; fresh agent deletion succeeds |
| Builder requester | Production ingress+workflow/model/action runtime creates as the authenticated Lead, lists only its agents, refuses sibling placement/root-default desk, same person's email obtains no authority/list; native binding actor/run-as equals requester | delegated_builder_runtime_tests; inherited builder_refuses_members_and_emailed_requests and requester lock tests |
| Constructor ordering | Public create final grant failure rolls back identity/membership/publication/native binding; competing construction/removal asserts complete commit or no creation; all three actual constructors preserve final gate phase | delegated_agent_construction_tests; delegated_agent_race_tests; actual Builder/onboarding regressions; exact production_calls structural guard updated for extracted identity phase |
| Inherited effects/read policy | Agent native direct address/publication, skill/tool selection, classification/resources, onboarding, capture wrappers, workflow route/lifecycle/authoring paths retain existing effects/refusals; configuration grants no conversation View | B3-affected-interfaces selection; new no-View tests and current inherited assertions |
| Repository checkpoint | Current schema applies on fresh owned DB; SQLx cache matches macros; offline all-target/check/clippy, test reachability/discovery, format/whitespace, graph/learnings | Final B3 author checks to follow. Full Plan06 final script/stack/lease/browser/service gates intentionally pending original remaining scope and 05/ph03 |

Source contract impact remains the accepted D3/r2+r4 inventory plus
B3-{agent,company,definition,management-target,workflow-root-authority}-* consumer
traces. All public definition captures use the same identity-bearing creation/
existing authority; native trusted constructors preserve their separate existing
contracts/root default. Existing-only lifecycle targets never use missing creation
fallback. Other workflow readers/runtime origins retain their prior policy. Agent
CompanyPersistence capability is explicit on both real and shared mock impls;
shared root-only mock tests prove propagation, not hierarchy. Authoring and Builder
queries apply the common agent predicate before LIMIT, preserving existing bounds.
Existing root credentials/model/company controls and ordinary agent metadata/View
consumers are unchanged. No new dependency package is required; missing producer
is the accepted Plan05 scope implementation, not a crate/tool installation.

### B3 broader run and complete correction set

B3-affected-interfaces.txt exercised822 selected tests:805PASS/9FAIL/8ignored.
The eight ignored fixtures remain NOT EXERCISED (browser export/manual harness
requirements), never counted as executed proof. All nine actual failures are
accounted for; none was labeled unrelated or silently removed:

- Exact structural production-call assertion still named the old constructor;
  updated its two entries to insert_company_agent_identity_on, retaining the exact
  primitive/caller set and all other expected entries.
- Directory unknown-department message drift: restored the existing refusal text
  for both missing and unavailable units, preserving no-existence disclosure and
  filtering policy. The existing exact assertion remains unchanged.
- Six real Cloudroom peers were absent (each failed its explicit script-is-file
  prerequisite before executing). npm ci installed55 locked plugin dependencies,
  no source/lockfile diff, and built18 existing test peer entries. Initial build
  ran from wrong cwd and failed; retained B3-plugin-build-fixtures.txt and corrected
  workdir result B3-plugin-build-fixtures-corrected.txt. Local Node26.13.1/npm11.20.0
  differ from package engine24.21.0/11.19.0, warning retained; this is fixture proof,
  not final pinned-runtime/package acceptance.
- The actual new Builder journey passed creation/listing/refusal assertions then
  its fixture's root-desk absence query referenced channels.slug (no such column).
  Corrected the assertion to addresses with target_kind='desk'; no production
  behavior changed. Email refusal assertions still require current executed proof.

All corrections are together in one author recheck. Current sequential exec27296:
format/whitespace, live SQLx prepare on fresh owned openbot_p06_b3_gate, offline
alltargetcheck, clippy, affected delegated/directory/structural/peer tests, source
reachability/discovery and graph/check; individual logs/status retained. Fresh
B3-migration-final.txt PASS. This does not run or claim the whole-plan final gate.
Earlier805PASS evidence may be reused only for untouched production paths and
assertions; current proof must exercise every failed or materially changed branch.

Current correction proof: B3-corrections.txt75PASS/0FAIL/3ignored, including all
nine previously failed cases, actual Builder email refusal and all six rebuilt
Cloudroom peer journeys. Format/whitespace/liveSQLx/offlinealltargets/clippy PASS;
SQLx cache unchanged. Source-module/inventory/graph checks pending current script.
Source candidate831c4502e070a2fa76966cc2ccdec073f50f34dea936055963fcfb1ab1566d6a,
unchanged exact two exclusions, is not yet review/acceptance. No source edits since
this candidate was captured. Focused existing2048KiBstack proof will precede freeze.

### B3 R1 frozen whole-batch review input

Source831c4502e070a2fa76966cc2ccdec073f50f34dea936055963fcfb1ab1566d6a,
start/reviewbase/HEADd748c2ff64309903449a3d860377a5e0385377a4; same exact exclusions
(p01docs and exactCOORDINATION.md). Source FROZEN for independent whole-B3review.
52-file delta from acceptedB2 retained in B3-versus-accepted-B2.diff.txt and
B3-delta-paths-final.txt, with full frozen dirty-source archive
B3-frozen-source.snapshot.tar SHA256
cc560a97d7d344766ff58bbc725abbc86181b93c422f98271b8ec5b010f62d53.
No preserved outside-task changes. Localr3/producerr9 pins match current authorities;
no notice requirement changed and no05artifact imported.

Current75PASS/3ignored correct all9 earlier failed assertions. Historical broader
805PASS/9FAIL/8ignored retained; the805 unaffected assertions remain reusable only
where production paths and fixtures are unchanged. Current focusedstockstack8PASS
at unchanged2048KiB, including actual agent create/copy/edit/publish/delete and
Builder runtime. Source-module reachability8PASS, alltargetoffline/clippy/liveSQLx/
freshschema/format/whitespacePASS. SQLx cache unchanged. Comparable lib discovery
4782->4810 (+28/no removals); alltarget inventory additionally includes13unchanged
bin/source-module tests absent from olderlib-onlyinventory. No renamed/moved tests.

RawTrailcheckFAIL is retained: unit_management_role's comment/function tokens
"order department principal" lexically match an old teardown learning. This query
adds configuration authority, does not reorder/drop peer FK cleanup, and the new
owner FK is deferredNOACTION with actual teardown regression proof. B1/B2 already
independently confirmed this same lexical false positive; B rechecks B3delta.
This is not relabeled PASS. Final Plan06 script/CI/stack/lease/browser/service gates
and ignored fixtures remain NOTEXERCISED at the required dependency checkpoint.
Manifest06DA-P01-B3-R1.evidence.json validates13PASS/5FAIL/1NOTEXERCISED; one early
historical definition test command's exact shell filter was not retained, disclosed
as historical evidence rather than invented. Actual current commands/logs retained.
B3 source is not yet accepted; independent review and root matchingdigest required.

### Root B3 acceptance and required dependency stop

Independent06DA-P01-B3-R1/N1PASS accepted source
831c4502e070a2fa76966cc2ccdec073f50f34dea936055963fcfb1ab1566d6a.
D3-F1 is FIXED at implementation/evidence level; no new findings. B independently
ran39actualDBtests, allPASS/noneignored, includingall28newtests. Root reproduced
exact fullsource/base in B3-root-acceptance.snapshot.txt and validated BOTH author
and reviewer evidence manifests. Currentlocalr3/producerr9byte-matchpins; all
applicable instructions satisfied for independentB3. Required acceptance records
are saved. B1/B2/B3 are ACCEPTED IN PHASE; no wholep01/Plan06 acceptance/integration.

User-requested STOP reached at01B.6knowledge/memory. Required accepted producer is
Plan05ph03A+03B (p02/p01transitive), not yet available in producercoordinationr9.
NoPlan05source/artifact imported and no knowledge-scope stand-in implemented.
Remaining01B.6/01B.7, full02routes/UI/audit,03docs/integration/finalgates remain.
Do not silently continue other phases. Original remaining scope, ignoredfixtures,
Node/npm pinned-runtime difference and finalrepository/package gate stay pending.
RawTrailFAIL remains an independently confirmed semanticfalsepositive, not PASS.

Local coordinationmetadata advances tor4 solely to record acceptedB3and stopstate;
no sharedcontract/notice/userrequirement changed since B'sr3pin. Future work resumes
from this exact accepted source after rechecking authoritativecoordination and
acceptedproducerartifact identity. No commit/push/deploy/sharedDBreset authorized.
Independentreviewer completed, noownedtest/serverworker running. Ownedprivate
PostgreSQL18 cluster stopped successfully (B3-owned-postgres-stop.txt); its data,
allownedDBs, targetcache, pluginfixtures, source and evidence retained for resumption.
No unrelatedprocess/resource touched. This concludes only the authorized independent
checkpoint, not the complete original Plan06 implementation.

## 2026-11-18 handover to Claude Code / implement-opus-reviewed-2_4

- Workflow now implement-opus-reviewed-2_4 (records shared across variants within p01).
  Root/A: Claude Code root, session b36d9dc4-495e-4131-9804-05ebcc381915, Opus 5.5,
  effort xhigh confirmed by the user. B: Opus 5.5 / xhigh, general-purpose, no
  inherited context (user chose Opus over the skill's Fable/medium; Fable alias gives 5.1).
  Prior Codex root and its reviewer /root/p06_reviewer are finished; not reused.
- Compaction: CLAUDE_AUTOCOMPACT_PCT_OVERRIDE unset; user declined to add it. Default
  auto-compaction, never claimed as 75%.
- Coordination authority r5 (pin logs/B4-coordination-r5.md, sha256
  6bc308a75150785346dce0964cfa0b02e0bef7faf76d98a5fe45cbfca858d5df). No notices.
- Git (user-authorized): WIP commit d7d81ad8 of accepted B1–B3 (source snapshot tars
  left untracked); merge of origin/main be1b504b (Plan05 complete) as 174c83b3.
  Plan05 producer contract is now the published main source, not a dirty tree.

## Batch B4 — Plan05 integration, 01B.6 knowledge/memory, 01B.7 invites

Not architecture-gated: knowledge and invite changes reuse the accepted D1 predicate
(unit_management_role) and the existing company-serialized transactions; no schema,
transaction-boundary or lock-order change. Reviewed together in one batch review.

**Merge resolutions (174c83b3, see `git show --remerge-diff 174c83b3`)**
1. init schema: both tails kept (Plan06 definition management functions, then Plan05
   run scope snapshot tables/guards). Fresh migration + live SQLx check pass.
2. department creation: Plan05's `insert_department_on` (creates the unit memory
   namespace) with Plan06's lead-before-desk order (B1 decision: the lead's desk
   grant needs the role first).
3. agent preparation: Plan06 `authorize_model_source_on` then Plan05
   `require_consumed_units_on`; both checks are independent refusals.
4. Plan05's new fixtures (8 call sites) use Plan06's actor-checked
   `ChannelPersistence::create/delete` via `test_support::{create_channel,company_owner_actor}`.

**B4 decisions**
- K1 (user): manual knowledge Permissions for an ordinary person add every unit where
  `unit_management_role(actor, company, unit)` is not NULL to BOTH read and write
  selections (src/adapters/persistence/knowledge/permissions.rs `ordinary`). Bounded by
  MAX_RUN_READ_UNITS with the existing generic scope-limit error. Owner/admin remain
  `All`. Level ceilings, hidden-layer and collision rules unchanged. Frozen workflow
  run scopes (`run_scopes`, scope snapshots) unchanged. The one-step lead-children
  write insert is removed as subsumed (children of a led unit are always managed);
  the lead-children facts still feed `run_scopes` read computation unchanged.
- K2: knowledge writes already lock_team (company) inside the same transaction as
  role writes' company lock, so stale-role commits are impossible; proven by
  competing tests in both orders.
- M1: memory settings stay root-managed (owner/admin) — existing behavior; added a
  delegated regression that a unit lead is NotFound for enable/disable.
- I1 (user): `CompanyMembership::may_invite_as(role)`: owner any role; admin only
  Member; others none. Use case (create/update/delete) and persistence recheck
  (create_invite/update_invite under lock_team) both enforce it over every role the
  write touches: create = requested + any existing invite for that address (upsert);
  update = previous + requested; delete = existing (use-case only; persistence
  delete_invite has no actor, pre-existing). Membership level ceilings unchanged.
  update_member_role/remove_member stay owner-only (root roles, D1 item 5).
- Department placement of an invited person = department manager: already B1
  (add_member requires managing the department); no change.
- Regression fix (found by M1 full run): Plan06 B1 made workflow/authorization.rs
  import `use_cases::administration`, violating the shared-port boundary test
  `workflow_and_transport_depend_on_shared_ports_without_reverse_usecase_imports`.
  The module moved unchanged to `application/ports/administration.rs`; 50 references
  updated. No behavior change.
- Intentional test changes: Plan05 `knowledge_scope_search_manual_roles_ancestors_one_step_leads_and_level`
  renamed `..._managed_subtree_and_level`; lead-of-A now lists/searches {root,a,b,c}.
  `knowledge_scope_upload_requires_current_placement_and_parent_lead_without_sideways_grants`:
  grandchild C now writable by lead of A; root/foreign still NotFound.
- Deferred within plan: `manages_company_operations` removal (01A.2) — remaining
  consumers are root-level operations and UI/list filtering owned by 02A; removal is
  done with 02A's consumer dispositions.

**B4 acceptance mapping**

| Criterion | Production path/case | Decisive assertion | Evidence | Status |
| --- | --- | --- | --- | --- |
| 01B.6 subtree knowledge | lead of A on D (3 below): list/search/replace/move/delete | D listed & searched; replace/move/delete succeed; L6 layer Forbidden & untouched | delegated_admin_knowledge_lead_manages_the_whole_subtree_within_its_level | PASS |
| 01B.6 no sideways/upward | sibling, root, other company | replace/move/list/delete NotFound; sibling doc remains | same | PASS |
| 01B.6 revocation | lead removed | D replace/list NotFound | same | PASS |
| 01B.6 competing | upload vs lead removal both orders (real writers, barrier proves waits) | upload-first commits then removal; removal-first upload NotFound, no row | delegated_admin_competing_knowledge_upload_precedes_role_removal, delegated_admin_competing_role_removal_precedes_knowledge_upload | PASS |
| 01B.6 memory root-level | unit lead enable/disable | NotFound; tenant unchanged | delegated_admin_memory_settings_stay_with_root_managers | PASS |
| 01B.7 invites | admin member invite create/update/delete; admin-role create/promote/overwrite/demote/delete; member | admin member ops succeed; admin-role ops Forbidden in use case AND persistence; owner's admin invite unchanged; member refused | delegated_admin_invites_root_admin_writes_member_invitations_only | PASS |
| Plan05 integration | merged tree | full lib suite on merge baseline | logs/M1-merge-lib-tests.txt: 4846 PASS/5 FAIL/30 ignored | see failures |
| Boundary regression | dependency scan | no use_cases import in workflow/transport | workflow_and_transport_depend_on_shared_ports_without_reverse_usecase_imports in B4-focused-final | PASS |

**Contract impact**

| Changed contract | Consumers | Disposition | Checks |
| --- | --- | --- | --- |
| knowledge Permissions read/write | list, unit_choices, preflight_search, search_manual, preflight_upload, replace, delete, move (persistence/knowledge mod/rows/search/moves); HTTP knowledge routes/pages, department pane knowledge | all consume the same selections; widened only for managers | knowledge (persistence+HTTP+pages+workflow knowledge) in B4-focused-final |
| CompanyInvite authority | use case create/update/delete; persistence create_invite/update_invite; HTTP company invite routes/team UI | admin allowed for member invites only | company_invite, ui_team_authorization, company_access_level, company_owner families |
| administration port path | 50 imports | path only | full all-target clippy |

**M1 merge-baseline failures (logs/M1-merge-lib-tests.txt; reruns logs/M1-isolated-reruns.txt)**
- workflow_and_transport_…reverse_usecase_imports: Plan06 regression, FIXED in B4.
- security_auth_limits_login_counter_is_shared_and_success_resets_it and
  security_auth_limits_unicode_registration_aliases_share_budget_and_code_digest:
  deterministic on the owned cluster; it was initdb'd with C locale
  (`lower('Älice')`='Älice' there, 'älice' on the system en_US.UTF-8 server). Files
  untouched by Plan05/06 since d748c2ff. Environment-unrelated; final gate must use a
  UTF-8-locale cluster.
- cloudroom_e2e_cut_points_…: timeout under full-suite load; PASS isolated.
- doop_agent_current_preparation_reaches_ready_before_model_io: asserts prepared
  version 7; Plan05 on main set PREPARED_AGENT_VERSION=8 and ran with Doop filtered.
  Pre-existing on origin/main; Doop deferral preserves it unchanged.

**B4 frozen artifact**: base 174c83b37a7ed4156abe15dbf3870a0e04f68f68, digest
2c9a298f7c0fb765c1fbfd6858d2c80a245039f9d4699b1de7e39647463b691b, exclusions
plan/06-delegated-administration/phases/p01/docs and
plan/06-delegated-administration/COORDINATION.md (logs/B4-author-freeze.snapshot.txt).
Review scope: merge commit 174c83b3 resolution (vs both parents) plus working-tree delta.
Checks (owned PG18 127.0.0.1:55406, DB openbot_p06_merge05, SQLX_OFFLINE=true):
fmt PASS, diff --check PASS, sqlx prepare --check PASS, source_modules 8 PASS,
focused 562 PASS/0 FAIL/2 ignored (B4-focused-final.txt). Discovery 4881→4886:
+5 new, 1 rename, 0 removed (B4-discovery-{before,after}.txt).
Next: independent B4 batch review (06DA-P01-B4-R1).

### B4 R1 findings and correction (R2)

B4-R1 (note 06DA-P01-B4-R1-N1): CHANGES REQUIRED. All five findings are dispositioned
in one correction:
- F1 (medium) FIXED: `CompanyInvitePersistence::delete_invite` now takes
  `DeleteCompanyInvite{actor_id,company_id,invite_id}`; persistence locks the account,
  the invite row (company-scoped) and the company, rechecks `require_inviter([role])`,
  and deletes `WHERE company_id AND id`. Callers updated (use case, use-case mock,
  two persistence tests). The removed `query!` drops one .sqlx file (regenerated).
- F2 (medium) FIXED: create_invite authorizes against `authority::read_invite_by_email`
  read after `lock_team`. All role/level writers hold the company lock, so the read is
  stable; a plain read avoids re-locking the row after the company (accept takes
  company then invite row). Real competing tests, both orders, via a new
  `WriteBarrier::CompanyInvites`.
- F3 (low, under user decision b) FIXED: `authority::refuse_admin_address`: a
  non-owner create/update addressed to a current root admin is Forbidden (acceptance
  would demote them). The owner may still address one.
- F4 (low, under user decision b) FIXED: `set_invite_access_level` rechecks
  `require_inviter([role])`. The recheck in it and in update_invite now runs after the
  pending-status check, so a closed invitation stays "no change" (None). The existing
  race `access_level_acceptance_and_own_invite_assignment_do_not_deadlock_or_restore_levels`
  exposed the order: acceptance demotes the acting admin before the edit locks.
- F5 (low) FIXED in records: the "Contract impact" invite row is corrected. Team UI
  invite forms are still owner-only (companies.rs); exposing them to admins and admin
  delete buttons is carried to 02A.

New tests: delegated_admin_invites_persistence_rechecks_delete_level_and_admin_address,
delegated_admin_competing_owner_admin_invite_precedes_admin_member_upsert,
delegated_admin_competing_admin_member_invite_precedes_owner_admin_upsert.

R2 frozen: base 174c83b3, digest 4228a622933c9bfbf84f4e55fcc2bd71ff3325f5538c80814ec861e1640544df,
same exclusions (logs/B4-R2-author-freeze.snapshot.txt). Coordination r5 unchanged.
Checks: fmt, diff --check, sqlx prepare --check, source_modules PASS; focused 565 PASS/0 FAIL/2
ignored (B4-R2-focused-final.txt). Discovery vs R1: +3 new tests, 0 removed (B4-R2-discovery-after.txt).

Correction to the R1 "Contract impact" invite row: consumers are use case
create/update/delete, persistence create/update/delete/set_invite_access_level and
the HTTP invite routes. The team UI does NOT expose invite forms to admins yet (02A).

### B4 R2 findings and correction (R3)

B4-R2 (note 06DA-P01-B4-R2-N1): F2–F5 closed; F1 open (low), F6 new (medium), digest
drift from an untracked trail learning added after freeze.
- F6 FIXED: `team_mutations_refuse_members_admins_foreign_and_unknown_company_scopes`
  (src/adapters/http/routes/ui_team_authorization_tests.rs) encoded owner-only invites.
  It now refuses member-actor member invites and admin-actor admin invites, and asserts a
  root admin's member invite through the workspace route succeeds and stores role member.
  Root cause of the miss: focused filters `ui_team_authorization` and
  `company_access_level` matched zero tests (modules mount as `ui_team::authorization_tests`
  and `company_invite::access_level_tests`). Every R3 filter was checked against
  `--list` and matches at least one test; `ui_team`, `invite` and `team` added.
- F1 FIXED: competing test
  `delegated_admin_competing_owner_promotion_precedes_admin_invite_delete` (owner
  promotes to admin while the admin's delete waits; delete Forbidden, role Admin).
- Digest: the trail learning is part of the change. A replacement learning corrects its
  lock-order wording (`...-mac02-2.md`, replaces l-1a464501; both untracked, included in
  the digest, not edited further).
R3 frozen: base 174c83b3, digest ee4c49d0820f1ca469e825507a06b714ec3f308a54a0b2ea417739eb04f320f4, same exclusions
(logs/B4-R3-author-freeze.snapshot.txt). Checks: fmt, diff --check, sqlx --check,
source_modules PASS; focused 593 PASS/0 FAIL/2 ignored (B4-R3-focused-final.txt);
discovery +1 vs R2, 0 removed (B4-R3-discovery-after.txt).

### Root B4 acceptance — p01 complete in phase

06DA-P01-B4-R3/N1 PASS for ee4c49d0820f1ca469e825507a06b714ec3f308a54a0b2ea417739eb04f320f4.
Root reproduced the digest and file list (logs/B4-root-acceptance.snapshot.txt), validated
the author and reviewer R3 manifests, coordination r5 is byte-identical to its pin, and no
finding is open (F1–F6 fixed). B4 ACCEPTED IN PHASE. With B1–B3, p01 (doc 01) is accepted
in phase except the deliberate 01A.2 removal of `manages_company_operations`, carried to
p02/02A with its consumer dispositions. Not integrated, not pushed; no full Plan06 claim.
Remaining unrelated/deferred: C-locale auth pair, Doop version assertion, cloudroom load
timeout (final gate on a UTF-8 cluster). User authorized committing B4 locally (2026-11-19).
Next: p02 (doc 02) in phases/p02/docs.
