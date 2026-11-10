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
