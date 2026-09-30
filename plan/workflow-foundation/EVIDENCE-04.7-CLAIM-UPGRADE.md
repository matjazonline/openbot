# 04.7 group 2f upgrade — implementation verified, independent review pending

Final evidence: `/private/tmp/workflow-04.7-upgrade-finish-01a0fb23/`; inherited
implementation: `/private/tmp/workflow-04.7-upgrade-01a0f971/`. PREPARATION.md there
maps original Execution6, BRIEF04.7:194–240/261–325 and CLAIM-BUDGET152–169/204–205.
HEAD remains c9cb4b434258b6d33ca26def4ff6b5728dd5a6bc; all prior WIP is preserved.
Final manifests and combined-upgrade.delta.patch bind five edited test-support/module
files and two new test files against accepted 2e. No production behavior, migration,
Cargo, stack setting or SQLx metadata bytes changed. Root alone owns acceptance.

All three genuine database-backed upgrade tests PASS at stock2097152-byte stacks:

- Fresh53: exact versions/checksums/success, empty provenance and idempotent rerun.
- Actual historical50-prefix through20261001090000: real admission/old claim,
  lost-applied dispatch, retirement, recovered receipt/evidence/command/audit, old
  claim2 and completion. Upgrade to53 preserves every original public table's
  canonical rows and prior migration metadata, creates no guessed provenance,
  and reruns without changing history.
- Same genuine scheduled pending lease-free historical job: twice migration
  20261001100000 rejects23514 with the exact named ambiguous-provenance error.
  Every public row, migration row and public relation/attribute/constraint/trigger/
  function catalog remains unchanged. No schema reconstruction or history backfill.

Historical claim fixture remains byte-for-byte committed pre-upgrade lease_claim.rs,
SHA2564d1f12cc5d45eb2324fbce070881d82002a8a5ceceb1f0512da86a099e1be3d2,
compiled test-only in its matching namespace. It establishes historical input,
not current-owner coverage. Shared post-claim setup is extracted unchanged;
OwnDatabase adds boxed migrator selection and constructs its guard before migration.

The inherited131 regression completed130 and stalled in the existing verifier-clock
test, then was terminated; that original run remains incomplete. Source confirms
its join could await verifier notification forever after an early service result.
The original log cannot identify its exact stalled phase. The correction observes
notification, an early actual result, or a bounded watchdog, requires an active
verifier before stopping, and retains100ms caller budget, service5s ceiling,
original elapsed assertions, calls1/active0, cancellation/drop ownership and exact
durable-state preservation. No deadline or stack bound was raised. diagnosis.md
records the distinction between this confirmed harness hazard and the unknown
cause of the original run; it is not classified as flaky or passing.

Final commands.jsonl/logs bind exact commands, URLs, stock2MiB stacks and results:
focused verifier1PASS; full original-parallel action/upgrade131PASS172.56s; additional
workflow/control/admission/budget/lease/recovery/completion/maintenance plus task
claim/delivery/inbound integration325PASS349.89s. coverage.json proves456 unique
affected tests. Format, whitespace, offlinealltargets, strictClippy, SQLxprepare,
SQLxprepare--check and graft build PASS. Live53 SHA384 matches every migration;
retained normalized schema matches verified fresh53 schema, with unchanged source
and frozen historical log provenance binding reuse. Sandbox failures remain in logs.

Every disposable database was handle-cleaned. Final cluster query confirms
system7691265791172745923, data/private/tmp/workflow-admission-pg-e3aa, port55439,
workflow_admission/mac03, socket/private/tmp, max_connections200; onlypostgres and
workflow_admission remain, with zero other clients or probe/test_gate triggers.
Retained daemon remains RUNNING. Independent Astra actual-code/original-criteria/
integration review is still required. No full04.7 or phase04 completion,04.8,
compatibility widening, stage, commit or deployment is claimed.
