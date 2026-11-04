# 01 — Agent history contracts and conversation identity

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and implementation order

An agent saves its inputs and outputs and loads its prior exchanges automatically. Workflows
provide the current task input; they do not need to assemble conversation history for the default
`agent.run` path. A later feature may let a workflow supply an explicit conversation. This plan
establishes one selection boundary for that future work without implementing override modes now.

This is a proposed implementation plan, not implemented behavior. Implement the numbered files
in order; every phase depends on its predecessors. The final verification gate is in phase 07.

**Prerequisite:** phases 01 and 03 of [knowledge access levels](../knowledge-access-levels/README.md).
Those phases provide `AccessLevel`, member levels, each run's knowledge ceiling and content level,
message stamps, and the release check on every egress. This plan stamps and filters exchanges
with them from the first schema. History must never become a way for content above a reader's
level to reach that reader.

| Phase | Deliverable |
| --- | --- |
| [01 — Contracts and identity](01-contracts-and-conversation-identity.md) | Ownership, conversation scope, typed input/output contracts |
| [02 — Durable exchanges](02-durable-exchanges-and-completion.md) | Input capture, accepted output, immutable linkage, failure records |
| [03 — Ordering and recovery](03-turn-ordering-and-recovery.md) | Serialized turns, durable waiting, fenced recovery and cancellation |
| [04 — Automatic context](04-automatic-context-and-frozen-preparation.md) | Bounded history selection, model messages, frozen retry context |
| [05 — Execution integration](05-callers-continuations-and-channel-integration.md) | Roots, children, human continuations, samples, channel separation |
| [06 — Operations and retention](06-history-views-retention-and-operations.md) | Authorized inspection, bounded retention, documentation and diagnostics |
| [07 — Verification](07-integration-and-verification.md) | End-to-end acceptance, competing claimants, final CI gate |

## Shared design invariants

- The agent application/runtime owns history selection and recording. Persistence implements
  those operations. The existing workflow scheduler, harness checkpoints, and action ledgers
  continue to own execution; this work adds no second agent engine or general memory service.
- One logical exchange contains the original structured input supplied to an `agent.run`
  activation and its accepted, schema-valid output. Store the input separately from selected
  history and the assembled provider prompt. Never save the expanded prompt as the next logical
  input: that would recursively copy earlier history and count it repeatedly.
- Capture input before provider entry. Commit accepted output with the existing durable final
  checkpoint. Retries, lease replacement, repair calls, and human continuations of that same
  activation do not create additional logical exchanges.
- Saving and loading are runtime guarantees, not optional model tools. No explicit
  `context.history` step is needed for ordinary agent continuity. Keep that existing step's
  frozen thread-message semantics for workflows that explicitly use it.
- History represents what the agent received and produced, not what a recipient received.
  A valid agent output survives a later workflow failure, transformation, suppressed response,
  or failed delivery. Channel publication and delivery remain separate linked records.
- Previous completed input/output pairs are the default conversation. Failed/cancelled inputs
  remain inspectable records but are excluded from the automatic prompt; no assistant answer is
  fabricated. Invalid repair candidates, tool traces, and provider continuation tokens stay in
  the execution checkpoint rather than becoming ordinary cross-execution conversation turns.
- Freeze selected history once the exchange obtains its turn, before provider entry. Retries
  replay that preparation; later exchanges see newer completed turns. Current authority remains
  a live gate even for frozen preparations.
- `agent.run` gains this default. `ai.classify` and `decision.agent` retain their existing isolated
  model-step semantics. A shared implementation must not silently enable history for those steps.
- History is tenant-, agent-, conversation-, and visibility-scoped. Reusing an agent never grants
  access to every interaction that agent has had. Credentials and approval tokens are not history
  payloads; retain existing secret separation and input validation.
- History is also level-scoped. Each exchange carries the `AccessLevel` of the content it was
  built from: the run's content level, which only rises. A call loads only exchanges at or below
  its run's knowledge ceiling, and loading them raises the run's content level. An exchange above
  the ceiling is invisible to that call: it is not selected, not counted as omitted, and leaves no
  marker. Stamps never decrease. Anything an exchange's output reaches still passes the knowledge
  plan's release check. Example: a level-5 rep's earlier pricing exchange in a thread is not in
  the prompt once a level-0 customer is CC'd, so "summarise this thread for them" cannot repeat it.

## Batch 01A — Define identity and access boundaries

1. Add typed conversation and exchange identities in the owning inner layer. Identify the stream
   by company, agent, and a trusted conversation identity; keep agent revision as exchange
   provenance rather than starting a new conversation for each publication. Same-agent calls in
   different workflows can share a stream only when their trusted scope and authority permit it.
2. For a root call attached to an authorized channel/thread, derive the default conversation from
   that association and its visibility boundary. Do not derive it from model-written JSON,
   mutable slugs, agent ID alone, or a workflow run ID that changes on every incoming message.
3. A standalone caller may reuse an authorized session identity through the normal admission
   boundary. Without one, allocate a fresh private session and retain it for that run's calls
   and retries. Scheduled runs without an explicit trusted session start separately; never use
   a company-wide or agent-wide fallback. Expose only the minimal session reuse contract needed
   by existing callers, not a workflow conversation-override feature.
4. Record the trusted audience/access boundary and enforce current rights to the stream and its
   inputs. Thread membership alone must not expose private workflow inputs, reviewer data, or
   another caller's private session. Use distinct sessions for private/reviewer/child contexts
   when the caller cannot establish the same access boundary. Reject unauthorized reuse rather
   than silently loading a partial or foreign conversation.
   The level boundary is the exception to "never load a partial conversation". A conversation
   stays one stream whatever its participants' levels, and each call sees the part at or below its
   ceiling. Do not split the stream by level: a thread whose audience drops to 0 and later returns
   to 5 sees its level-5 exchanges again.
5. Define default child isolation now: child invocations receive a private branch identity tied
   durably to their child admission. They cannot acquire their waiting parent's active stream.
   Explicit parent input is the way information reaches a child in this scope; phase 05 handles
   the concrete producers. Samples have separate identities and cannot read production history.

Focused checks: future `agent_history_identity_` unit and database tests for thread reuse across
root runs, standalone defaults and reuse, agent/revision distinction, cross-tenant and private
scope refusal, sample isolation, and deterministic recovery of allocated identities. Also add
`agent_history_level_` tests: a thread conversation keeps one stream across audience changes,
and a lower-ceiling call sees only its part of that stream.

## Batch 01B — Define logical exchanges and integration ownership

1. Define input, accepted output, provenance, order, access level (`AccessLevel`), history-selection
   boundary, and lifecycle states using existing typed values and execution identities. Distinguish queued, active,
   completed, failed, and cancelled exchanges without duplicating the whole workflow state
   machine. Completion of an exchange means accepted agent output, not workflow or delivery success.
2. Preserve exact bounded structured input and output, including JSON scalars and arrays. Record
   agent revision/model and the output contract used by that exchange. Historical output is data
   from its original contract; do not revalidate or reinterpret it under a newer call's schema.
3. Assign the decision about history selection to one preparation operation. Keep domain types
   independent of SQLx/Rig. A narrow application port is appropriate where persistence is needed;
   no provider abstraction, summary service, embeddings, search index, or generic strategy registry
   is needed. Workflow-supplied conversation and model-generated summaries remain future work.
4. Coordinate shared types with [autonomous agent ownership](../autonomous-agent/01-capability-contracts-and-storage.md)
   and [channel response ownership](../channel-handles-send/01-contracts-and-response-ownership.md).
   Their implementation is not claimed by this plan. Reconcile the checked-out state at execution
   time; use existing frozen agent selection if independent agent revisions have not landed yet.
   Avoid two competing definitions of identity, revision selection, or response authority.

Focused checks: `agent_history_contract_` for structured-value round trips, state transitions,
schema provenance, and model-step eligibility. Run affected agent preparation/registry tests and
offline all-target compilation after shared type edits.

## Current ownership anchors

- [Agent preparation types](../../src/application/workflow/agent.rs), `PreparedAgent::new` and
  `spec`: currently wrap the supplied context as a prompt and describe one checkpoint.
- [Preparation adapter](../../src/adapters/persistence/workflow/agent_preparation.rs),
  `prepare_agent`: frozen workflow inputs and selected agent configuration.
- [Harness runtime](../../src/adapters/harness/rig/workflow.rs), `WorkflowAgentRuntime::run`:
  checkpoint opening, model execution, tool/decision continuation.
- [Checkpoint state](../../src/application/services/harness/runs/state.rs), `RunCheckpoint`:
  existing per-execution messages, turns, tool invocations, and final output.
- [Checkpoint persistence](../../src/adapters/persistence/workflow/checkpoints.rs),
  `open_workflow_checkpoint`, `matching`, and `open_on`: ownership and immutable preparation.
- [Provider history](../../src/adapters/harness/rig/protocol.rs), `history`: currently restores
  one checkpoint's model/tool protocol, not a persistent cross-execution conversation.

Re-query graft and trace callers before editing each owner. These are scoping anchors; explicit
module paths and symbol locations may change as the related plans are implemented.

## Verification rules

All new test prefixes in this directory are future requirements. For each batch, list its filter
with `cargo test --locked --offline --lib PREFIX -- --list`, confirm a nonzero intended selection,
then run `cargo test --locked --offline --lib PREFIX` against a disposable database. Record exact
commands, selected/executed counts, results, and unavailable checks. Use focused regressions
during implementation and the final gate in [07](07-integration-and-verification.md) once all
phases are integrated. Every new concurrency protocol needs actual competing claimants.
