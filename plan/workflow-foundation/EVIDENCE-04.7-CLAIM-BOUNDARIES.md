# 04.7 claim-budget boundaries — bounded implementation evidence

Direct queue2b implementation/checks PASS; independent Astra actual-code review and
root acceptance pending. No full CLAIM-BUDGET/V1/foundation/04.7 acceptance or04.8.
Original README rules, Execution6, BRIEF-04.7:194–240,261–326 and accepted
CONTRACT-04.7-CLAIM-BUDGET.md remain authoritative. Preparation/criterion mapping,
own-before snapshots, exact delta, dependencies, checksum/hash manifests and complete
logs: `/private/tmp/workflow-04.7-claim-boundaries-01a0f865/HANDOFF.md`.
HEAD c9cb4b434258b6d33ca26def4ff6b5728dd5a6bc plus inherited WIP preserved.

Only source delta: nested test registration in action_reconciliation_claim_sql_tests.rs
and new action_reconciliation_claim_boundary_tests.rs. No production, SQL guard,
API, migration, bound, accounting or policy change. Limits are fixed at fixture creation;
no existing test limit or persisted budget is changed to obtain a claim.

| Case | Actual-owner discriminatory check |
| --- | --- |
| `workflow_action_reconciliation_claim_boundary_activation_repetition_equality` | Actual first activation at limit1 and genuine Granted repetition2 at limit2 precede real final-proof scheduling. Two competing claim_io calls install exactly one next attempt on the frozen execution. Both exact usage equalities, saved accounting/episode/command/truth/provider facts and prior attempts survive; no double charge, new remote entry or proof consumption. |
| `workflow_action_reconciliation_claim_boundary_receipt_only_model_headroom` | Real provider effect with lost response, trusted applied verification and recovered receipt schedule receipt-only work. An actual descendant claim/reservation uses1 or2 model calls at the same unchanged limit2. Below-limit permits exactly one of two actual claims; its real dispatcher returns the saved result with zero provider polls. Equality permits zero claims and records one exact original episode refusal/audit/retirement with no new attempt/debit/entry/consumption. Receipt/effect/history remain; subsequent competing polls are completely inert. |
| `workflow_action_reconciliation_claim_boundary_roots_and_companies_same_database` | One migrated isolated database holds the subject episode, an independent same-company root and an independent foreign-company root. Real admission/claim/debits bring each unrelated root to its own unchanged limit2. Their own shared predicates become false; the subject predicate remains true and every subject-scoped public fact stays exact. Explicit company/run/root links and usage0/2/2 are checked. Two subject claimants install one next attempt, preserving both unrelated roots and every saved accounting/truth/provider fact. Separate databases do not substitute for this test. |

Tests retain the owning isolated-database handles and use the accepted canonical
REPEATABLE READ READ ONLY all-public-table snapshot helper. The claim assertions
preserve all prior attempts, exactly one additional attempt, frozen execution,
episode/refusal, immutable evidence/commands/receipts/markers/entries/consumption,
all accounting and every fixture provider/authority table. No copied owner UPDATE,
disabled guard, synthetic retirement/witness, fabricated claim charge or detached task.
New functions remain below80lines; extended fixture/service seams are boxed for stock stack.

Stable final `claim_sql_tests`: **21passed,0failed,0ignored**,7.88s runtime,
stock2MiB. This includes accepted18 plus new3 (receipt test covers two boundaries).
Broader dispatch:119passed/0failed/0ignored,70.34s; budget:43passed/0failed/0ignored,
14.22s. Dispatch119 preceded only a pure new-test assertion extraction and removal
of a same-typed tuple; unchanged116 evidence is reused, and final21 reran every
affected assertion on stable bytes. All tests use explicit retained DATABASE_URL and
TEST_DATABASE_URL, SQLX_OFFLINE=true, RUST_MIN_STACK=2097152, locked/offline Cargo.
Earlier first3 and intermediate21 runs also passed; no failed run or production defect.

PASS on stable source: formatting and tracked/new-file whitespace, locked offline
all-target compilation, strictClippy -Dwarnings, migrate run/info, retained scoped
constraints/enabled immediate/deferred guard inspection, SQLx prepare/check alltargets,
graft build. Every fixture migrates a fresh database with all53 migrations. Runtime SQL
is exercised by these actual-owner tests; unchanged macro cache is not runtime SQL proof.
All53 local migration SHA384 checksums match retained live values; all53 migration
hashes and45 SQLx hashes remain unchanged. Root PROGRESS updates are excluded from
worker preservation assertions. No commit/stage/publish/deploy/reset/drop of retained data.

Accepted saved-Granted equality, model-equality final-proof refusal, genuine debit/claim
race orders/rollback and consumed/distinct episode lifecycle evidence are reused only
while their unchanged source/criteria/fixture assumptions hold. They are not duplicated.
Remaining late accepted result after exhaustion, cancellation/lost-response/recovery,
fairness/lock-order and fresh/populated-upgrade matrices, unresolved SQL A/C, every
other nested original RESUME criterion and final full integration/acceptance gates
remain open. This subset has no worker acceptance authority.

Retained PG18.6/system7691265791172745923/data/private/tmp/workflow-admission-pg-e3aa,
DBworkflow_admission/usermac03/port55439/socket/private/tmp/max_connections200 was
verified and restarted with authorized options. Initial/final identity/schema-column/
53live-checksum outputs are byte-equal. Final other clients0, disposable databases0,
retained probe/attack triggers0; remaining databases postgres/workflow_admission.
Fast STOP exit0; final control verifies identical system/shut down. All own operations
are drained and source frozen for independent review.

Worker `/root/claim_boundaries`, verified CODEX_THREAD_ID
01a0f865-7c33-7ea1-8b7f-aa9b6f59ca2b, Sol implementer/no children.
Context token_usage_record.usage/task_started.model_context_window estimates:
startup26864/258400=10.40%; preparation85489=33.08%; implementation96733=37.44%;
stable-tests104037=40.26%; final-gates112293=43.46% at2026-10-01T17:21:05.372Z.
Fresh completion sample in frozen HANDOFF/context-final.json. Graft estimated discovery
savings3103594tokens; graph estimates are not model spend, cache or optimization savings.

F1 correction: independent review found run_facts omitted background_tasks and task_attempts. It now takes exact company/run/execution/job scope, validates one exact owned execution and job plus one genuine prior attempt, and includes both complete row sets in the before/after unrelated-debit and subject-claim comparisons. Affected3 PASS stock2MiB; all changed-source format/static/migration/SQLx/graft gates PASS. Unchanged18 focused/116 broad/43 budget evidence remains reused. No production/schema change; F1 re-review pending. Correction before/delta/hashes/logs are frozen in f1/.
