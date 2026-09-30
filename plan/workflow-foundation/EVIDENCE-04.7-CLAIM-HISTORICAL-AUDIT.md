# Historical same-execution audit with genuine current retirement

Implementation and required scoped checks PASS; independent actual-code review and
root acceptance pending. This covers only the stronger historical-audit substitution
subcase, not full V1 item 2, claim-budget foundation or 04.7. No 04.8 work.
Original Execution 6, BRIEF-04.7 section 5/acceptance, CLAIM-BUDGET:89–118,140–145,
208–218 and AUDIT remain authoritative. Accepted fixture decision:
CONTRACT-04.7-HISTORICAL-AUDIT-TEST.md SHA256
`df9b1bef65913ab46f8678bff0837560ddb4e1e0e499e080485ba75bc2f6a8da`;
independent feasibility check `/private/tmp/workflow-04.7-historical-audit-expansion-check.md`.

Own delta: three-line nested registration in `action_reconciliation_claim_sql_tests.rs`,
new `action_reconciliation_claim_sql_historical_tests.rs`, and this evidence. Removing
only that registration restores the accepted seven-test parent exactly; the six
previous cases and accepted wrong-execution fixture remain unchanged. No production,
migration, API, guard, limit or protected-history change.

The new fixture uses the existing real action/provider/park/final-proof/command
schedule and actual child admission/claim/shared-root reservation. Its audit owner
commits a same-execution literal exhaustion event and records the actual writing xid
and event xmin (:15–24). The event is independently labelled history; no historical
genuine budget retirement is reopened. Before the attack, :26–49 explicitly checks
the active unexpired owner, genuine pending lease-free episode, activated uncompleted
execution, no greater attempt, false shared budget predicate and absent retirement/
refusal. The all-public-table baseline follows the history commit.

The test transaction installs a uniquely named, exact-scoped BEFORE INSERT event
probe (:54–152), takes existing `lease::lock_scope`, and calls the complete production
`pending_recovery::settle(...RootBudgetExhausted)` (:164–192). Before the attempted
second exhaustion event reaches its unique index or AFTER INSERT audit linker, the
probe verifies exactly one database-generated current-xid witness for the genuine
episode/ordinal, confirmed retirement, NULL audit link, failed lease-free job with
unchanged retry count/no greater attempt, activated uncompleted execution, correct
retired run state selected by actual action truth, future owner/witness deadlines,
and current ineligibility. It also checks the old event's exact scope/kind, distinct
sequence/transaction and unchanged original xmin, with no existing refusal.

Only the malicious refusal INSERT is inside the exception handler. It uses the real
episode's full composite references and historical sequence. The handler captures
actual RETURNED_SQLSTATE/MESSAGE_TEXT and rethrows their exact values; Rust requires
`23514 invalid current workflow reconciliation budget refusal`. Prerequisite errors,
other database errors and unexpected INSERT acceptance each have distinct diagnostics.
The probe never writes witnesses, modifies history/NEW, disables a trigger, suppresses
the production event, or continues into event uniqueness after rejection.

Explicit outer rollback and exact all-public-table equality (:197–202) preserve
history and all episode/witness/refusal/run/job/revision/attempt/accounting/receipt/
proof/action/provider facts. Catalog checks prove transactional function and trigger
disappearance; final event query preserves the original xmin (:203–210). No successful
second retirement/audit is claimed. This tests missing current audit linkage, not a
standalone timestamp-only comparison, historical genuine witness/xid substitution,
or deferred greater-attempt revalidation; those remaining criteria stay open.

Final stock-2MiB focused refusal subset: 8 PASS, 0 failed/ignored, 2.40s runtime;
includes the real committed owner refusal positive control and wrong-kind/wrong-
execution cases. Separate non-reconciliation uniqueness/audit test: 1 PASS. Formatting,
whitespace, locked offline all-target compilation, strict all-target Clippy, mandatory
SQLx prepare/check and graft refresh PASS. SQLx's 45 macro-cache hashes are unchanged;
runtime probe SQL is verified by the actual database test. Previous broader suites
are reused only for unchanged production owners/migrations; prior 104-test evidence
predates the earlier accepted lock-scope helper correction, as previously disclosed.
Fresh scoped checks cover that helper; no full phase gate is inferred.

Exact commands, full logs, before/delta snapshots, criteria/dependency/migration/cache
hashes and resumable freeze: `/private/tmp/workflow-04.7-historical-audit-01a0f746/HANDOFF.md`.
All 53 local/live migration checksums remain unchanged. PG18.6 system7691265791172745923,
retained data `/private/tmp/workflow-admission-pg-e3aa`, database workflow_admission,
user mac03, port55439/socket/private/tmp/max_connections200 verified before/after.
Only isolated test resources were used; none remain. Retained PG cleanly STOPPED.
No retained reset/drop/checksum rewrite, staging, commit, deployment or child agent.
Worker verified UUID `01a0f746-1636-7450-b93c-3a3595671954`; measured samples and exact
provenance are preserved in the freeze. Root alone records acceptance.
