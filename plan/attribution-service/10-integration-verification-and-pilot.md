# 10 — Integration verification and pilot

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [09](09-operations-retention-and-separate-deployment.md) and all preceding phases. Prove
the full tracking-to-sale flow, the application boundary, account access, and operational behavior
with real PostgreSQL and the selected first merchant integration. Saving this plan does not imply
that any implementation or integration verification below has happened.

## Batch 10A — Cross-application and commerce acceptance

Add `attribution_e2e_` acceptance tests and browser journeys covering:

1. Start two independently built applications with distinct databases/roles, sessions, secrets,
   assets and origins. Login centrally, enter attribution, select an authorized company and create
   a store/key/link. Account/company/member administration remains in Openbot.
2. Click Ad A then Ad B, observe the landing page with permitted consent, associate the latest
   eligible click with a server-controlled checkout, pay for the advertised and another product,
   and reconcile both products to Ad B. Later clicks cannot reassign that order.
3. Deliver simultaneous duplicate paid events, different IDs for the same paid order, partial/full
   refunds, cancellation, refund-before-payment, and a conflicting replay. Assert one paid order,
   exact refund amounts, immutable attribution and visible reconciliation failures.
4. Exercise missing/expired/foreign/bot click evidence, no consent, blocked browser tracking,
   multiple currencies, period boundaries/DST, zero-click conversion rates, and filtered product
   reports. Totals and labels must match the phase 08 contract.
5. Try every detail/mutation/report/export with another company's IDs, including an actor who
   legitimately belongs to both companies. Try another store's IDs with a merchant key. Public
   installation IDs and browser events cannot authorize sales or management.
6. Revoke a key, downgrade/remove membership, disable an account, suspend/delete a company, and
   replay delayed lifecycle/provisioning messages. Verify local sessions cannot retain stale access,
   and machine/public access stops within the documented company-status bound.
7. Interrupt database commits, ingestion responses, reconciliation workers, company lifecycle
   delivery and cleanup. Verify durable retry behavior and competing-claimant protection with
   actual parallel transactions/processes. Do not substitute sequential mocks for race tests.
8. Move attribution's running artifact to the separate-host topology and deny cross-database
   access. Repeat login, link creation, real merchant sandbox purchase/refund and reporting. Build
   the extracted directory without any parent Openbot files; do not hide a shared library or proxy.
9. Verify production container UID, TLS/proxy trust, request/rate/body limits, CSRF, CSP/escaping,
   secret redaction, retention, restored backups, and the agreed peak-load/latency/error targets.

Focused checks: run `attribution_e2e_` and affected browser/contract suites while resolving these
cases; record exact commands, counts, fixtures, topology, outcomes and any sandbox limitations.

## Batch 10B — Final full-suite gate

Run once after the complete implementation is integrated. Reconcile commands against the current
[Openbot CI](../../.github/workflows/ci.yml) and the service CI added in phase 02. The existing
[local check script](../../scripts/check.sh) does not currently cover every CI gate. Prepare locked
dependencies, browser binaries and SQLx CLI before offline checks; use disposable PostgreSQL
databases and CI's PostgreSQL/query-statistics setup. Never reset a development/customer database.

For Openbot, from the repository root, run the complete current CI workflow, including:

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

Also execute CI's query-statistics activation assertions and fixture-backed HTML/team-autocomplete,
session-revocation and workflow-decision browser gates; create their fixtures as CI requires rather
than running browser tests against absent/stale artifacts. Preserve every existing gate.

For attribution, run its service-local verification entrypoint from `services/attribution/`, with
its own disposable `DATABASE_URL`/test configuration and offline metadata. It must include:

```sh
cargo fmt --all -- --check
cargo check --locked --offline --all-targets
cargo clippy --locked --offline --all-targets -- -D warnings
SQLX_OFFLINE=false cargo sqlx migrate run
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets
cargo test --locked --offline --all-targets --no-run
cargo test --locked --offline --all-targets
```

Add the service's actual browser/asset checks, OpenAPI validation, boundary/extracted-copy test,
container test, network-isolated database suite, two-application contract suite, restore test and
representative-load gate. Record their concrete command names when implemented; do not advertise
future scripts as existing commands. Root Openbot commands alone never verify attribution.

For both applications, set `SQLX_OFFLINE=true` for compilation/tests; use live mode only for fresh
migrations/metadata generation and checking. Regenerate each application's own `.sqlx/` cache after
SQL changes. Each database role/test pool is isolated; global claim/sweep tests use dedicated
databases. No missing-database skipping or zero-match focused filter can count as a pass.

Record revisions, environment, commands, selected/executed counts, outputs, latency data and any
unavailable checks in this file. After final-verification fixes rerun affected checks; repeat the
full suite when their impact warrants it. Refresh graft after substantial implementation and
inspect the changes. Keep every plan file registered in Openbot's startup warning list.

## Batch 10C — Pilot and completion criteria

- The agreed first store completes a purchase and refund through the real supported integration;
  generic test payloads alone do not satisfy the commerce pilot.
- Reports reconcile with merchant order/line/refund facts, and remaining unmatched observations
  have explicit reasons. Platform impressions/spend remain separately labeled future imports.
- Phase 01's consent, retention, traffic, latency and failure decisions are recorded and met.
  Demonstrate restore and verify secret rotation/revocation procedures.
- Attribution is deployable and buildable independently, with its own UI and data. Its documented
  Openbot identity/company dependency is the only required application integration. There are no
  shared application modules, tables, cookies, session/merchant secrets or frontend assets.
  Cross-application API credentials are dedicated to their explicitly scoped integration.
- No unresolved cross-company access, lost acknowledged event, duplicate sale/refund, mutable
  attribution, unbounded processing or hidden source/deployment dependency remains.
- Additional commerce platforms, ad metrics imports, cohort reporting, currency conversion,
  ClickHouse and independent account-service extraction stay out of this release.

## Planning verification record

This section records checks of the plan files and required startup-warning registration only.
Implementation, database, browser, provider and performance gates above remain future work.

### 2026-11-04 — Plan creation

- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- `python3` inline document audit: passed; ten consecutively numbered phase files, all 35 local
  Markdown links resolve, every file has the database-policy note and focused/final verification
  guidance, every file is registered exactly once in `NO_BACKWARD_COMPATIBILITY_NOTES`, and new
  files have clean whitespace.
- Source scope is ten required startup-warning paths in `src/main.rs`. No application feature,
  dependency, schema, deployment or database reset was implemented.
- Compilation, database-backed, browser, provider and load suites were not run for this
  documentation/path-list change. The implementation gates above are requirements for future work,
  not claims of completed verification.
