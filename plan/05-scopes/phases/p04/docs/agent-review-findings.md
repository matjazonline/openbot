# Plan05 p04 — reviewer findings

Shared across this phase's batches. Findings are recorded against the artifact actually
examined; resolution updates are appended, never rewritten.

## 05SC-P04-B1-R1-F1

- **Context:** 2026-11-18, implement-opus-reviewed-2_4, phase p04, batch B1, batch review,
  round note [05SC-P04-B1-R1-N1](agent-review-notes.md#05sc-p04-b1-r1-n1). Reviewer Fable 5.1
  (host-reported; effort not exposed). Worktree /Users/mac02/orca/workspaces/openbot/plan03-04,
  branch feature/plan05-scopes, base/HEAD 3d7148b1b496e491e139fc90e0534ef8d7d296ab, staged
  digest 499d9b65b220c1c968394cf561c3252ce9338c8876226019b65bb1b700761a42. Records
  [TASK](TASK.md), [REVIEW](REVIEW.md).
- **Classification:** actionable defect, severity medium, category requirements/documentation
  accuracy (overstates read authority).
- **Evidence:** `docs/agent-authoring.md`, section "Memory and knowledge scopes", third
  paragraph: "Otherwise its base chain is the root and its default write is personal. Lead
  entitlements can still add direct-child reads." Actual behavior when a deskless run has no
  single unambiguous placement: `src/application/workflow/scopes.rs:70-120` inserts the root
  only for an empty chain and never calls `add_reports` (doc comment line 68-69: "confers root
  read only and never lead-report expansion"); `src/adapters/persistence/workflow/run_scopes.rs:130-166`
  runs the lead-children query only `if !chain.is_empty()`; unit test
  `run_scopes_resolve_absent_or_ambiguous_placement_is_readonly_root`
  (`src/application/workflow/scopes_tests.rs:67-90`) asserts the lead's report child is not
  readable. The original requirement agrees: `plan/05-scopes/02-scope-resolution-and-tools.md`
  item 1, "no chain and several departments → units = root only". The sentence therefore
  documents a lead read entitlement the code refuses in that case; the original 03A requirement
  is "Name only behavior the code has", and AGENTS.md requires production docs to describe
  actual behavior. Impact: an operator or author would expect deskless unplaced lead agents to
  read report units and might rely on it. The preceding sentence ("base chain is the root") is
  also loose: the run gets a read-only root entry, not a chain with a write unit. Required fix:
  state that without a resolved local unit the run reads the root only, writes personal, and
  lead report reads do not apply; lead reads apply only when a local unit is resolved (desk or
  single unambiguous placement). Check `docs/departments.md` "Memory and knowledge" paragraph 2
  stays consistent (it currently describes the desk case only, which is accurate).
- **Instruction lesson:** observation: the author's contract audit checked links/anchors and
  startup registrations but did not trace each behavioral sentence to a source branch or test;
  a sentence about a refusal/limit branch was taken from the general rule rather than the
  specific case. Candidate (scope: documentation batches in this workflow): the acceptance
  mapping for documentation criteria should cite, per behavioral paragraph, the branch or test
  that proves it, with refusal/edge branches listed separately. Hypothesis only; one instance.
- **Resolution history:** open at B1-R1.

## 05SC-P04-B1-R1-F2

- **Context:** same round, reviewer, worktree, base/HEAD and digest as F1.
- **Classification:** actionable defect, severity low, category verification reporting.
- **Evidence:** `plan/05-scopes/04-integration-and-final-verification.md` completion record,
  row "02 focused checks": "180 service tests and actual subject/service proof retained". The
  accepted p02 evidence reads "service suite178PASS/2ignored" (`plan/05-scopes/phases/p02/docs/TASK.md:617`)
  and "service178/0/2 opt-in" (`plan/05-scopes/phases/p02/docs/agent-review-notes.md:261`).
  The record sums passed and ignored into one count, blurring PASS versus not exercised, which
  the verification-contract reference and AGENTS.md verification rules require to stay
  distinct in user-facing reports. Required fix: "178 service tests PASS, 2 ignored" (or
  equivalent). Other numbers in the record (4706/2/27/85, 26 scope cases, binary 6 and
  source-module 8, lease 139, live memory 7, backup 1, journey 13 steps, Linux scope 8
  categories, native download 404, reviewer identity, source digest, main commit) were
  checked against p03 records and remote main and match.
- **Instruction lesson:** none supported beyond the existing rule; a transcription precision
  slip. Cause: author condensed two numbers into one.
- **Resolution history:** open at B1-R1.

## 05SC-P04-B1-R1-S1

- **Context:** same round, reviewer, worktree, base/HEAD and digest as F1.
- **Classification:** optional suggestion, severity info, category documentation completeness.
- **Evidence:** `docs/departments.md` route table "The knowledge JSON routes are under
  `/api/companies/{company_id}/knowledge`" lists four routes; the router also serves
  `DELETE /documents/{document_id}` with JSON `expected_revision`
  (`src/adapters/http/routes/knowledge/mod.rs:54-57`, `DeleteDraft` lines 120-130). Nothing
  stated is wrong; the table reads as a listing and omits an existing route. Suggested: add the
  row, or say the table lists the scope-related routes added by this plan.
- **Instruction lesson:** none.
- **Resolution history:** recorded as optional at B1-R1; does not block acceptance.

## Resolution updates — B1-R2 (2026-11-18)

Recheck round [05SC-P04-B1-R2-N1](agent-review-notes.md#05sc-p04-b1-r2-n1), base/HEAD
3d7148b1b496e491e139fc90e0534ef8d7d296ab, staged digest
db7a281d2171092056186a794140a1c710c1b67174817a19e183457a5257210f, reviewer Fable 5.1
(user-authorized for this phase).

- **05SC-P04-B1-R1-F1 — fixed.** `docs/agent-authoring.md` deskless paragraph now reads
  "Otherwise it reads only the root, gains no lead direct-child reads, and its default write is
  personal." Rechecked against `scopes.rs:70-120` empty-chain branch, `run_scopes.rs:130-166`
  and `scopes_tests.rs:67-90`; matches. The following lines are a reflow with unchanged content;
  the `## Memory and knowledge scopes` heading is still unique.
- **05SC-P04-B1-R1-F2 — fixed.** 04 completion record row 02 now says "service suite 178 PASS/2
  ignored", matching p02 TASK.md:617 and p02 notes:261.
- **05SC-P04-B1-R1-S1 — implemented (optional).** `docs/departments.md` route table adds
  `DELETE /documents/{document_id}` with JSON `expected_revision`; matches
  `routes/knowledge/mod.rs:54-57` and `api_delete` (`Json<DeleteDraft>`, lines 121-123,163-168).
