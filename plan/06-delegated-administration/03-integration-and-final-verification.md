# 03 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Order

[01](01-predicate-and-owners.md) then [02](02-routes-ui-and-audit.md). This plan replaces the
authorization owner every management route exercises; run the authorization suites after each
batch, not only at the gate.

## Batch 03A Documentation

`docs/departments.md` gains an administration section; `docs/authentication-operations.md` and
`docs/workflow-operations.md` replace "owner or admin" with the unit rule where it applies. Name
only behavior the code has.

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
| 01 focused checks (predicate, owners, competing) | Passed; batches accepted, records in `phases/p01/docs` |
| 02 route tests and audit checks | Passed; batches accepted, records in `phases/p02/docs` |
| Authorization suites after each batch | Passed after each batch; `phases/p01/docs` and `phases/p02/docs` TASK |
| Formatting, offline compilation, clippy and reachability | Passed (`tests/source_modules.rs` run separately, see p03 TASK) |
| Fresh migrations and SQLx metadata | Passed; `.sqlx` unchanged on a fresh UTF-8 schema |
| Database-backed full suite and network isolation | Run isolated; 4883 passed, 1 failed: the deferred Doop assertion that also fails on main. Linux isolation scope runs in CI only |
| Stack and lease budgets | Lease passed; stack: no overflow at 2048 KiB, red only on the same Doop assertion |
| Docs, startup plan paths and graph refresh | Done |

Unavailable locally, not passed: Docker image gates and Doop (deferred).
Failed in the `scripts/check.sh` run and passed only on rerun: the browser protocols (one test
hung; cause not established), the transport boundary and attribution service gates (missing
`rg` and Python 3.11), the plugin package (rerun under the pinned Node 24.21.0), and the memory
gates (rerun once the ONNX Runtime 1.24.4 pin moved from the deleted GitHub repository to NuGet).
`phases/p03/docs/TASK.md` records every gate.

Do not declare the implementation complete while a required check is unavailable.
