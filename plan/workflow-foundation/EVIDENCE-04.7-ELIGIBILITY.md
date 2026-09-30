# 04.7 remaining eligibility — bounded budget counterexample

Implementation evidence only; root owns acceptance. Original Execution item6 in
04-actions-http-and-delivery.md and BRIEF-04.7:194–240,261–326 remain authoritative,
including accepted attribution/witness/observation refinements, AUDIT and BOUNDS.
No04.8, production edit, migration edit, limit increase, or acceptance-record edit.

This assignment was split after the actual next-claim path exposed a production
gap. The counterexample remains failing; neither the assignment nor04.7 is complete.
Frozen evidence: /private/tmp/workflow-04.7-eligibility-01a0f680/HANDOFF.md.
HEAD remains c9cb4b434258b6d33ca26def4ff6b5728dd5a6bc; all inherited WIP preserved.

## Criterion mapping and route

Direct preparation traced settlement.rs:154–202 through its SQL predicate in
20260930180000_workflow_action_evidence.sql:319–346, and next claim through
lease.rs:103–137, lease_claim.rs:5–72,99–120, activation.rs:44–111,
budget.rs:113–143 and pending_recovery.rs:27–96. Existing SIBLINGS tests already
cover running/human/terminal/deadline/attempt-cap gates and preserve existing job,
execution, immutable attempts, frozen invocation and accounting facts.

| Remaining criterion | New discriminatory evidence | Status |
| --- | --- | --- |
| Current root budget eligibility at optional scheduling | `workflow_action_reconciliation_eligibility_root_model_budget_blocks_schedule`: actual unsafe invocation; real Granted2 model-call debit reaches frozen2; actual park and genuine barrier; final evidence records but exact IneligibleJob prevents scheduling; full old job/execution/attempt/debit/usage/intent/marker/entry equality, retained evidence and zero consumptions; real claim returns None with full-table equality | Focused PASS; independent review pending |
| Actual next claim rechecks exhaustion after scheduling | `workflow_action_reconciliation_eligibility_claim_rechecks_shared_root_budget`: actual unsafe invocation, park, genuine barrier and scheduled final proof; actual child admission and claim debit remaining2; explicitly one root/two run links and usage2=limit2; two competing parent claimants must obtain zero continuations | FAIL: one continuation is claimed; promote affected production work to Architecture |
| Child admission; runnable sibling; unactivated/completed/output/route/successor/max_steps; failed-job lease fields; missing suitable retired attempt; poison; other root-budget limits | Not implemented in this bounded assignment | Pending; preserve every original criterion |

The failing test uses accepted private ancestor fixtures and normal admission,
claim, budget, dispatch, park, verifier and reconciliation owners. No disabled
guard, copied proof, forged consumption, altered frozen allowance, or synthetic
usage update creates the counterexample. The isolated migrated database is disposed
normally even on assertion failure. It verifies no new attempt or changed usage if
the claim is refused; existing accounting facts must remain present, while a future
owner may append a refusal fact under the reviewed contract.

## Required focused contract review

The original criterion requires the next claim to recheck current exhaustion. The
actual frozen activation branch skips budget activation/reservation and the claim
scope/install paths have no current-budget check. Independent expansion must settle
the narrow predicate, shared-root usage-lock ordering, claim/dispatch race boundary,
and existing-owner refusal/retirement behavior without taking root/parent run locks
or replenishing allowances. Preserve accepted retry, receipt-only and supported
replay semantics; test real competing debit/claim owners. No production fix was
attempted before that contract review.

## Verification and remaining work

Complete logs and final exact results are retained in the frozen handoff. Focused
stock2MiB suite is intentionally1PASS/1FAIL/0ignored; the failure is a production
counterexample, not an accepted red gate. Earlier compile failure used an incorrect
singular trait import and was corrected. Existing stock2MiB sibling regressions
5PASS/0FAIL/0ignored,6.18s. Final code strengthens shared-root identity/count
assertions and permits append-only refusal accounting facts; rechecked below in the
frozen focused log. Full fmt/whitespace/offline-all-target/SQLx results are recorded
only when completed in the handoff. No expensive unchanged full-action repeat.

Retained PostgreSQL system7691265791172745923/PG18 was verified cleanSTOPPED before
startup; exact data/private/tmp/workflow-admission-pg-e3aa, databaseworkflow_admission,
usermac03, port55439, socket/private/tmp/max_connections200 used with both explicit
URLs. All50 migrations immutable. No retained reset/drop/stage/commit/deploy.

All remaining eligibility, high-entry exclusions/deferred/current-xid/upgrades,
RESUME groups2–4, strictClippy/runtime/final gates, fresh independent FULL04.7
original-criteria/actual-code/integration PASS and root acceptance remain pending.
