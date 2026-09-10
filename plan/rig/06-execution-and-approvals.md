# Step 6: Bounded execution, approvals, and suspension

Dependencies: steps 2–5. Implement execution inside the existing durable task system. Rig owns
model interaction; application services own authorization, task transitions, continuation state,
and delivery. No second scheduler, workflow engine, or Rig agent-as-tool delegation service.

## Existing task-system capabilities

These are current implementation facts, not capabilities supplied by Rig:

| Existing code | Reuse |
| --- | --- |
| `src/application/services/task_worker.rs` | Bounded workers, inbound/scheduled dispatch, shutdown and ownership-change cancellation, retries/backoff, maintenance |
| `src/application/task_queue.rs` | `TaskPersistence`, `TaskLease`, `while_leased`, and atomic `commit_agent_dispatch` port |
| `src/domain/entities/task.rs` | `TaskLeaseRef` carries task, worker, execution generation, claimed owner, and ownership version; existing statuses cover processing, approval, third-party wait, completion, stop, and dead letter |
| `src/adapters/persistence/task/queue.rs` and `operations.rs` | Claims, lease renewal/reaping, attempt accounting, quorum transitions, reply/outbox persistence |
| `src/application/services/harness/approvals.rs` | Shared `AgentApprovalHandler`, stored decisions, deterministic step keys, approver and internal-delegation policy |
| `src/adapters/persistence/approval.rs` | Approval creation, task parking, thread notice, and notification outbox already share a transaction |
| `src/application/use_cases/approval.rs` | Approval link actions and the current 24-hour approval lifetime |
| `src/application/services/native_tools.rs` | Our directory, provisioning, outreach, and task-ownership tools |
| `src/application/use_cases/thread/dispatch.rs` | Inbound/scheduled harness deadlines, suspended-result handling, final durable dispatch |

Keep `background_tasks` as the only claimable queue. Approval and quorum continuations return the
same task to `pending`; workers claim a fresh execution generation. Approval continuation preserves
`retry_count`; actual failures and expired leases consume attempts. Attempts and status events
remain the operational ledger, not the source of model conversation state.

## Gaps that require changes

1. **No durable model/tool continuation.** `AgentRun` takes a composed prompt and host ports, with
   no checkpoint port. Inbound payloads intentionally contain stable identifiers, not provider
   content. Restart reloads entities and composes a new run; approval decisions and outreach context
   alone cannot restore the exact pending tool call or completed results.
2. **Ordinary approval decisions and task transitions are separate commits.**
   `consume_pending_approval` updates the approval before `apply_decision` separately resumes or
   stops the task. A crash between them leaves a consumed decision and a parked task.
   `consume_quorum_timeout_action` already demonstrates a transactional alternative.
3. **No general approval-expiry maintenance sweep.** `run_maintenance` reaps leases, checks quorum
   timeouts, and reports stuck work. Ordinary expiry is handled on link access; unattended approvals
   need a bounded sweep that settles their tasks as well.
4. **Business deduplication is not a complete invocation checkpoint.** Outreach and provisioning
   have operation-specific keys, but no common record of every model call/result. Ownership command
   IDs depend on `execution_generation` plus call ID, which changes across claims.
5. **Some host failures lose their type.** Outreach, provisioning, and ownership tools turn
   persistence errors into `ToolInvocation::failure` text. Rig needs to distinguish invalid/denied
   requests from infrastructure failures and lost ownership.
6. **Some effect ports lack the full execution fence.** Provisioning carries `source_task_id`, not
   a lease. The ownership tool sends an expected ownership version without the execution generation.
   Add transaction-level lease checks where missing; an earlier adapter read is insufficient.

The additions below are required for restart-safe Rig execution. Existing task statuses and inbound
payload versions need not change merely to hold a conversation.

## 1. Add continuation storage through application ports

Add a cohesive `HarnessRunStore` under `src/application/services/harness/`, with an adapter such as
`src/adapters/persistence/task/harness_runs.rs`. Pass a run-scoped implementation through `AgentRun`
and the application `AgentRunner`. Use typed run/invocation/revision identities. Keep Rig types out
of application ports, domain types, SQL rows, and persisted state.

Provide explicit operations to open/load a run, reserve model work, commit a model turn, prepare an
invocation, record a result, and save final output. Every worker mutation requires the full
`TaskLeaseRef` and expected checkpoint revision. Approval/quorum/ownership event transactions use
their own explicit authority and expected wait/ownership revision because parked tasks have no
lease. Return applied/already-applied/ownership-lost outcomes. No silently successful defaults on
these correctness operations.

Add two bounded, versioned stores through an additive migration:

| Proposed table | Required contents |
| --- | --- |
| `task_harness_runs` | Run/company/task/agent IDs; owner/version; harness and provider/model identity; capability fingerprint; schema version/revision; active/waiting/completed/superseded state; conversation checkpoint; pending call position; budget reservations/usage; wait reference; final output |
| `task_harness_invocations` | Run and stable invocation IDs; model-turn/call ordinal; provider wire IDs; canonical tool ID and argument fingerprint/content; prepared/waiting/completed/failed/indeterminate state; approval/outreach/ownership-command reference; bounded result/disposition |

Enforce one open continuation per task and owning-agent execution scope, unique call ordinals per
run, tenant-consistent references, supported schema versions, and record/count/byte bounds. Keep
transcripts out of task payloads and diagnostics. Never store credentials or approval tokens here.
Apply task-scoped access control and retention; effect identities must survive the supported replay
window so cleanup cannot make completed effects executable again.

Represent ordered assistant/tool messages and required provider continuation metadata in an
application-owned schema, including distinct call/item IDs and required opaque continuation blocks.
Use explicitly bounded/versioned adapter metadata where necessary, not serialized Rig structs.
Prove round trips per supported provider in steps 2/9; reject unsupported response shapes before
executing any tool from that turn.

A run ID survives approval/quorum waits and automatic task retries. Execution generation identifies
one worker claim, not a logical run. Transfer/release supersedes the old continuation: the new owner
starts its own conversation with existing handoff instructions and permitted task context, never
the previous agent's private transcript. An explicit operator restart may start an audited new
budget epoch, preserving effect records and without implicitly reauthorizing rejected actions.

Reload credentials and authorization each claim. Record configuration identity and enforce step
1/3's active-run edit fence for harness, model, and execution-affecting configuration/instructions.
Never reinterpret saved state under a different harness. Recheck grants before new execution even
when an invocation was previously approved.

## 2. Implement the bounded Rig loop

Implement `AgentHarness` for `RigHarness`, returning `HarnessKind::Rig`. Build fresh runtime state
per claim; share only immutable factories/transport resources. Keep pure compilation synchronous
and box the external runtime future at the existing seam.

Rig documents a runner with model-call limits, sequential tools, explicit history, hooks, and a way
to disable its conversation memory. These are useful building blocks, not proof of our persistence
contract. Source: [Rig AgentRunner API](https://docs.rs/rig/latest/rig/struct.AgentRunner.html).

Extend step 2's pinned-version proof to capture a complete model turn before its first tool, await
checkpoint persistence before dispatch, stop before later calls, and resume a partially processed
batch without another model request. Use `AgentRunner` only if it supports all these boundaries;
otherwise drive Rig's lower-level completion/run API in `rig/execution.rs`. Disable Rig-managed
conversation persistence and automatic tool retries. Do not re-prompt and hope the model repeats
the action that was approved.

Use this sequence for inbound and scheduled tasks:

1. Load/create the fenced continuation; validate schema, owner, configuration, grants, budgets,
   and deadline. Resolve a saved wait/result before requesting more model work.
2. Atomically reserve a model call and input/output allowance before sending. Persist its request
   identity. A crash with unknown usage keeps the reservation charged until reconciliation;
   uncertainty must not restore spend allowance.
3. Commit the complete response, usage, provider IDs, and ordered tool-call batch before invoking
   a tool. Stop on persistence failure. A response lost before this commit may require another
   bounded provider request, but cannot have caused a tool effect.
4. Process saved calls sequentially. Derive stable invocation identity from run and persisted
   turn/call position, separately from provider IDs and approval keys. Validate arguments/grants,
   reuse committed results, then apply the tool's approval policy and invoke the guarded bridge.
   MCP tools skip approval; explicit `request_approval` checkpoints handle their own decisions.
   Identical arguments
   at different positions are distinct calls; retain business deduplication where it deliberately
   merges operations.
5. Commit each result and cursor advance before another tool/model call. Pending approval, outreach
   wait, transfer/release, cancellation, and lease loss stop the loop immediately.
6. On continuation, finish the saved batch using its original wire IDs. Completed calls return
   stored results; newly approved calls execute saved arguments under the new lease. Each provider
   tool call must receive exactly its corresponding result before another model request.
7. Save sanitized `AgentExecutionOutput` durably before returning it. A retry can then reuse final
   output without another provider call. Dispatch continues through the existing atomic
   message/outbox/review path; the harness never sends the reply itself.

Checkpoint step 5's `read_resource` results, skill URI/content fingerprints, and ordered instruction
text as conversation content. Revalidate access on resume and reject incompatible skill edits.
The model follows those instructions inside this loop: there is no recursive skill executor or
skill cursor. Persist bounded mutable built-in state such as `todo`, or reconstruct it from committed
calls/results before continuing. Reuse recorded nondeterministic read results on normal resume.

## 3. Make approvals continuation-aware and atomic

Keep policy in `HarnessApprovals::decide` with existing `ApprovalAsk`/`ApprovalTrigger`. Preserve
step-key bytes: SHA-256 of task, thread, and the current rendered action. Freeze argument rendering
in fixtures; canonicalization changes need an explicit compatibility strategy for stored approvals.
An approval is permission, not proof of tool execution.

Extend approval requests with an optional typed run/invocation reference, required for Rig task
runs. A prepared invocation may be saved first, but `create_approval` must link that exact call,
mark its waiting state, park the task, and insert the existing notice/outbox in one transaction.
After parking clears the lease, no subsequent checkpoint write may rely on that lease. Repeated
creation must validate existing wait linkage/state rather than return pending while leaving a
processing task unparked.

Replace ordinary consume-then-resume/stop with a purpose-specific application persistence operation,
`decide_approval_and_transition`. Use a consistent row-lock order shared with ownership/outreach
transactions. Validate company, pending token, expiry, expected ownership, and the currently awaited
approval/invocation. In one transaction:

- Approve: consume the decision, mark the invocation ready, clear its wait, and set the same task
  to `pending`/due now, preserving retry count.
- Reject: consume the decision, record denial, close the wait, and stop the task, preserving current
  human-rejection behavior. No automatic model reply is sent.
- Repeat/stale decision: return the recorded outcome or typed stale result without releasing a
  different wait or resurrecting stopped/transferred work.

Remove the second task transition from `apply_decision`. Preserve non-task approvals and existing
quorum-timeout actions with explicit non-Rig paths. Emit wakeups only after commit; queue polling
recovers a lost wakeup. Required decision notices belong in the transaction or a durable follow-up,
not an unrepeatable best-effort action after consuming the token.

Add an approval-expiry sweep to `TaskWorker::run_maintenance`. Claim due pending approvals in bounded
batches with row locks/`SKIP LOCKED`; atomically expire each approval, close its matching wait, and
stop the associated task with an observable expiry reason. Keep the existing 24-hour lifetime.
For quorum-timeout approvals, settle/cancel outreach consistently. Never stop work now waiting on
another decision or owned by someone else. Do not automatically reissue expired approvals every
poll; explicit restart follows a defined new approval cycle. Use bounded backoff and test two
consecutive iterations containing poison records.

Route link-triggered expiry through the same atomic operation; otherwise a link can mark a row
expired before the sweep sees it and leave the task stranded. Reconcile already expired/consumed
legacy approvals with unresolved waits in a bounded upgrade/maintenance path. The current approval
upsert reuses expired rows with a new token: bind waits to the invocation and approval cycle as well
as approval ID, so an old event cannot release a later cycle.

### Explicit checkpoints through `request_approval`

Add a grantable application-owned native tool, `request_approval`, for instructions such as
"prepare a plan, ask a human to approve it, then continue." This complements automatic approval
before protected tools. It can be called directly by the model or named in a stored
`SkillInstruction::Tool`; no new skill instruction variant is needed.

Implement it in proposed `src/application/services/approval_tool.rs`, register its stable ID in
`src/domain/entities/tool_catalogue.rs`, and wire its declaration/dispatch in `NativeToolHost` and
the application `AgentRunner`. Include it in tool pickers and skill-implied grants. Offer it only
when the run has a durable task, invocation identity, and approval context. Missing approvers or
required context must produce a clear denial/preflight error, never implicit approval.

Use a strict V1 input schema, rejecting unknown fields:

```json
{
  "title": "Approve the proposed response plan",
  "proposal": "Summarize the investigation, then draft a response for review."
}
```

Require nonblank `title` (at most 120 characters) and `proposal` (at most 8,000 characters), with
an additional 64 KiB serialized argument bound enforced before parsing. These are proposed new
limits to implement and test. `proposal` must contain the concrete text the human is approving;
do not accept only a transient reference to model context. Freeze that text with its fingerprint
in the invocation and approval payload, and show it in the existing approval page and notification
using escaped rendering. The model cannot set approver, tenant/task IDs, token, decision, timeout,
or an executable callback. Resolve these from trusted application context and the existing policy.

The tool itself calls the shared approval service. Its declaration must avoid a separate generic
pre-execution approval gate, which would otherwise ask permission to ask for permission. Document
this explicitly: `requires_approval_by_default = false` suppresses that outer gate only; the tool
body always checks/requests the human checkpoint decision. It is neither read-only nor
concurrency-safe: it can create an approval notice/outbox and park a task. Apply normal invocation
budgets, output bounds, and full lease fencing. Internal-delegation auto-approval must not approve
an explicit checkpoint.

Add a typed checkpoint trigger to `ApprovalTrigger` and the shared handler, carrying the trusted
stable invocation identity and proposal fingerprint. Give this new trigger its own versioned
step-key encoding while preserving all existing tool/condition/state encodings. Replaying the same
invocation returns its decision; a new checkpoint or changed proposal requires a new decision.
Never let a model-supplied title serve as the approval identity. Update serialized-payload and
action-type handling, approval page labels, and compatibility tests for the new trigger.

Execution and recovery use the same transaction protocol described above:

1. Persist the complete preceding conversation, loaded skills, results, and checkpoint call.
2. In one transaction, create/link the approval, save its waiting invocation, enqueue the human
   notice, and park the task. Return a typed suspended outcome, not a final tool result saying
   "pending" that would let the model continue. No later tool in that batch may run.
3. Approval resumes the same task. Restore the original conversation and mark the saved checkpoint
   invocation completed with `{"status":"approved"}` and its approval reference. Associate this
   result with the original provider call ID, then continue the saved batch/model loop. This tool
   has no deferred business action to execute and must not send another approval request on resume.
4. Rejection or expiry stops the task and closes the checkpoint under the existing rules. If an
   already rejected checkpoint is encountered during recovery, return the typed terminal outcome;
   do not continue reasoning as though it were an ordinary optional tool denial.

Checkpoint approval authorizes the presented proposal only. Later tools still undergo their own
grants, argument validation, and ownership checks. Protected native tools retain their approval
checks; MCP calls require no additional approval. A skill's instruction to call this tool is
model-followed guidance. If a particular
action must be impossible before a checkpoint, enforce a prerequisite referencing that approved
checkpoint and action scope in application policy; prose or the presence of a skill step cannot
provide that guarantee.

Wire the native tool for both harnesses through shared application ports, with an explicit
compatibility gate: do not advertise it for ai-agents until its suspension and stable-invocation
replay fixtures pass. Do not claim full-context restoration for that adapter without implementing
the same continuation contract. Preflight must reject an unsupported grant rather than dropping it.

Add fixtures for a skill with prompt → `request_approval` → prompt/tool instructions, direct model
use, and both approval modes in one run. Test positive approval after process restart, rejection,
expiry, duplicate calls/clicks, changed proposal/new checkpoint identity, missing grants/context,
oversized inputs, escaped approval rendering, and a first checkpoint stopping a second tool in the
same response. Verify exactly one approval notice, no automatic internal-delegation bypass, no
double approval around the checkpoint itself, and independent approval of later protected tools.

## 4. Integrate our tools with durable effect records

All sub-agent work uses our `list_company_agents`, `create_agent_channel`,
`outreach_and_await_quorum`, and `transfer_or_release_task` tools through the step 4 bridge.
Never register a Rig agent as an executable tool. Preserve tenant restrictions, sub-agent allowlists,
internal-delegation approval policy, quorum, and task ownership.

Extend host invocation context with stable invocation identity and the full current lease; keep
provider IDs separately for history/trace correlation. Adapt ai-agents explicitly when shared ports
change. Keep input/policy denial as a tool result; propagate infrastructure failure and lost
ownership as typed application/control outcomes. A database outage must not become model advice
followed by additional effects.

| Tool/effect | Required change |
| --- | --- |
| Directory and `read_resource` | Persist bounded results before reuse; interrupted reads can repeat subject to current authorization/budgets |
| `request_approval` | Atomically persist checkpoint wait/notice; on human approval restore context and supply the saved call's approved result, without executing another action |
| HTTP MCP tools | Execute calls from selected company definitions without human approval; persist results and saved schema/argument identity, revalidate company definition/credential and agent selection revisions, and reconcile indeterminate remote effects |
| Provisioning | Carry full lease into its port and fence the transaction; commit invocation receipt/result with provisioning and its existing request-hash deduplication record |
| Outreach | Extend `create_outreach_and_pause` to link the prepared invocation and its waiting result in the transaction that creates outreach/targets/messages/outbox and parks the task |
| Quorum ready/timeout action | Link transitions to the waiting invocation. Reconstruct its ready result from durable outreach progress and bounded response context, then finish the saved batch; do not send outreach again to obtain a result |
| Transfer/release | Fence the command on execution generation and ownership version. Rig uses an invocation-derived stable command ID; commit its result and supersede the old continuation with the ownership change |
| Reply/review candidate | Reuse `commit_agent_dispatch`, its ownership fence, review behavior, and stable delivery IDs; mark saved final output consumed in that transaction |

Do not write an effect and its completion marker in separate transactions. For database effects,
use shared transaction helpers behind purpose-specific application ports. Never hold a database
transaction across provider work. For remote effects, persist intent and use the existing outbox
or a supported idempotency key, recovering via a durable receipt. An indeterminate non-idempotent
result stops for reconciliation rather than blind retry. Cancellation cannot undo an accepted
remote request; do not promise exactly-once external delivery.

## 5. Preserve cancellation, deadlines, and budgets

Keep `while_leased` and worker shutdown supervision around the actual harness future. The existing
15-minute renewed lease is not the run deadline. Dispatch uses `agent.run_timeout(...)`, with
`AGENT_RUN_TIMEOUT_SECS` defaulting to 300 seconds. Pass the caller's effective deadline through
`AgentRun` so connect/request/tool timeouts can be capped by remaining time. Audit simulation and
direct paths for equivalent timeout coverage; without task context, do not expose durable
side-effecting or suspending tools.

Do not detach correctness-critical work. Cancellation must stop the provider/tool future and await
any supervised children during bounded shutdown. Check the full lease inside each durable effect
transaction. Deliberate parking/transfer clears ownership: recover from its committed checkpoint
even if a heartbeat or ownership notification wins the race with the `Suspended` return. Old-worker
closeout must not overwrite the new state or charge another generation for that cancellation.

Define enforced server ceilings in `RigExecutionPolicy`. Step 3 proposes 8 model calls by default,
requested range 1–16, capped by server policy. Before implementation is complete, choose numeric
ceilings for tool calls, per-request and cumulative input/generated tokens, result/checkpoint bytes,
and continuation count using representative step 1 fixtures. Bound active execution time across
continuations as well; parked time is bounded by approval/outreach deadlines. These are new limits,
not guarantees provided by existing token diagnostics.

Persist counters/reservations across automatic retries and waits. Count initial requests, provider
retries/recovery, every tool request/resource load, and loaded skill text. There are no nested skill
calls to count separately. Reserve output room and recheck the full prompt after appending results,
including catalog, schemas, history, and provider metadata. Use provider/model tokenization or a
documented conservative fallback; the existing approximate usage counter is not a prompt guard.

Keep tool concurrency at one. Bound provider retries and charge every underlying request; disable
SDK retries that cannot be counted. No automatic retry of side-effecting tools. Reuse task
retry/backoff/reaping for infrastructure failures. Add a typed execution-failure category to
`AppError` if needed for exhausted budgets, invalid provider protocol, and indeterminate effects;
update exhaustive worker/HTTP/diagnostics/adapter matches. Do not map terminal execution failures
to `Internal`, which the worker currently treats as retryable.

## Outcome contract

| Event | Required outcome |
| --- | --- |
| Approved, still authorized | Execute saved arguments under current ownership or return the committed result |
| Pending approval | Durable `pending_approval`; `Suspended` with empty reply content |
| Granted MCP call | Invoke without an approval request or approval suspension; persist/replay its result through the normal execution protocol |
| Rejected in the current run | No invocation; sanitized denial; continue reasoning only if application policy permits |
| Human rejects parked work | Stop task and close continuation; preserve existing rejection behavior |
| Native outreach wait | Durable `waiting_for_third_party_reply`; no later tools/model requests |
| Transfer/release | End old run as suspended/superseded even if the resulting task status is not a wait status |
| Persistence/approval lookup error | Propagate typed error and stop |
| Budget/protocol terminal failure | Observable terminal task failure, never an emailed error string |
| Transient provider failure/deadline | Existing retry/timed-out classification, retaining progress and charged reservations |
| Successful final response | Durable sanitized `Completed` output/usage, then existing reply/review/outbox commit |

Use a typed run-local stop reason when Rig's hook/tool error surface cannot carry these outcomes.
Inspect it before mapping runtime errors; never detect suspension by output text. Verify durable
wait/ownership state before accepting suspension, accounting for a fast decision already making the
task pending again. `close_out_task` must neither overwrite that transition nor leave an unparked
processing task stranded merely because a harness returned the wrong disposition.

## Implementation order and verification

1. Add continuation types/ports, migration, lease/revision fences, and storage bounds. Wire inbound
   and scheduled execution through the application runner; preserve old ai-agents tasks.
2. Make approval request/decision/expiry transactions continuation-aware, including non-Rig paths.
   Add `request_approval` and its checkpoint trigger, grant/schema/UI wiring, and replay tests.
   Add invocation receipts and missing execution fences to provisioning, outreach, and ownership.
3. Implement Rig loop boundaries, provider history restoration, budgets, and cancellation. Integrate
   saved final output with dispatch and restore skill/built-in context.
4. Extend existing task monitoring with sanitized run/invocation IDs, wait and stop reasons, budget
   usage, replay counts, and indeterminate-effect state. Keep transcript content out of logs.
5. Pass this matrix and step 9 CI before enabling Rig by default.

Use the existing simulated agent endpoint with the
[step 9 request-checked scenarios](09-verification-and-ci.md#request-checked-scenarios) and actual
PostgreSQL transactions for persistence/concurrency tests. Script model tool-call batches; execute
the actual tool/approval path. Keep scenario state outside the worker being restarted, assert exact
model/invocation counts, and coordinate race tests with bounded barriers:

- First of two calls needs approval: zero invocations and no further model request. First tool
  suspends through outreach/ownership: no second call and no reply from the harness.
- Restart after approval under a new worker/generation: execute exact saved arguments, reuse prior
  results, avoid duplicate approval requests, and preserve valid provider call/item IDs.
- Inject crashes after model-response commit, before/after approval parking, after effect commit,
  before delivering its result to Rig, after final-output save, and after reply/outbox commit.
  Recovery must preserve progress without repeating committed effects or saved final generation.
- Competing claimants/checkpoint writers, stale generation with the same worker ID, and simultaneous
  approval/rejection/expiry/ownership changes: only one valid transition commits. Late approval or
  quorum replies cannot resume a different wait or the previous owner's conversation.
- Approval arrives before old-worker closeout; parking races heartbeat cancellation: both orderings
  preserve the correct task state, continuation, and retry accounting.
- Ordinary decision and task transition roll back together on database failure; duplicate link
  clicks enqueue once. Expiry settles unattended waits and avoids immediately reclaiming the same
  poison batch in the next iteration.
- Quorum reached, proceed-partial, extend, rejection, and timeout affect the correct invocation
  without resending requests. Custom-tool authorization holds; no Rig agent-as-tool is registered.
- Lease loss, shutdown, and deadline cancel blocked provider work and prevent later commits,
  including provisioning and ownership commands. Simulation cannot bypass deadlines.
- Budgets survive restart/repeated skill loads; unknown usage remains conservatively reserved.
  Malformed, oversized, incompatible, and cross-tenant checkpoints fail before effects. Stateful
  built-ins and loaded skills resume consistently.
- Existing non-Rig tasks, approvals, attempt/retry accounting, operator controls, review, and
  delivery deduplication remain covered. Fresh and upgraded databases pass the migration.

Regenerate and commit `.sqlx/` metadata for changed queries. Keep CI formatting, locked offline
compilation, clippy, migrations, and database-backed tests. Measure `scripts/stack-frames.sh`
before/after; `scripts/stack-budget.sh` must pass without raising stack limits. Any raised resource
ceiling requires a documented reason and an early-failure regression gate in CI.
