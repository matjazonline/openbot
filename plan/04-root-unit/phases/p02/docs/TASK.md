# Plan04 p02 — required desk ownership and two access modes

Status: B1 final focused verification; D1/r1 and amended D1/r2 independently accepted.
Current status/evidence is in the last B1 sections below; earlier checkpoints remain historical.
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

D1 review resource checkpoint: fresh empty pa04_p02_schema1 and
pa04_p02_core1_test allocated on owned PG18:51167; external private env
/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa04-p02-env.sh sources base
settings and selects these URLs, task target and GNU coreutils path. Do not run
migrations until the reviewed schema edits are ready. Before-inventory retained
logs/B1-before-test-inventory.txt, listing-only PASS4739tests/0benchmarks.

D1/r1 accepted: independent B04RU-P02-D1-R1/N1 PASS, no findings, decision
SHA158e38af4621c3addba3d98a07a4f1aca0920a66d2da55af4050ac5ded9bc0d4,
source54bfc2c64c4b501ee0539b514f0e0e26432a2ac823b0de55d9ef6a14fd588cdc,
coordination7. Implementation begins on the accepted decision; execution proof
and whole-B1 review remain outstanding. Source freeze ended after gate PASS.

B1 checkpoint: revised init schema migrated successfully on fresh pa04_p02_schema1
(logs/B1-initial-migration.txt). All-target locked/offline compilation PASS after
mechanical required-owner fixture/type fixes (logs/B1-all-targets2.txt,1m45s).
Initial diagnostic failures retained in B1-initial-lib-check/B1-lib-check2/
B1-initial-all-targets logs; no failure is called unrelated. Source now includes
required owner, Members/Public domain and SQL consumers, all-creator ownership/
grant insertion, set-wise delta reconciliation, one revision per desk edit,
required-owner JSON/form/Public controls, relevant ownership/workflow test changes
and docs. First access_mode_ focused run is compiling. Remaining work: migrate old
private-mode fixtures to actual child ownership, add enduring small write-count
and root/child authenticated relay/grant concurrency proofs, run affected focused
suites and full-scale probe, SQLx/CSS/reachability/inventory proof, whole-B1 review.

## D1/r2 amendment gate — source freeze

Actual unverified root-member regression contradicts D1/r1's assumption that the
old email-proof predicate can apply indiscriminately to new membership grants.
Independent decision/proof contract logs/D1-r2-decision.md; coordinate8 proposed
C004/r1 pending B, active C001r1/C002r1/C003r2 unchanged. No user choice pending;
original root-Team and Plan03 security contracts determine the correction. Do not
weaken assertions or blanket-verify fixtures to clear these failures.
B1 core consumer run291PASS/88FAIL/1ignored (17failure-location clusters), retained
B1-core-consumer-tests.txt. Full-scale delta PASS at1745ms join400inserts/0deletes,
23ms removal0inserts/400deletes; setup regressed to78682ms and needs query-plan
investigation. Existing31access-mode cases PASS. All failure logs retained.
Current entire source frozen for D1/r2 review; exact exclusions remain p02/docs and
COORDINATION.md. Before semantic corrections get B PASS, then resolve all other
fixture/consumer failures and execution proof before whole-B1 review.

D1/r2 accepted with root-recomputed sourcea349e3ad...dd6b6 and immutable decision
SHA c3cccfe77156a647c5f9515a491bbcc3ede1c1145d9b2b6fcd6157248c9bbe11;
independent04RU-P02-D1-R2/N1 PASS, no findings. Coordination9 activates C004/r1;
p02 acknowledges it along with C001r1/C002r1/C003r2. Source freeze ended.
Planner-only temp-table experiment (B1-insert-planner-experiment.txt, no FK/triggers,
not an end-to-end claim) shows existing INSERT_MISSING uses indexed nested-loop
anti-joins even at400000rows: custom10.623ms vs explicit-pruned16.346ms; generic
18.023ms vs17.863ms. The proposed redundant explicit INSERT desk predicate is not
adopted: evidence does not justify it. Preserve the same approved logical set and
SQL structure; investigate setup phase timings and remeasure quiescent production
before attributing the79sec setup to this query. All experiment temp data rolled
back. This disposition changes no decision invariant or persistent protocol.

Verification checkpoint after R2: B1-r2-core-security-tests3.txt482PASS/1FAIL/1ignored,
only old Some([]) projection expectation; corrected and followed by
B1-targeted-corrections.txt79PASS/0FAIL/2ignored including all modes/grants and
provisioning_ cases. Existing unverified root attention/notes, email-only proof,
provider/profile repair and actual approval/follow retirement invariants now PASS.
The email-only proof fixture changes actual ownership and preserves configured
rows; no intentionally unverified user/assertion was bypassed. Initial88 failures
and subsequent15/1 failures remain retained. No new failure is called unrelated.
Source file-size guide: extracted channel.rs inline tests to channel_tests.rs,
preserving channel::tests runner identity; before-inventory retained, after-list
and removed-name dispositions remain required. Final SQLx prepare is underway.


## Resumed B1 consumer verification (2026-11-17)

Coordination9 rechecked: C001r1/C002r1/C003r2/C004r1 and inherited both-way
one-step audience remain acknowledged. SQLx final all-target preparation PASS
(B1-final-sqlx-prepare.txt); no .sqlx metadata diff. The old process handle was
not retained by the host; completed command output establishes the result.
Workflow authorization_cases PASS7 (B1-final-workflow-authorization.txt).
Before/after channel test extraction inventories contain exactly the same4746
runner names; removed/added sets are both empty.

The previously retained remaining-consumer run was901PASS/8FAIL/7ignored. A
specific recheck reproduced all8 failures (plus1passing related rejoin case),
B1-remaining-failures-recheck.txt. These are affected Plan04 integration failures,
not declared unrelated or covered by the Doop deferral:

- Rejoin privacy fixtures assumed only configured grants on a default-root desk.
  Give that private fixture real empty-child ownership; preserve its exact two
  configured-grant/principal/thread assertions.
- Knowledge delivery/current-level fixtures inserted the verified email identity
  that root membership materialization already creates. Reuse the existing
  production same-address account-identity refresh after explicitly verifying the
  fixture account, in a shared message-test helper. Retain real delivery/current
  level and classified-subject/body denials.
- Principal deletion used the protected current company owner. Assert that the
  owner deletion is refused, transfer to a genuine replacement member through
  the existing ownership helper, then exercise the same principal deletion and
  immutable audit/live-authority assertions.
- The direct raw principal demotion racer acquired principal before the new root
  company/role trigger. Reuse revoke_membership_now, which maintains the existing
  account/company/principal order without workflow delivery cancellation; keep
  the real competing first-dispatch and reconciliation assertions.
- Profile deletion already translated immediate foreign keys to Conflict, but
  its deferrable session/workflow FK fires at COMMIT. Apply that same error
  mapping to commit as well as DELETE; no constraint is weakened. The original
  referenced-profile denial test is the regression proof. This is a correction
  to an integration failure exposed by the affected private-session checks.

Correction test compilation/execution pending; preserve all failure output.


### Actual generic-plan setup regression and bounded anti-join

B1-remaining-failures-corrected2.txt PASS9, including all8 reproduced failures.
An initial compile-only path-resolution failure is retained in corrected.txt.
The explicit quiescent B1-final-scale.txt reproduces setup78682ms (members2515ms,
first50desks62804ms, remaining150desks13336ms), join1770ms400insert/0delete,
removal20ms0insert/400delete; all grant/revision assertions PASS.

Actual production sampling B1-scale-active-query.txt observed INSERT_MISSING
waiting on BuffileRead. A second fixture run is diagnostic, not a quiescent
benchmark: B1-scale-plan-fixture.txt includes a concurrent rollback-only EXPLAIN
of the actual source query on the same real migrated grant table. Immutable
B1-live-reconciliation-plan.{sql,txt} captures46000rows/table5758976B/index13123584B,
work_mem4MB and forced-generic plans with2000desired grants. Original anti-join
materializes44000company rows, rejects85999000join pairs and takes4021.206ms;
adding the already-known desk set inside the permission subquery materializes
2000rows, rejects1999000pairs and takes97.553ms. Both insert0/conflict0on an
already-complete desk and roll back; this proves the read-plan defect, not new
row trigger or end-to-end cost. Real fullscale creation/join measurements cover
those separately.

This contradicts the earlier synthetic temp-clone experiment's applicability:
that table missed the fresh-table statistics/real prepared-plan shape. The
responsible SQL now explicitly prunes permission.channel_id=ANY($2) inside the
anti-join. Logical desired set, row locks, revision steps and unique conflict
handling are unchanged; no schema/index/fallback or architecture decision changes.
Rerun exact fullscale and affected grant, production create/provision, HTTP and
consumer correction checks; regenerate SQLx before B1 freeze. Remove the obsolete
SubmittedChannel::write stored-value parameter and its callers while completing
the required-owner simplification.


### Bounded anti-join fullscale result

B1-pruned-scale.txt explicitly exercised PASS1/0FAIL/0ignored at1000members,
200desks,400000grants: setup15842ms (member2628ms/desks13178ms), join1486ms,
400insert/0delete; removal17ms,0insert/400delete. All200desks move exactly one
revision for both transitions. Compare the untouched-query quiescent setup78682ms,
join1770ms/removal20ms in B1-final-scale.txt and original full-rewrite baseline
join12252ms/400400insert/400000delete in baseline-scale-run2.txt. No bound raised.
Shared ordinary CI delta/competing tests remain discoverable. The historical
fresh-table generic-plan cause and rejected temp-clone assumption are recorded
in new .trail/learnings/2026-11-17-bound-grant-anti-joins-for-fresh-mac02.md.

Source reachability PASS8 (B1-source-modules.txt); CSS rebuilt and current
(B1-final-css-{build,check}.txt), no compiled asset diff. HTTP21 proof uses exact
runner names in B1-http-authorization-exact-proof.json. B1-test-dispositions.json
maps every one of37removed baseline names to discoverable replacements, with
removed optional-owner/members_only behavior explicitly noted. The extracted
channel test module loses/gains no test names. Final focused stock2MiB run is
pending (B1-final-focused-command.json); includes exact HTTP21 names, production
provisioning, grants/revisions/competing root join-create, changed consumer
corrections and the previously unexercised renamed escalation ownership case.


## B1 acceptance mapping for whole-batch review

D1/r1 plus amendment D1/r2; active coordination9/C001r1/C002r1/C003r2/C004r1.
No schema/policy decision changed after the amendment. The explicit permission
subquery desk bound is logically redundant and fixes the measured generic plan.

| Criterion | Production path / decisive case | Actual decisive assertion and retained proof | Status |
| --- | --- | --- | --- |
| AC1 Required tenant-scoped owner | ChannelWrite, insert_channel_row_on, write_channel_settings_on, JSON/HTML; channels_department_fk | access_mode_raw_null_owner_and_removed_modes_are_refused rejects SQL NULL/old modes; access_mode_json_department_rejects_explicit_null_and_invalid_ids; access_mode_json_owner_is_preserved_and_can_be_reassigned_but_not_cleared; ownership foreign-company/deletion/teardown cases in final focused log | PASS |
| AC2 Members/Public policy and UI | Channel::{participant_access,viewer_access}, channel_access, ChannelForm, required department selector | access_mode_ domain matrix: root grant, private child/admin refusal, owner exception, outsider Public mail/no reads, exact current email authority; HTML/JSON/Public checkbox assertions; current CSS build/check | PASS |
| AC3 Every creation producer establishes initial derived grants | insert_channel_row_on is the shared normal/native/plain-department boundary; provision_department_desk_on reassignment | access_mode_grants_default_root_desk_admits_root_members_but_child_requires_its_grant; native production initial-commit assertion in final14-case log; real department lead/context journeys in final focused log | PASS — B1-final-relay-and-initial-grants2.txt |
| AC4 Atomic delta reconciliation and audience | company root join/retire, department member/role/tree changes, ownership writes -> sync_department_grants_on/rederive_on/reconcile_grants_on | access_mode_grants_join_and_removal_write_only_the_delta asserts changed row counts, stable existing grants/configured provenance and exact revisions; access_mode_grants_competing_root_join_and_default_desk_creation_keep_every_grant uses competing real production callers; one-step own/parent/direct-child tests | PASS |
| AC5 Current UUID placement versus email proof | Channel::has_capability; locked workflow association; notification_principal_can_view; workflow_follow_reviewer_eligible | access_mode_grants_sql_and_domain_keep_membership_but_withhold_unverified_email_grants; original unverified team attention/notes journey, security_email_identity_ and actual reply/follow/retirement races in r2 core and targeted logs; no blanket proof changes | PASS |
| AC6 Authenticated relays require receiving participate grant everywhere | relay_desk_can_participate plus current source proof, incarnation, receiving grants and routing fences | real root/child agent allowed with participate, view-only/revoked refused without canonical hop/SMTP; unproved/system/foreign/cycle/hop/current identity tests; final14-case relay recheck | PASS — final relay recheck14PASS with initial-grant/delta proofs |
| AC7 Affected consumers and tests remain collected | workflow authorization, private/direct/history, knowledge/memory/notification, local machine controls, provisioning and HTTP | r2 core482PASS/1FAIL corrected by targeted79PASS; remaining901PASS/8FAIL corrected9PASS; final stock-stack206PASS; exact HTTP21 successful runner names;37removed names mapped to discoverable exercised replacements; extraction loses0names | PASS |
| AC8 Materialized scale acceptable without fallback | real root invite acceptance and removal at1000members/200desks | B1-pruned-scale PASS:400000initial,400join inserts/0deletes,0removal inserts/400deletes; all200revision+1. Setup15842ms/join1486ms/removal17ms; original rewrite join12252ms and unwanted800400mutations. Actual generic EXPLAIN/mutation/setup evidence distinguishes plans and writes | PASS |
| AC9 Focused build/interface verification | fresh init schema, runtime SQL, Cargo targets, source reachability, UI bundle | schema2 migrated; SQLx all-target locked/offline preparation PASS; final new-test prepare3 PASS; fmt/whitespace, source modules8PASS, CSS current; final stock2MiB focused206PASS/0FAIL/3ignored | PASS — final SQLx prepare3 completed |

Stock-stack ignored cases are explicitly the separately executed scale probe, browser
fixture generation and live memory-service journey. They are not206executed checks.
Final full suite, external integration/network/stack/lease gates remain p03 work;
whole Plan04 completion/publication is not claimed by p02 acceptance.

### Contract impact / unchanged obligations

Required owner producers include normal HTTP/use-case creation, manual/native
assistant provisioning and plain/agent department desks; all converge on the shared
insert boundary. Consumers include CHANNEL_SELECT/ChannelDb/domain entity,
workflow role binding/context/snapshots, routing/address-directory, JSON/HTML and
fixtures. Optional workflow company/principal contexts remain optional; a required
desk owner must not silently give those a department.

Members/Public and provenance authority flow through domain policy, SQL association,
notification/follow/relay predicates and their callers; preserve current membership,
principal incarnation, classification, tenant concealment, source proof, row/gate
locks, cancellation and direct-history privacy. Native effect account-union/NOWAIT
protocol is unchanged. Private fixtures change actual ownership rather than add
email-proof or owner bypasses. Grant materialization retains company -> principals
-> sorted desk gates/rows lock ordering, independent configured/manager provenance
and exactly one revision fence per affected desk. See original captured caller and
literal inventories plus final-reconcile-callers.txt and final-submitted-write-callers.txt.

### Trail check disposition

B1-trail-check.txt exits1 with one lexical conflict against l-2cfeeacd (shared
browser cleanup), pointing at init schema14087's existing deferrable native-profile
foreign key and comment about separate tenant cascade owners. This is a false
positive: the learning forbids pruning unseen concurrent browser storage keys;
Plan04 changes no attribution browser storage/cleanup code or its concurrency
contract. The cited native cascade FK is preserved, not removed or weakened.
This explanation must also appear in the phase commit message as required by Trail.
Other advisory matches are explicitly nonblocking. The checker warns its400KB
branch-diff cap; independent B1 review still covers the complete batch diff from
334e79c7, not just the clipped Trail heuristic.


## B1 freeze / review entry

Final stock2MiB native initial-grant, exact unchanged grant snapshot, authenticated
root/child relay, current source identity and target grant races PASS14/0FAIL/
0ignored,25.06s (B1-final-relay-and-initial-grants2.txt). Final all-target SQLx
prepare3 PASS22.69s; no metadata diff. Earlier private-trait compile diagnostic is
retained. Final runner inventory4747: exactly the previous4746plus the explicit
native initial-commit regression; no further removed names. Every mapped removed
name has a discoverable exercised replacement. Final206-case HTTP runner output
contains all exact21authorization names, proved by B1-http-authorization-final.json.

No owned test/scale/SQLx process remains. Whole-B1 architecture and behavior checks
are complete; p03 full integration gate remains separate. Source must remain frozen
for independent review. Final source snapshot, coordination9copy, evidence manifest
and validator results follow in logs/B1-R1.*. Exact exclusions p02/docs and exact
Plan04 COORDINATION.md. Preserved earlier p01/integration workflow records remain
part of the digest and are not implementation changes in this batch.

Frozen B1-R1 artifact **7ce85fba312130879a9e00c49febd3e48f54e71f42483284637524f51ed72539**, base/start/review-base/HEAD334e79c7. Exact exclusions `plan/04-root-unit/phases/p02/docs` and `plan/04-root-unit/COORDINATION.md`. Immutable coordination9 in logs/B1-R1-coordination.md. Evidence manifest logs/B1-R1.evidence.json retains failed runs and their correction evidence; Trail conflict is explicitly disposed above. Await independent whole-batch review before source edits or acceptance.


## B1-R1 correction — 04RU-P02-B1-F1

Independent whole-batch review04RU-P02-B1-R1/N1 CHANGES REQUIRED for source7ce85fba…72539;
only blocking findingF1: live classic create/edit forms still produce the old text
marker and omit Public/required owner controls, while their shared parser expects
the new checkbox. This can silently close an existing Public desk on an unrelated
save. AC2/equivalent-surface AC7 are reopened; other covered evidence is retained,
not accepted as a separate batch. Source freeze is released for one cohesive F1
correction. No architecture or coordination revision is required.

Correction approach: reuse the canonical channel field renderer for all live
classic and modern producers; load/pass departments at all classic edit callers;
remove the obsolete classic marker-based spam helper/event case rather than
support two form contracts. Exercise real rendered create/edit controls through
submission and verify Public unchanged/off, owner selection/preservation and spam
confirmation. Preserve JSON's canonical @public API representation. Run affected
page/route/HTTP21 and native browser protocol checks, then freeze a new artifact
and ask B for one F1 recheck with unchanged contract evidence reused.


## Latest user scope — stop after Plan04 publication

Explicit user2026-11-17: "stop after 04 push". Authoritative coordination10,
C002/r2 acknowledged. Finish04/required verification, push main, then STOP;
do not start05or06. Earlier continuation-through06 text is historical and
superseded. C001r1/C003r2/C004r1 and one-step-both-ways policy are unchanged.
F1 correction remains active; narrowing continuation scope waives no04gate.


## F1 correction evidence and recheck scope

F1 implementation is complete, awaiting independent recheck. Live classic create
and edit now reuse the canonical ChannelFields/channel_fields renderer; all three
edit callers provide real departments. Named addresses exclude the projection's
@public marker; Public has its own checkbox and required owner. The obsolete
classic spam function/markup and dead event cases are removed. The one canonical
spam handler reads Public and synchronizes after native reset. Existing JSON
@public representation, receiving-agent grants and every SQL/domain policy fence
are unchanged. User-facing spam text describes Public intake rather than a hidden
serialization marker.

Verification uses two existing locked parser packages as pinned dev dependencies
(html5ever0.27/markup5ever_rcdom0.3); Cargo.lock adds only two root dependency edges,
no package/version changes. Real GET-generated controls are parsed and URL-encoded
through real classic create/edit handlers on PostgreSQL. New tests prove missing
confirmation refuses without a created row/renamed desk, selected root/default
and explicit child ownership, unchanged Public/name edit, checkbox-off Members
and exact retained named addresses. They read actual checkbox values from the
rendered control and cannot supply a nonexistent checkbox silently.

The first parser fixture returned Handles after its RcDom owner had dropped;
RcDom's root Node::drop drains child lists, so selected options disappeared even
though attributes remained. Retain diagnostics; keep the owner through traversal.
This was a test-helper ownership error, not missing production departments. The
first broader run168PASS/2FAIL found obsolete old tag/@public text-field assertions;
updated their actual wording/projection contract, retaining all refusal assertions.
B1-F1-focused-tests2.txt now170PASS/0FAIL/2ignored on stock2MiB and includes all exact
HTTP21names. The two new real-handler cases pass in that run.

The native browser follows all four real classic/modern create/edit producers,
shipped app.js, production CSP and vendored assets. It checks native FormData,
accessible Desk Name label, Public preservation/off/reset, additive addresses,
required default/selected/reassigned owners, confirmation enable/clear behavior,
and actual field styles in both themes. Classic fixtures use their real base
layout (4px radius/2px outline offset); modern panes use real UiShell (8px/-1px).
The earlier style probe incorrectly hosted modern panes in base_layout and used
workspace expectations for classic controls; retained as failed fixture evidence.
This changes no production style tokens or platform bound. Lazy binding markup
and the workspace's vendored SSE asset are served explicitly by the fixture,
never hidden by suppressing unexpected requests. Final B1-F1-browser-tests5.txt
PASS2/0FAIL with diagnostic successful lines for all four variants. The fixture
producer PASS1 is explicitly exercised under --ignored; no ignored-pass claim.
These are browser control/serialization proofs plus real-handler DB proofs, not
a claim of a browser-to-live-server authentication journey.

Fresh all-target locked/offline compilation PASS (B1-F1-all-targets.txt,1m57s);
new source-module reachability PASS8 (B1-F1-source-modules.txt). SQL/schema bytes
and .sqlx remain unchanged by F1, so prior migration/prepare/security/performance
proof remains applicable. A macOS test linker emits a nonfatal compact-unwind
16MB warning during fixture generation; retained, no bound/configuration raised.
Final runner inventory is4749, exactly4747plus two classic form regressions; no
old name was removed. CSS rebuilt/current. Final formatting/whitespace/Trail and
new frozen source/evidence pins follow below; no whole-suite rerun in p02.

Authoritative coordination10 rechecked. ACK C002/r2: finish04/required gates,
push main and STOP; do not begin05/06. Other notice/policy revisions unchanged.
Recheck only F1's renderer/caller/form/spam/reset/style/fixture/dependency delta
and affected integration, reusing B1-R1's unchanged whole-batch coverage. B owns
finding closure. Plan04 full p03 gates/publication remain outstanding.


Staging exposed whitespace in byte-pinned generated logs/HTML, which were formerly
untracked. Extend the repository's existing Plan03 evidence .gitattributes pattern
specifically to Plan04p02logs (-text/-whitespace), preserving exact output hashes;
product source/deliverable docs remain under normal whitespace checks. No evidence
was rewritten. Include this retention-only attribute change in F1 recheck scope.

F1 recheck source FROZEN at **6391c0ba46d382417a618bdc9238b32c1b8e1b976a457ee42f60b2382971e660**, base/start/review-base/HEAD334e79c7. Entire implementation staged before snapshot; exact exclusions unchanged p02/docs and exact Plan04 COORDINATION.md. Coordination10 copied to B1-R2-coordination.md; C002r2 acknowledged. Immutable staged implementation diff retained. Manifest B1-R2.evidence.json carries unchanged original coverage plus new correction evidence; validator result follows. Independent F1 recheck pending; no source edits until verdict.


## Accepted in phase p02 — 2026-11-17

Independent04RU-P02-B1-R2/N1 PASS for exact source6391c0ba46d382417a618bdc9238b32c1b8e1b976a457ee42f60b2382971e660; F1 fixed, no new findings. B reused original whole-B1 coverage, independently reran native browser2PASS across4variants and confirmed final source/coordination pins. Root independently recomputed the digest, validated retained evidence and rechecked coordination10/C001r1/C002r2/C003r2/C004r1. No blocking p02 defect remains. Accept B1 in phase and commit locally; main integration/publication and all p03/full gates remain pending. Doop remains explicitly deferred, not whole-CI green. User latest scope: finish04, pushmain and STOP; no05/06.
