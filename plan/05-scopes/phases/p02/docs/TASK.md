# Plan05 p02 — frozen run scopes, memory vocabulary and subject provenance

Status: ACCEPTED IN PHASE at R2 source; local commit and later phase03 publication pending.
Workflow implement-astra-reviewed-2-3. Root/A /root user-confirmed Sol6.1/xhigh;
B /root/p05_reviewer independent requestedAstra/high, retained from p01. Root owns
TASK/HANDOFF/coordination; B owns REVIEW/notes/findings. Context telemetry/default
compaction controls unexposed; preserve durable records.

Worktree /Users/mac02/orca/workspaces/openbot/plan03-04, branch feature/plan05-scopes.
Start/reviewbase/HEAD e6eb5bc8fff93492f23935b9c84114905801824c. Clean p01source at entry; only new p02records and
coordination edits. Prerequisite p01 source0713361f…90dabf, commite6eb5bc8fff93492f23935b9c84114905801824c,
independent05SC-P01-B1-R1/N1 PASS with original failures/corrections retained in01.
Exact p02 snapshot exclusions: plan/05-scopes/phases/p02/docs and exact
plan/05-scopes/COORDINATION.md. Preserve closed01records unchanged.

Authoritative coordination /Users/mac02/orca/workspaces/openbot/plan03-04/plan/05-scopes/COORDINATION.md,
root writer entry revision6, current revision9; ACK C001–C004/r1 and inherited grant/classification/current
identity/fence contracts. User checkpoint: main push after accepted03 and required
source gates, remaining05 docs thenSTOP before06. Doop separate; all future
libtest commands --skip doop --skip Doop. Do not touch another worktree's resources.

## Original requirements / acceptance and proof planning

Original README decisions4–10 and02-scope-resolution-and-tools.md. No backward
compatibility, fresh own schemas and init edits only. C004 service subject assumption
is false in original plan; a separately reviewed narrow opaque service extension
must fulfill propagation/query/purge, preserve department-agnostic service and
existing client credentials/classification boundaries. Do not claim the old API
already carries subject or can list every record.

| Criterion | Required production behavior / decisive proof | Focused checks planned |
| --- | --- | --- |
| AC1 | pure resolver: chain plus one-step lead children, no sideways broadening, DM singleton versus ambiguous/root-only, strict local write; depth8 boundary | run_scopes_resolve_ pure matrix and controlled bound refusal |
| AC2 | same atomic admission snapshot serves steps/tools/knowledge; immutable topology after roster/tree/lead changes; root/child/native MCP creation covered; acting-agent isolation and actual own-session person boundary | real Deployment/admission competing topology/role mutations, two desks, lead/nonlead, DM1/2 departments, missing/foreign snapshot, SQL append/update/delete refusals |
| AC3 | new step/tool grammar and activation refusal for own without valid target; search/timeline scopes personal/unit/ancestors/reports, default all, strict local saves/personal override; labels and host context; ceilings/consumption replay intact | production compiler/activation/runtime/recall/capture/checkpoint tests, oldcompany/user refusal, rootread ceiling and ancestor-write refusal |
| AC4 | visible optional subject persists local and actual service provenance; operator lists/deletes exact subject across owned unit namespaces without tenant/credential widening | real HTTP/service + OpenBot production dispatch/operator tests, foreign/missing subject denial, explicit namespace/tenant/privilege/pagination/competing saves and deletion cases |
| AC5 | focused complete proof, fresh migrations/SQLx for affected crates, fmt/offline compile/reachability and independent review | only affected checks now; one full repository gate after03source before mainpush; no Doop execution |

Existing six added01 test names and no removals documented in01 reviewer collection;
its final test inventory logs/B1-R1-reviewer-test-list.txt is p02 baseline if names
move/rename/remove. Source base unchanged; do not recollect needlessly.

## Traced contracts and unsettled decisions (not implementation)

- External admission_write::create, runtime children::insert_child_on and native_mcp::admit_on
  all use admission_write::insert_run. Qualified consumers were found by exhaustive
  trail grep after call edges missed them. Native broker does not capture an ordinary
  dependency manifest; inspect owner harness and prepared broker context explicitly.
- Children::create_on copies actual parent association/actor/levels; attach_child_on
  shares root dependency manifest. A copied frozen scope set therefore fits existing
  child selection semantics; choose exact behavior in D2 before edits.
- Workflows have multiple captured agents with different lead entitlements. A shared
  union of their reports would widen access. Snapshot needs per-acting-agent scope
  ownership plus common/base static-step context, including captured static agent
  scopes. Own-session person memory is additional inherited authority, not the binding
  author's namespace; personal save target and own-agent read need explicit disposition.
- Multiple unit namespaces can share the same selector category. Recall/consumption
  proof must identify namespace, not category alone; overlapping chain/report selectors
  must neither duplicate results nor make an entitled explicit category unavailable.
- Late INSERT can widen a snapshot even when every row refuses UPDATE. Need an
  immutable admission seal/header and fail-closed missing/incomplete snapshot behavior,
  plus whole-transaction creation/copying. Reuse existing company write prefix and
  immutable/cascade guard patterns; no speculative scheduler or repair protocol.
- Service SaveItem/SaveRequest deny unknown fields; Search is query-ranked/limited,
  Timeline is anchor-neighborhood only, no subject index/list exists. Existing client
  Delete refuses expired records and disabled tenants. Thus simple local filtering or
  client timeline iteration cannot prove complete operator subject purge. A narrow
  opaque subject + managed-admin list/delete extension can reuse lock_tenant/may_manage
  and existing provisioner capability (already able to delete whole namespaces),
  preserving normal client grants. Decide exact API/retention/concurrency in D2 gate.
- Service operation registry generates HTTP/Admin/MCP/OpenAPI surfaces; Admin is HTTP
  only. lock_tenant already checks managed ownership and scope, with Share/NoKeyUpdate
  modes; runtime client CLIENT_OPERATIONS excludes destructive operations and should
  remain so. No new operator credential/config is needed if using the already stronger
  managed provisioner authority with exact tenant/namespace predicates.
- Actual department chain loaders are in agent_manifest/roles.rs; no generic native
  department-chain SQL function exists. workflow_target_departments returns at most2
  placements for unique/ambiguous target (desk or agent membership/roles).

## Resources / next

Borrowed PG18 localhost51167 and previous/shared DBs must be preserved. New p02 own
pa05_p02_schema/pa05_p02_core_test databases are allocated; service contract checks need a distinct
owned pgvector fixture (prior04 containers were cleaned, image/model caches remain).
Reuse external session Cargo/Node/GNU caches and private env setup, not old04DB URLs.
Private env in the external session cache pa05-p02-env.sh selects those DBs.
No p02 server/container/test process yet. Next finish bounded producer/consumer tracing,
record D2 decisions and criterion-to-assertion map, publish cross-phase contract notice
if needed, pin coordination and obtain independent architecture PASS before edits.

## D2 / revision 1 — architecture gate

This is the proposed complete p02 contract, not executed proof. Source remains p01.
All decisions below are reviewed together before dependent edits.

### A. One pure resolver, independently scoped actors

- application/workflow/scopes.rs owns `run_scopes`: typed unit chain/root and
  bounded unit-parent facts, acting PrincipalId, and held-role facts. It validates
  the chain (root last, no repeats, maximum8 edges), unions chain units with only
  direct children of lead roles held by this principal, and returns personal,
  readable units/origin relations and optional local write unit. Persisted/loaders
  supply facts, not policy. Roles of another principal and non-lead roles grant no
  reports; grandchildren/siblings do not enter merely through topology.
- Desk: use its admitted unit chain. No desk: actual acting agent's distinct
  department memberships/roles, LIMIT2 to distinguish singleton; exactly1 uses
  its chain, otherwise readable units=root only/write_unit=None. Ambiguous/no
  chain suppresses report expansion under README decision6. Root is readonly in
  this fallback; it is not silently a writable `own` unit.
- Sixteen departments in roster display is not an authorization bound. New fixed
  MAX_RUN_READ_UNITS=256 bounds each resolved set; fetch at most257 candidate facts
  and reject over-limit with a controlled scope-limit refusal, never truncate a
  read set. Retain the existing manifest closure/agent count bounds and depth8;
  no existing budget increases. Unit tests keep max/max+1 failures in CI.
- A workflow can select several agents. Freeze a separate group keyed by each
  acting principal, not a union of their reports. All captured agents are company
  agents: agent_active_revisions has composite FK to agents(company_id,id), and
  library installation creates a company copy/principal. A missing/foreign current
  principal refuses that group/admission, never falls back to another principal.
- Common/base scope rows serve static memory steps and reflect the admitted desk/
  direct-target context, already captured in the manifest. A static captured-agent
  step uses that captured agent's personal group. No desk/common context has no
  writable unit; explicit UnitRoot save therefore refuses unless root is local.

### B. Immutable SQL snapshot and all admissions

- Init schema adds `workflow_run_scope_snapshots(company_id,run_id,created_at)`
  (composite run FK/cascade) and `workflow_run_scopes` rows: company/run,
  nullable acting_principal_id (NULL=base), matching unit_id or principal_id,
  access in read/write/personal, frozen unit slug/depth plus own/ancestor/report
  relation flags for unit reads. Native UNIQUE NULLS NOT DISTINCT prevents duplicate
  actor/owner/access rows; shape checks reject mismatches. IDs remain plain retained
  owner IDs, with INSERT guards proving current tenant/type; deleting a readable
  owner can make its scope unavailable without rewriting historical snapshots.
- Build rows then insert the immutable snapshot header LAST in the same admission
  transaction. Rows reference this header with a deferred composite FK, so a
  partially written row set cannot commit without the seal. Row INSERT and header
  INSERT take the established company NO KEY UPDATE prefix; row insertion refuses
  an existing seal. UPDATE/DELETE refuse while the owning run exists; owning-run
  cascade is the existing immutable-history exception. Header checks require the
  company's root read, valid row shape/owner kinds, and each unit write to match
  an own-unit read; principal writes must match an admitted personal read.
  Missing header/rows is `scope_unavailable`, never company-wide fallback. No
  mutable background sealer, repair job or MVCC/xmin protocol is introduced.
- Root external admission captures topology once under its existing company prefix,
  after the dependency closure/session facts are known, and persists/seals before
  committing admission. Static/base and every captured agent get their own groups.
  Namespace IDs are intentionally NOT frozen: lookup by frozen owner at use allows
  later provisioning; retired/missing/foreign owners cannot resolve to new owners.
- Children inherit actual parent association/actor and root dependency manifest
  (children::create_on/attach_child_on). Copy the parent's sealed rows/header into
  the child transaction rather than re-reading the live tree, preserving the
  root's frozen closure contract. Missing parent snapshot refuses admission.
- Native MCP broker admission is a real third producer with no ordinary manifest.
  It must create/seal scopes using its authenticated NativeMcpOwner/harness agent
  and association before the first job. It does not acquire a person's private
  scope merely because its synthetic workflow trigger is Manual. Real own-session
  eligibility remains the original trusted root/session rule, not the trigger alone.
- Headers/rows contain organization topology, not a waiver of current membership
  incarnation, audience/classification, source-proof, live agent, credential, epoch,
  tenant or lease checks. Runtime calls continue to execute those independent fences.

### C. Personal sessions, selectors, write targets and references

- Every acting agent reads its own principal namespace. An actual person's own
  unattached manual root (and same-actor children) additionally reads that person's
  namespace; capture this once using the existing user_scope_owner rule. Both are
  selected by `personal`. Store the optional person as a base personal row, so
  per-agent reports remain separate. No other person or external/system namespace
  is provisioned. A personal save in that own session targets the person; otherwise
  it targets the acting agent. Default save uses local write_unit if present and
  this personal target otherwise. Explicit `unit` with no local unit refuses.
- MemoryToolScope becomes Personal/Unit/Ancestors/Reports. Scope metadata represents
  personal principal or unit+slug+origin flags; an overlapping chain/report unit
  supports both selectors but is queried only once. Canonical label is `unit:<slug>`
  for own/ancestor and `reports:<slug>` for report-only, `personal` for principals.
  Requested selectors filter the frozen set; no all-company enumeration fallback.
- Ready access wraps scope metadata plus opaque namespace/credential; not-ready
  access retains that same metadata/owner. Provision an exact allowed owner in its
  own short transaction, after validating it belongs to this fenced selection.
  Missing/retired identities are unavailable, live owners without ready namespace
  are not_ready; retain today's ready-subset behavior during preparation.
- MemoryRecallRef stores namespace identity rather than selector category as the
  security key. Search/timeline/details, frozen recall packs, consumption external
  sources, revalidation and replay all match exact namespace+observation/revision.
  This prevents two readable unit namespaces in one category from being confused.
  Normal credential operations/ceiling/watermark/consumption-before-exposure remain.
- Step literals are strictly {"unit":"own"|"root"} or {"agent":captured_key}; old
  kind/company/user forms are refused. Compiler still refuses uncaptured agent keys.
  Company-wide binding with Own fails activation as binding.memory_scope_needs_desk;
  a direct principal with one unambiguous unit resolves like existing role bindings.
  Extend the existing direct-placement mutation checks for active Own requirements;
  do not leave a newly ambiguous active Own binding. Descendant requirements are
  also checked during existing admission dependency traversal before effects.
- Thread capture follows default local unit (or own agent fallback without local
  unit). Private-session automatic capture stays in the person's private namespace
  under its existing explicit own-session policy, not a newly shared unit. Child
  branches/attached private sessions remain ineligible. This preserves the inherited
  private capture boundary while adopting unit ownership for desk learning.
- A small frozen host scope context is bound before history/recall budgeting and
  names the write unit and searchable units in one sentence for department runs.
  Store it in PreparedAgent and restore it from the saved preparation; do not read
  the live tree on retry. Bump PREPARED_AGENT_VERSION and all strict readers/fixtures
  together if the persisted preparation shape changes. Keep existing department
  roster visibility/current audience rules; admitted scope text is explicitly about
  this run and derives only validated bounded slugs, not free-form instructions.

### D. Optional subject and complete managed purge

- Save tool accepts optional PrincipalId `subject`. Validate inside the fenced save:
  same-company current person/agent with a directory address, or a principal in
  this run's actual thread. Unrelated external, foreign, missing or malformed IDs
  refuse without service call or intent. A subject never grants scope access.
- Add optional opaque bounded `subject` to service SaveItem and observations storage;
  OpenBot writes the validated UUID string. The service treats it only as an opaque
  provenance key. Corrections retain it; physical deletion removes it. Generation
  has no subject unless explicitly supplied by the supported save path. Existing
  search/fetch/pack response shapes and classification algorithms remain unchanged;
  subject is exposed through the managed listing, not newly copied into model text.
- Freeze subject with the exact save body and local delivery subject_principal_id.
  Delivery principal_id identifies the host-known producing principal (selected
  agent for a tool/thread capture, authenticated workflow actor for a plain step),
  unit_id the relevant unit/target when present. Namespace owner remains in the
  namespace row; these fields are provenance, never authority. Root/unit/personal
  cases and replay preserve identical payload/key/label bytes.
- Existing ranked Search and neighborhood Timeline cannot enumerate a subject;
  ordinary client Delete cannot erase expired records/disabled tenant. Implement
  narrow managed Admin list/delete operations using existing lock_tenant/may_manage
  ownership and namespace predicates. The provisioner already may issue read9 credentials and delete complete
  owned namespaces. The new managed surface deliberately lists opaque IDs/levels
  (including expired rows) for cleanup, never bodies; it needs no new secret.
  Runtime CLIENT_OPERATIONS stays unchanged (no delete/purge grant to model clients).
- List is keyset-paged by observation UUID with bounded page<=100 and exact opaque
  subject/tenant/namespace predicates, including expired stored records. Delete
  takes at most100 listed IDs and rechecks subject/tenant/namespace in the atomic
  delete. Reuse existing physical deletion/cascade/audit mechanisms; admin requests
  authenticate before body parsing. Foreign/not-managed tenants uniformly refuse.
  Disabled owned tenants can be cleaned under their existing managed-admin authority.
- Lock tenant then namespace (namespace NO KEY UPDATE for deletion) before rows,
  matching D13; saves hold namespace SHARE so delete/save and competing purgers
  serialize. A purge removes currently stored tagged records; subsequent saves
  are ordinary new records, not a permanent subject ban. Retries of an ID set are
  safe/idempotent; never claim a failed/ambiguous batch completed.
- OpenBot operational CLI provides list and purge forms for company+subject,
  using existing DATABASE_URL and memory service/provisioner configuration. Resolve
  all local unit namespace IDs (including retiring IDs), keyset-paged; never select
  principal namespaces. List exact remote IDs then delete those matching batches.
  Bound per-request work and the whole command with a fixed deadline; on partial
  failure report it and allow rerunning the same command to finish remaining rows.
  No background purge service, confirmation flow, new credential store or operator UI.

### Verification contract for D2

- Pure resolver matrix: root/child/depth8, lead/member/reviewer/wrong-principal,
  sibling/grandchild, overlapping relations, DM0/1/2, max256/max+1, malformed chain.
- Actual root admissions on two desks, different actors' lead roles, competing tree/
  roster/lead change in both orders, same admitted set after edits/new set next run,
  parent/child copying and native broker sealed snapshots. SQL foreign owner, shape,
  duplicate/update/delete/late insert and incomplete snapshot refusals.
- Production step/compiler/activation and tool dispatch across every selector,
  rootread ceiling3, ancestor-write refusal, default local/personal writes, actual
  own-session person access versus forged/attached/scheduled/child actor differences,
  duplicate relation unit queried once, exact namespace recall/ref replay after changes.
- Real save subject visibility/body/intent/service storage and correction retention;
  actual managed list/delete for several unit namespaces, unrelated subject and
  principal namespaces survive, foreign owner/admin/client refusals, expired/disabled
  cases, pagination boundaries, competing save/delete and competing purgers. Use
  real service HTTP and owning CLI path, not a local-column stand-in. Update ContractFake
  faithfully and rerun the affected standalone service suite under owned pgvector DB.
- Current classifications, consumed exactness/unknown watermark, egress/source proofs,
  revoked credentials, disable/retirement/fence races, deterministic replay, all affected
  wrappers/checkpoint/frozen-preparation/host-context budget tests remain intact.
- SQLx prepare root and services/memory with fresh own migrations; focused suites,
  formatting, locked/offline all-target check and source reachability now. Full root
  gate/affected real-service integration once after03 before publication. Preserve all
  failed/unavailable evidence and explicit Doop deferral; do not claim whole CI green.

## D2 / revision 2 — F1/F2 corrections and precise seal locking

Supersedes the affected r1 paragraphs below; other r1 decisions remain. P02source
is still unedited. This revision preserves existing independently authorized MCP
execution rather than coupling an MCP-only graph to unavailable memory authority.

### F1: fresh identity versus trusted historical inheritance

- Ownership validation is finalized at header/seal insertion, where the complete
  row set and canonical source are known. Before the seal, row guards enforce
  shape, same-run ownership and no prior seal; the deferred FK still prevents
  committing rows without a header. A fresh root/conversation snapshot must prove
  every live owner tenant/type and bounded safe metadata. It cannot claim historical
  inheritance merely by supplying an arbitrary source UUID.
- Ordinary children derive their source ONLY from workflow_runs.parent_run_id.
  Their complete row set must equal the actual sealed parent's rows (all actors,
  base context, owner IDs, labels, relations and accesses), using IS NOT DISTINCT
  FROM semantics for nullable fields. Validate the trusted row origin, not current
  existence/slug equality of every historical readable owner. A report unit that
  was renamed/deleted after admission therefore does not block a correct copy.
- Workflow-native broker inheritance derives its source/actor projection from the
  actual native call's canonical source session, described under F2. Header guards
  compare to that trusted source projection; they do not weaken fresh validation.
- Runtime still resolves current live owners and ready namespaces, independent of
  historical copy acceptance. Missing/retired owners are scope_unavailable, never
  re-provisioned by slug or replaced by another namespace owner. Foreign/changing
  copy tuples and a missing/incomplete source seal refuse authority.
- Proof: actual child creation after report-unit rename and deletion retains the
  frozen IDs/labels; copied scope reads refuse the deleted owner. SQL wrong parent,
  foreign source, changed tuple, missing source seal and extra-row copies fail.

### F2: preserve canonical native source identity and never recompute workflow scopes

- Extend NativeMcpOwner with the authenticated SessionAssociation already available
  in local_agents/protocol/mcp.rs::owner_on. Do not reconstruct source kind from its
  final RelatedAssociation or the broker's synthetic Manual trigger. Source data
  comes from authority.session, not request arguments.
- Source is already stored structurally: local_agent_sessions has exclusive
  conversation_id/workflow_run_id columns, and native_mcp_calls.source_session_id
  identifies the original session. The seal guard can follow the actual new run's
  native call to this session. No new source-run FK/parallel origin schema is needed;
  existing native provenance/retention rules remain unchanged.
- Workflow-origin: use the source's sealed base unit context plus ONLY the frozen
  group for this authenticated session's harness agent principal. Do not union other
  actors' reports or recalculate memberships, lead roles or hierarchy. Copy a private
  person's base row only when the existing trusted own-session rule proves this
  actor's eligibility for that source and the actual source snapshot contains it;
  source actor mismatch never inherits somebody else's private row. The header
  validates the exact permitted source projection, including historical owner IDs.
- Missing source seal/harness group grants NO scope authority. Record a sealed
  unavailable snapshot with ZERO scope rows; every memory/knowledge scope consumer
  returns scope_unavailable and provisioning is refused. It is immutable and cannot
  be upgraded from live topology later. This is the explicit fail-closed scope
  disposition. Current PreparedNativeMcp accepts only MCP-backed operations, so its
  independently authorized MCP action may still be admitted; lack of memory scope
  does not invent a new reason to break existing MCP-only execution. No direct
  memory disclosure through the current broker is claimed. Tests must distinguish
  MCP permission from unavailable memory/knowledge permission.
- Add a small available/unavailable state to the immutable scope header. Available
  fresh snapshots require root/base and all declared shape invariants. Unavailable
  headers contain no rows, name a controlled reason, and are permitted only for a
  canonical workflow-native source whose required frozen context is unavailable.
  Ordinary root/child failure paths do not silently become available empty sets.
- Conversation-origin: fresh capture under the actual authenticated source/harness
  and existing company/source prefix is allowed at this genuinely new broker-run
  admission. It still cannot get a person's private namespace from Manual alone.
- Proof: real workflow-origin broker after lead/tree mutation matches only the old
  source group's projection; a new root captures the new state. Wrong/missing source
  group or seal yields sealed-unavailable/no scopes and no live fallback while an
  otherwise authorized MCP-only operation retains its behavior. Test another actor's
  reports and private person row exclusion, correct own-session inheritance, and
  fresh direct-conversation capture. Preserve all native source/current-authority
  checks and actual MCP-only contract limits.

### Seal serialization without upgrading native authority to waiting writers

- R1's unconditional company NO KEY UPDATE inside guards is too strong for native
  NOWAIT boundaries. Snapshot data needs a consistent read prefix, not a new company
  writer. Use company FOR SHARE NOWAIT (reusing the caller's already-held stronger
  prefix where present), then this new run's row FOR UPDATE NOWAIT as the seal mutex.
  Root admissions retain their existing stronger company writer; native admissions
  never add a waiting company upgrade. Run insertion is owned by the creating
  transaction, so normal header/row writes contend only with their own mutex.
- Row insertion and header insertion both acquire this same prefix/mutex and
  recheck whether a header exists. Once it exists, no further rows can be added.
  UPDATE/DELETE remain forbidden except the owning-run cascade. Competing append
  and seal cannot cross because the common run lock orders them; lock contention
  on a native path is a controlled busy refusal with no new dispatch.
- Fresh topology loads use the already-held company prefix, which conflicts with
  production topology/roster writers. Historical copies never reacquire live tree
  authority. Header validation checks rows using the applicable fresh/inherited
  rule above. No mutable sealer job or MVCC/xmin protocol is introduced.
- Proof: competing SQL append/seal claimants and native company/run lock contention
  produce ordinary serialization or bounded/NOWAIT refusal, no late widening,
  duplicate scope rows, partial snapshot or new native effects. Keep D1's owner
  creation/retirement lock order and independent current identity fences intact.

Resources are ready: own pa05_p02_schema/core_test onPG18:51167, and owned pgvector
container openbot-plan05-p02-memory-pg/label05SC-P02 at loopback32770, image pinned
inlogs,1GiBshm from validated04fixture setup. Private pa05-p02-env.sh selects them.
No02migration/test/service process or source edit has run. D2r1 findings are open
until independent recheck of this revision; acceptance/proof execution still pending.

## Resume / D2 acceptance

Independent 05SC-P02-D2-R2-N1 PASS received; F1/F2 resolved at decision level.
Root reproduced source cc06127eac2c2713ece05d3c97376c9c33a62e128a5e390c6c7feb573f9eb484
at HEAD e6eb5bc8 using the private env's GNU tools. Coordination revision8 matched
retained review input before activation. Root activated C005/r2 in revision9 and
ACK C001–C004/r1 and C005/r2. No user scope or publication timing changed.
Continue one cohesive implementation batch; independent batch review and all
declared actual production/service/concurrency evidence are still required.

## B1 implementation checkpoint (not acceptance)

- Pure resolver4tests PASS in B1-resolver-tests-r3; initial compiler failures retained.
- B1-migrations-r1 PASS on owned core schema; B1-snapshot-tests-r1 exercised4DBtests:
  3PASS,1FAIL because test raw DELETE skipped canonical owned-desk retirement. Fixture
  now removes its desks through ChannelPersistence then real department deletion;
  that corrected case has NOT yet been rerun. It is not relabeled PASS.
- B1-tools-check-r1/r2 library checks PASS, then B1-all-targets-r2 PASS before the
  latest prompt/version edits. Original all-targets-r1 compile failures retained.
  Test constructors now supply host producer/subject and exact namespace references.
  Test names were retained; old selector-budget assertions were replaced with the
  required fixed-total variable-namespace bound. No dropped test names or weak assertions.
- New frozen RunScopeContext is budgeted before history/recall. PREPARED_AGENT_VERSION
  moves7to8; both explicit version assertions move to8, including only that payload
  expectation in an existing Doop-named test. No Doop test execution, declaration,
  control-policy assertion, existing Doop defect, or Doop disposition changed.
- Subject service contract/store/Admin operations are authored, not behaviorally
  verified. Service schema migration PASS in B1-service-migrate-r3 after installing
  required pgvector in owned pa05_p02_memory_schema. Initial Docker default-socket
  and missing-extension failures retained. Explicit Docker context
  colima-openbot-plan04 reaches owned container133a20e27176/label05SC-P02; no global
  context/profile was changed. Subject APIs remain Admin-only; runtime client grants
  unchanged. Service SQLx prepare r3 failed on use of a sibling-private validator;
  now uses its existing local id validator, r4 currently pending.
- Remaining production obligations: operator/real service/fake proof; subject-save
  tests; namespace-read commit/replay fences and exactness audits; capture default
  target and host producer; strict step/tool fixtures/vocabulary and all production
  admissions (two desks, DM1/2, role changes), exact child/native inherited and
  unavailable context proofs, full competing contention/final focused gates. No B1
  review/freeze/acceptance, phase03 or main publication yet.


## Resume 2026-11-17 — focused checks and interface corrections

Coordination revision9 rechecked; C001–C004/r1 and C005/r2 ACK unchanged.
Remote main explicitly confirmed d748c2ff64309903449a3d860377a5e0385377a4;
local HEAD remains accepted p01 e6eb5bc8. No p02 review/acceptance, p03 work or push.

- Corrected missing `.await` in the new subject tool proof. B1-read-proof-check-r3
  retains its original compile failure. B1-scope-tests-r4 exercised59 tests:
  50PASS/9FAIL. Snapshot immutability, historical report labels and competing
  append/seal passed. Failures exposed old positive vocabulary fixtures, a fake
  admin router limited to two path segments, and shared agent preparation refusal.
- Root cause of the preparation refusal: Rust writes version8 while two SQL
  agent-history validators still required7. Both init-schema predicates now8;
  owned schema/core_test databases alone were reset/migrated successfully in
  B1-core-schema-refresh-r3. No limit or assertion was relaxed. Root SQLx refresh
  is still required after the complete batch.
- Updated positive step/tool/recall tests to current selectors, exact namespace
  references and personal save targets. Old company/user/agent selectors remain
  explicit negative schema/parser cases. All existing test names retained.
  Root-unit slug is the actual company-root slug, not a literal `root`.
- Fake Admin routing now accepts the full operation path; contract validation
  still restricts every request/response. New subject list/delete adapter tests
  passed in B1-scope-tests-r6. No real-service proof is inferred from the fake.
- New host-context tests cover mandatory-budget growth, serialized frozen context,
  preparation/restore and refusal of a write unit outside the readable set.
  B1-scope-tests-r5 retained a new proof-test compile error (AgentLaunch wrapper),
  corrected before r6. R6 exercised61:58PASS/3FAIL. Remaining failures were an
  incorrect literal root-slug expectation, the subject save refusal (diagnostics
  now exposed), and scheduled-source personal targeting.
- The scheduled-source fixture exposed that eligible_on had dropped the existing
  runtime own-session check. It now filters the frozen private-person row through
  the trusted current own-session proof; workflow-native brokers resolve only
  their canonical source session/run and matching actor. Actor-owned personal
  access remains available when private-person access is omitted. This preserves
  D2/C002 fences rather than changing frozen unit topology. Verification pending.
- Memory step save fixtures now admit on a real root-owned desk, preserving the
  strict local-write invariant; generic company-wide lease fixtures are unchanged.
- Standalone affected memory service full test gate PASS: B1-service-suite-r1,
  `SQLX_OFFLINE=true cargo test --locked --offline --all-targets`, owned pgvector
  fixture,178passed/0failed/2existing opt-in ignored. Includes all3subject HTTP/SQL/
  concurrency tests, real model retrieval, migrations and existing contracts.
  The ignored backup/restore and measurement checks were not executed.
- B1-scope-corrections-check-r1 failed on new test-helper imports (retained); imports
  corrected. `cargo fmt` applied. B1-memory-focused-r1 (`cargo test --locked
  --offline --lib memory_ -- --skip doop --skip Doop`) is running/queued behind the
  completed-error all-target compilation; retain/poll the exact session result.
  Doop tests were not run. No independent B source review has started.

Still required: finish affected memory fixture/runtime proofs; actual two-desk/DM/
multi-actor and child/native topology/projection/refusal cases; real cross-service
OpenBot save plus operator purge/CLI proof; bounded frozen recall/replay and capture
proof; root SQLx/fmt/all-targets/reachability and cohesive independent02review.
Then03knowledge and the full required source gate before main publication.


## B1 verification continuation / 2026-11-17

No change to coordination revision9, C001–C004/r1 or C005/r2; all ACK. Source remains
uncommitted/unreviewed at p01 HEAD e6eb5bc8. P03 has not started. No Plan05 push.

Current production corrections: two SQL version predicates aligned to prepared8;
fake Admin wildcard routing supports the three-segment subject endpoints; current
own-session eligibility filters the frozen person row, with canonical native-source
selection. The latest SQL CASE explicitly gives a native run with no canonical
workflow source **no** person authority; it never falls back to the broker's synthetic
Manual context. That final strict-selection delta is awaiting r9 focused verification.
Frozen recall assembly is now a synchronous helper, preserving shallow async chains.

| Criterion | Actual assertions and retained evidence | Current disposition |
| --- | --- | --- |
| AC1 | Original4 run_scopes_resolve_ matrix tests: bounded chains, role/principal isolation, ambiguous fallback, max256/max+1; B1-resolver-tests-r3 and B1-focused-r2 | PASS unchanged code; also included in r10 |
| AC2 | Two actual configured desk admissions keep local units separate and deny root writability; sealed rows reject update/delete/late append/missing/foreign authority; actual competing append/seal; B1-scope-tests-r6 and B1-admission-native-child-recall-r2 | PASS |
| AC2 | Actual child producer after report rename/deletion copies exact historical IDs/labels; deleted owner refuses current namespace use; corruption trigger alters actual copied tuples while all guards stay enabled, sealing refuses, no child survives rollback, retry succeeds with exact parent rows; B1-native-scope-integration-r8 | PASS |
| AC2 | Real hybrid native/hosted source captures two independent acting groups. Broker projection excludes the other actor's reports, retains genuine own-session person proof, ignores later placement, and fresh direct-principal admissions exercise0/1/2 placements plus member versus lead child reads; r8 | PASS before strict person-source CASE delta; r10 rechecks it |
| AC2 | Actual workflow-native missing harness group seals unavailable with zero rows while separately authorized MCP still works; conversation-native fresh capture has one agent personal group and no person-private row; original NOWAIT/source/owner/fence/teardown cases; r8 | PASS66/0,1existing opt-in ignored |
| AC3 | Production steps/tools/automatic recall/capture plus shared ceilings, consumption-before-exposure, current revocation, exact replay, source citations and fixed global budgets; B1-memory-focused-r2 | PASS192/0,8live opt-in ignored (live cases separately below) |
| AC3 | Host preamble context counts before history and restores; actual freeze80fetched items across2ready namespaces commits only64 items; foreign empty pack refuses before consumption; byte-bound trimming keeps whole items and the carried label; B1-native-and-freeze-r3 and B1-native-projection-and-byte-bound-r6 | PASS2freeze +1byte checks; controlled fetched DTOs, not new real-service retrieval claims |
| AC3 | Old company/user/agent selectors remain negative schema cases; exact namespace IDs now identify hits/details/consumption/citations. Root read label uses actual unit slug. Actual private-session, scheduled-source, child and thread capture targets pass in r2/r8 | PASS; r10 checks the current person-source delta |
| AC3 | New own binding activation and zero/ambiguous active placement refusals; actual memory-save port refuses root ancestor write with no delivery intent | r10 pending (r9 test-only compile failure retained) |
| AC4 | Invisible same-company agent refuses as subject; adding its directory address permits the save, freezes exact producer/subject/target/body, foreign/missing subjects create no intent; r8 | PASS |
| AC4 | New factual boundary test uses genuine admitted/claimed thread runs: current-thread external participant accepted, another thread refused, removal revokes visibility | r8 PASS, no claim of a second end-to-end tool dispatch; directory dispatch is covered above |
| AC4 | Actual service stores subject, correction retains it, Admin-only exact list/delete handles expired/disabled records, tenant isolation, pages, competing save/purge and competing purgers; B1-service-suite-r1 | PASS178/0,2existing opt-in ignored |
| AC4 | Actual OpenBot freeze/delivery to real service then real CLI list/purge:105 tagged unit records across2unit namespaces and a100-record page boundary; bad auth reports incomplete, valid rerun deletes105, retry deletes0, other subject and principal-owned record survive; B1-live-integration-r2 | PASS7/0 real cross-service suite; source classification is supplied by the existing trusted freeze fixture |
| AC5 | Fresh owned core/schema migrations r3, service SQLx prepare4, service OpenAPI generation, root SQLx prepare r1, whitespace | PASS; root cache unchanged as expected for runtime queries. Final current-source compile/fmt/clippy/reachability/evidence/review still pending |

Original failures remain in their logs. B1-memory-focused-r1 exercised185PASS/7FAIL/
7ignored; failures were changed labels/targets, old literals, directory-less fixture
subject and old thread-capture target. All corrected in r2. The thread capture test
was renamed from
memory_capture_a_thread_exchange_is_captured_into_the_agent_memory_at_the_thread_level
to memory_capture_a_thread_exchange_is_captured_into_the_unit_memory_at_the_thread_level;
r1 retains the runner-discovered old name and r2 the passing replacement. No assertion
was dropped: the same production generation now targets the frozen local unit.

The new hybrid native fixture had retained setup failures: original workflow_id
mismatch (native-focused-r1), memory-only references not declaring hosted agents
(child-recall-r2), missing finite reference universe (native-and-freeze-r3), missing
hosted model (projection-r4), and missing local-step resource resolution (projection-r5).
It now uses explicit hosted model connections, actual later agent.run steps and the
existing AgentPublicationDirectory wrapping StepBuiltinPublicationDirectory. Original
native-only fixtures still omit hosted models; configure() returns the shared helper
future without adding an async forwarding frame. Projection + byte-bound r6 PASS2;
full native/scope r8 PASS66. These fixture errors were not product defects or passes.

Live integration r1 exercised6PASS/1FAIL: CLI subprocess stopped at required existing
cipher configuration. B1-operator-startup-diagnostic-r1 retains a redacted direct
startup diagnosis. The fixture now supplies its own test cipher, not a production
setting change. r2 PASS7. Script-owned temporary service DB/roles/probe/server/fake
were cleaned automatically. New retained base pa05_p02_core_live belongs to p02 on
its owned pgvector container; it does not touch shared/core/Plan06 data. Private live
env is in the external session cache. No production purge was performed.

Current command B1-scope-acceptance-focused-r10 (exec71176; r9 compile failure retained) covers current person-source
selection, pure resolver, admission and own-binding refusals, subject facts, exact
scope/ref/capture behavior. Poll and inspect it before acceptance. Then finish the
current-source fmt/offline all-targets/clippy/reachability gates, source audit,
criterion/evidence manifests, coordination pin and frozen digest; request one cohesive
B source review. Source corrections after that require affected rechecks/review.

Two retained Trail notes are in scope. l-aa159407 replaces l-b603fb20 solely to correct
the SQL function name to agent_history_preparation_valid; no policy changed. Keep
both records and explain this correction in the commit if trail check reports the
unmerged replacement conflict. Commit authorization already persists with this task;
no new permission question is needed. No unrelated learning or skill was edited.


R9 retained a compile-only failure in the new activation test: BindingState has no
Debug and LifecycleError has no Display. The assertion now matches the typed
LifecycleError::Application(BadRequest(code)) without changing production types or
weakening refusal checks. R10 is the current execution. The direct-root-save test
uses the real claimed MemoryStepStore effect port and asserts scope_unavailable and
zero intents; own direct binding mutations must atomically refuse both zero and two
placements. R8 includes current-thread external visibility, actual corruption/rollback/
retry and member/lead/no-desk matrices. A later person-source SQL CASE removes even
the possibility of falling back to synthetic Manual when native source is missing;
R10 covers the affected own-session/native/capture consumers.


## B1 pre-review correction checkpoint / 2026-11-17 continuation

Coordination9 re-read and C001–C004/r1 plus C005/r2 remain acknowledged. Actual
remote main reconfirmed at d748c2ff64309903449a3d860377a5e0385377a4; no Plan05 push.
The pending executions have completed: scope-acceptance-focused-r10 PASS28/0 and
runtime-assembly-r12 PASS6/0. They cover the strict native person-source CASE,
activation/direct-root-save refusals, own-session eligibility, host context,
actual production dispatch and current recall/consumption consumers. The new
runtime corrections replace extra async forwarding with a synchronous merge helper.
Pre-review root SQLx prepare-r2 and locked offline all-targets-r1 PASS; root cache
unchanged. Root/service formatting checks passed before the lint cleanup.

Pre-review clippy-r1 FAIL28 on the changed helper consumers:24 explicit
PoolConnection dereferences,2 cloned single-element slices,1 double-ended iterator
using last, and1 inline test module before production code. All28 were corrected
mechanically without changing assertions or production behavior. R2 clippy is
running; final fmt-r3 PASS after moving the inline test module to the file end.
Source-module reachability and runner test inventory are queued behind clippy.
Doop was not executed and no baseline declarations/assertions were changed.

Trail graph refreshed (2478files). B1-pre-review-trail-r1 ran and returned5 lexical
conflicts on new snapshot DDL. They match unrelated learned phrases rather than
the prohibited behaviors: PostgreSQL bulk-insert spill performance, RcDom document
ownership, X.509 authority keys, full cascade lock ordering, and concurrent browser
storage cleanup. This DDL creates immutable run-snapshot rows and deferred seals;
it touches none of those mechanisms. Keep this explicit disposition in the commit
message as required by trail check. Its400KB diff cap is retained as a limitation;
independent whole-batch review is still required. Prepared8 Rust/SQL readers are
aligned and have fresh-migration execution evidence, satisfying the relevant new
learning. Shared read-prefix/NOWAIT scopes remain independently covered by native
contention tests. All advisories remain visible in the retained output.


Current-source clippy-r2 PASS (`cargo clippy --locked --offline --all-targets --
-D warnings`); source_modules-r1 PASS8/0; fmt-r3 and whitespace-r2 PASS. Runner
collection (`cargo test --locked --offline --all-targets -- --list`) PASS:
4768baseline names →4799current unique names. Exactly1 removed name is the thread
capture agent→unit rename already recorded; its replacement is discoverable and
keeps the capture/classification assertions.32added names include that replacement.
See B1-R1-test-accounting.json. Current combined affected check is running:
`cargo test --locked --offline --lib -- memory_ run_scopes_resolve_ --skip doop
--skip Doop`,215 selected including live opt-in ignores; no current result yet.

Implementation is now frozen at HEAD/reviewbase e6eb5bc8fff93492f23935b9c84114905801824c,
digest4371836f22d139758bc0e6c2de4e4cf124a12f34c3016e251cf989b1acc7321f,
exact exclusions unchanged. B1-R1.snapshot.txt retains all92implementation paths.
The first snapshot attempt omitted the private env/GNU tools and stopped on BSD
realpath's unsupported -m option; sourcing the owned env resolved it without code
or skill edits. Keep source frozen until whole-batch B review concludes. This is
not phase acceptance. The full repository gate remains03integration's obligation.


## B1 R1 frozen review input — ready for independent review

Current combined affected execution completed:207PASS/0FAIL/8opt-in ignored,
`B1-pre-review-focused-r1.txt`, exact command above. No Doop was executed. Actual
service suite178PASS/2ignored and live service+CLI7PASS remain valid: no service
production source has changed since those executions. Earlier native integration
66PASS/1ignored and strict current-source r10/r12 proof remain supplemental evidence;
all newly named memory scope cases run in the current combined check as well.

Review artifact4371836f22d139758bc0e6c2de4e4cf124a12f34c3016e251cf989b1acc7321f,
base/HEAD e6eb5bc8fff93492f23935b9c84114905801824c,92paths, unchanged exact exclusions.
Pinned coordination9 in B1-R1-coordination.md, C001–C004/r1 and C005/r2. Original
requirements and accepted D2/r2 remain the review criteria. B1-R1.evidence.json
retains actual successful checks, relevant failed attempts and the intentionally
pending03full gate. Test-accounting json proves the1removed name's replacement.
No phase acceptance or03dependent source work before B's explicit digest PASS.


B1-R1 assignment sent via followup_task to retained independent B /root/p05_reviewer.
No source mutation during review. Evidence validator PASS15check entries plus4
retained failed attempts and1pending03full gate; declared results do not themselves
confer acceptance. Initial validation caught an omitted NOT EXERCISED reason in
new manifest metadata; corrected before B assignment. Root reproduced digest and
coordination before assignment. B1-R1-reviewer records remain B-owned.

Final-gate operational planning (no03implementation started): unmodified
scripts/check.sh contains Doop boundary/probe/private gates and broad libtest/stock
runs. User's active C001/C003 deferral overrides executing those. For03's final
run, retain an auditable phase-local derivative/harness that leaves repository
scripts, CI declarations, assertions and budgets unchanged, marks named Doop gates
USER-DEFERRED, and adds --skip doop --skip Doop to the root full and stock libtest
invocations. scripts/stack-budget.sh already forwards its arguments, so it needs
no source change. Lease filters exclude Doop. Retain original runner/CI identities,
exact derivative and all gate statuses; separately execute Linux-only supervisor
scope on an owned Linux fixture because local Darwin branch reports NOT EXERCISED.
Do not claim whole CI green or mistake user-deferred checks for PASS. No other
source gate is waived by this planning note.


## B1 R1 outcome and cohesive R2 corrections in progress

Independent B completed05SC-P02-B1-R1-N1 CHANGES REQUIRED for4371836f…7321f;
all3medium findings are retained in B-owned findings and REVIEW. No SQL bypass or
service disclosure was claimed. B independently exercised17root and3real-service
subject tests, allPASS, but correctly identified uncovered original criteria.
Source freeze ended only after this whole-round return. Coordination9/C001–C004/r1
and C005/r2 unchanged. Same D2 architecture; no new decision/permission required.

- F1: new actual root admission with an agent workflow-tool child requiring Own
  reproduces the bug: B1-R2-own-preflight-red-r2 has1PASS(valid desk)/1FAIL(missing
  context incorrectly Created). Zero-turn scripted provider makes provider effects
  observable. A now stores the already-resolved base department in the bounded
  admission Closure and checks every reached workflow, including agent-tool edges,
  before any root commit. Removed redundant root-only check from scope capture;
  activation's ordinary pinned-child check stays. Shared graph publication fixture
  separates publish from bind so an unbound workflow tool can be tested correctly.
- F2: added real fresh-seal SQL tests for foreign unit/ineligible external owner,
  missing root, unpaired local write and deferred header-FK commit refusal, asserting
  no retained authority. Ordered actual admission versus actual department reparent
  uses PostgreSQL waiter chains and oneshots in both orders; exact actor/base tuples,
  historical immutability and next-run change are asserted. Added actual native
  memory eligibility scope_unavailable/no-new-namespace assertion while MCP remains
  authorized in the existing missing-harness case. None of these tests is accepted
  before successful execution and B's recheck.
- F3: added a real lead desk to the existing SearchFixture through explicit fixture
  placement variants. It leads root/local, has two direct reports plus a root report,
  and makes local both own/report. New production tool cases exercise Unit/Reports/
  Ancestors, category overlap, exact service-query counts, labels, namespaces and
  consumption, timeline/details, default/explicit local and personal saves, and an
  explicit no-local Unit refusal with zero intents. ContractFake covers host policy;
  no new service propagation claim. Shared Company fixture behavior is preserved.

Retain correction setup failures: red-r1 unresolved test imports/types; red-r2 is
the actual behavioral regression proof above; preflight-and-seal-r3 has a missing
Duration test import (corrected). Current all-target correction compilation is
exec22565, logs/B1-R2-corrections-check-r1.txt. F1 production changes and newest F3
fixture/tests came after earlier compile attempts; final current-source execution
must cover their combined artifact. No main push, phase03, Doop, budget increase,
assertion removal or external production mutation. Next inspect compilation, fix
actual errors, run focused correction/affected consumer tests, fmt/clippy/reachability/
SQLx/inventory as needed, update evidence, freeze and send one F1/F2/F3 recheck to B.


R2 focused-r1 completed273PASS/3FAIL/1existing opt-in ignored (277selected), exact
command `cargo test --locked --offline --lib -- memory_scope_ autonomous_agent_
knowledge_search_ native_mcp_workflow_created_origin_obeys_original_knowledge_ceiling_and_parent_run_locks
--skip doop --skip Doop`. F1 negative/positive now PASS; actual both-orders topology
race PASS; deferred-FK and missing-root/unpaired-write PASS; unavailable native
memory eligibility plus retained MCP authorization PASS. All previously affected
Company fixture/knowledge/autonomous consumers selected by this command passed.

The3failures are new fixture setup gaps, not accepted proof: foreign-owner test
created its second RoleFixture in a different owned database, so its SELECT inserted
zero rows (the explicit rows_affected assertion correctly caught this). It now
creates the second real company/root in the first owned DB. Two lead-desk tool cases
stopped at admission's controlled NotFound("Workflow resource"); actual human viewer
now receives real local membership before admission, without changing authorization.
A focused3case retry r2 retained a CompanyWrite default-field compile error; r3 adds
those fields and is currently running as exec1576/logs/B1-R2-corrections-r3.txt. No
production or assertion weakening. Current all-target check-r1 PASS before these
fixture corrections. Current correction output must be inspected before recheck.


R2 correction proof update: corrections-r3 PASS1(foreign/ineligible fresh seal),
FAIL2(desk provider preparation), retaining that actual invocation as FAIL.
Actual viewer membership fixed the admission refusal. Diagnostic-r1 retained a
moved-outcome test compile error; diagnostic-r2 exposes agent.unresolved before the
provider. Traced candidate_on → department_context_on → roster construction: this
new desk fixture used a persistence with no application domain. LeadDesk now uses
with_app_domain("agents.test"), preserving Company fixture configuration. Desk-tools-r4
PASS3/0: both new selector/save scenarios plus the original own-person Company case.
No runtime authorization, provider failure mapping, source labels or assertions
were weakened. There is now passing proof for each R1 finding, pending B recheck.

Final current-source gates are sequential exec76089: format, root SQLx, clippy,
source_modules and all-target runner inventory, each logs/B1-R2-final-*.txt. Inspect
completion before claiming their PASS. New Trail learning l-9091bbc9 records F1's
actual missed agent-tool edge and regression proof; commit with accepted02source.
Original273passing focused cases are unchanged apart from diagnostics; the3initial
failures have exact passing replacements in corrections-r3 and desk-tools-r4. Do not
rerun that broad277case filter without new concern; only remaining affected gates
and B review are needed before02acceptance. Final whole-plan source gate remains03.


## R2 current acceptance mapping / review candidate

Source7a555e95283fa22b7face97a1463d8efdfb6cf8c2ab92a3b77e5eae734ab51a5,
reviewbase/HEAD e6eb5bc8fff93492f23935b9c84114905801824c, exact exclusions unchanged.
Pinned coordination9 in logs/B1-R2-coordination.md, C001–C004/r1/C005/r2 unchanged.
Current format/root SQLx/clippy all-targets/source_modules8 PASS; root .sqlx unchanged
as expected for runtime queries. Full runner inventory still finishing in exec76089.
Source frozen for the forthcoming correction recheck; no acceptance yet.

| Finding / original criterion | Decisive exercised production assertion | Current retained PASS |
| --- | --- | --- |
| F1 / AC2–3, D2C | Actual root→agent→workflow-tool Own edge refuses controlled binding.memory_scope_needs_desk before any new run/job/delivery/provider call; same descendant admits on actual desk and seals its local write; unchanged plain bundle/graph cases retained | B1-R2-focused-r1, both memory_scope_admission_agent_workflow_tool_own cases; original incorrect Created retained in red-r2 |
| F2 / AC2 | Actual valid-looking rows reach deferred COMMIT and fail23503 without seal; fresh header insertion rejects missing base root and unpaired write with its named23514guard, leaving0authority rows | B1-R2-focused-r1, seals::memory_scope_admission_valid_looking… and fresh_seal_rejects_missing_root… |
| F2 / AC2 | Both real companies exist in same owned DB; foreign unit row and ineligible external-owner row actually insert, fresh seal refuses named23514guard, rollback leaves0authority | B1-R2-corrections-r3, fresh_seal_rejects_foreign_and_ineligible_live_owners PASS (the overall invocation retains2then-failing desk cases) |
| F2 / AC2 | Actual production admission and actual department update queue in controlled PostgreSQL prefix orders, no detached tasks; exact per-actor/base tuples before/after, old run immutable, next run sees move | B1-R2-focused-r1, races::memory_scope_admission_competing_topology_change_captures_coherent_scopes_in_both_orders |
| F2 / AC2–3 | Canonical native harness with unavailable source invokes actual memory eligible_on and returns scope_unavailable with no new namespace; same real broker lease still authorizes MCP | B1-R2-focused-r1, native_mcp_workflow_created_origin_obeys_original_knowledge_ceiling_and_parent_run_locks |
| F3 / AC3 | Real admitted lead/desk, 2direct reports and root report, local overlaps own/report; Unit/Reports/Ancestors and union produce exact namespaces/labels/consumption; query count equals unique set; timeline/details roundtrip | B1-R2-desk-tools-r4, unit_reports_filter_deduplicate_and_keep_exact_labels_and_references |
| F3 / AC3 | Actual tool save default/explicit Unit freezes local owner, Personal freezes eligible actor, exactly3intents; no-local explicit Unit refuses with0intents | B1-R2-desk-tools-r4, default_and_explicit_unit_saves_freeze_only_the_local_target; B1-R2-focused-r1, explicit_unit_save_without_a_local_unit_creates_no_intent |
| Shared consumers / AC3–5 | Existing selected Company/knowledge/autonomous/native consumers in273passing cases remain passing; Company own-person case rerun after fixture refactor | B1-R2-focused-r1 (not whole invocation PASS), B1-R2-desk-tools-r4 own_manual_session PASS |

New current host-policy fixtures use ContractFake and trusted stored facts, not a new
real-service retrieval or classification claim. Real subject/service/CLI evidence
from R1 remains reusable because its producers/consumers/contract are unchanged by
R2; all original limits, source/credential/lease/consumption fences remain in force.
Current no-local namespace assertion exercises the actual memory eligibility
consumer on a genuine native broker; it does not claim a model-memory tool invocation
on an MCP-only broker. Existing MCP operation stays positive.

R2 Trail build PASS; trail check returned the same5unrelated lexical conflicts and
400KBcap as R1, with the same semantic disposition and required commit rationale.
The new F1 learning's advisories refer to code that now checks the full closure; no
prohibited root-only preflight was reintroduced. Preserve all failed logs and current
R2 PASS corrections. One final whole-plan gate remains03before main publication.


## R2 ready for independent correction recheck

All sequential current-source final gates completed PASS: fmt, root SQLx/all-target
compilation, clippy warnings-denied, source_modules8 and full runner collection.
Inventory4808current/4768baseline; original1removed thread-capture name has its
retained Unit replacement;41added including that replacement. R1→R2 adds9test names,
removes0. B1-R2-test-accounting.json retains the exact lists/disposition.
A clean final correction-only invocation B1-R2-final-focused.txt PASS10/0/0 covers
all3findings on current source, avoiding partial-PASS inference from failed earlier
broad invocations. The separate Company own-person regression also PASS in r4.
No root or reviewer check process remains. R2 manifest retains original failures,
current clean correction/gate outputs, reusable unchanged service/subject/CLI proof,
and the still-pending03full gate. No Doop execution or clean-whole-CI assertion.

B will recheck F1/F2/F3 against the frozen7a555e95…b51a5source, original criteria and
same coordination9. Only14paths differ from R1:3agent_manifest files, graph publication
fixture and new own-tool tests, run_scopes, scope admission registration plus new
seal/race tests, native MCP test, SearchFixture/Tools setup/new selector tests, and
new F1 Trail learning. Other R1 source/contracts unchanged. Root must reproduce
digest/authority and validate current manifest before acceptance/commit. All accepted
source remains local until03checkpoint; no03implementation before02acceptance.

R2 correction recheck assigned to retained B /root/p05_reviewer. Implementation
remains frozen; only excluded execution records may change. Do not commit or start03
until B returns explicit current-digest PASS and root validates retained evidence.


## Root acceptance

Root reproduced source7a555e95283fa22b7face97a1463d8efdfb6cf8c2ab92a3b77e5eae734ab51a5
and pinned coordination9 after independent05SC-P02-B1-R2-N1 PASS. B-owned REVIEW,
notes and F1/F2/F3 resolved dispositions are saved. Independent11case check PASS,
author current10case check PASS; retained current-source required phase gates pass.
R2 manifest validates15PASS/8retainedFAIL/1NOTEXERCISED. Original failures remain
failures, and the full task/source gate remains03's obligation. No Doop exercised.

Accepted in p02 with no unresolved in-scope findings; may commit and serve as03's
prerequisite. Main integration not yet performed. Root owns normal accepted commit;
user publication checkpoint is03, then04docs/evidence andSTOP before06. Coordinator
will record accepted dependency and later local commit identity. No pre-existing
source was swept in, and R1 acceptance was never claimed for the deficient artifact.
