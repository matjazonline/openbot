# Decision 05-D3 — narrow delta to D2 after 05-R002

Status: **proposed by A, awaiting B**. Author A (implement-opus-reviewed-2.1), 2026-10-18.

- **What D3 amends.** [D2](decision-D2.md) (`831890270a87273b2cbbccf580a942c4582841cc2c4430b15d542d95754815b3`),
  which itself amends [D1](decision-D1.md) (`4e3a46eb…7931`). D3 replaces exactly the passages
  named below. Everything else in D1+D2 stands as 05-R002 accepted it.
- **Base.** HEAD `feefe3e` plus the accepted batch S tree. D3 is docs only: no source edits, no cargo.
  The code was checked read-only.

## 0. Dispositions (05-R002)

| Item | Severity | Disposition | Where |
| --- | --- | --- | --- |
| 05-F008: the approval reopen reuses the parked attempt number and is unguarded | medium | **Fixed.** The parked attempt is consumed **at reopen**: the reopen sets `retry_count = retry_count + 1`, and the park close leaves the count unchanged. The park precondition becomes `retry_count + 1 < max_retries`. A deferred guard branch refuses every workflow `completed` → `pending` transition except an approval reopen that meets all of: the parked attempt, the accounting, the run `running`, and `approve` submitted in the same transaction. Three tests added | §2 |
| 05-F009: `notice_delivery_id` | low | **Fixed.** `UNIQUE (notice_delivery_id)`. The delivery is inserted first, then the reviewer row with its id: `NOT NULL`, `ON DELETE NO ACTION`, the whole row immutable. A provenance trigger binds the delivery's purpose, shape and idempotency key to the tenant-scoped decision and principal. A transaction-scoped DB test rejects references to another decision's or another company's delivery | §3 |
| Observation: a deterministic composer error at park retries until the deadline | low risk | **Classified terminal.** Composition runs before any write. A deterministic composition error (`BadRequest`) settles `decision.reviewer_unavailable`: the run fails, the error is never routed, and nothing is parked. Infrastructure errors still propagate and retry | §3.2 |
| Observation (05B): up to 16 deliveries in the dispatch transaction | — | **Included.** The approval park with 16 approvers is part of the 05B lease-budget coverage and has a WriteBound test | §4 |
| Self-approval by the run actor | user decision | **Allowed** (user, 2026-10-18): any listed owner or admin may approve, including the run actor. D2 §16.1's rules are unchanged, and one test is added | §1 |

## 1. Amendment to D2 §16.1 (who may approve)

Append to D2 §16.1:

> **Self-approval is allowed** (user decision, 2026-10-18, given directly to root). Any owner or
> admin listed in `approval.reviewers` may approve, including the run's actor
> (`workflow_runs.actor_id`).
> - **Change:** none to D2 §16.1's rules. Park-time eligibility and the submit recheck (D2 §9.1)
>   apply to the run actor exactly as to any other listed approver: they must be listed, owner or
>   admin, and able to see the run's association, and they are rechecked under the D2 §8 locks.
>   No separation-of-duties check exists, and none is added.
> - **Test (05B):** `workflow_action_approval_run_actor_may_approve`. An admin who started the run
>   and is listed as the only approver approves. The job reopens, and the action dispatches once.
>   If the same admin is demoted to member before submitting, the submit is refused with
>   `NotFound` and the approval stays open.

## 2. Replacement of D2 §16.2 (05-F008)

D2 §16.2 is replaced in full by the following.

### 16.2 Parking, reopening, and attempt accounting

**What the code does today** (re-checked):

- A claim takes attempt `retry_count + 1` and inserts it (`lease_claim.rs:64-69, 171`). It requires
  `retry_count < max_retries` (`lease_claim.rs:43`).
- `task_attempts_task_attempt_key UNIQUE (task_id, attempt_number)` (init 8209) makes an attempt
  number single-use.
- A fenced completion leaves `retry_count` unchanged. The precedent is the effect park,
  `message_publish.rs:262-266` (`retry_count+1=$6`, no increment).
- A retirement increments it: `check_workflow_fenced_retirement` requires
  `NEW.retry_count = OLD.retry_count + 1`.
- No trigger guards a workflow `completed` → `pending` transition, and no production path performs
  one today. The only workflow reopen is `control_retry.rs:37`, which goes `failed` → `pending`
  after a retirement and is guarded by `check_workflow_control_retry`.

**Accounting choice: consume the parked attempt at reopen.**

- **Park close.** The attempt is closed `completed` / `approval_parked`, and the job goes
  `processing` → `completed` with `retry_count` **unchanged**. This is the same shape as the
  accepted effect park.
  - Write `k` for `retry_count` while the parked attempt `k+1` is open. Every completion-side reader
    keeps assuming that the closed attempt is `retry_count + 1`:
    - `check_workflow_fenced_completion` (init 215-243);
    - the supervisor's suspension confirmation (the approval counterpart of `confirm_suspension`,
      `message_receipts.rs:357-384`);
    - `route_parked_failure`'s job close.
- **Reopen** (approve only). The job goes `completed` → `pending` with
  `retry_count = k + 1`, `run_at = clock_timestamp()`, and lease fields NULL.
  - The next claim therefore takes attempt `k + 2`, a fresh number.
  - Reject and expiry never reopen. They close the job `completed` → `failed` through
    `route_parked_failure`, as today's effect failure does, so the parked attempt needs no further
    accounting.
- **Why not at park close.** Incrementing in the fenced `processing` → `completed` close would change
  a transition that already has three accepted readers. The reopen is the one new, guarded
  transition, so the accounting belongs there.

**Precondition at park:** `retry_count + 1 < max_retries`, i.e. `k + 1 < max_retries`.

- The reopened job then has `k + 1 < max_retries`, so it is claimable (`lease_claim.rs:43`).
- The `exhausted` predicate (`lease.rs:409-411`, `retry_count >= max_retries` on a pending job)
  cannot fire on it.
- When the parked attempt would be the last one (`k + 1 = max_retries`), the call does **not** park.
  It fails Terminal `<family>.approval_required` through the action failure writer, as today.

**The settlement transaction for `approve`.** This is one transaction under the D2 §8 lock order
(authority → run → association → execution → job → wait → decision).

1. Record the submission, including the new column `workflow_decisions.submitted_xact xid8`, set to
   `pg_current_xact_id()`.
   - It joins D2's submission all-or-none CHECK; the
     `workflow_action_*_witnesses.transaction_id` columns are the precedent.
2. Complete the wait.
3. Set the run to `running`, `waiting_reason` NULL.
4. Reopen the job.
5. Audit `action_approved`.

**The deferred guard.** A new branch is added to `check_workflow_fenced_completion` (init 215-243;
the deferred constraint trigger `workflow_fenced_completion_guard`, init 11192). For
`OLD.queue_kind = 'workflow' AND OLD.status = 'completed' AND NEW.status = 'pending'`, it raises
`'workflow job reopen requires an approval in this transaction'` (`23514`) **unless all** of the
following hold:

| Condition | Requirement |
| --- | --- |
| Accounting | `NEW.retry_count = OLD.retry_count + 1` |
| Retry bound | `NEW.max_retries = OLD.max_retries` and `NEW.retry_count < NEW.max_retries` |
| Identity | `NEW.workflow_execution_id = OLD.workflow_execution_id` and `NEW.company_id = OLD.company_id` |
| Lease fields | `NEW.worker_id`, `NEW.execution_generation` and `NEW.lock_expires_at` are NULL |
| Parked attempt | Attempt `(task_id = OLD.id, attempt_number = OLD.retry_count + 1)` has `status = 'completed'` and `stop_reason = 'approval_parked'` |
| Execution | Still open: `completed_at IS NULL` and `committed_route IS NULL` |
| Run | `state = 'running'` and `deadline > clock_timestamp()` |
| Approval | The execution's `workflow_decisions` row has `subject_kind = 'action_approval'`, `choice = 'approve'` and `submitted_xact = pg_current_xact_id()` |
| Wait | Its wait has `reason = 'decision'` and `state = 'completed'` |

- Every other workflow `completed` → `pending` is refused.
- The approval decision is `UNIQUE` per execution, and after a reopen the attempt
  `retry_count + 1` is no longer `approval_parked`. So one approval can reopen the job at most once.
- **Processing → completed** keeps the approval-park branch D2 already added: the attempt is
  `approval_parked`, the run is `waiting/decision`, and a waiting `decision` wait exists with an
  `action_approval` decision.

**Unchanged from D2 §16.2:**

- `workflow_action_capture_schedule` (failed → pending only) and
  `workflow_action_confirm_claim_retirement` (pending → failed only) neither fire nor block.
- With no witness, `workflow_action_pending_claim_episode` is NULL, so the budget gate passes
  (`lease_claim.rs:180-215`).
- On re-claim, `pending_recovery::activate` returns the existing activation, and
  `actions.rs::prepare` returns the existing intent `replayed`.
- `task_attempts.stop_reason` gains `approval_parked`.

**Tests (05B):**

- `workflow_action_approval_dispatch_claims_next_attempt_and_dispatches_once`: park at attempt N, then
  approve; the job is `pending` with `retry_count = N`; the claim inserts attempt N+1; exactly one
  `workflow_action_dispatches` row exists; replay is refused.
- `workflow_action_approval_reopen_without_approval_is_refused`: a direct
  `completed` → `pending` on the parked job, with no approve in the transaction, fails at commit with
  23514. The same refusal applies with an approve committed in an earlier transaction, a
  `reject` choice, or a wrong `retry_count`.
- `workflow_action_approval_last_attempt_fails_without_park`: with `retry_count + 1 = max_retries`
  the call fails Terminal `<family>.approval_required`, and no wait, decision or delivery is written.

## 3. Amendments to D2 §5.1, §7.1 and §10 (05-F009 and the composer observation)

### 3.1 Replacement of the D2 §5.1 `workflow_decision_reviewers` block

```text
company_id, decision_id, ordinal smallint (0..15), principal_id uuid, notice_delivery_id uuid NOT NULL
PK (company_id, decision_id, principal_id); UNIQUE (company_id, decision_id, ordinal)
UNIQUE (notice_delivery_id)
FK (company_id, decision_id) → workflow_decisions ON DELETE CASCADE
FK (company_id, principal_id) → principals(company_id, id)
FK (notice_delivery_id) → message_deliveries(id)      -- ON DELETE NO ACTION (default)
```

- **Insert-only.** Every column, `notice_delivery_id` included, is immutable: a BEFORE UPDATE trigger
  raises. D2's sentence "Immutability applies to every other column" and its `ON DELETE SET NULL`
  are withdrawn.
  - `NOT NULL` is always satisfiable: every reviewer is a member, and `users.email` is `NOT NULL`
    (init 5978-5981).
  - `NO ACTION` holds because production never deletes a delivery, which B verified. A test-only
    delivery delete must first delete the run, which cascades through the decision to the reviewer
    row.
- **The FK is id-only.** That is the only form expressible for a tenant-less standalone delivery
  (init 5036), as for `notifications.email_delivery_id`. Tenant provenance is proven by the trigger
  below.
- **Provenance trigger** `workflow_decision_reviewer_notice_provenance`, BEFORE INSERT. It raises
  `23514` unless the referenced `message_deliveries` row has all of:
  - `purpose = 'notification'`;
  - the standalone shape: `company_id`, `channel_id`, `message_id`, `source_binding_id` and
    `destination_binding_id` all NULL, and `external_destination` NOT NULL;
  - `starts_with(idempotency_key, 'notification:workflow-decision:' || NEW.decision_id || ':principal:' || NEW.principal_id || ':')`.

  **Why this proves the tenant.** The decision id is a tenant-scoped key: the reviewer row's FK
  `(company_id, decision_id)` proves the company. So a delivery minted for another decision, or for
  a decision in another company, can never be linked.

  **The key is never hashed.** The key shape is `delivery_key`'s `"{purpose}:{source_key}:{destination}"`
  (`compose.rs:246-270`). The source key is `workflow-decision:{decision_id}:principal:{principal_id}`
  and the destination is the member's bounded address, so the key stays under the bound and is never
  hashed. Were it ever hashed (`notification:sha256:…`), the trigger refuses the insert and the park
  rolls back: it fails closed.
- **DB test** (transaction-scoped; the persistence AGENTS composite-FK obligation, adapted):
  `workflow_decision_reviewer_notice_provenance_rejects_foreign_delivery`. Within one rolled-back
  transaction, each of these is refused by the trigger:
  - a reviewer row that references a valid standalone notification delivery whose key names
    *another* decision of the same company;
  - one whose key names a decision of *another* company;
  - one whose key names a different principal of the same decision;
  - a non-notification delivery.

  A second insert of the same `notice_delivery_id` is refused by the UNIQUE key, and an UPDATE of
  `notice_delivery_id` is refused by the immutability trigger.

### 3.2 Replacement of the D2 §7.1 park steps 5–6, and the matching D2 §10 "Mechanism" bullets

The new order of the park transaction, after D2 §7.1 steps 1–4:

5. **Resolve** the reviewers, with no locks (D2 §9.3), and read each member's `users.email`. If any
   listed reviewer is unresolvable or ineligible, `settle(ReviewerUnavailable)` and stop (D2 §7.1).
6. **Compose** every notice, before any write, using `DecisionNotices` (pure, D2 §10).
   - **A deterministic composition error is terminal.** An `AppError::BadRequest` from
     `CanonicalContent::parse` or from the part bounds means the reviewer cannot be notified, which
     is the same configuration class as an unreachable reviewer. It becomes
     `pending_recovery::settle(PendingFailure::ReviewerUnavailable)`: the run fails, the error is
     never routed, and nothing is parked.
   - This settles while the job is still pending and unleased, as `settle` requires
     (`pending_recovery.rs:81-85`).
   - Any other error (internal, database) propagates and the park retries, as today. This removes
     the "retries until the run deadline" path the observation described.
   - Test: `workflow_decision_park_composition_error_fails_run_unrouted`. It uses a test-only
     `DecisionNotices` whose content fails `CanonicalContent::parse`. The run ends `failed`, no wait
     is written, and the audit event is `decision.reviewer_unavailable`.
7. **Insert** the wait, then the decision.
8. **Per reviewer, in ordinal order:**
   1. `insert_standalone_delivery_on(tx, &notice)`. The result must be `Created`; `Absorbed` cannot
      occur for a fresh decision id, and is an internal error that rolls the transaction back.
   2. Insert the reviewer row with `notice_delivery_id` set to that id.
9. `completion_job::finish(Pure)`, set the run to `waiting/decision`, and audit `decision_opened`
   (D2 §7.1 step 7, unchanged).

The D2 §10 bullet "store the resulting `Created`/`Absorbed` id in
`workflow_decision_reviewers.notice_delivery_id`" now reads: "insert the reviewer row carrying the
`Created` delivery id (§3.1); the row is never updated". The rest of D2 §10 is unchanged.

## 4. 05B lease-budget coverage (observation)

The approval park writes up to 16 standalone deliveries and 16 reviewer rows inside the fenced
dispatch transaction, under `WriteBound` (D2 §16.1–16.3, through §10's enqueue).

- **05B gate.** The 05B batch gate runs `scripts/lease-budget.sh` with that path included.
- **New test:** `workflow_action_approval_park_with_sixteen_approvers_commits_within_bound`. It parks
  a protected call with 16 owner/admin approvers and asserts that the commit lands inside the attempt
  bound, with 16 deliveries and 16 reviewer rows.
- **Final gate.** The phase 05 final gate's `lease-budget` step covers it.
- **05A.** The 16-reviewer business park has a twin test,
  `workflow_decision_park_with_sixteen_reviewers`. It runs outside a lease, but the lock timeouts are
  the same.

## 5. Verification additions

These add to D2 §14.

| Batch | Additions |
| --- | --- |
| 05A focused | `workflow_decision_reviewer_notice_provenance_rejects_foreign_delivery`, `workflow_decision_park_composition_error_fails_run_unrouted`, `workflow_decision_park_with_sixteen_reviewers` (under the `workflow_decision_` filter) |
| 05B focused | The three §2 tests and the §4 test (under `workflow_action_approval_`); `completion_` and `lease_` for the guard branch; `scripts/lease-budget.sh` |
| Acceptance mapping | "No approved action dispatches with different arguments" gains `…dispatch_claims_next_attempt_and_dispatches_once`; "durable precise notices" gains the provenance test |

D1+D2 is unchanged except for the passages replaced above.
