# 04.7 bounded authoritative proof / one-use / clock evidence

Implementation evidence only; root owns acceptance. Focused independent review and
full04.7 criteria/integration/gates remain pending. No04.8 work. Preserve baseline
655aaaba3132c561809f97fd25c89f600bca1fa9 plus allpreexistingwork; applied migrations
through20260930184000 immutable. No production/schema edit or material contract gap.

New `src/adapters/persistence/workflow/action_reconciliation_proof_tests.rs` and
`action_reconciliation_proof_clock_tests.rs` contain seven isolated real-DB tests.
Only existing source delta is authority_tests child-module wiring. They reuse the
exact existing genuine provider ledger, actor grants, service/SQL and competing
claim helper. Provider file and authority_support are unchanged by this assignment.
Functions stay within80lines; DB/provider phase seams are boxed for stock2MiB stack.

All no-change assertions snapshot **every public table** through a repeatable-read
catalog read, sorting complete rows. This includes provider facts, all reconciliation
joins/observations/receipts/conflicts/commands/audits/witnesses and runtime budget,
attempt, intent, marker, run/job/execution rows.

Test prefix is `workflow_action_reconciliation_proof_`:

| Criterion | Concrete check |
| --- | --- |
| Genuine barrier versus delayed effect | `provider_barrier_competes_with_delayed_effect`: three joined real DB races on the provider operation lock. Effect winner creates Applied receipt-only continuation; barrier winner creates final proof. Effect count matches winner; later covered application fails. |
| Concurrent commands/claimants/entries; one-use | `competing_commands_claimants_loss_and_fresh_coverage`: both genuine verifier calls rendezvous after snapshot/verification before settlement. Exactly2command receipts,1effective evidence/proof. Two claimants yield1owner. Same-attempt concurrent dispatch produces1poll/timeout and1PossibleDispatchExists:2total entries/1consumption/0effects. Scheduling preserves old attempts/debits/intent/marker/entry. |
| Old remote request remains quiescent after new entry | Same test captures exact OLD entry before new claim and calls authoritative apply under provider-operation lock while new request is active: false. This avoids attributing a callback to the existing delayed_apply helper's latest-row selection. |
| Loss after consumption, restart, ordinary retry, fresh full coverage | Same test parks lost second Pending request: needs_reconciliation/retry_safe=false. Fresh service records genuine Unknown; exact replay changes no table; ordinaryRetryUnsafe and claimNone. Genuine barrier then fresh proof covers2entries; next actual call succeeds once:3entries/2consumptions/1effect. |
| Legitimate new consumed request Applied | `new_consumed_entry_applied_recovery_is_legitimate`: new call really applies, response is lost, recovered Applied receipt schedules receipt-only and creates0conflicts. Restart returns saved receipt with0provider polls, retaining2entries/1consumption/1effect. |
| Current original dispatch authority | `admin_cannot_authorize_revoked_original_new_call`: separately original principal deletion and resource revocation. Distinct valid admin may attach final proof/schedule existing job; actual new dispatch refuses with0polls,1old entry/0consumptions and full-table equality. |
| Expiry before settlement | `expiry_while_settlement_waits_for_run_lock`: genuine short-lived proof returns while joined competitor holds actual owning-run lock past validity. Blocked settlement has no evidence, unchanged revision,1entry/0consumption; all tables equal except1immutable refusal receipt. |
| Expiry before reserve/enter; expired successful replay | `expiry_reservation_enter_and_successful_replay`: DB clock proves expiry. Reserve refuses with1entry/0consumption; enter refuses retaining2entries/1consumption. Refusal and successful exact replay preserve all tables; exact original outcome/revision/evidence returned, verifier calls1, actual provider polls0/effects0. |
| Bounded/cancellable/owned verification | `verifier_budget_ceiling_cancel_and_drop_are_owned`: actual verifier performs genuine ledger read then pauses. Separate100msbudget,5sceiling despite10sbudget, callerCancel and callerDrop. Correct Timeout/Conflict, live Drop guard0/calls1, all-table equality,1entry/0consumption. No detached future/spawned competitor. |

Exact retained PG inspected workflow_admission/mac03/data/private/tmp/workflow-admission-pg-e3aa/
port55439/max_connections200. Root released authority freeze and transferred sole
PG/Cargo/SQLx/project ownership. Tests use isolated AdmissionFixture databases;
retained data never reset/dropped. Root alone edits PROGRESS/RESUME/acceptance.

Exact commands:

```
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib action_reconciliation_receipt_tests
cargo fmt --all -- --check
git diff --check
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=false RUST_MIN_STACK=2097152 cargo sqlx prepare -- --all-targets --locked --offline
```

Focused7PASS/0FAIL/0ignored in `/private/tmp/workflow-04.7-proof-r3.log`,8.68s.
Final combined24PASS/0FAIL/0ignored (new7+retained17),2290filtered,10.57s in
`/private/tmp/workflow-04.7-proof-combined-final.log`. Final fmt/whitespace PASS in
`-fmt-final.log`/`-whitespace-final.log`. Earlier SQLxprepare PASS37.23s/no.sqlxdelta
in `-sqlx.log`; final preparation after exact-old-entry runtimeSQL addition is
`-sqlx-final.log`, status in frozenHANDOFF. Runtime SQL is verified by real DB tests;
macro cache does not prove these queries. Initial focusedr1/r2 preceded clocks and
temporary duplicate support warnings (corrected by nesting). No failed runtime tests.

Remaining full04.7: paused changed coverage/marker/operation/late receipt; all sibling
receipt/proof/replay/Unknown/AppliedNoResult/conflict combinations and128/129all-gate
bounds; authentic foreign tenant/association/errors; full recovery eligibility,
budget/attempt/child/poison/deadline boundaries; SQL provenance/orphan/immutability/
witness/historical retrofit/rollback; terminal applied/final ordering and shared
conflict/completion; populated-history preservation/malformed migration rejection.
Broader affected stock2MiB suites, strictClippy/offlinealltargets, fresh migrations/
schema, SQLxprepare/check, graft and independent FULL original-criteria/actual-code/
integration PASS/root acceptance remain required. Scoped evidence does not accept04.7.

Preparation `/private/tmp/workflow-04.7-proof-preparation.md`; frozen review tree/delta/
hashes `/private/tmp/workflow-04.7-proof-frozen/`. Worker /root/reconciliation_proof_matrix,
verifiedUUID01a0f46e-64c5-7d73-a024-3758af0e73f2, Sol6.1/high. Startup24183/2584009.36%;
preparation78064/25840030.21% exceeded25%aim from guide/output truncation (context,
not measured spend). Milestone115087/25840044.54%@22:48:58.211Z, usage/window estimate.
50%worker stop enforced; fresh final sample and ownership handback in frozenHANDOFF.
