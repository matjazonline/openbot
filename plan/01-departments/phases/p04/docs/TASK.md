# p04 UI and documentation — TASK

## Identity

- Plan / phase: `plan/01-departments/`, phase `p04` (document `04-ui-and-docs.md`)
- Workflow: `implement-opus-reviewed-2_2` (skill dir `/Users/mac02/.claude/skills/implement-opus-reviewed-2_2`)
- Record root: `/Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/phases/p04/docs/`
- Root = Author (A): session 68933786, Opus 5.5 / xhigh (user confirmed earlier in the session).
  Reviewer (B): agent `ad409b2d3d149c2c6` (Opus 5.5 / xhigh) if it can be resumed, otherwise a
  fresh Opus 5.5 / xhigh reviewer; recorded on first review.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`.
  Phase start = the merge of `origin/main` (6091c3cd) into 30654e3c (p01–p03 accepted); see
  "Start state" below.
- Snapshot exclusions: `plan/01-departments/phases/p04/docs` `plan/01-departments/COORDINATION.md`
- Authorizations: user 2026-11-10 "pull main and continue" — merge main, continue the plan with
  phase 04 (then 05). Local commit per accepted batch; no push.
- Resources: test DB `openbot_dept_merge_test` (merge checks), `openbot_dept_p04_test` (phase);
  worktree `target/`.

## Start state: merge of origin/main

- `git fetch origin`; branch was 6 ahead / 16 behind; merge base 3752ea35. Incoming: the standalone
  memory service integration (41be07b2 and follow-ups) plus plan archival.
- Conflicts (4), resolved keeping both sides:
  - `migrations/20260817000000_init_schema.sql`: both appended a block at the same anchor;
    departments block, then the memory block.
  - `src/application/workflow/agent.rs`: `mod department` + `mod memory`, both re-exports.
  - `src/application/workflow/agent_context.rs`: both description arms (`LIST_COMPANY_AGENTS`
    provisioning, `Memory { tool }`).
  - `src/adapters/persistence/workflow/agent_history/preparation.rs`: `prepare_on(tx, claim,
    bound, domain, recall, capture)`.
- Semantic fix beyond the markers: main's new `agent_history/recall.rs` calls
  `preparation::candidate_on` three times (recall plan, capture targets, revalidation). Those now
  pass the app domain, so the recall plan's candidate carries the same department context — and
  so the same mandatory tokens for `recall_tokens()` — as the preparation it plans for.
- Auto-merged and reviewed by reading: `frozen.rs` restore binds capture and memory, then the
  saved department, then history (both orders hold: `with_memory` needs no history; department is
  refused only after history); `freeze_on` takes recall/capture.
- Checks: `cargo check --all-targets` (offline) clean, `cargo fmt --check` clean after `cargo fmt`;
  affected DB tests in `logs/merge-main-affected-1.txt` (result recorded under Checks).

## Requirements

- `plan/01-departments/04-ui-and-docs.md` (04A–04D, focused checks); README decisions 2, 6, 7 as
  amended by COORDINATION C001–C004 (plan 04/05 documentation targets).
- p02 optional follow-ups carried here: `01DEP-P02-B2-F2` (remaining "channel" page strings and
  user-facing error texts embedded in desk flashes), `01DEP-P02-B2-F3` ("Continue to desk").
- Instructions: `AGENTS.md`, `src/AGENTS.md`, `src/adapters/http/AGENTS.md`,
  `src/application/AGENTS.md`, `src/adapters/persistence/AGENTS.md`.

## Batches

- **B1** = 04A Departments tab (pane, tree, forms, routes, workspace count) + 04B.1–04B.3 (desk
  form department/members-only, desk lists grouped with a badge, agent settings departments and
  default desk, onboarding) + B2-F2/F3 copy.
- **B2** = 04B.4 run board department filter and "Waiting on a person" + 04C.1–04C.4 (dashboard,
  attention, model usage, `_help`/bounce grouping, JSON fields).
- **B3** = 04C.5 + 04D documentation.

No architecture gate: no schema, transaction, concurrency or authorization-model change; every
write goes through existing use cases. Reads added are bounded and company-scoped.

## Merge checks

| Command | Result | Log |
| --- | --- | --- |
| `cargo check --all-targets` (SQLX_OFFLINE), `cargo fmt --check`, `cargo clippy --locked --offline --all-targets -- -D warnings` | clean | — |
| `cargo test --locked --offline --lib -- department_ desk agent_default_desk_ agent_history_ memory recall provisioning workflow_agent_ workflow_fixtures_ workflow_storage_ sample authoring picker agent_preparation application::workflow::agent_tests channel_access channel_reply --test-threads=4` (TEST_DATABASE_URL `openbot_dept_merge_test`, since dropped) | 849 passed, 0 failed, 10 ignored | `logs/merge-main-affected-1.txt` |

Merge committed as b1026003 (staged resolutions only).

## B1 implementation (04A + 04B.1–3 + B2-F2/F3)

Code facts it builds on: `DepartmentUseCases` (p01) already had every write and the
`visible_departments`/`visible_department` reads; the desk write already took
`ChannelWrite.ownership` (p02); the directory entry already carried departments and the desk
address (p03 B3); onboarding already read back each agent's own desk (p03 B1).

- **Reads** (`DepartmentPersistence`, `PostgresDepartments`): `principals(company, limit)` (people
  and agents, agents first, by current agent name / account username; `MAX_DEPARTMENT_PRINCIPAL_CHOICES`
  = 500), `principals_by_id(company, ids)`, `desks(company, department)` (primary slug). Use cases:
  `department_view` (visible detail + named principals + owned desks), `principal_choices`
  (managers only, member → `NotFound`). Entity: `DepartmentPrincipal`, `DepartmentDesk`,
  `department_tree` (pre-order, siblings by name; a department whose parent is not listed is a root).
- **Departments tab** (`pages/company_departments.rs`, `routes/ui_departments.rs`): `CompanyTab::Departments`,
  `CompanyPaneBody::Departments`; the section `#department-section` holds the tree as nested menus
  and the selected department (name, address, slug, parent, description, charter, lead, members,
  owned desks with links) or the create form. Managers get create/edit/delete, add/remove member,
  assign/change/clear lead; pickers list `principal_choices`; a new department's slug follows its
  name, an edit's does not (the slug is the desk address). Routes under
  `/ui/companies/{company}/departments…`; GETs need read access (a member sees only their own via
  `visible_departments`), every write and the create form first check `managed_company` and turn a
  member's `Forbidden` into the stranger's `404`; past that a use-case refusal renders on the form
  that made it (`DepartmentForm`), success re-renders with a notice and `HX-Push-Url`. The company
  pane mounts the same section (`department_section`) for `?tab=departments[&department_id|&new=1]`.
  `workspace_links` shows a Departments count beside Desks (owner counts).
- **Desk form** (`channel_settings.rs`, `ui_channels.rs`, `ChannelForm`): `department_id` select (tree
  order) and `members_only` box, hidden until a department is picked (`toggleDeskMembersOnly`,
  `data-input="desk-members-only"`); help text per C002/C001. `SubmittedChannel::ownership` asks
  for an ownership write only when it differs from the stored desk (no department lock on a plain
  save), ignores members-only without a department, refuses a non-id value; a form without the
  select (classic page) never touches ownership. Desk workspace loads `list_departments`.
- **Desk lists**: `grouped_by_department` groups the `/ui/channels` sidebar and the classic
  `/companies/{id}/channels` list under department headings in tree order, "No department" last,
  no headings when no department owns a desk; `channel_row_fragment` takes the department name for
  a badge; the classic handlers load `list_departments`.
- **Agent settings**: `AgentDirectoryEntry.desk` is now `AgentDesk { id, address }` (was the address
  string; JSON directory shape documented in `docs/agent-authoring.md`); the edit pane reads
  `agent_directory_entry` and shows a read-only "Desk and departments" card (own desk address +
  settings link, or "was deleted"; departments with role badges linking to the tab); the assistant
  desk card is now "Additional assistant desk" / "Create additional desk"; `docs/custom_tools.md`.
- **Copy (B2-F2/F3)**: "desk" in `ChannelWrite::normalize` errors, `SlugKind` nouns,
  `channel_not_found`, slug-conflict and email-binding conflict texts, the UI `NotFound("Desk not
  found")` sites, the bindings refusal notice, the simulation sender banner, onboarding "Continue
  to your first desk". Kept: `Company or Channel not found` (ingest delivery result), internal
  errors, the provisioning tool's `channel address` (model-facing field names).
- **CSS**: `assets/app.css` regenerated (also fixes drift from p01–p03); `logs/B1-css-check.txt`.

### B1 verification mapping

| Criterion | Test (decisive assertions) |
| --- | --- |
| 04A.1 tab, pane, tree indentation, detail fields | `department_ui_company_pane_has_a_departments_tab_and_count` (tab link, `tab-active`, `from_query`); `department_ui_pane_indents_the_tree_and_shows_the_selected_department` (order, two nested `<ul>` before Inside sales, depth attr, escaping, lit selection + push URL, address, parent, description, charter, lead, members, desk link); `department_ui_tree_orders_each_root_before_its_children` |
| 04A.2 forms, pickers, routes, refusals on the form | `department_ui_owner_writes_render_and_refusals_land_on_their_form` (create + slug from name + notice + push URL + own desk listed; taken slug on create form with typed value; cycle on edit form, parent unchanged; delete of a desk-owning department refused on the delete card; add member / assign / clear lead / remove member; empty choice on members form; delete leaf + push URL); `department_ui_refusal_renders_on_the_form_that_made_it`; `department_ui_reads_name_people_and_agents_and_list_owned_desks` (agents first, people, no external/system, current names, bound, by-id scoping, owned desk, member refused choices, admin allowed) |
| 04A focused: member 404 on every write, sees only theirs | `department_ui_writes_answer_404_to_members_strangers_and_foreign_companies` (8 endpoints × member/stranger/foreign owner, body never names the company, state unchanged); `department_ui_member_sees_only_their_departments`; page `department_ui_member_sees_their_departments_without_forms` |
| 04A.3 Departments count | `department_ui_company_pane_has_a_departments_tab_and_count` (count value, after Desks) |
| 04B.1 department select + members-only + help text | `department_ui_desk_form_offers_the_department_and_members_only`; `department_ui_desk_form_asks_only_for_the_ownership_it_changes` |
| 04B.2 grouped lists + badge | `department_ui_desk_lists_group_by_department_with_a_badge` |
| 04B.3 agent departments/roles, default desk address + link, additional desk, onboarding | `department_ui_agent_settings_show_its_desk_and_departments`; `department_directory_picker_entries_name_departments_and_the_desk` (desk id + address); `desk_wording_flash_messages_say_desk` (additional desk copy); onboarding unchanged: `agent_default_desk_onboarding_completion_reads_back_the_default_desk` (p03) |
| B2-F2/F3 copy | `desk_wording_*`, `bindings_tests` notice assertion, `use_cases::channel` slug-noun tests |

Stand-ins / not proved: the members-only toggle script is asserted present, not run in a browser;
the `/ui/channels` update route is not driven end to end with an ownership change (the derivation
is unit-tested and the write is the p02-tested `ChannelWrite.ownership` path); the agent settings
route's directory read and the classic desks handlers' department lookups are covered by
compilation plus page/read tests, not route tests.

### B1 checks (TEST_DATABASE_URL `openbot_dept_p04_test`; SQLX_OFFLINE=true; 4 threads)

| Command | Result | Log |
| --- | --- | --- |
| `cargo check --all-targets`, `cargo fmt --check`, `cargo clippy --locked --offline --all-targets -- -D warnings` | clean | — |
| affected: `cargo test --locked --offline --lib -- department_ui_ adapters::http::pages adapters::http::routes department_directory_ department_ use_cases::channel thread::reads email_binding desk_wording onboarding agent_default_desk_ --skip cloudroom` (first run) | 656 passed, 2 failed: `desk_wording_flash_messages_say_desk` (intended heading change; test updated) and `rendered_markup_carries_no_inline_javascript` | `logs/B1-affected-1.txt`, `logs/B1-test-inventory.txt` |
| same (rerun) | 658 passed, 1 failed: `rendered_markup_carries_no_inline_javascript` — inline `<script>` at `pages/workflow_authoring/sample.rs:153`, file identical to origin/main and the merge base (last change 48461f66): **pre-existing, unrelated** | `logs/B1-affected-2.txt` |
| focused final: `-- department_ui_ department_directory_ desk_wording agent_default_desk_onboarding` | 30 passed | `logs/B1-focused-final.txt`, `logs/B1-focused-inventory.txt` |
| CSS regenerate + reproduce | matches sources | `logs/B1-css-check.txt` |

`--skip cloudroom`: 28 `cloudroom_*` tests under `routes::local_agents` match the routes filter;
local-agent routes are untouched and their e2e peer build is absent here (memory note). No tests
removed; renamed assertions only (`desk_wording_flash_messages_say_desk`, bindings notice).

## B1 corrections after 01DEP-P04-B1-R1 (all five findings)

- **F1** (Medium): `DepartmentDesk.own` (the desk the `department:{id}` record names, still owned);
  `desks()` marks it and lists it first. The detail header shows that desk's address
  (`#department-address`) or "No department desk."; the desk carries a "Department desk" badge.
  The slug sentence is create-only ("The department's desk is created at this slug"); the edit form
  says a slug edit does not move the desk. Tests: page (desk at `sales-desk`, header asserts it and
  no `sales@`), route (`…owner_writes…`: edit slug to `revenue`, header keeps `sales-team@`), reads
  (`own` true for its desk, false for a second owned desk, order).
- **F2** (Low): members-only help text names "the addresses in Participant Emails"; asserted.
- **F3** (Low): `department_ui_foreign_ids_under_the_owners_path_change_nothing` — company B's
  department id under A's path for show/update/delete/add/remove member/assign/clear lead → 404
  without naming B; B's department as a parent on A's create form → refused on the form; B's person
  in A's department member/lead forms and paths → refused on the form; A's department and B's
  department (name, member, lead) unchanged.
- **F4** (optional, done): `principal_choices` returns `(choices, cut)` reading one past the bound;
  each picker says "Only the first 500 people and agents are listed." when cut; tested.
- **F5** (optional, done): desk links in the tab only for managers (members see name + address);
  deleted-desk text says "Create an additional desk below"; tested.
- Checks: `logs/B1-R2-focused.txt` (31 passed), `logs/B1-R2-affected.txt` (659 passed, 1 pre-existing
  unrelated failure), clippy/fmt clean, CSS matches sources (`logs/B1-css-check.txt` R2 line).
  Artifact `9cad3b579f1e5121427ee277b620bebd9a8dffa3786d299f748844354bb63848`; manifest
  `logs/01DEP-P04-B1-R2.evidence.json` (validated).

## B2 scouting (read-only, during B1-R1)

- Run board filter: `RunFilter` (`application/workflow/reads.rs:251`) gains `department`;
  `push_run_page` (`persistence/workflow/reads.rs`) adds a `channel_id IN (desks of the
  department)` clause (also the agent-version variant); route plumbing in
  `routes/workflows/ui/runs/mod.rs` (`WorkspaceQuery`, `ListQuery`, `ScopeIds`, `Scope`,
  `scope()`, `scope_options()`, `run_filter`) and the JSON `bindings.rs:469` filter; options and
  resolution through `DepartmentUseCases::visible_departments`/`visible_department` (a member sees
  their own; another id is not found).
- "Waiting on a person": attention items (`entities/attention.rs`, `AttentionSourceKind::Decision`
  = parked human decisions; reconciliation/failure/refusal kinds = escalations) carry
  `channel_id`; group by the desk's department.
- Dashboard/attention grouping and filter: `routes/ui_dashboard.rs` (`DashboardSnapshot`),
  `routes/attention.rs` / `pages/attention.rs`.
- Model usage by department: `UsageGrouping` (`services/model_usage/mod.rs:74`) gains
  `Department`; `model_usage_calls.run_id` → `workflow_runs.channel_id` → `channels.department_id`
  (current ownership); `model_usage_history` has no run link → its own "not recorded" group.
- `_help`/bounce: `ChannelDirectoryEntry` (`thread/notices.rs:67`) gains the department;
  `writable_channel_directory` (`ingest/routing.rs:672`) orders by department tree, root first;
  names need a `ChannelPersistence` read (5 implementors incl. 4 mocks).
- JSON: `Channel` already serializes `department_id`/`members_only` (verify in route tests);
  agent responses need `departments` with held roles.

## Status

- Merge: committed b1026003; checks above.
- **B1 accepted in phase**: B1-R1 changes required (F1–F5), corrected; B1-R2 **PASS** for digest
  `9cad3b579f1e5121427ee277b620bebd9a8dffa3786d299f748844354bb63848` (root recomputed; coordination
  r7 unchanged; manifest valid). Committed after b1026003.
- B2: next.
