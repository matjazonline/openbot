# Task Ownership and Transfer

## Summary

Introduce task-level business ownership, distinct from worker leases and delegation. Every task
has one owner state: a human principal, an agent principal, or explicitly unassigned. The mailbox
derives its displayed owner from the active task driving the thread.

Transfers are database operations, never messages to another agent's address. They preserve the
original task, correlation chain, thread metadata, approvals, outreach, and delegated results. A
mandatory private handoff instruction tells the new owner what to do.

## Domain, Persistence, and Interfaces

- Add `TaskOwner` (`Human`, `Agent`, `Unassigned`) and
  `TaskOwnership { owner, version }` to `BackgroundTask`.
- Add `owner_principal_id` and monotonic `ownership_version` to `background_tasks`. Enforce tenant
  matching and permit only person or agent principals.
- Backfill existing tasks from the position-zero agent on the task's primary channel; use
  `Unassigned` when no valid principal can be resolved. Record migration-origin initial ownership
  events.
- Add immutable `task_ownership_events` containing:
  - task/company, sequence, ownership versions, and idempotency command UUID;
  - operation: initial assignment, claim, transfer, release, or owner removal;
  - actor, previous owner, and new owner snapshots;
  - typed reason, bounded optional detail, mandatory transfer handoff instruction;
  - timestamp.
- Bound reason detail to 512 bytes and handoff instructions to 8 KiB. Keep handoffs outside
  canonical messages so they cannot enter customer-visible history or delivery accidentally.
- Add application operations for `claim`, `assign`, `transfer`, and `release`. Every command
  carries `expected_ownership_version` and a stable command UUID:
  - Repeat of the same command returns its original result.
  - Reuse with different parameters is a conflict.
  - A different command against a stale version returns a version conflict.
- Extend `TaskLeaseRef` with the claimed owner and ownership version. Claims, lease renewal,
  outreach/approval writes, agent dispatch, failure, and completion must match both execution
  generation and ownership version.
- Replace mailbox-only `ThreadActivity` lookups with a `ThreadWorkSummary` containing the
  activity-driving task ID, ownership, and activity. The thread owner is the owner of the same
  most-recent unfinished task currently used for activity selection.
- Add browser endpoints for claim, assign, transfer, release, and human completion. Return conflict
  feedback in-place for stale forms. Add an owner-only agent tool for transfer/release; its
  idempotency key derives from the task execution and tool-call ID.

## Ownership and Execution Behavior

- Initial owner:
  - New inbound and scheduled tasks are assigned to the position-zero agent of their primary
    channel.
  - Approval and outreach resumes retain the existing owner.
  - Failure and retry never change ownership.
  - A task without a resolvable eligible agent is unassigned and cannot be claimed by a worker.
- Eligible targets:
  - An agent must be assigned to the task's primary channel. Transfer changes only this task; it
    does not reorder channel configuration.
  - A human must be an active company teammate who may view the task's channel/thread.
- Authorization:
  - Company owners/admins may assign, transfer, release, or claim any company task.
  - The current human or agent owner may transfer or release its task.
  - A teammate who can view the thread may claim an unassigned task for themselves.
  - Nobody may take work away from another owner unless they are a company owner/admin.
  - Owning agents may autonomously transfer to eligible agents or humans.
- Every owner-to-owner transfer requires a non-empty private handoff instruction. Claims and
  releases use typed reasons without a handoff.
- Agent transfer:
  - A pending task becomes claimable for the new agent.
  - A processing task is atomically returned to pending, its lease is revoked, and its open attempt
    ends with `ownership_transferred`.
  - The old run is cancelled through an identifier-only ownership-change wake-up; missed wake-ups
    remain safe because lease renewal and every commit are fenced.
  - The new owner replaces the primary channel agent for this task only. Other channel agents
    remain pipeline contributors and cannot become owners implicitly.
  - The new owner's prompt receives its own configuration, the original request and thread
    history, outstanding delegation results, current wait state, and the latest private handoff
    instruction.
  - A successful transfer ends the old agent run immediately without producing a response.
- Human transfer:
  - Workers exclude human-owned and unassigned pending tasks.
  - The mailbox shows a dedicated owner response composer. Submission creates a human-authored
    outbound message, reuses the task's original thread/recipient metadata and response delivery
    planning, queues delivery, and completes the task in one ownership-fenced transaction.
  - Duplicate submissions return the existing completion and cannot enqueue another delivery.
- State rules:
  - Transfers during approval or outreach waits preserve that state and all related records; the
    new owner acts only when the wait resolves.
  - Transferring stopped, failed, or dead-lettered work changes ownership but does not resume it.
    Resume remains explicit.
  - Completed tasks retain their final owner and reject ordinary ownership mutations.
  - Human completion is unavailable while stopped or while an approval/outreach wait is active;
    resolving or cancelling those states remains a separate action.
  - Transfer racing with final dispatch locks the task row: either dispatch commits first and
    transfer is rejected as already answered, or transfer commits first and the stale dispatch
    writes nothing.
- Removing or disabling an owner uses the same transactional ownership service to release active
  work, revoke any lease, and record `owner_removed`. Foreign keys prevent bypassing that cleanup.
  Terminal tasks may clear the live reference while retaining immutable owner snapshots in
  history.
- Quiet-message behavior remains unchanged:
  - `[quiet]`, quiet addressing, and the mailbox quiet option file context without creating a new
    task.
  - Quiet messages never claim, release, transfer, resume, or complete ownership.
  - A quiet message added to a thread with an active task leaves that task and its waits unchanged.
  - Transfer handoff instructions are ownership metadata, not quiet messages.
  - The human-owner final-response action does not parse `[quiet]`; filing context and publishing
    externally are separate controls.

## UI and Notifications

- Mailbox thread rows and detail panes show the current owner and distinguish "agent working,"
  "assigned to Alice," and "unassigned."
- Thread viewers see the owner and may claim visible unassigned work. Only the current owner and
  company managers see the private handoff instruction.
- The manager-only Tasks workspace gains owner filters, owner chips, assignment controls,
  ownership version conflict handling, and ownership events in the chain timeline. Operational
  payloads remain hidden from ordinary members.
- The human-owner composer is visually separate from quiet context entry and clearly states that
  submission sends externally and completes the task.
- Ownership events trigger existing SSE reconciliation for the affected thread and task chain.
  Assigning an agent to a pending task also wakes the worker queue.
- Add `user_notification_preferences` with `task_assignment_email_enabled`, defaulting to `true`,
  and expose the toggle in profile settings.
- A transfer to another human atomically queues one durable assignment email unless the recipient
  disabled it or assigned themselves. The email contains a secure mailbox deep link and minimal
  task/company identification, but not the private handoff text.
- Reuse the existing delivery queue and delivery metrics; do not build the broader notification
  center from the later notifications plan.

## Test and Rollout Plan

- Database tests:
  - Tenant and principal-kind constraints, ownership-version increments, immutable history,
    bounded handoffs, backfill, and owner-removal cleanup.
  - Two simultaneous claimants produce exactly one owner.
  - Retried command UUIDs are idempotent; stale versions and command-payload mismatches conflict.
- Execution tests:
  - Transfer versus agent dispatch proves exactly one outcome and no duplicate reply.
  - A stale worker cannot renew, write a tool effect, store a response, or queue delivery.
  - Agent A transferring to B ends A's run; B receives the handoff and replies as B on the original
    thread.
  - Human completion atomically stores one message, one logical delivery, and one completed
    transition.
  - Approval/outreach transfers preserve waits and delegated results.
  - Stopped/dead-lettered transfers do not resume automatically.
  - Quiet messages leave ownership and task status unchanged and enqueue neither a replacement
    task nor a delivery.
- Authorization/UI tests:
  - Manager, owner, ordinary teammate, unassigned claimant, restricted-channel, and cross-company
    cases.
  - Owner badges and controls reconcile through SSE without leaking handoff content.
  - Assignment email respects preference, self-assignment suppression, and retry deduplication.
  - Quiet context and human final-response controls cannot be confused or submitted through each
    other's endpoint.
- Verification includes formatting, offline compilation, migrations, SQLx metadata regeneration,
  the database-backed suite, competing-claimant tests, and the stock-stack budget because the
  worker supervision path changes.
- Stop all workers, reset the database, apply the squashed baseline, and deploy ownership-aware
  workers. Ownership controls are always available, so mixed deployments with workers that ignore
  ownership fences are unsupported.

## Assumptions

- This implements ownership only, not the full message-visibility or general notification plans.
- "Channel agents only" means agents assigned to the task's primary channel at transfer time.
- Agent-to-human transfers are autonomous, as selected, but remain fully auditable and
  preference-aware.
- Delegation/outreach continues to request input without changing ownership.
- Existing `[quiet]` compatibility behavior remains intact and orthogonal to ownership.
