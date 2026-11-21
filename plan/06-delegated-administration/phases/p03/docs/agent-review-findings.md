# Reviewer findings — 06DA-P03

## Round 06DA-P03-B1-R1 — 03A docs + 03B final gate (note 06DA-P03-B1-R1-N1)

Common context:
- Date 2026-11-19. Workflow implement-opus-reviewed-2_4. Reviewer: Claude Opus 5.5 (`claude-opus-5-5`); effort not visible to me (assigned xhigh).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan06`, branch `plan06`, HEAD `a93cd38844c76dd48dbbe42ceba8ebf68864d4df` (p01 and p02 committed). The artifact is the working tree: three docs plus the 03 completion record.
- Digest `61b33200230c82b9b1c12e746326ceb6c6d14f44dc9b5823e4bce65e44c5c57d`, exclusions `phases/p03/docs`, `phases/p02/docs`, `phases/p01/docs`, `COORDINATION.md`.
- Coordination r10.

## 06DA-P03-B1-F1 — the docs name some authority the code does not have, and keep stale "admin" statements

- **Actionable / low / requirements (03A: "replace 'owner or admin' with the unit rule where it applies. Name only behavior the code has") — OPEN.**

**Edited text that overstates or understates the code**

1. `docs/departments.md` Administration table, row 1, "(the root itself: owner or root admin)". Root edits go through `managed_parent` → `ManagementTarget::Root` → `unit_management_role(NULL→root)`, which a **root lead** also satisfies (U5: a root lead manages every unit). Description and charter edits are open to the root lead; the slug and parent are refused for anyone (`lifecycle.rs` update guard).
2. Same table, members row: "root agents are the owner's or a root admin's".
   - Removal is (`remove_root_member` → `administers_company`).
   - Adding a root member follows the predicate (`add_member` → `managed(Some(root))`; persistence `require_department_management_on(Some(root))`), so a root lead can add root agents through the JSON API. The pane only hides this (p02 B1-F4, recorded as conservative).
3. Same table, last row: "Company settings … Company administration: the owner and root admins". `docs/authentication-operations.md` says the same ("Company-level settings … stay with the owner and root admins"). The company settings edit and delete (`owned_company`: `company.rs` update/delete, `ui_companies.rs:634`), model-connection writes (U3), skill writes and onboarding are **owner-only**. Only team levels, connections, memory, local agents and the run views are owner + root admin.
4. "Recent administration … lists … who created, edited, moved or **deleted** it". A deleted department has no pane, and its `department_deleted` row is unreachable (`administration_history` requires `managed(Some(D))`). The card can never show a deletion.
5. `docs/authentication-operations.md`: "A management refusal is `404`, as for someone outside the company." This contradicts the F10 rule that `departments.md` itself states: business refusals inside the caller's authority stay `403`.
6. `departments.md` intro: "managed … by whoever manages them: … the lead of the department or of one of its ancestors". By the doc's own table, a department's fields and deletion need its *parent's* manager, so the lead of D cannot edit or delete D. Reword so the intro does not imply otherwise.

**Pre-existing statements the code now contradicts**

These are in `docs/departments.md`, which the assignment asked me to check:

7. Line 105–106: "the binding then runs as the **admin** who changed the lead". Since p01, a lead change is made by whoever manages the parent, often a unit lead. The binding runs as that `actor` (`provisioning_department.rs`, `follow_department_lead_on(…, actor, …)`).
8. Line 109: "when the **admin** cannot see the desk". Same actor.
9. Line 124: the Agent Builder's `list_company_agents` "stays an owner's or admin's tool". The Builder authorizes its requester at `ManagementTarget::AnyUnit` (`provisioning.rs` `authorize_requester`, `ProvisioningActionAuthority`), and its directory filters by `unit_management_role` (`provisioning_directory.rs:103`), so unit managers use it for the agents they manage (01B.5).

**Checked and still accurate:**
- workflow-operations TypeSafe connections (company administration);
- final-reply reviewers must be company admins (`channel_reply.rs` `validate_reviewers`);
- memory capture is owner/admin (`lock_authority`);
- run inspection is owner/admin;
- escalation acknowledge;
- "a company admin needs a view grant" (auth-ops:70).

**Required correction.** Edit the nine places to match the code, including the root-lead cases U5 allows, the owner-only company settings, the 403 business refusals, and the history card's actual content. Rerun `scripts/check-integration-docs.py`.

**Instruction lesson.** For "name only behavior the code has", grep the whole document for role words (owner, admin, lead), not only the paragraphs being edited.

## 06DA-P03-B1-F2 — the completion record omits two gate outcomes, and one gate record calls the hang "pre-existing" without evidence

- **Optional suggestion / low / records — OPEN.**
- **Browser gate.** The authoritative `scripts/check.sh` run failed "Browser protocols using native markup" (a 17,129 s hang; `final-check.txt` 5696–5737). It passed only on a separate rerun (`final-browser-rerun.txt`). The 03 completion record never mentions this.
- **Tool-gap gates.** The transport boundary and the attribution service gates failed in the run because of tool gaps (`rg`, `tomllib`) and passed only on shimmed reruns. The completion record does not say so either.
- **"Pre-existing".** TASK calls the hang an "intermittent **pre-existing** hang". The test, `assets/app.js` and the knowledge page are unchanged by Plan06 (`git diff be1b504b HEAD` is empty for those files), and two reruns pass. But nothing shows the hang occurring before Plan06.
- **Missing log.** The "knowledge-browser.mjs alone … PASS 1/1 (1.3 s)" run has no retained log.
- **Suggestion.** Add one completion-record sentence listing the gates that passed only on rerun (browser hang; tool-gap transport and attribution). Word the hang as "intermittent, not reproduced in two reruns; test and page unchanged by Plan06; cause not established". Either retain the single-file rerun output or drop the claim.

---

## Round 06DA-P03-B1-R2 — recheck (note 06DA-P03-B1-R2-N1)

Context: 2026-11-19. Reviewer Claude Opus 5.5 (`claude-opus-5-5`), effort not visible to me (assigned xhigh). HEAD `a93cd388`. Artifact `f8bfaccd7dadb06a774f1bde3dffd5001be0b3fb5246f67f9804df5d73f2d160`, same four exclusions. Coordination r10.

### 06DA-P03-B1-F1 — recheck 2026-11-19 / B1-R2

**FIXED.** Each item was re-verified against the code.

1. Root row: name, description and charter belong to the owner, a root admin or a root lead (`managed_parent` → Root predicate). The slug and parent are refused (`update_department_on` guard). Root deletion is refused. The name is indeed editable; only the slug and parent are guarded, and `companies_follow_root` keeps the root in step with company renames.
2. Members row: removing a person from the root is the owner's (`team.require_owner`); removing a root agent needs owner or admin (`administers_company`); adding stays under "whoever manages".
3. New owner-only row (company edit/delete, model connections, skills, onboarding) and the owner + root-admin row, split the same way in authentication-operations. It says "who may change", so U3's read access for unit managers is not contradicted.
4. The history card no longer claims a deletion entry, and notes that deletions are recorded without a card.
5. authentication-operations now states 404 for what the caller does not manage and 403 inside their authority, matching F10 and `departments.md`.
6. The intro now says a lead looks after what is inside their department, while the department's own name, deletion and roles belong to its parent's manager.
7–8. The lead-change binding runs as, and is refused when, "the person who changed the lead" cannot see the desk.
9. The Agent Builder's `list_company_agents` answers whoever manages a department, for what they manage.

`scripts/check-integration-docs.py` PASS (`logs/B1-R2-integration-docs.txt`).

Non-blocking observation: `departments.md:39` "The root's roles, and its people, are changed by the owner only" reads slightly narrowly next to admin member invitations. Both docs state the invitation rule explicitly, and authentication-operations words it as "the root's people are the owner's; root admins may invite members", so no change is required.

### 06DA-P03-B1-F2 — recheck 2026-11-19 / B1-R2

**APPLIED.**
- The 03 completion record now names the gates that failed in the `check.sh` run and passed only on rerun: the browser protocols (one test hung; cause not established), and the transport boundary and attribution service gates (missing `rg` and Python 3.11).
- The TASK hang disposition reads "cause not established".
- The single-file rerun is retained (`logs/final-knowledge-rerun.txt`: 1/1 pass).
