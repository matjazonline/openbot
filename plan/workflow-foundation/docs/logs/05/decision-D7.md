# Decision 05-D7 — consumed action approval cannot grant another remote entry

No backward compatibility. Proposed amendment to D1 §9/D2 §16.5 during 05B;
05A remains accepted. Root author under implement-astra-reviewed-2.1.

## Evidence and required behavior

D1 §9 promises at most one dispatch from an approval and cites
workflow_action_replay_supported as preventing replay. That predicate rejects a marker
whose current_policy.approval_required is true. However, action_receipts::marker_on
also accepts workflow_action_not_applied_available, and the deferred
workflow_action_remote_entry_commit_guard has the same alternative. A valid 04.7
not-applied proof can therefore bypass the cited replay predicate. No dependent
correction is implemented pending this gate.

## Amendment

An approval authorizes one remote entry. Its immutable dispatch marker retains
approval_required=true and approval_decision. A later attempt cannot allocate another
entry under that marker, even if operator protection is later removed or a provider
proves the previous effect was not applied. Existing receipts remain returnable after
current access checks; reconciliation continues to record truthful applied/not-applied
results without changing its evidence protocol.

- Reservation, after authorization and receipt lookup, detects an existing protected
  marker owned by an earlier attempt and returns ApprovalRequired without writes or I/O.
  The existing failure classifier makes this terminal approval_required. The shared
  retirement predicate still overrides unknown-effect failures to reconciliation. If
  genuine not-applied evidence proves safety, the terminal failure can take the normal
  final-error route; it does not dispatch again.
- Same-attempt competitors retain the existing possible-dispatch result. They cannot
  allocate another entry because the existing unique key and marker logic prevent it.
- marker_on never reuses a marker carrying approval_required=true (regardless of the
  newly observed policy) through either provider replay or not-applied evidence.
- The deferred remote-entry guard rejects a second entry for an approval-protected
  marker whenever any distinct remote entry already exists. Its existing run/execution
  locks serialize competitors. This persists the invariant against raw SQL, while all
  current fence, proof and conflict checks remain intact.
- No changes to evidence validity, evidence consumption, receipt history, or the
  workflow_action_not_applied_available predicate. No new table or interface.

## Verification

Focused tests must cover: first approved entry succeeds; same-attempt competitors have
one winner; after a real not-applied proof and next claim the protected action is not
sent again; policy removal cannot allow a second entry; raw second-entry insertion
fails even with otherwise-valid proof; an existing receipt is still returned without
I/O. Rerun affected action replay/reconciliation/receipt tests and the 05B gate.

Review only D7 at this gate. The 05B implementation artifact remains WIP; full code
review follows completed tests and a frozen snapshot.
