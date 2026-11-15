# 04 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Order

[01](01-sponsor-role-and-escalation-default.md) is delivered on its own, after
[02-workflow-roles](../02-workflow-roles/README.md): it runs batches 04A and 04B below for itself
and is complete without 02 and 03 of this plan. [02](02-unit-lifecycle.md) then
[03](03-goal-units-and-tracking.md) follow after
[07-unit-resources](../07-unit-resources/README.md), and 03 waits for `channel.call`. Batches 04A
and 04B run again once after 03.

Before starting each part, recheck its file anchors against `main` and settle the open proposals
in the [README](README.md) that the part depends on: 1 for part 01; 2 and 3 for part 02; 4 and 5
for part 03. Record each answer in the README's decisions.

## Batch 04A Documentation

- After 01: `docs/departments.md` gains sponsors and the escalation default;
  `docs/workflow-language-v1.md` gains `sponsor` in "Roles".
- After 03: `docs/departments.md` gains "Archive" and "Goals", with the recommended goals office
  shape and when a piece of work deserves a department and when a goal;
  `docs/workflow-operations.md` describes goal budgets beside unit budgets and the goals page;
  `docs/custom_tools.md` and `docs/agent-builder-context.md` describe `goal.status` and the kind
  argument on department creation.

Name only behavior the code has.

## Batch 04B Final gate, once after each delivery

1. Finish schema edits in the init migration. Use fresh disposable databases and regenerate `.sqlx`
   metadata with `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets`.
2. Run `scripts/check.sh` with `DATABASE_URL` set to a disposable database; record every gate.
3. Compare with `.github/workflows/ci.yml` and run any gate the script omits. Record unavailable
   prerequisites as unavailable, never as passed.
4. Evidence must include formatting, patch whitespace, locked offline all-target compilation,
   clippy, fresh migrations, SQLx metadata, source-module reachability, the database-backed suite,
   network isolation, stack budget and lease budget. Keep existing thresholds; follow AGENTS.md if
   one must move.
5. Refresh the graph with `graft build`.

## Completion record

| Evidence | Part 01 | Parts 02 and 03 |
| --- | --- | --- |
| Open proposals settled and recorded | Pending | Pending |
| 01 focused checks (sponsor, resolution, admission, escalation default, competing) | Pending | n/a |
| 02 focused checks (archive, refusals, runs, scopes, authority, competing) | n/a | Pending |
| 03 focused checks (unit, trail, budget, rollup, authorization, competing) | n/a | Pending |
| Formatting, offline compilation, clippy and reachability | Pending | Pending |
| Fresh migrations and SQLx metadata | Pending | Pending |
| Database-backed full suite and network isolation | Pending | Pending |
| Stack and lease budgets | Pending | Pending |
| Docs, startup plan paths and graph refresh | Pending | Pending |

Do not declare a delivery complete while a required check is unavailable.
