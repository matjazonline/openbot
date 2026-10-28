# 05 — Authoring, scenarios, and release verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Make `jev.evaluate` authorable and observable, then verify the complete integration once.
Only this phase exposes the fully wired step in the deployed catalogue.

## Batches and focused checks

1. Add company TypeSafe connection management and workflow resource selection in existing
   company/workflow forms. The step catalogue shows a compiling mixed-question example,
   exact per-question output paths, versioned model selection, Box's experimental status,
   and why Noul has no separate confidence. Display connection readiness and permission
   failures without secret values. Update `docs/workflow-language-v1.md`, operations/deploy
   guides, and UI examples with configuration keys the application actually reads.
   Focused checks: relevant company form tests, `workflow_authoring_` tests, and
   `node --test scripts/tests/workflow-authoring-browser.cjs`.
2. Extend the sealed sample evaluator with scripted Jev outputs and usage. Add an end-to-end
   scenario: one mixed Noul/Choice/Score step, `decision.rule` thresholds, an uncertain
   `decision.human` path, and a successful automatic path. Add a separate Box fixture and
   rejection case. Run through compilation, publication, binding, worker execution, result
   validation, run history, and sample mode. Tests use loopback/scripted provider responses
   and a fresh database; the test network guard must reject external egress.
   Focused checks: `jev_end_to_end_`, `workflow_sample_`, relevant run-view tests,
   `node --test scripts/tests/workflow-samples.mjs`, and both sample-boundary scripts.
   List filters first, and record actual executed-test counts.
3. Review final failure semantics, credential rotation/revocation, competing workers, spend
   accounting, model version pinning, image bounds, and redaction. Refresh `graft` with
   `graft build` if available. Any raised bound must have a CI check that fails as it is
   approached. Regenerate and commit `.sqlx/` for every changed SQL query.

## Final full-suite gate

Run the following once after all implementation phases, on a fresh migrated database, before
declaring completion. After a final fix, rerun affected checks and repeat the full gate when
the change crosses shared compiler, publication, binding, or execution behavior:

```sh
cargo fmt --all -- --check
git diff --check
scripts/tests/transport-boundary.sh
scripts/transport-boundary-check.sh
python3 scripts/tests/workflow-sample-boundary.py
python3 scripts/workflow-sample-boundary.py
cargo check --locked --offline --all-targets
cargo clippy --locked --offline --all-targets -- -D warnings
SQLX_OFFLINE=false cargo sqlx migrate run
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets
cargo test --locked --offline --all-targets --no-run
scripts/test-network-isolation.sh cargo test --locked --offline --all-targets
scripts/test-network-isolation.sh scripts/stack-budget.sh --offline
scripts/test-network-isolation.sh scripts/lease-budget.sh --offline
npm run check:css
node --test scripts/tests/sse-navigation.mjs scripts/tests/agent-runtime.mjs scripts/tests/workflow-runs.mjs scripts/tests/workflow-conversations.mjs scripts/tests/workflow-samples.mjs scripts/tests/workflow-authoring-browser.cjs
```

Use the CI job in `.github/workflows/ci.yml` for additional browser/deploy gates that the
changed surfaces require. If PostgreSQL, SQLx CLI, Node, browser dependencies, or `graft` is
unavailable, record the exact skipped command and reason; never report full verification as
passed. Keep the command/result/test-count record with the implementation evidence, not only
in a summary.

Acceptance: one published, bound Jev step answers multiple typed questions against the same
state; downstream rules can read exact typed fields; the optional image path is authorized and
bounded; revocation and crash recovery behave as documented; and all final gates pass.
