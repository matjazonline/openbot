# 07 — Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [06](06-authoring-and-operations.md) and all earlier phases. Prove reusable agent
capabilities, independent revision activation, stable run configuration, and live authority across
the complete system. These are future implementation requirements; saving the plan does not mean
the implementation or these gates have run.

## Batch 07A — End-to-end acceptance and recovery

Add `autonomous_agent_e2e_` tests using real persistence, admissions, workers, and counted scripted
model/MCP/HTTP peers. Cover these journeys:

1. Publish one agent with instructions, a skill, an HTTP operation, and a reviewed MCP tool.
   Publish two unrelated workflows containing `agent.run` with no MCP resource slots. Both agents
   call their configured tools successfully with no duplicated workflow capability configuration.
2. Admit and queue run A on agent v1, publish v2, then admit run B through the unchanged workflow
   version. A executes v1 and B executes v2. Editing a draft or shared skill without publishing
   changes neither. Record exact model, skill text, binding, and revision identities used.
3. Suspend A for approval or a workflow-tool child, publish another revision, reconstruct the
   worker, and resume A. It retains the original manifest and saved calls. Children first started
   after publication inherit the captured revisions, including agents not yet executed by A.
4. Remove/revoke a binding after preparation but before guarded dispatch. No new remote effect
   begins after the authority check refuses it. Retarget the same alias and prove an old call
   cannot invoke the new target. Account accurately for requests already in flight at revocation.
5. Change a reviewed MCP schema/operation, revoke a connection grant, rotate a credential, remove
   model availability, and detach a skill assignment. Exercise the intended freeze/live-authority
   boundary for each; no unreviewed operation or secret enters a saved revision or sample.
6. Exercise built-ins, library copying, provisioning, manual/message/scheduled admissions, nested
   workflow tools, samples, and model-only steps. All use the same revision semantics, preserve
   tenant boundaries, and preserve message/approval authority supplied by execution context.
7. Attempt foreign-tenant bindings/revisions, undeclared dynamic agents, reserved-name spoofing,
   alias collision, missing skill requirements, oversized manifests, and recursive dependencies.
   Refuse deterministically before provider entry or production effects.
8. Crash/reconstruct at publication commit, admission commit, saved model call, action reservation,
   remote effect/receipt, child parking, and continuation. Assert stable identities and exact
   counted effects where the provider contract permits it. Ambiguous remote outcomes remain
   uncertain and follow existing reconciliation; do not claim universal exactly-once effects.

Race actual competing publishers, duplicate admissions, publication versus admission, revocation
versus reservation, duplicate dispatchers, and child resumers. Use the existing real-database race
and failure-injection facilities, with `own_database` for unscoped queue claims/sweeps. Sequential
mock replay is not adequate evidence for a concurrency protocol.

## Focused checks during implementation

All prefixes below name required future test families. List each before running and confirm a
nonzero selected/executed count. Run `cargo test --locked --offline --lib PREFIX` with an explicit
disposable test database and include the smallest affected existing regressions.

| Phase | New focused prefixes | Existing regression areas |
| --- | --- | --- |
| 01 | `autonomous_agent_contract_`, `autonomous_agent_storage_` | Agent entities, schema constraints, capability serialization |
| 02 | `autonomous_agent_resolution_`, `autonomous_agent_skills_`, `autonomous_agent_publication_` | Required skills, reviewed MCP catalog/operation policy, workflow-tool pins |
| 03 | `autonomous_agent_workflow_reference_`, `autonomous_agent_admission_` | Registry/compiler/publication, admission replay, child dependencies |
| 04 | `autonomous_agent_dispatch_`, `autonomous_agent_authority_`, `autonomous_agent_recovery_` | Agent replay, MCP/HTTP approvals and uncertainty, message authority, lease fencing |
| 05 | `autonomous_agent_provisioning_`, `autonomous_agent_entrypoints_`, `autonomous_agent_children_`, `autonomous_agent_samples_` | Provisioning, schedules, libraries, child park/resume, sample isolation |
| 06 | `autonomous_agent_authoring_`, `autonomous_agent_views_` | Forms/API authority, rendering, sessions, agent/run/sample browser protocols |
| 07 | `autonomous_agent_e2e_` | Existing reviewed workflow and provider execution scenarios |

For SQL-bearing batches, apply migrations to a fresh disposable database and run
`SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets`; retain required `.sqlx/` changes.
An empty metadata diff does not verify runtime SQL: execute database-backed tests. After init
schema edits, recreate only the dedicated databases used for the implementation. Do not run
queue tests or destructive resets against development/production data.

Example setup with a local PostgreSQL role matching the current user:

```sh
agent_plan_db="open_bots_autonomous_agent_$(date +%s)_$$_test"
createdb "$agent_plan_db"
export DATABASE_URL="postgres://$(whoami)@localhost:5432/${agent_plan_db}"
export TEST_DATABASE_URL="$DATABASE_URL"
export SQLX_OFFLINE=true
SQLX_OFFLINE=false cargo sqlx migrate run
```

Keep `ALLOW_MISSING_DATABASE_URL` unset. Provision dependency caches, Node packages, Chromium,
and SQLx CLI before offline/network-isolated checks. Use a disposable PostgreSQL 18.6 instance
with CI's query-statistics settings for the final gate; do not reconfigure a shared server.
Clean up only the explicitly created test databases once no worker/test owns them.

## Batch 07B — Final full-suite gate

Run the full gate once after all phases are integrated. Reconcile the commands below against
[CI](../../.github/workflows/ci.yml) at implementation time; CI is the authoritative inventory.
[scripts/check.sh](../../scripts/check.sh) currently omits some CI gates and defaults its migration
URL to a development database, so it is not sufficient by itself.

Record the revision and environment, then run:

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
```

Also run CI's query-statistics activation assertions and fixture-backed browser gates:

```sh
export HTML_INJECTION_BROWSER_FIXTURES="$(mktemp -d)"
export APP_JS_OUT="$HTML_INJECTION_BROWSER_FIXTURES/app.js"
cargo test --locked --offline --lib dump_html_injection_browser_fixtures -- --ignored
node --test scripts/tests/team-autocomplete-browser.mjs scripts/tests/html-injection-browser.mjs

export SESSION_REVOCATION_BROWSER_FIXTURES="$(mktemp -d)"
cargo test --locked --offline --lib dump_session_revocation_browser_fixtures -- --ignored
node --test scripts/tests/session-revocation-browser.mjs

export WORKFLOW_DECISION_BROWSER_FIXTURES="$(mktemp -d)"
cargo test --locked --offline --lib decision_browser_markup_and_closed_draft_recovery
cargo test --locked --offline --lib source_message_outside_window_is_exact_and_authorized
node --test scripts/tests/workflow-decisions.mjs
```

Register any new required agent publication/selector browser or boundary gates in CI. Preserve
stack/lease budgets and production security checks. If implementation raises a bound, record why
and identify the CI check that still fails before exhaustion.

Record every batch/final command, exit result, selected/executed counts, relevant artifacts, and
unavailable checks. After fixes rerun affected checks; repeat the complete gate when impact
warrants it. Never claim full verification with skipped database tests, failed network isolation,
missing browser fixtures, or unexecuted required checks.

## Release acceptance

- All seven phases and end-to-end cases pass. No unresolved authority, duplicate-effect,
  cross-tenant, mutable-revision, or admission consistency finding remains.
- Agent definitions are reusable and publish independently; workflows keep task/orchestration
  contracts; runs record complete stable revision/dependency selections. Revocation stays live.
- Old workflow-owned agent capture and name-only grant paths are removed after their consumers
  migrate. Source schema, query metadata, built-ins, templates, samples, UI, and documentation agree.
- Reset incompatible data rather than adding legacy decoders, backfills, or staged data rollout.
- Refresh the graph with `graft build` after substantial implementation, inspect the result, and
  record availability/results. Keep all plan files listed in `NO_BACKWARD_COMPATIBILITY_NOTES`
  in [src/main.rs](../../src/main.rs).

## Planning-only verification record — 2026-11-03

- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- An inline `python3` document audit passed: seven consecutively numbered phase files, all 61
  local Markdown links resolve, each file contains the database-policy note, every new plan path
  appears exactly once in `NO_BACKWARD_COMPATIBILITY_NOTES`, and new-file whitespace is clean.
- The only source edit for this plan is its seven startup-warning path entries. Existing unrelated
  plan files and warning entries were preserved. No execution logic or database was changed.
- Rust compilation, database, browser, and end-to-end suites were not run for this documentation
  and warning-list update. All implementation gates above remain future requirements.
