# 04 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Order

[01](01-role-references-and-publication.md), then [02](02-admission-resolution-and-snapshots.md),
then [03](03-reviewers-and-ui.md). The standard operating procedures plan's role contract
(`plan/standard-operating-procedures/01-shared-contracts-and-workflow-integration.md`, batch A
item 3) is satisfied by this plan's `DepartmentRole`; update that plan to reference it instead of
defining a second role key.

## Batch 04A Documentation

Update `docs/workflow-language-v1.md`, `docs/workflow-operations.md` (admission refusals) and
`docs/departments.md` (roles section). Name only behavior the code has.

## Batch 04B Final gate, once after integration

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
| 01 focused checks (source, publication, binding) | Pending |
| 02 focused checks (resolver, admission, competing) | Pending |
| 03 focused checks (reviewers, role reviewers, rendering) | Pending |
| Formatting, offline compilation, clippy and reachability | Pending |
| Fresh migrations and SQLx metadata | Pending |
| Database-backed full suite and network isolation | Pending |
| Stack and lease budgets | Pending |
| Docs, startup plan paths and graph refresh | Pending |

Do not declare the implementation complete while a required check is unavailable.
