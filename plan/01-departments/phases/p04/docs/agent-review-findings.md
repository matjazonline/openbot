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
