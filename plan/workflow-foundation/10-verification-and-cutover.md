# 10 — Verification, operations, and fresh-database cutover

## Outcome and dependencies

Depends on phases 1–9. Verify the whole architecture, retire obsolete paths, and prepare a deliberate
fresh-database deployment. This phase does not introduce backward compatibility or data conversion.

## End-to-end acceptance matrix

| Scenario | Required evidence |
| --- | --- |
| Reviewed response | Classification/context/memory visible; exact accepted artifact delivered |
| Agent capability defaults | No classifier/profile uses saved agent tools and skills; explicit empty or invalid selections never broaden into defaults |
| Human feedback | Comments do not advance; revision feedback reaches new work; old records survive |
| Alternate route | Human or agent choice selects only an eligible declared successor |
| Concurrent messages | Same-thread messages progress independently through decisions and replies |
| Immutable execution | Version/binding/agent edits cannot change an admitted run |
| Duplicate ingress | One admission per source identity and binding |
| Competing workers | One logical result/successor under lease contention and stale writes |
| Human races | One winner among submission, timeout, cancellation, and competing reviewers |
| Recovery | Crash/restart at each durable boundary retains completed work and pending continuations |
| External uncertainty | Unknown non-idempotent writes are not blindly repeated |
| Partial delivery | Successful destinations are not resent when a sibling fails |
| Workflow tools | Parent agent resumes its saved call after child completion/review |
| Bounded repetition | Last-round acceptance works; exhaustion never auto-approves |
| Authorization | Cross-company references, revoked grants, and stale approvals fail closed |
| Sample run | No customer messages, external writes, or production memory changes |

Use scripted local model/provider endpoints and real PostgreSQL transactions. Concurrency tests
must use competing claimants, not only sequential mocks. Use dedicated test databases, and isolated
fixtures for unscoped queue claims and whole-table assertions. Test poison batches explicitly.

## CI and reproducibility

Keep formatting, offline compilation, fresh migrations, SQLx metadata verification, database-backed
tests, transport boundaries, and the stock-stack budget in CI. Regenerate and commit SQLx metadata
when queries change. Run checks with explicit intended database URLs; never target a production
or developer inbox with worker-claim tests.

```sh
cargo fmt --all -- --check
git diff --check
SQLX_OFFLINE=false cargo sqlx migrate run
SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets
SQLX_OFFLINE=true cargo check --locked --all-targets
SQLX_OFFLINE=true cargo clippy --locked --all-targets -- -D warnings
SQLX_OFFLINE=true cargo test --locked --all-targets
scripts/transport-boundary-check.sh
scripts/stack-budget.sh
```

The commands above assume database variables have already been set to the appropriate isolated
environment. Keep dependency lockfiles and pinned git revisions. Keep the production image non-root.
If a resource limit must increase, document the reason and preserve a CI check that fails early
when growth approaches the new limit; do not remove the signal by only raising the ceiling.

## Operations

Expose metrics for queue age, active/waiting runs, step latency, lease expiry, retries, decision age,
deadline expiry, unknown effects, delivery acceptance/failure, and model token/spend reservations.
Keep payloads and secrets out of metric labels. Link actionable failures to the exact run/action.

Document safe retry, decision reassignment, cancellation, unknown-effect reconciliation, disabled
connections, and expired waits. Every parked state must have a tested deadline and maintenance path.
Verify per-company fairness and bounded worker capacity under mixed load.

## Cutover

1. Confirm the deployment/database target and retain any required export outside this implementation's
   data-migration scope. A fresh schema is the design target, not permission to reset an arbitrary DB.
2. Stop old admission and workers, and account for in-flight external effects before retiring the old
   deployment. Do not let old and new dispatchers concurrently send from the same inbox.
3. Deploy the fresh schema and application, recreate company resources/connections and workflow
   bindings, and install validated template fixtures. Keep automatic admission disabled initially.
4. Execute isolated samples, then controlled email/manual/schedule smoke runs. Verify decision
   feedback, exact reply threading, independent concurrent runs, and restart recovery.
5. Activate selected bindings deliberately and monitor queues, waits, and receipts. Emergency stop
   disables admission and undispatched work; it does not claim to undo accepted provider effects.
6. Remove obsolete flags, examples, runtime selectors, test fixtures, and execution paths. Mark older
   overlapping planning documents as superseded where needed so they do not prescribe another engine.
7. Refresh the context graph with `graft build` after the code changes and finalize operational docs.

## Completion gate

The supported business entry points use one workflow engine, one agent runtime, and one action
execution boundary. The acceptance matrix and required CI checks pass. Production configuration
documentation names only implemented keys and guarantees. Remaining extensions are recorded in
[NOT_PLANNED.md](NOT_PLANNED.md), not left as partially enabled production paths.
