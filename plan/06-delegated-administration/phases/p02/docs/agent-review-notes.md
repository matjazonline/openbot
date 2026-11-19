# Reviewer notes — 06DA-P02

## 06DA-P02-D1-R1-N1 — architecture gate D1/r1 (02A routes/pages + 01A.2, 02B audit)

### Identity

- Date 2026-11-19; workflow implement-opus-reviewed-2_4; phase p02, stage: architecture gate.
- Records: [TASK](TASK.md) "## 06DA-P02-D1/r1", [REVIEW](REVIEW.md), [findings](agent-review-findings.md). The p01 records were not touched.
- Reviewer: Claude Opus 5.5 (`claude-opus-5-5`); effort is not visible to me (assigned xhigh).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan06`, branch `plan06`. HEAD `5874dfb1908aeb8d7663664ff2e6b2e4631c2c40`. `git log -3`: 5874dfb1 is a learning-only commit (34 lines, `.trail/learnings`) on top of 2352dc38 (p01 accepted).
- Source is clean: there are no p02 source edits. Code-identity digest `f720ea29105c22221f52270c15b8877e3833ce593aa1833099d60ee39bb79bbb`, with exclusions `plan/06-delegated-administration/phases/p02/docs` and `plan/06-delegated-administration/COORDINATION.md`. It also covers the untracked p01 tars.
- Decision revision D1/r1: TASK.md sha256 `7a6b8015507f4c47b4fa35186af6c7d7cc7120f8471fc58cbe88b5f6e11070d3`, reviewer copy `logs/06DA-P02-D1-R1-reviewer-task-r1.md`.

### Coordination

- The authority is **r7**, not the r6 named in the assignment and in TASK. It is uncommitted, sha256 `2458f32e72b93ed5ac685c2c29ff43dfb4cf24079fa5451a0a7fd5f12dc2a1f6`; reviewer copy `logs/06DA-P02-D1-R1-reviewer-coordination-r7.md`.
- r7 adds user decisions (c) and (d), matching U3 and U4; there are no notices. I applied (a)–(d) and the inherited Doop deferral as criteria.
- Freshness: TASK and coordination hashes were rechecked unchanged before returning.

### Coverage

The decision was checked against README decisions 1–6 and "What does not change", 02A.1–4, 02B.1–2, the 02 focused checks and 01A.2. I verified the inventory against code where a decision depends on it.

- **Gates.** `readable_company`, `owned_company` (403) and `managed_company` (403) in `company.rs`. The company settings page is readable (`load_readable_company`), so item 8 holds. Departments-pane forms are owner/admin-only (`editable`). `ui_team` is owner-gated.
- **Predicate.** `unit_management_role` SQL, the AnyUnit candidate query, and `management_role_on` arms. Root role assignment rules (`root_role_authority_on`, `guard_root_roles`) show a person lead on root is possible (F2).
- **Department use cases.** create, update and reparent, delete, assign/clear role (root needs the owner), add/remove member. I compared these with the item 5 pane rules (F8 wording). `visible_departments`, `visible_department`, `desk_departments` and `principal_choices` are as the inventory describes. `list_departments` consumers matter (F4). Team members are readable by every member, so AnyUnit `principal_choices` exposes nothing new.
- **01A.2.** Every production `manages_company_operations` caller matches item 7's list (company_departments, mailbox rail and local-agent link, attention ×3, ui_departments, department/root.rs, thread_activity, company.rs ×2, department.rs ×3, mcp, access_level). Two test usages need renaming.
- **U3.** `CompanyModelConnection` carries `has_api_key` only, with no credential, so reading it is safe. The agent pane reads model connections, skills (`list_company_page`, `get_company`), library agents and the prompt generator.
- **Rail.** `rail_section_visible` matches item 6.
- **Audit.**
  - `binding_audit_events` holds integration and channel-binding endpoint events. Its writers are desk create/update via `write_canonical_email_binding` and agent/person address syncs. `IntegrationUseCases::link_binding` has no HTTP caller.
  - `reject_binding_audit_rewrite` is depth-based.
  - The **existing `department_administration_events`** table and its guards were missed (F1).
  - `workflow_binding_events` and `workflow_definition_events` already record `actor_id` for management writes delegated in B2/B3 (F6).
- **Department-table writers.** I enumerated them (F6): `department.rs` entries, `lifecycle.rs`, `department/root.rs::remove_root_member`, `place_in_department_on` from company_agent, provisioning_builder and onboarding, and `company/root.rs` join/admin.
- **Carry-forwards.** p01 HANDOFF/TASK carried 01A.2 and the admin invite UI to 02A (F7).

Inspection only: no tests were run. Architecture review does not need behavioral execution, and there is no p02 source.

### Assumptions challenged

- 404 versus 403 for owner-only routes (F3).
- Root-manager equivalence (F2).
- AnyUnit entry exposing company-wide reads (F4).
- "Plus what they may read" (F5).
- Whether a new audit table is warranted (F1).
- Completeness of the acting-role threading (F6).

### Accepted parts of D1/r1

- `administered_company` as an entry gate only, with per-target checks unchanged.
- `managed_company` refusals becoming 404.
- `any_unit_management_role` as the single SQL decision, with `administers_units` as a rendering hint only.
- The class (a)/(b)/(c) split, apart from F2/F3, and U4 for runs/deliveries/dashboard.
- Scoped desk, department and definition lists, apart from F4/F5.
- Per-department pane flags derived from the managed set, with server refusals unchanged.
- The rail.
- The 01A.2 removal, apart from F2's equivalence.
- The audit insert inside the existing serialized write, the competing role-removal test, the company cascade, no FK on principals or target, and fresh schema.
- The verification plan is otherwise adequate. It needs additions for F1, F2, F4 and F6, and the root-lead fixture rows.

### Outcome

**CHANGES REQUIRED for D1/r1.**
- Actionable: F1 (medium), F2 (medium), F3 (medium), F4 (medium), F5 (medium), F6 (low), F7 (low).
- Optional: F8 (low).
- No dependent p02 edits should start until a corrected immutable D1/r2 has an explicit PASS.
- A recheck should cover the F1–F7 decision corrections and the user decisions recorded for F2, F3 or F5 where chosen.

### Process feedback

- The gate was necessary, and its size was reviewable. F1 is the costliest kind of miss for a schema gate.
- The read-only helper inventory was accurate for gates and lists. It searched writers and gates, but not existing schema objects in the same domain (F1) or alternative role holders (F2).
- Candidate (architecture gates adding schema): "Enumerate existing tables, triggers and functions in the same domain and dispose each." Evidence: F1.
- Candidate (authorization redesigns): "Test the predicate's boundary holders (root lead, agent lead) against every membership-based gate the redesign keeps." Evidence: F2 and the B4-R1 root-lead observation.

## 06DA-P02-D1-R2-N1 — architecture gate recheck of D1/r2

### Identity

- 2026-11-19; implement-opus-reviewed-2_4; p02 architecture-gate recheck.
- Records: [TASK](TASK.md) "## D1/r1 review result and user decisions U5/U6" and "## 06DA-P02-D1/r2", [REVIEW](REVIEW.md), [findings](agent-review-findings.md).
- Reviewer: Claude Opus 5.5 (`claude-opus-5-5`); effort not visible to me (assigned xhigh).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan06`, branch `plan06`, HEAD `5874dfb1908aeb8d7663664ff2e6b2e4631c2c40`, with no source changes. `git status`: only `COORDINATION.md` and the p02 records changed, plus the untracked p01 tars.
- Decision revision D1/r2: TASK sha256 `173a25c6…b4c7`, byte-identical to the pin `logs/D1-r2-TASK.md`. Diffed against my R1 copy, the only change is the appended r2 and U5/U6 sections (r1 text unchanged).
- Coordination r7, `2458f32e…`, byte-identical to `logs/D1-r2-coordination-r7.md`; no notices.
- TASK, coordination and HEAD were rechecked unchanged before return.

### Inputs

- U5 and U6 (authoritative, recorded in TASK).
- The R1 findings F1–F8.
- Code checked again where r2 depends on it:
  - `department_administration_events` and its guards (`init_schema.sql:17174–17260`);
  - `remove_root_member` (`department/root.rs`);
  - `removable_member` (`pages/company_departments.rs`);
  - `readable_channels` / `list_managed_readable_channels` (`use_cases/channel.rs:152, 261`);
  - the single event insert sites (`workflow/definitions.rs:46`, `workflow/bindings.rs:88`);
  - the agent placement writers (`department/placement.rs`, `company_agent.rs:105`).

### Coverage

Each finding was rechecked against the original criteria and the user decisions (details in the findings file):
- **F1 FIXED.** Reuse of the existing history. Compatible with its UNIQUE key, system guard and immutability. Two binding implementation conditions: the deferred role-cleared check is limited to system rows, and the ordering of person root-member removal rows.
- **F2 FIXED** under U5, with two named rules.
- **F3 FIXED.** Scope wording is optional F10.
- **F4 FIXED.**
- **F5 FIXED** under U6.
- **F6 FIXED.** Note: an agent placement that carries a role also needs `role_assigned`.
- **F7 FIXED.** Note: the member-invite edit control, and inline handling of the admin-address refusal.
- **F8 FIXED.**

I looked for new problems introduced by r2 and found two optional ones:
- **F9:** the root-lead pane shows root-member removal the server refuses.
- **F10:** the scope of "all refusals 404" versus the p01-accepted 403 payload and level refusals.

Neither is costly to unwind, and both are UI or text alignment that the B1 review will verify. The verification plan additions (two-rule tests, the 403→404 inventory, the picker-destination test, the readable-desk row test, system-row regressions, per-writer acting-role rows, definition and binding event role) cover the changed decisions. No tests were run; this is a decision review and there is no p02 source.

### Outcome

**PASS for decision revision D1/r2** (TASK `173a25c6…b4c7`, coordination r7 `2458f32e…`, HEAD `5874dfb1`).
- F1–F8 resolved at decision level.
- Optional F9 and F10, plus the F1, F6 and F7 notes, are to be carried into B1 and B2 and checked in their batch reviews.
- This accepts the architecture and verification plan only. No implementation is accepted, and no execution of the promised tests is implied.

### Process feedback

- Pinning the decision and coordination copies, and recording the user decisions verbatim, made the recheck cheap and unambiguous.
- U5 and U6 resolved the two requirement conflicts with no residual interpretation.
- No new instruction candidates beyond R1's.

## 06DA-P02-B1-R1-N1 — batch review of B1 (02A routes, pages, lists, rail, 404, 01A.2)

### Identity

- 2026-11-19; implement-opus-reviewed-2_4; p02/B1 whole-batch review.
- Records: [TASK](TASK.md) "## Batch B1 (02A)" to the end, [REVIEW](REVIEW.md), [findings](agent-review-findings.md).
- Reviewer: Claude Opus 5.5 (`claude-opus-5-5`). The user asked for Fable 5.2, which is unavailable, and kept this reviewer (r8 (f)). Effort is not visible to me (assigned xhigh).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan06`, branch `plan06`. HEAD `5874dfb1908aeb8d7663664ff2e6b2e4631c2c40`; the batch is the uncommitted delta (84 paths).
- Artifact `6248bc8cd74f200a4e11f9ba2d00fd37eee979cfc341cb09f363d7e05814897d`, exclusions exactly `plan/06-delegated-administration/phases/p02/docs`, `plan/06-delegated-administration/phases/p01/docs`, `plan/06-delegated-administration/COORDINATION.md`.
- My snapshot reproduced the digest and the freeze file list (`logs/B1-author-freeze.snapshot.txt`) exactly, before review and again after my test run.
- Gated decision: D1/r2 (PASS) with its carried notes. User decisions U3–U7.

### Coordination

- r8 is byte-identical to `logs/B1-coordination-r8.md` (`3ea7a412…`).
- Against r7 it adds only: the revision number, (e) U7 (binding outside the managed units stays 404, no new code), (f) the reviewer choice, and a status row. No notices.
- Rechecked unchanged at return.

### Coverage

I read every production hunk in the 84-path delta and the new and changed tests. Areas checked:
- **Domain and SQL.**
  - `administers_company()` rename (owner|admin), and `grep manages_company_operations` is empty.
  - `any_unit_management_role` holds exactly the former AnyUnit query.
  - `ACCESSIBLE_COMPANY_COLUMNS` binds `$1` = user in both queries.
- **Entry gates.** `administered_company` (readable plus AnyUnit, otherwise 404), the 404 refusals of `managed_company` and `owned_company`, the `level_manager` 404, the onboarding persistence 404, and the switchers via `list_administered_companies` / `load_administered_company`.
- **Lists.**
  - `managed_units` port (two real implementations plus doubles).
  - `list_managed_channels` / `list_workspace_channels`: managed desks, then readable desks via the existing `readable_channels` rule, read-only rows linking to the mailbox, and an "Other desks" fallback for owners not in the list.
  - `list_departments` returns the managed set; `visible_*` and `desk_departments` show placements ∪ managed.
  - `principal_choices` at AnyUnit (the team is readable by members anyway).
  - `list_definitions(managed_by)` for the library and the JSON list; runs keep `None`.
- **By-id reads after unit entry.**
  - Every authoring route reads its definition through `Ui::detail`, which authorizes `ManageDefinition`: agent_picker, bindings pane and form, editor ×4, sample ×2, versions/compare, tools register. The retry path in `create_draft` reaches `created` → `detail`.
  - Bindings use `visible_binding` / `Visibility` (`Inspect` = view plus unit management).
  - Desk by id uses `get_company_channel` → `verify_unit_manager`; agent by id uses `ManagementTarget::Agent`; schedules use `ManageSchedule`.
  - The versions tool form, the Runs and Tools links, the runs, events and controls, and the tools pane stay `ui.company` (root).
- **Workspaces.** ui_channels, ui_agents (model choices, skills and prompt at AnyUnit under U3), ui_schedules, workflows authoring and bindings, ui_departments writes, and legacy channel.rs/agent.rs, are all at unit entry. Every remaining `managed_company` / `owned_company` / `load_managed_company` HTTP caller is a (b) or (c) surface.
- **Departments pane** (F9 included): `manages_placement`, `assigns_roles` (root → owner), `manages_members` (root → `administers_company`; root persons → owner via `removable_member`), New button, parent options managed-only, root option via `manages_root`, desk links. I compared each against `DepartmentUseCases` create/update/delete/assign/clear/add/remove and the persistence rechecks.
- **Rail.** `administers_units` drives Channels, Agents, Workflows and Schedules; Runs, Deliveries and Dashboard use `administers_company`; every page builder was checked.
- **Team UI (decision b).**
  - Admin sees the invite form with the member role only.
  - On member invites, an admin gets edit with no role field (stored role kept), withdraw, and level; admin invites are read-only for admins.
  - `require_inviter` refuses with 404; business Forbidden refusals render inline (F10). Owner-only role and removal paths return 404 through `owned_company`.
- **F10.** Business refusals keep 403 (`require_inviter`, `refuse_admin_address`, level ceilings, knowledge level).
- **Test changes.** Discovery 4890→4902: +12, the two renames listed, nothing else removed. Every removed assertion line in the diff maps to an inventory entry, with none dropped.

### Checks

- **Executed by me:** `logs/06DA-P02-B1-R1-reviewer-focused-r2.txt`, **215 PASS / 0 FAIL / 0 ignored**. It covers all 12 new tests, both renamed tests, every changed family in the inventory, the attention authorization tests (still asserting 403), and the binding and definition authority families.
- **Void run:** my first run (`…-focused.txt`) matched 0 tests because zsh did not split the filter variable. It is retained and marked NOT EXERCISED.
- **Manifests:** mine (`logs/06DA-P02-B1-R1-reviewer.evidence.json`) validates 1 PASS / 1 NOT EXERCISED. The author's validates 8 PASS / 2 FAIL. The full lib run `B1-full-r2.txt` (4869/3/30) postdates the last source edit (12:41:52). Its failures are the known C-locale auth pair and the deferred Doop prepared-version assertion; cloudroom passed. The exploratory FAIL run is retained.
- **Inspected only:** fmt, whitespace, clippy, sqlx check, source_modules, grep logs.

### Not covered or reviewed lightly

- Visual and HTML behaviour of the lead's `ui_agents` pane and desk page beyond code reading (F2).
- Performance of the per-row `unit_management_role` scans used by `managed_units`, `list_definitions(managed_by)` and `ACCESSIBLE_COMPANY_COLUMNS`. There is no evidence either way; it is bounded by department count, as in the accepted B3 lists.

### Outcome

**CHANGES REQUIRED.**
- Actionable: [06DA-P02-B1-F1](agent-review-findings.md) (medium), [B1-F2](agent-review-findings.md) (medium), [B1-F3](agent-review-findings.md) (low).
- Optional: [B1-F4](agent-review-findings.md).
- Accepted as reviewed: the gates, the list scoping, the pane rules, the rail, the team UI, the inventory and 01A.2.
- A recheck should cover the F1–F3 delta and the new lead page tests. Unaffected evidence can be reused while the source stays unchanged.

### Process feedback

- The batch was large but reviewable in one pass. Shared helpers (`Ui::detail`, `administered_company`) made the by-id audit tractable.
- Two misses share a pattern: the HTTP audit followed top-level handlers and gate helpers but not (i) refusal helpers outside them (`company_forbidden` in attention) or (ii) lazy and SSE sub-requests embedded in pages. Candidate instructions are in F1 and F2.
- Reviewer lesson: under zsh, pass test filters literally or with `${=var}`, and confirm the "running N tests" line. This is the same zero-match pitfall as p01 B4-F6.

## 06DA-P02-B1-R2-N1 — B1 correction recheck (F1–F4)

### Identity

- 2026-11-19; implement-opus-reviewed-2_4; p02/B1 correction recheck.
- Records: [TASK](TASK.md) "### B1 R1 findings and correction (R2)", [REVIEW](REVIEW.md), [findings](agent-review-findings.md).
- Reviewer: Claude Opus 5.5 (`claude-opus-5-5`); effort not visible to me (assigned xhigh).
- HEAD `5874dfb1` (unchanged). Artifact `2c8e4add9aa78a22a356146f998028cbe4b0c0c8e9a35b668c29f4b692eb2f3d`, same three exclusions.
- My snapshot reproduces the digest and `logs/B1-R2-author-freeze.snapshot.txt`. The file list grows by exactly the 5 paths the corrections touch: agent_settings.rs, attention_authorization_tests.rs, workflows/bindings.rs, authoring/tools.rs, runs/authorization_tests.rs.
- Coordination r8 is byte-identical to `logs/B1-coordination-r8.md`.
- Rechecked at return.

### Coverage

- Diffed every correction hunk: attention, company.rs (`require_management`, `company_forbidden` removed), tools.rs `agent_usage`, both page templates, workflows/bindings.rs `managed_listing`, and the four test files.
- Traced every `visible_binding` caller (board, runs UI scope, `start_command` in JSON and UI controls, authoring) and the start service `Admit` authorization.
- Enumerated every `sse-connect` / `run_counts_source` emitter and the run-count client script.
- Re-grepped all production `Forbidden` refusals against the F10 scope.
- Discovery: R1→R2 adds 2 tests and removes none.

### Checks

- **Executed by me:** `logs/06DA-P02-B1-R2-reviewer-focused.txt`, filters `attention routes::workflows delegated_admin_ pages::tests ui_team company::tests`, ran 400 tests: **396 PASS / 0 FAIL / 4 ignored**. Every new and changed test is included. My manifest validates (1 PASS).
- **Author evidence:** manifest 8 PASS; focused 491 PASS / 6 ignored; dashboard / operational-summary / department views 66 PASS; fmt, clippy, sqlx and source_modules logs inspected.
- **No full-suite rerun for R2** by either party. R1's full run predates these small, localized edits. The final repository gate in p03 remains required.
- **Reused:** R1's unaffected evidence and conclusions; the code paths outside the five changed files are byte-identical by construction.

### Outcome

**PASS for B1 (02A) at `2c8e4add9aa78a22a356146f998028cbe4b0c0c8e9a35b668c29f4b692eb2f3d`.**
- F1, F2 and F3 FIXED; F4 dispositioned.
- No new findings.
- Batch acceptance only. 02B audit, p03 docs and the final full gate are outstanding. The known unrelated failures (C-locale auth pair, deferred Doop assertion) remain recorded.

### Process feedback

- The correction was tight, and each fix came with a discriminating test.
- The author's caller analysis for `visible_binding` was accurate and saved verification time.
- No new instruction candidates.
