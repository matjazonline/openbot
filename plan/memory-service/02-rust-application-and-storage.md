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
