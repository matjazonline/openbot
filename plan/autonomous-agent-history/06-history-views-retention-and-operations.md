# 06 — History views, retention, and operations

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [05](05-callers-continuations-and-channel-integration.md). Authorized operators can
inspect which exchanges informed an agent call and diagnose waiting/recovery without confusing
agent output with published communication. History growth and cleanup have enforced bounds.

## Batch 06A — Inspect history through existing views

1. Extend existing agent/run readers with bounded paginated conversation/exchange provenance:
   source run/execution, agent revision, order, state, selected history boundary, omitted pairs,
   and links to the existing checkpoint/output. Reuse current pages and APIs rather than adding
   a second conversation product or separately maintained overall-success state.
2. Show original input and accepted output only under the history's current authorization policy.
   Customer thread visibility does not grant access to private workflow input or full agent
   history. In addition, show an exchange's input and output only to a viewer whose level is at
   least the exchange's `access_level`, using the knowledge plan's `visible_to` rule (05A). A
   lower viewer sees a "Restricted exchange" placeholder with order, state and timing, and no
   payload. Selected-history lists show restricted entries the same way. Exchanges excluded by
   level are not presented as omitted from a preparation. Enforce tenant/session scope on detail routes, pagination cursors, source links,
   continuation views, and any live updates; escape stored content in rendered pages.
3. Distinguish waiting for a conversation turn, running, waiting for human input, failed/cancelled
   before output, and completed exchange. Workflow and channel statuses remain separate. Label
   an agent output as produced, not delivered; a later workflow failure cannot hide its existence.
4. Expose selected/omitted history facts without promising that every past exchange fit into the
   prompt. Preserve exact frozen-preparation inspection while the record is retained; make
   retention expiry explicit rather than displaying an incomplete reconstruction as exact.
5. Add bounded structured events for turn queueing/acquisition/release, preparation selection,
   recovery, refusal, completion, and pruning. Log IDs, counts, states, and level numbers, never
   input/output bodies, credentials, provider continuations, approval tokens, or high-cardinality metric labels.

Focused checks: `agent_history_views_` for authorized pagination, foreign/private/sample denial,
escaped content, source lookup outside the current page, omitted-history display, and independent
agent/workflow/delivery states. Add `agent_history_level_` view cases: the placeholder markup
contains no input or output, pagination cursors and source links do not reveal restricted
payloads, and the owner sees everything. Run affected agent/runtime and run/decision browser protocols;
add fixture-backed cases when UI behavior changes, including keyboard/accessibility behavior.

## Batch 06B — Enforce retention and document recovery

1. Extend the existing retention/cleanup owner where it fits. Define and enforce finite retained
   exchange/payload bounds and cleanup batch limits; prompt truncation alone does not bound stored
   history. Use existing policy settings when applicable, otherwise choose documented code-owned
   limits. Avoid a new configurable policy framework for one fixed behavior.
2. Retain records/payloads needed by queued or active exchanges, live waits, pinned preparations,
   and required audit/source relationships. Completed preparation snapshots can expire under the
   stated retention policy. Do not let cleanup invalidate active replay or cascade-delete history
   merely because a source workflow finished. When pins prevent cleanup at a hard bound, apply
   controlled admission backpressure rather than unbounded growth or deleting required data.
3. Record a durable retained-history boundary and omission/expiry facts. Future calls can load
   only the retained recent history; never fabricate summaries of pruned content. Coordinate
   agent retirement, session/thread deletion, and company deletion with existing lifecycle rules.
   Recheck authority if a source scope is removed while work is queued or waiting. A member's
   lowered level applies at their next call and at any resume (04A). Stored exchange stamps never
   decrease, and cleanup never rewrites them.
4. Reuse safe existing cancellation/recovery controls for stuck turns. A retry targets the same
   logical exchange; a genuinely new task creates a new exchange. Do not expose a reset button
   that erases active fencing or repeats effects. History reads/recovery do not rerun models.
5. Update [workflow operations](../../docs/workflow-operations.md),
   [verification guidance](../../docs/workflow-verification.md), agent help, and representative
   executable examples. Explain automatic input/output history, scope, retained-window bounds,
   private child sessions, human continuation, output versus delivery, and future workflow
   conversation support. Document only implemented controls and configuration keys.

Focused checks: `agent_history_retention_` and `agent_history_controls_` with cleanup racing
history selection/continuation, pinned-payload protection, bounded batches, admission pressure,
agent/session/company lifecycle, authority revocation, safe retry, and deterministic retained
history. Use a dedicated database for unscoped cleanup. Run affected lifecycle and query-plan tests.

## Acceptance

Operators can explain the exact context selection and waiting state without exposing private
data. Both prompt size and stored growth are bounded, and cleanup cannot corrupt a live exchange.
Record commands/results and any operational limits in [07](07-integration-and-verification.md).
