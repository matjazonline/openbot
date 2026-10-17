# 04.11-D1 — Messaging transaction and receipt ownership

No backward compatibility: reset databases for incompatible schemas/payloads; no
backfills, dual paths, legacy decoders or staged rollout.

Status: proposed architecture gate, not approved. Original requirements are
04-actions-http-and-delivery.md (messaging and Acceptance), EXPANSION.md04.11 and
cross-point preservation, and accepted04.1–04.10 contracts.04.10 is accepted at
2be42e3e…bc4c/R005. User explicitly chose users/channels and rejection of groups.
This is one cohesive batch; phase05+ and production composition remain out of scope.

## 1. Input, routing and authority

Keep canonical subject/body inputs and UUID destinations. Remove group from the
message.send catalogue and reject it at runtime. A channel destination publishes
in-app into that company channel. A user destination emails that current company
member via the run's explicit channel; a company-only run without a channel cannot
infer a sender and is refused. Duplicate destination identities are rejected. Keep
the existing MAX_SELECTIONS/MAX_TEXT bounds and enforce them at runtime too.

One canonical message can have several channel/thread associations and email
intents. Reuse the run thread when it belongs to the selected channel; otherwise
create the necessary thread inside the atomic publication transaction. For a user
recipient use the run channel/thread (create a thread if only channel is selected).
Never select a first/default channel. Source and destination IDs remain tenant
scoped in SQL and in composite foreign keys. Channel publication is visible to that
channel's existing readers; it is not an email to every channel participant.

message.reply resolves the exact canonical source UUID. Require its tenant and,
when present, the run's channel/thread scope. With no run thread, require an
unambiguous source association rather than choosing the latest association. An
external email source replies to its actual author using its exact RFC Message-ID
and references, not the latest message's headers. Internal source replies publish
in-app in the source thread. Internal-only/private content must never be promoted
to external delivery. Preserve recipient identity and threading in the frozen plan;
no redirect to a newly changed address on retry. No attachments or arbitrary email
addresses are added to the workflow input contract.

Use explicit, trusted built-in ToolSnapshots for message.send/message.reply,
selected through a message PublicationDirectory. They have fixed schemas and
side-effect/recovery metadata and no connection slot. Publication/binding verifies
them against those built-ins rather than routing them to HTTP registrations.
Runtime uses ActionTarget::Local(company_id, kind=messaging), existing frozen
capability ceiling and company administrator/owner authority. Current authorization
is read from durable owners; forged policies/snapshots are rejected. Frozen stronger
approval remains stronger and returns ApprovalRequired without creating a message
or delivery; actual approval settlement is phase05. No separate messaging grant
registry or fabricated resource connection is introduced.

## 2. One logical invocation and atomic local publication

Reuse ActionService::prepare_step/prepare_tool, FrozenAction, invocation/digest and
saved model-call identity. A purpose-specific message dispatch port reuses
PostgresActionDispatch::authorize_on and its current-policy/fence checks. The same
service is called by the workflow handler and direct-tool entry point. Rendering
uses DeliveryComposer and injected application TransportRenderers, never SMTP or
provider-specific code inside the workflow handler.

Prepare bounded canonical content/rendered parts without provider I/O. Recheck exact
resolved current recipients, channel/binding readiness, source and authority in the
commit transaction. Locks: company/actor, run/execution/job/attempt, immutable intent,
then selected channels/threads in UUID order, then delivery rows. No network I/O
inside a transaction. Publication is not allowed to outlive the current fence.

For purely in-app publication, the canonical message, all associations, local action
marker and accepted receipt commit together using the existing local invariant.
Return {message_id,status:accepted} only after commit; normal shared completion
then advances the step.

For email, atomically write canonical message/associations, frozen delivery intents
and parts, action dispatch linkage, a durable message-action wait, and (for a message
step) workflow_waits(reason=effect). Park the message step and close its current
attempt/job in that same transaction. No second delivery queue or execution lease.
All outbound keys derive from the immutable action invocation plus destination,
never the attempt or a fresh delivery UUID. Existing renderer owns stable Message-ID.

Add a delivery effect kind to the existing action marker/receipt vocabulary rather
than pretending queued work is an accepted local receipt. Add tenant-scoped
workflow_message_actions (one per invocation: message, origin step/model_tool,
state, deadline, last processed event sequence and original commit-fence expiry),
and workflow_message_deliveries (exact required delivery IDs). The action row is the
durable per-invocation wait; no separate action wait queue. Immutable linkage/content
and allowed mutable state/cursor fields are protected by SQL. Cross-tenant and
wrong-message/delivery links are rejected by composite FKs/guards.

The deferred dispatch guard keeps the existing local/remote cases unchanged. Its
new delivery case proves exact message linkage/required deliveries and either the
original live processing fence (direct tool) or the exact atomically closed attempt
and effect wait (message step). In both cases the captured original expiry and run
deadline must still be live at COMMIT. Merely writing a completed job is not proof.
A delayed trigger past expiry rolls back message, deliveries, wait and marker.

Ambiguous COMMIT uses WriteBound and a fenced read under the original locks:
committed linkage returns the same message/wait; definite absence permits retry;
unresolved acknowledgement stays unknown. Never mint a new logical effect because
an acknowledgement was lost. Competing claimants create one canonical message and
one set of delivery intents. A saved action receipt replays without rendering/sending.

## 3. Handler suspension and direct-tool boundary

Introduce an explicit handler outcome: completed existing WorkflowHandlerResult or
suspended with a committed wait identity. Keep the existing Result alias for ordinary
handler/service tests where possible, using conversion at the supervisor boundary.
The supervisor validates that suspension names the exact closed fence/committed wait
through a required lease-port operation; it must not call complete_io or treat a
successful park as an error. No silent-success default on test mocks or production.
Crash after atomic park is recovered through the existing durable wait/poller.

Direct message tools return Accepted/Failed receipt, Pending durable invocation,
or ApprovalRequired; Pending never claims provider acceptance or produces successful
model output. Their action row is the atomically registered durable wait, but they
**do not complete or park an enclosing agent execution as if it were a message step**.
The phase06 saved-call owner must suspend on Pending and resume the saved call from
its receipt before model generation; phase08 composes it into the production host.
This explicit owner integration obligation is documented and tested as a port seam,
consistent with HTTP/MCP's06/08 composition boundary. No agent.run implementation or
multiple-waits-per-agent redesign is smuggled into04.11. Direct re-entry with the
same model call observes the same message/deliveries; changed arguments are refused.

## 4. Delivery receipts, aggregation and progression

Existing delivery rows/parts own provider outcomes. Add small immutable receipt-event
rows for relevant status transitions of workflow-linked deliveries. Events reference
the immutable delivery-link row; their write must not lock workflow runs or mutable
action rows while holding a delivery parent. Duplicate unchanged callbacks add no
new logical event. An event is a wake hint plus retained evidence; aggregation reads
the authoritative delivery/part states, not a callback's supplied result.

Extend the existing bounded workflow poller with a message-receipt work kind for
executions having unconsumed message events. This remains discovery of existing
work, not another queue/lease. It also collects late receipts on cancelled/terminal
runs. The consumer takes run then action/delivery locks in stable order, advances
the per-action event cursor, and atomically records a canonical action receipt once
all destinations have a known terminal outcome. A whole execution's bounded set of
message actions is processed; use existing action-count bounds and bounded batches,
with progress cursors so an unchanged poison set cannot monopolize scanning.

All required deliveries accepted => accepted action receipt. Known terminal failure
with no uncertainty => failed action receipt, retained accepted siblings. Extend the
message result schema to accepted|failed (message_id/status); a failed receipt routes
a message step to finalError through the established completion/failure machinery.
Queued/retryable/sending remain Pending; any outcome_unknown remains Uncertain,
without a fabricated success/failure receipt or unsafe resend. Email acceptance means
provider acceptance only, not inbox receipt/read. No email verifier or inference of
not-applied from timeout is invented; uncertain deliveries remain held until genuine
provider evidence/owning reconciliation support exists, or the run deadline ends it.

For a live message step, receipt aggregation resumes only its exact effect wait.
Commit receipt before step advancement; output validation/route failure cannot resend.
For model_tool origin, collect the receipt but leave enclosing step progression to
its saved-call owner. Cancellation/terminal runs retain receipt truth without any
successor or returned tool output. Receipt replay still requires current authority
when returning a result to a caller. Background evidence retention does not.

## 5. Cancellation, current permission and duplicate callbacks

Extend the existing delivery cancellation helper to accept workflow-linked delivery
IDs and explicit workflow cancel/expiry/revocation reasons, retaining the same unsent
versus in-flight logic. Workflow controls/expiry cancel pending deliveries in their
run transaction; no accepted sibling is altered. Sending/unknown retains the live
lease and cancellation intent so late provider acceptance is still recorded.

Before begin_part marks possible provider entry, workflow-linked deliveries lock
company/actor and run before the parent and recheck live run/deadline, current
membership/recipient identity and source channel/binding. A refusal cancels unsent
work without a provider call. Existing non-workflow deliveries retain their path.
Cancellation and begin_part have the same run-before-delivery order; receipt callbacks
hold only the delivery parent plus immutable event linkage, avoiding an inversion.
Cancellation after committed begin_part cannot promise rollback of the provider.

Verify and fix complete_part's ability to overwrite an already-delivered part while
siblings keep the parent leased: only a currently sending part may accept its first
outcome. Duplicate callbacks must preserve the first durable acceptance and avoid
advancing twice. Retain existing crash/reaper behavior: sending becomes unknown;
prepared/retryable may retry; delivered never re-enters. No raised resource limits.

## 6. Verification and acceptance

Focused implementation checks, default parallel and real isolated PostgreSQL:
1. Registry/runtime rejects groups, duplicates, malformed IDs, excessive content/items;
   trusted built-in publication, forged capability/current revocation/protection refusal.
2. Real publish/bind/admit/worker in-app and email fixture; exact-source reply with a
   newer distractor message; stable Message-ID across restart/safe retry; correct
   in-app channel association; foreign/private/ambiguous-source refusal.
3. Competing dispatchers and model-call re-entry; atomic injected rollback at message,
   delivery/link/wait writes; commit-time expiry; lost acknowledgement; changed args.
4. Provider acceptance resumes only after every destination; accepted+retryable retries
   only the latter; accepted+failed finalError; accepted+unknown remains held; duplicate
   callbacks; crash-after-send/reaper; restart gives no duplicate provider send.
5. Cancel-first/begin-first, revoke-first, cancel versus receipt/continuation competitors;
   late accepted truth without advancement; stale delivery/workflow fences; deadline.
6. Event-before-observer/downtime, competing receipt consumers, bounded poison batch,
   current-result authorization, direct Pending→receipt seam without enclosing-step
   completion; shared wait/supervisor/poll/control regression tests.

Record per-batch commands/results. Final combined phase04 gate after implementation:
cargo fmt/check/clippy --locked --offline --all-targets; SQLx prepare/check; fresh
migrations/schema equivalence; full real-DB all-target test suite; full stock2048KiB
library suite; lease-budget script; transport boundary/regression and credential CLI;
graft build. Reuse earlier original HTTP/MCP fixture matrices through the combined
suite. Independent whole-batch review of implementation and combined effects, then
root digest/evidence acceptance. Retain every failed gate and diagnosis.
