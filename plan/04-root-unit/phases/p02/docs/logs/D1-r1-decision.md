# Plan04 p02 — required desk ownership and two access modes

Status: D1/r1 architecture review preparation; product choice resolved.
Workflow implement-astra-reviewed-2-3, phasep02. Root/A /root user-confirmed
Sol6.1/xhigh; retained independent B /root/p04_reviewer requested Astra/high,
actual settings unexposed. Root owns TASK/HANDOFF/coordination, B review/notes/findings.
Worktree /Users/mac02/orca/workspaces/openbot/plan03-04, branch feature/plan03_04.
Start/review-base/HEAD334e79c74b383dffd8e0d565c33a77c6ac24c94d. Preserve existing
uncommitted workflow-only scope/publication record updates, no preexisting dirty source.
Exact snapshot exclusions this phase docs directory and exact Plan04 COORDINATION.md.
Context controls unexposed; retain durable checkpoints and same author/reviewer roles.

Authority /Users/mac02/orca/workspaces/openbot/plan03-04/plan/04-root-unit/COORDINATION.md,
revision5 checked at startup, revision7 on continuation; root writer. ACKNOWLEDGED04RU-C001/r1 (accepted main
integration334e79c7/source09f2972f…d22c4a8; keep canonical membership/fences and
attribution terminal evidence),04RU-C002/r1 (finish04/push main, then05/06 in order,
no scope beyond06), inherited01DEP-C002/r1 USER2026-11-10 (one step BOTH WAYS).
ACKNOWLEDGED04RU-C003/r2, explicit user2026-11-17: require receiving-desk grants
for real authenticated agents on every Members desk, root and child; supersedes
the inherited01DEP-C001 trusted-relay bypass. No product choice remains pending.
Futurep03 and Plan05 must acknowledge this active notice.

## Original scope and dependencies

Original plan/04-root-unit/README.md and02-access-modes.md. p01 accepted d4bbde66,
source2a5714f5…7ab793, independent04RU-P01-B1-R2/N1. Imported main03f0bda2 merged60b9a28b;
small authority/terminal-evidence integration accepted334e79c7, source09f2972f…d22c4a8,
independent04RU-MAIN-B1-R1/N1. Current pre-p02 libtest4738 retained in integration logs.
Doop C008/r1 remains explicitly deferred under fix-doop; new affected failures are
not waived. No full CI-green claim. Current real SSO/native/manual browser/live-memory
journeys remain for their separate required integration gates.

One cohesive B1 covers required department ownership/default root, Members|Public,
removal of members_only, canonical email/explicit grant behavior, all materialized
department grant producers/consumers and UI/create/edit/fixtures/docs. Preserve tenant
boundary, owner exemption, principal-incarnation pins, current role/classification
and one-step audience BOTH WAYS (own members/all own role holders,parent Lead,direct
child Leads). Root membership alone never gives all child-department membership.
No old value aliases, nullable stored owner fallback, dual writes/backfill or rollout.

Read root/src/persistence/application/HTTP/pages/infra/SMTP instructions and selected
skill verification/coordination references. Current startup pre-scope retained in
logs/startup-prescope.md; it is an investigation aid, not an accepted architecture.
Trace every affected shared symbol and data-string/SQL consumer before editing.
Architecture D1 with contract/failure/proof mapping must pass B before dependent edits.

## Verification plan

- Focused per B1: access_mode_ domain/storage/interface matrix, access_mode_grants_
  join/removal/provenance/revision fences and competing creation/membership actors,
  one-step both-direction boundaries and real named external principal grants.
- Include required HTTP21authorization suite after this batch and affected private
  thread/direct/relay/knowledge/memory/notifications/workflow controls; select from
  actual impact, retain failures and exact test runner inventories/replacements.
- Measure1000members×200root desks before acceptance, including real join/removal,
  changed grant rows and one revision bump per affected desk. Keep existing bounds.
  Avoid full cartesian allocation/rewrite when the responsible SQL layer can own it;
  do not adopt the permitted department-reference fallback without actual measured
  need and a revised architecture gate.
- Fresh disposable init migration, SQLX_OFFLINE=false locked/offline all-target prepare,
  meaningful source reachability, formatting/whitespace, Trail graph/checks, immutable
  snapshot+evidence and ONE whole-B1 independent review. No layer-by-layer review.
- Final full scripts/check.sh/CI omissions, stack/lease budgets and external journeys
  once after p03 integration; do not repeat full suite per batch.

## Resources / next

Base env /Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-p04-env.sh;
Node24/GNU/Python3.13/PG18 localhost51167/task target/en_US.UTF-8. Next allocate
fresh pa04_p02_schema1/core1_test when schema changes; preserve p01/m01 DBs and
source artifacts. Task Docker profile openbot-plan04 via external pa04-docker-env.sh,
default context unchanged. No task Cargo/test process active. Do not stop others.
Next finish exact caller/data impact, pin original/inherited requirements, write D1,
criterion mapping and get independent architecture PASS before new model/schema edits.

## Pending relay product clarification / independent scale investigation

Async user clarification is pending: old explicit01DEP-C001 trusted-relay bypass
versus current Plan03 receiving-agent grant rule (shipped inter-channel guide and
relay_desk_can_participate). Options: grants every Members desk (recommended),
trusted real agents only on root desks, or on all Members desks. Do not assume an
answer from elapsed time; keep dependent relay policy/D1 acceptance pending.

Independent baseline probe added in access_mode_scale_tests.rs, registered under
existing department desks tests. It exercises UNCHANGED accepted production code,
using existing explicitly root-owned members-only desks as a faithful materialized
root audience at1000members/200desks, before any p02 schema/model/policy edits.
It owns its database, seeds before measuring, records actual invite join/removal
time and per-statement transition-table insert/delete counts (no per-row timing
distortion), and asserts cardinality and one revision step per desk. Explicitly
ignored scale probe must be run with--ignored; no default-ignored PASS claim.
Baseline SQLx all-target prepare PASS; retained logs/baseline-scale-sqlx-prepare.txt.
New schema/model/permission changes still require cohesive D1+B architecture PASS.

### Baseline probe correction

`cargo test --locked --offline --lib access_mode_grants_scale_1000_members_200_desks
-- --ignored --nocapture` initially FAILED (logs/baseline-scale-run.txt): the new
fixture created an unverified candidate account, and production invite acceptance
correctly returned None before joining. No grant performance claim follows from
that run. The candidate now explicitly verifies its current address. The second
run also retains a representative EXPLAIN (ANALYZE, BUFFERS) of the production grant
selection predicate and actual table/index sizes before timing. That read plan
does not prove mutation/trigger cost; real join/removal timings and statement
transition counters establish those separately. No permission check was weakened.

## D1/r1 architecture — submitted for independent gate

Product input resolved by explicit user2026-11-17, "require grants as recommended".
Every Members desk requires an actual authenticated sending agent's participate
grant, root desks included. No bypass. Retain Plan03 source proof and all tenant,
incarnation, hop and cycle guards. Public intake remains Public. Historical
pending-choice text above and C003/r1 record the investigation, not current policy.
Current decision identity04RU-P02-D1/r1 includes D1-A/B/C and the refinements below.

### D1-A Required owner, one access discriminator

Proposed: `Channel.department_id: DepartmentId`, `ChannelDb.department_id: Uuid`,
and SQL NOT NULL; default creation to the existing canonical `company::root_on`
inside the established company write lock. Keep the composite company/department
FK and RESTRICT deletion. Explicit SQL NULL/foreign-tenant ownership must fail;
do not add a trigger that silently turns explicit NULL into a valid owner.
Every raw SQL fixture must supply its real owning department.

Collapse optional ownership edits to `ChannelWrite.department_id:
Option<DepartmentId>` (None means unchanged on edit, root on create). Remove the
single-purpose `ChannelOwnershipWrite`/`DeskOwnership` layers together with the
members_only flag and detach APIs; `set_desk_ownership_on` receives DepartmentId.
No stored optional owner, old-value aliases, compatibility decoding or backfills.
JSON and HTML creation/editing use the same required-owner rules. Existing
company/principal workflow contexts may still lack a department; do not confuse
that with a desk's now-required owner.

Use Members|Public, parse only members/public. Reuse @public email normalization
and configured_allowlist provenance. Members participant authorization is a
participate grant or owner; Members viewer additionally requires current company
membership and a view grant, with owner exemption. Public accepts all mail,
trusts listed/current company members, and admits only current company members to
UI reads. Public department desks still carry derived grants. Root membership
alone never admits a person to all subdepartment Members desks.

### D1-B Established transactional materialization

Retain company -> eligible principal key shares -> sorted desk gates -> desk rows
ordering and current principal-kind/incarnation/security fences. Reuse ELIGIBLE's
own members/all own role holders/parent Lead/direct child Leads, no further step.
Visit every owned desk on relevant membership, role, tree and owner changes,
including Public and default-root desks; configured/manager grants remain intact.
All creation producers must establish the required root ownership AND initial
derived grants, including native assistant provisioning (which currently calls
insert_channel_row_on without sync) and department/plain desk provisioning.
Deletion races keep the current missing-desk skip and revision-fenced commit rule.

Measured baseline (logs/baseline-scale-run2.txt): PASS, real PG18, 1000 members,
200 desks, 400000 derived rows. Fixture setup16212ms. Real verified invite join
12252ms, inserted400400/deleted400000 rows; real removal21ms,
inserted0/deleted400rows. Both cardinality transitions and exactly one revision
step on all200desks passed. Table48611328B/indexes81526784B; grant-selection
EXPLAIN (ANALYZE,BUFFERS) actual400000rows,50.018ms,5934sharedbufferhits.
This isolates join's800400row mutations from a roughly50ms selection. Current
member retirement already removes only the departing member's grants; preserve it.

Proposed decision: the12.25second synchronous join/full rewrite is unacceptable
at the plan's required scale. Retain materialized per-principal grant rows and
change set reconciliation in this same responsible SQL layer, preserving unchanged
grant rows and provenance. Compare missing/stale principal/capability rows set-wise,
never loop a query per desk/principal. Enduring regression proof must assert400
inserted/0deleted for this join and0inserted/400deleted for removal, not only their
final400000row cardinality. Preserve one settings_revision step for each affected
desk in the membership transaction.
The plan's department-reference grant fallback requires measured need and a new
explicit decision revision, not a speculative extension point.

### D1-C Consumers and proof plan

| Contract | Producers / consumers / disposition | Decisive focused proof |
| --- | --- | --- |
| Required department | channel create/edit, native assistant and department provisioning; domain/DB/JSON/HTML projections, workflow role-context readers and direct SQL fixtures | create default root; create/edit/read explicit owner; NULL and foreign company fail without partial effects; reassignment replaces former detach tests |
| Members/Public | Channel policy; channel_access/CHANNEL_SELECT; workflow association; SQL notification_principal_can_view/workflow_follow_reviewer_eligible; relay SQL pending product choice | access_mode_ matrix, SQL/domain equivalence, owner exception, unlisted member refusal on child desk, Public outsider mail accepted but UI refused |
| Materialized audience | sync_department_grants_on/sync_desk_grants_on; invites/root removal, department members/roles/tree, owner edits, principal lifecycle; every channel creation path | access_mode_grants_ real join/removal, exact grants and revisions, configured provenance survival, one step both ways; competing member/create and ownership/update commits |
| Privacy fixtures | existing raw old-allowlist updates no longer mean private under root | move restricted fixtures to an actual nonroot department with no unwanted members; retain real named principal grants and decisive stranger/colleague denial assertions |
| Workflow ownership | role binding, follow reviewers, escalation, department lead/reviewer snapshots | department reassignment refreshes/restricts current grants; historical role-context identity remains; company/principal missing-context checks remain |
| Interfaces | JSON routes, HTML draft/settings and required department selector, @public checkbox, participant list | focused create/edit route tests and HTTP authorization21; label Open to anyone / Also allow these addresses; no clear-owner/members_only control |
| Affected consumers | private threads/direct histories, relay identity, knowledge/memory classification, notifications, workflow admission/decisions/actions and local-agent management | select exact tests from captured caller/literal inventories, preserve incarnation/classification and current authorization failures; do not relabel new failures unrelated |
| Scale | isolated real invite acceptance and RemoveCompanyMember at1000members/200rootdesks | cardinality400000; join400new grants/removal400 fewer; all200desks revision+1; elapsed time/actual inserted/deleted row counts and representative plan/statistics retained |

Captured impact sources: logs/{grant-sync-callers,access-mode-callers,
access-mode-uses,access-mode-type-uses,members-only-uses,ownership-callers,
preferred-approver-uses,unindexed-mode-surfaces}.txt. Additional continuation
caller trace insert_channel_row_on includes native provisioning and test rivals;
prelock_ownership_on includes channel_edit, plain department provisioning and
existing ownership writers. Continuation retained logs/participant-access-callers.txt,
viewer-access-callers.txt and channel-row-insert-uses.txt. Ambiguous policy symbol
names cause graph-edge undercount: logs/policy-method-uses.txt exhaustively captures
88literal hits across23files, including workflow authorization and address-directory
consumers absent from the caller edges. Only3current raw channel INSERT symbols were found (production
insert helper plus address-race and schema-provenance tests), not an assumed large
fixture migration. Preserve both producer and data-string SQL coverage.

Before B1 freeze, map actual test names/assertions/results, enumerate all removed
runner names and their discoverable replacements (baseline4738 plus new ignored
scale probe), refresh SQLx against a fresh p02 schema, and run the required focused
HTTP21 suite. Final full verification remains after p03, once.

### Continuation checkpoint / verification

- Corrected scale probe PASS (1exercised/0failed/0ignored,4738filtered), retained
  logs/baseline-scale-run2.txt; initial fixture failure retained separately.
- SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets --locked --offline PASS
  after the new runtime SQL; logs/baseline-scale-sqlx-prepare2.txt, no metadata diff.
- cargo fmt --all -- --check and git diff --check PASS. No new model/schema/policy
  edit or full suite execution, architecture/batch review not yet submitted.
- Baseline checkpoint HEAD334e79c7, source54bfc2c64c4b501ee0539b514f0e0e26432a2ac823b0de55d9ef6a14fd588cdc;
  exact exclusions p02/docs and Plan04 COORDINATION.md, retained
  logs/baseline-scale-final.snapshot.txt. This identifies the measured test source,
  not architecture or phase acceptance. Snapshot invocation needs GNU coreutils
  bin path before macOS realpath; the earlier attempt failed and is not evidence.
- Task-owned scale database was automatically dropped. No owned Cargo/test process
  remains. Preserve existing p01/m01 databases, task PG and Docker resources.

### D1/r1 ownership/reconciliation refinements

- One production creation boundary, insert_channel_row_on, resolves the requested
  department or canonical root under prelock_ownership_on, inserts its required
  owner and synchronizes initial grants in the same transaction. Remove callers'
  conditional prelock/sync so native, normal and department creators cannot omit
  the invariant. A new uncommitted desk cannot be routed to by another transaction;
  inserting its row before its own initial grant gate cannot invert a competing
  committed-desk gate. Existing-desk edits preserve the established complete lock
  prefix and pass the resolved owner into the settings write.
- Reconciliation retains company serialization, eligible principal key shares,
  sorted desk gates, and row locks. With these held, delete only derived rows absent
  from the desired (channel, principal, capability) set, and insert only missing
  derived rows using set-wise SQL/anti-joins and existing unique-key ON CONFLICT.
  Other provenances and unchanged row identity/created_at remain untouched. No new
  persistent staging table, permission resolver, cache or department-reference type.
- All normal edits synchronize grants, including unchanged department/Public edits.
  Use that one sync's settings_revision update for the entire edit, removing the
  settings/ownership writer's duplicate increment. Each affected desk changes once;
  a failure in any setting/address/binding/grant/role-follow stage rolls everything
  back. Ordinary root retirement's efficient existing removal path stays intact.
- Explicit API null owner fails deserialization; missing owner is allowed for root
  default on create / preserving owner on edit. UI selects an existing department
  with root selected by default. No clear-owner flag or old field aliases.
- Update private fixtures to real restricted child ownership; a root Members desk
  with configured addresses is still open to its root membership. Address-race tests
  relying on unguarded insert_channel_row_on must exercise actual new company-lock
  serialization or direct SQL namespace constraints without creating a barrier that
  deadlocks the now-correct production lock. Retain competing production create/join
  proof. Reassignment replaces old detach behavior; existing non-desk workflow
  missing-context restrictions remain.

### D1/r1 review identity

Coordination revision7; ACK C001/r1,C002/r1,C003/r2 and inherited01DEP-C002/r1.
Immutable decision copy logs/D1-r1-decision.md and coordination copy
logs/D1-r1-coordination.md. Phase start/review-base/HEAD334e79c7. Review snapshot
logs/D1-r1.snapshot.txt; exact exclusions p02/docs and Plan04 COORDINATION.md.
Only the new measurement probe and preserved earlier workflow record updates are
uncommitted; no model/schema/policy implementation before the gate.
