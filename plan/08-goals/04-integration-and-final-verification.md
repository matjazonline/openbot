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
| Open proposals settled and recorded | In p01's records (`phases/p01/docs/`) | Settled: 2 and 3 on 2026-11-21, 4 and 5 on 2026-11-24 (README) |
| 01 focused checks (sponsor, resolution, admission, escalation default, competing) | In p01's records | n/a |
| 02 focused checks (archive, refusals, runs, scopes, authority, competing) | n/a | PASS in p02 (`fe7791d3`, `phases/p02/docs/`) |
| 03 focused checks (unit, trail, budget, rollup, authorization, competing) | n/a | PASS in p03 B1 and B2 (`481b14cd`, `f1eb82dc`); `goal.status` through the agent runtime PASS in p04 |
| Formatting, offline compilation, clippy and reachability | In p01's records | Formatting, patch whitespace, offline compilation and reachability PASS. Clippy FAIL on only the 9 `redundant_field_names` lints from merge `73afd5a7`, which predate this plan |
| Fresh migrations and SQLx metadata | In p01's records | PASS |
| Database-backed full suite and network isolation | In p01's records | 5195 passed, 1 pre-existing failure (doop prepared version: the test expects 7, the code is 9 since `358e662a`), run under macOS `sandbox-exec`. The Linux network-isolation scope is UNAVAILABLE locally (CI only) |
| Stack and lease budgets | In p01's records | Stack: the same single pre-existing failure, no overflow. Lease PASS |
| Docs, startup plan paths and graph refresh | In p01's records | 04A docs PASS; plan paths in `NO_BACKWARD_COMPATIBILITY_NOTES`; `graft build` PASS |

Parts 02 and 03, final gate (p04, 2026-11-24, `phases/p04/docs/TASK.md`): unavailable here are the Node
24.21.0 plugin package, the three Docker images, managed Doop (Chrome), the attribution contract check
(`jsonschema`) and the Linux network-isolation scope. Attribution browser integration fails as it did before
this plan. These are environment gaps, not defects found in this plan. The delivery is accepted in phase and
is not declared complete until they run.

Do not declare a delivery complete while a required check is unavailable.
