# 04 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Order

This plan runs after [04-root-unit](../04-root-unit/README.md) and after the memory service
integration is on `main`. [01](01-unit-and-principal-namespaces.md) then
[02](02-scope-resolution-and-tools.md) then
[03](03-knowledge-scopes.md); 03 depends on 02's snapshot only and may run before 01 if the memory
integration is late. Update `plan/memory-service/05-openbot-integration.md`
batch 05A item 2 to point here instead of describing company, agent and user scopes.

## Batch 03A Documentation

`docs/departments.md` gains a memory and knowledge section; the memory service's OpenBot integration
notes and
`docs/agent-authoring.md` describe the scope vocabulary. Name only behavior the code has.

## Batch 03B Final gate, once after integration

1. Finish schema edits in the init migration. Use fresh disposable databases and regenerate `.sqlx`
   metadata with `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets`.
2. Run `scripts/check.sh` with `DATABASE_URL` set to a disposable database; record every gate. The
   memory service's own suite under `services/memory` is unaffected and need not rerun unless the
   contract fake changed.
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
| 01 focused checks (shape, lifecycle) | Pending |
| 02 focused checks (resolver, admission, tools) | Pending |
| 03 focused checks (schema, search, upload) | Pending |
| Formatting, offline compilation, clippy and reachability | Pending |
| Fresh migrations and SQLx metadata | Pending |
| Database-backed full suite and network isolation | Pending |
| Stack and lease budgets | Pending |
| Docs, startup plan paths and graph refresh | Pending |

Do not declare the implementation complete while a required check is unavailable.
