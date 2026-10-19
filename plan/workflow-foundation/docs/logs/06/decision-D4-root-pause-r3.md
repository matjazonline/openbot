# 06-D4 revision3 addendum — shared allowance pause scheduling

Status: PROPOSED, not accepted. Extends accepted revision2 in decision-D4.md;
all revision2 ownership, approval, admission, settlement and proof requirements remain.
This gate concerns only sibling progression while the shared allowance awaits review.
Existing data is disposable; no compatibility layer or migration rollout is introduced.

## Required behavior and discovered gap

The phase06 user requirement is that an optional workflow token limit pauses automated
progression for manual approval, retaining usage and saved execution state. D4 revision2
already gives the model owner an exact checkpoint/decision wait and forbids subsequent
model/tool/output progression after refusal. A sibling can cross the shared allowance
while another local execution is already pending or processing. Simply rejecting that
sibling's effect/completion can enter its ordinary retry/error route; a SQL progression
guard can then reject that route indefinitely. There must be bounded scheduling behavior,
not a hot loop and not an error successor around the gate.

## Proposed extension

Use the existing pending job schedule, local attempt ledger and hard retry bounds. Do
not introduce another queue, lease, root run lock, or root-wide child mutation.

1. Under local run/execution/job locks and then the shared root usage lock, classify
   progression as Open, AwaitingAllowance, or Refused(TokenRefusal). The allowance check
   uses the existing settled/unconfirmed ledger and authorized total. Hard caps remain
   independent. Checks never refund unknown requests or authorize provider entry.
2. A pending execution that is AwaitingAllowance stays pending with the same checkpoint,
   retry/continuation/attempt counters and no new activation/model debit. Advance its
   existing run_at to min(now + 5 seconds, run deadline). Return no runnable work; polling
   must honor run_at. Deadline handling retains the existing terminal deadline path.
   Repeated observations cannot form a zero-delay poison batch. This is a bounded poll
   of shared authorization, not a successful continuation or fabricated attempt.
3. Pending Refused executions retire through the existing local pending-failure owner,
   carrying the exact saved refusal code, never an error successor. New claims recheck
   after acquiring the shared lock. Approval changes only the root authorized total and
   its original parked job; siblings become eligible on their next bounded poll.
4. A processing sibling can lose eligibility after claim. Before new effect entry or
   output commit, AwaitingAllowance becomes an explicit retryable token-gate failure.
   Retire its current real attempt through existing recovery, preserving actual effect
   safety. This interruption consumes the existing failure-attempt debit; it does NOT
   create a successful-continuation witness or exempt the hard retry bound. Unknown
   effects remain reconciliation-owned. If retries are exhausted, terminalize without
   error routing. Otherwise use ordinary retry backoff; once pending, rule2 prevents
   further attempt consumption until the shared gate opens. The model owner's own
   checkpoint-bound approval pause still uses D4's successful continuation protocol.
5. Refused processing owners retain received model/action truth but cannot progress;
   recovery preserves the exact refusal and cannot route/retry around it. SQL guards
   block new effects, new model admissions, execution completion and operator reopen.
   Terminal rejection/expiry may temporarily move run state through running inside its
   transaction; deferred reopen guards validate the final committed owner state.
6. Native model admission retains D4's more specific behavior: a still-unconsumed
   prepared request reparks with a new exact proposal when sibling usage exhausts an
   approved total. It must not be replaced by the generic pending delay. A saved response
   that needs review retains its turn and parks as before. The generic gate handles
   sibling execution scheduling and late progression races, not model request approval.

## Bounds and failure handling

The 5-second deferral is fixed, not another configuration option. Work stays bounded by
run deadline and existing hard attempt/continuation limits. Database/lock/notification
failures propagate and do not masquerade as approval/refusal. No root failure flag is
cleared by retry, replay, operator action or late approval. If review cannot be offered,
the model owner reports reviewer-unavailable; siblings cannot silently progress and will
be bounded by their existing deadlines unless a valid approval becomes available.

Existing retry safety, including action receipts and unknown remote effects, remains
owned by the current recovery predicate. Do not equate the absence of a new effect in
this failed operation with proof about older attempts. No handler-originated text is
accepted as the gate classification: the persistence boundary derives it under locks.

## Affected contracts and proof

Extend the current token budget classifier and pending/recovery writers. Entry/completion
checks consume the typed classification. Pending polling must use the persisted due time;
no new wait subject or approval identity is introduced. Add SQL/runtime parity wherever
retirement guards inspect recovery classifications; exact refusal codes remain distinct
from hard model-call exhaustion.

Required tests: sibling crossing between claim and effect/output; pending deferral twice
without time advance with no extra claims/debits; approval opens on next poll; denial and
expiry retire without error successors; deadline while deferred; one technical interruption
charges exactly one existing failure attempt; last allowed attempt does not get a retry
exemption; saved receipt/unknown-effect safety unchanged; root lock contention and no parent
run lock; raw SQL/operator bypass refusal. Rerun affected polling/lease/recovery/action/
completion suites at stock2MiB and four threads, then existing full06B2b/phase gates.
