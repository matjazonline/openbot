# 04.7 terminal revision coverage: reachable owner histories

Architecture decision for row2a only; pending Sol independent check. Original
`BRIEF-04.7.md` §§1/5, lines194–240/294–299 and execution item6 in
`04-actions-http-and-delivery.md` remain authoritative. The temporary terminal
inventory is proposed test tactics, not another requirement. Baseline
`9cff5164fa0694a4e7664f6ff8c69e473d4c2a97`; preserve all current WIP. No04.8 expansion.

## Owner boundary and ordinary Failed constructors

`recovery.rs:21–96` replaces supplied safety with `workflow_action_retry_safe`
when action history exists. `domain/workflow/recovery.rs:83–118` selects Reconcile
before terminal failure/deadline checks for unknown effects. Thus a terminal-class
`release_io` cannot manufacture ordinary Failed while this execution's action
history remains unresolved. Do not change production policy, force safety, write
raw terminal state, disable triggers, or fabricate provider truth to meet a matrix.

Use two independent genuine histories (all paths use existing production owners):

1. **Receipt history:** admit/claim one Reconcile action, invoke the actual ledger
   provider with valid result, `recover=true, lose=false`, and assert Committed
   receipt/one actual effect. Before `complete_io`, call `release_io` using the live
   fence with Terminal failure. Use a source without a `final_error` route, so
   `route_failure` (`recovery.rs:189–238`) returns false and ordinary retirement
   fails the run without committing output/route/successor. Assert run Failed,
   failed lease-free job, exact retired attempt, future deadline, usable receipt
   preserved and shared action safety true. The constructor is the first part of
   `completed_action` (`action_reconciliation_eligibility_history_tests.rs:25–68`)
   followed by release instead of completion; `recovery_tests.rs:77–130` exercises
   the terminal release owner and no-route branch.
2. **Final-proof history:** real Pending provider entry → park through retirement →
   real provider quiescence barrier → authoritative final reconciliation that
   schedules the existing job → real `new_claim`. Before any new remote entry,
   release that live claimant with Terminal failure and no final_error route.
   Assert Failed, future deadline, original entry/proof preserved, proof still
   unconsumed, no extra provider call/effect/entry. This builds on
   `action_reconciliation_provider_tests.rs:190–260`, stopping after new_claim
   and replacing its new provider invocation with terminal release. Claims reserve
   attempts/budget normally; fixture setup is not counted as command mutation.

For receipt history, test fresh terminal UnknownNote and fresh authoritative same
Applied (receipt=true), preferably separate fixtures. Unknown is an audit statement
of uncertainty, not a retraction of the known receipt. Applied must preserve the
existing receipt exactly, not replace its provenance or pretend it was newly
recovered after terminalization. For proof history, test fresh authoritative
FinalNotApplied, preserving the existing unconsumed proof and provider barrier.
These are the ordinary-Failed reachable truth families for this bounded fixture.

Do not request recovered Applied on the same finally-absent covered entry: the
barrier prevents that effect. A later real permitted entry would be a different
history and is not needed here. Final absence on an already receipted invocation
is a conflict, not the nonconflicting terminal Final case. No forced-verdict
verifier should stand in for these positive truth histories.

## Other terminal histories retained in this substep

- **Deadline-expired Failed:** actual `expire_run` after a due deadline, retaining
  unresolved provider history. Refresh expected revision after the owner acts,
  then attach truthful Unknown / recovered Applied / final absence from independent
  ledger histories. A stale verification losing to expiry alone is not this case.
- **Succeeded:** actual provider receipt followed by fenced `complete_io` ending
  the run. Fresh UnknownNote and same Applied retain all committed history. Reuse
  `completed_refusal:239–288` for its genuine success history, extending missing
  stale/refusal replay assertions; do not reinterpret conflicting final absence.
- Waiting/Cancelled cases remain their accepted real owner histories. Recovered
  Applied inserting a new terminal receipt is exercised where reachable, including
  expiry/Cancelled; it is not a Cartesian-product requirement for every owner.

## Exact audit/revision acceptance for each fresh terminal command

Capture the full public-table snapshot **after** genuine terminalization and build
the command against that current revision. Assert the expected terminal state and
unchanged deadline/attempt/job/committed execution history before submission.

- Result is respectively UnknownRecorded, AppliedRecorded{receipt:true}, or
  NotAppliedRecorded; never Scheduled. Locate the new immutable command by key,
  not array index (proof construction already created a command). Require result
  revision = saved command result_revision = current run revision, greater than
  expected revision; never assume expected+1. Match returned evidence ID, outcome,
  request expected_revision, exact scoped evidence/coverage and actor audit links.
- Compare append deltas, retaining every pre-existing evidence/command/audit row.
  Each case adds its linked truth/coverage/audit/command; receipt history adds no
  receipt and preserves existing receipt bytes/provenance. Only genuinely newly
  recovered Applied histories add the expected receipt. Normalize exactly these
  justified additions and the generated run revision, then demand whole-public
  snapshot equality. No conflict, scheduling, proof consumption, remote entry,
  effect, attempt/debit, output/route/successor or terminal-state mutation.
- Exact successful replay returns the saved evidence/outcome/revision, replayed=true,
  with whole-public equality. A different key retaining the old expected_revision
  returns RevisionConflict/current revision and no evidence/audit; the only append
  is its immutable refusal command with null evidence/audit/scheduled-job links.
  Require no revision bump and preserve all prior command rows. Exact refusal replay
  returns that same refusal and leaves the entire snapshot unchanged.
- A real subsequent `claim_io` returns None and leaves the full snapshot unchanged;
  no attempt, budget debit or provider entry is created. Existing provider ledger
  effects/call counts and proof consumption are unchanged across command/replays.

`action_reconciliation_revision_tests.rs:47–156` supplies the existing delta/replay
pattern, but its zero-receipt/first-command assumptions must not be copied into the
resolved-history cases. This is a fixture adaptation, not a production contract
change. Independent Sol checking and actual DB tests are still required; this
document does not establish implementation acceptance or waive original behavior.
