# 04 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Order

[01](01-unified-addresses.md), then [02](02-direct-threads-and-bindings.md), then
[03](03-agent-default-binding-and-relay.md). Until this plan lands, agents are reached through the
desks they staff; no per-agent desk is provisioned in the meantime.

## Batch 04A Documentation

`docs/system_addresses.md`, `docs/transport_architecture.md`,
`docs/inter_channel_agent_communication.md`, `docs/agent-authoring.md`, `docs/custom_tools.md` and
`docs/departments.md` describe principal addresses, direct threads and the agent binding. Name
only behavior the code has.

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
| 01 focused checks (namespace, resolution) | Pending |
| 02 focused checks (direct threads, bindings, competing) | Pending |
| 03 focused checks (create, delete, relay identity) | Pending |
| Formatting, offline compilation, clippy and reachability | Pending |
| Fresh migrations and SQLx metadata | Pending |
| Database-backed full suite and network isolation | Pending |
| Stack and lease budgets | Pending |
| Docs, startup plan paths and graph refresh | Pending |

Do not declare the implementation complete while a required check is unavailable.
