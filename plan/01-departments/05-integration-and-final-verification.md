# 05 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Order and dependencies

1. [01](01-departments-and-membership.md) first; it is the contract the others build on.
2. [02](02-desks-ownership-and-access.md) 02A and 02B next; 02C (the word desk) is independent
   and can run in parallel with everything else, in a separate commit so the rename does not hide
   behavior changes in review.
3. [03](03-leads-and-coordination.md) after 02B, since dispatch tests depend on derived grants.
4. [04](04-ui-and-docs.md) last, once the use cases are stable.

This plan calls `assistant_channel_on` with a new command key, `department:{id}`; update the
`workflow_provisioning_commands` table comment (`init_schema.sql:5594`) to list it. Agents get no
desk of their own: that is [03-principal-addresses](../03-principal-addresses/README.md), and the
earlier agent-default-channel plan was removed in its favor.

## Batch 05A Final gate, once after integration

1. Finish schema edits in the init migration. Use fresh disposable databases and regenerate `.sqlx`
   metadata with `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets`. Do not reset a shared
   developer database.
2. Run `scripts/check.sh` with `DATABASE_URL` explicitly set to a disposable database. Record every
   gate's result rather than stopping at the first failure.
3. Compare the script with `.github/workflows/ci.yml` and run any required gate the script omits,
   such as `scripts/test-network-isolation.sh` and `scripts/lease-budget.sh --offline`. Record
   missing Docker, browser or model-artifact prerequisites as unavailable, never as passed.
4. The evidence must include formatting, patch whitespace, locked offline all-target compilation,
   clippy, fresh migrations, the SQLx metadata check, source-module reachability, the
   database-backed all-target suite, network isolation, stack budget and lease budget.
5. Keep the existing stack and lease thresholds. If a bound must change, follow AGENTS.md: record
   why, and keep a CI check that fails before the bound is reached again.
6. After verification fixes, rerun the affected checks, and repeat the full gate when the impact
   warrants it. Refresh the graph with `graft build`.

## Completion record

| Evidence | Status |
| --- | --- |
| 01 focused checks (schema guards, writes, competing writers) | Pending |
| 02 focused checks (ownership, members-only, relay, desk wording) | Pending |
| 03 focused checks (department desk, round trip, loop, directory) | Pending |
| 04 rendering and route tests, documentation checks | Pending |
| Formatting, offline compilation, clippy and reachability | Pending |
| Fresh migrations and SQLx metadata | Pending |
| Database-backed full suite and network isolation | Pending |
| Stack and lease budgets | Pending |
| Startup plan paths and graph refresh | Pending |

Do not declare the implementation complete while a required check is unavailable.
