# 05 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Order and dependencies

1. [01](01-departments-and-membership.md) first; it is the contract the others build on.
2. [02](02-desks-ownership-and-access.md) 02A and 02B next; 02C (the word desk) is independent
   and can run in parallel with everything else, in a separate commit so the rename does not hide
   behavior changes in review.
3. [03](03-leads-and-coordination.md) after 02B, since dispatch tests depend on derived grants:
   automatic agent desks (03A), department desks (03B), then placement/directory/coordination
   (03C). Wire the placement calls into the shared agent-creation transaction before accepting
   the phase.
4. [04](04-ui-and-docs.md) last, once the use cases are stable.

This plan calls `assistant_channel_on` with command keys `agent:{agent_id}` and `department:{id}`;
update the `workflow_provisioning_commands` table comment (`init_schema.sql:5594`) to list both.
Each company agent gets its default desk here. The later
[03-principal-addresses](../03-principal-addresses/README.md) plan replaces automatic agent desks
with direct principal addresses after a database reset, while department and other shared desks
remain desks. No old rows or addresses need conversion.

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
| 01 focused checks (schema guards, writes, competing writers) | Accepted in p01; B1-R2 artifact `ccc4ee26…3880` |
| 02 focused checks (ownership, members-only, relay, desk wording) | Accepted in p02; B1-R2 `cbca0a00…438f`, B2-R1 `6cd8b092…e5bf` |
| 03 focused checks (automatic agent desks, competing creates, department desk, directory/context) | Accepted in p03; B1-R2 `a75c40e7…5798`, B2-R2 `31d16940…39ea`, B3-R2 `0db98506…dbf1`. Dispatch round-trip/loop checks deferred by user decision C004 r1; they are not implemented or claimed as passing. |
| 04 rendering and route tests, documentation checks | Accepted in p04; B1-R2 `9cad3b57…3848`, B2-R2 `a97fdbeb…58ba`, B3-R2 `30567a29…c675` |
| Formatting, offline compilation, clippy and reachability | PASS in p05 resumed gate |
| Fresh migrations and SQLx metadata | PASS; fresh disposable DB preparation left `.sqlx` unchanged; resumed migrate/prepare check passed |
| Database-backed full suite and network isolation | Network guard PASS; library 4,142 passed, 2 unrelated failures, 24 ignored. Binary target 5/5 PASS separately. See p05 TASK and retained source-identity evidence. |
| Stack and lease budgets | Stock run at 2,048 KiB completed: 4,143 passed, 1 unrelated scanner failure, 24 ignored; no overflow. Lease budget PASS at unchanged 50% WORK threshold. |
| Startup plan paths and graph refresh | PASS: CI integration-docs check and `graft build` |
| CI-only static/browser protocols and plugin packaging | All 28 checks have successful evidence, retaining initial browser-prerequisite failures. Reproducible plugin package and locked production install PASS with Node 24.21.0/npm 11.19.0. |
| Query statistics activation and privacy | PASS on a p05-owned PostgreSQL 18.6 cluster after fresh app migrations: required settings, zero parameter-log limits, numeric queryid and normalized query text asserted. Shared server unchanged. |
| Docker production image, Doop deployment and live memory-service CI jobs | PASS: actual production image guards/retrieval/RSS; pinned managed Doop service (2,711 full-run passes plus all 17 previously Darwin-skipped clipboard cases passing on isolated Linux), current private image and all four acceptance profiles (104 checks); six real memory-service tests with pinned model/pgvector. Failed attempts and fixture correction retained in p05 R3 evidence. |

Do not declare the implementation complete while a required check is unavailable.

The authoritative execution record is [p05 TASK](phases/p05/docs/TASK.md), with exact prerequisite
artifact identities in `phases/p05/docs/logs/B1-prerequisite-identities.json`. The required affected integration gates now have successful execution evidence. Independent
review and phase acceptance are recorded in p05 TASK/REVIEW. The full/stock commands still retain
their confirmed unrelated failures; this completion record does not claim a wholly green suite.
R4 completes the missing Linux clipboard coverage by applying the existing synthetic rendering
fixture adapter to its HTML harness; all 17 original assertions remain unchanged. The ignored
Doop helper checks and closes each consumed model after its editor step commits, retaining the
exact exchange/secret checks and existing timeouts. The final real Doop run used the updated
package/image and immutable current helper on an owned 8-CPU Docker fixture. Exact identities,
failed attempts, resource isolation and cleanup are retained in the phase records.

Raw `.txt` and `.diff` evidence under p05 `docs/logs/` is archived as binary data by the
phase-local `.gitattributes`, preserving pinned bytes and line endings. Source, Markdown, Python
and JSON retain normal whitespace checks. Final staged whitespace and evidence retention are
checked after staging the complete correction.
