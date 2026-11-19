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

## D1/r2 PASS (06DA-P02-D1-R2-N1) and implementation notes

Gate passed for the design and verification plan (pins logs/D1-r2-TASK.md,
logs/D1-r2-coordination-r7.md). Carried conditions and optional findings, dispositioned:
- 02B: the deferred role-cleared constraint trigger applies only to system rows; a
  root person removal writes its person row before the principal is retired.
- F6 add-on: agent placement with a role (e.g. created as lead) also records role_assigned.
- F7: the team page also gets an edit control for member invites (decision b allows it);
  the "addressed to a current admin" refusal renders inline.
- F9 (accepted): root-department member-removal controls follow the server rules —
  root agents need owner/root admin, root persons need the owner — not the generic
  "manages D" rule; render test for a root lead.
- F10 (accepted, scope of r2-2): 404 applies to entry/authority refusals (the viewer
  may not manage or administer the target or company). Business refusals inside an
  authorized scope keep their status: admin-role invite and admin-address refusals,
  access-level ceilings, knowledge level limits stay 403. Listed in the B1 inventory.

## Batch B1 (02A) — implementation started 2026-11-19
Checkpoint (B1 in progress, uncommitted): manages_company_operations→administers_company
(all callers); SQL any_unit_management_role + CompanyAccess.administers_units;
administered_company/list_administered_companies + 404 for managed/owned refusals;
load_administered_company; AdministrationPersistence::managed_units (+ doubles);
ChannelUseCases::{list_managed_channels,list_workspace_channels(WorkspaceChannel)};
department list_departments (managed), visible_* (placements ∪ managed), principal_choices
(AnyUnit); WorkflowReads::list_definitions(managed_by); agent model choices/prompt
generator and skill reads at AnyUnit. Next: switch (a) UI routes, pane, rail, team UI, tests.
Checkpoint 2 (B1 production changes compiled, uncommitted): UI (a) routes use
administered_company/load_administered_company (ui_channels, ui_agents, ui_schedules,
workflows authoring+bindings via Ui::unit_company/unit_frame, ui_departments writes,
legacy channel.rs/agent.rs); Ui::detail now authorizes ManageDefinition (every authoring
by-id read passes through it); Runs/Tools buttons and versions tool form only for company
administrators; desk workspace = managed + read-only readable rows (link to mailbox);
department pane controls per rule (manages_placement / assigns_roles / manages_members,
root exceptions F9); rail via MailboxUser.administers_units (with_company_access/option/
unit_hint); team page: admins invite/edit/delete member invites only (TeamRole::invites/
manages_invite/invite_roles), Forbidden business refusals inline; level_manager refusal is
404. Running full lib suite (logs/B1-explore-full-r1.txt) to inventory changed expectations.
User decisions 2026-11-19 (cont.):
- U7: binding a version to a desk outside the managed units stays the stranger's 404 (p01
  B2/B3 accepted design, 02A.4); the 01B.4 code `binding.outside_managed_units` is not added.
- Reviewer: user asked for Fable 5.2/medium; 5.2 is not offered here (latest Fable is 5.1,
  `fable` alias = claude-fable-5-1, as in Plan05 p04). User chose to keep the Opus 5.5 reviewer.

### B1 (02A) acceptance mapping

| Criterion | Production path/case | Decisive assertion | Test | Status |
| --- | --- | --- | --- | --- |
| 02A.1 lead reaches unit routes | /companies/{id}/channels page + desk fragment; agents page; department writes | lead Ok, sibling/member 404, root desk not listed to lead | delegated_admin_routes_desk_pages_list_only_managed_desks_and_hide_the_rest; ..._unit_leads_and_root_admins_reach_the_agents_page; ..._department_writes_follow_the_lead_unit | see run |
| 02A.1 authoring | editor/versions/bindings panes; bind in/outside unit; library | lead edits+binds inside; outside refused (404/U7), sibling 404 on all tabs; library lists only managed | delegated_admin_authoring_lead_edits_a_definition_bound_in_their_unit_and_cannot_bind_outside | see run |
| 02A.1/U4/U5 root-only routes | company edit/delete, onboarding, legacy invites, dashboard | lead (and admin on owner-only) 404 | delegated_admin_routes_company_settings_hide_from_leads_and_admins; ..._onboarding_hides_from_leads_and_admins; ..._legacy_invite_pages_hide_from_unit_leads; dashboard_resolves_operator_... (lead case) | see run |
| 02A.2 lists | desk workspace, department visibility/pickers, definitions | managed first then readable-only; pickers = managed; visible = placements ∪ managed | delegated_admin_routes_desk_workspace_lists_managed_then_readable_only_desks; delegated_admin_lists_departments_visible_managed_and_picker_scopes; authoring test library assertion | see run |
| 02A.2 pane controls | DepartmentSection per-rule controls, F9 root | each control only where server rule passes; root lead gets no root member controls | delegated_admin_ui_pane_offers_each_control_only_where_its_server_rule_passes; delegated_admin_routes_department_writes_follow_the_lead_unit (new form options) | see run |
| 02A.2 rail | MailboxUser.administers_units | lead sees Channels/Agents/Workflows/Schedules, not Runs/Deliveries/Dashboard; member none | delegated_admin_ui_lead_rail_and_read_only_desk_rows | see run |
| 02A.3 settings | company settings owner-only; departments tab scoped | covered by company settings 404 + departments visibility tests | as above | see run |
| 02A.4 404 | managed_company/owned_company/level_manager/onboarding persistence/require_inviter | member/admin-on-owner-only/lead → 404; business refusals stay 403 (F10) | shared fixture denial_cases + updated families (inventory below) | see run |
| U5 two rules | administers_company vs predicate | root lead: administered_company Ok, managed_company 404, all units | delegated_admin_lists_unit_entry_gate_and_hint_follow_the_predicate | see run |
| F7/decision b UI | team page | admin member invite via route stored member; admin-role inline refusal; admin invite pane edit/delete/level for member invites only; no role control | team_mutations_refuse_members_admins_foreign_and_unknown_company_scopes; access_level_both_team_pages_give_admins_only_level_controls | see run |
| 01A.2 | manages_company_operations removed | `grep -rn manages_company_operations src` empty | B1-grep evidence | PASS |

### B1 contract impact

| Changed contract | Consumers | Disposition |
| --- | --- | --- |
| CompanyMembership::manages_company_operations → administers_company | all former callers (rename, same semantics: owner/admin) | company-level gates only; unit gates use predicate |
| managed_company / owned_company refusal 403→404 | every route/use case using them (connection levels, local agents, skill writes, onboarding, company edit, simulation, runs, deliveries, dashboard, team owner ops, mcp level_actor unaffected) | tests updated (inventory) |
| level_manager refusal 403→404 | invites, level routes | tests updated |
| CompanyAccess.administers_units (new field) | persistence ACCESSIBLE_COMPANY_COLUMNS, mocks, ui_dashboard operator access | rendering hint only |
| AdministrationPersistence::managed_units (new) | department use cases, ChannelUseCases | all impls (2 real + 5 doubles) |
| WorkflowReads::list_definitions(managed_by) | authoring library (Some), JSON definitions list (Some, ListManaged), runs scope options (None), tests | JSON shape unchanged |
| JSON /companies/{id}/channels list | now managed desks only for unit managers; root managers unchanged | shape unchanged |
| DepartmentUseCases::list_departments (managed only) and principal_choices (AnyUnit) | desk forms, department pane | pickers offer accepted destinations |
| visible_departments/visible_department/desk_departments | department pane, attention/dashboard department filters, desk grouping | non-admins add managed subtree |
| skill reads, model choices, prompt generator → AnyUnit (U3) | agents pane, skill JSON list | writes unchanged (owner) |
| Ui::detail authorizes ManageDefinition | every authoring by-id pane | closes by-id read for unit-entry users |
| onboarding persistence begin refusal 403→404 | onboarding | consistent with owned_company |

### B1 test-change inventory (existing assertions changed, none dropped)
- authorization_test_support denial_cases: member 403→404.
- agent_authorization_tests (member 404); ui_dashboard (member 404 + lead case).
- company_invite_level_route_tests: plain member 403→404 (2 sites); admin cap refusals stay 403.
- local_machines_tests member POST 403→404; skill_library http_bundle_tests member read 403→404, owner-only import 403→404.
- local_agents pairing_tests member 403→404; onboarding persistence tests member/admin 403→404.
- company use-case tests: managed_company member 404; owner-only update/delete admin/member 404.
- RENAMED: only_the_owner_may_update_or_delete_and_members_are_refused_not_hidden →
  only_the_owner_may_update_or_delete_and_everyone_else_is_told_nothing;
  team_roles_come_from_membership_and_nonowners_get_forbidden →
  team_roles_come_from_membership_and_noninviters_are_told_nothing.
- team_mutations_refuse_members_admins_foreign_and_unknown_company_scopes: member 404;
  admin admin-role invite inline refusal; admin role/removal 404.
- access_level_both_team_pages_give_admins_only_level_controls: admin's member-invite pane
  now has email/withdraw/level but no role; admin-role invite offers an admin nothing.
- knowledge tests unchanged in B1.

### B1 frozen artifact (06DA-P02-B1-R1)
Base 5874dfb1 (p01 accepted + learning commit), digest
6248bc8cd74f200a4e11f9ba2d00fd37eee979cfc341cb09f363d7e05814897d, exclusions
plan/06-delegated-administration/phases/p02/docs, plan/06-delegated-administration/phases/p01/docs
(another phase's immutable records incl. untracked tars), plan/06-delegated-administration/COORDINATION.md
(logs/B1-author-freeze.snapshot.txt; 84 changed paths). Coordination r8 pinned
(logs/B1-coordination-r8.md, 3ea7a412…). Checks (owned PG18, fresh openbot_p06_merge05,
SQLX_OFFLINE=true): fmt, diff --check, clippy -D warnings, sqlx prepare --check, source_modules PASS;
full lib 4869 PASS / 3 FAIL / 30 ignored (B1-full-r2.txt): the 3 failures are the known
unrelated C-locale auth pair and the deferred Doop version assertion. Focused new/changed set
33 PASS (B1-focused-r1.txt, pre-final-clippy edit: only `std::slice::from_ref` in a test).
Discovery 4890→4902: +12 new, 2 renames (B1-discovery-after.txt vs p01 B4-R3-discovery-after).
Exploratory inventory run before test updates retained: B1-explore-full-r1.txt (23 FAIL).

### B1 R1 findings and correction (R2)
06DA-P02-B1-R1-N1: F1, F2 medium; F3 low; F4 optional. All dispositioned:
- F1 FIXED: attention team view and operational summary refuse non-administrators with
  company_not_found (404); `company_forbidden` had no other caller and is removed.
  attention_authorization_tests member 403→404; new
  delegated_admin_routes_attention_team_views_hide_from_unit_leads (lead 404 on team view and
  summary, own queue OK, admin/owner summary OK). Runs authorization test gains a unit lead in
  its denied set (404 on reconcile/step).
- F2 FIXED: agent "Used by" card (GET /ui/workflows/agents/{agent}/usage) enters with
  unit_company and requires ManagementTarget::Agent via new CompanyUseCases::require_management
  (listed bindings still filtered per desk by visible_usage); test
  delegated_admin_authoring_agent_usage_card_follows_the_agents_departments. Run-count bar and
  its SSE source render on desk/agent pages only for company administrators (U4); render
  assertion added to delegated_admin_ui_lead_rail_and_read_only_desk_rows.
- F3 FIXED: JSON bindings list and single-binding read (visible_binding) enter with
  ListManaged; each binding still requires its association's Inspect (visible_to). Shared
  callers checked: run board and runs UI keep their own root gate; start_command with no
  association resolves via visible_binding, but the start service authorizes Admit on the
  association (desk view + unit management, p01-accepted), so no authority is added.
- F4 (optional) dispositions: legacy /companies/{id}/channels HTML/JSON lists show managed
  desks only (configuration lists; U6 read-only rows live in the desk workspace) — recorded,
  not changed; legacy /companies/{id}/invites page stays owner-only (the team page is the
  admin path) — recorded; authoring test tightened: outside-desk bind renders exactly the
  not-available message a nonexistent desk gets (form-level 404, U7); root lead is not offered
  root "add member" although the server allows it — conservative, recorded.
R2 frozen: base 5874dfb1, digest 2c8e4add9aa78a22a356146f998028cbe4b0c0c8e9a35b668c29f4b692eb2f3d, same three exclusions
(logs/B1-R2-author-freeze.snapshot.txt), coordination r8 unchanged.
Checks: fmt, diff --check, clippy -D warnings, sqlx --check, source_modules PASS; focused
491 PASS/0 FAIL/6 ignored (B1-R2-focused.txt: delegated_admin, attention, routes::workflows,
pages, ui_team, company_invite); dashboard/operational_summary/department_views 66 PASS
(B1-R2-dashboard.txt). Discovery +2 vs R1, 0 removed (B1-R2-discovery-after.txt).

### Root B1 acceptance
06DA-P02-B1-R2/N1 PASS for 2c8e4add9aa78a22a356146f998028cbe4b0c0c8e9a35b668c29f4b692eb2f3d.
Root reproduced the digest and file list (logs/B1-root-acceptance.snapshot.txt), validated author
and reviewer R2 manifests, coordination r8 byte-identical to its pin, no open finding.
B1 (02A + 01A.2) ACCEPTED IN PHASE. Full suite not rerun after R2 corrections (localized); the
final full gate in p03 is required. User authorized (2026-11-19) a local commit after each
accepted batch for the rest of Plan06, never a push. Next: B2 (02B audit).

## Batch B2 (02B audit) — implementation (gate D1/r2 items r2-5, r2-6, r2-9, r1-14)

Base 1bb9c969 (B1 committed). Implementation:
- Schema (init migration, fresh only): `valid_acting_role(text)`; `acting_role` column with that
  CHECK on binding_audit_events, workflow_binding_events, workflow_definition_events;
  department_administration_events extended per r2-5 (nullable subject/role for department
  kinds, person transition kinds, actor_kind system|person, actor_principal_id/actor_label/
  acting_role, `department_administration_event_shape` CHECK per kind, recent index). Insert
  guard branches: person rows must snapshot a current same-company person actor (label =
  display_label), the current department label and, for member/role kinds, the subject's
  current kind+label; system rows keep the original guard. Deferred role-cleared check applies
  to system rows only (carried condition). Immutability trigger unchanged.
- Writer: `department/audit.rs::record_on` (INSERT…SELECT snapshot; one operation id per
  transaction via tx-local `openbot.administration_operation`; refuses with Internal if actor or
  subject is not current). `require_department_management_on`, `require_parent_management_on`,
  `root_role_authority_on` now return the `ManagementRole` they checked; the writes record it:
  create (in create_department_with_id_on, before its lead; onboarding passes Owner), update
  (Updated with parent role; Moved with destination role when the parent actually changed),
  delete (row before the delete), add/remove member (only on actual change; `write_placement_on`
  now reports `Placed{member_added, role_assigned}`), assign/clear role (in the `_on` helpers,
  so create-with-lead and every caller audit), root member removal (owner/admin; person row
  before retirement), team-page update_member_role (set_admin_on now returns whether it changed;
  RoleAssigned/Cleared admin on root as Owner) and remove_member (row before retirement), agent
  placement (prepare: MemberAdded with its check's role; place_in_department_on: MemberAdded on
  actual insert with the predicate's role for that unit under the same lock, RoleAssigned with
  its role check's role). Invite acceptance (invitee's own act) records nothing.
- Workflow events: definition events record `management_role_on(DefinitionCreation(workflow))`,
  binding events `management::association_role_on` (new; target_on + predicate), both in the
  write's transaction (the write does not change the target's management). binding_audit_events
  computes the person's role over the binding's target in the INSERT (desk unit via
  unit_management_role, agent via agent_management_role; NULL for system provenance and person
  addresses). Desk settings updates now pass the updating person's provenance to the email
  binding write, so update audit rows name who changed the desk (previously system).
- Pane: entity `DepartmentAdministrationEntry`/`Action` + `DEPARTMENT_HISTORY_ENTRIES`=10; port
  `administration_history`; use case `administration_history` (Unit management, else 404);
  Departments pane renders "Recent administration" only for the department's managers, escaped,
  acting role named ("lead of <department>").

### B2 acceptance mapping
| Criterion | Path/case | Decisive assertion | Test |
| --- | --- | --- | --- |
| 02B.1 department writes | lead create/edit/move/add/assign/clear/remove; owner delete; admin create | exactly one row per kind, ordered, acting role lead:<Sales> (move: destination), owner/admin; no row for no-op re-add or refused sibling write | administration_audit_each_write_kind_records_its_admitting_role |
| 02B.1 immutable/forgery/cascade | UPDATE, DELETE, forged person row, company delete | immutability refusal; source guard refusal; rows gone with company | administration_audit_rows_are_immutable_and_forgery_is_refused |
| r1-14 competing | lead add_member vs owner clear lead, both orders, real writers, own DB | write-first: member + row (lead role); revoke-first: refused, no member, no row | administration_audit_competing_role_removal_and_management_write_in_both_orders |
| team page root writes | owner promote admin (twice), remove member | role_assigned admin + member_removed as owner; no-op records nothing | administration_audit_team_page_root_role_and_removal_are_the_owners |
| 02B.1 existing audits record role | desk binding audit; workflow binding events; definition events | lead:<unit> on lead writes; owner on owner writes; system NULL | administration_audit_desk_binding_events_record_the_acting_role; delegated_admin_binding_service_configures_activates_and_pauses_captured_messaging (added assertions); delegated_admin_definition_unbound_crud_uses_the_recorded_creating_unit (added assertions) |
| 02B.2 pane | manager vs outsider render | card with escaped labels, "as lead of …", system entries; none for outsider | administration_audit_pane_shows_recent_history_to_managers_only; administration_history 404 for sibling in the first test |
| system retirement unchanged | existing retirement tests | same assertions on system rows | retirement_audit / follow retirement families |

### B2 test-change inventory (no assertion dropped)
- department_retirement_audit_tests: `events()` and the forged re-insert select only
  `actor_kind='system'`; the fault trigger corrupts system rows only (person rows now share
  the table; the tests' subject is the system retirement guard).
- follow_retirement_tests: three audit reads and one rollback count filter `actor_kind='system'`.
- delegated_definition_tests / delegated_binding_management_tests: assertions added.
- New: delegated_administration_tests (3 tests), delegated_invite_tests (1), delegated_desk_tests
  (1), department_ui_tests (1); WriteBarrier::DepartmentMembers.
Focused: B2-focused-r1 (9 FAIL, retained), r2 (3 FAIL: shared-DB table lock, forged select),
r3 15 PASS, final 1072 PASS/0 FAIL (B2-focused-final.txt). fmt/ws/clippy/sqlx/source_modules PASS.

### B2 frozen artifact (06DA-P02-B2-R1)
Base 1bb9c969 (B1 commit), digest b2c252facf79abb3066bf667a843e7e91e522cea7ac82534d3630188fa9f2dce,
same three exclusions (logs/B2-author-freeze.snapshot.txt, 28 paths). Coordination r9
(committed with B1; pin logs/B2-coordination-r9.md). Full lib 4877 PASS / 3 FAIL (known unrelated
C-locale auth pair, deferred Doop assertion) / 30 ignored (B2-full.txt, fresh schema). Discovery
vs B1-R2: +6 new, 0 removed (B2-discovery-after.txt).

### B2 R1 findings and correction (R2)
06DA-P02-B2-R1-N1: F1, F2 low actionable; F3 optional.
- F1 FIXED: `write_placement_on` returns the holders a single-holder role was taken from
  (`DELETE … RETURNING`); `audit::record_role_placement_on` records `role_cleared` for each, then
  `role_assigned`, same operation and acting role; used by assign_department_role_on (so the
  pane's lead change, create-with-lead) and place_in_department_on. Test
  administration_audit_replacing_a_lead_records_the_replaced_holder.
- F2 FIXED (tests): agent placement with a role (member_added once + role_assigned, lead role)
  administration_audit_agent_placement_records_membership_once_and_its_role; root removal on the
  pane, agent as admin and person as owner, rows before retirement
  administration_audit_root_removal_records_every_department_left; onboarding Created/member rows
  as owner, once across replays (assertions added to
  agent_default_desk_onboarding_creates_and_replays_the_first_department); unmanaged placement
  refusal with no row administration_audit_unmanaged_placement_is_refused_without_history.
- F3 part 2 FIXED: a person leaving the company (team-page remove_member, pane root person
  removal) records `member_removed` in every department they were a member of, root included,
  before retirement (`audit::record_member_departure_on`); covered by the root-removal test.
- F3 part 1 SCOPE DECISION (recorded, to report to the user): invite acceptance (root membership
  and an invitation's admin role) records no department history row. It is the invitee's own act;
  the invitation that carries the role is the administration (owner-only for admin-role invites,
  decision b), and invites store no inviter to attribute it to.
R2 frozen: base 1bb9c969, digest 802700ab809070e6ef89a5b33288e0b60579cbb53012ead62e5404211634f32f,
same exclusions (logs/B2-R2-author-freeze.snapshot.txt), coordination r9 unchanged. Checks:
fmt, diff --check, clippy, sqlx --check, source_modules PASS; focused 1083 PASS/0 FAIL/1 ignored
(B2-R2-focused-final.txt, before a test-only type alias for clippy); final administration_audit +
delegated_administration 46 PASS on the frozen source (B2-R2-final-audit.txt). Discovery +4 vs R1,
0 removed (B2-R2-discovery-after.txt).

### Root B2 acceptance — p02 complete in phase
06DA-P02-B2-R2/N1 PASS for 802700ab809070e6ef89a5b33288e0b60579cbb53012ead62e5404211634f32f; root
reproduced digest and files (logs/B2-root-acceptance.snapshot.txt), validated both R2 manifests,
coordination r9 byte-identical, no open finding. B2 ACCEPTED IN PHASE; with B1, p02 (doc 02 + 01A.2)
is accepted in phase. Committed locally per the user's standing authorization. Next: p03
(03A docs, 03B final gate on the owned UTF-8 cluster 127.0.0.1:55407).
