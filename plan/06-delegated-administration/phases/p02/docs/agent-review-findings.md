# Reviewer findings — 06DA-P02

Common context for every finding in round 06DA-P02-D1-R1:

- Date: 2026-11-19. Workflow: implement-opus-reviewed-2_4. Phase: p02.
- Stage: architecture gate D1/r1 (TASK.md "## 06DA-P02-D1/r1", immutable reviewer copy `logs/06DA-P02-D1-R1-reviewer-task-r1.md`, sha256 `7a6b8015…070d3`).
- Round note: [06DA-P02-D1-R1-N1](agent-review-notes.md).
- Reviewer: Claude Opus 5.5 (`claude-opus-5-5`); effort not visible to me (assigned xhigh).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan06`, branch `plan06`, HEAD `5874dfb1908aeb8d7663664ff2e6b2e4631c2c40` (2352dc38 plus a commit that adds only a learning). There is no p02 source; the code reviewed is the committed tree.
- All findings concern the gated decision text, not implementation.

## 06DA-P02-D1-F1 — the decision adds a second department audit table and ignores the existing one

- **Actionable / medium / architecture (02B.1, 02B.2) — OPEN.**
- **Evidence.** The schema already has `public.department_administration_events` (`migrations/20260817000000_init_schema.sql:17178–17260`). It is append-only history of department role transitions: role holder, department, role, `transition_kind` demoted/deleted, `actor_kind` CHECK 'system'. It has:
  - snapshot labels (`department_audit_label`), "so historical facts do not hold live principals or departments in existence";
  - an insert guard (`guard_department_retirement_audit`);
  - an immutability trigger that allows DELETE only once the company row is gone;
  - a deferred check that the role was actually cleared.

  It is exercised by `department_retirement_audit_tests.rs`, `root_unit_tests.rs` and `workflow/follow_retirement_tests.rs`. Neither D1/r1 nor its inventory mentions it.
- **What D1/r1 proposes instead.** A separate `administration_audit_events` table that differs from the existing one:
  - immutability via `pg_trigger_depth()>1`, which allows any trigger-initiated DELETE, not just a company cascade;
  - names resolved at read time with a "removed person" fallback, where the existing table snapshots labels;
  - a pane that would show manager events but not the system demotions and deletions of the same department's roles.
- **Impact.** Two parallel audit stores for the same department administration facts, with different immutability and retention semantics, contrary to the "one decision, one place" rule in `src/AGENTS.md`. The pane's "last ten administration events for the selected department" (02B.2) would silently omit system role removals. A deleted department's events would have no label left to render.
- **Required correction.** Choose and record one of:
  1. Extend `department_administration_events`: person actors with `acting_role`, the new actions, and a NULL-able subject. Keep its label snapshots and its company-absence immutability.
  2. Keep a new table, but justify the separation and define the pane as an ordered, bounded union of both sources.

  Either way:
  - align the immutability predicate with the existing company-absence form, or state why depth-based is enough;
  - state how the actor, subject and target names render after deletion, preferably as snapshots like the existing table;
  - add verification of the chosen pane union or extension.
- **Instruction lesson.** The helper-agent inventory searched writers and gates but not existing schema objects with the same purpose. Candidate for architecture gates that add a table: "List existing tables and triggers in the same domain, e.g. `grep -n 'CREATE TABLE public\..*<noun>'`, and dispose each." Evidence: this finding.

## 06DA-P02-D1-F2 — the decision equates `manages_root_unit()` with root management, but a root lead breaks that

- **Actionable / medium / authorization consistency (decision 1, 01A.2, U4) — OPEN.**
- **Evidence: a person can hold `lead` on the root department.**
  - `root_role_authority_on` (`src/adapters/persistence/department/root.rs:4–44`) requires the owner for any root role change. It refuses only `Owner`, and `Admin` off the root.
  - `guard_root_roles` returns early for roles other than owner and admin (`init_schema.sql:19863`).
  - `unit_management_role(user, company, root)` then returns `lead:<root>` (`init_schema.sql:15754`), and p01 per-target checks with `ManagementTarget::Root` (company-wide bindings, root desks, child creation under the root, AnyUnit) already treat that person as managing the root and every unit.
- **Evidence: the decision's equivalence claim does not hold.** D1/r1 item 7 defines `CompanyMembership::manages_root_unit()` as owner|admin and documents it as "equal to `unit_management_role(user, company, root)` being Some". With a root lead that is false. Item 7's membership-based consumers would exclude the root lead from root-level gates that decision 1 says they hold: `managed_company`, `list_managed_companies`, the MCP `level_actor`, `LevelActor::manages_levels`, `department/root.rs` root-member removal, and the mailbox local-agent link. Meanwhile `administered_company` and the per-target predicates include them.
- **Impact.** The same person is a root manager for some root-level surfaces and not for others. Which side applies depends on whether a call site uses membership or the predicate. The decision gives no rule, no user decision and no test for this.
- **Required correction.** Define root-lead semantics explicitly and test them, with a user decision where an original criterion conflicts (U4 already names owner/admin for Runs, Deliveries and Dashboard). Choose one:
  1. Forbid `lead` on the root unit by schema/persistence (and handle existing fixtures), which makes the equivalence true.
  2. Derive every root gate from `unit_management_role(root)`.
  3. Keep root-level operations owner/admin and document that a root lead manages every unit but not company-level operations.

  Fix the documented equivalence accordingly. Add a root-lead row to the `manages_unit_` matrix and to the route authorization fixtures.
- **Instruction lesson.** The B4-R1 note already raised a root lead who cannot invite as an observation. Root-level semantics should be one explicit decision, not an emergent property of whichever gate a call site uses.

## 06DA-P02-D1-F3 — owner-only routes keep 403, which contradicts the decision's own 404 rule and 02A.4

- **Actionable / medium / requirements consistency (02 Outcome, 02A.1, 02A.4) — OPEN.**
- **Evidence.**
  - Item 1 keeps `owned_company` refusing members and admins with 403 (`src/application/use_cases/company.rs:203–216`). Its call sites include company edit/delete (`routes/company.rs:130/141/156`, `ui_companies.rs:640`), onboarding, skill-library writes (`skill_library.rs:261/272/415`, `bundles.rs:70/93`), simulation (`channel/simulation.rs:76`), and the invite/team owner paths (`company_invite.rs:135/337/366`, `ui_team.rs:298`).
  - Item 9 promises that "leads get 404 on (b)/(c) root routes", and item 3 places exactly these routes in class (b).
  - 02A.4 says refusals are 404, and the 02 Outcome says management routes refuse "the rest the way it refuses strangers".
- **Impact.**
  - The decision is internally inconsistent; the tests planned in item 9 cannot pass on (b) owner-only routes as designed.
  - Leads and admins get a distinguishable 403 where a stranger gets 404.
  - "What does not change" fixes who may perform owner-only operations, not the refusal status.
- **Required correction.** Choose one and make items 1, 3 and 9 agree:
  1. Owner-only management refusals become 404 for non-owners, and existing 403 expectations are updated deliberately.
  2. Record a user decision that owner-only refusals stay 403, and restrict item 9's 404 claim to `managed_company`/`administered_company` routes.

  List the affected HTMX handlers whose inline error mapping depends on the status.
- **Instruction lesson.** None beyond checking that the test promises match the gate table.

## 06DA-P02-D1-F4 — `list_departments` at AnyUnit entry would show leads every department name in desk pickers

- **Actionable / medium / visibility + 02A.2 — OPEN.**
- **Evidence.**
  - Item 4 moves `DepartmentUseCases::list_departments` (`src/application/use_cases/department.rs:329–339`, which returns `list_by_company`) "from Root to AnyUnit entry" without filtering.
  - Its consumers include the class (a) Channels workspace desk-settings department picker (`ui_channels.rs:136–140`, commented "The scope is the managed company, so the caller may list every department"), the legacy desk pages (`routes/channel.rs:352/451/509/539/612`) and settings counts (`ui_companies.rs:207`).
  - Today a non-root member sees only their own placements (`visible_departments`); sibling and ancestor department names are not visible to them.
- **Impact.**
  - A lead on the Channels workspace would see every department as a destination for desk assignment, which widens department visibility beyond "managed plus readable".
  - The picker would offer targets the server refuses (desk reassignment requires managing the destination).
  - Item 5 restricts parent options only in the Departments pane.
- **Required correction.**
  - Give non-root callers of department pickers the managed set. Use `managed_units`, or a filtered `list_departments` that returns everything for root managers and the managed subtree for others.
  - Audit every `list_departments` consumer, keeping owner-only counts on the root path.
  - Add list-filter tests for the desk department picker (lead sees subtree only; sibling absent).
- **Instruction lesson.** When an entry gate moves from Root to AnyUnit, every unfiltered company-wide read behind it becomes a visibility decision; trace each consumer.

## 06DA-P02-D1-F5 — "plus what they may read" is narrowed without a recorded decision

- **Actionable / medium / requirements (02A.2) — OPEN.**
- **Evidence.**
  - 02A.2 says: "departments, desks, agents, definitions, schedules and bindings pages show what the viewer manages plus what they may read".
  - D1/r1 item 4 reinterprets this so that configuration lists "do not add readable-only rows, except departments". For example, the desk list (`list_managed_channels`) omits desks the lead can read through desk grants (viewer access) but does not manage.
  - The user decisions recorded so far (a–d / U3–U4) do not cover this.
- **Impact.** A deviation from explicit original text, decided by the author. The narrower reading is defensible: it fails closed, and for agents and definitions "readable" may have no defined meaning. But the gate cannot accept a reinterpretation of an original criterion silently.
- **Required correction.** Either:
  1. implement readable-only rows (rendered read-only) where a read concept exists — at least desks via viewer access, and any others the author identifies; or
  2. obtain and record a user decision adopting the narrower reading, listing per kind what "may read" means or why it is empty.

  Map the decision in the acceptance table.

## 06DA-P02-D1-F6 — the audit writer inventory is incomplete

- **Actionable / low / architecture + verification (02B.1, 02 Outcome) — OPEN.**
- **Evidence.** Production writers of `departments`, `department_members` and `department_roles`, and existing management audit trails, that items 11–12 do not dispose of:
  - Root-member removal through the Departments pane: `DepartmentPersistence::remove_member` dispatches to `department/root.rs::remove_root_member`. This is an owner/admin department membership change, distinct from the team-page owner removal that item 11 excludes.
  - Agent placement by managers: `place_in_department_on` called from `company_agent.rs:105` (managed agent create/edit, B3) and `workflow/provisioning_builder.rs:120` (Builder, attributed to its person requester). Onboarding (`onboarding.rs:252`) can reasonably stay excluded.
  - Create-with-lead (`lifecycle.rs:47`): one `created` row or `created` plus `role_assigned`?
  - Reparent: `update` runs two rechecks (old parent and destination) that can return different roles (e.g. a person leading both B and C). Which acting role is recorded, and does a rename plus move produce one row or two?
  - Existing management audits that already record an actor but no role: `workflow_binding_events` (configured/activated/deactivated with `actor_id`, `init_schema.sql:5175`) and `workflow_definition_events` (copied/saved/archived/published, `:5399`). B2/B3 delegated both kinds of write to leads. 02B.1 says "Management writes that already audit … record the acting role", and the Outcome says "every management write records who did it as which role". Item 12 covers only `binding_audit_events`.
  - The agent and person address syncs that write `binding_audit_events` (`email_binding.rs:179` via `agent.rs:383`, `company_agent.rs:181`) run inside manager agent writes, yet item 12 passes `None` for "agent" paths without saying why.
  - Team-page root admin grant and removal (`set_admin_on`) is a `department_roles` change on the root. Excluding it is plausible under "from 01-departments", but it should be justified against the Outcome.
- **Required correction.** Add a contract-impact table listing every writer above with its disposition: audited (and with which acting role) or excluded (and why). Specify the event cardinality for create-with-lead and reparent. Extend the `administration_audit_` checks to the audited paths.

## 06DA-P02-D1-F7 — the admin invite UI carried from p01 has no disposition

- **Actionable / low / requirements carry-forward (user decision (b), p01 B4-F5) — OPEN.**
- **Evidence.**
  - p01 TASK (B4 R2 correction, F5) carried "exposing [invite forms] to admins and admin delete buttons" to 02A. Today both team UIs are owner-gated: `pages/companies.rs:286/312/536` and `ui_team.rs:298`. The stale comment in `ui_team.rs:581–585` reflects that gating.
  - D1/r1 puts "team/invites" in root class (b) and item 8 says "company-level controls stay owner-only as today … not widened". It never mentions the admin member-invite forms.
- **Impact.** Decision (b) would remain unusable from the UI, and the p01 carry-forward would be dropped silently.
- **Required correction.** Add a 02A item:
  - admins see member-invite create, edit and delete controls (admin role options and admin-addressed invites stay owner-only);
  - inline Forbidden handling;
  - the stale comment fixed;
  - route and page tests.

  Alternatively, record a user decision deferring it.

## 06DA-P02-D1-F8 — the pane rule text and the records are inaccurate

- **Optional suggestion / low / records — OPEN.**
- **Pane rule wording.** Item 5's first sentence groups "create child" with operations that "need management of D's parent". Creating a child under D requires managing D (`department.rs:222–234`: `managed(write.parent_id)`). The next sentence about the New button implies the correct rule; the gate text should state it exactly.
- **Coordination revision.** TASK "Identity and authority" cites "COORDINATION.md r6", but the authority is r7 (adds (c)/(d) = U3/U4, sha256 `2458f32e…a1f6`; reviewer copy `logs/06DA-P02-D1-R1-reviewer-coordination-r7.md`). There is no immutable pin of the decision revision under `logs/`.
- **Suggestion.** Fix the wording, update the revision reference, and pin the coordination and decision copies with the next revision.

---

## Round 06DA-P02-D1-R2 — recheck of D1/r2 (note 06DA-P02-D1-R2-N1)

Common context for this round:
- Date 2026-11-19. Reviewer Claude Opus 5.5 (`claude-opus-5-5`); effort not visible to me (assigned xhigh).
- Decision revision D1/r2: TASK sha256 `173a25c6d6c2fd6e7dd5313838c7c6a961af7e04e690deb71e6bb101c878b4c7`, byte-identical to the pin `logs/D1-r2-TASK.md`.
- Coordination r7, `2458f32e…`, byte-identical to `logs/D1-r2-coordination-r7.md`.
- HEAD `5874dfb1`; no source changes.

### 06DA-P02-D1-F1 — recheck: FIXED (decision level)

r2-5 reuses `department_administration_events` as the single department history. That keeps snapshot labels, the stricter DELETE guard (allowed only once the company row is gone) and one source for the pane.

The schema changes are sound:
- `actor_kind` system|person, with actor fields required for person rows and NULL for system rows;
- per-kind CHECKs tying each kind to its required fields;
- an insert guard split by actor kind, with the system guard unchanged;
- person rows validated against current labels;
- deleted-department labels captured before the delete;
- the pane index.

The existing `UNIQUE(company_id, operation_id, principal_id, department_id, role)` key is compatible: NULL subject and role keep multi-row operations distinct. Departing from the original "new `administration_audit_events`" table name is recorded and justified; the required facts and their append-only property are preserved.

Binding implementation conditions, to be checked in the B2 review:
1. The deferred `check_department_retirement_role_cleared` must be limited to system rows. Otherwise every person `role_assigned` row fails at commit.
2. For an owner removing a person root member, the person `member_removed` row (subject label and kind) must be written before `retire_member_on` retires the principal in the same transaction.

### 06DA-P02-D1-F2 — recheck: FIXED

U5 is authoritative. r2-1 names two rules:
- `CompanyMembership::administers_company()` (owner | root admin) for company-level gates, including a documented `managed_company`;
- the unit predicate for `administered_company` entry and per-target checks.

The false equivalence is withdrawn, root roles still need the owner, and the root-lead, root-admin and owner tests are specified. This is consistent with p01 code: `remove_root_member` and the knowledge `manages_levels` stay membership-based, and root-lead knowledge goes through the predicate.

### 06DA-P02-D1-F3 — recheck: FIXED

r2-2 makes management refusals 404 for members and admins, `owned_company` included. Owner-only *who* is unchanged, and 403 assertions are converted through a listed inventory. The scope wording needs one clarification, recorded as optional F10.

### 06DA-P02-D1-F4 — recheck: FIXED

r2-3: `list_departments` returns the managed set (all units for owner, admin and root lead), so pickers offer only destinations the server accepts. The visible-department reads stay managed ∪ placements, and the picker-destination test is planned.

### 06DA-P02-D1-F5 — recheck: FIXED

U6 is authoritative. r2-4: desks = `list_managed_channels` ∪ the existing `readable_channels` rule (View grant; `channel.rs:152`), with rows tagged managed or readable and readable-only rows read-only. Agents, definitions, schedules and bindings are managed only, and departments are managed subtree ∪ own placements. A readable-row test is planned.

### 06DA-P02-D1-F6 — recheck: FIXED, one note

r2-6 lists every writer I enumerated:
- create (+ `role_assigned`), update, delete, and reparent, recorded with the destination-check role;
- member add and remove, and root-member removal in the pane and on the team page;
- role assign and clear, and team-page `update_member_role`;
- manager agent placement on create, copy and Builder construction.

The single insert sites I verified (`workflow/definitions.rs:46` `event`, `workflow/bindings.rs:88`) gain `acting_role`. Address syncs and provisioning are explicitly NULL, with a reason.

Note for B2: `place_in_department_on` / `prepare_new_agent_placement_on` can carry a role (an agent created as lead, `placement.rs:9–60`). A role-bearing placement should also append `role_assigned` in the same operation, the same as create-with-lead.

### 06DA-P02-D1-F7 — recheck: FIXED, one note

r2-7 puts the admin member-invite UI in B1:
- form with the member role only;
- member invites with delete and level controls;
- admin invites read-only for admins;
- stale comment fixed;
- route tests.

Note: decision (b) also lets admins *update* member invites. Expose the edit control, or record that it is deliberately omitted. Inline handling of Forbidden must also cover `refuse_admin_address` (a member invite addressed to a current admin).

### 06DA-P02-D1-F8 — recheck: FIXED

r2-8 gives the exact pane wording; the coordination r7 and the decision copy are pinned.

## 06DA-P02-D1-F9 — root-membership controls in the pane would offer removals a root lead cannot make

- Optional suggestion / low / UI–server consistency (U5, r2-8). Open; carry into B1.
- Evidence:
  - r2-8 says "members of D need management of D". For D = root, a root lead manages root through the predicate.
  - `remove_root_member` (`department/root.rs:72–103`) still requires owner|admin (`company_not_found` otherwise) and the owner for persons. That is company-level administration under U5.
  - Today's `removable_member` (`pages/company_departments.rs:587–611`) already special-cases root (persons: owner; agents: the editable flag).
  - Root-lead pane flags derived from r2-8 alone would show agent-removal buttons on the root that the server refuses with 404. Adding at root follows the predicate (`require_department_management_on(Some(root))`), so the add control is consistent.
- Suggested disposition: state the root exception in r2-8. Removing a root agent needs `administers_company()`; removing a root person needs the owner. Add a root-lead pane render test.

## 06DA-P02-D1-F10 — the "all management refusals are 404" rule needs an explicit scope

- Optional suggestion / low / requirements clarity (r2-2). Open; carry into B1.
- Evidence: r2-2 says "All management refusals are 404". p01 accepted these payload-level refusals as 403 Forbidden, for actors who may otherwise act on the object, and its tests assert them:
  - `TeamAuthority::require_inviter` / use-case `require_inviter` (admin-role invites);
  - `refuse_admin_address`;
  - `may_assign_level` level ceilings;
  - knowledge `may_write_document` (L6 layer).

  If they are read as in scope, accepted behavior and tests silently change.
- Suggested disposition: restrict the 404 rule to gate refusals, meaning the entry (`administered_company`, `managed_company`, `owned_company`) and per-target management (`require_management` and the persistence rechecks). State that payload and level refusals stay 403, and list them in the B1 test-change inventory as unchanged.

---

## Round 06DA-P02-B1-R1 — batch review of B1 (02A) (note 06DA-P02-B1-R1-N1)

Common context:
- Date 2026-11-19. Reviewer Claude Opus 5.5 (`claude-opus-5-5`); effort not visible to me (assigned xhigh).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan06`, HEAD `5874dfb1908aeb8d7663664ff2e6b2e4631c2c40`; the batch is the working-tree delta (84 paths).
- Artifact `6248bc8cd74f200a4e11f9ba2d00fd37eee979cfc341cb09f363d7e05814897d`, exclusions `phases/p02/docs`, `phases/p01/docs`, `COORDINATION.md`.
- Coordination r8 `3ea7a412…`.
- Gate D1/r2 (PASS) with carried notes F9 and F10; user decisions U3–U7.

## 06DA-P02-B1-F1 — attention's company-administration refusals stay 403, and two authorization test files gained no lead cases

- **Actionable / medium / requirements (02A.1, 02A.4, D1 r2-2 + F10, U4/U5) — OPEN.**
- **Evidence, refusal status.**
  - `src/application/attention.rs:139–143` refuses the TeamWork view, and `:189–194` the operational summary, with `company_forbidden` (403) when the viewer does not `administers_company()`.
  - This is an authority refusal in exactly the sense F10 assigns to 404: "the viewer may not … administer the target or company". It is not a business refusal inside an authorized scope. U5 lists the "attention team view" among company-level administration.
  - It reaches clients through `routes/attention.rs` `list_attention` / `operational_summary`, and `attention_authorization_tests.rs:31–54` still asserts `StatusCode::FORBIDDEN` for a member.
  - The file is unchanged in B1, and the B1 test-change inventory and contract table do not mention it.
- **Evidence, missing lead cases.** 02A.1 requires every `*_authorization_tests.rs` to gain the lead cases, and D1 r1 item 9 promises "leads get 404 on (b)/(c) root routes". Two files have none:
  - `attention_authorization_tests.rs` (unchanged);
  - `workflows/ui/runs/authorization_tests.rs` (unchanged; its only test covers foreign and hidden channels).

  `agent_library_authorization_tests.rs` concerns the operator library and is N/A. The shared fixture gained `delegation()`, but `denial_cases` itself has no lead row, so files that use only `denial_cases` do not exercise leads.
- **Impact.** A lead or member asking for team work or the operational summary is told 403 ("you can see the company but not this"), where every other company-administration refusal now says 404. The (c) runs routes have no regression test proving a lead is refused.
- **Required correction.**
  - Return 404 for both attention authority refusals, keeping business refusals 403.
  - Update `attention_authorization_tests` (403→404) and add lead and sibling-lead cases there and in the runs authorization tests (lead → 404; root admin succeeds).
  - List the changes in the test-change inventory.
- **Instruction lesson.** A cross-cutting status change should be audited by grepping the remaining `company_forbidden(` / `AppError::Forbidden(` call sites and classifying each, not by following the gate helpers only. Candidate scope: the batch checklist for status-convention changes. Evidence: this finding.

## 06DA-P02-B1-F2 — two lead workspaces embed root-only fragments: a "used by" card that fails, and a run-count stream that keeps getting 404

- **Actionable / medium / requirements + UI integrity (02A.1 "a lead of the right unit succeeds", decision 2, U4) — OPEN.**
- **Evidence, agent "used by" card.**
  - The agent edit pane always renders `workflow_agent_usage_card` (`pages/agent_settings.rs:343`): a lazy card fetching `/ui/workflows/agents/{agent}/usage`.
  - That handler (`routes/workflows/ui/authoring/tools.rs:273–300`) gates on `ui.company` = `managed_company` (company administration), with only a company-scope `company_agent` check.
  - So every agent pane a lead opens (now possible through `ui_agents` at unit entry) shows "Where this agent is used could not be loaded.", although decision 2 makes the agent's tool grants and bindings the lead's to manage.
- **Evidence, run-count stream.**
  - The desk workspace (`pages/channel_settings.rs:167`, plus the per-desk `run_counts_slot` at :285) and the agents workspace (`pages/agent_settings.rs:163`) always render `run_counts_bar`: "Open runs" with an SSE source `/ui/run-counts/events?company_id=…`.
  - `ui_run_counts.rs:92` gates the stream with `load_managed_company`, which is root-only per U4.
  - For a lead the stream answers 404, and the htmx SSE extension reconnects repeatedly, so the label stays with no counts and the server is hit again on every reconnect.
- **Not covered by tests.** No test renders the `ui_agents` pane or the `ui_channels` page for a lead. `delegated_admin_routes_unit_leads_and_root_admins_reach_the_agents_page` exercises the legacy `agent.rs` list page, and the rail test renders only the shell.
- **Required correction.**
  - Render the run-count bar and slots only for company administrators, the same way the authoring Runs and Tools links were gated.
  - Either move `agent_usage` to unit entry with a `ManagementTarget::Agent` check (its per-binding `visible_usage` filtering stays), or omit the card for viewers who don't administer the company, with the reason recorded.
  - Add lead page tests for the agent pane and the desk workspace asserting neither root-only fragment is present or failing.
- **Instruction lesson.** When widening a page's entry gate, enumerate the lazy and SSE sub-requests the page embeds (`lazy_card`, `sse-connect`, `hx-get` on load) and classify each route like a top-level one.

## 06DA-P02-B1-F3 — the JSON workflow-bindings list is still root-only

- **Actionable / low / consistency (02A.2, 02A.4, D1 r1 item 4 "bindings already scoped; only their entry gates change") — OPEN.**
- **Evidence.**
  - `routes/workflows/bindings.rs:60–81` `list` authorizes `Inspect` on `RelatedAssociation::Company`, which `company_management_target` maps to `ManagementTarget::Root`, so a unit lead gets 404.
  - The per-item `Visibility::allows` filter after it already restricts rows to associations the caller may Inspect, meaning view plus manage.
  - B1 moved the equivalent JSON definitions list (`definitions.rs:38–58`) to `ListManaged` with a managed filter, and the JSON single-binding read (`bindings.rs:172`) is per-target. A lead can therefore read or configure a binding by id but cannot list their bindings.
- **Required correction.** Authorize the list with `WorkflowOperation::ListManaged` and keep the per-item filter. Add a lead and sibling JSON list test. Record the JSON behaviour change in the contract table.

## 06DA-P02-B1-F4 — legacy and residual surfaces, plus one test that tolerates more than U7 allows

- **Optional suggestion / low / consistency — OPEN.**
- The legacy HTML desk list (`routes/channel.rs` `list_channels_page`) uses `list_managed_channels`, i.e. managed desks only, while U6 defines the desks list as managed plus read-only readable rows. Either align it with `list_workspace_channels` or record that it is a legacy surface and exempt.
- The legacy `/companies/{id}/invites` page (`pages/companies.rs:286/312/536`) still renders invite controls only for the owner. Its HTMX handlers (`routes/company_invite.rs`) put only BadRequest and Conflict inline, so an admin who uses it gets error pages for the decision-(b) business refusals. Exempt it explicitly or align it with the team page.
- `delegated_admin_authoring_lead_edits_a_definition_bound_in_their_unit_and_cannot_bind_outside` accepts either `NotFound` or an Ok page without "Saved binding" for the outside binding. U7 states the refusal stays the stranger's 404. Assert the exact outcome; if the UI renders it inline, say so in the mapping.
- A root lead is not offered "add member" on the root department (`manages_members` uses `administers_company`), although the server's `add_member(root)` follows the predicate. This is conservative and acceptable, but either record it or align the server rule.

### 06DA-P02-B1-F1 — recheck 2026-11-19 / B1-R2 (note 06DA-P02-B1-R2-N1)

**FIXED** at `2c8e4add9aa78a22a356146f998028cbe4b0c0c8e9a35b668c29f4b692eb2f3d`.
- `application/attention.rs` now refuses the team view and the operational summary with `company_not_found` (404) for non-administrators. `company_forbidden` is removed, and nothing calls it.
- The remaining production `AppError::Forbidden` sites are the decision-(b) business refusal (`company_invite.rs:42`) and transport-relay proofs, consistent with F10.
- `attention_authorization_tests`: the member case moved 403→404. The new `delegated_admin_routes_attention_team_views_hide_from_unit_leads` covers the lead's 404 on the team view and summary, the lead's own queue, and the admin and owner summary.
- The runs `reconciliation_and_step_refuse_foreign_and_hidden_channel_identities` adds a unit lead to the denied set.
- All pass in my run.

### 06DA-P02-B1-F2 — recheck 2026-11-19 / B1-R2

**FIXED.**
- **Agent "used by" card.** `agent_usage` now enters with `unit_company` and requires `ManagementTarget::Agent` through `CompanyUseCases::require_management` (`tools.rs:273–300`). The listed bindings are still filtered by `visible_usage`. `delegated_admin_authoring_agent_usage_card_follows_the_agents_departments` covers: a lead on a placed agent OK; the owner on a root agent OK; a lead on a root agent, a sibling, and a member all 404.
- **Run counts.** `run_counts_bar`, the only place that renders `run_counts_source` (the SSE `sse-connect`), is now emitted on the desk page (`channel_settings.rs:168`) and agent page (`agent_settings.rs:164`) only when `administers_company()`.
  - The per-desk `run_counts_slot` is an empty span, and `RUN_COUNTS_SCRIPT` does nothing without a source element, so no lead-facing run-count request remains.
  - Every other `sse-connect` on a lead-reachable page belongs to a team-readable view (mailbox, attention, notifications).
  - The desk page render check asserts no stream for a lead and a stream for the owner. The agent page is covered by the same gate on inspection, not by a test.

### 06DA-P02-B1-F3 — recheck 2026-11-19 / B1-R2

**FIXED.** The JSON bindings `list` and `visible_binding` enter via `managed_listing` (`ListManaged`, AnyUnit), and each binding still needs its association's `Inspect` (view plus unit management). The other callers, checked:
- `board` (`board.rs:221`) authorizes `Inspect` on Company (root) before `visible_binding`. Unchanged.
- The runs UI `scope` (`ui/runs/mod.rs:307`) is reached only from root `ui.company` / `ui.frame` handlers.
- `start_command` with no association resolves it through `visible_binding`. The service then authorizes `Admit` on that association (`service.rs:45–60`): Company → Root, desk → view plus unit management. An explicit association was already admitted the same way, so no authority is added.

The JSON single-binding read gains lead access only through the per-target check.

### 06DA-P02-B1-F4 — recheck 2026-11-19 / B1-R2

**DISPOSITIONED** (optional).
- The legacy desk lists stay managed-only and the legacy invites page stays owner-only. Both are recorded with reasons (the desk workspace and the team page are the U6 and decision-(b) surfaces).
- The U7 assertion is now exact: the outside-desk bind renders the same "not available" message as a nonexistent desk, which is the form-level rendering of the command's 404. A non-saved control is included.
- A root lead not being offered "add member" at root is recorded as conservative.

Accepted.
