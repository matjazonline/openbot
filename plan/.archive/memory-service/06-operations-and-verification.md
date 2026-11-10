# 06 Operations and verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [05](05-openbot-integration.md) and all preceding phases. Demonstrate that the service
works independently, integrates safely with OpenBot, and can move to another host or repository
without changing its domain/application code.

## Batch 06A Establish operational behavior

1. Implement and document retention separately for canonical memories, raw evidence, generation
   inputs, idempotency results, audit records and deletion barriers. Bound cleanup work. Preserve
   source provenance needed by live jobs without making sensitive data impossible to delete.
2. Define correction, forgetting and tenant purge across observations, vectors, jobs, pending
   deliveries and client snapshots. Document the limits of deletion after information has already
   been consumed. Cancel inference before cleanup and ensure stale workers cannot resurrect data.
3. Expose readiness, queue age/depth, indexing failures, retries, terminal/unknown generation,
   lease loss, tenant fairness, provider usage and context omissions. Correlate requests through
   capture, generation, save and recall using IDs; do not log prompts, full memory text or secrets.
4. Document credential rotation, provider outage handling, explicit recovery and database restore.
   Restore canonical content and operation state consistently; never interpret restore/restart as
   authorization to replay old inference. Test backups on disposable databases.
5. Pin image/model/runtime inputs and validate the non-root image with network downloads denied
   at startup. Exercise resource budgets on supported hardware and keep an early CI failure for
   growth near any limit that is raised. Keep process and test-thread stack bounds consistent.

Focused checks: `memory_operations_` covers retention, in-flight deletion, rotation, restore,
quota/auth recovery, expired requests and missing model assets. Measure the agreed representative
load and verify controlled overload, tenant fairness and the context/worker resource budgets.

## Batch 06B Prove standalone operation and integration

1. Copy only `services/memory/` to a clean directory and build/test it with its own lockfile,
   metadata and model assets. Deny access to parent files, OpenBot code/configuration/database and
   OpenBot HTTP endpoints. A generic client provisions a tenant/namespace/key, saves a note,
   searches, fetches details, obtains context, generates observations through a test provider,
   corrects a note and deletes its data. This journey cannot require OpenBot-shaped payloads.
2. Run OpenBot and the service as separately built artifacts with separate database roles and
   credentials. Exercise automatic recall, intentional save, capture, generation and later recall;
   validate source provenance, classification, current audience, revocation and replay behavior.
   With readers at 6/8/9, prove recall returns only levels 0–6, a context consuming level 4 saves
   at 4, and any context consuming level 6 saves at 6. Verify level-3 readers cannot retrieve the
   level-4 save through HTTP, MCP, snippets, timeline, context or source metadata. Repeat with an
   audience downgrade, a public latest recall after private history, and tampered labels. Missing
   precise consumption evidence with a trusted ceiling of 6 must save at 6; missing trusted
   authority or known level-7 source content under that ceiling must fail safely.
3. Move the unchanged memory artifact to a separate-host topology. Change only explicit deployment,
   endpoint and credential configuration. Repeat the independent-client and OpenBot journeys.
   Prove database roles cannot read each other's tables and builds need no sibling source.
4. Run real competing-claimant cases for saves, job claims, delivery claims, stale completions,
   corrections, deletion, credential revocation and recall/save classification accounting.
   Include crash/timeout boundaries around
   provider dispatch and result commit; mocked sequential transitions are not concurrency proof.

Focused checks: `memory_standalone_` and `memory_e2e_` with actual PostgreSQL/pgvector, independent
HTTP clients, a counted test provider and the production image. Record topology and unavailable
checks explicitly. A paid-provider smoke check needs a named model and bounded authorized budget;
ordinary CI uses the test provider and real local embeddings.

## Batch 06C Run the final full-suite gate

After all implementation is integrated, run the complete service verification entrypoint and the
complete current [OpenBot CI](../../.github/workflows/ci.yml) gates. The root
[check script](../../scripts/check.sh) currently omits some CI gates; compare against live CI at
implementation time. Do not run the full suite after every batch.

For each independent Rust project, run from its own directory with its own disposable databases
and offline SQLx metadata:

```sh
cargo fmt --all -- --check
git diff --check
cargo check --locked --offline --all-targets
cargo clippy --locked --offline --all-targets -- -D warnings
SQLX_OFFLINE=false cargo sqlx migrate run
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets
cargo test --locked --offline --all-targets --no-run
cargo test --locked --offline --all-targets
```

Set `SQLX_OFFLINE=true` for compilation/tests. Prepare dependencies and the required model/runtime
assets before denying external network access. Use the projects' network-isolation harnesses for
database tests and preserve OpenBot's stack/lease budget checks. Never migrate or reset a
development/customer database to run verification.

Also run every current OpenBot browser, CSS, deployment, credential, transport/sample boundary,
model-artifact, query-statistics and fixture-generation gate. For the service, run contract/MCP
checks, extracted-copy/boundary checks, non-root container startup, real-model retrieval, restore,
load and independent-client acceptance. Add concrete service script names when those scripts
exist; root commands alone never verify the service.

Record exact commands, revisions, environment, nonzero selected/executed counts and results in the
owning phase files. Rerun affected checks after fixes; repeat the full gate when their impact
warrants it. Refresh graft after substantial implementation and inspect the resulting changes.

## Completion criteria

- The service builds, starts and serves a generic client with OpenBot absent.
- Its own database durably owns memories, provenance, searchable vectors and accepted operations.
- Keyword and semantic retrieval enforce identical tenant, namespace and classification rules.
- Saves use the proven consumed level, never a lower model-selected label. Read ceilings are
  distinct from saved labels; summaries, copies, corrections and retries cannot launder private
  context into lower-level memories. Missing precise consumption evidence saves at the producing
  context's trusted authorized ceiling, retained across retries and audience changes. Missing
  trusted authority or known content above that ceiling is a controlled refusal.
- Agents receive bounded untrusted memory context and can retrieve selected details through tools.
- No duplicate logical saves, cross-scope leaks, stale-worker commits, implicit paid replay or
  resurrection after deletion remains unresolved.
- Every phase's focused checks and the final service/OpenBot gate have recorded results; missing
  required infrastructure is reported as unavailable, not converted into a pass.

## Planning verification record

Plan creation changes only these documents and their startup-warning path registration. Runtime,
schema, provider, container and integration checks above are future implementation requirements.

2026-11-08:

- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- `python3` inline document audit: passed; seven documents, six consecutive numbered phases,
  all 33 local Markdown links resolve, each document carries the database-policy note, every
  document is registered exactly once in `NO_BACKWARD_COMPATIBILITY_NOTES`, and all new files
  have clean whitespace and final newlines.
- Source changes are seven startup-warning paths in `src/main.rs`. No feature implementation,
  dependency, schema, provider call, deployment or database reset was performed.
- Compilation, database, model, container and integration suites were not run for this
  documentation/path-list change. The full implementation gate above remains required.

Access level refinement on 2026-11-08:

- Integrated ordered levels, bounded reads, consumed-level saves, and the authorized-ceiling
  fallback across the contract, storage, retrieval, generation and OpenBot integration phases.
  Added required future cases for inherited private context, audience changes and recall/save
  races. The service remains independent of OpenBot's participant and workflow models.
- `git diff --check`: passed.
- `python3` inline document audit: passed; seven documents, six ordered phases, all 38 local
  Markdown links resolve, and policy notes, startup-warning registrations, focused-check sections
  and whitespace are valid. Reviewed missing-evidence rules across all phase files for consistency.
- This refinement changes documentation only. Rust, database, provider and runtime security tests
  were not run; their execution remains part of the implementation gates above.
