# 02 — Independent application foundation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [01](01-application-boundaries-and-contracts.md). Attribution builds, starts, migrates,
and tests from its own directory without the Openbot package or database. Co-location is a
deployment convenience only.

## Batch 02A — Create the standalone package and runtime

1. Create `services/attribution/` with its own `Cargo.toml`, committed `Cargo.lock`, `src/`,
   `migrations/`, SQLx metadata, tests, web assets, README, configuration example, Dockerfile, and
   applicable instructions. Keep it an independent Cargo project; do not turn shared parent
   dependency configuration into a build requirement. Package name: `attribution_service`.
2. Use Rust/Axum/PostgreSQL and the repository's applicable safety patterns, with local ownership
   of domain, application, HTTP, and persistence code. Share maintained dependencies, not private
   Openbot source. Avoid placeholder abstractions and empty worker frameworks.
3. Give the application its own listener, public origin, database configuration, tracing startup,
   health/readiness endpoints, connection and request limits, graceful shutdown, and timeouts.
   No agent runtime, workflow engine, email transport, or Openbot session authority is loaded.
4. Use its own database role and database, even when the PostgreSQL server is shared. Neither
   application's role may read the other's database. Attribution alone owns its migration history.
5. Build a non-root production image with its assets inside the image. Pin git dependencies by
   revision/tag and use locked builds. Document only implemented configuration keys and actual
   startup validations; secret values never appear in logs or examples.

Focused checks: `attribution_startup_` and `attribution_storage_boundary_` cover bad configuration,
readiness, migration errors, graceful shutdown, and separate-role denial against real PostgreSQL.
Run service formatting and offline all-target compilation after dependencies are available. A
missing database must fail database tests rather than make them silently pass.

## Batch 02B — Establish independent builds and extraction checks

1. Add a service-local verification entrypoint and CI job that runs from `services/attribution/`.
   Use its own frontend manifest/lockfile only if a frontend toolchain is needed; server-rendered
   pages with small local browser scripts are sufficient. No Openbot asset pipeline dependency.
2. Add an executable boundary check: reject parent path dependencies, imports of Openbot, parent
   asset/config references, and SQL access to Openbot tables. Check Cargo's resolved dependency
   graph as well as source references; a wrapper crate must not bypass this rule.
3. In CI copy only the service directory into a temporary standalone checkout, provide explicitly
   named test dependencies, and build/test it there. Exercise identity through a contract-faithful
   HTTP test server; run real two-application contract tests in phase 10.
4. Keep Openbot's existing build and CI independent. Add service gates without silently replacing
   Openbot coverage or causing root-only commands to appear to test the service.

Focused checks: `attribution_boundary_`, standalone-copy compilation/test, container non-root
smoke, and fresh-database migration. Run dependency/boundary checks in CI from this phase onward.
Use service-local `cargo test --locked --offline --all-targets PREFIX -- --list`, verify the intended
nonzero selection, then run the same command without `-- --list`. Record commands and results here.

Completion: only HTTPS contracts and external identifiers connect the applications. Full
verification remains the gate in [10](10-integration-verification-and-pilot.md).
