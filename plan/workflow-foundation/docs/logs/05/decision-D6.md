# Phase 05 D6 — correction to D5 lifecycle evidence and races

No backward compatibility. D6 amends D5; all other D5 decisions remain unchanged.

## Actual removal and snapshot invariant

CompanyInvitePersistence::remove_member (company_invite.rs:355–384) first updates
principals to kind=external,user_id=NULL, then deletes company_members. Thus production
removal preserves a principal UUID but invalidates its live person identity; the existing
review projection can fail decoding NULL user_id. D5's raw DELETE test separately proves
that a direct membership deletion is blocked by the new historical FKs. Cover both.

Reviewer.user_id is an immutable historical snapshot and remains readable after either
principal conversion or deletion. Comment/submission principal UUIDs remain immutable
historical snapshots, not live authority. Commands still require present membership and
a current person principal. A removed and re-added member has no implicit authorization
from the snapshot alone; normal current reviewer matching remains in force.

Use FOR SHARE, not D5's FOR KEY SHARE, for the new initial snapshot identity checks, so
principal kind/user changes serialize with the check as well as DELETE. Park eligibility
locks both member and principal FOR SHARE. The existing command authority selects and
locks the current member and person principal before taking run/association/job locks.
The production removal's principal-first mutation can oppose a query's member-first
row locking; do not claim this joined query has a global no-cycle proof. Existing 1s
lock/statement bounds fail closed and roll back the decision transaction on contention;
a caller may retry. No partial decision/continuation/notice is committed on that path.
Do not broaden this batch into a global membership mutation protocol rewrite.

## Required verification amendment

In addition to D5 raw-delete, visibility and initial provenance tests:
- Invoke the actual remove_member port after park, before submission: removed member's
  submit/read/comment fail; a remaining authorized member reads the original reviewer
  user IDs without a NULL decode and sees unchanged decision/continuation state.
- Add a comment, submit, then invoke remove_member: remaining authorized reader sees
  unchanged reviewers/comment author/submitted_by, feedback, data and settled state.
- Exercise the actual port competing with submission. Hold the principal with a separate
  transaction to coordinate removal reaching it; when the decision owns authority,
  prove remove_member blocks until the decision commits. For removal-first, complete its
  principal conversion in a transaction with the same SQL ordering, then start submit,
  release removal and prove refusal. Also invoke the unmodified port as a direct
  competitor to check no partial writes on any bounded contention outcome.
- Park-time membership conversion/removal winning eligibility must settle unavailable;
  park winning member/principal ownership must commit then allow removal, with historical
  identity intact. Preserve the original tests for a principal that disappears entirely.

No assertion may accept a timeout as proof of the successful business branch. Controlled
orders must reach their expected durable states; a separate contention test may accept
only an explicit timeout/rolled-back branch and verify no partial writes before retry.
