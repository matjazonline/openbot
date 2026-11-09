# 04 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Order

[01](01-unified-addresses.md), then [02](02-direct-threads-and-bindings.md), then
[03](03-agent-default-binding-and-relay.md). Until this plan lands, company agents are reached
through the automatic default desks provisioned by 01-departments, plus any other desks they
staff. Reset the database for the new address and binding schema, then remove automatic
agent-desk provisioning; shared department desks remain. Phase 01 disables the old automatic-desk
call before registering principal addresses at the same slugs, and phase 03 supplies their workflow
bindings. Deploy only the integrated plan. Do not backfill principal addresses from old desk rows
or keep both default-address paths.

## Batch 04A Documentation

`docs/system_addresses.md`, `docs/transport_architecture.md`,
`docs/inter_channel_agent_communication.md`, `docs/agent-authoring.md`, `docs/custom_tools.md` and
`docs/departments.md` describe principal addresses, direct threads and the agent binding. Name
only behavior the code has. Explain the independent controls for public desks and public direct
agent addresses. Record external-mail prerequisites (company-domain MX/inbound routing, selected
SMTP or provider-webhook setup, and outbound sender-domain setup) and distinguish them from the
per-entry-point access setting. Existing routed domains need no per-agent DNS or mailbox setup.

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
| 01 focused checks (namespace, resolution, external SMTP/webhook ingress) | Pending |
| 02 focused checks (direct threads, bindings, public settings, public desk/private agent, competing) | Pending |
| 03 focused checks (create, delete, relay identity) | Pending |
| Formatting, offline compilation, clippy and reachability | Pending |
| Fresh migrations and SQLx metadata | Pending |
| Database-backed full suite and network isolation | Pending |
| Stack and lease budgets | Pending |
| Docs, startup plan paths and graph refresh | Pending |

Do not declare the implementation complete while a required check is unavailable.
