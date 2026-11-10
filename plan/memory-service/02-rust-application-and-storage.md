# 02 Rust application and storage

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [01](01-service-boundaries-and-contracts.md). The service builds, starts, migrates and
stores explicit memories from its own directory without OpenBot. Retrieval follows in
[03](03-retrieval-and-context.md).

## Batch 02A Build the standalone application

1. Create the Rust/Axum/Tokio/SQLx application with cohesive domain, application and adapter
   ownership. Define ports only where multiple implementations or an actual I/O boundary need
   them. Do not copy the upstream's module tree or introduce empty services for future features.
2. Initialize structured tracing before fallible startup. Implement explicit configuration
   validation, health/readiness, bounded requests and connection pools, cancellation, and graceful
   shutdown. Missing required storage or model capabilities must produce a clear startup/readiness
   failure, rather than silently advertising a reduced retrieval mode.
3. Commit the service lockfile and its own SQLx metadata. Use locked builds and pin git
   dependencies by revision/tag. Produce a non-root image with required runtime assets inside it;
   avoid network downloads or source-tree mounts at startup. Document configuration only as it is
   implemented, including which checks startup actually performs.
4. Add an independent verification entrypoint and CI job. Add a boundary check for parent source,
   path dependencies, assets, configuration and SQL access. Build a copy containing only the
   service directory in CI; a root build is not evidence of service independence.

Focused checks: `memory_startup_` and `memory_boundary_` cover bad configuration, readiness,
migration failure, non-root startup, cancellation, separate database-role denial, and extracted
directory compilation. Run service formatting and offline all-target compilation.

## Batch 02B Store isolated memories and operation state

Use a small relational model with explicit invariants. Exact Rust types and table names belong to
implementation; the required responsibilities are:

| Records | Required responsibility |
| --- | --- |
| Tenants and namespaces | Isolation, active/deleted state and external opaque identity |
| Credential grants | Verifiers, tenant/namespace/operation grants, read/write levels and revocation |
| Sources and sessions | Bounded immutable evidence or references, source access levels, revision and session grouping |
| Observations and source links | Durable content, kind, access level, consumed-level evidence, provenance and revision |
| Searchable observation parts | Bounded text, parent access level, embedding model identity and pgvector values |
| Generation operations | Explicit request, read ceiling, consumed watermark, expiry, allowance and fenced ownership |
| Idempotency and audit records | Stable logical operation results and traceable security/state changes |

1. Use tenant-consistent composite keys/foreign keys for relationships inside the database.
   Credentials, source links and session links cannot borrow another tenant's rows. Keep authority,
   lease state and access levels in constrained columns rather than opaque metadata. Constrain
   levels to 0–9 and preserve the parent observation's label on derived searchable parts.
2. Implement save, fetch, correction and deletion through the owning application use cases.
   Commit observations, source links and their idempotency result together. Concurrent identical
   saves produce one logical observation; conflicting content is not last-writer-wins by accident.
   Freeze content, source revisions, the producing context's authorized ceiling and whether the
   consumed watermark is proven or a ceiling fallback with the save label. Validate that the label
   is no lower than contributing sources; include it in the canonical idempotency request.
   Model-selected labels cannot override the host-derived exact or fallback level in phase 01.
3. Define correction semantics with expected revisions. Invalidate searchable parts and stale
   context references when content changes. Do not lower an existing record's classification or
   deduplicate a higher-level record into a lower-level one. Deletion removes derived vectors
   and source links; it must not leave content retrievable through timeline or context endpoints.
4. Use PostgreSQL full-text indexes and a separate pgvector table for observation parts. Preserve
   canonical memory text independently of embeddings. Index tenant/namespace filters and due job
   claims. Choose embedding width from the pinned model and reject incompatible vectors.
5. Provide isolated database test support. Tests with whole-table assertions, claims or sweeps
   receive their own migrated database. Missing PostgreSQL fails database tests explicitly.
   Regenerate service-owned SQLx metadata after every SQL change.

Focused checks: `memory_storage_` and `memory_idempotency_` exercise fresh migrations, cross-tenant
foreign-key rejection, concurrent identical saves, conflicting keys/levels, concurrent correction,
classification-preserving copies, exact/fallback label persistence, source-label tampering,
delete/write races and vector cleanup against real PostgreSQL. Run focused credential tests,
offline all-target compilation and metadata checking after the batch.

## Implementation verification (P2, 2026-11-08)

Environment, logs and DB note as in [01](01-service-boundaries-and-contracts.md#implementation-verification-p2-2026-11-08).

| Command | Selected | Result |
| --- | --- | --- |
| `cargo fmt --all -- --check` | — | passed |
| `cargo check --locked --offline --all-targets` | — | passed |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | — | passed (`p2-clippy-2.log`) |
| `scripts/boundary-check.sh --probe` | — | passed, including the poisoned parent (`p2-boundary-probe.log`) |
| `scripts/focused.sh memory_startup_ --network-isolated` | 9 | 7 passed; 2 DB-backed (migration without pgvector, owner-role migration) **unavailable** |
| `scripts/focused.sh memory_boundary_ --network-isolated` | 3 | 2 passed (environment probe, source reach); 1 DB-backed (role denial) **unavailable** |
| `python3 -I scripts/prepare-model.py --check-manifest` | — | passed |

Unavailable: `scripts/verify.sh` and `scripts/extracted-build.sh` (they need
`MEMORY_TEST_DATABASE_URL` and sqlx-cli 0.8.6), `cargo sqlx migrate run` / `prepare --check`
against a real server, and the image build and non-root startup check (no Docker on this host).
CI (`.github/workflows/memory-service.yml`) runs all of these.

## Implementation verification (P2 correction round 1, 2026-11-08)

Environment and logs as in [01](01-service-boundaries-and-contracts.md#implementation-verification-p2-correction-round-1-2026-11-08).
This round supersedes the "unavailable" entries above for the DB-backed cases, `verify.sh` and
`extracted-build.sh`.

| Command | Selected | Result |
| --- | --- | --- |
| `scripts/focused.sh memory_startup_ --network-isolated` | 9 | 9 passed, before (`a2-db0-`) and after (`a2-db1-`) the corrections; 2 DB-backed |
| `scripts/focused.sh memory_boundary_ --network-isolated` | 3 | 3 passed, before and after; 1 DB-backed (role denial) |
| `scripts/focused.sh memory_storage_ --network-isolated` | 1 | 1 passed (m4: frozen sources, link label FK, audit request id) |
| `scripts/verify.sh` | 59 | every gate passed (`a2-verify-1.log`): fmt, boundary with probe, fetch, offline check, clippy `-D warnings`, `cargo sqlx migrate run` as the owner role, `cargo sqlx prepare --check -- --all-targets` (34 queries), model manifest, test build, 59 of 59 tests under network isolation (19 DB-backed) |
| `MEMORY_EXTRACT_UNTRACKED=1 scripts/extracted-build.sh` | 59 | every gate passed in the extracted copy (`a2-extracted-1.log`), 59 of 59 tests |
| `scripts/boundary-check.sh --probe` | — | passed after the last script change (`a2-boundary-probe.log`) |
| root `cargo fmt --all -- --check`; root `git diff --check` | — | passed (`a2-root-fmt.log`, `a2-root-diff-check.log`) |
| `git diff --no-index --check` over the 107 untracked service, workflow and plan files (the CRLF licence excluded) | — | clean |

Limits of this evidence:
- Nothing under `services/memory` is committed yet. The extracted run therefore needed the
  untracked opt-in, and `verify.sh`'s "Patch whitespace" step checks no service file; the
  `--no-index` check above covers them.
- To fit the disk, the extracted run shared the service's target directory
  (`CARGO_TARGET_DIR=services/memory/target`). The crate was rebuilt from the copied sources;
  CI uses a fresh checkout and target.
- The cleanup traps dropped every `memverify_*` database and role. Each test process leaves one
  `memtest_*_owner` role, which the next run drops (D2).

Unavailable here:
- the Linux network-isolation path (F3: conntrack rule and self-test);
- the CI verify job on the PostgreSQL 18/pgvector 0.8.7 service container;
- the image build and the non-root, `--network none --read-only` startup checks.

There is no Docker on this host and pushing is not authorized, so the first CI run of
`.github/workflows/memory-service.yml` is still required.

## Implementation verification (P3, batch 02B, 2026-11-08)

Environment as in [01](01-service-boundaries-and-contracts.md#implementation-verification-p2-correction-round-1-2026-11-08)
(the private PostgreSQL 18.6 + pgvector 0.8.7 cluster on 127.0.0.1:55432, sqlx-cli 0.8.6 on
`PATH`, macOS `sandbox-exec` isolation whose self-test passed in every run). Logs are
`tmp/memory-service-run/logs/a2-p3-*.log`. Decisions are in the record's "P3 implementation
notes".

| Command | Selected | Result |
| --- | --- | --- |
| `scripts/focused.sh memory_storage_ --network-isolated` | 7 | 7 passed (`a2-p3-1-`): fetch filtering and provenance, cross-tenant FK rejection, frozen sources, label tampering, sessions, correction/delete part and link cleanup, concurrent corrections (8 claimants, one winner), delete racing correction and copy |
| `scripts/focused.sh memory_idempotency_ --network-isolated` | 2 | 2 passed: 8 concurrent identical saves give one observation set and 7 replays; conflicting key, namespace scope, kind scope, replay after the deadline, expiry with no record left, 7-day window |
| `scripts/focused.sh memory_access_level_ --network-isolated` | 14 | 14 passed: ceiling 6 with exact 4 saves at 4; unknown saves at 6 (`ceiling_fallback` persisted); inherited 6 with a level-0 source saves at 6; exact 0 saves at 0; consumed above ceiling, missing ceiling, a forged lower label, external and service sources above the ceiling are refused; above `write_max` is 403; a replayed label is frozen and a relabel conflicts; copies and corrections never declassify |
| `scripts/focused.sh memory_authority_ --network-isolated` | 23 | 23 passed, including client writes re-checking namespace delete, tenant disable and revoke |
| `scripts/focused.sh memory_contract_ --network-isolated` | 10 | 10 passed (`a2-p3-2-`, after regenerating `openapi.json`; the first run, `a2-p3-1-`, failed only on the stale file): HTTP statuses for 409/410/404/422 over PostgreSQL, 8 unimplemented operations still 501 |
| `scripts/focused.sh memory_mcp_ --network-isolated` | 4 | 4 passed: save/fetch results and denials (403, 422) identical over HTTP and MCP |
| `scripts/focused.sh memory_startup_` / `memory_boundary_` `--network-isolated` | 9 / 3 | 9 / 3 passed |
| `cargo sqlx prepare -- --all-targets`, then `cargo check --locked --offline --all-targets` | — | 61 queries; offline compile from the metadata passed |
| `scripts/verify.sh` | 72 | every gate passed (`a2-p3-verify.log`), including `prepare --check` and 72 of 72 tests under isolation (32 DB-backed) |

Unavailable, as before: the Linux isolation path, CI and the image job.

## Implementation verification (P3 correction round 1, 2026-11-08)

Review `p3-code-r1` fixes. The environment is as above; logs are `tmp/memory-service-run/logs/a2-p3r1-*.log`.

| Command | Selected | Result |
| --- | --- | --- |
| `scripts/focused.sh memory_storage_ --network-isolated` | 10 | 10 passed. New: fetch omits provenance the reader cannot see (F1); carried label, source-retention conflict and reference bound (m1, m3, m4); cross-tenant session, parent-link and part keys (m5). Concurrent corrections show a lock wait in `pg_stat_activity` on a multi-thread runtime. |
| `scripts/focused.sh memory_idempotency_ --network-isolated` | 2 | 2 passed. Identical saves show the losers waiting on the uncommitted record. A changed deadline conflicts (F2), and the stored deadline is the request's own. |
| `scripts/focused.sh memory_access_level_ --network-isolated` | 14 | 14 passed |
| `scripts/focused.sh memory_authority_ --network-isolated` | 24 | 24 passed. New: citation needs `fetch` authority and reveals nothing (F3); delete above `write_max` is 403 (F4); a cross-credential replay re-checks authority (m8); self-citation is 400; a cited namespace deleted after authentication is `not_found` (m2). |
| `scripts/focused.sh memory_mcp_ --network-isolated` | 5 | 5 passed. New: correction and copy denials are identical over HTTP and MCP. |
| `scripts/focused.sh memory_contract_ --network-isolated` | 10 | 10 passed, after regenerating `openapi.json` |
| `cargo sqlx prepare -- --all-targets` | — | 65 queries |
| `scripts/verify.sh` | 77 | Every gate passed (`a2-p3r1-verify.log`), including `prepare --check`. All 77 tests passed under isolation (37 DB-backed). |

The `memory_storage_`, `memory_idempotency_` and `memory_access_level_` runs logged under these names are the reruns. In the first run, four tests failed:
- three replayed with a re-minted deadline, which F2 now refuses as a conflict;
- one fetched without naming the parent's namespace, and D4 filters provenance by the namespaces the read names.

The tests were corrected and the reruns passed.

The Linux isolation path, CI and the image job remain unavailable.
