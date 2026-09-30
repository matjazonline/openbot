# Original row 3c — high-entry native proof and reservation bounds

Root accepted 2026-10-05. Full 04.7 remains incomplete; no 04.8.

The four new high-bound test children retain the genuine 128-call owner history, competing claims, final 128-entry proof, committed historical reservation 129, and independently selected native entry/consumption/xid guards. Their corrected independent original-row3c/code review is unchanged at `/Volumes/ssd1/dev/workflow-recovery-20261005/row3c-code-review-20261005.md`. All reviewed source bytes match the frozen checkpoint; root combines that review with the completed integration gates below. Supported entry 130 is deliberately separate from the unchanged unsupported 129-prior refusal. No selected constraint flush is presented as a failed natural COMMIT.

Fresh evidence: `/Volumes/ssd1/dev/workflow-recovery-20261005/continuation-20261005-token-efficient/`.

- `admission-current.log`: 431 PASS / 0 FAIL / 0 ignored, 284.50s. Includes the new native high-bound test, accepted unsupported 128/129 control, low-bound exclusions, historical identity controls and affected admission/recovery/lease/control/budget/completion/wait/pending/maintenance tests.
- `application-current.log`: 10 PASS / 0 FAIL / 0 ignored. These are disjoint from admission: 441 distinct affected tests.
- `infrastructure-results.json`: formatting/whitespace, locked offline all-target SQLx prepare/check, fresh disposable migrations, retained/fresh schema dumps and explicit graft build all exit 0. All 53 retained and fresh native SHA384 checksums equal immutable migration files. Schema dumps match after removing only random restrict/unrestrict token lines.
- Existing corrected frozen-source locked offline all-target compilation and strict all-target Clippy PASS are reused; no source changed during these gates. Their original logs are `../row3c-offline-alltargets-reviewed-20261005.log` and `../row3c-strict-clippy-reviewed-20261005.log`.
- All 831 checkpoint source paths and 125 protected paths match. `accepted-source-comparison.json` also verifies all 827 accepted row3b paths, normalizing only the exact high-bound child registration. HEAD and empty staged content remain unchanged; LICENSE is preserved. WIP backup: `pre-row3d-wip/`.

Every runtime command uses the authorized SSD cluster, both DATABASE_URL and TEST_DATABASE_URL `postgres://mac03@127.0.0.1:55440/workflow_admission`, SQLX_OFFLINE=true, RUST_MIN_STACK=2097152, locked/offline and default parallelism. SQLx preparation uses live metadata without changing any protected cache byte. The fresh UUID database was dropped; final cluster query found no other clients and only postgres/workflow_admission.

The retained historical 428 PASS / 3 FAIL run is not erased or relabelled fixed. `failure-investigation.md` records all three unchanged focused PASS results, the controlled 430-test comparator PASS and source-level timing windows. Exact historical causes remain unproven; the old unsupported-bound assertion did not print its differing error. No bound, timeout, clock or concurrency adjustment was used to clear the gate.

Original 3d stored/source/time/bounds obligations, 3e full logical/native-COMMIT rollback, 3f populated upgrades and final original full-04.7 acceptance remain open. Original damaged cluster 55439 is untouched and unrecovered; surviving raw copies are not a verified recovery backup.
