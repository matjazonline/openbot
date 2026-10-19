# 06-D4 — workflow Rig checkpoints and token continuation, revision2

Proposed architecture gate for06B2b. No dependent implementation. 06B2a root-accepted under06-R010.
Revision2 resolves architecture finding06-F007; revision1 is retained separately. No backward compatibility: edit init schema, reset identified test DBs.

## Required behavior and ownership

Agent/model calls and token usage feed existing model_usage_calls and analytics. Prices
remain display-only. Durable model turns, saved tool calls, bounded repair, provider choice,
and conservative unresolved reservations reuse RunCheckpoint and its Mutation validator.
Workflow execution uses WorkflowFence; no fabricated TaskLeaseRef, legacy queue claim,
implicit memory load/save, dispatch of generated text, or legacy ToolBridge.

Add workflow-owned checkpoint persistence keyed by company/run/execution/job, with unique
execution identity, immutable prepared agent/input/tool contract, revision and checkpoint.
Keep one durable Rig drive loop. Replace its legacy-specific scope/CompiledRun assumptions
with typed legacy/workflow bindings at preparation, checkpoint write, authorization and
tool dispatch boundaries; use a closed internal enum, not optional leases or behavior flags.
Request/history/capture/repair and pending-call-before-model ordering remain shared.
Workflow preparation and ActionService dispatch are the necessary different edges. Legacy
bindings remain for current callers until their06C migration; no second model runtime or
copy of the durable drive loop is introduced.

Open/refresh/mutate checks current run actor authority, exact fence, deadline, revision and
checkpoint identity. Writes lock authority then local run/execution/job/attempt, checkpoint,
then budget usage (the existing shared root model-call usage last). Never acquire a root or
parent run lock from a child. Recheck the live lease after contended locks and before commit.
Use existing bounded persistence/commit-resolution machinery; ambiguous reserve cannot
silently permit provider I/O. Exact replay proves original reservation/turn, never grants a
second call. New provider request IDs are persisted before I/O. Lost/cancelled provider
requests stay reserved and count against model-call/hard token safety bounds.

## Frozen preparation and actual model identity

Add a trustworthy agent UUID to publication AgentSnapshot, bind company ownership before
runtime use, retain AgentKey for selection. Frozen prepared specification includes model,
ordered selected skills/tools, actual explicit context/history, output contract, target
identities and capability selection provenance. Current grants/revocations are checked
before each model request and saved tool dispatch without reloading changed defaults.
Never accept an attribution UUID or tool target from model-generated arguments.

Resolve named registered tools to target IDs during preparation and retain those IDs with
the checkpoint. A saved call cannot silently follow a registry rebind. Continue validating
current connection/catalog/argument schema and current policy at phase04 dispatch.
Prompt composition clearly separates instructions/selected skills from untrusted context,
history and feedback. Final output is schema validated; bounded repair is tool-free and
charged to the same checkpoint/usage/budgets. Persist final result before step completion.

## Root token allowance and review policy

Extend limits.root_budget with optional tokens: {max_tokens, reviewers}. max_tokens must
be positive and bounded for checked arithmetic; reviewers uses the existing bounded list
of explicit user reviewers. The root freezes this policy at admission; descendants inherit
its allowance and reviewer policy through existing root budget links. No descendant may
widen it. The run deadline bounds the decision; no extra timeout configuration.
Omitted allowance means no human token gate; existing hard model-call, token and deadline
safety bounds always apply. No price, currency, monetary cap or cost approval enters runtime.

Reserve conservative input plus maximum output before each provider call. Used amount is
settled reported/estimated tokens plus unresolved reservations, never a zero for unknown
usage. A request which would exceed the current authorized allowance parks before I/O;
a settled response exceeding its reservation parks before dispatching pending tools or
returning output. Settlement remains durable even when it causes the pause. Hard safety
bounds remain errors, distinct from the optional human allowance.

A token approval records exact checkpoint/revision/request-or-settlement identity, observed
usage, old allowance and proposed new allowance. Proposed increase is at least one original
allowance tranche and enough for the already known next reservation; it is displayed before
approval. Approval authorizes only the recorded new total, exactly once; never resets counters,
turns, unresolved reservations, deadline, failure allowance or model-call safety limits.
Use distinct token_allowance decision subject, exact wait/job/parked-attempt and immutable
per-decision audit. Reuse authorized admin/owner reviewer and notification machinery.
Reject/expire/cancel leaves no runnable continuation. Old decision replay is side-effect
free, even while later approval is waiting. The scope is shared root usage, as accepted06-D2 requires. Concurrent descendant
pauses may each have an exact local decision. Proposals save a new total allowance, not an
unconditional increment: approving uses max(current allowance, proposed allowance), so
two proposals based on the same old total cannot double-grant. Each parked execution still
requires its own authorized decision to reopen. Current admin/owner eligibility and root
reviewer membership are checked on approval; missing/revoked eligible reviewers fail closed.

Each model reservation, subsequent pending-tool dispatch and final-result publication
checks the shared allowance under its existing local-run fence; no in-flight remote work
is claimed to be revoked retroactively. When a settled response exceeds its reservation,
usage and the local approval park commit together. A sibling next reaching a guarded
boundary also parks if allowance remains exhausted. Token lock comes after local scope
and authority locks. Approval locks only its local run and shared usage, not the root run.
The approval is explicit about current usage and proposed total; prices never appear as
authorization. Ordinary root model-call exhaustion remains terminal and unchanged.

## Sequential saved effects and delivery waits

Process only checkpoint.pending_call before requesting another turn. Prepare each saved
invocation durably before ActionService and record matching receipt before progressing.
HTTP/MCP/message direct tools exclusively use existing action policy/marker/receipt paths.
Preserve operation-digest dedup. Approval parks the execution while the saved call remains
pending; successful continuation uses the new attempt ordinal, same checkpoint and call.
Uncertain effects remain blocked by existing recovery/finality rules, never sent again.

For outbound messaging, preserve actual effect wait ID for model-tool publication. On
receipt, complete that wait and reopen the agent job with continuation_count debit only;
resume saved call and consume receipt. Do not complete agent.run with the delivery receipt.
Effect waits need per-invocation provenance, exact parked-attempt witness and separate audit
identity to allow multiple sequential deliveries. Ordinary message steps keep their current
complete-step semantics. SQL guards distinguish the two transitions using immutable origin
and checkpoint association. Existing one-active-wait rule remains.

Classify agent.run as marker-owned only once every effect passes ActionService, retaining
unknown model reservations across crash/retry. Integrate confirmation, timeout, cancellation,
reconciliation and operator reopen with checkpoint identity and receipt uncertainty.

## Required proofs before acceptance

- Real scripted provider: saved model turn, sequential tool calls, two approval cycles and
  delivery waits, restart after each boundary, final validated output; actual provider/effect
  call counts prove no duplication. Identical and distinct operation identities covered.
- Exact attribution into analytics by model/workflow/agent, prices optional, reported/mixed/
  estimated/unresolved usage, duplicate reserve/settlement, transaction rollback.
- Below/exact/over allowance, request larger than initial allowance, response over reservation,
  approval extension and second pause; unknown requests remain charged; hard limits unchanged.
- Competing reserve/reserve, settle/settle, approve/approve, approve/cancel/expiry, revoked
  reviewer, stale worker, actor removal, wrong tenant/job/attempt and prior approval replay.
- Frozen profile/default/empty tool selections, changed agent defaults, target rebind, current
  revocation, model-invented tool names/targets, untrusted context and output repair.
- SQL provenance/mutation guards and failed final statements preserve atomic state. Fresh
  migration/schema equality, SQLx, formatting/offline all-target/strict Clippy.
- Focused runtime/budget/approval/message tests then broader workflow/harness DB suite for
  shared recovery changes. Full phase06 stack/lease/all-target suite remains the final gate.

## Token facts and pre-I/O identity

Use model_usage_calls as the single per-request accounting ledger. Token admission sums
settled input/output and unresolved reserved input/output for the exact root, under the
existing root usage serialization row; add the matching bounded root index if needed.
Keep original/proposed/authorized allowance facts in the root budget/decision records,
not a second per-call ledger. Settlement and reserve take the same serialization lock.
Checked arithmetic refuses overflow and does not drop unknown or partial token counts.

A blocked model request has not been made: do not insert a model_usage_calls row for it.
Save its prepared reservation and request fingerprint with the workflow checkpoint's
blocked proposal. Approval only authorizes allowance and reopens the job. A freshly
claimed worker then consumes the proposal and reserves the exact request atomically with
model-call debit, following the explicit admission protocol below. Recomputed history/
schema/preamble must match; changed preparation is refused. This prevents a denied request being reported as an unconfirmed provider call
and prevents approval authorizing a different model request. Usage-triggered pause after
a response instead binds the already settled request, leaving any tool/final output saved.
When hard model-call or checkpoint bounds refuse, no token decision can override them.

## Approval failure and public proposal

The token decision view shows measured/estimated/unresolved token contributions, original
allowance, current authorized allowance, and exact proposed new total. It never labels a
reservation as reported usage or shows missing price as zero. Keep the existing accessible
approve/reject flow; token choice data cannot replace the saved proposal amount.

Token denial or expiry terminally closes the parked execution without an automatic error
successor or operator retry that bypasses the allowance. Root token authorization records
that continuation was refused; new descendant model/effect admission must not bypass that
fact. Cancellation likewise cannot be undone by a late approval. Any root-wide stop must
be implemented as a shared budget fact consumed under each local run's normal lock order,
never a transaction acquiring every child run while holding its parent's lock.

Publication changes require a new compiler/bundle revision with exact SQL budget admission
support in the same batch; carry forward06-F002's production activation regression and
reject the previous revision. No revision fallback or old-row decoder. Existing public
agent.run inputs remain valid when no token allowance is configured. If token policy is
configured, explicit root reviewers are required and validated before admission; no model
chooses approvers. Admin membership changes are rechecked at actual pause and submission.

## Exact new associations

Checkpoint table has composite FKs proving company/run/execution/job and agent company,
unique(company,execution), immutable prepared inputs/target map and monotonic checkpoint
revision. The embedded RunIdentity.task_id is the workflow job UUID; it is not a lease.
Each mutation verifies it against the relational owner and RunCheckpoint::validate/apply.
Workflow checkpoint stays internally Active while its owner is waiting; only an exact live
WorkflowFence may mutate or use it. Terminal/cancelled owners make it non-executable even
if retained for diagnostics. No separate recovery queue or second ownership mechanism.

A blocked pre-I/O request is stored alongside the checkpoint with exact decision identity,
request fingerprint and reserved amounts; it is not yet an active model reservation. It
is consumed only atomically with its granted reservation; terminal owners may retain it
for diagnostics but can never consume it. Settlement-
triggered approval binds the committed turn instead. SQL guard shapes distinguish these.
Token decision adds exact old/proposed allowance and blocked request/settled request
association, requiring job/parked attempt as for action approvals, with no action FK.
Business/action/token subjects have disjoint required/null field shapes.

Model-message delivery wait binds immutable action origin, saved invocation, wait and
parked attempt. Generic one-shot timer/event uniqueness stays; multiple historical effect
waits are allowed only with distinct exact invocation provenance and one active wait.
Use wait-bound effect audit kinds/provenance, rather than weakening ordinary audit event
uniqueness. Deferred reopen guard requires completed delivery receipt and exact waiting
attempt; action approval and token approval cannot serve as its continuation witness.
Replayed old delivery completion cannot reopen a later parked attempt.

Provider construction reuses Rig ProviderRegistry/ResolvedModelRequest. Persist only the
trusted model connection reference/endpoint policy identity with the published preparation,
never a secret. Resolve current authorized credentials for that same connection at runtime;
rotated credentials can be used, but changing an agent's connection cannot redirect a saved
run. Validate company/agent/connection ownership and provider agreement before open/reserve.
A provider error is redacted through the existing transport mapper; no prompt/key/endpoint
leaks into usage analytics. Scripted ProviderRegistry factories supply deterministic real
Rig-protocol tests without external calls. Native workflow runtime must not initialize MCP
unless a saved selected MCP call actually enters the ActionService bridge.

## Known usage above reservation

Existing commit_turn/validate_ledger rejects usage above the reservation before settlement.
Change this shared checkpoint rule explicitly: bounded valid received counters are retained,
and hard-budget accounting uses max(reserved,received) per request with checked sums.
If received counters exceed hard per-request or aggregate policy, persist the turn/usage
in a terminal BudgetExceeded checkpoint state before returning failure. No pending tool,
final output or later provider request can execute from that state; token approval cannot
revive it. Counters above the analytics integer bound are malformed provider data and are
refused observably. If only the optional root allowance is exceeded while hard policy still
holds, settle usage and park for token approval atomically. Extend ledger validation and
SQL projection fault/replay tests together; no clamp or unknown-as-zero accounting.

Extend agent.run with optional with.approval using the existing action approval schema,
static reviewer checks and runtime deadline validation factored once from registry/actions.
This config authorizes requesting review, not executing tools. It does not replace the
root token-reviewer policy and neither approval can authorize the other's subject. Missing
protected-action review policy still fails closed before provider/effect entry. Selected
tool declarations must have a concrete ActionService dispatch binding; unsupported bindings
fail preparation explicitly rather than becoming prompt-only tool names or silent defaults.

The workflow preamble is frozen data, unlike the legacy CompiledRun's dynamic clock/facts.
Do not insert a new wall-clock timestamp on retry or approval resume. Request fingerprints
cover canonical provider/model, frozen preamble, saved history, tool declarations, repair
reason, output schema and generation limits. Credentials are excluded. Thus pre-I/O token
approval can compare the reconstructed request without silently authorizing changed inputs.

## Reuse the existing checkpoint table

Extend task_harness_runs rather than duplicate its checkpoint/schema/response constraints.
Add an explicit legacy/workflow owner discriminator and native run/execution/fence/prepared
fields with disjoint SQL shapes: legacy principal/ownership_version required only for the
legacy branch; native company/run/execution/job plus generation/worker/attempt required for
workflow writes. Never populate a fake legacy owner or version for workflow. Replace only
this table's blanket require_legacy_auxiliary_task trigger with the stronger discriminated
owner guard; all other legacy auxiliary guards stay unchanged. Native execution uniqueness
is a partial unique key; legacy continuation identity remains its existing key. Prepared
workflow data and blocked request facts are mandatory/allowed only on the native branch.

Legacy persistence remains explicitly queue_kind=legacy scoped. Trace every reader/writer
of task_harness_runs and prove legacy approval/MCP/outreach/supersede/final-output consumption
cannot see or mutate native rows. Native methods expose only WorkflowFence and exact scoped
checkpoint operations through a cohesive application-owned port. Shared row serialization,
RunCheckpoint mutation and model-usage projection helpers are reused behind these separate
authority boundaries. This is a native owner extension, not a legacy lease compatibility shim.

A denied/expired root allowance is a durable typed refusal, not a model-call-count failure.
Existing activation admission and ActionService entry consult that shared refusal so an
already admitted child cannot route around a denied token decision. In-flight settlement
may still record received truth, but cannot publish a successful step or authorize another
effect. Other pending token approvals cannot clear the refusal. Propagate the saved token
refusal reason through recovery instead of inventing an ordinary retry or monetary error.


## F007 correction — approval, admission and provider-entry protocol

These are separate transactions and authorities:

1. **Park before I/O.** The current live fenced worker saves the blocked request UUID,
   fingerprint, amounts and immutable decision proposal, then closes its attempt and parks
   the job. No model reservation, model_usage_calls row or root model-call debit is created.
   The proposal retains checkpoint revision at pause and exact wait/job/attempt identity.
2. **Human approval.** Under existing decision/reviewer/local-run locks and shared root usage
   lock, verify the current proposal, reviewer authority, deadline and root refusal state.
   Complete the exact decision/wait, authorize max(current allowance, proposed total), and
   reopen only its parked job as pending, incrementing continuation_count. This transaction
   does not mutate the checkpoint, consume the blocked request, reserve tokens/model calls,
   create provider-entry permission, or invoke a provider. No worker fence is fabricated.
3. **Fresh worker claim.** A real workflow claim produces a fresh live WorkflowFence. Open
   the saved checkpoint and charge its existing execution-time/continuation safety policy.
   The saved request must match the immutable proposal and reconstructed fingerprint. A
   revision change solely recording the new execution reservation is allowed; changes to
   prepared inputs, history, model/tool/schema/repair state are not. The native write uses
   the current expected checkpoint revision and retains the original pause revision as audit.
4. **Fenced request admission.** In one bounded transaction, lock authority/local owner,
   checkpoint and shared usage in the established order; verify exact live fence, current
   deadline, approved proposal, absence of an active request for this same generation,
   hard checkpoint/root model-call safety limits, current root refusal and token allowance.
   Sibling usage after approval can exhaust allowance: in that case create a new exact
   proposal/wait for the still-unconsumed request and park this attempt, with no model-call
   debit or usage row. The earlier approval cannot authorize spending past the current total.
   If allowed, append the exact reservation to RunCheckpoint, project model_usage_calls,
   debit the root model-call receipt, set the active request's owning fence/generation and
   consume the blocked-request pointer atomically. Recheck lease before commit.
5. **Provider entry.** Only a fresh, definitely committed admission returns an internal
   single-use `NewAdmission` permit. The shared driver consumes it for one immediately
   awaited, cancellation/deadline-bound provider invocation. Exact existing reservation,
   CAS conflict, lost ownership, unresolved reservation, or ambiguous commit acknowledgment
   never returns this permit. Neither `AlreadyApplied` nor a successful lookup of an old
   reservation is an entry permit. Concurrent refusal after a committed admission treats
   that request as already admitted/in-flight; it may record received truth but authorize
   no subsequent tool, output or provider progression.
6. **Settlement.** Only the matching current live fenced owner may settle its admitted
   request and clear the active-request slot, atomically recording turn and usage. Lost
   owners cannot publish a late response into a newer execution. Known saved turns are
   reused on recovery, and pending tools are processed before any new model request.

The native checkpoint row carries the currently active request ID and admitting generation
(with the exact attempt/worker witness), separate from its last-write fence. This is an
ownership slot, not a second per-call ledger. A second driver using the same live generation
cannot admit a fresh request while that slot is unresolved, even if it reloads a newer
checkpoint revision. Exact repeated admission returns `PreviouslyAdmitted`, with no permit
and no additional charge. Internal permit construction/consumption is private and non-clone.

After crash or ambiguous admission acknowledgement, the stored reservation remains charged
and unresolved. Only a later genuinely claimed generation, proving retirement of the prior
owner, may release its active ownership slot. It never reuses that request ID for provider
entry. If policy permits continued model work, create a new request ID/reservation and debit
it separately, retaining the unresolved old reservation and every hard limit. Do not treat
an expired lease, approval, or request lookup as proof that the old provider call did not
happen. A response lost before durable settlement may therefore require a new charged call;
exactly-once logical model inference is not claimed. A durably saved turn or action receipt
is never repeated. Approval of a blocked, never-admitted request retains its original ID
until one fresh admission consumes it; afterward it is never a blocked request again.

Required F007 proof matrix (real scripted provider counts, request IDs and ledger assertions):
- crash after approval/before claim: zero calls/debits until fresh claim, then exactly one;
  duplicate approval has no checkpoint/usage effects and cannot reopen a later wait;
- crash after reservation/before I/O: original request entered zero times; later generation
  never sends its ID, retains its charge, and may admit one new separately charged request;
- crash after provider entry/before settlement: original ID entered once and remains unknown;
  recovery never re-enters that ID, and any permitted fresh call has a new ID/additional debit;
- crash after turn settlement: replay saved response/tools without another model call;
- competing claims and duplicate same-fence drivers: at most one fresh admission/entry permit
  and debit for that request, no parallel same-generation request while the slot is active;
- sibling consumption/refusal between approval and claim: fresh admission rechecks shared
  usage and either reparks without debit or refuses terminally; old approval cannot bypass it;
- ambiguous commit acknowledgment: no provider entry from the ambiguous/replayed result,
  stored charge retained, bounded recovery without duplicate request ID or counter reset.
