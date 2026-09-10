# Step 9: Contract tests and CI gates

Dependencies: steps 1–8. Main files: Rig sibling test modules, shared application test support,
persistence/HTTP tests, `.github/workflows/ci.yml`, and relevant scripts.

## Test matrix

Use local scripted provider HTTP servers and deterministic models in required CI. Real provider
credentials and model nondeterminism must not be prerequisites for a green build. Keep all fixtures
free of credentials and private mail. Exercise serialization at each actual provider boundary,
not only a fake `AgentHarness` that bypasses the new adapter.

| Area | Release evidence |
| --- | --- |
| Registry | Five keys, exact models, unsupported/duplicate registration, trusted endpoints |
| Isolation | Concurrent tenants, key rotation, isolated mutable tools/run state |
| Providers | Text, tool call/result continuation, malformed payload, auth error, rate limit, timeout |
| Configuration | Unset env → Rig, ai-agents override, explicit precedence, invalid env, typed bounds |
| Tools | Native/built-in schemas, allowlist, unavailable context, forged names, bounded output |
| Skills | Ordered calls, named output interpolation, shared budgets, suspend/resume |
| Approval | Approved/pending/rejected/error; no execution before approval |
| Suspension | Stops same-turn remaining tools and subsequent model calls; no response delivery |
| Recovery | Approved retry, quorum continuation, no duplicate committed effects |
| Lifecycle | Timeout, lease loss, cancellation, shutdown, bounded retries, no detached work |
| Accounting | Multiple calls, missing/partial usage, safe errors and diagnostics |
| Product | Save/edit/copy, configured defaults, explicit dispatch, simulation and historical readers |
| Database | Fresh/upgrade migrations, both harness values, bad value rejection |

Parameterize shared harness contract scenarios over both implementations where behavior should
match. Retain provider-specific fixtures for differing message formats, tool-call IDs, and usage.
At least one database-backed task test must enter Rig via production dispatch and finish through
the existing task/message/outbox transaction.

Use competing database claimants for any new or modified ownership, edit-fencing, resume, or
idempotency protocol. Prove an expired worker cannot commit a tool effect after another claimant
takes over. Sequential mocks do not establish this guarantee. Preserve poison-batch retry tests
if worker error classification changes.

## Required checks

Run against an isolated migrated test database. Prepare metadata against the intended schema;
provide `DATABASE_URL` through the test environment, not copied production credentials.

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

Regenerate/commit `.sqlx/` when queries change, rather than including unrelated metadata churn.
`SQLX_OFFLINE=true` means compilation needs no live database for SQL macros; it does not prohibit
Cargo dependency downloads. If verifying dependency-network independence, fetch the lockfile first
and then run `cargo check --locked --offline --all-targets` as a separate check.

Extend the architecture gate to reject `rig`, `rig_core`, and `rig_agent` imports in domain/
application code, following the existing script's treatment of production versus test source.
Do not exempt new adapter imports into inner layers.

Measure the task-worker → Rig chain with `scripts/stack-frames.sh` and retain the stock-stack CI
gate. Ensure its exercised scenarios actually enter Rig. Keep `clippy::large_enum_variant` enabled.
Do not raise `RUST_MIN_STACK`, `RUNTIME_THREAD_STACK_BYTES`, or `STACK_BUDGET_KIB` to hide growth;
shrink/box the chain first. Any justified raised bound needs a recorded reason and an early-failing
CI signal, with platform calibration distinguished from regression.

## Acceptance

All mandatory CI gates pass with both harnesses compiled and exercised. Optional live-provider
smoke results are recorded separately and do not substitute for deterministic contract coverage.
Run bootstrap/default-resolution integration scenarios with the property absent and explicitly set
to ai-agents. Existing ai-agents-specific fixtures must request that harness explicitly instead of
accidentally relying on the old static default.
