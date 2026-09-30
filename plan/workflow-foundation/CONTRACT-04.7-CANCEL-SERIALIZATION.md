# 04.7 — observing cancellation/reconciliation serialization

Status: bounded Architecture proposal, frozen for independent Sol check. Root alone
accepts. This resolves only row2c's cancellation lock observation; no production,
migration, authorization, state-machine, or resource-limit change is proposed.

## Authority and interpretation

Original authority remains Execution6 (`04-actions-http-and-delivery.md:36–38`)
and `BRIEF-04.7.md:35–45,137–152,192–239,273–277`. They require company/principal
authorization before run locking, expiry/cancel/evidence/late truth serialization
through the owning run, accepted receipt truth, and no terminal advancement.
The residual inventory's row2c requires genuine competing owners in both orders
and observed run-lock competition. Neither original requires the *second business
operation* to reach the run-lock statement before the first commits.

The attempted fixture imposed that additional observation on both cancel orders.
It is unreachable with the authorized owners: both first acquire conflicting
`FOR NO KEY UPDATE` locks on the same company and retain them until commit. Thus
the second waits at company authorization, before its run statement. Changing
actor does not change the company row; changing company loses the authentic scope.
A longer observation timeout cannot make this constructor reachable.

Keep the original invariant: each actual owner holds the exact owning run before
its execution/job decisions and mutations. For cancellation pairs, observe both
the actual company contention and independent contention on that exact run while
the first owner is stopped before execution acquisition. Do not accept company
contention alone as proof of owning-run serialization. Expiry retains its direct
second-owner run wait. This is a correction to the derived observation, not a
waiver of either run ownership or a genuine competing-claimant test.

## Actual ownership inventory

Paths below are relative to `src/adapters/persistence/workflow/` unless stated.

| Owner / seam | Lock and transaction evidence |
|---|---|
| Shared authority | `authority.rs:9–36`: company `FOR NO KEY UPDATE`, then member/principal `FOR SHARE`; transaction remains caller-owned. |
| Actual cancellation | `controls.rs:29–41,64–129`: `cancel` calls `control`, which authorizes company, loads head `FOR UPDATE OF run`, authorizes association, checks replay/revision, then cancels and commits. `cancel_on:163–183` locks executions, jobs, retires work and updates run. |
| Actual reconciliation | `action_reconciliation/mod.rs:44–94,148–209`: association and snapshot transactions authorize company then lock run; restore locks execution then job. `action_reconciliation/settlement.rs:5–79` repeats company → run → association → execution/job before facts/continuation/audit and commits. `head_on` callers are association, snapshot, settlement. The paused verifier fixture has already released association/snapshot transactions; the tested first/second transaction is settlement. |
| Actual expiry | `polling.rs:31–40` owns the expiry transaction. `maintenance.rs:5–43` locks run, checks DB deadline/association, then locks executions/jobs and retires; no company authorization precedes this runtime owner. `expire_on` also serves wait resume; no new expiry owner is needed. |
| SQL revision ownership | `migrations/20260928200000_workflow_controls.sql:5–44` generates run revisions and updates owning run for execution/job/wait changes. `20260930180000_workflow_action_evidence.sql:124–127` attaches the existing owner revision trigger to evidence/conflicts. These later writes are not a substitute for locking run **before** the execution gate. |
| Additional SQL run locks | Evidence migration `:207–210,256–277` contains evidence commit, conflict parking and remote-entry guard run locks. They neither release the authority lock nor offer an authentic alternate cancellation entry point. No guard or applied migration is changed. |

## Concrete constructor and discriminator

Retain `action_reconciliation_control_serialization_tests.rs` and its four named
tests, each with Applied / FinalNotApplied / Unknown: twelve variants total.
Retain genuine provider histories and the application reconciliation service,
actual `cancel`, and actual `expire_run`. Only test observation helpers change.

1. Finish the real snapshot/provider attestation and pause before settlement, as
   the existing constructor does. Own an isolated fixture with no other run writer.
   Hold **only the exact execution row** in the gate transaction. Record its PID.
   A separate short transaction probes the exact company/run with
   `SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE NOWAIT`;
   assert one matching row and roll back. This positive proves the execution gate
   itself does not lock the run. Complete it before starting either contender.
2. Start the designated first real owner. Observe its active execution-lock query
   with `wait_event_type='Lock'` and the gate PID in `pg_blocking_pids(first_pid)`.
   Keep that future alive and fail if it finishes early. No facts/state mutations
   or revision-trigger run locks have happened past this gate.
3. In a distinct observer transaction repeat the exact run `FOR UPDATE NOWAIT`.
   Require PostgreSQL SQLSTATE `55P03`, not a generic timeout/error/zero rows;
   roll back the failed probe transaction immediately. Given the positive probe,
   execution-only gate, isolated fixture and only first owner started, the first
   owner is the run blocker. This detects removal or postponement of its run lock
   even though company locking would still serialize the two business calls.
   The probe is observation only: it never supplies an actor, proof, grant, or
   workflow mutation and is never substituted for either actual contender.
4. Start the second genuine owner while the first remains at the execution gate.
   For **both cancel orders**, observe the exact company-authority query
   (`SELECT user_id FROM companies WHERE id = $1 FOR NO KEY UPDATE`) and require
   first PID in `pg_blocking_pids(second_pid)`, distinct PIDs and Lock wait. For
   **both expiry orders**, keep the existing direct run-query/blocker observation.
   Continue polling the first future so early completion fails. No sleep alone,
   pending-future inference or uncorrelated lock-table row counts as evidence.
5. Release the execution gate, await both real calls within existing production
   deadlines, assert both transactions drained, and repeat the successful exact
   run NOWAIT probe with rollback. Retain bounded observation and cleanup on
   failure; do not raise production timeout/stack limits to accommodate the test.

Both orders independently put cancellation and settlement first, so step3 proves
run ownership for each authentic owner. A second business operation need not also
wait directly on run in the same variant. If a fixture cannot establish exclusive
knowledge of the run blockers, replace the NOWAIT attribution with an additional
bounded observer run-lock wait tied to first PID via `pg_blocking_pids`, then
cancel/drain/roll back that observer before gate release; do not infer ownership
from an unattributed NOWAIT failure. This fallback remains test observation only.

## Results and gates retained

All original outcome/history assertions remain mandatory. Command-first cancel
may return revision conflict for the old expected revision; assert that exact
result and preserve committed truth, then use a fresh real cancel to terminate.
Terminal-first cancellation invalidates the paused snapshot; assert the refusal
without evidence/grant, then a fresh bounded command records terminal audit truth
without reopening. Preserve the expiry orders and terminal deadline behavior.
For every truth, retain immutable operation/marker/entry/old attempt and debit,
real provider effect and call counts, exact command/evidence/audit/revision linkage,
no invented output/route/successor, and no claim or new remote entry after terminal.

Deliverable: test-only correction, with exact lock observations and all twelve
variant outcomes. Independent Sol checks this frozen contract against originals
and owners **before** correction. Then run all four final tests at stock2MiB and
affected required checks; initial expiry-only success predates the latest assertion
edits and is not final-source verification. Independently review actual final
source and evidence before root acceptance. SQL query changes retain repository
SQLx preparation requirements. Remaining row2c late/conflicting actual receipt
cases, row2d completion contention and full04.7 gates remain separate and open.

If an independently authoritative requirement is later found to demand the second
business operation itself wait directly on run, this same-company constructor
cannot meet it. Reopen only that observation contract; do not bypass company
authorization, release its lock early, fabricate history, call `cancel_on` directly,
or silently mark that stricter condition passed.
