# p04 agent review findings

Shared findings log for phase `p04` of `plan/01-departments/` (workflow `implement-opus-reviewed-2_2`).
Records: `TASK.md`, `REVIEW.md`, `agent-review-notes.md` (this directory).

---

# Batch B1 review — round `01DEP-P04-B1-R1`

Common context for the B1 findings below:

- Date 2026-11-10; batch B1 (04A Departments tab + 04B.1–3 desk form, desk lists and agent
  settings + p02 B2-F2/F3 copy); stage: batch review; round `01DEP-P04-B1-R1`. Includes a check of
  the unreviewed merge `b1026003`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`;
  phase start, review base and HEAD are all `b1026003`; B1 is uncommitted. Artifact digest
  `8e8f63fe8cb3a161ee7af33529f5a432ab49359a7515dd403960d564fa3c1840` (exclusions
  `plan/01-departments/phases/p04/docs`, `plan/01-departments/COORDINATION.md`).
- Coordination revision 7 (`logs/coordination-r7.md`, sha256 `3b556f5a…5414`): I001 r1, I002 r2,
  I003 r2, C001 r1, C002 r1.

---

## 01DEP-P04-B1-F1 — The department pane shows a desk address that a slug edit makes wrong

- **Classification:** actionable defect · **Medium** · correctness (user-visible). 04A.1, with p03
  D8.
- **Evidence:**
  - The detail header builds the department's address from its slug:
    `format!("{}@{}.{}", department.slug, company.slug, domain)` (`pages/company_departments.rs:385-392`).
  - The create and edit forms share `department_fields`. Both say "The slug is also the address of
    the department's desk: slug@…" (`:276`).
  - But a department slug edit never moves its desk. The desk is the channel the
    `department:{id}` record names (p03 gate r4 D8: "A department slug or name edit never renames
    or loses the desk"). `update_department_on` only updates `departments`, and p03's
    `department_lead_desk_survives_a_rename_and_is_left_once_detached` asserts the desk keeps its
    address.
  - So after a slug edit, the header advertises `newslug@company.domain`. No desk answers there:
    mail to it is not routed to the department, or reaches whichever desk later takes that slug.
  - The edit form's help text tells an admin that the slug they are changing is the desk's
    address.
  - The same holds once the department desk is detached, deleted or renamed through desk
    settings.
  - The member test asserts only `shown.contains("mine@")`, so this goes unnoticed.
- **Required:**
  - Show the department desk's current address: the `department:{id}` record's channel at its
    primary slug, while the department owns it. When there is none, say so ("No department desk").
    Do not build an address from the slug.
  - Keep the "slug becomes the desk address" sentence on the create form only. On the edit form,
    say that changing the slug does not move the department's desk.
  - Add a test: edit the slug, and assert the header shows the desk's unchanged address.
- **Resolution history:** 2026-11-10 open (B1-R1).
  2026-11-10 resolved (B1-R2, digest `9cad3b57…3848`): `DepartmentDesk.own` is set from the `department:{id}` record joined to a desk the department still owns (`persistence/department.rs` `desks`, the same join as `department_roster.rs`). The header shows that desk's address, or "No department desk.". The slug sentence is on the create form only, and the edit form says a slug edit does not move the desk. Tests: the route slug edit `sales-team` → `revenue` keeps `sales-team@` in `#department-address`; page and reads tests cover it too.

---

## 01DEP-P04-B1-F2 — The members-only help text says everyone else is refused, but the desk's Participant Emails still get in

- **Classification:** actionable defect · **Low** · requirements (04B.1: "the checkbox's help text
  says who gets access").
- **Evidence:**
  - The help text says: "Only the department's members, its lead, the lead of its parent department
    and the leads of its direct child departments can use this desk. Everyone else in the company,
    and anyone outside it, is refused." (`pages/channel_settings.rs:485-493`)
  - A members-only desk keeps its configured allowlist. `set_desk_ownership_on` keeps the
    `ConfiguredAllowlist` grants and derives the mode from them (`department_desks.rs:350-390`).
    `stored_mode` refuses only `@public` (`:53-73`). The demotion trigger also keeps allowlist
    entries on purpose (schema `department_principal_demoted`).
  - The same form still offers the Participant Emails field. So an address listed there, even one
    outside the company, is admitted, and the text says otherwise.
  - The list of department roles follows C002 correctly, and C001's trusted relays are reflected
    by "Other desks in the company can still hand work to it".
- **Required:** say that the addresses in the desk's Participant Emails are admitted too, and that
  `@public` cannot be listed. For example: "Only the department's members, … and the addresses in
  its Participant Emails can use this desk; everyone else is refused." Assert the wording.
- **Resolution history:** 2026-11-10 open (B1-R1).
  2026-11-10 resolved (B1-R2): the help text names "the addresses in Participant Emails" (`channel_settings.rs:490`), asserted at `department_ui_tests.rs:478`. The help text does not mention `@public`, which I had suggested. That is acceptable: `stored_mode` refuses it on the form with "A members-only desk cannot be open to @public."

---

## 01DEP-P04-B1-F3 — No test tries another company's department or principal id under the caller's own company

- **Classification:** actionable defect · **Low** · verification (`src/adapters/http/AGENTS.md`:
  "Tests must attempt a valid id belonging to another tenant for every new detail, nested,
  download, and mutation route").
- **Evidence:**
  - `department_ui_writes_answer_404_to_members_strangers_and_foreign_companies` covers callers
    without access: a member, a stranger, and an owner addressing another company's path.
  - No test has the owner of company A send company B's valid department id under A's path, to
    `show_department`, `update_department`, `delete_department`, `add_member`, `remove_member`,
    `assign_lead` or `clear_lead`.
  - No test sends B's valid principal id in A's member and lead forms, or in the
    `members/{principal_id}` and `lead/{principal_id}` paths.
  - The use cases scope by company: `visible_department`; `check_principal_on` → "Principal not
    found in this company". But the routes, and how those refusals render (404 against an on-form
    refusal with B unchanged), are untested.
- **Required:** a route test with a second company that owns a department and a principal.
  Assert that each route answers 404, or an on-form refusal for principal ids, and that B's rows
  are unchanged.
- **Resolution history:** 2026-11-10 open (B1-R1).
  2026-11-10 resolved (B1-R2): `department_ui_foreign_ids_under_the_owners_path_change_nothing` covers B's department id under A's path on all 7 routes → 404, without naming B. B as a parent is refused on the form. B's person in A's member and lead forms and paths is refused on the form. A and B are unchanged. Passed in my run.

---

## 01DEP-P04-B1-F4 — The people-and-agents pickers stop at 500 without saying so

- **Classification:** optional suggestion · Low · UX.
- **Evidence:** `principals(company, MAX_DEPARTMENT_PRINCIPAL_CHOICES = 500)` lists agents first
  (`ORDER BY principal.kind`; `persistence/department.rs` `principals`). In a company with more than
  500 people and agents, the remaining people never appear in the member or lead pickers, and
  the form gives no hint.
- **Suggestion:** fetch `limit + 1`, and when there are more, show a note such as "Showing the first
  500 people and agents".
- **Resolution history:** 2026-11-10 open (optional).
  2026-11-10 resolved (B1-R2): `principal_choices` reads 501 and returns `(choices, cut)`. Each picker shows "Only the first 500 people and agents are listed." when the list is cut (page test). The `limit + 1` cut is not exercised with more than 500 principals, but it is trivial.

---

## 01DEP-P04-B1-F5 — Desk links a member cannot open, and a copy mismatch

- **Classification:** optional suggestion · Low · UX copy.
- **Evidence:**
  - The Departments tab's "Desks" card links each desk to `/ui/channels?…&channel_id=…`
    (`pages/company_departments.rs` `desks_card`) for every viewer. For a member who is not an
    admin, that page is managed-only (`ui_channels.rs` `managed_company`), so the link answers 404.
  - The agent settings card's deleted-desk text says "Create an assistant desk below", but the
    button below now reads "Create additional desk" (`pages/agent_settings.rs`).
- **Suggestion:** render desk names as text, or link to the desk's threads, for viewers who are not
  editors. Align the deleted-desk copy with the button.
- **Resolution history:** 2026-11-10 open (optional).
  2026-11-10 resolved (B1-R2): only managers get desk links; members see the name and address (`department_ui_tests.rs:294`). The deleted-desk text now reads "Create an additional desk below…" (asserted).

---

# Batch B2 review — round `01DEP-P04-B2-R1`

Common context for the B2 findings below:

- Date 2026-11-10. Batch B2: 04B.4 (run board department filter and "Waiting on a person") and
  04C.1–4 (dashboard and attention, model usage, `_help`/bounce, JSON). Stage: batch review.
  Round `01DEP-P04-B2-R1`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), agent `ad409b2d3d149c2c6`.
- Phase start `b1026003`. Review base and HEAD are `399efe2c` (B1, accepted); B2 is uncommitted.
- Artifact digest `e167c54ade2bb78b74c80524b9afc479b7b16b869c0a50c09b53c8ab4048171d`
  (exclusions `plan/01-departments/phases/p04/docs`, `plan/01-departments/COORDINATION.md`).
- Coordination r7, sha256 `3b556f5a…5414`: I001 r1, I002 r2, I003 r2.

---

## 01DEP-P04-B2-F1 — A plain member sees and can filter by every department, against the accepted rule that a member sees only their own

- **Classification:** actionable defect · **Medium** · authorization/visibility (a change to an
  accepted decision).
- **Evidence:**
  - p01 decision r2 (F4) (`phases/p01/docs/TASK.md:173-177`, plan 01 item 3) says:
    `visible_departments` "need only company membership … a plain member sees departments where
    their `person` principal is a member or holds a role … A department the member cannot see is
    `NotFound`". B1 follows it on the Departments tab.
  - B2 adds `DepartmentUseCases::desk_departments` (`use_cases/department.rs:351`). It only needs
    `readable_company` and returns **every** department of the company
    (`list_by_company` + `desk_owners`).
  - `/ui/work` and `/ui/work/list` (`load_readable_company`, open to plain members for My work
    and Unassigned) pass that list to `department_filter` (`pages/attention.rs:36`). So a member's
    select lists every department name, indented to show the whole tree.
  - `known_department` (`routes/attention.rs:276-286`) accepts any of them, and so does the JSON
    `GET /companies/{id}/attention?department_id=` route. A department the member cannot see
    answers 200, not `NotFound`.
  - TASK records this as a new rule ("department *names* are company-visible"). The B2 scouting
    note had planned to go through `visible_departments`. No coordination entry or gate revision
    records the change, and the assignment says B2 changes no authorization model.
  - Plan 04's outcome ("everyone sees which department a desk belongs to") covers naming the
    department of a desk the member can see. It does not cover listing every department.
  - The other users of `desk_departments` are manager-only, so they are fine: the runs workspace
    (`load_managed_company`), the company dashboard (managed companies; an operator outside the
    company gets none), agent JSON (`verify_company_manager`) and usage (manager recheck).
- **Required:**
  - For a viewer who is not an owner or admin, offer and accept only:
    - the departments `visible_departments` returns;
    - the departments that own a desk the viewer can read, so that the departments of desks on
      their own items can still be named.
  - Answer `NotFound` for any other department id, on both the page and the JSON route.
  - Alternatively, bring a revision of p01 D4 r2 through coordination before keeping the
    company-wide list.
  - Add a member test: a department the member is not in, and that owns no desk they can read, is
    absent from the select and answers 404 as a filter.
- **Resolution history:** 2026-11-10 open (B2-R1).
  2026-11-10 resolved (B2-R2, digest `a97fdbeb…0c58ba`), in code under p01 D4 r2 with no coordination change. `DepartmentUseCases::desk_departments(user, company, readable_desks)` (`use_cases/department.rs:353`) gives owners and admins every department; anyone else gets `list_for_user` (the same rule as `visible_departments`) plus the owners of the readable desks they were given, and an owner map cut to those desks. The three attention routes (`routes/attention.rs:104`, `:218`, `:266`) list first, then pass `AttentionListing.readable_desks`, which is `read_context`'s `visible_channel_ids` taken before any department narrowing (`application/attention.rs:177`), so the select stays whole while a department is chosen. An unseen department narrows the read to nothing (only readable desks it owns) and then answers 404 through `known_department`. The other callers (company dashboard, runs workspace) pass `&[]` and are reached only by owners and admins (`list_managed_companies`/`load_managed_company`). Test `department_views_a_member_filters_only_by_departments_they_may_see` (`attention_authorization_tests.rs:251`): `hidden` is absent from the member's departments, named once its desk is readable, and 404 on the JSON route once the desk is made members-only; passed in my run. It does not assert the 404 body; by inspection, `known_department` is the only `NotFound` left on that path.

---

## 01DEP-P04-B2-F2 — The dashboard's department filter narrows the company's first 20 jobs, so it can report "nothing outstanding" for a busy department

- **Classification:** actionable defect · **Medium** · correctness (04C.1 "with a department
  filter").
- **Evidence:**
  - The outstanding query is capped company-wide before any department narrowing:
    `outstanding(company)` binds `OUTSTANDING_LIMIT` = 20 (`persistence/dashboard.rs:628-633`,
    `entities/dashboard.rs:263`).
  - `outstanding_panel` (`pages/dashboard.rs:307-375`) filters those 20 by department at render
    time. A company can have 20 or more outstanding jobs, none of them on the chosen department's
    desks among the first 20, while that department has many later in the list.
  - In that case the panel returns early with "Nothing outstanding on this department's desks."
    (`:353-359`). That is false, and the "Showing the first 20." note is skipped.
  - When some do match, the note says "Showing the first 20." under fewer rows, without saying
    that the 20 were the company's.
  - The test covers only an uncapped snapshot.
- **Required:**
  - Either apply the department in the outstanding query, so the cap counts the department's
    jobs, or make the panel honest. When the company list is capped and a department is chosen,
    never claim "nothing outstanding". Say that only the company's first 20 jobs were looked at.
  - Add a test with a capped snapshot and a department that has no rows among those 20.
- **Resolution history:** 2026-11-10 open (B2-R1).
  2026-11-10 resolved (B2-R2): the honest-panel option. `outstanding_panel` (`pages/dashboard.rs:355-379`) knows the company list is cut at `OUTSTANDING_LIMIT`. With no match it says "None of the company's first 20 outstanding jobs is on this department's desks; it may have later ones.", and with matches it says "Only the company's first 20 outstanding jobs were looked at; this department may have more.". The uncut text is unchanged. `department_views_dashboard_groups_and_narrows_outstanding_jobs` asserts both with a capped snapshot, and that the false claim is absent; passed in my run.

---

## 01DEP-P04-B2-F3 — Switching company on the work queue with a department chosen answers 404

- **Classification:** actionable defect · **Low** · UX/correctness.
- **Evidence:**
  - The attention header is one GET form, `<select name="company_id">` plus the department select
    plus "Switch" (`pages/attention.rs:109-117`).
  - Pick a department, then pick another company and press Switch. The request is
    `/ui/work?company_id=B&department_id=<A's department>`, and `work_page` →
    `known_department` answers `404 Department not found` (`routes/attention.rs:205-208`).
  - So a user in several companies has to reset the department to "All departments" before
    switching company.
- **Required:**
  - Drop the department when the company changes. For example, give the department filter its own
    form with a hidden `company_id`, or keep the company switch from submitting `department_id`.
  - Add a page test asserting the company-switch form carries no `department_id`, or a route test
    showing the switch lands on the other company.
- **Resolution history:** 2026-11-10 open (B2-R1).
  2026-11-10 resolved (B2-R2): the department select has its own form, `#attention-department`, with hidden `company_id` and `view` (`pages/attention.rs:148`), and the company switch form submits no `department_id`. Asserted in `department_views_attention_groups_and_keeps_the_department`; passed in my run.

---

## 01DEP-P04-B2-F4 — The usage rendering check the plan asks for is missing, and three of the stated behaviours have no positive test

- **Classification:** actionable defect · **Low** · verification. The plan's "Focused checks":
  "Rendering tests for the dashboard, attention and usage grouping and the `_help` body".
- **Evidence:**
  - **Usage, rendering:** `pages/model_usage.rs` gains the "Department" option and a department
    row label, and `routes/model_usage.rs:45` accepts `group=department`. No test renders them;
    `UsageGrouping::Department` appears only in the aggregate test and the persistence test.
  - **Usage, join:** the persistence test seeds only a run with no desk, giving "No department".
    The join `run_id → workflow_runs.channel_id → channels.department_id → departments`
    (`persistence/model_usage/report.rs:149-178`) is never shown to name a department.
  - **`_help` and bounce order:** `writable_channel_directory` (`thread/ingest/routing.rs:672-713`)
    is meant to order desks by department tree rank, root first, with no department last. Every
    `ChannelPersistence` mock's `list_departments` returns `[]`. The formatter test feeds entries
    already in order, so that ordering is untested.
  - **Attention narrowing:** the use case's narrowing to one department's desks
    (`application/attention.rs:142-156`) is tested only with an empty department and through
    the company-items flag. It is never tested with an item on an owned desk beside one on
    another desk.
  - TASK lists these four as stand-ins. They are the behaviours 04C.1–3 state.
- **Required:** add:
  - a `model_usage_page` rendering test for `group=department`: the option selected, a department
    row, and "No department";
  - a usage persistence case with a run on a department-owned desk;
  - a thread use-case test whose mock returns departments, asserting root, then child, then
    "No department", by address within each;
  - an attention case where a one-department view keeps the owned desk's item and drops the other
    desk's item.
- **Resolution history:** 2026-11-10 open (B2-R1).
  2026-11-10 resolved (B2-R2), all four: `department_views_usage_page_groups_by_department` (`department_ui_tests.rs:718`: option selected, escaped label, department before "No department"); `department_views_usage_names_the_department_of_the_runs_desk` (`agent_history_department_tests.rs:159`: a call on a run whose desk Sales owns is grouped under Sales with id); `department_views_directory_orders_desks_by_department_root_first` (`thread/tests.rs:3276`: the mock now returns departments, the child is defined before its root and the desks arrive out of order, and a bounce lists revenue, ab-sales, zz-sales, then aaa-loose); `department_views_attention_reads_only_the_departments_desks` (`attention_authorization_tests.rs:177`: a recording store shows the one-department read gets only the owned desk with `company_items` false). All passed in my run.

---

## 01DEP-P04-B2-F5 — Small cleanups

- **Classification:** optional suggestion · Low · maintainability.
- **Evidence and suggestions:**
  - **Duplicated check.** "A department id must be the company's, or `NotFound`" is written three
    times: `known_department` (`routes/attention.rs:276`), inline in `DashboardScope::resolve`
    (`routes/ui_dashboard.rs:245-253`) and `Ui::company_department`
    (`routes/workflows/ui/runs/mod.rs:328-341`, which also re-reads `desk_departments` on every
    call). This breaks `src/AGENTS.md` "One decision, one place". Make it one
    `DeskDepartments::resolve(Option<Uuid>) -> AppResult<Option<DepartmentId>>`.
  - **Unneeded attribute.** `ChannelDirectoryEntry.department`'s `#[serde(default)]`
    (`thread/notices.rs:72`) changes nothing for an `Option` field. TASK justifies it with queued
    notices, which is the kind of compatibility reasoning I001 rules out. Drop it.
  - **Next-page link.** The "Waiting on a person" view reads at most 100 items. Its "Next page"
    link, from `attention_list`, leaves the runs workspace for `/ui/work`. Say so, or link within
    the workspace.
- **Resolution history:** 2026-11-10 open (optional).
  2026-11-10 resolved (B2-R2, optional): one `routes::known_department` (`routes/mod.rs:67`) used by attention, dashboard and the runs workspace; `#[serde(default)]` removed from `ChannelDirectoryEntry.department` (`thread/notices.rs:72`); the waiting view drops the next-page link and says "Showing the first N. Open the work queue for the rest." with the department kept (`pages/workflow_runs/board.rs:277-302`). That note has no test (inspection only). The runs workspace still reads `desk_departments` twice on a request with a department (scope resolve and scope options); this is harmless. Moving the helper left a stray doc line, logged as B2-F6.

---

## 01DEP-P04-B2-F6 — `known_department` took over the doc comment of `deserialize_empty_string_as_none`

- **Classification:** optional suggestion · Low · maintainability. Found in round
  `01DEP-P04-B2-R2` (digest `a97fdbeb6c8026e22cfc316bc13f961b61b087a7131c5c4c58ad1c1d2f0c58ba`).
- **Evidence:** `routes/mod.rs:64-77`. The new `known_department` was inserted below the doc
  comment of `deserialize_empty_string_as_none`. So `known_department`'s rustdoc now starts with
  "A query or form field where an empty string means "not given" rather than a value to parse.",
  which is not what it does, and `deserialize_empty_string_as_none` (`:79`) has no doc comment.
  Behaviour is unaffected.
- **Suggestion:** move line 64 back above `deserialize_empty_string_as_none`.
- **Resolution history:** 2026-11-10 open (optional, B2-R2).
