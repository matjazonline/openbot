# Plan05 p01 — unit/principal namespaces and owner lifecycle

Status: investigation; no implementation/schema edits yet.
Workflow implement-astra-reviewed-2-3. Root/A /root, user-confirmed Sol6.1/xhigh;
B not yet assigned. Actual worker/context controls unexposed. Record root this
directory; root owns TASK/HANDOFF, B owns REVIEW/notes/findings when assigned.

Worktree /Users/mac02/orca/workspaces/openbot/plan03-04, branch
feature/plan05-scopes. Start/base/HEAD d748c2ff64309903449a3d860377a5e0385377a4.
Clean at startup, no preexisting dirty work. Exact snapshot exclusions planned:
plan/05-scopes/phases/p01/docs and exact plan/05-scopes/COORDINATION.md.

## Requirements and authority

Original plan/05-scopes/README.md and01-unit-and-principal-namespaces.md; retain
02scope snapshot/tool and03knowledge consumer obligations while establishing the
foundation. Final full gate once after all phases per04 document and root AGENTS.
No backward compatibility: edit init schema, use fresh owned DBs, no old-row shims.

Authoritative /Users/mac02/orca/workspaces/openbot/plan03-04/plan/05-scopes/COORDINATION.md,
root writer revision1 checked at startup. ACK05SC-C001r1 (only05/push/STOP, no06,
Doop separate) andC002r1 (accepted trust boundaries). Inherited01DEP-C002r1,
04RU-C003r2/C004r1/C005r1 acknowledged. Plan04 prerequisite implementation86d225cc,
publicationd748c2ff, source9b394b31…e2f0e, review04RU-P03-B1-R1-S1/N1 PASS.

## Cohesive B1 and architecture gate

Trace NamespaceOwner and namespace/owner creation, retirement, capture/delivery,
reconciler, memory step/tool consumers, manual-person versus agent identity and
admission facts before writing D1. D1 must resolve schema shape and deleted-owner
retention, disabled/enabled provisioning semantics, retirement/rejoin/tombstone
identity, transaction/lock ordering and affected consumers together. Preserve
current classification/credential/incarnation guards and service opaque contract.
Architecture PASS is required before dependent implementation.

| Criterion | Production path / decisive proof | Focused evidence plan | Status |
| --- | --- | --- | --- |
| AC1 namespace shape | only unit/principal; one live owner; tenant/type refusal; retired owner retained | real fresh-schema shape/tenant/live uniqueness tests | tracing |
| AC2 owner lifecycle | actual create/retire/reparent/membership paths, rejoin new namespace, no external/system namespace | production writers plus competing create/delete/enable cases | tracing |
| AC3 consumers | steps/tools/capture/delivery/reconciler decode only new owner schema; provenance subject fields | affected memory/protocol/compiler/checkpoint families; caller dispositions | tracing |
| AC4 independent service contract | opaque namespace IDs/subject, unchanged credential and watermark/egress contracts | production client/fake contract and levels/fencing/replay tests | tracing |
| AC5 phase verification | fresh init, SQLx regeneration, fmt/offline all-targets/Clippy/module reachability and focused DB tests | phase-owned DB/cache, retained logs/manifest/review | pending |

No tests moved/deleted yet. Save discovered names before any extraction/removal.
Phase02/03/04 are authorized only in original dependency order after accepted
prerequisites; one final full repository gate after all implementation, excluding
separately deferred Doop and unaffected independent-service reruns as authorized.

## Resources / next

Previous p03 disposable DBs/images were cleaned; preserve all p01/m01/p02 and shared
PG18localhost51167/Colima services. Existing task Cargo/Node/GNU/dependency/model
caches may be reused; allocate new05-owned schemas and private env before tests.
No05 DB or process allocated yet. Next trace actual owners/admission semantics,
record D1/proof/consumer mapping, assign independent Astra architecture review.

## D1 / revision 1 — architecture gate (2026-11-17)

1. Namespace ownership is `Unit(DepartmentId)` or `Principal(PrincipalId)`;
   namespace columns are `unit_id`/`principal_id`, exactly one matching kind.
   Tenant and historical owner IDs remain plain UUIDs on retained remote lifecycle
   records. A BEFORE INSERT/owner-change trigger validates owner existence, tenant
   and principal kind under `FOR SHARE`; a foreign key to a deletable owner would
   prevent the specified retained-removal state, so the trigger enforces the same
   composite ownership invariant without a permanent owner FK. Existing namespace
   identity and owner columns cannot be reassigned after insert.
2. Owners created while memory is enabled call ensure_on in their creation
   transaction. Without an enabled tenant there is no remote resource to provision;
   owner creation succeeds without namespaces. The existing enabled-after-creation
   lazy backstop provisions only live, same-company units/persons/agents. Enabling
   does not bulk provision old owners. This follows01B2's explicit backstop.
3. Person rejoin keeps the lifetime principal (Plan03) but gets a new namespace ID:
   demotion retires every live namespace, cancels its intents and supersedes its
   lifecycle. Agent and department deletion do likewise in the existing transaction;
   reparent/member/role edits do not touch namespace ownership. Company delete keeps
   durable detached absent lifecycles as before. Tombstone cooldown/redrive unchanged.
4. Provisioning lock order stays account (for person) -> company -> agent (when
   applicable) -> principal/unit -> tenant -> namespace -> lifecycle. Owner metadata
   may be probed to choose locks, then must be revalidated under those locks; a
   changed kind/account/agent is unavailable, not a different owner. Creation paths
   already holding these locks call the same ensure helper. Preserve all independent
   tenant/epoch/credential/incarnation/fence checks at runtime.
5. Phase01 changes every namespace SQL reader and lifecycle owner decoder together.
   Existing tool/step selectors are adapted to new owners in this phase, then
   replaced by02's vocabulary/snapshot in dependency order before publication.
   NamespaceOwner has no legacy variants or decoders. Save/capture use this same
   owner type (remove duplicate SaveScope owner model); exactness targets become
   UnitScope/PrincipalScope with the existing company/personal truth table unchanged.
   No service ownership semantics change: opaque namespace and tenant IDs only.
6. Deliveries retain nullable unit_id/principal_id/subject_principal_id provenance
   as plain IDs (not live-owner FKs); owner columns identify the target namespace.
   Phase02 supplies authenticated actor and optional validated visible subject;
   subject is propagated through the service's existing opaque provenance metadata.
   A subject never authorizes a scope. Frozen payload bounds/idempotency remain.

### Contract impact / focused evidence

- NamespaceOwner: settings, namespace persistence, lifecycle reconciler, memory
  steps, checkpoint tools, agent-history recall/capture, delivery saves and tests.
  Caller/exhaustive inventories: trail NamespaceOwner, namespace.{agent,user}_id,
  SaveScope and owner_kind. Types have no reference edges; exhaustive uses traced.
- Owners: create_agent_principal_on -> agent/company-agent/onboarding/publication/
  message-author; create_person_principal_on -> root join/address/adoption; department
  lifecycle/root bootstrap. Retirement: agent/member/account/company/department.
- Proof AC1: production namespace ensure plus direct SQL rejection of foreign unit,
  external/system principal, invalid matching columns and duplicate live owner;
  nullable subject survives delivery persistence. Retiring does not lose history.
- Proof AC2: production create/delete/rejoin; competing owner/provisioning transactions
  in both orders, existing disable races and lifecycle claimant tests. Reparent and
  child membership changes preserve IDs; recreated slug gets distinct owner/namespace.
- Proof AC3/4: existing memory scope/lifecycle/delivery/capture/access-level families,
  workflow memory steps/tools/recall/history and compiler families against fresh schema.
  Contract fake unchanged unless opaque metadata requires a narrow fidelity fix;
  retain real-service integration and new behavior proof at final combined gate.
- Proof AC5: fmt, locked offline all-target compilation, fresh init, SQLx prepare,
  reachability and focused DB tests now; root full gate once at04. Preserve test
  collection before any relocation/removal. No tests removed yet.

Architecture acceptance pending independent B. Source unedited. Coordination1,
C001r1/C002r1 rechecked and ACK. Exact exclusions unchanged. Root/A actual session
settings are user-confirmed; context threshold remains host default/unexposed.

## D1 / revision 2 — corrections to architecture review

Supersedes only D1r1 items4/6; other decisions remain. No source edit yet.

- All actual principal upsert paths take the ownership prefix BEFORE the first
  principal write: create_agent_principal_on takes company FOR SHARE then agent
  FOR SHARE, before its INSERT/ON CONFLICT. This includes repeated message-author
  upserts, not just new-agent creation. A missing/foreign agent fails before the
  principal write; the existing deferred-FK regression must be updated to assert
  this earlier refusal without weakening its independent commit-constraint proof.
  create_person_principal_on takes account FOR SHARE then company FOR SHARE before
  identity/principal resolution. Existing callers that hold the prefix reuse locks.
- Namespace owner locking is centralized in a database function shared by ensure_on
  and the namespace guard trigger: probe person/agent metadata only to choose locks,
  then account -> company -> agent -> principal/unit, revalidate the exact owner
  kind and metadata, refuse changed/missing/foreign owners. This avoids maintaining
  subtly different ownership lock protocols for direct SQL and application ensure.
  Retired namespaces bypass live-owner revalidation only for non-owner state edits;
  identity/owner columns are immutable. No permanent FK blocks owner deletion.
- Add competing REAL production create_agent_principal_on upsert vs agent delete
  in both orderings, including existing principal, plus same-company/person retire
  and ensure competition; demonstrate ordinary results without deadlock/timeouts.
- D1r1item6's service assumption is contradicted by source: service SaveItem67..79,
  SaveRequest83..97 in services/memory/src/application/contract.rs carry no subject
  or generic opaque provenance, and deny_unknown_fields. Phase01 only adds nullable
  retained delivery unit/principal/subject columns. Subject-bearing service request,
  query and purge support require a narrow opaque provenance contract extension,
  production client/fake changes and affected service tests under a separate reviewed
  p02 architecture gate BEFORE those dependent edits. No existing service capability
  is claimed and phase01 cannot satisfy p02 subject/purge criteria.

Baseline `cargo test --locked --offline --all-targets -- --list` PASS, retained
logs/B1-before-tests.txt. No tests moved/deleted. Fresh owned DBs pa05_p01_schema
and pa05_p01_core_test on borrowedPG51167; private env in external session cache
pa05-p01-env.sh. Never run a second libtest against this core DB concurrently.

Coordination2/05SC-C003r1 ACK: latest user requires main publication after accepted
phase03. Run final source gate before that publication;04 can finish docs/records
and reuse unchanged evidence. Stop after05, no06/Doop scope expansion.
