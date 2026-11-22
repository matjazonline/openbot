# 05 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Order

[01](01-unit-head.md) starts when part 02 of [08-goals](../08-goals/README.md) is committed, and
is complete on its own. [02](02-agent-home.md) needs only
[06](../06-delegated-administration/README.md). [03](03-requests-between-units.md) follows 01 and
02; its `channel.call` step waits for
[agent-workflow-call](../agent-workflow-call/README.md). The batches of
[04](04-authority-corrections.md) are independent of each other and of 03.

Before starting each part, recheck its file anchors and settle the [README](README.md)
proposals it depends on: none for 01; 2 for 02; 1 for 03's `channel.call` step; 3 to 8 for the
matching batch of 04. Record each answer in the README's decisions, and record proposal 1 in
agent-workflow-call as well.

## Batch 05A Documentation

- `docs/departments.md` gains the table from the README as the one statement of who reaches
  what, and each section below it refers to the table and does not restate a rule. It gains
  "Requests between units" and the agent's home unit.
- `docs/agent-authoring.md` and `docs/inter_channel_agent_communication.md` describe asking
  another unit, the refusal codes and the ceiling of a request.
- `docs/workflow-operations.md` describes the ceiling of a request beside the other three
  sources of a run's ceiling.

Name only behavior the code has.

## Batch 05B Final gate, once after each delivery

1. Finish schema edits in the init migration. Use fresh disposable databases and regenerate
   `.sqlx` metadata with `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets`.
2. Run `scripts/check.sh` with `DATABASE_URL` set to a disposable database; record every gate.
3. Compare with `.github/workflows/ci.yml` and run any gate the script omits. Record unavailable
   prerequisites as unavailable, never as passed.
4. Evidence must include formatting, patch whitespace, locked offline all-target compilation,
   clippy, fresh migrations, SQLx metadata, source-module reachability, the database-backed
   suite, network isolation, stack budget and lease budget. Keep existing thresholds; follow
   AGENTS.md if one must move.
5. Refresh the graph with `trail build`.

## Completion record

| Evidence | 01 | 02 | 03 | 04 |
| --- | --- | --- | --- | --- |
| Proposals settled and recorded | n/a | Pending | Pending | Pending |
| Focused checks of the part | Pending | Pending | Pending | Pending |
| Formatting, offline compilation, clippy and reachability | Pending | Pending | Pending | Pending |
| Fresh migrations and SQLx metadata | Pending | Pending | Pending | Pending |
| Database-backed full suite and network isolation | Pending | Pending | Pending | Pending |
| Stack and lease budgets | Pending | Pending | Pending | Pending |
| Docs, startup plan paths and graph refresh | Pending | Pending | Pending | Pending |

Do not declare a delivery complete while a required check is unavailable.
