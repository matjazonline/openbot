# 04.11-D4 — Delta over D2 + D3: conflict veto on effect resume, evidence shape, phase-06 seam

No backward compatibility: reset databases for incompatible schemas/payloads; no
backfills, dual paths, legacy decoders or staged rollout.

Status: proposed architecture gate, a correction after 04.11-R004 (CHANGES REQUIRED).
Not approved.

The effective decision is:

- D2 (`decision-D2.md`, SHA256 `1dcd1a9e99e57cd32e15bf095490961722f185153c66945cd82d29bd4d943a44`),
- plus D3 (`decision-D3.md`, SHA256 `c462e4eb56cb76ac816fa107b7b01f7651706796b8723fed8f206c6315858a89`),
- with the sections below amended.

Every section not named here applies unchanged. D1–D3 are retained unchanged.

| D4 section | Amends |
| --- | --- |
| §0 | replaces D3 §0 in full |
| §M | D2 §4.4 (effect-wait discovery and the `resume_on` effect branch). Adds a wait failure code, a `WaitFailureReason` and a cancellation reason. (F012) |
| §N | D3 §C "Evidence subject": `applied_request` for message evidence (F013.1) |
| §O | D3 §C "One-use reopen consumption" guard (F013.2) |
| §P | replaces D2 §5.2's `workflow_action_effect_states` bullet; confirms the value in D3 §G row 16 (F013.3) |
| §Q | replaces the conflict-fact parenthetical of D3 §G row 20 and the "Finality breach" bullet of D3 §H.4 (F013.4) |
| §R | D3 §H.4 (outcome naming, no `continue_on`), D3 §L and D2 §7 "Docs" bullet (BRIEF-04.11 content): the R004 §L conditions |
| §S | D3 §K (tests for F012, F013 and the §L conditions) |

## 0. Finding dispositions (replaces D3 §0)

| Finding | Severity | Disposition | Resolved in |
| --- | --- | --- | --- |
| F001–F008 | — | Resolved at decision level (R003). Unchanged. | D2; D3 |
| F009 (optional) | low | Resolved (R004). Unchanged. | D3 §A, §B |
| F010 | medium | Resolved at decision level (R004). Its integration gaps are closed by F012 and F013 below. | D3 §C–§K; D4 §M–§Q |
| F011 | low | Resolved (R004). Unchanged. | D3 §E, §J, §K |
| F012 | low | Adopted. The `resume_on` effect branch checks `workflow_action_has_evidence_conflict` under the run lock before any advance, whether the receipt is accepted or failed. On a conflict it does not advance. It applies the same Reconcile outcome that `complete_io` produces through `retire_on` for Terminal `action.evidence_conflict` / EffectOutcomeUnknown. Receipt truth stays recorded. A conflict also wakes the effect wait. Tests added. | §M, §S |
| F013 | low | Adopted. All four items: (1) `applied_request='delivery'` for message applied evidence; (2) the reopen guard checks for an evidence conflict in SQL; (3) a message-specific effect-state projection derived from action state, ahead of the generic evidence branches; (4) the existing conflict orientation (`evidence_id` = breached proof). SQL and projection tests added. | §N–§Q, §S |
| R004 §L conditions | — | Adopted. BRIEF-04.11 records the phase-06 obligation and its trigger. Message outcomes never enter the shared `continue_on`; the reopen outcome is renamed `DeliveryReopened`. A port-seam test is added. The divergence from 04.7 is recorded. | §R, §S |

## M. Finality-breach veto on effect-wait resume (amends D2 §4.4; F012)

The accepted 04.7 invariant: a finality breach permanently vetoes automatic
continuation, even when a usable receipt exists (`20260930180000:240-241`). It is
enforced on the shared completion path at `completion.rs:106-125`. Effect-wait resume
now enforces it too.

- **Discovery.** The `CANDIDATES_SQL` effect branch becomes
  `wait.reason='effect' AND (EXISTS(receipt for the action whose wait_id = wait.id) OR workflow_action_has_evidence_conflict(wait.company_id, wait.execution_id))`.
  A conflict therefore wakes the wait even before every destination is terminal. A
  conflict is an immutable fact, so the wait is not re-polled hot once it has been
  handled. The wait leaves `waiting` (below), and discovery requires a waiting row.
- **`resume_on` effect branch, first step.** Under the run lock that `waits::lock`
  already takes, and before reading the receipt, evaluate
  `workflow_action_has_evidence_conflict(company, execution)` through the same
  `action_uncertainty::conflicted_on` that `complete_io` uses.
  - Evidence settlement inserts conflicts under run `FOR UPDATE` (D3 §D), so a
    conflict is either visible to this check or committed after the resume
    transaction.
  - A conflict committed after the resume transaction is historical truth about an
    already-completed execution, as in 04.7.
- **On conflict**, whatever the receipt state (none, accepted or failed), the new
  writer `park_conflicted_effect_on` applies, in one transaction, the outcome that
  `retire_on` gives `complete_io`'s conflict failure (Terminal `action.evidence_conflict`,
  EffectOutcomeUnknown → `RecoveryDecision::Reconcile`):
  1. the wait → `failed` with `failure_code='evidence_conflict'`. This value is added
     to the `workflow_wait_failure` CHECK, and `WaitFailureReason::EvidenceConflict`
     is added.
  2. the `EffectParked`-closed job → `failed`, the same terminal job state that
     `finish_job` writes for a Reconcile decision.
  3. the run → `waiting/reconciliation` through the existing
     `recovery::settle_run(…, Reconcile)` writer.
  4. `action_uncertainty::record(company, run, Some(execution), action.evidence_conflict)`.
  5. unsent sibling deliveries of the conflicted invocation (`pending`/`retryable`)
     are superseded through the workflow cancellation helper (D2 §6), with a new
     cancellation reason `evidence_conflict`. In-flight deliveries keep their intent
     and late truth is still recorded. After the veto, no new provider entry starts
     for the conflicted invocation.
  6. run audit event `action_evidence_conflict`.

  There is no `final_error` route, no successor and no step output. The receipt, if
  any, stays recorded truth.

  `workflow_action_reconciliation_reopen_safe` excludes conflicted executions, so the
  04.7 `continue_on` can never reopen this park. It lasts until cancel or the run
  deadline (`expire_on`). This matches the remote behavior.
- **No conflict** → accepted or failed receipt, handled as in D2 §4.4.

## N. Applied attribution for message evidence (amends D3 §C; F013.1)

- `workflow_action_evidence.applied_request` CHECK gains `'delivery'`
  (`20260930181000:3-4`). `workflow_action_applied_request_shape` gains:
  - `(applied_request='delivery' AND disposition='applied' AND applied_remote_entry_id IS NULL AND delivery_id IS NOT NULL)`.
  - Every existing remote alternative additionally requires `delivery_id IS NULL`.
- The extended evidence commit guard requires:
  - message applied evidence ⇔ `applied_request='delivery'`;
  - remote evidence never `'delivery'`.

  `workflow_action_applied_request_guard` (non-NULL for applied) is unchanged.
- The remote `unattributed` handling is untouched and cannot see message evidence:
  - `workflow_action_receipt_finality_conflict` (`20260930181000:83-108`) fires only
    for receipts with `remote_entry_id` or `reconciliation_evidence_id`;
  - message receipts carry neither (D2 §2.4, D3 §H.4);
  - message conflicts are written only by settlement (§Q).

## O. Reopen guard conflict parity (amends D3 §C; F013.2)

The `workflow_message_delivery_reopens` BEFORE INSERT guard additionally requires
`NOT workflow_action_has_evidence_conflict(NEW.company_id, <invocation's execution>)`.
It is evaluated after the guard's run lock. This is the SQL equivalent of the Rust
check in D3 §H.4, mirroring how `workflow_action_not_applied_available`
(`20261001090000:9`) guards remote consumption.

## P. Effect-state projection for message markers (replaces the D2 §5.2 effect-state bullet; F013.3)

`workflow_action_effect_states` gains a message branch **immediately after the
receipt branch, ahead of every generic evidence/reconciliation branch**:

```text
WHEN receipt.invocation_id IS NOT NULL THEN 'committed'
WHEN marker.effect_kind='message' THEN
     CASE WHEN message_action.state='pending' THEN 'possible_dispatch'
          ELSE 'needs_reconciliation' END   -- 'uncertain' | 'stalled'
```

- `accepted` and `failed` always have a receipt, so they hit the first branch.
- The generic branches (applied, conflict, not-applied, reconciliation-row, evidence
  rows) never apply to message markers. A row-16 reopen therefore projects
  `possible_dispatch`, as D3 §G row 16 states. That value is now correct and
  unchanged.
- Applied evidence on one delivery while a sibling is still pending projects
  `possible_dispatch`, not `applied_without_result`.
- The `evidence_conflict` column is unchanged and still shows a breach separately.
- The vocabulary is unchanged. The only non-test reader (`mcp.rs:342`) is unaffected.

## Q. Conflict orientation (amends D3 §G row 20 and §H.4; F013.4)

A message finality breach inserts `workflow_action_evidence_conflicts` with the
existing orientation (`20260930181000:90-93`):

- `evidence_id` = the breached, consumed `final_not_applied` proof;
- `contradictory_evidence_id` = the new applied evidence;
- `remote_entry_id` NULL;
- `reason='finality_breach'`.

The existing CHECK (contradictory or entry present) is satisfied. Row 20's text
"evidence = applied, contradictory = the consumed proof" is replaced by this
orientation. The rest of row 20 stands, now also governed by §M.

## R. Phase-06 seam conditions (amends D3 §H.4, D3 §L, D2 §7 "Docs")

1. **BRIEF-04.11 records the obligation and its trigger.** The brief, created in
   implementation and listed in `NO_BACKWARD_COMPATIBILITY_NOTES`, must state:
   - Phase 06 obligation: a `model_tool`-origin agent execution parked for
     reconciliation is reopened by the phase-06 saved-call owner once **every message
     marker of that execution has a receipt and
     `workflow_action_reconciliation_reopen_safe` holds**. Until then it stays parked.
     - It is unreachable in phase 04, because publication refuses agent snapshots
       (`authority.rs:53-60`).
     - Trigger: the first phase-06 change that exposes direct message tools in an
       agent host. That change must implement the reopen and the saved-call resume
       before shipping.
   - The divergence (below).
2. **Message outcomes stay out of the shared `continue_on`**
   (`action_reconciliation/settlement.rs:154-202`).
   - Settlement for a `MessageDelivery` target returns before the 04.7 continuation
     step for every outcome.
   - D3's reopen outcome `Scheduled { receipt_only: false }` is replaced by a new
     `ReconciliationOutcome::DeliveryReopened` (command-receipt outcome kind
     `delivery_reopened`, `scheduled_job_id` NULL). No message outcome can match the
     job-scheduling branch or the receipt CHECK's `scheduled` case.
   - `AppliedRecorded`, `NotAppliedRecorded`, `UnknownRecorded`, `Blocked` and
     `EvidenceConflict` for message targets likewise never call `continue_on`.
3. **Port-seam test** (§S).

**Divergence recorded.** The 04.7 `continue_on` reopens a reconciliation-parked owner
for remote invocations (`settlement.rs:154-202`, with
`workflow_action_reconciliation_reopen_safe` at `20261001100000:281-306`). Message
settlement deliberately never does, for two reasons:

- step-origin progression already flows through the effect wait (D2 §4.4, D4 §M);
- reopening a `model_tool` agent job before phase 06 would resume an execution whose
  saved-call replay does not exist.

## S. Verification additions (extends D3 §K)

Filters: area C (`workflow_message_wait_`) and area H (`workflow_message_reconcile_`).
Deliberate-delay variants use the `workflow_message_race_` prefix with
`recorder::Injected::mark()` (D3 §K).

- **F012**, each test asserting execution, job, wait, run, receipt, uncertainty and
  delivery rows:
  - a breach (row 20), then every destination accepted, then the consumer writes the
    `accepted` receipt and the poll resumes. Expect:
    - no successor or step output; the execution is not completed;
    - wait `failed/evidence_conflict`, job `failed`, run `waiting/reconciliation`;
    - an uncertainty row with `action.evidence_conflict`;
    - the receipt retained.
  - the same with a `failed` receipt;
  - a conflict while a sibling is still pending: the wait wakes without a receipt, the
    step is parked as above, the pending sibling is superseded with
    `evidence_conflict` and no provider call is made, and an in-flight sibling's late
    acceptance is recorded;
  - after the park, two poll scans yield no candidate;
  - a later 04.7 command cannot reopen the park (`reopen_safe` is false).
- **F013:**
  - SQL-only checks on `applied_request`:
    - message applied evidence without `'delivery'` is rejected;
    - remote evidence with `'delivery'` is rejected;
    - a `'delivery'` value without a subject is rejected;
  - a reopen insert raises when a conflict exists for the execution (SQL-only, with
    the other guard conditions satisfied);
  - `effect_state` asserted per outcome row:
    - pending → `possible_dispatch`;
    - after a row-16 reopen → `possible_dispatch`, despite a reconciliation row and
      final-not-applied evidence;
    - applied evidence with a pending sibling → `possible_dispatch`;
    - uncertain or stalled → `needs_reconciliation`;
    - receipted → `committed`, with `evidence_conflict` true after a breach;
  - conflict-row orientation and `reason='finality_breach'` asserted.
- **§L port-seam test** (direct-tool port; `agent.run` is unreachable in phase 04). An
  agent-kind execution holds a `model_tool` message invocation with a pending email
  delivery and is interrupted, so it parks Reconcile (`waiting/reconciliation`, job
  `failed`).
  - Before any receipt, `workflow_action_retry_safe` is FALSE.
  - The delivery is accepted and the consumer records the receipt. Afterwards:
    - the run stays `waiting/reconciliation` and the job stays `failed`;
    - there is no reopen and no successor;
    - `retry_safe` is TRUE now that every marker has a receipt.
  - A `MessageDelivery` reconciliation (`DeliveryReopened`) for an unknown delivery of
    such an execution reopens only the delivery, never the job.
