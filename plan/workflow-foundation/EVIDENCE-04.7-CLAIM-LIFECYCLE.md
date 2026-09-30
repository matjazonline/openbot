# 04.7 F1 owner lifecycle correction — focused checks passed; not accepted

Scope is only F1/P2 from `/private/tmp/workflow-04.7-claim-budget-foundation-code-review.md`.
Original Execution6, BRIEF-04.7:194–240,261–326, BRIEF-03.9-BUDGETS:27–58,
AUDIT contract and frozen CLAIM-BUDGET SHA2564ea6c4f8652cdd1889ead7ac77abea7702e89b04bb70fa080b393236e45a3938
remain authoritative. No04.8, no foundation acceptance. V1 and every pending
foundation/protocol acceptance requirement remain pending without substitution.

Artifacts/full logs: `/private/tmp/workflow-04.7-claim-lifecycle-01a0f6bd/`.
HEADc9cb4b434258b6d33ca26def4ff6b5728dd5a6bc unchanged; all inherited WIP retained.
`initial-sha256.json`/before/ and initial.diff.patch freeze the inherited baseline.
`own.delta.patch`, actual/ and artifact/dependency/all-migrations/SQLx manifests
freeze this correction. All51 pre-existing SQL files unchanged.53 SQL files now.

## Criterion/check mapping

- Owner lifecycle: additive110000 makes the old state witness and the complete
  immutable action effect/evidence/episode graph participate in run ownership.
  Shared fact/retirement/actor-audit/refusal-audit DELETE exceptions require nested
  trigger execution and an absent owning run. UPDATE checks remain unchanged.
  Additive120000 coordinates native FK cascades across the exact14 named action
  fact tables while retaining all scoped columns and original deferral modes.
  No row backfill, state rewrite, fake charge, protocol or accounting-policy change.
  These inherited facts are necessary ancestors/siblings of the populated new
  claim graph; repairing only the new4 leaves the company deletion blocked.
- Ordinary operator retry: `workflow_control_ordinary_retry_owner_cascade_preserves_live_witnesses`
  uses actual safe failure + RetryCommand, asserts old state/new schedule witnesses
  exist, rejects direct DELETE/UPDATE and deletes the company. Owned graph is empty.
  `ordinary-final.log`:1PASS, stock2MiB, real task-owned DB.
- Scheduled/refused reconciliation: `workflow_action_reconciliation_owner_lifecycle_scheduled_and_refused`
  uses genuine remote uncertainty/final barrier/schedule, plus real descendant
  reservation and actual claim refusal. Both scheduled and refused variants pass.
  All new4 tables have their expected pre-delete cardinalities; all owned fact,
  budget, job, attempt and linked audit tables are empty after company deletion.
  Direct DELETE/UPDATE of every populated immutable fact table and protected audit
  DELETE/no-op/kind-identity/execution mutation fail and preserve full snapshots.
  `focused-final.log`:1PASS covering both variants, stock2MiB, real own DB.
- Existing lifecycle control: `workflow_control_owner_cascade_removes_immutable_receipt`
  `old-control-final.log`:1PASS. Original migration181000 blocker is corrected.
- Formatting/whitespace: standalone fmt-check.log and whitespace.log exit0.
  `graft-build.log`: explicit rebuild exit0,3 files parsed/746 replayed.

## Execution error and immutable recovery

First broad `owner_` test filter also matched unrelated shared `test_pool` tests.
Those applied the first draft110000 to the retained database. I initially mistook
its application as disposable-only and edited that migration. `focused-r2.log`
records the shared database VersionMismatch plus the extra direct-run-control
failure. This is my execution error, not a pre-existing failure attribution.

Restored exact first-applied110000 bytes from my first-write session transcript,
then independently compared SHA384 against retained `_sqlx_migrations.checksum`.
`restoration-checksum.json` proves equality; restored110000.sql freezes the bytes.
No checksum metadata was rewritten; no retained DB was reset/dropped.120000 is the
separate additive correction. It is present in fresh test-owned53-migration DBs
used by passing final tests; it has NOT been applied to retained52-migration DB.

First draft run: focused.log27PASS/2FAIL: ordinary/scheduled owner cascade exposes
NO ACTION ordering before other queued cascades. r2:22PASS/7FAIL:6 unrelated shared
checksum mismatches plus extra direct run deletion blocked by inherited
background_tasks_workflow_execution_fk. r3/r4 repeated the old direct-run control
because transcript extraction failed before writing; no additional DB migration
change. Restored before final focused tests. Direct run deletion would require a
separate existing-job lifecycle decision; the original required company-owner
cascade is tested, and no background-task lifecycle widening was made.

## Remaining work and resources

Before acceptance: apply120000 to retained fixture and inspect all53 schema/migration
checks; locked offline all-target compilation; SQLx prepare/check; affected action,
budget and control regressions. Independent actual-code F1 review/root acceptance
remain pending. A successor should also explicitly exercise attempted individual
job/execution/attempt deletion with a live run to challenge coordinated cascade
entry points; direct populated fact/audit deletion is already covered. Strict
Clippy and broader original04.7 matrices remain pending. No new protocol race,
raw-SQL refusal/substitution/deadline matrices were attempted; V1 is unchanged.

Verified retained PG18 system7691265791172745923, data/private/tmp/workflow-admission-pg-e3aa,
workflow_admission/mac03,55439/socket/private/tmp/max_connections200. Final retained
migrations52/max20261001110000; cleanSTOPPED. pg-final-identity.log/pg-stop.log/
pg-final-control.log preserve identity/state. Own fixtures automatically dropped.
No stage/commit/deploy/reset/drop/bound raise; no active command sessions or agents.

Own UUID01a0f6bd-0a0d-7242-a895-e22e6a86415c. Startup33503/25840012.97%; milestones
10159639.32%,10739641.56%,11216043.41%,11713045.33%,12121946.91%; final substantive
12395547.97%@2026-10-01T09:25:40.329Z. Sources token_usage_record.usage /
task_started.model_context_window. Fresh checkpoint sample follows in HANDOFF.
