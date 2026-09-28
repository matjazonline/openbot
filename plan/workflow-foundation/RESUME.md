# Workflow foundation resume

## VERIFIED — whole03.2;03.3 expanding

2026-09-28 12:56Z: root accepted03.2 Execution and commits item1. ID-only job codec,
run-first atomic input freezing, immutable ordered execution facts and retries are verified.
BRIEF-03.2.md contains implementation/review/correction evidence and exact logs.
1992 full database tests PASS/22 existing ignored/0failed at stock2MiB; SQLx prepare,
locked offline alltarget check/Clippy, migrations, fmt/diff and graft build PASS.
03.3 bounded pure transactional batches is now assigned for expansion reconciliation.
No03.3 source edits yet. Earlier03.1 acceptance below remains valid.

## Governing decisions

- No backward compatibility, compatibility shims or old business-data upgrades required.
- A schedule is fixed to its creation channel; reassignment is rejected atomically across
  application/API/persistence/DB. Creation-time selection and legitimate edits remain.
- Preserve ALL staged/unstaged/untracked work. No staging/commit/reset/deploy authorized.
- Applied migrations through20260928130000 are immutable; additive changes only.
- Reuse background_tasks/task_attempts as sole job/attempt owners; no second queue.

## Accepted scope and evidence

Prior01/02 and03.1 codec/definitions/binding lifecycle/run schema/selection reader remain accepted.
This run completed and independently reviewed:
- A2 fixed-channel schedule correction, additive102000.
- B insertion-only scoped legacy provenance for historical handoff events, additive103000,
  with deletion serialization and retained immutable history.
- C/D exact NULL-aware job/run association, immutable run/execution identities, additive104000;
  reviewed enablement105000 removed workflow_disabled/channelNOTNULL while retaining required
  legacy channels, exact workflow scope and legacy query/control/recovery isolation.
- E atomic admission writer: immutable snapshot, command/source deduplication, real competing
  admissions, first execution/job, bounded committed history, final/deferred rollback.
- F current actor/channel/source/saved-resource authority and observed revocation races,
  invalid/foreign source matrices, deactivation replay and unsupported child-action failure.

ADMISSION-03.1.md:148 contains the final eight-criterion mapping, exact commands/logs and combined
independent actual-code/evidence PASS. JOB-SLICE-03.1.md:813/878/953 holds A2/B/C-D checkpoints.
Final18admission tests PASS;1981fullDBPASS/22existingignored/0fail at stock2MiB; migrations,
locked offline all-target check/Clippy, SQLx prepare, fmt/diff and graft build PASS.
Logs /private/tmp/workflow-admit-f-*.log. Earlier failed fixture/build/environment runs are
recorded and superseded by corrected passes; no failures waived.

## Resources and identities

TaskPG RUNNING for active03.3 worker. Retained task-only
/private/tmp/workflow-admission-pg-e3aa port55439 DBworkflow_admission socket/private/tmp,
max_connections200 log/private/tmp/workflow-admission-postgres.log. No existing database changed.
No running build/test/prepare processes. Future full tests must explicitly set TEST_DATABASE_URL
and DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission when reusing this task DB.
SQLx prepare must remain sequential with offline builds.

Current root01a0e7ff-1731-7f72-8389-cd0f74926fed Astra/low verified; latest57764/258400
=22.35%12:55:44Z token_usage_record.usage/task_started.model_context_window.
Active implementer/root/activation_gates UUID01a0e810-4390-7870-8bae-ce6d69530aac
Astra/medium parent-verified57853=22.39%12:55:37Z; nested reviewer
/root/activation_gates/combined_review UUID01a0e810-a6dd-7eb0-b913-6db615622a85
Astra/medium49725=19.24%12:54:46Z token_count.info sources, currently idle.
Prior execution_activation subtree retired; do not reuse. Root owns PROGRESS/RESUME,
implementer owns BRIEF evidence and taskPG. No commit/stage/reset/deploy authorized.
All current edits preserved, including externally updated website work.
