# 06-D3 draft — successful approval continuations

Status: investigation/draft, NOT approved; no dependent edits.
No backward compatibility: edit squashed init, reset only identified task test DBs.

## Why a stable protocol boundary is needed

06B2 integrates saved model turns and multiple sequential tool approvals plus token
allowance approvals into one execution. Existing phase05 action approval is one per
execution and spends retry_count when approved. Existing task_attempts uniqueness,
fences, retirement receipts and SQL commit guards derive identity from retry_count.
Reusing this unchanged would turn successful human continuations into failed attempts;
simply removing max_retries checks would remove a safety limit and still leave ambiguous
history. This protocol must be corrected before workflow Rig/token integration.

Proposed dependency split:06B2a owns reusable successful approval continuation/history;
06B2b owns workflow Rig/checkpoint/action bridge plus token reservations/approval. These
are cohesive feature boundaries with separate recovery proof, not layer reviews.06B3
and06C obligations remain unchanged. Root acceptance of06B1 precedes implementation.

## Proposed accounting contract

- Keep retry_count as failed-attempt debit and max_retries as its bound.
- Add workflow continuation_count, nonnegative bounded at63 successful reopens so
  an agent run stays within the existing MAX_CONTINUATIONS=64 execution ceiling.
  Legacy jobs require zero. No raised bound. Terminal failure is visible at ceiling.
- Introduce database-generated attempt_ordinal = retry_count + continuation_count + 1;
  every workflow attempt/fence/dispatch/retirement identity uses that ordinal.
  Legacy remains numerically unchanged; do not widen legacy guard permissions.
- Failed retirement increments retry_count exactly once, continuation_count unchanged.
  Successful approval increments continuation_count exactly once, retry_count unchanged.
  RecoveryPolicy receives retry_count+1, never the ordinal. Existing max_retries remains.
- Update retirement/reconciliation witness identities (retired ordinal is current
  attempt_ordinal-1 after retirement), not only live lease checks. Explicit operator
  retry/reconciliation changes neither counter and retains the current next ordinal.
- SQL commit guards enforce these transitions and reject fabricated continuation
  changes, wrong generation, wrong parked attempt, old approval, and stale fences.

## Proposed multiple-approval identity contract

- Business decision remains unique per execution. Action approval unique per immutable
  invocation. Reuse requires exact invocation + argument + operation identity.
- Each approval binds its parked task attempt. An old decision cannot reopen a later
  parked attempt; same-transaction submission evidence remains mandatory.
- Remove the blanket 'any prior decision/dispatch' prohibition. It becomes exact
  invocation collision/dispatch checks; prior completed unrelated effects do not
  prevent approval for the next sequential tool call. Existing unknown effects still
  block progression through marker/receipt recovery policy.
- Preserve one active wait per execution. Non-decision waits retain their existing
  uniqueness; decision waits keep immutable IDs/history and are selected via the
  requested decision's exact wait_id, never merely the latest wait for an execution.
- Replays of earlier decision commands return their saved result without touching a
  later wait/job. Reads/comments/deadline sweeps/cancellation use exact associations.
- Add explicit decision_id provenance to approval audit events with a tenant/run/
  execution composite FK and per-decision event-kind uniqueness. Existing uniqueness
  for other event kinds remains. History is not silently overwritten or deleted.
- Preserve existing operation-digest idempotency: within an execution, identical frozen
  operations resolve to one intent even with different model call IDs. approved_request
  already checks full frozen operation and exact scope/digests through that unique intent.
  No provider port expansion is required. A different target or operation with the same
  arguments has a different identity and requires its own approval.

## Verification before acceptance

Focused existing approval/decision/lease/recovery/reconciliation families plus:
- max_retries=1 still permits successful approval/resume; next real failure spends
  the failure allowance and terminates. Multiple approvals produce distinct monotonic
  attempts, zero failure debit, immutable separate decisions/waits/audits.
- Same arguments for different targets/operations require separate approvals; identical
  frozen operations retain existing single-intent receipt replay. Replay old
  approval while a new wait is active cannot reopen, settle, or authorize it.
- Competing approve/approve, approve/cancel, approve/expiry and stale worker races.
- Direct SQL guard negatives: counter mutation, wrong parked attempt, prior-transaction
  approval, wrong decision, forged audit provenance; correct transaction positive.
- Failure after continuations: Rust/SQL equivalence, exact retirement/reconciliation
  witness identity, stale fence rejection and bounded retry eligibility.
- Existing legacy tests prove zero continuation constraint leaves legacy ownership.
- Fresh migration/schema equality, SQLx prepare/check, fmt/offline all-target/Clippy.
  Shared queue/recovery impact requires broader workflow database suite at this gate.
  Final whole phase06 full suite/stock-stack/lease gate still required.

Open investigation before requesting gate: generated-column fixture compatibility,
precise continuation ceiling semantics for non-agent action steps and audit
immutability guard changes. No architecture PASS claimed.

Investigation updates:
- Native generated ordinal fits existing task row ownership; generic record-copy fixture
  job_auxiliary_fixtures::insert_record must omit generated columns (query pg_attribute,
  as job_source_association_tests already does). Preserve actual constraint assertions.
- Keep63 successful continuations as the protocol ceiling derived from existing64
  harness executions. Actual harness execution budget still counts failures and may
  exhaust sooner. No promise of unbounded approval; run deadline and all other bounds
  remain. Ordinary one-shot action steps receive the same protocol accounting.
- Decision submit currently obtains a latest execution wait before loading decision;
  replace with exact decision.wait_id lookup. Existing read_on already joins exactly.
- Approval audits need explicit decision_id column + composite provenance; guard all
  update/delete against immutable established facts while preserving company cascade.
