# 06DA-P02: Routes, UI and audit

## Identity and authority

- Workflow: implement-opus-reviewed-2_4. Phase: p02 (Plan06 doc 02, plus the 01A.2
  removal of `CompanyMembership::manages_company_operations` carried from p01).
- Original criteria: ../../../README.md (decisions 1–6, "What does not change") and
  ../../../02-routes-ui-and-audit.md; ../../../01-predicate-and-owners.md 01A.2.
  User decisions 2026-11-18 (COORDINATION r5/r6): manager manual knowledge read+write
  over the subtree; root admins manage member invites only.
- Record root: /Users/mac02/orca/workspaces/openbot/plan06/plan/06-delegated-administration/phases/p02/docs.
- Root/A: Claude Code root, session b36d9dc4-495e-4131-9804-05ebcc381915, Opus 5.5/xhigh
  (user-confirmed). B: Opus 5.5/xhigh general-purpose subagent (user's choice), the
  same reviewer that accepted p01 B4, continued via SendMessage.
- Worktree /Users/mac02/orca/workspaces/openbot/plan06, branch plan06. Phase start and
  review base: 2352dc38 (p01 accepted and committed). Preserved pre-existing changes:
  none besides untracked p01 snapshot tars (ignored).
- Coordination authority: ../../../COORDINATION.md r6, no notices.
- Snapshot exclusions: plan/06-delegated-administration/phases/p02/docs and
  plan/06-delegated-administration/COORDINATION.md.
- Context: default auto-compaction (user skipped 75%).
- Resources: owned PG18 cluster 127.0.0.1:55406 (C locale), DB openbot_p06_merge05;
  SQLX_OFFLINE=true; worktree-local target.

## Status

Investigation started 2026-11-19: inventory of HTTP gates/lists/pane/settings and
02B audit writers. Architecture gate expected (audit schema + acting-role recording;
UI entry gate for unit managers).

## Inventory (2026-11-19)

Read-only Explore inventory of every HTTP gate, list producer, the Departments pane,
company settings and authorization fixtures; key facts verified by root where they
drive decisions below. Choke points: `CompanyUseCases::managed_company` (403 to
members), `load_managed_company` + `list_managed_companies` (switcher empty for a
lead), `ChannelUseCases::list_company_channels` (root, all rows),
`DepartmentUseCases::{list_departments,principal_choices}` (root), `visible_departments`
/`visible_department`/`desk_departments` (root = all; others = own placements only, no
descendants), `reads().list_definitions` (unfiltered), rail `rail_section_visible`
(pages/mailbox.rs ~1674). Already scoped: agents list (`list_managed_agents`),
schedules list and binding choices, per-item binding visibility, every use-case write.

## User decisions (2026-11-19)
- U3: unit managers may READ the company model choices (provider/model names; the
  entity carries no credentials) and the company skills list for agent authoring, and
  may use the system-prompt generator (spends the company model key). Writing model
  connections and skills stays owner-only.
- U4: Runs, Deliveries and Dashboard stay root-only (owner/admin), decision 5.

## 06DA-P02-D1/r1 — architecture gate for 02A and 02B (independent PASS required)

### 02A: lead entry, lists, pane, rail, 404, 01A.2

1. **Two company gates, both 404.** `CompanyUseCases` gains
   `administered_company(user, company) -> CompanyAccess`: readable company AND
   `management_role(user, company, ManagementTarget::AnyUnit)` is Some; otherwise
   `NotFound` (same as a stranger). `managed_company` keeps meaning "manages the root
   unit" (owner/admin) but its refusal becomes `NotFound` too (02A.4). `owned_company`
   (owner-only operations, README "What does not change") is unchanged, keeping 403.
   `administered_company` is an entry gate only; every object still requires its own
   target check (existing use cases/persistence). Switchers use
   `list_administered_companies` for (a) workspaces.
2. **One "any unit" decision.** New SQL STABLE function
   `any_unit_management_role(user, company)` holds the existing AnyUnit query
   (root plus the actor's lead units, each through `unit_management_role`); the
   persistence `ManagementTarget::AnyUnit` arm calls it. `CompanyAccess` gains
   `administers_units: bool` computed in ACCESSIBLE_COMPANY_COLUMNS by the same
   function. It is a rendering hint (rail/links) only, never authority.
3. **Class (a) workspaces move to `administered_company`:** ui_channels, ui_agents,
   ui_schedules, workflows/ui authoring + bindings (`Ui::company`/`Ui::frame` gain an
   administered variant; runs/history/controls/events stay root), ui_departments
   writes, legacy HTML/HTMX channel.rs and agent.rs pages. Class (b) stays root
   (`managed_company`/`owned_company`): connection levels/MCP, local agents, skill
   library writes, onboarding, company edit/settings/team/invites, simulation,
   workflow tool registry (company-wide tool names). Class (c) stays root: runs,
   deliveries, dashboard, run counts, attention team view (U4).
4. **Scoped lists (configuration shows what the viewer manages):**
   - Desks: `ChannelUseCases::list_managed_channels(user, company)` — AnyUnit entry,
     rows whose `department_id` satisfies `unit_management_role IS NOT NULL` (one SQL
     filter, root managers get all). Replaces root `list_company_channels` on (a)
     pages; `list_company_channels` stays for root-only callers (deliveries).
   - Departments: `visible_departments`/`visible_department`/`desk_departments` for a
     non-root viewer = current placements (members/role holders, as today) UNION the
     managed subtree (`unit_management_role IS NOT NULL`). Root managers unchanged.
     `list_departments` and `principal_choices` move from Root to AnyUnit entry
     (principal choices are names for pickers; every assignment is rechecked).
   - Definitions: `list_managed_definitions` filtered by
     `workflow_definition_management_role IS NOT NULL`, used by the authoring library;
     the root runs pages keep the unfiltered read.
   - Agents, schedules, bindings: already scoped; only their entry gates change.
   - Agent pane reads (U3): `list_company_model_connections`, skills list and prompt
     generator authorize with AnyUnit instead of root.
   "Plus what they may read": read surfaces (mailbox, knowledge, department membership
   view) keep their own grant rules; configuration lists do not add readable-only rows,
   except departments, where the viewer's own placements stay listed (as today).
5. **Departments pane per-department controls.** `DepartmentSection` carries the set
   of departments the viewer manages (`AdministrationPersistence::managed_units`,
   bounded, same SQL filter) plus `is_owner`. Rules mirror decision 4/D1 r1:
   create child / edit / delete / reparent / lead assign-clear of D need management of
   D's parent (root itself: edit needs root management; root roles need the owner);
   members of D need management of D. The New button and parent options list only
   managed units (root only for root managers). The role picker (lead form) is shown
   only where the viewer manages the parent. `editable` derived from membership is
   removed. Server refusals are unchanged (use cases), so the UI only reflects them.
6. **Rail:** `MailboxUser` gains `administers_units`; Channels, Agents, Workflows and
   Schedules show for `administers_units`; Runs/Deliveries/Dashboard stay root.
   All `with_company_membership` sites set it from `CompanyAccess` (one builder).
7. **01A.2:** `CompanyMembership::manages_company_operations` is removed. Remaining
   root-level callers use `CompanyMembership::manages_root_unit()` (owner|admin),
   documented as equal to `unit_management_role(user, company, root)` being Some.
   Consumers: attention (c), thread_activity (c), department/root.rs root-member
   removal (root unit), mcp level_actor (b), LevelActor::manages_levels (b),
   company.rs managed_company_access (root gate), list_managed_companies (root
   switchers), mailbox Runs/Deliveries/Dashboard/local-agent link (c/b). Department
   visibility callers move to subtree-aware queries (item 4).
8. **Settings page (02A.3):** company-level controls stay owner-only as today (a
   subset of root managers; not widened). A lead reaching `?tab=departments` sees the
   subtree-aware department list with per-department controls (items 4–5). Counts
   remain owner-only.
9. **Tests (02A.1/02A.4):** shared `authorization_test_support::Fixture` gains a root
   admin and a department lead (`delegated_unit` helper) plus a sibling lead; every
   `*_authorization_tests.rs` adds: lead of the right unit succeeds on (a) routes;
   sibling lead and plain member get the same 404 as a stranger; leads get 404 on (b)/
   (c) root routes. Member 403 expectations on `managed_company` routes become 404.
   authoring `access_tests.rs` gains a lead who edits a definition bound in their
   subtree and cannot bind it outside (`binding.outside_managed_units`).

### 02B: administration audit

10. **Schema:** `administration_audit_events(id uuid PK, company_id uuid NOT NULL
    REFERENCES companies ON DELETE CASCADE, actor_principal_id uuid NOT NULL,
    acting_role text NOT NULL, target_kind text NOT NULL, target_id uuid NOT NULL,
    action text NOT NULL, subject_principal_id uuid NULL, created_at timestamptz NOT
    NULL DEFAULT clock_timestamp())`. `target_kind` is `department`; `target_id` is the
    department (pane key; no FK so history survives department deletion). `action` ∈
    {created, updated, moved, deleted, member_added, member_removed, role_assigned,
    role_cleared}; `subject_principal_id` names the member/role holder (needed for the
    pane; one column beyond the plan's list). `acting_role` CHECK
    `^(owner|admin|lead:<uuid>)$`. No FK on principal ids (history survives removal),
    company FK cascades. Append-only trigger mirroring `reject_binding_audit_rewrite`
    (UPDATE always refused; DELETE refused unless cascaded, pg_trigger_depth()>1).
    Index (company_id, target_id, created_at DESC, id DESC). Fresh schema, no backfill.
11. **Writers:** department create/update/reparent/delete, add/remove member,
    assign/clear role in persistence (department.rs/lifecycle) append one row in the
    same transaction, after the locked recheck, using the `ManagementRole` that
    recheck returned and the actor's person principal. Writes without a person actor
    (company bootstrap, system/provisioning paths that bypass the managed entry) do
    not audit; agents never administer (decision 6). Owner-only root team writes
    (company member removal/role change on the team page) are outside 02B.1's list.
12. **binding_audit_events.acting_role text NULL** with the same CHECK; desk writes
    that already hold the locked management role (create/settings update through
    ChannelPersistence) pass it to `append_audit_event`; system/agent/person-address
    paths pass None. Readers decode it; JSON shape of existing endpoints unchanged
    unless already exposing audit rows (then additive field only if present).
13. **Pane:** the Departments pane shows the last ten events for the selected
    department, newest first (created_at, id), to viewers who manage that department.
    Rendering escapes names; actor/subject names resolved at read time, falling back
    to "removed person" when gone.
14. **Concurrency:** audit insert is inside the existing serialized write, so a
    role removal racing a write yields either (write + its audit row with the old
    role) or (refusal, no row). Competing test reuses `competing_writes`.

**Failure behavior:** gates return 404 without leaking existence; list queries are
bounded as today; audit insert failure aborts the management write (atomic).
**Verification plan:** focused HTTP authorization families per file, department pane
render tests for lead/sibling/root, rail visibility test, list filter DB tests,
`administration_audit_` (each write kind one row with right acting role; pane renders
ten; UPDATE/DELETE refused; cascade on company delete works; competing role removal),
binding audit acting role test, fresh migration + sqlx prepare, source_modules,
`manages_company_operations` absent (grep), discovery comparison.

Batches: B1 = 02A (items 1–9), B2 = 02B (items 10–14), each with one batch review.

## D1/r1 review result and user decisions U5/U6

06DA-P02-D1-R1-N1: not passed. F1–F7 actionable, F8 optional. Coordination is r7
(TASK previously cited r6; pinned now as logs/D1-r2-coordination-r7.md).
User decisions 2026-11-19:
- U5 (F2): a person who is lead of the root manages every unit (departments, desks,
  agents, definitions, schedules, bindings, knowledge) through the unit predicate, but
  company-level administration stays owner + root admin (membership roles): team
  levels, invites, MCP/HTTP connections and levels, memory settings, local agents,
  runs/deliveries/dashboard/attention team view. Two named rules, both tested.
- U6 (F5): desks list = managed desks plus desks the viewer may read (View grant),
  readable-only rows read-only; departments = managed subtree plus own placements;
  agents, definitions, schedules, bindings = managed only (no read rule exists).

## 06DA-P02-D1/r2 — supersedes r1 where stated; r1 items not mentioned stand

- r2-1 (F2/U5) Two named rules. `CompanyMembership::manages_company_operations` is
  replaced by `CompanyMembership::administers_company()` (owner | root admin), doc:
  "company-level administration; distinct from unit management — a root lead manages
  every unit but does not administer the company (Plan06 U5)". r1 item 7's claimed
  equality with `unit_management_role(root)` is withdrawn. Company-level gates
  (`managed_company`, which keeps its name and now documents "company
  administration") use membership. Unit gates
  (`administered_company` entry = AnyUnit, per-target checks) use the predicate, so a
  root lead enters (a) workspaces and manages all units. Tests: root lead succeeds on
  (a) routes and per-target writes; gets 404 on (b)/(c) routes; root admin succeeds on
  both. Root roles still need the owner (D1 r1 item 4 of p01), unchanged.
- r2-2 (F3) All management refusals are 404, including `owned_company` (owner-only
  operations) for members and admins; strangers already get 404. Owner-only *who* is
  unchanged (README "What does not change"); only the refusal status moves to 404 per
  02A.4. Existing 403 assertions on these routes become 404 (each listed in the B1
  test-change inventory; no assertion is dropped).
- r2-3 (F4) `list_departments` (desk/department pickers, destinations) returns the
  departments the viewer manages (`unit_management_role IS NOT NULL`; all for owner/
  admin/root lead). The visible-department reads (r1 item 4) stay managed ∪ placements.
  Pickers therefore offer only destinations the server accepts.
- r2-4 (F5/U6) Desk workspace list = `list_managed_channels` ∪ desks readable through
  the existing desk read rule (current `list_managed_readable_channels`-style read,
  View grant), each row tagged managed/readable; readable-only rows render without edit
  controls and link to the conversation. Agents/definitions/schedules/bindings managed
  only.
- r2-5 (F1) **Reuse `department_administration_events`** instead of a new table. It
  becomes the one department administration history (README 02B "a new table" is
  satisfied by extending the existing append-only department history; recorded
  deviation, justified by one history for the pane, snapshot labels, and the stricter
  company-gone-only DELETE guard). Changes (fresh schema):
  - `actor_kind` ∈ {system, person}. New nullable columns `actor_principal_id uuid`,
    `actor_label text` (snapshot, ≤255 bytes), `acting_role text` (CHECK
    `^(owner|admin|lead:<uuid>)$`).
  - `transition_kind` adds person kinds: department_created, department_updated,
    department_moved, department_deleted, member_added, member_removed, role_assigned,
    role_cleared. System kinds stay demoted, deleted.
  - `principal_id/principal_kind/principal_label` become NULL-able only for the four
    department_* kinds; `role` NULL-able except for demoted/deleted/role_*; CHECKs tie
    each kind to its required fields; system rows require actor fields NULL, person rows
    require all three actor fields.
  - Insert guard split by actor_kind: system rows keep the existing guard unchanged
    (trigger depth, actual held role snapshot) and its deferred role-cleared check;
    person rows require a current same-company person `actor_principal_id`, a label
    equal to its `display_label`, `department_label` equal to the current department's
    audit label (for department_deleted, taken before the delete in the same
    transaction), and, for member/role kinds, `principal_label` equal to the subject's
    current label. The existing immutability trigger (UPDATE refused, DELETE only once
    the company row is gone) covers both kinds unchanged.
  - Index (company_id, department_id, created_at DESC, id DESC) for the pane.
  Plan columns map: actor_principal_id, acting_role as named; target_kind/target_id =
  department_id (+ principal_id for member/role); action = transition_kind.
- r2-6 (F6) Complete writer inventory (each appends in the same transaction, after its
  locked recheck, using that recheck's `ManagementRole`; one `operation_id` per user
  operation):
  - department create (+ role_assigned for a create-with-lead, same operation),
    update, delete; reparent records department_moved with the role returned for the
    destination parent check (both checks must pass; the destination is the authority
    that admitted the new placement).
  - member add/remove (department pane), root-member removal (owner, pane and team
    page), role assign/clear (pane), team-page `update_member_role` (root admin role
    assign/clear by the owner).
  - agent placement by a manager on agent create/copy/Builder construction
    (member_added for the agent principal, acting role from the placement check).
  - `workflow_definition_events` and `workflow_binding_events` gain `acting_role text
    NULL` (same CHECK); their single insert sites pass the role from the definition/
    binding management check; NULL only for system paths (provisioning).
  - `binding_audit_events.acting_role` (r1 item 12) for desk create/settings writes;
    agent/person address syncs and provisioning pass NULL because they are derived
    effects of a write audited at its own row or system writes (stated, not silent).
- r2-7 (F7) Admin member-invite UI in B1: team page shows the invite form to root
  admins with only the member role offered, lists invites with delete/level controls
  for member invites (admin invites read-only for admins), stale ui_team.rs comment
  fixed. Owner view unchanged. Route tests cover admin form/list/delete.
- r2-8 (F8) Pane rule wording: create child of D needs management of D; edit/delete/
  reparent/lead assign-clear of D need management of D's parent (root: edit needs
  root management; root roles need the owner); members of D need management of D.
- r2-9 Pane visibility of history (r1 item 13): shown to viewers who manage the
  department; rows show snapshot labels, so no read-time name resolution.

B1 = 02A (r1 items 1–9 as amended by r2-1..4, r2-7, r2-8). B2 = 02B (r2-5, r2-6,
r2-9, r1 item 14 competing test). Verification plan as r1 plus: root-lead/admin
two-rule tests; 403→404 change inventory; picker-destination test; readable-desk row
test; system-row guard regressions (existing department_retirement_audit_tests stay
green); each new writer one row with the right role; definition/binding event role.
