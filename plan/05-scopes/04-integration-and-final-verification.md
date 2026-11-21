# 04 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Order

This plan runs after [04-root-unit](../04-root-unit/README.md). The memory service integration it
builds on has been on `main` since 2026-11-10. [01](01-unit-and-principal-namespaces.md) then
[02](02-scope-resolution-and-tools.md) then
[03](03-knowledge-scopes.md); 03 depends on 02's snapshot only. The archived memory-service plan
(`plan/.archive/memory-service/05-openbot-integration.md`, batch 05A item 2) still describes
company, agent and user scopes. It stays as recorded; the new scope vocabulary is documented in
the files named in 03A.

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
5. Refresh the graph with `trail build`.

## Completion record

| Evidence | Status |
| --- | --- |
| 01 focused checks (shape, lifecycle) | PASS; [accepted phase01 records](phases/p01/docs/TASK.md), commit `e6eb5bc8`. |
| 02 focused checks (resolver, admission, tools) | PASS; [accepted phase02 records](phases/p02/docs/TASK.md), commit `2edd0a73`; service suite 178 PASS/2 ignored and actual subject/service proof retained. |
| 03 focused checks (schema, search, upload) | All 26 scope regressions PASS in the current 2 MiB full run; [03 evidence](phases/p03/docs/logs/B1-R2-final.evidence.json). |
| Formatting, offline compilation, clippy and reachability | Current source PASS; binary 6 and source-module 8 PASS, including separate2MiB execution. |
| Fresh migrations and SQLx metadata | PASS; fresh owned schemas, metadata regeneration and current init-migration checksum match retained. |
| Database-backed full suite and network isolation | Current full library: 4,706 passed, 2 failed, 27 ignored and 85 Doop filtered. Both exact failures PASS unchanged isolated rerun; related competing role/native families: 8 passed and 3 existing provider tests ignored. Linux network scope: 8 categories PASS. |
| Stack and lease budgets | Original 2 MiB stack retained for full run and exact/family reruns; no overflow. Lease: 139 tests and original 50% work budget PASS. Full stock command retains its two non-stack failures. |
| Production and live integration | Knowledge image/retrieval/tamper PASS; memory backup: 1 PASS, standalone: 13 steps PASS, live OpenBot-memory: 7 tests PASS and non-root/read-only/RSS production image PASS. |
| Native model preparation | Fresh macOS runtime download FAIL HTTP 404 on a main-identical pin/script. Available cached model/runtime pass all pinned size/SHA256 checks, independently verified; runtime/image checks PASS. |
| Docs, startup plan paths and graph refresh | Final phase04 focused checks and independent acceptance are recorded in [04 records](phases/p04/docs/TASK.md); all five existing05startup paths remain registered. |
| Independent source review and phase03 checkpoint | Astra 6.2/high `05SC-P03-B1-R2-N2` PASS; accepted source `62d667d9e56395de26652accbbab2ee1eb9f854969e308607cae2aa003cb5907`; commit `3d7148b1b496e491e139fc90e0534ef8d7d296ab` confirmed on main. |
| Doop | User-deferred; three dedicated gates and 85 root names remain separate, with assertions and declarations unchanged. |

The full gate ran before the requested phase03 publication checkpoint. The two current full-run
failures were a role-writer scheduling expectation and a native cutpoint readiness timeout; their
precise timing causes remain unproven. Their unchanged current source exact and internally
competing family reruns passed at the same limits. Independent review found no remaining affected
verification gap. The parent runner exited 1 because the unchanged native download returned 404;
verified cached prerequisites remained available and were actually exercised. Neither the full
run nor whole CI is declared green.

Final documentation changes reuse that identical runtime proof; repository AGENTS does not require
Rust or database suites for documentation-only edits. The service suite reuses the accepted phase02
execution because service behavior and its contract fake have not changed afterward. Independent
plugin/attribution evidence reuses accepted Plan04 source, with provenance retained in phase03 logs.
Final docs review/publication are recorded under phase04; stop before Plan06 after the authorized push.

Do not declare the implementation complete while a required check is unavailable. Retain original
failures and the difference between an unavailable fresh download and available verified artifacts.
