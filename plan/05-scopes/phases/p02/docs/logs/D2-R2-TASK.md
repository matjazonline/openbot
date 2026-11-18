# Plan05 p02 — frozen run scopes, memory vocabulary and subject provenance

Status: investigation / architecture; no p02 implementation edits yet.
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
root writer revision6; ACK C001–C004/r1 and inherited grant/classification/current
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
