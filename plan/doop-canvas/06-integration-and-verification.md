# 06 — Integration and verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on all groups and the completed [file-access acceptance](../fix-file-access/04-verification-and-deployment.md).
These are future implementation checks, not results claimed by publishing the plan.

## Batch 06A — Focused and integrated acceptance

Each batch records exact revisions, commands, nonzero selected/executed counts, results, and
unavailable checks. Prefixes below are new tests to add. List before execution, for example:

```sh
cargo test --locked --offline --lib doop_connection_ -- --list
cargo test --locked --offline --lib doop_connection_
```

| Group | Focused proof |
| --- | --- |
| 01 | `doop_contract_`; actual pinned-Doop MCP probes and managed identity restrictions |
| 02 | `doop_connection_`, `doop_catalog_`; existing credentials, endpoint, MCP publication tests |
| 03 | `doop_create_`, `doop_recovery_`, `doop_agent_`, `doop_sample_`; counted concurrent dispatch |
| 04 | `doop_result_`; real human access/PDF export and affected result/browser tests |
| 05 | `doop_lifecycle_`; rotation/revocation races and disposable deployment/restore |
| 06 | `doop_e2e_`; entire two-company workflow and the real Doop acceptance matrix |

Use real PostgreSQL and `own_database` for unscoped workers/claims or whole-table assertions.
Missing database/browser/Doop dependencies are unavailable checks, never silently skipped passes.
If SQL changes are actually required, update the init schema, migrate a fresh disposable database,
and regenerate `.sqlx` metadata. Avoid schema changes when existing owners already hold the data.

Integrated acceptance must cover:

1. A/B each create, edit, reopen and export a document using separate bot credentials at the same
   Doop endpoint. Human A/B see only their authorized private work; Doop operators remain trusted.
2. Foreign connection, workspace, canvas, frame and asset IDs cannot cross boundaries. Broad bot
   invitations, public sharing, forged URLs, and tool-returned credential selectors are rejected.
3. Two workflow workers contend for one create; restart/resume reuses a known result. A response
   lost after remote acceptance remains reconcilable/uncertain without a blind duplicate create.
4. Revocation, rotation, company membership changes, approvals, cancellation and lease loss retain
   existing final-dispatch guarantees. Changed tool schemas require review rather than adaptation.
5. The agent cannot expand tool access or publish a success for failed, oversized, partial,
   conflicted or unreviewed work. Samples cannot mutate the actual remote service.
6. Ordinary workflow/channel/result behavior and source-data access levels remain intact.
7. Real browsers and renderers load private images/fonts and export PDFs after the access fix;
   signed-out/foreign clients fail even with known IDs, warm caches and conditional requests.

## Batch 06B — Final Openbot full-suite gate

Run once after the whole implementation is integrated. Reconcile this inventory against
[CI](../../.github/workflows/ci.yml) at implementation time. Provision locked dependencies,
SQLx CLI, Chromium, and isolated PostgreSQL 18.6 with CI's query-statistics setup and parameter
logging disabled; do not alter a shared server. Run frontend dependency install and `cargo fetch
--locked` before offline/network-isolated checks.

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

Also run CI's image/knowledge memory, query-statistics activation, HTML/team-autocomplete,
knowledge-library, session-revocation, and decision browser gates with CI's exact fixture
generation/environment. Add new integration browser/fixture checks to CI rather than counting
a missing fixture as a pass. Preserve stack/lease budget signals when limits change.

## Batch 06C — Pinned external-service release gate

Run the separate Doop/fix-file-access full gate on the accepted Doop revision, then exercise the
real Openbot-to-Doop workflow, humans, private media and PDF export against that same image.
Use only disposable data until confidentiality passes. Real model credentials, if required for
a final live agent run, must be explicitly available for that test; record unavailable inference
instead of presenting scripted-provider acceptance as live inference.

The release evidence names both revisions/images, identities without secrets, executed counts,
concurrency results, confidentiality results, and limitations. Do not claim full verification if
either required service gate is unavailable. Re-run affected checks after fixes and repeat the
full gate when impact warrants it. Refresh graft after substantial implementation and keep every
plan file registered in the startup reset-note list.
