# 03 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Order

[01](01-root-department-and-membership.md), then [02](02-access-modes.md). This plan touches
the most authorization code of the four, so run the authorization test suites
(`*_authorization_tests.rs` under `src/adapters/http/routes/`) after each batch, not only at the
gate.

## Batch 03A Documentation

`docs/departments.md`, `docs/transport_architecture.md`, `docs/authentication-operations.md` and
the memory service plan's namespace mapping describe the root unit and derived membership. Name
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
| 01 focused checks (root, membership, competing) | Accepted: [p01](phases/p01/docs/TASK.md) and [main integration](integration/main-60b9a28b/docs/TASK.md) |
| 02 focused checks (modes, grants, measurement) | Accepted: [p02](phases/p02/docs/TASK.md), including real 1,000-member/200-desk delta measurement |
| Authorization suites after each batch | PASS in accepted batches; p03 recheck includes all 21 required HTTP cases |
| Formatting, offline compilation, clippy and reachability | PASS: p03 final formatting, locked offline all-target check, Clippy with warnings denied, binary 5 and reachability 8 tests |
| Fresh migrations and SQLx metadata | PASS: disposable core schemas, all-target SQLx regeneration and unchanged metadata |
| Database-backed full suite and network isolation | Executed: initial 31 fixture failures have passing corrections; current stock run 4,719 PASS/1 Doop timeout/29 ignored; Linux control-plane gate PASS. See p03 for exact dispositions |
| Stack and lease budgets | Stock 2 MiB: no overflow; Doop timeout retained and isolated stock recheck PASS. Lease budget: 139 PASS at unchanged thresholds |
| Docs, startup plan paths and graph refresh | Updated current root/grant/namespace mapping; no new plan path; Trail refresh PASS |

The [phase03 record](phases/p03/docs/TASK.md) retains every failed gate, corrected prerequisite,
successful recheck and final independent disposition. Browser protocols, all three production
images, memory extraction/backup/standalone/live integration and attribution extraction/contracts/
browser integration were exercised. The full-run Doop mock timeout remains explicitly visible;
its isolated pass does not claim a clean full run. Existing private Doop acceptance is deferred
under Plan03 C008/r1 to [fix-doop](../fix-doop.md). Whole CI is not claimed green.

Do not declare the implementation complete while a required check is unavailable.
