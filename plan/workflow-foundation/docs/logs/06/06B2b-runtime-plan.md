# 06B2b runtime completion plan (implementation decomposition, not a new gate)

No backward compatibility: edit init migration in place, fresh disposable test DBs only.
Implements accepted D4 revision2 + root-pause r3. No new architecture decision unless noted.
Author: root (user-confirmed Opus/xhigh). One consolidated independent review at the end.

## R1 — shared drive loop with closed bindings (rig/execution.rs)

`Execution` keeps one durable loop (deadline/state checks, pending call first, repair,
final validation, model turn). Binding-specific edges move behind a closed enum
`Binding::{Legacy, Workflow}`: authorization, preamble, tool declarations/supported ids,
model admission, settlement, invocation prepare/dispatch/result, final/invalid output.
Legacy behaviour is unchanged; its tests are the regression proof. Drive returns
`Driven::{Completed(output), Suspended(Option<WaitId>)}`. Request fingerprint =
SHA-256 of canonical {provider, model, CompletionRequest JSON}; credentials excluded.
Native preamble is frozen prepared text plus the same response-contract/repair suffix.

## R2 — trusted native preparation (persistence + application)

From the claim under the run/actor lock: frozen inputs of the claimed agent.run step,
`PublishedBundle::resolve_agent(company, key, capability_profile)`, current agent row
ownership (agent_id, company) and grants as the revocation ceiling, frozen model
(provider/model/max_output_tokens), and concrete tool targets (HTTP registered
connection UUID, MCP slot, built-in message kind). Unsupported bindings fail
preparation. The prepared document (frozen preamble, tool declarations with targets,
response schema, context JSON delimited as data, selection provenance) is stored once
as the immutable checkpoint preparation; resume compares the recomputed document and
refuses changed preparation. Current revocation is rechecked before each model
admission and saved dispatch without reloading changed defaults. Secret resolved per
attempt from company_model_connections(company, provider); never persisted.

## R3 — native handler (adapters/workflow_agent)

`WorkflowHandler` for `agent.run`: prepare → open → reserve execution → bind tools →
shared drive. Completed → validated JSON output; Suspended(wait) → SuspendedWorkflow.
Errors map to typed WorkflowFailure (budget/token refusal terminal, provider/transient
retryable, invalid output terminal). Blocked approved request is reloaded so the same
request ID/fingerprint is admitted (store already verifies).

## R4 — saved tool dispatch through ActionService

HTTP/MCP/message call_tool with ModelToolCallId = invocation UUID; sequential;
Committed receipt → InvocationResult; ApprovalParked(wait) → Suspended; Pending
message delivery → effect wait (R5); Uncertain/PossibleDispatch → indeterminate error.
Model-invented tool names/targets rejected (only prepared declarations dispatch).

## R5 — message delivery effect waits

Per-invocation wait provenance + exact parked attempt; delivery receipt completes the
wait and reopens the agent job (continuation debit only), not step completion.

## Proofs and gates

Scripted Rig provider end-to-end tests per D4 "Required proofs"; focused DB tests per
batch; then fmt/offline all-target/Clippy/fresh migration/SQLx; final full phase gate
after 06B3/06C. Commands/results recorded in TASK.md.
