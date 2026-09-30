# Queue 2e descendant claim — verification complete; independent code review pending

Scope: accepted `CONTRACT-04.7-DESCENDANT-CLAIM.md`, SHA256
`dee7a7a8865ab10a5aac97d63f8f889aff24f2e9891b6bc11c970c2569009aa1`.
Original Execution6 (`04-actions-http-and-delivery.md:36–38`),
`BRIEF-04.7.md:194–240,261–325`, and `CONTRACT-04.7-CLAIM-BUDGET.md:121–210`
remain authoritative. Preparation PASS is
`/private/tmp/workflow-04.7-descendant-expansion-check-01a0f912.md`.
Root alone owns acceptance, PROGRESS and RESUME. This evidence does not certify
independent actual-code review or complete 04.7/04.8/upgrade/SQL matrices.

The implemented two tests prove reachable ordinary descendant first activation
through two actual I/O claimants while both ancestor run and execution locks remain
held, and genuine child final-not-applied proof recording with
`Blocked(IneligibleJob)` plus a non-child Scheduled control. Neither test executes
a descendant reconciliation episode or its strict model-headroom predicate.
Production child policy, shared claim/accounting owners and all applied migrations
remain unchanged. Actual root reconciliation versus descendant reservation races
retain their separate strict eligibility, ordering, rollback and dispatch coverage.

## Exact verification and evidence reuse

Implementation bundle:
`/private/tmp/workflow-04.7-descendant-implement-01a0f952/`.
Finisher bundle:
`/private/tmp/workflow-04.7-descendant-verify-01a0f963/`.
Exact commands, environment, exits, durations and complete logs are in the finisher
`commands.jsonl` and `logs/`; original test commands and complete logs remain in
the frozen implementation bundle. Both URLs are explicitly
`postgres://mac03@127.0.0.1:55439/workflow_admission`; offline checks and tests use
`SQLX_OFFLINE=true`, `RUST_MIN_STACK=2097152`. SQLx CLI performs its live preparation.

| Gate | Result and complete log |
| --- | --- |
| Strict Clippy | Fresh PASS: `logs/clippy.log`, `cargo clippy --locked --offline --all-targets -- -D warnings` |
| SQLx preparation | Fresh PASS: `logs/sqlx-prepare-escalated.log`, `cargo sqlx prepare -- --all-targets` |
| SQLx consistency | Fresh PASS: `logs/sqlx-prepare-check-escalated.log`, `cargo sqlx prepare --check -- --all-targets` |
| Current action list | Fresh PASS: `logs/action-list-current.log`, `cargo test --locked --offline --lib workflow_action -- --list`; exactly152 names |
| Preservation/schema/coverage | Fresh PASS: `logs/source-schema-final.log`, `source-reuse-check.json`, `migration-live-check.json`, `schema-reuse-check.json`, `all-action-coverage.json` |
| Formatting/whitespace/offline compilation/graft | Implementation PASS reused: `logs/fmt-final.log`, `logs/whitespace-final.log`, `logs/offline-alltargets.log`, `logs/graft-build.log`; all current source bytes match that bundle |

The implementation has **20 fresh passes**:2 new tests,3 required descendant
controls,15 fairness tests, all at stock2MiB. These results were not rerun by the
finisher. The action list is exactly the disjoint union of **150 prior action
passes plus2 new passes**, with all names reconciled against the current binary.
Prior150 consists of126 dispatch passes and24 complementary passes. There is no
single combined152-test execution claim. The passing actual root reconciliation
race evidence and affected control/recovery evidence are reused conditionally;
their owners and fixture sources match the accepted prior bundle.

`source-reuse-check.json` verifies both frozen bundles (29 implementation entries,
40 prior recovery entries), all34 historical verification logs, all860 current
artifact paths (755Rust,53 migrations,45SQLx, dependencies and4 criteria), current
path sets and HEAD`c9cb4b434258b6d33ca26def4ff6b5728dd5a6bc`. Against the
implementation's own-before859 paths, only the inherited fairness file changed,
858 inherited paths are preserved, and only the child policy test file is new.
Eligibility wiring remains equal to own-before. Against prior recovery851 paths,
850 are identical; the sole eligibility difference is the exact85-byte additive
fairness module wiring, proved against the retained failed-attempt original.
Dependencies equal unchanged HEAD. The failed compile and original impossible
tests/checkpoint remain intact; no expectation or policy was weakened to obtain
a pass. SQLx regeneration preserves all45 cached query bytes and the path set.

Fresh live inspection verifies all53 successful SHA384 migration checksums equal
their immutable files. The current normalized retained schema equals both prior
retained schema and the earlier fresh53-migration schema, SHA256
`ff7b1b1366dffd88eb31e0b803c6cbd942af50aee4d899671c7d62e44f0e6452`.
The prior fresh fixture is reused after unchanged migration-byte/path-set and
current live checksum/schema comparison; the finisher did not create a new database.
An empty SQLx delta does not validate runtime SQL; the actual DB-backed tests supply
that evidence.

Sandbox-denied initial PG/schema/SQLx operations are retained beside authorized
host reruns. The sandbox runner's final `pg_ctl` reported a false negative;
`logs/pg-running-final-escalated.log` confirms the unchanged live PID70397.
Retained cluster identity: system7691265791172745923, data
`/private/tmp/workflow-admission-pg-e3aa`, databaseworkflow_admission/usermac03,
port55439/socket`/private/tmp`/max_connections200. No reset, drop, restart,
migration rewrite, unrelated code edit, stage, commit, deployment or root acceptance
edit occurred. The daemon remains running.

Finisher UUID`01a0f963-2662-7140-9612-d7ed9622142b` measured context with the
skill helper at startup/milestones/end; records in the finisher bundle. Worker50%
and root60% limits are unchanged. No additional source discovery was needed, so
the finisher adds zero Graft discovery-saving estimates; prior counts are not
double-counted. Spend/cache are unknown. Await independent Astra actual-code,
original-criteria and integration PASS before root acceptance.
