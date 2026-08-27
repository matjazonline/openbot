# Delegation Deadlines, Cancellation, and Reassignment

## Expected result

Authorized users and owning agents can manage outstanding delegated work. They can set or extend a deadline, cancel an unanswered request, reassign internal work to another specialist, and choose how the owning task proceeds with partial results. Late or duplicate replies are retained safely as context but cannot unexpectedly reopen or complete closed work.

All actions are race-safe, auditable, and compatible with task retries and existing timeout behavior.

## Plan summary

1. Define outreach and target lifecycle states, terminal-state rules, and allowed transitions.
2. Specify deadline semantics separately for the owning task, an outreach, and individual targets.
3. Add use cases for extend, cancel, reassign, proceed-with-partial, and stop-task with authorization and idempotency.
4. Make maintenance and reply-correlation paths safe when actions race with delivery, timeout, or incoming replies.
5. Define reassignment as a new correlated request while preserving the cancelled target and audit history.
6. Add UI actions with confirmation, reason capture, and a preview of consequences.
7. Add notifications and monitoring for approaching deadlines, overdue work, cancellations, and failed reassignment.
8. Test transition matrices, concurrent operations, late replies, partial quorum, and nested delegation.

## Scope decisions for the detailed plan

- Decide which actions agents may take autonomously versus requiring human approval.
- Define whether cancellation can stop work already running in another agent or only detach the caller's wait.
- Specify treatment of externally sent email, which cannot truly be recalled.
- Define default timeout policy by internal versus external target.
- Decide when partial context is sufficient to resume automatically.

## Dependencies and sequencing

Depends on the delegation status read model and task ownership. Notification behavior should be coordinated with plan part 7.

## Acceptance signals

- Every state transition is validated, idempotent, and recorded with its actor and reason.
- A cancelled or reassigned target cannot satisfy quorum accidentally.
- A late response is preserved but does not silently reopen a completed task.
- Extending a deadline updates worker scheduling without duplicate task execution.
- Users can safely recover from a stalled specialist without creating duplicate customer replies.

