# 03 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Order

[01](01-ownership-and-selection.md) then [02](02-models-budgets-and-usage.md). When
`plan/tenant-owned-execution-backends.md` or the Cloudroom machines are implemented, they take the
`unit_id` and selection check from 01 rather than a company scope.

## Batch 03A Documentation

`docs/departments.md` gains a resources section; `docs/custom_tools.md` and
`docs/agent-authoring.md`
describe unit-scoped selection and the default model cascade; `docs/workflow-operations.md`
describes unit budgets beside the root budget. Name only behavior the code has.

## Batch 03B Final gate, once after integration

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

| Evidence | Status |
| --- | --- |
| 01 focused checks (schema, selection, competing) | Passed (p01 B1–B3, accepted; phases/p01/docs) |
| 02 focused checks (model default, budget, competing) | Passed (p02 B1–B3, accepted; phases/p02/docs) |
| Formatting, offline compilation, clippy and reachability | Passed (final check.sh; source_modules 8 passed in the separate run) |
| Fresh migrations and SQLx metadata | Passed (fresh-schema `cargo sqlx prepare`, `.sqlx` unchanged; check.sh migrations and metadata) |
| Database-backed full suite and network isolation | Red only on the deferred Doop assertion (4948 passed, 1 failed); main.rs and reachability passed separately under the isolation guard; Linux network-isolation scope not exercised on macOS |
| Stack and lease budgets | Lease passed; stack red only on the same deferred assertion, no overflow, threshold unchanged |
| Docs, startup plan paths and graph refresh | Passed (03A accepted; startup list; `trail build`) |

Other check.sh gates are recorded in phases/p03/docs/TASK.md: Docker image gates unavailable (no
daemon); managed Doop deferred and red (05SC-C001/04RU-C005); the plugin package, Doop runner and
attribution gates red in the full run and passing on rerun; the CSS gate fixed in 03B. Status:
**accepted in phase, not declared complete** while those checks stay unavailable or red.

Do not declare the implementation complete while a required check is unavailable.
