# 07 — Verification and operations

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on all preceding groups. Verify a shared, tenant-safe eval system across different
workflow families. The commands and new test prefixes below are implementation requirements;
publishing this plan is not evidence that the eval system or live providers have been tested.

## Batch 07A — Focused checks and integrated acceptance

For each batch record exact command, revision, environment, nonzero selected/executed test counts,
results and unavailable checks. List filters before execution, for example:

```sh
cargo test --locked --offline --lib eval_scripted_ -- --list
cargo test --locked --offline --lib eval_scripted_
```

| Group | New focused test families | Additional proof |
| --- | --- | --- |
| 01 | `eval_contract_`, `eval_fixture_`, `eval_subject_`, `eval_bounds_`, `eval_verdict_` | Hashing, answer separation, all boundaries |
| 02 | `eval_storage_`, `eval_authorization_`, `eval_admission_`, `eval_dataset_` | Tenant constraints, competing writers, authoring forms |
| 03 | `eval_scripted_`, `eval_jobs_`, `eval_cancellation_`, `eval_fairness_` | Sample boundary, real worker races/poison batches |
| 04 | `eval_live_boundary_`, `eval_live_tools_`, `eval_live_budget_`, `eval_live_recovery_`, `eval_integration_` | Counted providers/effects, budget contention |
| 05 | `eval_assertion_`, `eval_hard_gate_`, `eval_metrics_`, `eval_judge_`, `eval_grader_security_`, `eval_calibration_` | Human labels and actual judge calibration |
| 06 | `eval_comparison_`, `eval_release_evidence_`, `eval_authoring_` | Matched results, browser workflow, no activation side effects |
| 07 | `eval_e2e_`, `eval_retention_` | Two-company acceptance, cleanup/restore, full CI gate |

Use `own_database` for global/unscoped worker claims, cleanup and whole-table assertions. Apply the
init schema to a fresh disposable PostgreSQL database; regenerate/check `.sqlx` for changed SQL.
No database/browser/provider absence may become a silently skipped passing acceptance check.
Broaden checks early at shared runtime/auth/budget boundaries; preserve one final full-suite gate.

Integrated acceptance uses routing, extraction, support/research, action, document and human-review
fixtures. Exercise two companies with restricted data, matching slugs and foreign identifiers.
Prove answer/reference isolation, offline samples, intercepted live tools, isolated child state,
explicit provider egress, hard gates, truthful partial reports and exact comparison identities.
Crash at admission/claim/provider/settlement boundaries; race claimants, cancellation, revocation
and budget reservations. Assert actual call counts and durable evidence, not just a final score.

## Batch 07B — Final full-suite gate

Run once the entire implementation is integrated. Reconcile against current
[CI](../../.github/workflows/ci.yml), install locked dependencies/SQLx CLI/Chromium and provision
isolated PostgreSQL 18.6 with CI's query-statistics configuration and parameter logging disabled.
Prepare dependencies before offline/network-isolated execution; do not alter shared databases.

```sh
cargo fmt --all -- --check
git diff --check
cargo check --locked --offline --all-targets
cargo clippy --locked --offline --all-targets -- -D warnings
SQLX_OFFLINE=false cargo sqlx migrate run
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets
cargo test --locked --offline --all-targets --no-run
scripts/test-network-isolation.sh cargo test --locked --offline --all-targets
scripts/test-network-isolation.sh scripts/stack-budget.sh --offline
scripts/test-network-isolation.sh scripts/lease-budget.sh --offline
npm run check:css
npm run test:run-counts
node --test scripts/tests/sse-navigation.mjs scripts/tests/agent-runtime.mjs scripts/tests/workflow-runs.mjs scripts/tests/workflow-conversations.mjs scripts/tests/workflow-samples.mjs
scripts/tests/deploy.sh
scripts/tests/credential-key-rotation.sh
scripts/tests/transport-boundary.sh
scripts/transport-boundary-check.sh
python3 scripts/tests/workflow-sample-boundary.py
python3 scripts/workflow-sample-boundary.py
python3 scripts/tests/knowledge-model-prepare.py
```

Also execute CI's production image/knowledge memory, query-statistics activation, HTML/team-
autocomplete, knowledge-library, agent-authoring, workflow-agent-picker, session-revocation and
decision browser gates with their exact fixture/environment setup. Add eval authoring/browser and
security/limit cases to CI. A missing
fixture is unavailable coverage, not a passing browser run. Pin any new git dependencies; preserve
non-root production operation, stack/lease budget checks and offline compilation.

Real-provider acceptance is separate from network-isolated CI. With an explicitly authorized
test budget and disposable data, run one live subject and calibrated judge, compare repeated cases,
and verify recorded actual usage/error behavior. Run actual integration/browser checks against
the disposable external services relevant to the workflow. Record missing credentials/services,
unsupported visuals or provider limitations; local fixtures do not establish live compatibility.

## Batch 07C — Retention, operating guidance, and release

Use the existing cleanup/job owner for bounded deletion of expired bulky run/trace/grading data;
define finite payload/summary retention defaults and CI boundary tests before release. Keep suite
revisions needed for supported comparisons until explicitly deleted under authorized lifecycle
policy. Report expired evidence honestly and prevent cancellation/cleanup races from reviving it.
Backup/restore must preserve company/access lineage and never automatically replay paid calls.

Document dataset review/capture/redaction, development vs held-out splits, rubric calibration,
provider consent/budgets, live-call uncertainty, cancellation and incomplete results. Configuration
examples name only keys actually read. Do not promise startup checks, exact model reproducibility,
statistical certainty or runtime security from an eval score that the implementation cannot supply.

Emit identifiers, progress/counts, typed failures and aggregate usage without putting input bodies,
expected answers, judge prompts, tool payloads or secrets into operational logs. Keep protected
evidence in authorized storage, with explicit export/deletion and provider disclosure controls.
Retain release evidence for the exact subject/dataset/grader combination and observed limitations.

Release requires no unresolved company leaks, production side-effect escape, answer leakage,
budget over-dispatch, stale-owner settlement or false passing verdict. Rerun affected checks after
fixes and repeat the full gate when warranted. Incompatible schema/payload changes reset data;
there are no old report decoders, dual formats or staged compatibility modes. Refresh graft after
substantial implementation and register every plan file in the startup reset-note list.

## Exit criteria

The implemented eval system has reproducible, tenant-safe evidence across workflow families and
accurate operating limits. Every required unavailable check is reported; full verification is
claimed only when the complete applicable gate actually ran and passed.
