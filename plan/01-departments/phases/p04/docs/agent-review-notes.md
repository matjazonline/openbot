# p04 agent review notes

## 01DEP-P04-B1-R1 — batch review B1 (04A Departments tab + 04B.1–3 + p02 B2-F2/F3), with merge b1026003

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B1; stage: batch review. Phase `p04`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`.
  Phase start, review base and HEAD are all `b1026003`, and B1 is uncommitted: 37 modified files
  plus 6 new source files and the regenerated `assets/app.css`.
- Artifact digest `8e8f63fe8cb3a161ee7af33529f5a432ab49359a7515dd403960d564fa3c1840`, recomputed with
  `snapshot.sh plan/01-departments/phases/p04/docs plan/01-departments/COORDINATION.md` at the start
  and before returning.
- Evidence `logs/01DEP-P04-B1-R1.evidence.json` validates with `--expect-artifact` (PASS 3, FAIL 2:
  the first affected run, and the claimed unrelated inline-JS failure).
- Coordination **revision 7** `3b556f5a…5414` = `logs/coordination-r7.md`: I001 r1, I002 r2, I003 r2,
  C001 r1, C002 r1. I diffed it against p03's r5: it adds the I002/I003 r2 wording, the p04 row and
  the p03 C003/C004 responses.

### Coverage and evidence

- **Requirements, read in the originals:** plan 04 Batch 04A items 1–4, 04B items 1–3 and the
  applicable "Focused checks"; README decisions 2, 6 and 7; p02 findings B2-F2 and B2-F3; C001 and
  C002.
- **Instructions:** `AGENTS.md`, `src/adapters/http/AGENTS.md` (object scoping, cross-tenant tests,
  same-origin, `hx-sync` for reads and no aborted writes).
- **Merge `b1026003`:**
  - Read the `--cc` resolutions for `agent.rs`, `agent_context.rs`, `preparation.rs` and the init
    schema. Read `git diff 6091c3cd b1026003 -- agent_history/recall.rs`, where all three
    `candidate_on` calls now pass the app domain. Read `git diff 30654e3c b1026003 -- frozen.rs`.
  - The restore order is capture → memory → saved department → history. That is sound:
    `with_memory` does not depend on the preamble, `with_department` is refused only after
    history, and the history budget sees both the saved department preamble and the saved memory.
  - The recall plan's `recall_tokens` is computed from a candidate that includes the department
    context, as at preparation.
- **B1 code read:**
  - `routes/ui_departments.rs` in full: `managed` before every write and before the create form,
    `Forbidden` → stranger 404, `readable` for GETs, and refusals rendered on their form.
  - `pages/company_departments.rs` in full: escaping, the tree nesting, `hx-sync` on read
    controls, `hx-disabled-elt` on writes, and confirmations.
  - Diffs of the use case (`department_view`, `principal_choices` managed), the port and
    persistence reads (`principals`, `principals_by_id`, `desks`), the entity (`department_tree`),
    `ui_companies.rs` (tab, counts only for owners, admin forms through
    `manages_company_operations`), `ui_channels.rs` (`ownership()` derivation, unchanged
    ownership → `None`, a classic form without the select → `None`), `channel_settings.rs` (select,
    box, help text, script via `data-input`), `channels.rs`/`channel.rs` (grouping, badge, every
    `list_departments` call after a managed check), `agent_settings.rs` and `ui_agents.rs`
    (placement card, additional desk), and the copy and docs.
- **Facts checked against p02/p03:**
  - A members-only desk keeps its configured allowlist (`department_desks.rs:350-390`, `stored_mode`)
    → B1-F2.
  - A department slug edit does not move the desk (p03 D8, `department_lead_desk_survives_a_rename…`)
    → B1-F1.
  - `/ui/channels` is managed-only → B1-F5.
  - Members already see colleagues' emails on the Team tab, so the department view's person
    details are not a new exposure.
- **Executed** (`openbot_dept_p04_test`, `SQLX_OFFLINE=true`, `--test-threads=4`):
  - the focused set `department_ui_ department_directory_ desk_wording
    agent_default_desk_onboarding`: 30 listed, 30 passed (`logs/B1-R1-review-focused.txt`);
  - the author's affected set with `--skip cloudroom`: 658 passed, 1 failed, 7 ignored
    (`logs/B1-R1-review-affected.txt`);
  - the one failure is `rendered_markup_carries_no_inline_javascript`. Its only offender is
    `pages/workflow_authoring/sample.rs:153`, a hostile test-input string. That file is not in the
    diff (last changed in 48461f66), so the failure is **pre-existing and unrelated**, and B1's new
    markup adds no inline JS.
  - CSS: I rebuilt `assets/app.css` from a scratch copy of the working-tree `src` and `assets`,
    with Tailwind from `/Users/mac02/dev/openbot/node_modules`, and compared: "CSS matches sources".
- **Not run:** the full gate (I003 r2: phase 05), the 28 `cloudroom_*` tests (Node peer build
  absent), clippy (the author reports it clean), and a browser run of the members-only toggle
  (a stand-in the author lists).

### Outcome

**Changes required** for digest `8e8f63fe…1840`. There is no PASS.

- Actionable:
  - 01DEP-P04-B1-F1 (Medium): the department header and the edit form present `slug@…` as the
    department desk's address, which a slug edit makes false;
  - 01DEP-P04-B1-F2 (Low): the members-only help text omits the desk's Participant Emails;
  - 01DEP-P04-B1-F3 (Low): no cross-tenant id route tests for the new department routes.
- Optional: B1-F4 (500-principal picker cap is silent), B1-F5 (members get desk links that 404;
  deleted-desk copy).
- **Sound as written:**
  - the merge resolution;
  - authorization on every new route;
  - the read and write `hx-sync` discipline;
  - the ownership derivation in the desk form;
  - grouping and badge;
  - the agent settings card;
  - the p02 B2-F2/F3 copy;
  - the docs entries;
  - the CSS.
- No item needs the user.

### Process and instruction feedback

- Candidate rule: UI text that names an address derived from an editable field must say which
  record decides the address (here the `department:{id}` record, not the slug) — B1-F1.

## 01DEP-P04-B1-R2 — correction recheck of 01DEP-P04-B1-F1..F5

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B1; stage: correction recheck.
  Phase `p04`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), agent `ad409b2d3d149c2c6`.
- Base and HEAD are `b1026003`; the work is uncommitted (37 modified files, 7 untracked paths,
  the same set as R1). Artifact digest
  `9cad3b579f1e5121427ee277b620bebd9a8dffa3786d299f748844354bb63848`, recomputed at the start and
  before returning (same exclusions).
- Evidence: `logs/01DEP-P04-B1-R2.evidence.json` validates with `--expect-artifact` (PASS 2,
  FAIL 1; the failure is the one already there).
- Coordination r7: `COORDINATION.md` sha256 = `logs/coordination-r7.md` = `3b556f5a…5414`.
- Correction description: TASK.md "B1 corrections after 01DEP-P04-B1-R1".

### Coverage and evidence

- **F1:**
  - `persistence/department.rs` `desks`: `own` means the `department:{id}` provisioning record's
    `result->'channel'->>'id'` names a desk that the department still owns. This is the same join
    `department_roster.rs` uses. The query orders by `own DESC`, then slug.
  - `pages/company_departments.rs:399-411`: the header takes that desk's address, or shows
    "No department desk.".
  - `:287-296`: the "created at this slug" sentence is on the create form only; the edit form says
    a slug edit does not move the desk; `slugify` is only on the create form.
  - The desks card has a "Department desk" badge.
  - Tests:
    - the route test edits `sales-team` → `revenue` and asserts the `#department-address` `<p>`
      keeps `sales-team@` and has no `revenue@`;
    - the page test has a desk at `sales-desk` and no `sales@`;
    - the reads test checks `own` true and false.
  - Small test weakness, not a finding: the second desk's slug `sales-overflow` also sorts after
    `sales`, so the test does not prove the `own DESC` ordering on its own. The ordering is
    cosmetic.
- **F2:** the help text now names "the addresses in Participant Emails", asserted.
  - The `@public` clause I had suggested is not in the help text. That is acceptable: the store's
    on-form refusal "A members-only desk cannot be open to @public." covers it.
- **F3:** I read `department_ui_foreign_ids_under_the_owners_path_change_nothing` in full:
  - all 7 department-id routes answer 404 without naming the foreign company;
  - B's department as a parent is refused on the form;
  - B's principal in A's add and assign forms and remove and clear paths is refused on the form;
  - A and B are unchanged afterwards.
- **F4:** `principal_choices` asks for `MAX + 1`, truncates, and returns `cut`. The route passes
  it to every picker, and the note is tested on the page.
- **F5:** desk links appear only when `editable`; members get a `<span>` (test asserts no
  `/ui/channels?`). The deleted-desk copy matches the "Create additional desk" button, asserted.
- No stale copy: I searched `src`, `docs` and `README.md` for the old strings and found none.
- **Executed** (`openbot_dept_p04_test`, `SQLX_OFFLINE=true`, `--test-threads=4`):
  - focused `department_ui_ department_directory_ desk_wording agent_default_desk_
    department_lead_desk_`: 63 passed, 0 failed (`logs/B1-R2-review-focused.txt`). This includes
    the new foreign-ids test, the reads test, and the p03 rename test.
  - the same affected set as R1 with `--skip cloudroom`: 659 passed, 1 failed, 7 ignored
    (`logs/B1-R2-review-affected.txt`). The only failure is again
    `rendered_markup_carries_no_inline_javascript` at `workflow_authoring/sample.rs:153`, which was
    already there and is unrelated.
  - CSS rebuilt from a fresh scratch copy (`scratchpad/p04css-r2`): "CSS matches sources".
- **Not run:** the full gate (phase 05 per I003 r2), the `cloudroom_*` tests, clippy (the author
  reports it clean), and a browser run.

### Outcome

**PASS** for digest `9cad3b579f1e5121427ee277b620bebd9a8dffa3786d299f748844354bb63848`.
01DEP-P04-B1-F1..F5 are resolved, and there are no new findings. No item needs the user.

### Process and instruction feedback

- None new.

## 01DEP-P04-B2-R1 — batch review B2 (04B.4 + 04C.1–4)

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B2; stage: batch review. Phase
  `p04`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`.
  Phase start `b1026003`; review base and HEAD `399efe2c` (B1, accepted). B2 is uncommitted:
  59 source/asset files, 1825 lines added and 104 removed.
- Artifact digest `e167c54ade2bb78b74c80524b9afc479b7b16b869c0a50c09b53c8ab4048171d`, recomputed
  at the start and before returning (exclusions p04 docs + `COORDINATION.md`).
- Evidence `logs/01DEP-P04-B2-R1.evidence.json` validates with `--expect-artifact` (PASS 3,
  FAIL 1). The failure is the affected run with the pre-existing inline-JS failure and the skill
  test that was later fixed.
- Coordination r7: `COORDINATION.md` sha256 = pinned `3b556f5a…5414`. I001 r1, I002 r2 and
  I003 r2 apply.

### Coverage and evidence

- **Requirements, read in the originals:**
  - plan 04 items 04B.4 and 04C.1–4, and the "Focused checks" bullets for the dashboard,
    attention and usage rendering, the `_help` body and the JSON fields;
  - README decision 2: labels say "desk", while JSON field names and URLs keep `channel`
    (`department_id`, `members_only`, `channel_id` unchanged);
  - plan 01 item 3 and p01 D4 r2, for member visibility.
- **Instructions:** root, `src/`, `src/application/`, `src/adapters/http/` and
  `src/adapters/persistence/` AGENTS.md.
- **Diff read in full:**
  - entity `DeskDepartments`/`HeldDepartment`;
  - the department port, persistence (`desk_owners`, `agent_departments`,
    `departments_of_company`) and use case (`desk_departments`, `agent_departments`);
  - attention: entity, use case, SQL bind, routes, page;
  - run board: `RunFilter`, `RunBoardScope`, `push_run_page`, `push_scope`, `scoped`;
  - the runs routes `scope`, `scope_options` and `waiting_workspace`, and the pages
    `runs_waiting`, toggle and `scope_controls`;
  - dashboard: route `DashboardScope::resolve` and the page's picker and `outstanding_panel`;
  - model usage: `load_calls` join, aggregate key, page, route;
  - `_help`/bounce: `writable_channel_directory`, `DirectoryCache::departments`,
    `push_channel_directory`;
  - agent JSON (`agent_response`, list/get/create/update), plus every mock and test-constructor
    update.
- **AttentionQuery split (as asked):**
  - There is one production constructor (`application/attention.rs:160`).
  - `company_items` is `manager` without a department and `false` with one.
  - `$14` now binds `company_items` and is used only in the five `channel_id IS NULL AND $14`
    branches: decision my_work and team_work, run reconciliation, run failure, binding refusal.
    The delivery-failure branch needs a channel.
  - `manager` still decides the run links (`persistence/attention.rs:410-418`).
  - Test constructors: five set both `true`, and two set `company_items: manager`. Sound.
- **Access per surface:**
  - The runs workspace is managed-only (`frame` → `load_managed_company`), so its
    TeamWork-backed waiting view cannot 403 a member.
  - The dashboard is managed companies, or an operator; an operator outside the company gets no
    departments.
  - Agent JSON is manager-only (`verify_company_manager`); usage is manager-rechecked.
  - `/ui/work` is open to plain members → B2-F1.
  - The bounce lists desks only to team senders, as before.
- **Skill test fix (as asked):**
  - The cause is confirmed. p03 B1 (`523732b7`) moved
    `picking_a_library_agent_copies_its_skills_with_the_agent_and_channel` onto
    `company_agents().copy_from_library`. Since 01DEP-P03-D3, that path refuses with "Add a model
    connection with an API key before creating an agent…" (`company_agent.rs:191`) when no keyed
    connection exists, and the test seeded none. The error is in `logs/B2-affected-1.txt:1174`.
  - D3 is accepted and says "No company agent can be created before a keyed model connection
    exists". So fixing the test by seeding a default keyed connection, a pattern used in six
    other test files, is right, and changing the code would be wrong.
- **Run board SQL:**
  - `channel_id IN (SELECT desk.id FROM channels … company_id = $ AND department_id = $)` is the
    run's desk's current department. It is company-bound on both list and board. `push_scope`'s
    `keyword` chaining is correct.
  - JSON run filters pass `department: None`; the plan asks only for the UI.
- **Usage join:**
  - The join runs after the call limit (inside the MATERIALIZED CTE), and every table is bound to
    `company_id = $1`.
  - History rows carry `NULL::uuid` for the run, so they fall under "No department".
- **`_help` ordering:** by inspection, the order is `department_tree` pre-order rank, then
  address, with `usize::MAX` for no department. The heading switch prints "No department" last.
- **Executed** (`openbot_dept_p04_test`, `SQLX_OFFLINE=true`, `--test-threads=4`):
  - **Focused:** `department_views_ department_ui_ agent_default_desk_ api_agents_carry_no_retired_runtime_fields department_desk_ownership_json_fields_are_kept_when_absent model_usage_workflow_exact_provenance_and_grouping picking_a_library_agent_copies_its_skills_with_the_agent_and_channel attention help_ run_board_queries_bind_each_scope_filter_once_in_placeholder_order`.
    77 passed, 0 failed, including all 10 `department_views_*` tests and the fixed skill test
    (`logs/B2-R1-review-focused.txt`).
  - **Affected:** the author's set plus `skill`, with `--skip cloudroom`: 1182 passed, 1 failed,
    9 ignored (`logs/B2-R1-review-affected.txt`). The failure is
    `rendered_markup_carries_no_inline_javascript`, whose only offender is still the pre-existing
    `workflow_authoring/sample.rs:153`. The author's skill-test failure is gone.
  - My first affected run ran out of disk (ENOSPC, `StorageFull` while listing tests). Its 74
    failures are not evidence; the log is kept as `logs/B2-R1-review-affected-ENOSPC.txt`. I then
    ran two subsets: non-database, 300 passed and the same 1 failure
    (`logs/B2-R1-review-nondb.txt`); database-backed, 315 passed (`logs/B2-R1-review-db.txt`).
    The full rerun above replaces both.
  - `cargo fmt --check` is clean.
  - CSS: I rebuilt it from a fresh scratch copy (`scratchpad/p04css-b2`): "CSS matches sources".
- **Not run:**
  - the full gate (I003 r2: phase 05);
  - `cloudroom_*` (no Node peer build);
  - clippy (the author reports it clean; not rerun, to avoid contending with the test build);
  - browser runs of the pickers.

### Outcome

**Changes required** for digest `e167c54a…171d`. There is no PASS.

- Actionable:
  - 01DEP-P04-B2-F1 (Medium): plain members see and can filter by every department (p01 D4 r2);
  - 01DEP-P04-B2-F2 (Medium): the dashboard department filter narrows the company's capped
    first 20 and can claim "nothing outstanding";
  - 01DEP-P04-B2-F3 (Low): switching company on `/ui/work` with a department chosen answers 404;
  - 01DEP-P04-B2-F4 (Low): no usage rendering test, and no positive tests for the usage join, the
    `_help` ordering and attention narrowing.
- Optional: B2-F5 (the duplicated department check, a no-op `#[serde(default)]`, the waiting
  view's next-page link).
- **Sound as written:**
  - the AttentionQuery split;
  - run board list and board SQL and the route plumbing, including foreign-company department
    resolution → 404, tested;
  - the waiting view's authorization;
  - the agent JSON `departments`/`more_departments` and the channel JSON field assertions;
  - the usage SQL;
  - the `_help` formatter;
  - the skill-test fix;
  - the CSS.
- No item needs the user. F1 may be settled either by code or by a coordination revision.

### Process and instruction feedback

- Candidate rule: a new read used by member-visible surfaces must name which visibility rule
  (`visible_departments` or managed) it follows, and a read wider than the accepted rule needs a
  decision revision first (B2-F1).
- Filters applied after a capped read should say so in the UI (B2-F2).

## 01DEP-P04-B2-R2 — correction recheck of 01DEP-P04-B2-F1..F5

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B2 (04B.4 and 04C.1–4); stage:
  correction recheck. Phase `p04`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), agent `ad409b2d3d149c2c6`.
- Phase start `b1026003`. Base and HEAD are `399efe2c`; the work is uncommitted (62 modified
  files, none untracked, outside the excluded paths). Artifact digest
  `a97fdbeb6c8026e22cfc316bc13f961b61b087a7131c5c4c58ad1c1d2f0c58ba`, recomputed at the start and
  before returning (exclusions `plan/01-departments/phases/p04/docs`,
  `plan/01-departments/COORDINATION.md`).
- Evidence: `logs/01DEP-P04-B2-R2.evidence.json` validates with `--expect-artifact` (PASS 2,
  FAIL 1; the failure is the existing `rendered_markup_carries_no_inline_javascript`).
- Coordination r7: `COORDINATION.md` sha256 `3b556f5a…5414`, unchanged (I001 r1, I002 r2,
  I003 r2). F1 was fixed in code under p01 D4 r2, so no decision or coordination revision was
  needed.
- Correction description: TASK.md "B2 corrections after 01DEP-P04-B2-R1".

### Coverage and evidence

- **Delta:** I diffed the tree against my B2-R1 copy of `src` and `assets`
  (`scratchpad/p04css-b2`, taken at digest `e167c54a…171d`). Exactly the 16 source files the
  author names, plus `assets/app.css`, changed. Nothing changed outside `src` and `assets`: no
  `.sqlx`, migrations or docs. I read every hunk.
- **F1 (inspection and test):**
  - `desk_departments` takes `readable_desks`. Managers get every department and owner. Anyone
    else gets `list_for_user`, the same rule `visible_departments` uses, plus the owners of those
    desks. The owner map is cut to those desks.
  - The three attention routes list first and then pass `AttentionListing.readable_desks`.
    `readable_desks` is `read_context.visible_channel_ids` taken before the department narrowing,
    so a chosen department does not shrink the select.
  - An unseen department narrows the read to readable desks it owns (none), so nothing leaks
    before the 404.
  - All production callers checked (`grep desk_departments`): the dashboard and the runs
    workspace pass `&[]` and are reached only through managed companies. An operator outside the
    company gets no departments, as before.
- **F2:** the panel's two cut-list texts and the unchanged uncut text, read and tested with a
  capped snapshot.
- **F3:** a separate `#attention-department` form carries hidden `company_id` and `view`; the
  company switch has no `department_id`, tested.
- **F4:** I read all four new tests. Each makes the decisive assertion I asked for, and the
  directory test cannot pass by insertion order (the child is defined before its root and the
  desks arrive out of order).
- **F5:** one `known_department`; `#[serde(default)]` gone; the waiting view's note keeps the
  department and has no test (inspection only).
- **Executed** (`openbot_dept_p04_test`, `SQLX_OFFLINE=true`, `--test-threads=4`):
  - **Focused:** `department_views_ department_ui_ adapters::http::pages::dashboard
    adapters::http::pages::attention adapters::http::routes::attention application::attention
    application::use_cases::thread adapters::http::routes::ui_dashboard
    adapters::http::routes::workflows::ui::runs model_usage department_directory_ desk_wording
    --skip cloudroom`: 189 passed, 0 failed (`logs/B2-R2-review-focused.txt`). This includes the
    five new tests and the two changed F2 and F3 page tests.
  - **Affected:** the B2-R1 set, with `--skip cloudroom`: 1187 passed, 1 failed, 9 ignored
    (`logs/B2-R2-review-affected.txt`), the same as the author's `logs/B2-R2-affected.txt`. The
    failure is `rendered_markup_carries_no_inline_javascript`; its only offender is still
    `workflow_authoring/sample.rs:153`, which existed before this work and is unrelated.
  - `cargo clippy --locked --offline --all-targets -- -D warnings` is clean
    (`logs/B2-R2-review-clippy.txt`), and `cargo fmt --check` is clean.
  - CSS: rebuilt from a fresh copy (`scratchpad/p04css-b2r2`): "CSS matches sources".
- **Not run:** the full gate (I003 r2: phase 05), `cloudroom_*` (no Node peer build), and browser
  runs of the two forms.

### Outcome

**PASS** for digest `a97fdbeb6c8026e22cfc316bc13f961b61b087a7131c5c4c58ad1c1d2f0c58ba`.

- 01DEP-P04-B2-F1..F4 are resolved, and so is the optional B2-F5. Dispositions are in
  `agent-review-findings.md`.
- New: 01DEP-P04-B2-F6 (optional, Low). `known_department` took over the doc comment of
  `deserialize_empty_string_as_none` (`routes/mod.rs:64`). It does not block.
- Gaps, not findings:
  - the F1 test's 404 does not assert the body;
  - the waiting view's new note is untested.
- This recheck covered the correction delta and its callers. For the rest of B2 it reused my
  B2-R1 review, whose code is unchanged.
- No item needs the user.

### Process and instruction feedback

- Diffing against the source copy kept from the previous round's CSS check gave an exact
  correction delta without a commit. Candidate (reviewer, recheck rounds): keep a copy of the
  reviewed tree at each round's digest so the next recheck can diff against it.
