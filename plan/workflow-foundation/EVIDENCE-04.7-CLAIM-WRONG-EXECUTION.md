# Bounded wrong-execution claim-refusal audit check

Implemented subcase of V1 item 2; independent actual-code review and root acceptance
remain pending. Full item 2, claim-budget foundation and 04.7 remain incomplete.
Original Execution 6, BRIEF-04.7:194–240,261–326, CLAIM-BUDGET refusal/audit criteria,
AUDIT and RESUME remain authoritative. No 04.8 work.

Frozen baseline: accepted six-test subset and named-witness correction at
`/private/tmp/workflow-04.7-claim-sql-struct-fix-01a0f727/HANDOFF.md`, with explicit
scoped PASS `/private/tmp/workflow-04.7-claim-sql-code-review-final.md`.
HEAD `c9cb4b434258b6d33ca26def4ff6b5728dd5a6bc` and inherited WIP are preserved.
Own artifacts, exact commands/full logs, before/delta/hash maps and final handoff:
`/private/tmp/workflow-04.7-wrong-execution-01a0f734/`.

The new nested test module is
`src/adapters/persistence/workflow/action_reconciliation_claim_sql_wrong_execution_tests.rs`.
The accepted SQL module receives only its three-line module registration; all six
test bodies and existing helpers retain their bytes. No production, shared fixture,
migration, protected-history or resource-bound changes.

| Criterion | Discriminating check |
| --- | --- |
| Two genuine executions, same company/run | A validated two-step source is frozen before admission; actual first claim and successful `complete_io` create the second execution/job through the existing successor owner. Company/run equality and execution/job inequality are asserted. |
| Genuine second-execution episode and exhaustion | Actual action invoke/lost-response park/barrier/proof command schedules the second execution. A genuine child of the completed first/start execution shares its root; actual child claim and `Granted(2)` reservation exhaust the model allowance. The shared child fixture's literal start step therefore names a real matching parent. |
| Genuine current retirement | Existing `current_retirement` takes the owner locks and calls `pending_recovery::settle`; database-owned witness asserts confirmed retirement/current xid and supplies the linked audit sequence. The event identity query confirms the exact second execution and exhaustion kind. |
| Otherwise valid wrong-execution audit | Existing audit writer appends an exhaustion-kind event referencing the genuine first execution. Its company/run/execution/kind and different sequence are asserted. Its execution FK and refusal's company/run/sequence FK are valid. This event is the deliberate substitution candidate, not retirement provenance. |
| Positive control excludes unrelated guard failures | Within a savepoint, the exact genuine second audit allows refusal insertion and `SET CONSTRAINTS ALL IMMEDIATE`; rollback to the savepoint removes only the positive refusal. The same episode/retirement transaction then proposes the first execution's sequence. |
| Exact refusal guard and rollback | The substitution fails SQLSTATE `23514`, exact `invalid current workflow reconciliation budget refusal`. Explicit full transaction rollback is followed by equality of every public table, including both executions, revision/job/attempt/episode/refusal/audit/accounting and action/proof history. |

Focused combined seven tests PASS: 7 passed, 0 failed, 0 ignored, 2.03 seconds after
compilation. Exact retained DATABASE_URL and TEST_DATABASE_URL, SQLX_OFFLINE=true,
RUST_MIN_STACK=2097152, locked/offline. First compile caught a wrong fixture type
import; corrected to the lease owner. First runtime kept the accepted six green but
exposed the shared child's literal start step against the second execution; corrected
only the new fixture to use its genuine first/start parent. Both failed logs remain.
Formatting/whitespace, locked/offline all-target compilation (23.12 seconds), strict
all-target Clippy (36.94 seconds), SQLx prepare (15.63 seconds), SQLx prepare --check
(10.26 seconds) and graph refresh all PASS. Full logs and exact argv/timestamps/exits
are recorded in the frozen handoff. All 45 SQLx cache files retain their hashes.

Prior 104-test affected action suite and unchanged schema evidence are reused only
for the unchanged owner paths. Fresh combined seven, offline all-target compilation,
strict all-target Clippy, formatting/whitespace and SQLx prepare/check cover the new
test/module wiring. Runtime SQL generates no new macro metadata, so unchanged cache
hashes alone do not prove the new identity query; the DB-backed test exercises it.

All other V1 item 2 cases remain pending, including episode command/ordinal/current-xid/
deferred/duplicate isolation, stronger historical genuine-retirement substitution and
deferred greater-attempt refusal discrimination. V1 items 3–4 and the full original
runtime/upgrade/04.7 matrices are outside this bounded assignment. Root alone records
acceptance after independent Astra PASS.

Retained PG18.6 system `7691265791172745923`, data
`/private/tmp/workflow-admission-pg-e3aa`, workflow_admission/mac03, port55439,
socket `/private/tmp`, max_connections200 is verified before use. All 53 local/live
SHA384 migration checksums match the accepted baseline. No retained reset/drop,
checksum rewrite, staging, commit or deployment. Disposable fixture databases belong
to the existing isolated owner. Final cleanup/clean shutdown is in the handoff.
