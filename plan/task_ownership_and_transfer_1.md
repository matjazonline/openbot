# Task Ownership and Transfer

## Expected result

Every active business task has exactly one accountable owner: a human, an agent, or an explicitly unassigned state. Teammates and specialist agents may contribute context or perform delegated work without accidentally taking ownership. An authorized actor can transfer ownership, and the transfer is durable, auditable, idempotent, and visible in the mailbox and task views.

A transfer does not create a duplicate customer request or cause both the old and new owner to reply. In-flight delegated work remains attached to the task unless deliberately cancelled. The final response is sent only by the current owner, using the original thread and reply metadata.

## Plan summary

1. Define the domain vocabulary and invariants for owner, contributor, delegate, transfer, claim, release, and unassigned work.
2. Decide whether ownership belongs to a task, thread, or both; prefer task-level ownership with a derived thread-level current owner.
3. Add persistent ownership and transfer-history records with optimistic concurrency or compare-and-swap protection.
4. Add application use cases for assign, claim, transfer, and release, including authorization and idempotency rules.
5. Update task execution so only the current agent owner may produce the final external response.
6. Preserve outstanding outreach and context across transfers while preventing the former owner from resuming execution.
7. Expose ownership state and transfer controls in task and mailbox UI surfaces.
8. Add audit events, monitoring, migrations, and tests for races, retries, stale workers, and transfer during a wait state.

## Scope decisions for the detailed plan

- Identify who may transfer human-owned and agent-owned work.
- Define whether an agent can transfer directly to a human and how that human is notified.
- Define the fallback when an owner is disabled or removed.
- Specify behavior for completed, failed, dead-letter, and approval-pending tasks.
- Keep delegation distinct from ownership transfer in APIs, UI labels, and audit history.

## Dependencies and sequencing

This is the foundation for the remaining plan parts. The private/customer-visible model should be defined alongside it so ownership controls can enforce who may publish externally.

## Acceptance signals

- At most one actor can successfully claim or own a task at a time.
- A stale worker cannot send after ownership has changed.
- Transfer history identifies actor, old owner, new owner, timestamp, and reason.
- Delegated results still resume the owned task, but do not change its owner.
- The current owner is unambiguous in API responses and the UI.

