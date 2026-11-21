# 04RU-P02-D1/r2 — membership grants are not email-derived grants

Amends accepted D1/r1 only in the following contract refinement. Original ownership,
Members/Public, per-principal materialization, ordinary relay receiving-grant policy,
company/principal/gate/row locks, delta reconciliation and revision rules remain.
No affected semantic correction is implemented before independent gate PASS.

## Contradicted assumption and actual proof

Original Plan04/02 requires root Members desks to retain Team expectations and all
Members desks to use the single materialized grant policy. PrincipalAccessContext's
existing grant_authority documentation explicitly withholds grants derived from a
current email, not account-ID-based department membership. Original Plan03 security
requires unverified authenticated team members to retain team attention/internal
notes, while losing email-derived restricted desk/decision permissions.

D1/r1 implementation currently applies the old global Available predicate to all
channel grants, including new root membership grants. New runtime evidence:
logs/B1-unverified-root-members-investigation.txt FAIL at ui_message_tests.rs250,
security_email_identity_unverified_team_keeps_attention_and_internal_notes: signed
account keeps its principal and root membership, but visible_channel_ids loses its
root desk. Do not fix this by verifying the test user, weakening its assertions,
reintroducing Team/root bypass, or treating unverified email as a grant.

Broader actual consumer run logs/B1-core-consumer-tests.txt:291PASS/88FAIL/1ignored.
69failures cluster at channel_reply_completion_tests.rs149 during real reply-policy
configuration: approval admins are root members but association::authorize_channel
requires current verified-email proof for every grant. Preserve their actual
retirement/replanning assertions, not blanket account verification. Other failures
include expected fixture/UI/nullable-owner changes and require their own corrections;
not all88are attributed to this one cause without proof. All21HTTP authorization
cases must still be independently counted by name.

## Decision

- Extend Channel's existing capability decision in its responsible domain layer:
  a current matching department_membership grant may be used even when the signed-in
  context is WithheldUnverifiedEmail; every other grant provenance still requires
  Available. A missing principal never matches. UI still requires current company
  membership, with existing owner/Public exemptions. Both participate/view capabilities
  use this same per-grant rule. No root-ID lookup, mode bypass or alternate resolver.
- This authorizes the stable principal UUID's real department/role placement, not its
  unproved email address. WithheldUnverifiedEmail is supplied by access_context_for_user
  for authenticated users. Existing observed-identity qualification, primary-address
  proof, retirement/incarnation, namespace and tenant fences stay unchanged. Direct
  principal policy and all non-department grant restrictions stay unchanged.
- In transactional workflow association's existing locked grant/account SELECT, and
  workflow_follow_reviewer_eligible, replace the blanket verified-email requirement
  with (department_membership provenance OR verified account). Keep company membership,
  person/current user joins and existing account/grant row locks; a boolean helper
  must not hide or remove those locks. Notification SQL must agree with the same
  grant provenance rule, including withholding configured grants from unverified
  account readers, while preserving its membership/owner/Public requirements.
- Receiving relay agents still require actual participate grants on root and child
  Members desks, per explicit04RU-C003/r2. This amendment creates no trusted-relay,
  outsider read, old-address adoption or principal-incarnation bypass.

## Failure / concurrency / verification mapping

| Criterion | Production mechanism | Decisive checks |
| --- | --- | --- |
| Root Team expectations preserved | current UUID department grant in Channel policy and SQL consumers | existing unverified team attention/internal-notes test PASS unchanged; root and child domain/SQL matrix for Withheld context with derived versus configured/manager/no grant and missing principal |
| Email proof retained | non-department provenance still requires Available; qualified identity and lifecycle boundaries unchanged | existing security_email_identity_ private workflow/decision/action/SMTP/direct/notification cases, actual primary-address change/rejoin/incarnation negatives; do not verify users that those tests intentionally leave unverified |
| Real approval/follow consumers | locked association grant/account rows and current reviewer eligibility use provenance distinction | channel reply/follow/approval account-retirement and finalization suites, real member removal and account/grant revocation races; preserve source context/proposals/history |
| Ordinary relay contract | same actual agent principal and receiving participate row | real root grant accept/view-only/revoked-refusal tests, existing child and source/target revocation/cycle/tenant cases |
| One policy and atomicity | no new schema/transport/context type; per-grant provenance owns authority distinction | cross-product SQL/domain equivalence and competing membership/creation/settings tests; locks and revision rules unchanged |

## Independent measured performance disposition

logs/B1-optimized-scale.txt PASS:1000members/200desks/400000grants, join1745ms,
inserted400/deleted0, removal23ms inserted0/deleted400; all200revisions+1. This
improves prior join12252ms/800400grantwrites. But setup78682ms versus prior16212ms
exposes desk-creation cost that still needs investigation. Add explicit selected-desk
scope to the existing INSERT_MISSING anti-join so the planner can prune unrelated
existing desks, retain representative query-plan evidence, and remeasure before
acceptance. This is the same logical set and reconciliation protocol, no bound,
index, persistent schema or architecture fallback changes.

## Artifact / authority

Worktree /Users/mac02/orca/workspaces/openbot/plan03-04, feature/plan03_04.
Start/review-base/HEAD334e79c74b383dffd8e0d565c33a77c6ac24c94d.
Exact snapshot exclusions plan/04-root-unit/phases/p02/docs and exact
plan/04-root-unit/COORDINATION.md. Source frozen for this gate; original r1 probe
and all subsequent B1 source/deliverable docs are part of the source identity.
Coordination8, activeC001r1/C002r1/C003r2 and inherited01DEP-C002r1 acknowledged;
proposedC004r1 is this amendment and cannot be activated before independent PASS.
Whole-B1 implementation review and required execution proof remain outstanding.
