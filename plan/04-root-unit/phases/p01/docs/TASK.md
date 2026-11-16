# 04RU p01 — root department and company membership

Status: B1 corrections and focused verification in progress; D1/r6 accepted at decision level; D1/r7 proposed after new real-schema failure.

## Assignment and authority

Original requirements: plan/04-root-unit/01-root-department-and-membership.md and Plan04 README.
Workflow: implement-astra-reviewed-2-3. Phase ID p01. Root/A `/root`, user-confirmed
Sol 6.1/xhigh; independent B `/root/p04_reviewer`, Astra/high, retained after completed
Plan03 R17. Root owns TASK/HANDOFF/coordination; B owns REVIEW/notes/findings.
Record root: /Users/mac02/orca/workspaces/openbot/plan03-04/plan/04-root-unit/phases/p01/docs.
Worktree /Users/mac02/orca/workspaces/openbot/plan03-04; branch feature/plan03_04.
Start and review base: 8fdb892b6e279ed0c09150e2336bbdc7462f877d. Clean baseline, no
preserved dirty source. Snapshot exclusions: this phase docs directory and exact
plan/04-root-unit/COORDINATION.md only. Context controls unavailable; durable records.

Authoritative coordination: /Users/mac02/orca/workspaces/openbot/plan03-04/plan/04-root-unit/COORDINATION.md,
revision1 checked at startup. Plan03 C008/r1 acknowledged: Doop follow-up deferred;
Plan04 proceeds, no whole-CI-green claim. Plan03 prerequisite is published main SHA
8fdb892b above, core artifact d27557291a01be97a3856d6cf1867e06900df104736a9c69f07754b3000f88fb
at c1ccbc73, independent PASS 03PA-P04-B1-R17/N1; later commits records only.

## Scope and batches

One cohesive batch B1 implements original 01A/01B schema and application together.
Exactly one root per company; root membership replaces company_members completely.
Root-only owner/admin roles mirror ownership and determine derived membership.
Membership access levels preserve meaning, owner stays level9. Atomic creation,
onboarding, invites, promotion, levels, retirement and deletion preserve tenant
foreign keys, principal incarnation and workflow/notification authorization.
No old table, view, dual reads, backfill or compatibility layer. Phase p02 owns
required desk ownership and the members/public policy change after p01 acceptance.

## Resources

Environment file external pa03-p04-env.sh under task cache thr_jt43h4qc8d, Node24,
Python3.13, GNU tools, PostgreSQL18 localhost:51167. Existing Plan03 databases and
binaries retained; allocate fresh pa04_p01* databases for changed init migration.
No active runtime/Cargo worker at startup. Docker daemon unavailable locally.
Reviewer and author share checkout; source writes and review freeze serialized.

## Investigation

Read-only pre-scope retained externally in pa04-readonly-scope/: exhaustive
company_members inventory, caller graphs, FK-cycle constraints. Reuse that work.
Current principals and notifications have tenant/member FKs referencing the old
table; root source must preserve these invariants without a new membership table.
Root/src/persistence/application/HTTP/pages AGENTS read. Verification-contract and
coordination references read. Design, criterion mapping and gate follow after
tracing the existing transaction/trigger/caller ownership.

## Final Plan04 gate

After p01, run focused root_department_, root_membership_, root_competing_ and all
HTTP *_authorization_tests.rs. After p02 repeat authorization plus mode/grant
checks and measure 1000 members × 200 root desks. After p03 integration regenerate
.sqlx from fresh migrated disposable schema and run scripts/check.sh plus any
CI omissions: format, whitespace, offline all-target compilation, Clippy, migrations,
source reachability, DB-backed/network isolated full suite, stock stack and lease
budgets, docs/startup paths and Trail refresh/check. Preserve failures and unavailable
checks explicitly. No full-suite rerun per batch.

Next: trace production contracts, write cohesive decision revision D1/r1 and
criterion/assertion mapping, request independent architecture gate before edits.

## Architecture decision 04RU-P01-D1 / r1 (proposed)

One cohesive gate covers schema, membership authority, ownership, lifecycle and the
transaction/lock changes below. No dependent source edits before B's explicit PASS.

### Root shape and bootstrap

`departments.is_root` is immutable after insert. A partial unique company index
and `is_root = (parent_id IS NULL)` establish at most one root and a single tree.
A deferred constraint trigger on companies/departments establishes at least one
root per surviving company and root slug = company slug at commit. The company
update trigger mirrors slug (and name) into root; standalone root slug changes
are refused by the integrity check. Root reparenting/root-flag changes are refused
immediately; deletion is refused by application and deferred DB integrity, which
allows the same transaction to delete a company and its root. No bypass GUC, depth
heuristic or tombstone column. Tenant composite parent FK and eight-level bound
remain; root consumes level1. Ordinary creation/default parent and reparent-to-top
resolve None to the company's root. The root is a plain department, with no
automatically provisioned desk in p01; p02 defaults all desk ownership to it.

Company creation uses one transaction: lock account, insert company, insert root
with company name/slug/system provenance, resolve/create the owner's existing
lifetime person principal, add root member at level9, insert owner role, commit.
Extend existing company persistence with a small transaction bootstrap helper,
reused by test fixture company creation. Onboarding uses the already-created root
and defaults any new department's parent to it; no separate root service.

### Membership authority and foreign-key cycle

Remove company_members completely from init schema and every production/test
query. Add nullable access_level to department_members. Existing principal-kind
guard is retained/extended: root rows require a level, non-root rows require NULL;
root owner is level9. This is enforced by joining the scoped department under
its lock; no cross-table CHECK or caller-supplied root flag.

Add a nullable `root_user_id` FK witness on department_members, derived/validated
by that same trigger from principal.user_id only for a person on the root. It is
NULL for non-root memberships and root agent memberships. A unique
(company_id, root_user_id) proves one root membership per user in that tenant;
a users FK protects the account relationship. Callers cannot forge the witness.
It exists to preserve declarative composite membership FKs, not to create a
second membership source. Root membership remains the sole authority.

Replace principals_company_user_fk with a DEFERRABLE INITIALLY DEFERRED NO ACTION
FK from (company_id,user_id) to department_members(company_id,root_user_id). The
existing opposite member->principal composite FK remains. This intentional cycle
permits principal then membership bootstrap in one transaction, while refusing a
committed live person without membership. NO ACTION prevents deleting historical
principals by removing a membership; lifecycle demotes the person before deleting
its root row. Replace notification recipient member FK with the same root witness
target, ON DELETE CASCADE; retain all tenant/recipient/generated-column checks.
Replace the companies owner-member FK with the same deferred root witness target.
Define these late in init schema after department tables exist.

### One derived read

Define canonical SQL `membership_of(user, company)` returning only live root person
membership facts: tenant/user/principal/root ids, role, access level and created_at.
Role = owner iff companies.user_id matches; otherwise admin iff root admin role
exists; otherwise member. No historical-person authority, cache, view of the old
table, dual read or fallback. It is introduced before earlier SQL policy functions
as STABLE PL/pgSQL (late table binding), keeping one derivation body. Company
persistence's existing company_access and list paths consume it; no extra trait
is needed when their CompanyAccess already carries membership plus level. All
notification/workflow/vault/memory/local-agent reads use the canonical derivation
or root-membership existence when no role/level is required. List paths start
from scoped person principals, not an all-users Cartesian scan.

A function result is not a lockable table. Every current FOR SHARE/UPDATE OF
member path is explicitly traced and rewritten to lock the underlying root
department_members row (and principal/account/company in the established order),
then re-read canonical facts under those locks. Do not replace fences with a
nonlocking function call. Preserve error propagation and all scalar membership
readers, notification RLS/policies/triggers, MCP live predicates and level floors.

### Root roles and ownership

Extend DepartmentRole with Owner/Admin, owner single-holder, admin multi-holder.
A partial unique owner index plus guard_root_roles only admits these on root,
for live person members; owner must match companies.user_id and level9. Existing
Lead/Reviewer rules are unchanged. Deferred owner agreement is checked on company,
root member and role changes, proving exactly one owner mirror for each company.
An UPDATE of companies.user_id requires the new owner's root membership; the
company trigger raises its level to9 and replaces the owner role in the same
transaction. The previous owner retains ordinary membership/admin only if they
already hold admin. No new ownership-transfer UI/API is added.

Ordinary department role surfaces cannot transfer ownership or clear owner;
the canonical company owner fact is the only transfer source. Admin assignment
uses existing management authorization and refreshes the same dependent workflow
authorization/notification state as team promotion today. Root membership removal
through department management delegates to the established company removal
operation for people, so cancellation, memory retirement and vault revocation
are never bypassed; owner removal is refused. Root add of a stranger requires the
invite/join path, not a bare membership insert with an external principal.

### Lifecycle, locks and dependent state

Keep user/account -> company (sorted for multi-company deletion) -> principal
(sorted for multiple targets) -> underlying membership/role -> desk/notification
locks consistent with current producers and removers. Acquire target account
lock during invite acceptance before company/membership writes so competing
account deletion either wins and join refuses, or waits and then removes it.
Do not introduce a new global mutex or increase existing timeouts.

Accept-invite reuses lifetime principal/address resolution, inserts root membership
with the invited level only on initial join, and updates admin role according to
the existing reacceptance behavior without overwriting an existing member level
or owner fact. Promotion changes the root admin role. Level changes update the
root row, preserve owner9, and retain dependent refresh/revocation.

Member removal captures the notice recipient under current locks, revokes delivery/
memory/vault dependents, removes every department membership/role, demotes person
to external preserving historical id, identities and thread participation, and
refreshes workflow consumers in one transaction. The demotion trigger still
removes addresses and department-derived grants, keeping configured grants.
Account deletion deletes owned companies with their trees before retiring any
owner principal; for other companies it uses the same root removal semantics.
Delete company removes desks first, department rows next, principals only after
the tree is removed, all inside the current cleanup transaction; deferred root
constraints permit this teardown and reject partial surviving-company deletion.
Exercise a three-level tree, roles, restricted desks, documents and history.

### Failure behavior and phase boundary

Unscoped/cross-tenant ids, a live person without root membership, forged witness/
level, a second/missing root, non-root owner/admin, foreign owner and direct owner
role deletion fail closed at the DB boundary. Concurrent root creation has one
winner via partial uniqueness; invite/deletion uses existing lock fences and
competing-real-transaction tests. No weakening of recipient FK, member-kind,
principal-incarnation, authorization, grant-source or security boundary.

p01 preserves current team/allowlist/public and optional desk department policy,
while all non-root departments sit under root. It does not materialize new root
desk grants or alter one-step visibility; those are p02's separately gated and
measured contract. Existing subdepartment Lead/Reviewer reach remains until p02
resolves its audience implementation against the original requirement.

## Acceptance mapping (planned; not execution evidence)

| Criterion | Production path | Decisive assertion / focused checks | Status |
| --- | --- | --- | --- |
| R1 exactly one root | company create/onboarding, shape triggers | root_department_: current slug, required root, second root refused; delete/reparent rejected; cross-tenant parent refused | Planned |
| R2 membership derivation | membership_of -> company_access/participant/UI | root_membership_: owner9/admin/member/stranger, actual team authorization, invite/rejoin and level round-trips | Planned |
| R3 owner mirror | companies update/root role guards | owner follows company.user_id atomically, invalid root/non-root role mutations refused, old owner no longer owner | Planned |
| R4 principal/recipient tenancy | deferred composite FKs, witness guard | direct invalid person/root witness/foreign notification insertion refused; valid bootstrap/rejoin retains lifetime principal | Planned |
| R5 retirement and teardown | remove_member/account/company deletion | all tree roles/members/addresses/derived grants removed, configured grants/history retained; three-level company deletion leaves no scoped row | Planned |
| R6 competing claimants | actual accept invite vs deletion and root insert | root_competing_: competing transactions, no leaked membership/person/role/address; bounded workers joined | Planned |
| R7 inherited authorization | HTTP, workflow, MCP, memory/vault/notifications | all *_authorization_tests plus focused membership/level/native-MCP/direct/retirement suites after B1 | Planned |
| R8 no old read/write path | complete literal inventory and macro regeneration | company_members absent from active code/schema, intended historical docs/evidence retained; focused all-target compile/source reachability/.sqlx | Planned |

## Contract impact

| Changed contract | Producers and consumers | Disposition / focused proof |
| --- | --- | --- |
| Membership table -> root rows | company/invite/levels/principal/access context; all 150 inventoried Rust files + SQL policies/triggers | migrate production queries/fixtures; SQL function is canonical derivation; exercise actual readers not only mocks |
| Root roles and owner mirror | company update, department writes, workflow role bindings/escalation, team UI | forbid owner role transfer through generic route; validate admin refresh; role string/exhaustive parser fixtures |
| Member FK locks | delivery projection, vault consent, memory level gates, native MCP, dispatch/retirement | explicit underlying-row locks; competing tests and tenant adversarial tests |
| One tree and depth | department creation/edit/delete, onboarding, UI trees, tests | default None parent to root; update depth fixtures with same bound, no new alias/legacy shape |
| Retired principals | account/member lifecycle, addresses, historical participants | reuse incarnation and cancellation paths; principal/member/role/address lifecycle assertions |

Before changed test names are deleted/moved, retain pre/post runner inventories
and map replacements. Existing raw SQL test fixtures become shared root bootstrap/
join helpers; no production compatibility writes. Regenerate .sqlx against fresh
pa04_p01 disposable schema, then focused compile and checks; full gate after p03.

### Pre-B1 inventories

Current accepted library test runner retains 4705 test names in
logs/pre-b1-libtest-inventory.txt. Binary is the retained Plan03 R18 library
projection, unchanged through published prerequisite; main warning strings are
outside that target. No tests executed by --list. Actual module registration for
all HTTP *_authorization_tests.rs is `authorization_tests`; 21 selected cases in
logs/pre-b1-authorization-selection.json. Initial filename-stem selection found
zero and was corrected before any behavioral check; it is not evidence of coverage.
Old membership/lock/grant callers are retained in pre-b1-* logs for migration and
affected-check mapping. D1/r1 remains unchanged pending architecture review.


---
D1/r1 is historical and superseded by the complete proposed decision below.

## Architecture decision 04RU-P01-D1 / r2 (proposed; supersedes r1)

One cohesive gate covers schema, membership authority, ownership, lifecycle and the
transaction/lock changes below. No dependent source edits before B's explicit PASS.

### Root shape and bootstrap

`departments.is_root` is immutable after insert. A partial unique company index
and `is_root = (parent_id IS NULL)` establish at most one root and a single tree.
A deferred constraint trigger on companies/departments establishes at least one
root per surviving company and root slug = company slug at commit. The company
update trigger mirrors slug (and name) into root; standalone root slug changes
are refused by the integrity check. Root reparenting/root-flag changes are refused
immediately; deletion is refused by application and deferred DB integrity, which
allows the same transaction to delete a company and its root. No bypass GUC, depth
heuristic or tombstone column. Tenant composite parent FK and eight-level bound
remain; root consumes level1. Ordinary creation/default parent and reparent-to-top
resolve None to the company's root. The root is a plain department, with no
automatically provisioned desk in p01; p02 defaults all desk ownership to it.

Company creation uses one transaction: lock account, insert company, insert root
with company name/slug/system provenance, resolve/create the owner's existing
lifetime person principal, add root member at level9, insert owner role, commit.
Extend existing company persistence with a small transaction bootstrap helper,
reused by test fixture company creation. Onboarding uses the already-created root
and defaults any new department's parent to it; no separate root service.

### Membership authority and foreign-key cycle

Remove company_members completely from init schema and every production/test
query. Add nullable access_level and an immutable id UUID DEFAULT gen_random_uuid() to
department_members, with tenant-scoped UNIQUE(company_id,id). The existing
(department_id,principal_id) key stays. The id is a membership incarnation: stable
for repeated acceptance/upsert of an existing row, fresh after actual removal and
rejoin even when root/user/lifetime principal are unchanged. Existing principal-kind
guard is retained/extended: root rows require a level, non-root rows require NULL;
root owner is level9. This is enforced by joining the scoped department under
its lock; no cross-table CHECK or caller-supplied root flag.

Add a nullable `root_user_id` FK witness on department_members, derived/validated
by that same trigger from principal.user_id only for a person on the root. It is
NULL for non-root memberships and root agent memberships. A unique
(company_id, root_user_id) proves one root membership per user in that tenant;
a users FK protects the account relationship. Callers cannot forge the witness.
It exists to preserve declarative composite membership FKs, not to create a
second membership source. Root membership remains the sole authority.

Replace principals_company_user_fk with a DEFERRABLE INITIALLY DEFERRED NO ACTION
FK from (company_id,user_id) to department_members(company_id,root_user_id). The
existing opposite member->principal composite FK remains. This intentional cycle
permits principal then membership bootstrap in one transaction, while refusing a
committed live person without membership. NO ACTION prevents deleting historical
principals by removing a membership; lifecycle demotes the person before deleting
its root row. Replace notification recipient member FK with the same root witness
target, ON DELETE CASCADE; retain all tenant/recipient/generated-column checks.
Replace the companies owner-member FK with the same deferred root witness target.
Define these late in init schema after department tables exist.

### One derived read

Define canonical SQL `membership_of(user, company)` returning only live root person
membership facts: membership incarnation id, tenant/user/principal/root ids,
role, access level and created_at. All direct receipt producers, immutable event
source guards and current visibility checks pin this exact membership id. Never
substitute the lifetime principal id or root id. Old events retain their old pin
and cannot revive on rejoin; configured grants and lifetime participant history
remain distinct from current membership authority.
Role = owner iff companies.user_id matches; otherwise admin iff root admin role
exists; otherwise member. No historical-person authority, cache, view of the old
table, dual read or fallback. It is introduced before earlier SQL policy functions
as STABLE PL/pgSQL (late table binding), keeping one derivation body. Company
persistence's existing company_access and list paths consume it; no extra trait
is needed when their CompanyAccess already carries membership plus level. All
notification/workflow/vault/memory/local-agent reads use the canonical derivation
or root-membership existence when no role/level is required. List paths start
from scoped person principals, not an all-users Cartesian scan.

A function result is not a lockable table. Every current FOR SHARE/UPDATE OF
member path is explicitly traced and rewritten to lock the underlying root
department_members row (and principal/account/company in the established order),
then re-read canonical facts under those locks. Do not replace fences with a
nonlocking function call. Preserve error propagation and all scalar membership
readers, notification RLS/policies/triggers, MCP live predicates and level floors.

### Root roles and ownership

Extend DepartmentRole with Owner/Admin, owner single-holder, admin multi-holder.
A partial unique owner index plus guard_root_roles only admits these on root,
for live person members; owner must match companies.user_id and level9. Existing
Lead/Reviewer rules are unchanged. Deferred owner agreement is checked on company,
root member and role changes, proving exactly one owner mirror for each company.
An UPDATE of companies.user_id requires the new owner's root membership; the
company trigger raises its level to9 and replaces the owner role in the same
transaction. The previous owner retains ordinary membership/admin only if they
already hold admin. No new ownership-transfer UI/API is added.

Ordinary department role surfaces cannot transfer ownership or clear owner;
the canonical company owner fact is the only transfer source. Root Admin assignment, clearing/demotion and person removal preserve the
existing owner-only team policy across every equivalent application/HTTP surface.
Generic department owner-or-admin management does not authorize these operations.
The actual actor travels through an owner-scoped command and is revalidated under
the company transaction lock; this is not just a menu restriction or actorless
persistence delegation. Existing non-root Lead/Reviewer management is unchanged.
Root person removal reuses the established company removal transaction after the
same owner/self-removal checks, preserving cancellation, memory retirement, vault
revocation and workflow refresh. Owner mirror assignment/clearing remains refused
through generic roles; owner removal is refused. Root Admin changes refresh the
same dependent workflow authorization and notification state as team promotion. Root add of a stranger requires the
invite/join path, not a bare membership insert with an external principal.

### Lifecycle, locks and dependent state

Keep user/account -> company (sorted for multi-company deletion) -> principal
(sorted for multiple targets) -> underlying membership/role -> desk/notification
locks consistent with current producers and removers. Acquire target account
lock during invite acceptance before company/membership writes so competing
account deletion either wins and join refuses, or waits and then removes it.
Do not introduce a new global mutex or increase existing timeouts.

Accept-invite reuses lifetime principal/address resolution, inserts root membership
with the invited level only on initial join, and updates admin role according to
the existing reacceptance behavior without overwriting an existing member level
or owner fact. Promotion changes the root admin role. Level changes update the
root row, preserve owner9, and retain dependent refresh/revocation.

Member removal locks and captures the actual root membership incarnation and
notice recipient before destructive changes, then revokes delivery/memory/vault
dependents while required facts remain available. Demote the person to external
while their held roles still exist. The established principal-boundary trigger
first authenticates and snapshots every held role into immutable retirement
history, then removes all department memberships/roles, addresses and derived
grants. Preserve historical person identity, configured grants and thread
participation. Refresh workflow consumers before the transaction commits. No
predeletion of roles, fabricated retirement records, or audit-source bypass.
Account deletion deletes owned companies with their trees before retiring any
owner principal; for other companies it uses the same root removal semantics.
Company and account-owned company teardown both retain current external-resource
retirement followed by DELETE companies in the same transaction. An AFTER DELETE
STATEMENT trigger with REFERENCING OLD TABLE AS removed_companies then deletes
channels for those tenants, departments next, and principals last. Company rows
are already absent, and all ordinary row-level tenant cascades (including retained
workflow history) have completed before the statement trigger runs. It does not
rely on ordering among row triggers, predelete history while the company survives,
or scan unrelated tenants. Change only the three direct company FKs on channels,
departments and principals from CASCADE to NO ACTION DEFERRABLE INITIALLY DEFERRED,
so these rows are owned by the ordered statement cleanup. Also defer the existing
principals_company_agent_fk NO ACTION check until commit, since ordinary tenant
cascade removes agents before ordered principal cleanup. All other FK guards and
parent-absence immutability exceptions remain. New root/member FK deferrals are
as specified above. The trigger runs on every company deletion, so direct SQL
and account deletion cannot bypass cleanup. Fresh transaction-scoped PostgreSQL18
probe D1-r2-teardown-probe.sql/txt proves cascade-history-before-statement-cleanup,
channel->three-level-tree->principal order, all rows gone and SET CONSTRAINTS ALL
IMMEDIATE succeeds, plus surviving-company history deletion still refuses. This
minimal probe validates the mechanism, not the real schema acceptance criterion.
Actual implementation must prove the same with retained workflow refusal/native
history, restricted desks, roles and knowledge documents for both company and
account-owned teardown; negative surviving-company history/root controls remain.

### Typed role consumers (explicit dispositions)

Owner/Admin are genuine DepartmentRole variants. Generic workflow references and
EscalationPolicy.notify use that same type, so canonical role parsing, reviewer
selector JSON schema, manifest role validation, role bindings/refusal payloads and
escalation storage/selection must accept all four roles consistently. Owner/Admin
resolve only where held on root; owner remains single-holder, admin multi-holder;
nearest-holder, admission availability, human-review eligibility, one/two-hop and
tenant bounds stay unchanged. No new escalation hop or implicit root membership
of subdepartments. Existing Lead/Reviewer cases retain behavior. Root Admin
retirement is recorded through the same authenticated principal-boundary audit;
add those role strings to retirement storage validation without weakening source/
immutability/role-cleared guards. Owner retirement in a surviving company still
cannot commit without a replacement company owner; tenant deletion may discard
all tenant history once the parent is absent.

Each of the ten SQL Lead/Reviewer checks/validators and registry/schemas.rs is
disposed according to its typed consumer, not blindly replaced: held roles,
workflow_run_role_bindings, valid_workflow_role_refusal, escalation policy, role
intent bindings/manifest checks, department_administration_events, escalation
case/hop roles and workflow_escalation_role_selection. A Rust/SQL matrix covers
all four valid values and unknown-role refusals, actual root role binding/reviewer
selection/escalation paths, alongside unchanged Lead/Reviewer tests.

### Failure behavior and phase boundary

Unscoped/cross-tenant ids, a live person without root membership, forged witness/
level, a second/missing root, non-root owner/admin, foreign owner and direct owner
role deletion fail closed at the DB boundary. Concurrent root creation has one
winner via partial uniqueness; invite/deletion uses existing lock fences and
competing-real-transaction tests. No weakening of recipient FK, member-kind,
principal-incarnation, authorization, grant-source or security boundary.

p01 preserves current team/allowlist/public and optional desk department policy,
while all non-root departments sit under root. It does not automatically create
root desks or blanket grant root members on ordinary desks. Existing explicitly
root-owned members-only desks still use current department grant synchronization
on root joins/removals/roles; no affected existing path is left stale. The policy
change to every root desk and one-step visibility belongs to p02; those are p02's separately gated and
measured contract. Existing subdepartment Lead/Reviewer reach remains until p02
resolves its audience implementation against the original requirement.

## Acceptance mapping (planned; not execution evidence)

| Criterion | Production path | Decisive assertion / focused checks | Status |
| --- | --- | --- | --- |
| R1 exactly one root | company create/onboarding, shape triggers | root_department_: current slug, required root, second root refused; delete/reparent rejected; cross-tenant parent refused | Planned |
| R2 membership derivation | membership_of -> company_access/participant/UI | root_membership_: owner9/admin/member/stranger, actual team authorization, invite/rejoin and level round-trips | Planned |
| R3 owner mirror and equivalent authorization | companies update/root role guards/application/HTTP | owner follows company.user_id atomically; root Admin assign/clear and person removal tested as owner/admin/member/cross-tenant; generic route cannot bypass existing team owner-only policy; old owner no longer owner | Planned |
| R4 principal/recipient tenancy and incarnation | deferred composite FKs, witness guard, direct receipt event pins | direct invalid person/root witness/foreign notification insertion refused; valid rejoin keeps lifetime principal but changes membership id; unprocessed and projected old direct events never revive, wrong-pin/retarget guards refuse | Planned |
| R5 retirement and teardown | remove_member/root removal/account/company deletion | surviving-company removal retains immutable held Lead/Reviewer/Admin retirement facts before roles disappear, regrant cannot rewrite; three-level company/account deletion with actual immutable refusal/native history and documents leaves no scoped row; standalone history/root removal remains refused | Planned |
| R6 competing claimants | actual accept invite vs deletion and root insert | root_competing_: competing transactions, no leaked membership/person/role/address; bounded workers joined | Planned |
| R7 inherited authorization | HTTP, workflow, MCP, memory/vault/notifications | all *_authorization_tests plus focused membership/level/native-MCP/direct/retirement suites after B1 | Planned |
| R8 no old read/write path | complete literal inventory and macro regeneration | company_members absent from active code/schema, intended historical docs/evidence retained; focused all-target compile/source reachability/.sqlx | Planned |

## Contract impact

| Changed contract | Producers and consumers | Disposition / focused proof |
| --- | --- | --- |
| Membership table -> root rows | company/invite/levels/principal/access context; all 150 inventoried Rust files + SQL policies/triggers | migrate production queries/fixtures; SQL function is canonical derivation; exercise actual readers not only mocks |
| Root roles and owner mirror | company update, department writes, workflow role bindings/escalation, team UI | forbid owner role transfer through generic route; validate admin refresh; role string/exhaustive parser fixtures |
| Member FK locks | delivery projection, vault consent, memory level gates, native MCP, dispatch/retirement | explicit underlying-row locks; competing tests and tenant adversarial tests |
| One tree and depth | department creation/edit/delete, onboarding, UI trees, tests | default None parent to root; update depth fixtures with same bound, no new alias/legacy shape |
| Retired principals | account/member lifecycle, addresses, historical participants | reuse incarnation and cancellation paths; principal/member/role/address lifecycle assertions |

Before changed test names are deleted/moved, retain pre/post runner inventories
and map replacements. Existing raw SQL test fixtures become shared root bootstrap/
join helpers; no production compatibility writes. Regenerate .sqlx against fresh
pa04_p01 disposable schema, then focused compile and checks; full gate after p03.

## R1 findings addressed in decision r2 (awaiting B recheck)

- F1: distinct root membership id and stable-upsert/fresh-rejoin semantics; preserve
  actual unprocessed/projected direct-notification and forged-pin assertions.
- F2: owner-scoped root Admin assignment/clear and person removal with transaction
  revalidation; no actorless delegation or UI-only restriction.
- F3: snapshot roles at established principal demotion boundary before deleting
  them; explicit root-role consumer and audit-schema dispositions.
- F4: company row disappearance/ordinary tenant cascades first, ordered AFTER DELETE
  STATEMENT cleanup with transition table; four named FK deferrals; real-schema
  history-rich company/account teardown and negative controls still required.

Additional focused cases: actual root role bindings/admission/reviewer/escalation
selection with all four typed roles; old Lead/Reviewer expectations retained.
`logs/D1-r2-teardown-probe.txt` is a successful temporary-table mechanism probe,
not a claim that production teardown is implemented or verified.

### D1/r2 accepted; B1 implementation begins

Root recomputed matching source snapshot 4d0aeba871fb23224b32d167e1a2461e7128084a12f548bb3d631627537b3110
and decision SHA256 50a70e404c18b6bdf7af95740bd1bfb8e2c19f94e106b94d2e5d079039c011f2.
Independent 04RU-P01-D1-R2/N1 explicitly PASS for D1/r2, resolving F1–F4 at
design level. Production proofs remain planned, not passed. Coordination rev1
unchanged and rechecked. Source freeze released for one cohesive B1 implementation.

### B1 implementation checkpoint (not accepted)

Plan03/fix-doop publication remains main8fdb892b. No Plan04 commit or push yet.
D1/r2 is the governing accepted architecture. Schema now removes company_members,
adds root shape and deferred owner integrity, root levels/user FK witness, distinct
membership incarnation id, canonical membership_of, root Owner/Admin roles and
ordered AFTER DELETE STATEMENT tenant cleanup. First fresh schema migration on
owned pa04_p01_schema PASS (B1-schema-first-migration.txt,1.42s). A later port of
the former user/company lookup index to root_user_id is not yet migrated there;
use another fresh pa04_p01 schema for the final metadata/check gates, not Plan03 DBs.

Production Rust now uses canonical root reads (existence/lock-only readers use
underlying department_members.root_user_id); no active persistence table reference
remains outside fixtures/comments. Company create uses company/root.rs bootstrap;
invites join with stable-incarnation upsert and tenant lock; role/removal commands
now carry actor for under-lock owner revalidation. Root generic Admin assign/clear
requires owner, Owner role edits refused. Root person removal and account/member
retirement share company/member_retirement.rs, snapshotting held roles before
demotion-trigger cleanup. Root removal preselects account locks but rechecks target
under company/principal locks, refusing a changed target. Nonroot department default
parent is root and root deletion/reparent/independent slug changes are refused.

All stored DepartmentRole consumers were explicitly expanded per D1/r2; actual
SQL policy functions and receipt guards use new membership id. New production
root helpers are wired; oversized company tests extracted to company_tests.rs
with names retained. Added explicit is_root:false to 12 existing pure literals.
Shared test_support now bootstraps the real root, seeds root members, and transfers
ownership via company trigger; many old fixture queries and actorless test calls
STILL NEED migration. Do not claim old 4705 tests pass against these changes.

Initial offline lib check failed6 errors (retirement Transaction argument and5
knowledge authority::start calls); corrected. B1-lib-check2.txt PASS2m28s for
then-current production library. Subsequent index/list query bounds were refined
without rerunning lib yet. Current all-target diagnostic check is running in
exec session49244, log B1-all-targets-first.txt; expected to identify remaining
fixture/type/API changes. This is NOT behavioral or final verification. No new
root/membership/concurrency/authorization tests have run. New root lifecycle and
actual history-rich teardown proofs remain required.

Useful retained inventories: pre-B1 4705 names,21 actual HTTP authorization cases,
all old membership literals, underlying member lock sites, DepartmentRole uses,
Department literal locations and B1-member-command-call-sites.txt (95 occurrences).
A later trace confirms workflow_target_departments only derives principal context
for AGENTS, so automatic root person membership/Owner/Admin do not introduce
implicit person-context ambiguity; no context-selection change was needed.

Next: migrate remaining fixture writes/reads and actor-bearing calls, preserving
meaningful owner-refusal and incarnation/history cases; finish new focused root/
membership/competing/equivalent HTTP authorization tests; regenerate .sqlx on fresh
schema; run focused B1 checks and authorization suites. Then freeze whole B1 and
request retained independent B batch review. Full Plan04 gate only after p02/p03.

### B1 fixture migration checkpoint

Initial all-target diagnostic completed FAIL with62 E0061 actor-command call sites.
All have now been migrated to explicit actors (including captured owner IDs in
competing tasks); wrapper fixtures use the actual containing owner fixture.
These changes are not yet type/behavior verified. Bare owner removals now must be
asserted as refusals; check existing company-invite/delivery tests for former noop
expectations before focused execution.

Positive membership seeds moved to shared insert_root_member/transaction RootJoin
(35 operations32files initially); existing label-only department fixtures reuse
root-owned person identity. Address collision/rejoin tests retain the actual
create_person_principal_on operation, adding its required root membership AFTER
resolution in the same transaction via a local fixture helper; no pre-created
address masks the operation. The competing rejoin still runs both real resolvers
under their original lock prefixes and preserves lifetime/obsolete-identity checks.

All active Rust/SQL word-literal company_members references are now gone. Level
fixtures update root rows, role fixtures mutate Admin grants, raw membership-loss
fixtures demote the live principal (retaining retirement/history semantics), and
lock observers match their actual new producer query. Owner transfer fixtures
change companies.user_id and let its trigger move the mirror. Root-row identity
readers now use canonical membership_of, retaining direct-event pin assertions.
Negative owner tests target root role/membership and accept the actual relevant
root constraints; former company_owner_column_cannot_change_alone is replaced by
root_department_owner_role_follows_company_owner, testing automatic valid transfer,
level9, stranger refusal and complete transfer back. Record this name disposition
against pre/post inventories. The no-leak table check includes all root tables.

Recheck all-target session/log is started separately (B1-all-targets-second.txt).
Still no behavioral test execution or batch acceptance. Remaining: compile type
corrections, raw duplicate-person fixture adaptation, root/route/concurrency tests,
SQLx metadata/fresh schema, focused authorization and inherited suites, self-audit
(including list query bounds/root UI role controls), then one cohesive B review.

### Resume: compile pass and first root behavior run

Remote main reverified by ls-remote at 8fdb892b; plan/fix-doop.md is tracked there,
introduced by a3194fe4. Coordination rev1 unchanged. No Plan04 commit/push yet.
Third all-target diagnostic failed on a stray is_root field in DepartmentSelection;
that field had been removed before this resume. Fourth offline all-target check
PASS (B1-all-targets-fourth.txt,4m49s). This covers the original five root tests'
types; the newly added competing tests are being compiled by the next run.

Fresh owned schema pa04_p01_schema2 migrated PASS1.12s; migration identity is
current at this checkpoint. Owned shared behavior DB pa04_p01_core_test created.
First network-isolated test command is building in exec session4601, retained
B1-root-first-tests.txt. Filter company_invite::owner_tests::root_unit covers five
root tests plus two root_competing_ tests. These execute real invite acceptance
and account deletion with two blocked backend transactions in each lock order.
No behavioral PASS is claimed yet. Existing B1 architecture gate is unchanged.

Remaining B1 proof: focused test failures/corrections, real-schema history-rich
three-level company/account teardown, root-route owner/admin/member/foreign
authorization, typed-role workflow consumers, SQLx metadata, inherited affected
suites and all HTTP authorization cases, test-inventory dispositions, evidence
manifest, frozen batch review and acceptance. p02/p03 have not started.

### First behavioral diagnostics and fixture corrections

B1-root-first-tests.txt: 5 PASS,2 FAIL (6.93s after8m19s compilation). The five
core root/role/rejoin/bounds tests passed. Both real invite/deletion race paths
completed; their final cleanup assertion used nonexistent principal_addresses.
Corrected to actual addresses; this is not yet a passing race result.

B1-initial-http-authorization-tests.txt:19 PASS,2 FAIL,6.22s on initial B1 binary.
One duplicate-person fixture in workflows/tests.rs is now ported to use the root
join's existing principal. Attention's raw positive department fixture now attaches
to the actual company root. These corrections still require execution.

B1-initial-department-tests.txt:118 PASS,25 FAIL,51.02s, initial B1 binary. Failures
include the same duplicate-person/raw-parent fixtures and inherited assertions
that assume no root, eight additional child levels, or no parent in directory
context. Ported15 positive person fixtures,6 raw child department writes; kept the
8-level bound by counting the company root as1; expected visible sets now include
the real root, without granting non-root department membership. Owner no-op tests
now assert the designed BadRequest refusal. Retirement regrant and native owner
incarnation fixtures restore root membership/owner mirror in the same transaction.
Onboarding's agent placement reads exclude automatic person root members.

SQLx prepare PASS3m06s (B1-sqlx-prepare-first.txt) against fresh schema2, all targets;
removed5 obsolete compile-time metadata entries. Its unused helper import warning
is corrected. It covers code as loaded during that check, before subsequent test-
fixture edits. Formatting and patch whitespace checks currently pass.

Added real-schema tree+documents teardown payload with immutable native/refusal
history negative controls, both company and account deletion; real HTTP root
owner/admin/member/foreign mutations; frozen/followed human reviewer Owner/Admin
selection through parent root and live admin role removal. None is yet claimed
passing. Second root test build running session48159/log B1-root-second-tests.txt,
network isolated on pa04_p01_core_test. Preserve source until its results are
collected. No B1 implementation review, acceptance, or Plan04 publication yet.


## 04RU-P01-D1/r3 — native teardown constraint completion

This revision retains all D1/r2 requirements and changes only the listed native
peer FK timing. Actual B1-root-second-tests.txt passed11/13: real refusal-history
company/account teardown passed, but real native-history company teardown failed
on local_agent_sessions_company_id_workflow_run_id_fkey. The new cross-cascade
proof contradicts D1/r2's assumption that four FK deferrals suffice. No dependent
schema edits have been made for this revision.

On the real schema, local_machines belongs to companies by CASCADE; profiles,
sessions and operations belong to the machine by CASCADE. workflow_runs has its
separate company cascade. Keep those owners and every column, tenant composite
key, required relationship, history guard and non-cascade deletion policy. Make
only these three existing NO ACTION FKs DEFERRABLE INITIALLY DEFERRED:

- local_agent_sessions(company_id,workflow_run_id) -> workflow_runs(company_id,id).
- local_agent_sessions(company_id,profile_id) -> local_agent_profiles(company_id,id).
- local_agent_operations(company_id,machine_id,installation_id,session_id) ->
  local_agent_sessions(company_id,machine_id,installation_id,id).

The first is exercised by the failure. The other two are peer dependencies across
the machine's independent profile/session/operation cascade branches, identified
by the real catalog in B1-native-fk-diagnostic.txt. This preserves exactly the same
valid committed states, refusing an orphan at commit/SET CONSTRAINTS ALL IMMEDIATE,
and lets ordinary cascades finish after company disappearance before the ordered
channel/tree/principal statement cleanup. No new CASCADE, bypass or broad ALTER of
unrelated native/workflow constraints. Conversation-session references need no
deferral: machines/sessions disappear before ordered channel/thread cleanup.

Add enduring negative controls to the real native fixture: each deferred scoped
relationship must still reject a nonexistent/foreign referenced id when forced
immediate or at commit, and leave the original row intact after rollback. The
existing immutable native-call DELETE refusal while company survives remains.
Then the actual three-level tree, root/child roles, restricted desk and knowledge
document/chunk company and owner-account teardown must both pass on a newly
migrated database and leave no live company-owned row. Run native/lifecycle and
existing affected authorization checks.

The global local_installations reservation is deliberately a tombstone, documented
at init schema's table definition: company_id is historical, has no companies FK,
and prevents reuse of an installation across tenants after tenant deletion. Keep
it and assert its count/identity survives teardown; exclude this specific global
reservation from the fixture's all-live-tenant-row assertion. This is an inherited
security requirement, not a new retention/backward-compatibility mechanism.

Independent non-gated correction: ReviewerAssignment.canonical_content currently
sorts new roles by enum declaration order (Owner,Admin,Lead,Reviewer), while SQL's
existing canonical assignment guard requires role-string order. Preserve that
wire contract by sorting serialized role references by role.as_str(), with an
all-four-role canonical order/permutation regression and real followed Owner/Admin
reviewer coverage. Do not weaken SQL's ordered-array or exact-fact guards.

D1/r3 status: proposed; await independent retained B decision PASS before native
schema changes. Source frozen for this gate; tests may continue against the current
binary and evidence records may be written under the existing exclusions.


## 04RU-P01-D1/r4 — both native origins and implicit placement

D1/r4 retains D1/r2 and supersedes r3's native timing addendum. It addresses
04RU-P01-D1-R3/F1 using the actual direct threads_company_fk company cascade.
Source remains unchanged at686872f7713869716cf5e387417dd23d7ad643011dd869689ee3583860becf24.

### Native peer relationships

Keep the existing companies -> workflow_runs, threads and local_machines ordinary
cascades; local_machines owns profiles/sessions/operations by CASCADE. After all
ordinary tenant cascades, the existing ordered statement trigger owns channel,
department-tree and principal cleanup. Direct threads are an independent company
child, not delayed solely by channel cleanup.

Make only these four existing NO ACTION peer FKs DEFERRABLE INITIALLY DEFERRED:

1. local_agent_sessions(company_id,workflow_run_id) -> workflow_runs(company_id,id).
2. local_agent_sessions(company_id,conversation_id) -> threads(company_id,id).
3. local_agent_sessions(company_id,profile_id) -> local_agent_profiles(company_id,id).
4. local_agent_operations(company_id,machine_id,installation_id,session_id) ->
   local_agent_sessions(company_id,machine_id,installation_id,id).

The first is the actual observed native-history teardown failure in
B1-root-second-tests.txt. The second is the corresponding conversation-origin
branch identified independently by B. The latter two connect the machine's
independent profile/session/operation cascade branches. Preserve every column,
composite tenant key, required relationship, non-cascade deletion policy, history
guard and uniqueness check. Only timing changes; exactly the same committed states
remain valid, and orphan/foreign references still fail at commit/forced immediate.
No bypass, new CASCADE, or broad native/workflow deferral sweep.

workflow_local_agent_calls' profile/operation/session references stay immediate:
the call's owner is workflow_action_intents by CASCADE, itself removed in the
ordinary workflow-run cascade, so calls disappear before the machine cascade
removes profiles/operations/sessions. Explicitly verify that disposition on both
real native-origin fixtures, rather than treating a catalog diagram as execution.
Its dispatch link stays immediate; its existing wait FK remains deferred. If
actual both-origin proof contradicts this disposition, reopen the decision.

local_installations stays a global immutable reservation: historical company_id
has no companies FK, and reserving installation identity after tenant deletion
prevents cross-tenant reuse. Preserve its exact row identities/content; the
all-live-company-row assertion excludes this single global tombstone table and
separately verifies it did not change. This retains the existing security contract
at the migration table declaration, not new data compatibility or retention.

Proof required before B1 acceptance: actual conversation-backed/direct native
and workflow-backed native history, each through company and owning-account
deletion, with the three-level root tree, roles, restricted desks, actual knowledge
documents/chunks and no remaining live scoped rows. Retain the surviving-company
immutable native-call deletion refusal. Each of the four changed relationships
gets actual-schema forced-immediate/commit orphan and cross-tenant negative
controls with rollback and named-FK attribution. Preserve/exercise native machine,
session, action and tenant isolation readers. No temporary-table substitute.

### Inferred Builder placement (new affected-consumer disposition)

B1-department-http-second-tests.txt exposed that Builder's existing single-person-
department inference now sees automatic root membership as a second candidate,
losing the previously unambiguous Support placement. The responsible query is
placement_on in workflow/provisioning_builder.rs:131–196, through builder_on ->
create_agent_on -> ProvisioningEffects.apply. It unions person membership/roles;
this is distinct from agent workflow target context already traced in r2.

For implicit Builder placement only, exclude is_root departments from that
existing bounded candidate query. One non-root membership/role still selects that
department; zero or multiple non-root candidates still select no implicit agent
placement. Explicit department slugs, including the company root, are unchanged.
Root company membership/Owner/Admin facts do not constitute an inferred working
subdepartment. Do not alter general membership reads, authorization, agent context,
role inheritance, or p02's separate rule that every desk defaults to the root.

Proof: keep the real existing Builder sequence's Support placement, explicit Sales
Lead placement and ambiguous-two-subdepartment no-placement assertions. Add an
explicit root-only/no implicit non-root placement check and explicit root placement
with an agent member, preserving named-role restrictions. Include actual Builder
requester-lock and provisioning call/interface tests in the focused scope.

Independent routine corrections under unchanged contracts: serialize reviewer
assignment role references in role-string order, preserving SQL canonical-array
checks; normalize an owner's proposed root join level to9 before the before-insert
guard/ON CONFLICT so reaccepting an owner invite cannot fail or alter membership.
Preserve existing nonowner membership level/id on reacceptance and both competing
acceptance/level-writer test lock paths. These corrections receive full B1 code
review and focused regressions, not acceptance by this design-only gate.

D1/r4 proposed; retained B recheck required before dependent source changes.

### D1/r4 recheck and r5 proposal

B 04RU-P01-D1-R4 resolved R3/F1 and supported bounded non-root Builder inference,
but recorded D1-R4/F1 for the actual inherited message_participants_identity_fk
teardown failure. No dependent source edit occurred. Proposed immutable D1/r5
retains r4 and explicitly defers that single composite NO ACTION edge, preserves
standalone history refusal, requires populated company/account counterparts and
exact FK negative proofs. Native four-edge and workflow-call dispositions remain.
Source still frozen at686872f7…cf24; B recheck required.

Inherited diagnostic B1-inherited-first-tests.txt:444PASS/13FAIL/1IGNORED,204.12s.
The ignore is the existing real native CLI/plugin fixture prerequisite; it is not
executed or passed. Remaining exercised failures include the new root owner-level
before-insert/upsert bug, two race triggers still using NEW.user_id, owner no-op/
raw-deletion fixtures that now need explicit owner protection/transfer, root Admin
retirement audit expectations, and the newly valid Owner role in a negative role
fixture. All are affected and retained; no unrelated-failure disposition is made.

### D1/r5 accepted for correction implementation

Independent04RU-P01-D1-R5/N1 explicitly PASS for decision r5, resolving R4/F1 at
design level. Root recomputed matching source686872f7…cf24 and decision SHA
d8be7fd9…bc93d; coordination rev1 unchanged byte-for-byte. The source freeze is
released for the complete correction batch under r5. This is not B1 acceptance;
fresh-schema behavior, negative controls, code/proof review remain required.

### D1/r5 correction batch applied; fresh schema3 checks started

Applied all five accepted peer FK deferrals, role-string canonical assignment
sorting, owner proposed level9 before root upsert trigger, and non-root-only
implicit Builder placement. New regression retains explicit root placement and
root-only no implicit placement. New all-four-role canonical ordering assertion.
Native real fixture controls now cover each deferred edge with valid, missing and
actual foreign-tenant references, forced named FK checks and rollback; helper uses
real schema and live native sessions plus a second tenant/machine/profile/thread.
Actual direct-origin native company/account tree/document/history regression added
beside existing workflow-origin one; installation reservation rows compared exactly
across teardown and excluded only from live-row scan. Populated protected-action
company/account counterpart includes identity FK controls and standalone referenced
identity deletion refusal. These are written, not yet runtime passed.

Corrected race trigger root_user_id, owner-refusal notice fixtures, explicit owner
transfers before obsolete-owner principal deletion, Admin+Reviewer audit assertions,
unknown role negative fixture and remaining root list/count expectations. Competing
invite/deletion cleanup now checks every lifetime person incarnation's role/address
via historical_person_user_id and confirms old principal is retained external.

Fresh owned pa04_p01_schema3 and pa04_p01_core3_test allocated; first current-schema
migration + second SQLx all-target prepare command running exec68839, logs
B1-schema-third-migration.txt and B1-sqlx-prepare-second.txt. Source held stable for
compilation diagnostic; no new behavioral result/acceptance. Next collect compile
errors, then root/dept/auth/affected checks on fresh core3_test and retain inventory.


## 04RU-P01-D1/r6 — one observed local-call dispatch peer

Retain D1/r5 except the explicit dispatch-link disposition below. Its fresh schema3
root run (B1-root-third-tests.txt) passed15/17, including direct-native company/
account teardown, all four native peer valid/orphan/actual-foreign controls and
installation preservation, message identity controls/protected-action company/
account teardown, and refusal history. The real workflow-native counterpart failed
on workflow_local_agent_calls_company_id_run_id_execution_id_fkey2 referencing
workflow_action_dispatches. This disproves r4/r5's immediate dispatch disposition:
local calls are owned through action intents, while dispatches have another run
cascade; ownership does not guarantee every nested dependent RI event is complete
at the sibling marker's immediate check.

Make only that existing seven-column composite NO ACTION FK DEFERRABLE INITIALLY
DEFERRED: (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,
effect_kind) -> workflow_action_dispatches(company_id,run_id,execution_id,
invocation_id,argument_digest,id,effect_kind). Keep the action-intent owner CASCADE
and all four local-call profile/operation/session peer FKs unchanged; their
conditional disposition still requires the full real both-origin lifecycle proof,
and a new contradiction must reopen the decision. Do not broadly defer the whole
table or remove frozen call identity/settlement/wait/dispatch guards. Valid committed
states, tenant/run/execution/invocation/digest/effect pins remain exactly unchanged.

Proof: re-run actual workflow-native and direct-native histories × company/account
teardown with original tree/docs and five earlier peer FK controls. For the new
dispatch link, use the actual populated local-call row in a rollback-only transaction:
retain its entire frozen scope/source data, delete and reinsert that same call row
with a valid original marker (positive), a nonexistent marker (named deferred FK),
and a different existing marker from the same fixture's broker run (wrong scoped
marker, named deferred FK). Replacing the row for a probe must not disable triggers
or mutate any durable application record; rollback restores exact original row.
Retain the original-call UPDATE immutability and surviving-parent history deletion
refusals. A foreign-company call mutation is also exercised and must fail its
existing tenant/action owner guard; record that it does not isolate the deferred
peer check. The FK's tenant column and its actual definition are independently
checked; earlier per-native-edge foreign-ID controls plus actual cross-tenant
application/HTTP suites remain required. Do not call the wrong-run marker a
foreign-tenant marker or claim a peer-specific foreign insert if an owner guard
preempts it. If the real row cannot be replaced without a stronger guard refusing,
retain that named guard proof explicitly and re-evaluate the planned specific probe.

The other root-third failure is an assertion error in the new followed role test:
assignment_refusal_on retains the existing fail-closed contract when ANY selected
role is unavailable. Clearing Admin yields RoleUnavailable with no eligible
reviewers, even while Owner stays held. The assertion is corrected to no reviewers/
no submission/refusal plus canonical Owner membership retained, matching the
existing production rule and older missing-role regression. No production role
availability behavior change is proposed.

D1/r6 proposed; source frozen for retained B recheck.

### D1/r6 accepted for dispatch-peer correction

Independent04RU-P01-D1-R6/N1 PASS is design-only. Root matching source4ac1d9d0…
3cec4e/decision49bbf416…6cde9b/coordination1 checks PASS. Source freeze released;
apply only the accepted dispatch-peer deferral and its real-row rollback proofs,
retaining other four local-call peers unchanged. Root-third:15PASS/2FAIL; all new
root cases except followed-role expectation and workflow-native cascade passed.
The followed assertion was corrected before this gate under unchanged production
role availability (missing Admin -> RoleUnavailable/no submit; Owner retained).

### R6 correction implementation and fixture lease diagnosis

Applied only accepted local-call dispatch FK deferral; other four local-call peer
FKs unchanged. Added actual frozen-row rollback valid/missing/wrong-scope marker
probes, exact original-row restoration, foreign-source guard attribution and
standalone immutability/deletion guards. Native historical ownership transfer now
uses member, preserving admin's level-classification exercise. Followed test asserts
actual persisted RoleUnavailable plus no submission and retained Owner membership.

Fresh schema4 PASS0.965s; SQLx all-target prepare PASS1m13s. Broad654 diagnostic
647PASS/5FAIL/2IGNORED,584.56s; isolated three-case retries: profile PASS3/3, directory
filters FAIL3/3, directory full FAIL1/3, retained B1-third-isolated-retries.txt.
Directory3s lease starts before heavy fixture setup. Shared claimed now freezes
bundle/model before claim; two directory cases claim only after agent/desk setup.
Production limits unchanged. Correction runtime build36668/B1-root-fourth-tests.txt
is in progress on fresh core4_test; no fresh behavioral pass or B1 acceptance yet.

### Fourth focused result and D1/r7 reopening

`B1-root-fourth-tests.txt`: 42 PASS / 1 FAIL, 35.53s after 3m40s build.
All root/competing, followed/frozen reviewer, directory/Builder/provisioning and
historical native revocation corrections pass. The actual workflow-native
company/account teardown still fails, now on
`workflow_local_agent_calls_company_id_profile_id_fkey`. The four unchanged
local-call native peers in r6 therefore do not have a sound immediate-timing
disposition. No phase acceptance or Plan04 push. Remote fetched on resume:
origin/main03f0bda2 contains Plan03 and plan/fix-doop.md; integration is pending.

Proposed replacement disposition is immutable `logs/D1-r7.md`. It treats exactly
the four local-call native NO ACTION peers as one bounded cross-owner group;
their owners and committed identities do not change. Source stays frozen for B's
decision review. Remaining broad dept/auth/inherited checks, whole-B1 review,
p02/p03 and final full gate remain outstanding.

### D1/r7 accepted and correction applied

Independent04RU-P01-D1-R7/N1 PASS is architecture only. Root matched source
d31771d6…f0b947, decision8ec33f0a…bb1a9 and coordination1 before releasing freeze.
Applied exactly the four accepted local-call native peer deferrals, preserving
owning cascades, tenant/origin columns and immutable guards. Real call probes now
include each named FK's missing and actual foreign native profile/session/machine
candidate, unchanged all-constraints-positive control, and exact source restoration.
Foreign operation-id isolation remains explicitly unclaimed; inherited operation
origin mutation now forces its exact deferred FK immediate and checks its name.
Shared native foreign fixture company is passed explicitly to the call probes.

Core4 department+authorization diagnostic169PASS/1FAIL in50.61s, only same native
workflow teardown. All21required HTTP authorization cases PASS, independently
compared with baseline names in B1-third-http-authorization-selection-result.json.
Fresh schema5/core5_test allocated for corrected migration/runtime proof; all prior
schemas/evidence preserved. Final fresh behavioral result and B1 code review pending.

### B1 whole-batch review preparation and execution mapping

Fresh schema5 root/provisioning/history/origin focused run44PASS/0FAIL in46.74s.
Both real native origins × company/account teardown and all named-FK rollback
controls pass. Compilation2m41s (retained log is authoritative); new source
reachability8PASS in15.00s using unchanged retained checker. SQLx all-target prepare
PASS1m04s, fresh migration0.693s. Format, whitespace and Trail check PASS (no conflicts).
Current discovered libtest4721 vs baseline4705: one owner-column rejection test
removed because owner transfer is now a required atomic mirror; replacement
root_department_owner_role_follows_company_owner is discoverable and runtime PASS.
All16net additional tests are retained in B1-final-test-inventory-delta.txt.
Exact-word exhaustive src/migrations/.sqlx old-table inventory is empty; substring
matches are legitimate company_membership field/method names.

| Criterion | Exercised path / decisive assertion | Current evidence |
| --- | --- | --- |
| R1 root shape/bootstrap | root_department_creation_and_company_rename_round_trip; database_guards preserve required/unique/root shape, slug and tenant rules | schema5 focused44 PASS |
| R2 canonical membership/level | root_membership_roles_and_equivalent_department_mutations_keep_owner_authority; reacceptance/rejoin identity and bounded roster cases | schema5 focused44 PASS; inherited654 pending |
| R3 owner mirror | root_department_owner_role_follows_company_owner updates owner+role+level atomically; invalid ownership/role mutations refused | schema5 focused44 PASS |
| R4 role consumers and mutations | frozen/followed human reviewer activation uses real stored role facts; missing Admin refuses submission with Owner retained; root department HTTP mutation denies nonowner | schema5 focused44 PASS |
| R5 deletion and history | direct-native, workflow-native, refusal, populated message-identity/protected-action histories × company/account; real tree/docs; all tenant live rows gone, exact installation tombstones retained | schema5 focused44 PASS incl named-FK controls |
| R6 competing claimants | invite wins vs account deletion wins, actual concurrent transactions; no membership/principal/role/address leak | schema5 focused44 PASS |
| R7 affected authorization | all21HTTP authorization baseline names PASS; corrected broad654 selectors include all170dept/auth names; actual operation-origin FK independently forced immediate | core4 HTTP21 PASS and schema5 origin PASS; broad654 in progress exec21732 |
| R8 complete replacement and reachability | zero exact old-table occurrences, .sqlx regeneration/all-target compile, discovered runner replacement mapping and source reachability | current PASS as logs named above |

Source frozen for whole-B1 review at HEAD8fdb892b, digest
e7f60808ad0ddd95d12c8e85cc5accb306030037e80dfd8b754425b793654247,
same exact exclusions. Initial immutable manifest B1-R1-initial.evidence.json
retains current success/historical failures and the pending broad check. Do not
issue final B1 PASS or accept p01 until the corrected broad result is retained in
a new complete manifest for the same source and B independently reviews it.
New learning documents the demonstrated cross-owner teardown cause; source tests
do not depend on its presence. Source mutation stays frozen during B review.

### B1-R1 corrected broad execution complete

Core5 affected654 selector run:652PASS/0FAIL/2IGNORED,328.47s;
all170department/authorization cases included and all21required HTTP names PASS
verified against baseline. Prior five affected failures no longer reproduce.
External native CLI/plugin and live-memory journeys remain NOT EXERCISED.
Complete immutable `logs/B1-R1.evidence.json` validates5PASS/2historicalFAIL/
1externalNOT EXERCISED for unchanged artifacte7f60808…654247.
Source and coordination remain frozen/unchanged during B1-R1 whole-batch review.
No p01 acceptance until independent complete code/proof PASS.

### Final-gate prerequisite preflight

Created task-owned Colima profile openbot-plan04 (4CPU/8GiB/32GiB sparse disk)
with --activate=false/--ssh-config=false. Docker29.5.2 responds on its explicit
socket; default context untouched and existing as-attribution-integration remains
stopped. External pa04-docker-env.sh selects only this daemon for later final
gates. No project source/fixture mutation or full-suite execution.

### B1-R1/F1 correction in progress

Independent whole-B1 review04RU-P01-B1-R1/N1 reports exactly F1 (medium): stored
Owner/Admin escalation lost by browser render/parser; remaining batch reviewed.
Source freeze released for cohesive correction. Rendering now derives choices
from DepartmentRole::ALL with exhaustive labels; parser uses canonical role parsing
and strictly one/two canonical hop values. No domain/schema/availability change.
New actual binding GET/render→POST/save→read-back tests cover all4roles×2hops,
browser creation, changing unrelated params, malformed-role/hop refusal without
creation/configuration writes. Rust/SQL matrix now supplies all4role valid policies
and retains unknown/invalid bounds. New actual notification worker cases select
root Owner/Admin after genuine refused admission for1/2hop policies and assert
committed recipient facts, projected notice and no fabricated workflow run.

Focused correction checks: locked/offline all-target SQLx prepare (exec94166), current source reachability, root_membership_,
actual authoring bindings tests, escalation_ consumers and all HTTP authorization
cases. Reuse unaffected44/652 migration/lifecycle evidence; no full-suite rerun.
No p01 acceptance/push; later phases remain pending.

### F1 correction focused result and inherited raw-retirement fixture

B1-R2-focused-tests.txt:174PASS/1FAIL/1IGNORED in311.33s after2m37s build.
All3new regression tests PASS: browser all4roles×2hops round trip, invalid-input
no-write, and actual Owner/Admin notification worker cases for both hop bounds.
All21required HTTP authorization cases PASS; widened Rust/SQL escalation matrix
PASS. Ignored manual browser journey is NOT EXERCISED, not a native browser claim.

The sole newly exposed inherited failure is escalation_history_tests.rs's assertion
that raw membership retirement deletes the principal. The B1 fixture had been migrated
from DELETE company_members to principal demotion but retained that obsolete assertion.
Corrected its probe to assert exact external lifetime identity/historical actor plus
zero department memberships/roles and no canonical live access, while retaining
original captured role count, command actor/principal history and revoked read checks.
No production lifecycle change, weakened history guard or architecture revision.
New SQLx prepare exec44412 runs B1-R2b-sqlx-prepare.txt; then rerun only this affected
retirement test and reuse unchanged174passes (do not repeat entire176).

### B1-R2 correction artifact frozen for independent recheck

Raw-retirement exact retry1PASS/0FAIL in2.67s after2m58s build; current all-target
SQLx prepare PASS48.05s. Reuse unchanged174passed correction cases,4decisive F1
new/browser/worker/matrix checks, all21HTTP authorization cases and prior652
affected proofs with explicit source-reuse limits. Three external/manual journeys
remain NOT EXERCISED. No unresolved exercised failure. Current test inventory4724
(3added/0removed since R1), final raw-test binary inventory matches exactly.

Implementation is staged and FROZEN at HEAD8fdb892b, source artifact
2a5714f52927de4b4b7d261ba970d95d2ed526d768f80ced4009c8b2dd7ab793,
same exact exclusions. Stage composition explains additional identity change;
source delta is the8F1/correction files, no schema/production lifecycle changes.
Immutable logs/B1-R2.evidence.json validates8PASS/3historicalFAIL/1NOT EXERCISED.
Ask retained B for ONE F1 correction recheck with affected raw-retirement fixture
proof; acceptance and commits/push still pending.

## p01 acceptance — 04RU-P01-B1-R2

Root ACCEPTS p01/B1 at source2a5714f52927de4b4b7d261ba970d95d2ed526d768f80ced4009c8b2dd7ab793,
HEAD/base8fdb892b, after independent04RU-P01-B1-R2/N1 PASS and F1 resolution.
Root acceptance snapshot, D1/r7 decisionSHA8ec33f0a…bb1a9 and coordination1
SHA2c7fd713…428b match review pins. No unresolved acceptance-blocking source
finding or exercised affected failure. Current manifest retention validates;
root44/broad652 proof reused only for unchanged contracts, current F1/new-role
proofs and raw-retirement retry supply affected correction coverage.

Implementation staged-whitespace check passes; unrestricted diagnostics may include
original tool-emitted whitespace in immutable raw logs, which is retained with its
hashes. No source formatting exception or changed bound. Final full repository
verification is intentionally after p02/p03, not claimed here. External/native/
live-memory/manual journeys remain unexercised and Doop remains deferred perC008.

Local reviewed commit follows, then newer main integration and p02. No Plan04 push
or whole-plan completion yet. Preserve all previous source/schema/log evidence.
