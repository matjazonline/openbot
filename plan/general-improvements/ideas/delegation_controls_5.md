# Delegation Deadlines, Cancellation, and Reassignment

## Expected result

Authorized humans and owning agents can recover stalled delegated work without duplicating customer
replies or pretending that externally sent messages can be recalled. Every operation has explicit
authority, a version fence, a stable command UUID, typed actor/reason data, and an auditable result.

## Lifecycle and deadline decisions

- Define and database-test the outreach and target transition matrices using the business statuses
  from the read model. `Cancelled`, `Superseded`, `Responded`, and `Expired` are terminal for quorum
  contribution.
- Keep the current 96-hour default outreach timeout and 720-hour maximum in V1 for both internal
  and external targets. Allow an explicit per-outreach deadline within the configured maximum;
  collect elapsed-time data before introducing different defaults.
- Keep the owning task's operational due time separate from outreach expiry and individual-target
  delivery state. Extending outreach updates its expiry and maintenance scheduling but does not
  silently change the task's business due time.

## Command behavior

- Add versioned commands for `ExtendOutreach`, `CancelTarget`, `CancelOutreach`,
  `ReassignInternalTarget`, `ProceedWithPartial`, and `StopTask`. Repeating the same UUID
  returns its original result; changed parameters or a stale version conflict.
- Human task owners and company managers may perform all commands. Owning agents may extend within
  policy and cancel/reassign internal delegation they created. Proceed-with-partial and stopping
  the owning task remain human-owner/manager actions in V1.
- Cancelling internal work revokes a pending/processing child task through its execution and
  ownership fences. If the child already completed, its result wins and cancellation reports a
  conflict rather than rewriting history.
- For an external target, cancel an unclaimed delivery when possible. Once sending, delivered, or
  outcome-unknown, cancellation means only "stop waiting for this target" and the UI must state
  that the external message may already have been received.
- Reassignment is available only for internal targets. It marks the old target `Superseded`, creates
  a new correlated request/child task, and preserves both histories. The old target can no longer
  satisfy quorum.
- Proceed-with-partial records the exact responses available at the decision version and resumes
  once. A response racing with the command either commits before the locked snapshot and is
  included, or commits afterward as late internal context.
- Late or duplicate replies are retained as `InternalOnly` delegation context, correlated to the
  original target, and never reopen or complete closed work.

## UI and operations

- Expose controls only from the collaboration detail and operational work-item surfaces. Each form
  includes expected version, command UUID, reason, and a preview of delivery/task consequences.
- Distinguish `Cancel waiting`, `Cancel unsent delivery`, and `Stop task`; never label any of them
  "recall email."
- Produce authoritative work-item changes for decisions that need a human. Notification delivery is
  deferred to the final notifications plan.

## Test and acceptance plan

- Database-test every allowed and forbidden transition plus actor/reason attribution and command
  idempotency.
- Add competing tests for cancel versus delivery claim, cancel versus internal completion,
  reassign versus response, extend versus timeout sweep, proceed-partial versus response, and stop
  versus final dispatch.
- Prove cancelled/superseded targets cannot satisfy quorum, old workers cannot commit after internal
  cancellation, and extension cannot create duplicate task execution.
- Verify late replies are preserved privately, provider-unknown outcomes require a decision, and no
  recovery path produces a second customer reply.
