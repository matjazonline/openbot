# AH-P03-D1/r2 — exclusive logical turns in the existing workflow owner

Proposed cohesive architecture for original03A/B, based on accepted02 source
c24664b2 / archivebfea3a87 and coordination6 C002/r5,C003/r2. No dependent code yet.
No backward compatibility; reset-only initialization and fresh owned databases.
This decision does not implement P04 automatic history selection or P06 retention.

## Persistent owner and ordering

Add nullable `active_exchange_id` to the existing conversation. A tenant/conversation-
qualified deferred FK binds it to that conversation's actual exchange. It is a
logical owner, not a process lease. An eligible Queued exchange can own the turn
before any checkpoint exists; `Active` continues to mean linked checkpoint work.
The existing workflow fence remains the execution owner. Do not add a second
lease, worker, provider abstraction, process mutex or independent runtime.

A native guard permits acquisition only when the pointer is empty and this is the
least durable position among Queued exchanges. Acquiring the already owned same
exchange is idempotent. A nonempty live pointer cannot be stolen, including by an
older Failed exchange reopened through the existing control owner. Once the
current owner terminalizes, the minimum *current Queued position* is eligible;
there is no greatest-completed watermark. Old reopened positions remain valid.
Revision, model, access level and lease generation do not split conversations.

Native agreement requires every Active exchange to own the pointer, and forbids
multiple Active exchanges in the same conversation (partial unique index plus
scope/pointer guards). Checkpoint open/reattachment and fresh execution/model
admission require the actual pointer owner as well as accepted02 scope/link/fence
proofs. Completed checkpoint/exchange replay needs no fresh turn and must not
reacquire or displace the current owner. Actual eligibility comes from the frozen
compiled agent.run step; classify/decision/sample paths remain isolated.

## Minimal bounded admission and lock order

Pending membership means Queued or Active. Limits are32 pending per conversation
and256 per company; both are server constants with native enforcement and positive
boundary tests. Run deadlines remain immutable, bounded by existing admission at
24h; there is no deadline extension or new wait-age clock. Existing continuation
limit63, retry/model/tool/token budgets and worker/poll bounds remain unchanged.

Serialize membership *increases* with one tenant-qualified admission-lock row per
company. This row contains only the company key; it is not a queue, counter,
worker or background service. Create lazily under the company FK, never delete or
retarget it while its company exists; whole-company teardown may cascade it.
Under its row lock, count committed Queued/Active rows through a new company-pending
partial index, then admit only below256. The conversation lock similarly protects
the count below32. Counts inspect at most the enforced pending set and no payloads.
Decreases need no tenant admission lock: a racing terminal commit can only make a
count conservative. No cached mutable count, reconciliation task or quota backfill.

The concrete protocol starts with existing global account/company/association/
agent authority wherever that operation checks it, then the **own run** lock that
serializes its source writers. Before the conversation, take the same-run subordinate
locks required by the operation: its execution/job/fence, its existing wait or
checkpoint, and its own existing exchange tuple. These are scoped to that locked
run; no unrelated checkpoint or job is acquired. New turn-wait resume explicitly
locks its own waiting job before reserving the conversation.

In particular, ordinary checkpoint open, execution/model admission, mutation and
model settlement follow source/fence → own checkpoint row FOR UPDATE (or insert)
→ own exchange transition → conversation. The existing own run lock serializes
checkpoint writers against run terminal/control owners; a terminal hook may read
its own checkpoint without taking a new checkpoint row lock. Preparation has no
checkpoint yet: after source/fence it locks the conversation, then the tenant
admission leaf for membership increase, then inserts the new own exchange. Native
control retry already owns its exchange tuple before conversation → admission leaf.
The accepted02 failed-row DELETE uses source SHARE NOWAIT instead of waiting in
reverse order. Do not add a late global agent/association lock after the own run.

The tenant admission lock is a leaf: while holding it, acquire no **new** source
run, job, checkpoint or conversation lock. Native insertion/counter/pointer checks
may reenter locks already held for this same source/conversation. Terminal release
takes its own conversation but never the tenant admission lock, and never locks a
successor or unrelated source run/job/checkpoint to wake it. Durable readiness,
not a cross-source release writer, makes successors eligible.

There is one explicit existing hierarchy exception to the otherwise source-local
protocol: bounded parent→child cancellation/guard cleanup may retain the parent's
conversation while the existing controls/guard owner locks and settles descendants.
It follows the existing ancestor→descendant order/depth/resource bounds; it is not
permission to walk an arbitrary queued successor. Accepted01 requires each child's
actual private branch, including same-agent children, so a child cannot need its
parent's shared conversation. The leaf order prevents the three-way cycle: a root
blocked on the parent's conversation has not yet acquired admission; the child can
finish its private conversation→admission work and release its source before the
parent's existing cascade proceeds. Keep a real parent-cancel/private-child/other-
root admission contender test. No new child or checkpoint owner is introduced.

Resource lifetimes are distinct. No DB transaction, connection, process mutex or
conversation lock survives into provider I/O or waiting for another turn. A blocked
Queued turn commits its native park and releases the workflow worker slot. A Ready
provider call intentionally occupies the existing bounded, supervised I/O worker
until completion/suspension; preserve its heartbeat, cancellation, deadline and
fairness ownership. This is the coherent interpretation of original03A2 alongside
its existing-owner/no-second-runtime rules, not a promise of zero worker occupancy
during real execution. Do not detach provider work or add another dispatcher to
satisfy that overbroad wording. Tests prove parked turns free slots and independent
Ready conversations use the existing bounded workers without held DB resources.

Every membership increase, including native Failed→Queued control retry, uses the
same caps. A transient full queue refuses retry atomically with a controlled
Conflict, retaining the Failed source/identity/order/checkpoint/counters and allowing
a later authorized command when capacity is available. It does not relabel the
source nonretryable, partially reopen it, allocate a later position, or widen retry
rights. The adapter performs the cap check under the same locks before its native
write; native guards remain mandatory for direct writes and contention. Resumable
failed records remain pinned under accepted02 even though they do not occupy a
pending slot until explicit reopen. Nonterminal approvals/effects keep their slot.

## Preparation disposition and parking

Extend the existing AgentPreparation result to an exhaustive application-owned
disposition: Ready(boxed AgentLaunch), Parked(WaitId), or AtCapacity. Model-only
work is always the existing Ready path without conversation allocation. Samples
keep their sealed nonproduction path. There is no trait default or caller override.
Update the one native implementation, production Rig caller and all36 current
source occurrences/17 files (mechanical fixtures plus real waiting cases).

The accepted02 bounded preparation transaction retains current global authority
before its local source fence. It builds the bounded original candidate, finds
or allocates the accepted conversation/exchange once, and under the conversation
lock acquires or checks the logical turn. It can only return Ready for its own
live pointer or a completed replay. Do not select history yet; P04 does that after
turn admission. Credential retrieval, checkpoint creation and provider entry occur
only after committed Ready. A capacity refusal rolls back new identity/order work
and returns AtCapacity before credentials/provider/checkpoint. The original source
frozen input remains durable; this is a refused agent admission, not an admitted
provider call. Rig records explicit terminal `agent.history_queue_full` with zero
model usage rather than pretending a provider request failed or silently retrying.

If another turn owns the conversation, insert one ordinary `workflow_waits` row
with reason `agent_turn`, deadline equal to the original run deadline, an exact
server exchange reference and parked attempt number. No event/human correlation,
reviewer, approval token or notification payload becomes a queue instruction.
The exchange FK restricts history deletion until its own terminal turn-wait proof
is explicitly pruned by the future bounded history owner; it must not cascade
from an unrelated run into active replay. Phase06 removes only eligible unpinned
terminal companion proofs before their exchange, under source/deadline retention.
Extend the existing wait shape so child-specific invariants remain exact; do not
reuse child/decision fields under a false reason. Native proof binds tenant/run/
execution/exchange/job and the real completed attempt's original worker/generation.
Add native reason/shape, park and reopen guards, and an `AgentTurn` WaitingReason
with exhaustive inspection/rendering/serialization matches.

Parking atomically marks the exact processing attempt completed with distinct
stop reason `agent_turn_parked`, clears its job lease into completed, and sets its
run waiting/agent_turn. It does not increment retry_count, model usage or any
failed-provider counter. It uses the existing suspension/continuation mechanism;
one legitimate resume consumes the existing continuation allowance and never resets
it. Add exact `confirm_suspension` evidence for this reason, valid even if a fast
successor wake already completed the wait. Parked returns the existing handler
Suspended outcome and immediately releases the worker slot/connection. Keep the
original overall claim/run deadlines across the Ready handler refactor.

## Readiness, resume, lost wakeups and current access

Integrate agent-turn readiness in the existing bounded polling SQL and wait sweep:
a waiting Queued exchange is ready if the pointer is empty and it is the minimum
pending position, or if it already owns the pointer. Deadline/terminal work remains
eligible through existing maintenance. Keep page/scan/statement/continuation bounds;
use the existing supervised workers and positive poll interval. No per-conversation
poller, detached task, queue service or new listener. Notifications are optional
hints; durable readiness alone must recover a crash after terminal commit before
any wakeup. Do not touch a successor's run/job from terminal release.

A turn-wait resume locks its own current run/execution/wait/job, then conversation,
checks the exact current queued source/wait and minimum position, reserves the
pointer and completes the wait/reopens the same job atomically. This prevents a
wake/repark loop when an older failed position reopens between readiness and claim.
The subsequent genuine lease and preparation still perform current association/
agent/model authority before any history, secret or provider entry. Internal
readiness is not a provider permission or history read. A source that loses current
rights reaches the normal leased handler's genuine terminal/uncertainty owner;
it cannot remain a repeatedly failing wait-head auth check or grant stale access.

Completed agent-turn waits are continuation evidence, not terminal business results:
exclude them from generic terminal-wait replay, as existing model-tool waits are.
Exact original park proof remains usable for suspension confirmation. Nonterminal
human/action/token/delivery/child waits retain the pointer and are resumed through
their existing authorized correlation path. Incoming human answers are not new
turn commands; phase05 supplies full repeated channel-round integration proof.

## Atomic release, recovery and ambiguous receipts

Add one native pointer-release owner on an exchange's genuine terminal transition,
clearing only a pointer equal to that exact exchange. It covers accepted final
checkpoint, exhausted-invalid model settlement, routed failure, run failure/cancel,
guard/deadline/parent cancellation and safe recovery terminalization already owned
by02. Completion/stamp/pointer release commit together. Queued cancellation removes
that pending blocker while preserving another active pointer. Accepted completed
output stays immutable through later workflow/channel outcomes and old worker code.

Lease expiry/retry transfers only the existing execution fence; the same logical
turn/input/order/checkpoint stays owned through recoverable uncertainty and waits.
An unknown model/effect remains charged and governed by existing reconciliation;
turn recovery never invents success or repeats it blindly. A crashed acquired turn
with no checkpoint is recovered by the same execution; original deadlines and native
terminal hooks provide a bounded exit if it cannot complete.

Update *all* ambiguous preparation/readback evidence for the new effects. Ready
requires the same durable exchange/input/provenance and own pointer (unless completed
replay), under current live ownership. Parked is proven by the exact immutable
completed `agent_turn_parked` attempt/wait witness, which intentionally retired the
old fence; it may acknowledge that park after a fast wake but grants no new work.
Do not require a live old job to prove an intentional park, nor return Ready from
an old matching checkpoint when pointer acquisition/activation is absent. Readback
is bounded and read-only, never a second writer. Retain accepted02's accurate
reopen/link proof and extend it for any new required turn effect. AtCapacity is a
known rollback/refusal, not ambiguous success. Actual commit gates must test these
boundaries, not only duplicate successful replay.

## Cohesive implementation and focused verification

Native schema/ports/runtime/preparation/waits/readiness/terminal hooks and all
fixture/match surfaces are one batch. P04 selected history and P06 bounded history
pruning remain later phases; all source/preparation and failed retry pins persist.
Fresh ownedp03 migrations and actual SQLx prepare/check accompany SQL changes.
No full suite; keep stock2MiB, offline/lint/fmt/diff and deterministic Graft gates.

List and run nonzero `agent_history_ordering_` and `agent_history_recovery_` families.
Prove competing same-stream admissions/claims, independent provider progress,
provider-call count0 while parked/no held connection or worker slot, complete
ordered positions and predecessor output durably observable before the next
provider boundary (automatic model seed injection is P04, proved end-to-end in07),
32/256 cap refusal with no partial allocation, queued cancellation,
older safe reopen behind a newer owner, exact retained checkpoint/budgets and no
permanent watermark. Use real native claimant/row-lock contenders and OwnDatabase. Include actual
checkpoint-versus-turn serialization and the bounded parent-cascade exception above.

Exercise crash/reconstruct after enqueue/acquire and before checkpoint, real
commit ambiguity for Ready/Parked/reopen/release, fast wake versus park receipt,
lost wakeups through normal polling, lease replacement/stale finish, current
revocation, deadline/resume and cancel/final contention, invalid model settlement
release, bypass guard failures, repeated nonterminal waits, same-agent parent/child
private branches and tenant-bound refusal during cancellation/retry. Preserve actual
unknown provider/effect accounting. Run affected native lease/claim/fairness, waits,
deadline/cancel/approval/child/delivery, checkpoint/usage/repair/channel regressions.
One whole-phase independent code review follows implementation and these checks;
architecture acceptance alone does not accept the protocol implementation.
