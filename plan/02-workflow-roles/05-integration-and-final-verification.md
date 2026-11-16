# 05 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Order

[01](01-role-references-and-publication.md), then [02](02-admission-resolution-and-snapshots.md),
then [03](03-reviewers-and-ui.md), then [04](04-escalation-and-reassignment.md). The standard
operating procedures plan's role contract
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
| 01 focused checks (source, publication, binding) | Accepted in p01 (02WR-P01-B1-A1), artifact `91741e06…473d` |
| 02 focused checks (resolver, admission, competing) | Accepted in p02 (02WR-P02-B1-A1), artifact `be7fd9d4…e6ba` |
| 03 focused checks (reviewers, role reviewers, rendering) | Accepted in p03 (02WR-P03-B1-A1), artifact `5bd5645a…a1db` |
| 04 focused checks (escalation ladder, parked reassignment) | Accepted in p04 (02WR-P04-B1-R2/N1 PASS), artifact `e9ce4b95…2b03`. The `channel.call` trigger is deferred by user decision 02WR-C001; it is not implemented or claimed as passing. |
| Formatting, offline compilation, clippy and reachability | Formatting, patch whitespace, offline compilation and reachability PASS (gate run 3 plus the p05 R3 delta). Clippy FAIL on 7 lints that rustc/clippy 1.99.1 reports in code predating plan02. A diagnostic clippy that allows only those lint names passes over all targets. |
| Fresh migrations and SQLx metadata | PASS: fresh disposable DB; `cargo sqlx prepare -- --all-targets` left `.sqlx` unchanged |
| Database-backed full suite and network isolation | Network guard PASS. Library 4,450 passed, 5 failed, 25 ignored. The 5 failures are unrelated: the inline-JS scan, an mcp cancellation race, 2 auth tests on a C-locale cluster, and `database_snapshot` with `track_utility=on`. See p05 TASK. |
| Stack and lease budgets | Stock 2 MiB run: no overflow, with the same 5 unrelated failures plus 4 doop timeouts under load that pass in isolation at 2 MiB. Lease budget PASS (139) at the unchanged 50% WORK threshold. |
| Docs, startup plan paths and graph refresh | PASS: docs reviewed against code (02WR-P05-R1/R2); plan paths in `NO_BACKWARD_COMPATIBILITY_NOTES` and the CI integration-docs check; `trail build` |
| CI-only gates | Plugin, browser protocols, Doop runner boundaries, workflow sample and integration boundaries, and evidence retention PASS (p05 `logs/gate-ci-extra1.txt`, `logs/gate-ci-boundaries2.txt`). Unavailable here: the Docker knowledge image, the Node 24.21.0 package pin, the memory-integration and doop-integration jobs, and CI's query-statistics reconfiguration. |
| Final review | p05 accepted on 02WR-P05-R3 PASS, artifact `bd5279a5…1da7`; no open findings |

Do not declare the implementation complete while a required check is unavailable.
