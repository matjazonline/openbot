# 07 — Integration and verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on all preceding groups. Prove company-scoped control, shared Cloudroom UI/session
behavior, safe uncertainty, and reproducible macOS/Linux operation. These are future implementation
requirements; publishing this plan does not claim any runtime integration has been tested.

## Batch 07A — Integration and failure acceptance

Add deterministic `cloudroom_e2e_` tests using real PostgreSQL/workflow workers, a real WebSocket
transport and a counted SDK fixture. Separate those from tests against the actual Cloudroom build
and logged-in native providers. Cover:

1. Pair once, advertise providers/projects, create a local profile, start a mapped thread, follow
   up, observe results and stop/archive from Openbot. The actual Cloudroom UI sees the same thread.
2. Two companies cannot share an installation or read/control one another's machine, session,
   operations or private workflow output. Authentication/database failures remain distinct.
3. Two claimants race for one pairing code; two Openbot replicas race for one connection; two
   workflow workers and duplicate plugin deliveries race for one logical invocation. Assert actual
   SDK effect counts, durable results and rejection of stale ownership after replacement.
4. Crash before/after command admission, socket write, local receipt, dispatch marker, SDK
   acceptance, mapping persistence, terminal outbox write, server receipt and workflow wake-up.
   Reconstruct both peers. Work is reconciled once or visibly uncertain; it is never blindly sent
   again because an acknowledgment was lost. Same ID/different payload always conflicts.
5. A human sends messages and answers permission prompts in Cloudroom while Openbot runs. Results
   stay correlated to the originating turn. Idle/latest-text notifications cannot settle another
   turn. Human edits/stops/deletes and local provider failures are accurately represented.
6. Disconnect, laptop sleep, plugin reload, server deploy and revoked credentials affect only the
   documented admission/connection state. Running work is not falsely declared stopped. No stale
   connection can clear or accept data for its replacement. Missed notifications do not lose work.
7. Completion races cancellation/deadline, changed authority and workflow lease loss. Late output
   is retained as appropriate but cannot revive a terminal workflow or publish twice. A broad
   thread stop is only explicit; no unsupported targeted-cancellation claim appears in the UI.
8. Malformed/oversized frames, prompts, output chunks, catalogs, queues and local journals produce
   bounded refusal/backpressure. Slow consumers, output gaps, missing mappings and journal loss
   produce honest recovery states without unbounded allocation, disk growth or hot reconnect loops.
9. Native `agent.run`, classifications/decisions, shared action approvals, message publication,
   company access and workflow samples retain their current contracts. Local runtime output
   cannot grant new tools, access unrelated channel history or escape sample execution boundaries.
10. Packaging pins the tested Cloudroom/SDK/provider versions, keeps its API loopback-only, uses a
    non-root account, and works without Electron/display on Linux. The Mac UI and headless launcher
    use one profile; backup/restore and uninstall preserve local work without automatic replay.

## Focused checks by group

Each implementation batch records commands, selected/executed counts, environment and results.
Use `cargo test --locked --offline --lib PREFIX -- --list` before the focused execution, and reject
zero matches. The prefixes are new test requirements, not existing tests asserted by this plan.

| Group | New focused Rust prefixes | Additional proof |
| --- | --- | --- |
| 01 | `cloudroom_contract_`, `cloudroom_bounds_` | Pinned public-SDK compatibility probes |
| 02 | `cloudroom_pairing_`, `cloudroom_company_`, `cloudroom_revocation_` | Competing claimants and tenant-qualified schema |
| 03 | `cloudroom_connection_`, `cloudroom_protocol_`, `cloudroom_commands_`, `cloudroom_observations_` | Real WSS, two replicas, duplicate/lost acknowledgments |
| 04 | Rust protocol regressions from 03 | Plugin typecheck/build/tests, counted SDK fixture, reload/cut-point recovery |
| 05 | `cloudroom_management_`, `cloudroom_workflow_`, `cloudroom_publication_`, `cloudroom_samples_` | Registry/wait/sample regressions and browser forms |
| 06 | `cloudroom_retention_`, `cloudroom_lifecycle_` | Packaged macOS/Linux services and restore/redaction checks |
| 07 | `cloudroom_e2e_` | Actual Cloudroom build/UI and native-provider matrix |

Use `own_database` for unscoped claiming/cleanup and global assertions. Migrate a fresh disposable
database and regenerate `.sqlx` after SQL changes. Do not point concurrent worker tests at shared
development data or permit missing-database skipping. Broaden earlier at shared workflow/auth seams,
but reserve the full suite for the integrated final gate rather than repeating it after each group.

## Batch 07B — Final full-suite gate

Run once the entire implementation is integrated. Reconcile this list against
[CI](../../../.github/workflows/ci.yml) at implementation time and record exact revision, tool/runtime
versions, commands, counts and unavailable checks. Provision locked dependencies, SQLx CLI,
Chromium and isolated PostgreSQL 18.6 before offline/network-isolated checks. Use CI's query
statistics configuration with parameter logging disabled; do not alter a shared database server.

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

Also run CI's production image/knowledge memory gate, query-statistics activation assertions,
HTML/team-autocomplete, knowledge-library, session-revocation and decision browser fixture gates.
Generate their fixtures with the exact CI environment variables and Rust fixture commands; a
missing fixture does not count as a skipped passing browser test. Include the new local-agent
management browser protocol and keep its required checks in CI.

For the implemented plugin package, require scripts named `typecheck`, `build`, and `test`; run:

```sh
npm --prefix integrations/cloudroom-plugin ci
npm --prefix integrations/cloudroom-plugin run typecheck
npm --prefix integrations/cloudroom-plugin run build
npm --prefix integrations/cloudroom-plugin test
```

These commands become runnable only after phase 04 creates the package. Its tests use a local
fake Openbot server and public-SDK fixtures without provider credentials. Add them to CI. Real
provider acceptance remains separate from the network-isolated suite and must never run against
ordinary development workspaces.

## Batch 07C — Platform/provider acceptance and release

Run one disposable native task, follow-up, output recovery after restart and stop for each
provider (`claude-code`, `codex`, `acp-opencode`) on macOS and headless Linux. Verify its UI thread
and SDK command/turn identities, provider login failure, permission wait, and local file access.
Use accounts/projects explicitly authorized for those real-inference checks; record unavailable
credentials/platforms rather than claiming the whole matrix passed. Source inspection and fake
wire fixtures cannot establish logged-in execution or released-artifact compatibility.

Release requires no unresolved cross-company disclosure, blind duplicate effects, stale-owner
completion, false workflow success/stop, unbounded external work, or broken sample boundary.
Document uncertain outcomes and offline-stop limitations in both interfaces. Re-run affected
checks after final fixes; repeat the full suite when impact warrants it. A raised limit must keep
an early-failure CI signal; preserve stack and lease budget checks.

Finish with tested operator/user documentation, the pinned compatibility manifest, artifact
integrity checks, cleanup/rollback instructions and accurate verification evidence. Breaking local
state/protocol changes follow the reset/re-pair policy, not compatibility shims. Refresh `graft`
after substantial implementation. Keep all plan files registered in the startup reset-note list.
