# Stop an agent's channel work when its assignment is removed

Status: proposed, not implemented. Written against the implementation on 2026-09-12.

## Outcome

When a manager removes agent A from channel C, commit the assignment change, stopping A's
affected tasks, and cancelling their queued deliveries in one database transaction. Preserve
tasks and their history. Tasks owned by A in other channels, and tasks owned by other agents or
people in C, remain unchanged.

A successful removal means that no new execution or publication can start from the removed
assignment, and no cancelled delivery can later be retried. It cannot recall a provider request
whose send was already authorized before the removal. Report those deliveries as potentially
sent, preserving their real outcomes.

This document covers explicit channel assignment edits. Deleting an entire agent, channel, or
company remains a separate lifecycle. The bounded message-to-task lookup from the database
review is also a separate change.

## Existing implementation to build on

| Area | Current code and relevant behavior |
|---|---|
| Authorized edit | `src/application/use_cases/channel.rs:418` — `update_channel` verifies the manager and tenant, and preserves an owned channel's mandatory owner agent. |
| Assignment replacement | `src/adapters/persistence/channel.rs:522` — `update` owns the transaction but deletes and reinserts **all** `channel_agents` rows. It does not stop their tasks. |
| Task stop | `src/adapters/persistence/task/queue.rs:191` — `stop_task_on` stops one task, cancels outreach and pending/retryable deliveries, supersedes harness runs, and expires harness approval waits. It opens and commits its own transaction. |
| Final dispatch | `src/adapters/persistence/task/operations.rs:1410` — `commit_agent_dispatch` locks outreach before task and verifies the current execution before publishing. |
| Ownership transfer | `src/adapters/persistence/task/ownership.rs:258` — changes ownership under a task lock, with channel-assignment eligibility checked by `principal_facts` at line 135. |
| Execution ledger | `src/adapters/persistence/task/harness_runs.rs:478` — `supersede_harness_runs_on` updates the checkpoint through the checkpoint API, rather than changing its projected SQL state alone. |
| Delivery execution | `src/adapters/persistence/delivery/queue.rs:53` — claims with row locks and execution IDs. Its completion, failure, release, and reaping paths own subsequent state changes. |
| Provider boundary | `src/application/services/delivery_worker.rs:193` — `send_part` commits `begin_part` before calling the provider. |
| Delayed publication | `src/adapters/persistence/response_review/commands.rs:223` — approving a saved draft can insert a delivery later, after the producing task has finished. |

Do not call the current `stop_task_on(pool, ...)` in a loop from channel update. Those independent
commits would leave stopped tasks behind if saving the channel later failed. Extract transaction-
accepting persistence helpers and retain the existing public stop operation as a wrapper.

## 1. Define exactly which work is affected

Read the old assignments inside the edit transaction. Compare normalized sets:

`removed_agent_ids = old_agent_ids - requested_agent_ids`

Reordering, adding an agent, or saving an unrelated channel field must not stop anyone's work.
Do not attach cancellation to every raw assignment DELETE: the current delete/reinsert strategy
temporarily removes retained agents too. Prefer updating the assignment set by its actual diff,
while preserving position uniqueness and the owned-channel position-zero invariant.

Resolve removed agents to their principals in the channel's company. A task is in scope only
when its `company_id`, primary `channel_id`, `owner_principal_id`, and `owner_principal_kind =
'agent'` match. Recheck these facts after acquiring the task lock; never act on a stale preview.
An agent merely appearing in a task's correlation chain is not sufficient.

| Task state | Removal behavior |
|---|---|
| `pending`, `processing`, `pending_approval`, `waiting_for_third_party_reply`, `failed`, `dead_letter` | Change to `stopped`; settle the associated live work below. |
| `stopped` | Keep the status; reconcile any remaining cancellable side effects without adding a duplicate stop event. |
| `completed` | Preserve completion and historical attempts. Cancel remaining queued deliveries and supersede pending publication/review work. |

Completed tasks matter: final dispatch can commit immediately before removal and leave a reply
in the outbox. Selecting only unfinished tasks would miss that reply. Find completed candidates
through their remaining live deliveries or pending drafts/reviews, rather than loading every
historical completed task into Rust.

Cancel all task-linked delivery destinations of an affected task, including its mirrors and
outreach messages. Do not restrict those rows to destination channel C: a task originating in C
can have queued output elsewhere. Do not stop independent downstream tasks in other channels.
Conversely, a task anchored in another channel is not selected merely because C is one of its
secondary `task_channel_targets`; changing that rule would require a separate fan-out policy.

Preserve the owner ID and ownership history. Re-adding an agent does not automatically resume
old tasks or revive cancelled deliveries. An explicit resume/transfer must validate the current
assignment and use the normal execution/ownership fencing.

## 2. Serialize removal with new work and publication

Task row locks settle races over existing tasks, but cannot prevent a concurrent transaction
from inserting another task or assigning a different task to the removed agent. A channel edit
lock by itself does not protect these writers today.

Introduce a transaction-scoped shared/exclusive advisory lock for a namespaced
`(company_id, channel_id)` key:

- Assignment changes acquire the exclusive lock before reading old assignments or locking the
  channel row.
- Writers that admit work through an assignment acquire the shared lock before their first
  mutable row lock, then reread assignment eligibility. Cover inbound and scheduled task
  creation, manual task starts, ownership transfers to agents, resumes, and delegated work.
- Final dispatch, creation/approval of review drafts, and other task-linked delivery producers
  participate before acquiring their existing outreach/task/draft locks. After removal commits,
  stale publications must fail their eligibility or supersession check, including publications
  based on a previously completed task.
- Multi-channel operations acquire all required channel keys in stable `(company_id, channel_id)`
  order. Determine keys with read-only lookups first and revalidate the rows after locking.

Under READ COMMITTED, reread eligibility in a new statement after obtaining the advisory lock;
do not combine a potentially waiting lock acquisition and the eligibility read into one stale
statement snapshot. Any stronger isolation mode must explicitly handle serialization retries.

This gate belongs at the outer transaction boundary, not inside `insert_task` after callers have
already acquired locks. Inventory all callers of `insert_task`, `insert_delivery_on`, ownership
commands, approval/resume paths, and review publication before implementing it. The graph has
ambiguous `insert_task` symbols, so use its exhaustive identifier search as well as call tracing.

The rule for existing locks remains **outreach before task**. Preserve stable ordering within
each relation and trace the harness, approval, review, and delivery paths before deciding their
remaining order. Do not hold a task and then attempt to acquire a channel gate. Include SQL
triggers in this audit: `lock_task_agent_harnesses` takes shared agent-row locks on task updates,
and agent/channel lifecycle operations have their own locks. A new channel gate must not create
an agent-lock/channel-gate inversion with those paths.

Global task claims can keep their current selection architecture: they compete on the selected
task rows, repeat pending eligibility on the locked version, and do not create new ownership.
If a claim wins first, removal observes and fences its execution. If removal wins, the claim
must skip the stopped row. Prove both orders with the production SQL.

The advisory protocol must cover every application writer; merely introducing a lock helper is
not protection. Concurrent direct SQL that ignores that protocol is not a supported mutation
interface. During rollout, drain old application instances before enabling assignment removal,
because an older writer does not acquire the gate or honor cancellation markers.

## 3. Make delivery cancellation durable

Reuse `dead_letter` plus `last_error_class = 'superseded'` for known-unsent cancelled deliveries;
do not invent a second queue status solely for this flow. Add a durable cancellation intent to
`message_deliveries`, provisionally:

- `cancellation_requested_at timestamptz NULL`;
- `cancellation_reason text NULL`, represented by a typed Rust reason such as
  `channel_agent_removed` and `task_stopped`.

Require both fields together with a NULL-safe CHECK, and constrain allowed reasons. Existing
rows start with both NULL. Preserve the first cancellation intent; ordinary retry, completion,
and reaping updates must never clear it. Carry it through the row struct, shared projections,
domain record, enqueue/read paths, and test doubles.

Under the parent delivery lock:

| Delivery state | Action |
|---|---|
| `pending`, `retryable` | Set cancellation intent and terminal `superseded` state. Preserve delivered parts, provider keys, and actual attempt counts. Mark only remaining known-unsent parts dead. |
| `sending` | Set cancellation intent, preserving the current execution ID, worker lease, and provider outcome fields. Report it as potentially sent. |
| `outcome_unknown` | Set cancellation intent and retain uncertainty. Do not relabel it as definitely unsent. |
| `delivered` | Preserve the outcome; it cannot be recalled. |
| `dead_letter` | Preserve existing evidence and idempotent cancellation state. Do not revive it. |

Every delivery claim and `begin_part` checks cancellation intent. A worker with an existing
lease cannot start an additional part after cancellation wins the parent-row lock. Teach the
worker a typed cancellation outcome so it stops without reporting a database failure or leaving
a lease stranded. An already authorized provider call may still finish: record that result under
its existing execution fence, including confirmed delivery or uncertainty.

Failure, shutdown release, lease reaping, and any retry/redrive path must consult the marker:
known-unsent work terminates as superseded instead of becoming retryable; an ambiguous provider
outcome remains ambiguous and is never blindly resent. If some parts were already delivered,
retain that partial-delivery evidence while cancelling the rest. Re-adding the agent does not
clear this intent. A deliberate new publication creates new work under the normal idempotency
contract, rather than recycling a cancelled delivery.

This closes a hole that a one-time `UPDATE ... WHERE status IN ('pending','retryable')` cannot:
an in-flight delivery can otherwise fail **after removal** and become eligible to send again.

## 4. Commit the channel change and cleanup together

After acquiring the exclusive channel gate:

1. Lock and reread the tenant-scoped channel and old assignments. Validate the requested set,
   compute removed principals, and establish the bounded affected working set.
2. Acquire related outreach locks, then task locks, in deterministic order. Revalidate current
   ownership and status. Final-dispatch or ownership-transfer results committed before these
   locks determine what removal actually changes.
3. Stop the six stoppable statuses. Clear worker/execution lease fields and obsolete wait
   pointers/deadlines consistently with the queue state machine. Write a typed
   `channel_agent_removed` transition reason and the initiating manager's actor ID; do not
   reconstruct the reason from an error string. Preserve ownership versions unless ownership
   actually changes.
4. Close exactly the processing attempt belonging to the captured pre-stop execution generation
   with a typed stop reason. Leave completed attempts and older generations unchanged. Revoke
   live harness work through its checkpoint transition API, keeping SQL projections and stored
   checkpoints consistent.
5. Cancel live outreaches and active targets; preserve responded target history. Expire pending
   human approvals and harness approval waits, and supersede pending drafts/reviews that could
   publish later. Their consume/approve paths must reject stale commands after this transaction.
6. Mark/cancel the affected deliveries using section 3, including outbox rows of completed tasks.
   Reconcile existing actionable notifications so cancelled work is not presented as a new
   operational failure. Do not create new reminders capable of restarting the stopped work.
7. Apply the complete channel edit and assignment diff. Commit once. Any error rolls back both
   the edit and every cleanup mutation.

Use set-based statements for task, attempt, outreach, and delivery updates. Decode checkpoint
documents in bounded batches inside this same transaction where Rust transitions are necessary.
Do not split the edit into separately committed batches or detach cleanup into a background task.

Return a named result with the updated channel, stopped-task count, cancelled-delivery count,
superseded-review count, and deliveries that may already have been sent. Pass the actor and
tenant in a named persistence request from the existing authorized application use case; update
every trait implementation and test double. Show a short result on the existing edit flow.
Repeated saves with no removed assignments must be no-ops for task/delivery state and audit
events. Do not add a second confirmation flow as part of this change.

## 5. Bound the interactive transaction

Atomicity across an arbitrary amount of history cannot promise constant work. The operation must
either finish within an explicit budget or fail with the assignment unchanged.

Proposed initial ceilings: 10,000 affected tasks, 100,000 dependent rows to mutate, 100 decoded
checkpoints per Rust batch, a 2-second lock timeout, and a 30-second overall operation deadline.
Use limit-plus-one queries to detect cardinality overflow, not an unbounded Rust collection.
Count dependent work too: a task cap alone does not bound its delivery/attempt/review history.
The overall deadline must cover the entire transaction, with explicit rollback on failure;
per-statement timeouts alone allow a long series of statements to run indefinitely.

On overflow or timeout, return a controlled error identifying that the removal did not happen.
Never silently stop only the first N tasks. Calibrate these proposed ceilings before release and
keep boundary tests in CI; raising one requires recording what still fails early. If real usage
requires larger removals, design a durable staged removal protocol separately, with a visible
removing state and resumable cleanup, rather than weakening the atomic contract unnoticed.

Start with existing unsettled-owner/channel and delivery-task indexes. Capture plans for both
unfinished tasks and completed tasks with queued output. Their partial-index predicates differ;
do not assume an unsettled-task index covers the completed branch. Add or change an index only
with evidence, following the repository's query-evidence rules. This is independent of the new
index proposed for the message-to-task lookup.

## Implementation sequence and acceptance

1. Add the cancellation intent and typed reasons, including all worker and retry guards. Extract
   transaction-accepting stop/cancellation helpers without weakening the current stop behavior.
2. Establish the channel gate across admission, transfer/resume, and publication transactions.
   Add the concurrency tests before wiring removal to it.
3. Wire the assignment diff, bounded cleanup, and result into the existing channel edit. Add
   completed-task outbox cleanup and pending review/approval supersession.
4. Validate both fresh installation and upgrade using additive migrations, per the persistence
   `AGENTS.md`. Regenerate SQLx metadata. Drain incompatible old writers before rollout.

Database tests must exercise real competing transactions, using `own_database` for global claims
or sweeps. Synchronize races with explicit barriers/locks rather than hoping timing exposes them.

Required cases:

- Every task status; two companies; the same agent in two channels; another agent and a person
  in the edited channel; retained/reordered agents; an owned channel's mandatory owner.
- Pending, retryable, sending, delivered, uncertain, and terminal deliveries, including a
  multi-part delivery with one part already delivered. Cover email and Slack queue behavior.
- Removal versus task claim, task creation, ownership transfer, resume, final dispatch, outreach
  creation/reply, and approval/review publication. Test both winners, including completed-task
  output created immediately before removal and stale publication attempted immediately after.
- Removal versus delivery claim, `begin_part`, provider-result persistence, failure/retry,
  shutdown release, and expired-lease reaping. No double send, overwritten provider evidence,
  resurrected cancellation, or deadlock. A crash after cancellation commit retains the fence.
- Exactly the current processing attempt is closed; superseded generations cannot later write.
  No duplicate transition/notification events on retries or unchanged assignment saves.
- Inject failure after task stopping, after delivery cancellation, and during assignment writes:
  all channel/task/delivery/approval changes roll back together.
- Limits and deadlines fail without partial cleanup. A representative 5,000-task backlog is
  removed from runnable work without scanning every historical completed task into memory.

Run formatting, offline compilation, Clippy, migrated database tests, and SQLx metadata checks.
Record structured duration/count metrics and timeout/overflow outcomes without logging message
bodies or addresses. Completion means that cancellation intent, execution fencing, publication
guards, and the channel edit ship together; a successful UI save followed by eventual best-effort
cleanup is not this feature.
