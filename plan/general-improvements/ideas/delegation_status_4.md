# Delegation and Waiting Status

## Expected result

Users can see the complete collaboration state of a task: who owns it, which specialists or external parties were asked, what each request is waiting for, which results arrived, and what will resume the task. Nested delegation is understandable without exposing internal transport details.

The status is derived from durable task, outreach, target, and message records rather than maintained as a second inconsistent workflow model.

## Plan summary

1. Define a read model for task owner, active outreach, targets, completion threshold, deadlines, replies, nested waits, and resume state.
2. Map current `task_outreaches`, targets, outbox messages, internal channel deliveries, and background tasks into stable delegation statuses.
3. Add efficient persistence queries and indexes for a task collaboration summary and optional timeline.
4. Expose the read model through application/API boundaries without leaking provider or internal security metadata.
5. Add mailbox and task UI components for waiting-on chips, progress, deadlines, partial results, and nested specialist work.
6. Emit monitoring for stuck waits, unmatched responses, inconsistent target state, and overdue delegation.
7. Test ordinary, nested, partial-quorum, timeout, duplicate-reply, and failed-delivery paths.

## Scope decisions for the detailed plan

- Decide how much nested delegation detail ordinary teammates may view.
- Define status names in business language rather than worker-state terminology.
- Define when an internal delegate is shown as an agent, channel, or business function.
- Specify eventual-consistency expectations while delivery and resume operations are in flight.
- Decide whether the first release is read-only or includes status actions.

## Dependencies and sequencing

Consumes task ownership and message visibility. It should precede deadline/cancellation/reassignment controls because those actions need a trustworthy status surface.

## Acceptance signals

- A user can answer “who has this and what are we waiting for?” from one view.
- Status remains correct after retries and idempotent duplicate operations.
- A specialist response is visibly correlated with the request that caused it.
- Nested work does not imply that the specialist owns the original task.
- Queries remain bounded and indexed for mailbox-list usage.

