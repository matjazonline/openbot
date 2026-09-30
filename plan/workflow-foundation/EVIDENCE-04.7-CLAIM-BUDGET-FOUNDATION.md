# 04.7 claim-budget foundation — implemented, not accepted

Original Execution6, BRIEF-04.7:194–240,261–326, BRIEF-03.9-BUDGETS:27–58 and accepted AUDIT/BOUNDS refinements remain authoritative. Frozen contract SHA256 4ea6c4f8652cdd1889ead7ac77abea7702e89b04bb70fa080b393236e45a3938; independent preparation PASS `/private/tmp/workflow-04.7-claim-budget-expansion-check-final.md`. No04.8.

Artifacts and complete logs: `/private/tmp/workflow-04.7-claim-budget-foundation-01a0f6a1/`. HEAD c9cb4b434258b6d33ca26def4ff6b5728dd5a6bc unchanged. Initial diff/status/hash manifests and before/ copies preserve inherited WIP. Own delta, actual/ copies and artifact/dependency/migration/SQLx hashes freeze this bounded foundation. Root alone accepts; independent implementation review is pending.

## Changes and criterion coverage

Additive 20261001100000 migration adds immutable scheduled-command/retired-attempt episode bindings, scoped native FKs and deferred current-transaction guards. Automatic database trigger integration binds the existing settlement atomically, using its real failed-to-pending OLD ordinal and initial waiting/reconciliation witness. Historical commands are untouched; ambiguous historical pending provenance aborts before mutation. All50 previous applied migrations remain hash-identical.

The shared budget predicate replaces the schedule budget clause and is reused only for the first pending claim of that exact episode. Claim locks its existing requesting run/execution/job, then shared usage before fairness; a fresh statement reads committed counters after the usage wait. No other run lock, speculative charge, fake attempt, new runnable queue or changed ordinary retry accounting. First installed next attempt makes the old binding inapplicable.

Ineligibility uses the existing pending_recovery RootBudgetExhausted owner. Database-owned OLD active/deadline/pending facts, exact pending-to-failed confirmation, current shared-budget serialization and current-transaction audit linkage protect the immutable refusal. Immediate and deferred refusal guards reject eligible/consumed/historical/mismatched episodes. Linked audit identity/kind/deletion is protected even within the transaction. Both retry predicates reject refused execution/job; ordinary retries outside a refused episode retain their original policy.

The existing2 real eligibility tests now both pass. Strengthened assertions prove exact company/run/execution/job/command/ordinal binding and refusal, one linked exhaustion audit, unchanged old attempt/debit/truth/proof state, no new lease/attempt/entry/consumption/charge, two inert repeated claims and actual ordinary RetryCommand Unsafe with only its immutable control receipt appended.

## Verification (all DB-backed tests stock2MiB, locked/offline)

Every main DB command explicitly names DATABASE_URL and TEST_DATABASE_URL postgres://mac03@127.0.0.1:55439/workflow_admission. Full literal tool commands/results are preserved in tool-transcript.jsonl; per-command full stdout/stderr logs follow.

- migration-dry-run.log: initial draft transaction-only syntax/FK dry run PASS/ROLLBACK; scoped command FK and multiple-candidate diagnostic were tightened before the single final application. migrate.log final additive migration applied PASS.
- focused.log: original2 eligibility tests2PASS. focused-provenance.log frozen strengthened2PASS (43.02s compile/0.97s runtime).
- action-regressions.log: workflow_action118PASS/0FAIL/0ignored,55.55s. Receipt/mixed/supported replay, late effects, proof/bounds/actor-audit regressions retained.
- budget-regressions.log: workflow_budget24PASS/0FAIL/0ignored,8.43s, including accounting replay, usage waits and descendants.
- control-regressions.log: workflow_control25PASS/1FAIL/0ignored,9.50s. Required combined gate is not green.
- The failed workflow_control_owner_cascade_removes_immutable_receipt hits old181000 workflow_action_state_witnesses_company_id_run_id_fkey (23503). No unrelated fix attempted. Fresh initial-source baseline with exactly50 original migrations reproduces1FAIL on the same old FK. Identified database mail_agents_own_6ffdf0820cf144069fccb69a8b4b97bb, OID15620632; baseline-database-identities.json captures15 samples including50/max20261001090000 and NULL claim_episodes. control-baseline-identified.log is the exact prebuilt original-source test run. Baseline source/dependency hashes freeze the no-newmigration tree; fixture auto-dropped.
- control-baseline.log: initial copy compile failed missing docs/rig.md; copied unchanged docs then baseline-r2/fresh logs reproduce failure, but lacked exact own-database identity. Identified run supersedes those for provenance. An unused explicit task baseline endpoint database was created then dropped; retained DB never dropped/reset.
- fmt.log cargo fmt --all -- --check PASS; whitespace.log git diff --check PASS; new artifact whitespace checked separately.
- offline-alltargets.log SQLX_OFFLINE=true cargo check --locked --offline --all-targets PASS24.54s.
- sqlx-prepare.log cargo sqlx prepare -- --all-targets --locked --offline PASS; sqlx-check.log corresponding --check PASS. Runtime SQL remains validated by the real tests, not SQLx metadata. Cache preservation is in sqlx-final-sha256.json.

## Explicit pending criteria and gates

This first foundation does not complete the frozen contract. Still required: actual debit-vs-two-claim races in both usage-lock orders, debit rollback/root/tenant isolation; equality matrix and saved logical reservation replay; receipt-only claim equality and late accepted-result behavior after exhaustion; evidence-only revision advance, consumed binding and later distinct episodes; deadline/timeout/cancel/lost-response/expired-lease/deferred-abort matrices and full state rollback; parent/root-lock and tenant-fairness lock-order competitors; isolated raw-SQL scoped/binding/current-transaction/refusal/audit substitution/mutation/deletion negatives; fresh and unambiguous populated upgrades, ambiguous-preflight zero-mutation negative; all remaining original04.7 RESUME criteria/matrices, broader affected suites, strict Clippy, complete migrations/schema and final graft gate. Graph auto-refreshed both changed Rust files during focused discovery; explicit final graft build is pending. Old owner-cascade failure must be resolved separately before claiming full combined acceptance. Independent Astra actual-code/original-criteria/integration PASS and root acceptance remain pending.

## Resources and context

Worker /root/claim_budget_foundation, own verified UUID01a0f6a1-5ed9-7d22-a155-b5a900328986, Sol implementation/no spawning. Startup28,933/258,40011.20%; milestones83,41532.28%,95,72537.05%,105,43840.80%,119,89846.40%@09:02:19.188Z. Sources token_usage_record.usage/task_started.model_context_window; estimates, not spend. Stop before50% and transfer sole resource ownership to root.

Retained PG18 system ID7691265791172745923 verified cleanSTOPPED before exact startup -h127.0.0.1 -p55439 -k/private/tmp -cmax_connections=200. Retained historical pending scheduled-job preflight count0, original applied50. schema.log final scoped constraints and51 migrations. Final identity/migration/state/stop logs and fresh context sample are linked from HANDOFF.md. No commit/stage/deploy/reset/retained drop/bound raise.
