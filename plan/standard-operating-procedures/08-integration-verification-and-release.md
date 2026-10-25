# 08 — Integration, verification, and release

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependencies

Prove the SOP product works through the implemented workflow engine, including contention,
recovery, external effects, and human use. Scenario/fixture design starts with 01. Execute each
milestone gate only after the groups listed in the [README](README.md#milestones-and-scope) are
integrated. A passed first milestone does not imply debriefs or experiments are implemented.

## Focused checks during implementation

New tests use these prefixes so each batch has a small reproducible check. These are required
future test names, not claims that they exist in the current repository.

| Group | Required focused command | Existing regression area |
| --- | --- | --- |
| 01 | `cargo test --locked --offline --lib sop_contract_` | Compiler, publication, binding, and samples |
| 02 | `cargo test --locked --offline --lib sop_guidance_` | Agent context/capabilities and authoring/sample routes |
| 03 | `cargo test --locked --offline --lib sop_verification_` | Decisions, action approval, delivery, and uncertain effects |
| 04 | `cargo test --locked --offline --lib sop_coordination_` | Decision authorization/races, waits, and run visibility |
| 05 | `cargo test --locked --offline --lib sop_flow_` | Metrics, capacity, admission, and fair polling |
| 06 | `cargo test --locked --offline --lib sop_improvement_` | Terminal transitions, notifications, and source ACLs |
| 07 | `cargo test --locked --offline --lib sop_experiment_` | Admission replay, binding guards, capacity, and publication |

Before accepting a new filter, use `cargo test --locked --offline --lib PREFIX -- --list` to
confirm it selects the intended tests; record a nonzero executed count. Run the relevant existing
tests named in [workflow release verification](../../docs/workflow-verification.md) alongside the
new tests. Use a migrated PostgreSQL test database: mocks alone do not verify DB behavior.
Unscoped claim/sweep tests use `own_database` per persistence `AGENTS.md`.

For every SQL-bearing batch, run fresh migrations in a disposable test database, regenerate the
offline cache with `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets`, and commit `.sqlx/`.
Re-run affected interface tests after shared migration, compiler, decision, or admission integration.
Broaden checks early when a shared protocol changes; do not run the entire suite after each batch.

## End-to-end acceptance journeys

1. Author the reviewed-invoice SOP with role bindings, guidance, completion criteria, required
   evidence, verification, and bounded rework. Publish/configure through normal workflow routes.
2. Start two independent cases in one thread. Verify model output is structured, failed checks
   block progress, and comments do not settle human work. Compare sample behavior with a controlled
   live run; samples must never produce customer deliveries or external writes.
3. Offer/clarify/accept a human handoff, then race acceptance against completion/cancellation.
   Verify the exact reviewer assignment, stale-command rejection, history, and durable notices.
4. Change a proposal, recipient, or action payload after review. Verify old evidence is rejected.
   Deliver/perform the valid accepted artifact through the real action/delivery path with a
   scripted transport/provider and counted effects. Retain normal protected-action approval.
5. Restart services at durable boundaries: before/after decision submission, handoff acceptance,
   effect dispatch/receipt, capacity reservation, and invitation creation. Prove saved outputs and
   identities survive, committed effects are not duplicated, and unknown effects are not resent.
6. For milestone 2, demonstrate advisory versus enforced WIP, competing last-slot claimants,
   reservation release, poison-batch backoff, and quiet-tenant progress under sustained load.
   Submit a debrief without altering the completed run and triage a proposed future change.
7. For milestone 3, run bounded baseline/candidate cohorts, race for the final pilot place, stop
   new allocation, inspect a frozen report, and explicitly adopt a version for future admissions.
   Existing cases retain their original versions.
8. Across all journeys exercise revoked membership, restricted channels, foreign-company IDs,
   expired waits, duplicate/conflicting command keys, missing evidence, and bounded inputs.

Automate real competing-claimant tests for new protocols. A sequential replay test is additional
coverage, not a substitute. Add the browser journeys to existing workflow authoring, decision,
run, and conversation protocols; exercise stale/closed forms as well as successful submissions.

## Final full-suite gate

Run once after the whole milestone is implemented, before declaring it complete. Use the current
[CI workflow](../../.github/workflows/ci.yml) as the authoritative environment and check inventory:
PostgreSQL 18.6, a freshly provisioned disposable test database, enabled query statistics where required,
locked dependencies cached for offline Rust commands, Node dependencies, and browser binaries.
`DATABASE_URL` must identify that test database. Never point migration/DB tests at a deployment.

```sh
export SQLX_OFFLINE=true
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

Also run CI's decision-browser fixture steps: set `WORKFLOW_DECISION_BROWSER_FIXTURES` to a
temporary directory, run `decision_browser_markup_and_closed_draft_recovery` and
`source_message_outside_window_is_exact_and_authorized` with the focused Cargo command, then run
`node --test scripts/tests/workflow-decisions.mjs`. Add new SOP browser scenarios to this gate and
CI when implemented. Preserve non-root production execution, dependency pins, stack alarms, and
lease/resource budgets. A raised limit must name what still fails early in CI.

For each batch and milestone record revision, environment, command, pass/fail, executed-test count,
relevant fixture/artifact identities, and unresolved findings. Mark an unavailable tool, database,
browser, or network-isolation check explicitly unavailable; do not claim full verification. After
fixes, rerun affected checks, and repeat the full suite when their impact warrants it.

## Human pilot and operating guidance

Use one procedure family and controlled recipients/providers. Have authors and participants find
the next action, owner, required evidence, failed check, handoff state, and escalation path without
implementation vocabulary. Record task time, errors, correction burden, and observed confusion.
For checks and WIP enforcement, collect advisory observations before enabling required behavior.

Update workflow operating guidance with the new supported handoff/exception commands and limits;
until those ship, the current cancellation/replacement guidance remains accurate. Document that
debriefs do not change outcomes, samples confer no authority, quality checks do not replace tool
approvals, and experiment adoption affects only future admissions. Production pilot acceptance
requires its own observed evidence; local automated tests do not establish provider readiness.

## Acceptance

- All required milestone groups and journeys pass, including full-suite and competing-worker gates.
- Documentation describes configuration and validation actually implemented at that milestone.
- No second SOP engine, legacy data path, hidden effect replay, or automatic agent adoption exists.
- Before closing the final milestone, refresh the context graph with `graft build` when available;
  record its absence if unavailable rather than inventing graph evidence.

## Planning rescope check record — 2026-10-25

- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- Local document/source audit using Python: passed for all nine new documents; checked relative
  links and heading anchors, source inventory paths, whitespace, database-policy notes, and startup
  warning registration. Both referring workflow-library documents passed the same link/note checks.
- `rg` search for removed SOP paths: no remaining references; the old directory is absent.
- `graft map` and `graft ask ... --source`: unavailable (`graft` not installed); no graph index was
  present. Scoping used the source inventory and current operating/verification guides.
- Runtime, database, and browser suites were not run for this planning rescope and startup-warning
  path-list update. The implementation gates above remain future requirements, not completed tests.
