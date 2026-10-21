# 06-D6 — 06B3 preparation steps: context.history, ai.classify, decision.agent, revision2 (PROPOSED)

Revision2 answers 06-R018 (06-F023, 06-F024, optional 06-F025); revision1 is retained as
`decision-D6-revision1.md`. Only section A, E, F and the verification plan changed.

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
`{items: [item] (0..256), token_count, omitted}` with
`item = {message_id, at, role, audience, entry_kind, author, subject, body, truncated}`:
`message_id` identifier; `at` the thread-association time (RFC 3339); `role` ∈
`human|agent|system`; `audience` ∈ `external_conversation|internal_only|legacy_unclassified`
and `entry_kind` ∈ `conversation|note|delegation|system_event` (06-F023: every item keeps its
disclosure boundary as typed data, the same facts legacy agent history labels, so a downstream
`agent.run` or `decision.rule` can tell internal-only, delegation and legacy content from the
external conversation; nothing is dropped silently); `author` display label, `subject`, `body`
text (body ≤16384 characters, longer bodies cut with `truncated: true`); `token_count` and
`omitted` non-negative integers.

Selection, in one read-only statement scoped by `company_id` + `run_id`: the run's frozen
`workflow_history_members` joined to the messages, internal notes excluded (the legacy rule:
private notes are never agent history); every other member is eligible whatever its audience.
For a message-triggered run the snapshot includes the triggering message (it is a thread member
at admission). Accumulate newest first and **stop at the first item that does not fit** (the
result is always a contiguous most-recent suffix, never a history with holes), then return the
items in chronological order; `omitted` = eligible members not returned.

Budget (06-F024): one item "fits" while both bounds hold — (1) the serialized items' total is
≤ `max_tokens`, counted with the Rig reservation rule (one UTF-8 byte of serialized JSON = one
token, conservative); (2) the complete serialized output envelope is ≤ the claim's
`max_result_bytes` (the run's `max_context_bytes`, the cap `supervise_io` enforces). The
handler applies the minimum at run time, so a long thread yields fewer items and a larger
`omitted`, never a terminal `workflow.invalid_result`. If even the newest eligible item does not
fit, the result is `{items: [], token_count: 0, omitted: n}`. A run without a thread association
returns `{items: [], token_count: 0, omitted: 0}`.

The handler (`WorkflowHistoryRuntime`, persistence-owned) performs no model call and no effect;
the committed step output is the frozen result (ReuseCommittedResult). Errors: storage errors are
retryable; a malformed stored message (unknown role/audience/entry kind) is a terminal
`context.invalid_history`.

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
them). `memory.save` keeps its unknown-effect classification. Optional 06-F025 adopted: one
predicate `actions::effect_free_kind(kind)` (pure/wait/decision kinds plus these three) replaces
the two hand-kept lists in `recovery.rs` and `pending_recovery.rs`, with a unit test over every
registry kind.

**F. Memory deferred.** `memory.load`/`memory.save` contracts, recovery mode and catalogue entry
stay unchanged and unhandled, like the phase-07 child kinds; no memory code changes. The phase
plan's memory acceptance item ("Memory retrieval is visible, reusable, scoped…") is recorded as
deferred by the user's decision. Optional 06-F025 publication refusal is not adopted now: no
production worker exists until 06C/08, so no run can wait on an unhandled kind before then.
Recorded 06C obligation: when the production worker is wired, publication (or binding) must
refuse every step kind that has no production handler, so no run can wait as Unsupported.

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
  internal notes, never drops an item's disclosure boundary, and never silently truncates:
  `omitted` and `truncated` report it. Its output always fits the run's result byte cap.
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
chronological order with internal notes excluded and every item's `audience`/`entry_kind`/`at`
exact (external, internal-only, legacy, delegation, system event); the triggering message
included; max_tokens budget newest-first, stopping at the first non-fitting item with `omitted`;
a `max_context_bytes` smaller than `max_tokens` bounds the output (boundary: output exactly at
and one byte over the cap) and the step completes rather than failing `workflow.invalid_result`;
newest item alone too large → empty items with `omitted`; 256 cap; long body truncation;
non-thread run empty; foreign run/company isolation. Unit: `effect_free_kind` over every kind. Scripted
provider e2e: classify returns a declared label and routes; invalid label repaired then terminal;
decision.agent selects a declared route with validated data; an undeclared choice is never
accepted; requests carry no tools; tool removal leaves classify runnable while agent/model
revocation refuses it before provider entry; token pause through `supervise_io` for a classify
step; crash after provider entry retries with a new charged request. Then fmt, offline
all-target check, strict Clippy, fresh migration, SQLx, focused + broad workflow DB suites.
