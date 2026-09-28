# Workflow foundation resume

## VERIFIED through03.8;03.9 blocked before expansion

2026-09-28 15:39Z: stopped on host agent-thread capacity, NOT a context threshold.
This run completed and independently verified03.3–03.8: bounded pure batches,
fenced I/O leases/actual-future cancellation, atomic fenced completion/replay,
event/timer parking/resumption, run-first parent wakeups, and durable DB polling.
Earlier01/02/03.1/03.2 acceptance remains preserved. BRIEF-03.3 through03.8 hold
original criteria, review findings/resolutions and exact command/log evidence.

Latest current-tree gate:13focused polling and2075fullDBPASS/22existingignored/0fail
at stock2MiB; SQLx prepare, locked offlinealltarget check/Clippy, fmt/staged+unstaged
diff/graftPASS. Isolated tests exercise fresh migrations; prior migration gates remain
valid. Independent actual-code/corrections/combined review PASS. Earlier failed
fixtures were corrected and superseded; no failed gate waived.

Next: expand/reconcile03.9 Failure/recovery and full phase03 acceptance from original
03-durable-runtime-and-persistence.md + EXPANSION.md before source edits. No03.9 brief
or source edits yet. Fresh /root/workflow_recovery could not create required nested
reviewer (`agent thread limit reached`); no handle/no close control. Skill requires
stop instead of self-review or topology substitution. Resume in fresh session with
new measured root and implementation/review pair. All current subtrees retired;
do not reuse their handles. Phase04 and beyond remain pending.

## Governing decisions

- No backward compatibility, shims or old business-data upgrades required.
- Schedules fixed to creation channel; reassignment rejected atomically.
- Preserve ALL staged/unstaged/untracked work. External staging and website changes
  occurred; inspect actual source and staged+unstaged diff, not unstaged diff only.
  No stage/commit/reset/deploy was performed by this root/subtrees.
- Applied migrations through20260928150500 IMMUTABLE; additive changes only.
- background_tasks/task_attempts remain sole job/attempt owners; shared completion
  writer owns pure/fenced/wait progression. No competing queue/result ledger.
- A supervised result/fence is not effect permission.03.5 rechecks live ownership
  with deferred commit-time expiry; phase04 owns effect reconciliation.
-03.7 parent wakeups are immutable child-owned terminal-transition facts. Phase07
  owns parent-call creation/wait settlement and result mapping;03.8 does not fake it.
-03.8 is a reusable bounded worker with injected handlers. Production effect/agent
  handlers and startup replacement stay04/06/08.03.9 still owns classified recovery,
  safe operator retry/cancel, budgets, sustained tenant fairness/global concurrency.

## Resources and identities

TaskPG STOPPED by root with fast/wait; pg_ctl status confirmed no server running.
Retained /private/tmp/workflow-admission-pg-e3aa port55439 DBworkflow_admission,
socket/private/tmp,max_connections200; log/private/tmp/workflow-admission-postgres.log.
No live build/test/prepare jobs. Reuse only task-owned cluster, no database reset.
Future DATABASE_URL and TEST_DATABASE_URL must both explicitly be
postgres://mac03@127.0.0.1:55439/workflow_admission. SQLx prepare sequential with builds.
The cluster was externally stopped/restarted during03.5; no data reset/removal.

Root01a0e81d-a106-7651-8b03-d0c253c67f38 runtime verified Astra/low; latest123293/258400
47.71%15:38:11Z token_usage_record.usage/task_started.model_context_window.
Retired latest verified implementer/root/workflow_polling
UUID01a0e893-c15e-73e0-bba7-ecab22bfa506 Astra/medium parent120231/25840046.53%
15:36:55Z token_count.info; nested reviewer01a0e893-ffed-76f3-9e26-c8d4f11109dc
owner-verified32.80%, completed. BRIEF03.8 holds actual-code and final review evidence.
Capacity-blocked/root/workflow_recovery UUID01a0e8a9-ef5a-7092-a9de-97738d69b06b
Astra/medium parent22977/2584008.89%15:37:55Z token_count.info, completed, no reviewer.
Root owns PROGRESS/RESUME; future worker owns new brief and restarted taskPG.
