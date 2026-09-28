# Workflow foundation resume

## VERIFIED — whole03.1;03.2 not started

2026-09-28 11:18Z: root accepted all selected03.1 Admission and independent contexts.
This run honored the prior instruction not to start03.2. No outstanding03.1 findings or gates.
Next queued work is03.2 Execution and commits item1 in03-durable-runtime-and-persistence.md;
continue only when the next scope is authorized. Do not repeat accepted discovery/tests without
new changes or unresolved concerns.

## Governing decisions

- No backward compatibility, compatibility shims or old business-data upgrades required.
- A schedule is fixed to its creation channel; reassignment is rejected atomically across
  application/API/persistence/DB. Creation-time selection and legitimate edits remain.
- Preserve ALL staged/unstaged/untracked work. No staging/commit/reset/deploy authorized.
- Applied migrations through20260928105000 are immutable; additive changes only.
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

TaskPG STOPPED with pg_ctl fast/wait; status confirmed no server running. Retained task-only
/private/tmp/workflow-admission-pg-e3aa port55439 DBworkflow_admission socket/private/tmp,
max_connections200 log/private/tmp/workflow-admission-postgres.log. No existing database changed.
No running build/test/prepare processes. Future full tests must explicitly set TEST_DATABASE_URL
and DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission when reusing this task DB.
SQLx prepare must remain sequential with offline builds.

Root01a0e76b-6497-7e12-a8f6-4f6db817bbef Astra/low latest88930/258400=34.42%
11:17:38Z token_usage_record.usage/task_started.model_context_window. This is completion of selected
scope, not a context/capacity stop. All implementation/review subtrees completed and quiescent;
do not reuse retired workers. Final implementer01a0e7ae-49eb-7c82-8178-6a35bebc362e
102126=39.52%11:17:31Z; reviewer01a0e7ae-ae7b-7673-891f-af13d150a7a1
81820=31.66%11:16:34Z token_count.info sources. Models Astra/medium. Prior identities/context
samples and evidence remain in PROGRESS/JOB-SLICE/ADMISSION. Fresh future root must measure context
and verify its implementer/nested reviewer capacity before edits.
