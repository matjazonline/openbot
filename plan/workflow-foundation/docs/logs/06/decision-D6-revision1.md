# 06-D6 — 06B3 preparation steps: context.history, ai.classify, decision.agent (PROPOSED)

Architecture gate for 06B3. No backward compatibility: edit
`migrations/20260817000000_init_schema.sql` in place and reset test databases; no backfill,
legacy decoder or staged rollout. Builds on accepted 06-D4 (shared durable Rig driver, token
pause, ActionService effects) and 06-D5 (agent-owned capabilities, frozen at publication,
current revocation).

## User decisions (chat, 2026-10-20; quoted in TASK.md)

1. `ai.classify` takes an `agent` input and uses that agent's model and instructions, with no
   tools ("as recommended").
2. Memory is not implemented now: no `memory.load`/`memory.save` handlers, scopes or grants.
3. `context.load` is renamed `context.history`: the thread history frozen at run admission,
   bounded by `max_tokens`.
4. Confirmed model: these are ordinary workflow steps wired by refs (history before
   `agent.run`, classification for routing/context); `decision.agent` is itself a branch.

## Evidence (survey, 2026-10-20)

- All five kinds are registry/compiler contracts only; no handler supports them
  (`worker.rs` returns Unsupported). `WorkflowAgentRuntime::supports` is `agent.run` only and
  `prepare_agent` hard-codes that kind (`agent_preparation.rs:162`).
- `ai.classify` has no agent/model input (`families.rs:44-57`), so it has no model source.
- Message-triggered thread runs freeze history membership at admission in
  `workflow_history_members` (`admission_history.rs:7-38`, one MVCC snapshot, ≤10000); there
  is no reader. Legacy agent history excludes internal notes (`thread/views.rs:393-432`).
- `decision.agent` completion already reads `output.choice` as the route
  (`completion.rs:70-82`) and validates `{choice, data}` against the step contract
  (`families.rs:70-76`, `batch.rs:117-161`). No choice-eligibility predicate exists anywhere.
- Recovery treats `memory.save`, `ai.classify`, `decision.agent` as unknown-effect kinds
  (`recovery.rs:138-144`, `pending_recovery.rs:132-176`); exhausted attempts park the run in
  reconciliation instead of routing the error.
- Runtime `current_on` checks agent ownership, every frozen tool ⊆ `workflow_tools`, every frozen
  skill still linked, and the model offer (`agent_preparation.rs:66-104`).

## Decision

**A. `context.history` (rename of `context.load`).** Registry contract
`with: {max_tokens: integer 1..131072}` (the `sources` input is removed). Output
`{items: [{message_id, role, author, subject, body, truncated}] (0..256), token_count, omitted}`:
`message_id` identifier; `role` ∈ `human|agent|system`; `author` display label, `subject` and
`body` text (body ≤16384 characters, longer bodies cut with `truncated: true`); `token_count`
and `omitted` non-negative integers. Selection, all in one read-only statement scoped by
`company_id` + `run_id`: the run's frozen `workflow_history_members` joined to the messages,
internal notes excluded (same rule as legacy agent history), newest first, adding items while
the running total stays ≤ `max_tokens` and ≤256 items, returned in chronological order;
`omitted` = eligible members not returned. Token accounting uses the reservation rule of the
Rig driver (one UTF-8 byte of the serialized item = one token, conservative). A run without a
thread association returns `{items: [], token_count: 0, omitted: 0}`. The handler
(`WorkflowHistoryRuntime`, persistence-owned) performs no model call and no effect; the
committed step output is the frozen result (ReuseCommittedResult). Errors: storage errors are
retryable; a malformed stored message is a terminal `context.invalid_history`.

**B. `ai.classify` agent input.** Contract `with: {agent, context, output_schema}` (all
required); `agent` follows `agent.run`'s rule (literal key or finite parameter enum, D5 agent
universe); `output_schema` keeps the finite-labels check. Publication captures and verifies the
agent snapshot exactly as for `agent.run` (agent universe, capture, publish-only equality).

**C. One durable driver for all model steps.** `WorkflowAgentRuntime` supports `agent.run`,
`ai.classify` and `decision.agent`. `PreparedAgent` records the step (`ModelStep::{AgentRun,
Classify, Decide}`):
- `AgentRun` — unchanged (tools, skills, `output_schema`).
- `Classify` — tool-free: no tool declarations, no skill text; preamble = agent instructions plus
  the untrusted-data note; response contract = the declared `output_schema`.
- `Decide` — tool-free as Classify; response contract
  `{type: object, required: [choice, data], additionalProperties: false, properties:
  {choice: {enum: <declared choice names in compiled order>}, data: <data_schema>}}`. Choices
  come from the trusted compiled step routes, never from inputs or the model. Every declared
  choice is eligible in this release (no eligibility predicate exists); a later predicate would
  narrow the enum deterministically before the model call.
All three use the same checkpoint row, model-call/token budgets, token-allowance pause and
supervisor confirmation, usage attribution to the selected agent, bounded tool-free repair
(MAX_REPAIR_CALLS) and terminal `agent.invalid_output`. The engine's existing completion path
validates the output against the step contract and selects the route; the model cannot name a
step, add a route or bypass the contract.

**D. Current authority covers exactly the prepared capabilities.** `current_on` always checks
agent ownership and the model offer; it checks tool grants against the *prepared* tool names
(empty for Classify/Decide) and skill links only for prepared skills (AgentRun). Removing a tool
or unlinking a skill from an agent therefore never blocks its classifier/decision steps;
removing the agent or its model still refuses them before provider entry (terminal
`agent.rejected`).

**E. Recovery classification.** `context.history`, `ai.classify` and `decision.agent` have no
effect other than ledger-charged model calls: interrupted retirement reports `SafeToRetry`
(`recovery.rs` list) and `pending_recovery::unknown_effect` returns false for them. Unsettled
model reservations stay charged and unresolved per D4; retry admits a new separately charged
request. They are not added to `marker_owned_effect_kind` (no ActionService markers exist for
them). `memory.save` keeps its unknown-effect classification.

**F. Memory deferred.** `memory.load`/`memory.save` contracts, recovery mode and catalogue entry
stay unchanged and unhandled, like the phase-07 child kinds; no memory code changes.

**G. Revisions and docs.** `CONTRACT_REVISION` → `workflow-catalogue-v1.5`; compiler
`SEMANTIC_REVISION` → 6 with the matching SQL admission revision; the previous revision is
rejected (no fallback). Help notes of touched kinds drop stale profile text. The
`triage-routing` fixture gains an `agent`. Production worker wiring stays in 06C/08.

## Invariants and failure behaviour

- A model step's effective capabilities ⊆ the frozen agent snapshot ∩ current grants; Classify
  and Decide have no tools and no skills.
- A decision output is a declared choice or the step fails (repair, then terminal); an invalid
  classification label likewise.
- History never reads beyond the admission snapshot, never crosses tenant/run, never includes
  internal notes, and never silently truncates: `omitted` and `truncated` report it.
- Lock order unchanged (authority class → local → checkpoint → root usage); `context.history`
  takes no locks beyond its read.

## Affected contracts

Registry (TYPES, families, metadata, help, examples, schemas), compiler revision, publication
(agent universe, validation, `prepare_step_inputs` for ai.classify), init SQL admission revision
and test fixtures, `application/workflow/agent.rs` (`ModelStep`, preparation), persistence
`agent_preparation.rs` (kind-aware preparation, `current_on` by prepared capabilities), Rig
`workflow.rs` (`supports`, failure mapping), new history runtime, recovery/pending_recovery
lists, fixtures using `context.load`/`ai.classify`.

## Verification plan

Pure: registry rename (old name unknown), `context.history` input/output schemas, ai.classify
requires `agent` and finite labels, decision contract enum from compiled routes, `ModelStep`
preparation (no tools/skills for Classify/Decide), recovery classification. Publication:
ai.classify agent captured/verified (literal and parameter enum). DB: history of a thread run in
chronological order with internal notes excluded, max_tokens budget newest-first with `omitted`,
256 cap, long body truncation, non-thread run empty, foreign run/company isolation. Scripted
provider e2e: classify returns a declared label and routes; invalid label repaired then terminal;
decision.agent selects a declared route with validated data; an undeclared choice is never
accepted; requests carry no tools; tool removal leaves classify runnable while agent/model
revocation refuses it before provider entry; token pause through `supervise_io` for a classify
step; crash after provider entry retries with a new charged request. Then fmt, offline
all-target check, strict Clippy, fresh migration, SQLx, focused + broad workflow DB suites.
