# 04.7 bounded siblings / existing-job evidence

Implementation evidence only. Root owns acceptance; independent scoped review and
FULL04.7 criteria/integration/gates remain required. No04.8. Preserve baseline
655aaaba3132c561809f97fd25c89f600bca1fa9 plus ALL pre-existing/accepted partial work;
applied migrations through20260930184000 immutable. No production/schema change.

Five isolated real-DB tests in action_reconciliation_sibling_tests.rs and its
sibling_boundary_tests child reuse the accepted genuine provider ledger and private
ancestor fixtures. Only existing-source delta is three-line child wiring in
proof_tests.rs. New invocations go through actual prepare_step with distinct schema-
valid arguments, current authority and real dispatch; no copied marker/receipt,
disabled guard or widened production port creates a safe sibling. The 129 case uses
normal renew_io on the original live fence, rather than raising any bound.

Every durable snapshot uses the accepted all-public-tables repeatable-read helper.
Scheduling explicitly preserves complete old attempt/debit/usage/intent/marker/entry
rows, same job id, execution association, retry_count/max_retries/payload and existing
activation/output/route/successor. Continued claims use two genuine contenders.

Test prefix workflow_action_reconciliation_siblings_:

| Test | Observable discriminatory check |
| --- | --- |
| all_receipts_all_proofs_and_mixed_continue_individually | Three invocations in one execution; each of three receipt/proof compositions. First two safe siblings cannot schedule while the remaining sibling is unresolved. Last resolves exactly the existing job, receipt_only agrees with all receipts. Normal claimed continuation returns receipt siblings with zero provider polls; every proof sibling creates exactly one new entry/consumption and effect. Old attempts remain byte-for-byte present. Normal complete_io succeeds. |
| unknown_and_applied_without_output_keep_all_parked | Receipt + final proof + Unknown, Applied without recoverable output, or schema-invalid recovered output. No command schedules; SQL safety false; real claim yields None with full-table equality. Exactly3 old entries/0consumptions, one accepted receipt, waiting/reconciliation, no replacement attempt/debit/output. |
| later_valid_result_resolves_positive_truth | Receipt + genuinely applied effect whose valid output is initially unrecoverable. Both initial commands leave parked/SQL unsafe. Provider later recovers the same saved result; new genuine Applied command schedules receipt-only without altering prior history; normal continuation has zero provider polls and complete_io succeeds. This case is Reconcile policy, not SupportedReplay policy. |
| existing_job_reopen_gates_preserve_history | Seven negative states: running, decision (existing human wait reason), failed/cancelled/succeeded, expired deadline, attempt cap. Each has two genuine final proofs. Last proof attaches authorized truth and advances generated revision but cannot schedule. Complete job/execution snapshots unchanged, old attempts/debits/lineage/counters unchanged, no consumption/output. Uses SQL-valid lower-level run states; this is not a competing cancellation/expiry owner test. |
| 128_and_129_snapshot_and_actual_recovery_never_truncate |128 genuinely dispatched accepted sibling receipts: SQL safe, authorized snapshot allowed, actual Rust expiry retirement classifies safe in rollback transaction restoring every table.129th genuinely accepted sibling: SQL unsafe despite all receipts; actual release retirement classifies unknown/parks. Fresh authorized snapshot returns BoundExceeded, real claim None; complete tables equal except one immutable refusal command. This covers sibling snapshot/shared ordinary recovery boundaries only. |

Runtime SQL fixture queries were added, so required SQLx prepare is run even though
macro cache is unchanged; real DB tests validate these runtime queries. Both URLs
are explicitly postgres://mac03@127.0.0.1:55439/workflow_admission;
SQLX_OFFLINE=true/RUST_MIN_STACK=2097152 and --locked --offline for tests. Retained
PG identity verified workflow_admission/mac03/data/private/tmp/workflow-admission-pg-e3aa/
port55439/max_connections200. Isolated fixture databases drained normally; retained
DB never reset/dropped. Root transferred sole resources after corrected proof PASS.

Commands / full logs:

```
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib action_reconciliation_sibling
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib action_reconciliation_receipt_tests
cargo fmt --all -- --check
git diff --check
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=false RUST_MIN_STACK=2097152 cargo sqlx prepare -- --all-targets --locked --offline
```

Focused5PASS/0FAIL/0ignored,2314filtered,5.69s: /private/tmp/workflow-04.7-siblings-r4.log.
Pre-split combined29PASS/0FAIL/0ignored,2290filtered,12.32s: -combined-final.log.
Final source split required by source500line test-module rule; final29 result, fmt,
whitespace/SQLx completion and hashes are in /private/tmp/workflow-04.7-siblings-frozen/HANDOFF.md.
Full logs retain initial fixture corrections: r1 ordered attempt index replaced by
full old-row membership; r2 incorrect human wait spelling corrected to decision;
r3 expired deadline now respects deadline>created_at and large fixture uses normal
renew_io to maintain lease. These were test-fixture defects, no production defect.

Exact remaining matrix from this group: mixed SupportedReplay sibling; any conflict
sibling absolute veto and shared completion ordering; SupportedReplay+AppliedNoResult/
invalid-result later valid receipt through real recovery with SQL/Rust equivalence;
128/129 remote entries and sibling settlement/scheduling/reserve/enter gates, including
paused overflow after128 snapshot. Eligibility negatives for child admission,
runnable sibling, unactivated/completed/output/route/successor, activation/max_steps,
lease-bearing failed job, missing retired attempt, poison codes and root budget
exhaustion; current claim exhaustion recheck. Terminal Applied/Unknown with exact
replay/refusal/generated revisions and real competing cancel/expire remain pending.
Paused changed coverage/marker/operation/late receipt; valid foreign associations/
resource errors; all isolated SQL provenance/source-XOR/orphan/immutability/witness/
retrofit/rollback; populated upgrade/malformed history; affected broad stock2MiB,
strictClippy/offlinealltargets/freshschema/allmigrations/SQLx check/graft and independent
FULL originalcriteria/actualcode/integration PASS/root acceptance remain required.
No whole04.7 completion claim.
