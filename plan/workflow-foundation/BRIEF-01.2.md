# 01.2 application orchestration and ports

Original phase 01 Implementation item 2 and README remain authoritative. Expanded by `/root/astra_01_2`, session `01a0ddec-10b4-7f30-bd63-943c987e3107`. No blocking ordering conflict with the bounded scope below.

## Scope and integration

Add `src/application/workflow/` and module registration in `src/application/mod.rs:1–10`. Suggested contracts.rs, ports.rs, service.rs, tests. Preserve verified 01.1. Reuse workflow IDs (domain/workflow/ids.rs:66–89), ValidatedWorkflow (graph.rs:47–65), bounded context machinery (context.rs:343–359), AppResult/AppError (application/app_error.rs:6–38), async_trait/Send+Sync convention (services/harness/runs/mod.rs:169–224). No new dependencies.

Do not extend TaskPersistence or reuse harness RunId, checkpoint status, TaskLeaseRef, BackgroundTask as workflow state. No existing execution entry points are rewired until phase 08. This is an internal foundation, not a deployed second path or compatibility shim. No SQL/migrations/adapters/harness/task-worker/configuration changes. No fake successful default trait methods.

01.3/01.4 own complete StepOutcome and RunState, so completion/wait/failure methods are added then. Do not invent placeholder statuses or opaque update payloads. 01.5 owns authorization; company scope is not authorization proof. Schema/publication/binding and production durable execution remain later phases.

## Five cohesive ports

1. WorkflowDefinitions: company-scoped lookup of published immutable VersionId, returning owned-company envelope plus ValidatedWorkflow. Missing vs infrastructure failure distinct; service verifies returned company/version/workflow identity before writes. Structural validation does not imply schema/resource safety.
2. WorkflowAdmission: one atomic prepared admission method covering dedup + immutable run snapshots + first execution/job. Command includes company, proposed run ID, workflow/version/entry, bounded input/params, checked idempotency key separate from correlation, first execution identity. Created/replayed/conflict explicit. Dedup company-scoped independent of proposed random ID. Same key + different definition/snapshots conflicts. No separate enqueue call after admission. Binding/trigger admission policies remain deferred.
3. WorkflowRunTransitions: initially narrow cancellation with company/run/expected revision. Atomically update authoritative progress and invalidate pending work/ownership; retain committed outputs and effect receipts. Applied/already-terminal-or-applied/revision-conflict/not-found distinct. No generic set_status/update or separate public successor enqueue.
4. WorkflowExecutionScheduling: bounded ready claim returning scoped company/run/execution/step, worker, fresh fence, ownership receipt. Positive bounded batch and lease. Exclusive claim excludes live owners, fresh fence and expiry required at later commits. No production worker/poll/retry/heartbeat/fairness/SQL yet. Admission and later transition ports create their jobs atomically.
5. WorkflowInspection: company-scoped run-head lookup with workflow/version/run/revision. Missing distinct from failure. No optional/string placeholder status; full state follows 01.4.

Correctness methods required in every implementation. Semantic strings use checked newtypes; purpose-specific infrastructure UUID wrappers may live in application. Never log full input/params requests.

## Orchestration

Admit loads company/version, checks returned identities, validates bounded snapshots, derives entry from definition, constructs prepared command, calls atomic admission once. Prepared fields are private/checked with read access for adapters. Caller cannot choose arbitrary entry.

Cancel loads scoped run head, verifies company/run, supplies expected revision to transition, preserves conflicts/missing/errors. No redundant forwarding async wrappers around scheduler/inspection. Reuse domain resolver for snapshot size/depth/work checking; define per-snapshot vs aggregate budget explicitly and enforce before unbounded cloning/traversal. Schema validation remains absent. Cancellation external-effects rollback is not promised.

## Edit order and verification

Register/document ownership and limitations; add checked requests/results/prepared command; add five ports; implement small admission/cancellation service; add focused tests; inspect diff/imports/checks; refresh graft. Read repo/src/application instructions, use graft and callers for existing-symbol changes. Preserve plans and parent records. No commits/deploy/reset DB.

Tests: entry derived from actual definition; atomic admission command carries scope/definition/input/params/first execution; lookup absence/error/mismatched company/version prevents write; exact/over snapshot bounds; write failure propagated; exact replay original run, changed payload conflict, same key in other company independent. Two competing service admissions produce one run/job in explicit in-memory implementation. Competing claimants claim one execution once; zero/over batch and invalid lease reject. Cancellation carries head revision, rejects stale update, preserves missing/errors, rejects wrong company/run head. Use barriers or joined futures, not sequential mocks. These establish application contracts only; PostgreSQL races/recovery remain phase 03.

```sh
cargo fmt --all -- --check
git diff --check
SQLX_OFFLINE=true cargo test --locked --offline --lib application::workflow
SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib application::workflow
SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib domain::workflow
SQLX_OFFLINE=true cargo check --locked --offline --all-targets
```

No DB/service resources, metadata regeneration, or cleanup required. Whole-phase guarantees of full states/transitions/auth/callers/causality remain pending later points. The interim ports grow in subsequent points with no compatibility obligation.

## Review 1 corrections

1. ports.rs:19–38: public trait/type docs must carry actual contract obligations, not leave them only in tests. Admission dedup is company-scoped, exact replay returns original run, changed identity/input/params conflicts, proposed random IDs excluded from replay equivalence. Cancellation preserves committed outputs/receipts, invalidates ownership, never promises rollback. Claims respect batch bounds, exclude live owners, persist fresh fence, require fence+expiry for writes; phase03 owns expiry/recovery accounting. Definition/inspection docs state scope, immutable publication, missing vs error, structural-only validation (no schema/resource/auth proof).
2. service.rs:96–101 and contracts.rs:84–91: replace `(Value, Value)` return and adjacent input/params Value constructor args with internal named snapshot struct. Avoid silent argument swaps, per src/AGENTS.md.

Architecture and tests otherwise pass review; Astra independently confirmed 8/8 at 2 MiB and whitespace check. No new runtime scope/tests required beyond validation of final refactor.
