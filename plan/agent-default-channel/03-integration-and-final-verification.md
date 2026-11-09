# 03 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Batch 03A Documentation

1. Update the documentation that describes agent creation or agent addresses: `docs/custom_tools.md`
   (Agent Builder section), `docs/agent-authoring.md`, `docs/agent-builder-context.md` and
   `docs/inter_channel_agent_communication.md`. Each states that a new company agent gets a team
   channel at its slug, answered autonomously by the agent, and that delete removes it while the
   agent has no history. Name only behavior and configuration the code actually has.
2. Record the D4.7 reversal and decisions 1–7 from the [README](README.md) next to the archived
   gate (`plan/.archived/workflow-foundation/phases/p08/docs/TASK.md`), as a pointer to this plan.

Documentation-only checks: links resolve, and quoted file:line anchors match the final code.

## Batch 03B Final gate, once after integration

1. Finish schema edits in the init migration. Use fresh disposable databases and regenerate `.sqlx`
   metadata with `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets`. Do not reset a shared
   developer database.
2. Run `scripts/check.sh` with `DATABASE_URL` explicitly set to a disposable database. It creates
   its own test database. Record every gate's result rather than stopping at the first failure.
3. Compare the script with `.github/workflows/ci.yml` and run any required gate the script omits,
   such as `scripts/test-network-isolation.sh scripts/lease-budget.sh --offline`. Record missing
   Docker, browser or model-artifact prerequisites as unavailable, never as passed.
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
| 01 focused checks (create, round trip, refusals, competing creates) | Pending |
| 02 focused checks (delete, refusals, competing delete) | Pending |
| Formatting, offline compilation, clippy and reachability | Pending |
| Fresh migrations and SQLx metadata | Pending |
| Database-backed full suite and network isolation | Pending |
| Stack and lease budgets | Pending |
| Docs, startup plan paths and graph refresh | Pending |

Do not declare the implementation complete while a required check is unavailable.
