# Decision 05-D4 — small delta to D3 after 05-R003

Status: **proposed by A, awaiting B**. Author A (implement-opus-reviewed-2.1), 2026-10-18.

- **What D4 amends.** It amends [D3](decision-D3.md)
  (`bd937b8630e48e6c7e2bf0bfeb223cd879004e4c3cfb898bb302b0d432af0b69`), which in turn sits over D2
  (`83189027…15b3`) and D1 (`4e3a46eb…7931`).
- **What it touches.** Only D3 §3.1's "never hashed" sentence and D3 §3.2 step 6. Everything else
  in D1+D2+D3 stands as 05-R003 accepted it.
- **Basis.** Docs only: no source edits, no cargo. Root chose **option (a)** of 05-F010.
  - Option (b), bounding `users.email`, is a wider product and validation change and stays out of
    scope. A records it for a later phase.

## 0. Disposition (05-R003)

| Finding | Severity | Disposition | Where |
| --- | --- | --- | --- |
| 05-F010: notice key not proven unhashed, so a long member address loops the park | low | **Fixed, option (a).** Each composed notice is checked for the expected unhashed key prefix before any write. If the prefix is missing, the park calls `settle(ReviewerUnavailable)`, so the run fails, the failure is never routed, nothing is parked and nothing is retried. The provenance trigger then can no longer be reached by an honest park. The "bounded address" sentence is corrected. The named test is added | §1, §2 |
| Optional separate code | — | **Not adopted.** A new `decision.notice_unavailable` would need another `PendingFailure` variant, another code and audit kind, and another test, for the same outcome (an unrouted run failure). The existing `decision.reviewer_unavailable` already means "this listed reviewer cannot be assigned and reached", the class D3 §3.2 already uses for composition `BadRequest`. The audit event carries the code. The step's frozen inputs and the member's address are durable, so the cause stays diagnosable | §2 |

## 1. Correction of D3 §3.1 ("the key is never hashed")

The sentence in D3 §3.1 beginning "**The key is never hashed.**" is replaced by:

> **Whether the key is hashed depends on the address length.**
> - The key is `delivery_key`'s `"{purpose}:{source_key}:{destination}"` (`compose.rs:250-270`), that
>   is `notification:workflow-decision:{decision_id}:principal:{principal_id}:email:{address}`:
>   121 bytes plus the lowercased address (`delivery.rs:86-93`).
> - `delivery_key` hashes any key over `MAX_DELIVERY_KEY_BYTES` = 512 (`delivery.rs:35`) to
>   `notification:sha256:…`. That happens for an address over 391 bytes.
> - Member addresses are **not** bounded: `users.email` has only a not-blank CHECK (init
>   5978-5987), `EmailAddress` is an unbounded newtype (`value_objects.rs:136`), and registration
>   only trims.
> - A hashed key therefore can occur, and it carries no provable provenance. The park detects it
>   before any write and fails the run (D4 §2).
> - The BEFORE INSERT provenance trigger stays as the database-boundary backstop. If it ever fires,
>   it is a defect in the park: a database error that rolls the transaction back, fail closed.
> - Bounding addresses (05-F010 option (b)) is out of scope for phase 05.

## 2. Amendment of D3 §3.2 step 6 (compose before any write)

The following is appended to D3 §3.2 step 6, after the composition-error bullet.

**The provable-key check (05-F010 (a)).**

- **When.** After composing each reviewer's notice, and before any write (no wait, decision,
  delivery or reviewer row yet).
- **What.** The park checks that the composed delivery's `idempotency_key` starts with
  `notification:workflow-decision:{decision_id}:principal:{principal_id}:`. This is exactly the
  prefix the provenance trigger requires (D3 §3.1).
- **On failure.** If the prefix is missing because the key was hashed, the reviewer cannot be given
  a provable notice. The park then:
  1. calls `pending_recovery::settle(PendingFailure::ReviewerUnavailable)`, while the job is still
     pending and unleased (`pending_recovery.rs:81-85`);
  2. leaves the run `failed` with `terminal_execution_id` set and the job `failed`;
  3. writes the audit event `decision.reviewer_unavailable`;
  4. never routes the failure and parks nothing.
  Because the outcome is a committed settlement rather than a database error, the job is not
  retried.
- **Where it lives.** The check is a pure string comparison in `DecisionNotices`. It returns the
  same `BadRequest` class as a composition error, so D3 §3.2's single mapping to
  `ReviewerUnavailable` covers both. No new error path is added.
- **Approval parks (05B).** The same composer and check run in the approval park (D2 §16.5). There,
  a failed check fails the step Terminal `<family>.approval_required` with no park, per D2 §16.1's
  "approvers not all eligible".

**Test (05A).** `workflow_decision_park_unprovable_notice_key_fails_run_unrouted`:

- **Setup.** A listed member's `users.email` is set by test SQL to an address of 392 bytes or more,
  which is accepted because only the not-blank CHECK applies.
- **Assertions.**
  - The park returns without parking.
  - The run is `failed`, with `terminal_execution_id` set to the decision execution.
  - The job is `failed`.
  - The audit event is `decision.reviewer_unavailable`.
  - No `workflow_waits`, `workflow_decisions`, `workflow_decision_reviewers` or `message_deliveries`
    row exists for the decision.
  - A second worker poll finds no candidate for the execution, so nothing is retried.
- **05B twin.** `workflow_action_approval_unprovable_notice_key_fails_without_park`.

The D2 §14 acceptance row "Durable precise notices; failure never approves" gains this test.
