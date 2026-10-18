# Phase 05 correction gate D5: membership deletion and disclosure validation

No backward compatibility: edit the squashed init migration and reset task test databases.
Amends D1–D4 only as specified here. No production wiring, commits or phase06+.

## Evidence and invariant

05-R005/F011's real membership-removal test fails: principals_company_user_fk cascades
company_members deletion to principals, while the three new decision principal FKs
prevent that deletion (correction-first.log). Historical decision facts must survive
membership revocation; their authors/reviewers do not retain current authority.
Changing the global principal lifecycle would affect unrelated authorization consumers.

## Local snapshot correction

Remove only workflow_decision_reviewers_principal_fkey,
workflow_decision_comments_author_fkey and workflow_decisions_submitted_by_fkey.
Their principal UUIDs become historical identity snapshots. Retain every decision/wait
composite FK, all immutable guards, notice uniqueness and provenance, and all command
membership/visibility checks. No cascade deletes or nulls historical decision facts.

Add NOT NULL reviewer.user_id, filled from the tenant/person principal by its BEFORE
INSERT guard, so reviewer reads no longer inner-join an ephemeral principal. The
existing immutable reviewer guard protects both identity snapshots. No user FK: account
removal also must not rewrite audit history. Comment/submission views already expose
principal UUIDs directly and do not need joins or new user fields.

At initial write, database guards select the exact (company_id, principal_id, kind=person)
row FOR KEY SHARE: reviewer INSERT fills user_id (and refuses a supplied mismatching user);
comment INSERT checks author; first submitted_by write checks the submitter. A decision
inserted already submitted must pass the same check. Reject missing/foreign/non-person
identities with 23514; immutable/update guards remain. These locks prove provenance at
write and serialize deletion until commit without preventing later revocation. Subsequent
history reads never reinterpret a snapshot as authority. Decision rows remain scoped by
the retained wait FK; comments/reviewers remain scoped by their decision FKs.

Park-time eligible_reviewer additionally locks member and principal FOR SHARE, so a
removal winning eligibility yields no reviewer and the existing unrouted
ReviewerUnavailable settlement. A park winning eligibility commits assignments/notices
before removal can complete; subsequent submit/read/comment by that removed member
fails current authority. The trigger's provenance lock alone is not used as a retry path
for normal park-time removal. No change to authorization or delivery policy.

Tests: real membership removal before/after submit (controlled blocking witnesses),
park-time removal in both orders, comment/submission/reviewer history readable by a
remaining authorized actor after removal; removed actor cannot read/comment/submit.
Cross-tenant initial principal/decision/wait references are independently refused by
specific guards/FKs; existing notice provenance/uniqueness cases remain required.

## Disclosure-stage correction

D2 §4.4 incorrectly promised compile-time rejection of a dynamic whole-decision object
bound to message content. The compiler permits dynamic bindings and validates their
resolved value during prepare_step_inputs; the new test demonstrated this.
Keep that existing shared boundary rather than add a special decision taint/type system.
A whole decision output {choice,data,feedback} is not a valid message content object
{body,subject?}; input preparation must reject it before dispatch or any delivery.
Explicit mapping of an approved data field to body is valid. Comments and reviewer
assignments are not decision output. Replace the inaccurate compile-stage assertion
with tests of resolved input rejection and explicit mapping acceptance, plus a runtime
integration check that the invalid whole-object binding creates no message delivery.
This amends validation stage, not the nondisclosure requirement.

## Verification

Focused decision/wait/registry/message tests, including all F011–F013 corrections;
fresh migration/schema, offline compile, strict clippy, SQLx and formatting. Rerun the
failed broader 05A workflow gate. Final full suite/stock-stack/lease gate remains after
05B. Independent D5 PASS precedes dependent schema/production edits; whole corrected
05A requires a matching frozen-artifact PASS before 05B starts.
