# 07 — Provider portability, verification, and operations

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on every earlier stage. Prove provider portability and the future service contract as
well as the reusable vault and Google lifecycle through actual runtime composition. Document
how users and operators connect and recover accounts and what service extraction still requires.
These are future implementation gates; saving the plan does not mean they have run.

## Batch 07A — Provider portability and service-contract proof

Add integration_portability_ and integration_service_contract_ test families. Extend the same
registry and gateway with a static API-key app fixture and a test-only non-Google OAuth adapter.
Use a workspace/installation identity with no email, custom scope-to-capability mapping, a fixed
non-Authorization API-key header for the static app, and strict rotating refresh tokens for the
OAuth fixture. These are tests of extension contracts, not production integrations with named
vendors. Production registry/configuration must not enable test providers.

Required portability cases:

1. Static bearer, API-key header, Google, and the OAuth fixture use the same account/grant/gateway
   contracts and unchanged HTTP/MCP workflow syntax; no vendor branch appears in shared handlers.
2. Two app definitions may use the same permitted external authorization and credential lock,
   while separate use grants constrain destination and capabilities. A fixture second Google
   app does not claim production Calendar support. Missing scopes require explicit consent and
   invalidate older authorization snapshots rather than silently upgrading access.
3. The fixture's workspace identity, multiple external accounts per owner, and different client
   namespaces remain isolated without requiring verified email or Google's uniqueness tuple.
4. Real competing resolvers exercise 05's strict rotation protocol. A lost response, cancellation,
   or crash after durable pending state cannot cause reuse of a potentially consumed refresh
   token. Reconnect/revoke races deny a delayed claimant and cannot resurrect old grants.
5. Unknown providers/auth methods, mismatched payload schemas, changed injection/destination,
   invalid host authority, and foreign account/grant references fail before remote dispatch.

Define bounded, versioned wire projections of the public gateway commands/results. Round-trip
them through an in-memory serialization transport and run the same authority, stale-version,
pre-dispatch/uncertain-outcome, deadline, and redaction scenarios as the local adapter. Runtime
cancellation uses a local execution context; the wire projection carries a deadline and opaque
cancellation/dispatch identity, never a Tokio token or host lease. Unknown protocol versions fail
closed; the reset policy still forbids compatibility decoders or staged payload support.

Use source/dependency boundary checks to assert that host consumers do not read integration
tables, the module's public DTOs do not import Openbot session/workflow/database types, and
credentials/private resolver results cannot be serialized into gateway responses. Include
sentinel credential values in scripted failure responses, logs, receipts, and fixtures to verify
redaction. Register these checks in CI using actual implemented commands; do not invent an
already-existing checker in documentation.

This proof does not implement a public HTTP listener or production RemoteIntegrationGateway.
The remote host authority adapter and full distributed dispatch/reconciliation protocol remain
the explicit extraction gate below, not guarantees supplied by serialization alone.

Focused checks, after listing each new family to establish nonzero counts:

~~~sh
cargo test --locked --offline --lib integration_portability_ -- --list
cargo test --locked --offline --lib integration_portability_
cargo test --locked --offline --lib integration_service_contract_ -- --list
cargo test --locked --offline --lib integration_service_contract_
cargo test --locked --offline --lib integration_renewal_
scripts/transport-boundary-check.sh
python3 scripts/workflow-sample-boundary.py
~~~

## Batch 07B — Integrated journeys and recovery

Add vault_google_e2e_ tests exercising real persistence, application ports, HTTP/MCP runtimes,
action approvals, and counted scripted Google/business endpoints.

Required journeys:

1. Save a static secret, bind it with owner consent, and reuse it in two workflows. Another user
   and an unapproved company cannot resolve it. Company admins cannot read it.
2. Authorize a mailbox whose address differs from the Openbot login, bind it, read a thread,
   and prepare a model response. Independently send a reviewed supplied MIME message through
   HTTP, recording the provider message identity and correct thread association.
3. Execute two runs through different HTTP/MCP connections referencing the same expired entry.
   Separate local gateway instances/pools compete and produce one Google token refresh. A valid
   cache produces none. This is not evidence that a remote service has been deployed.
4. Let a workflow wait for review until its original token has expired. On resumption, resolve a
   fresh token without losing valid approval of the same business operation/account.
5. Disconnect, remove membership, rebind the account, or revoke an operation while a run is
   stalled before dispatch. Current authority blocks the disallowed external call.
6. Crash/cancel while waiting for a vault lock, during token exchange, after token response but
   before commit, and after business dispatch. Recovery neither strands the lock nor invents
   a safe retry for an uncertain send.
7. Rotate keys while refreshing/replacing entries, then retire an old key. Every supported vault
   kind remains readable under its own immutable context, with no stale ciphertext overwrite.
8. Delete a company or account with bindings. Correct dependent references are disabled/removed;
   unrelated private entries and other authorized companies retain their proper ownership.
   Interrupt teardown after host denial and verify durable cleanup intent survives restart.
9. Invoke direct generic tools, nested model tools, and sealed samples. These perform zero
   production vault resolutions, Google exchanges, or unintended remote calls.

Use real competing claimants and deterministic barriers for lock/CAS/revocation cases. Use
test_pool only for row-scoped tests; use own_database when sweeping queues or asserting whole-table
state. Never make a live Google account or external network access a CI dependency.

Focused checks for 07B are vault_google_e2e_, affected gateway/authority families, and boundary
scripts. List new filters before running, record nonzero counts, and apply 02's SQL checks after
query changes. Complete 07C documentation and its structural checks before the final gate in 07D.

## Focused verification and database setup

Future new test families, listed before execution to prove a nonzero match:

| Stage | Focused new families |
| --- | --- |
| 01 | integration_contract_, vault_contract_, vault_authority_ |
| 02 | vault_storage_, vault_management_, vault_rotation_ |
| 03 | vault_binding_, vault_dispatch_authority_ |
| 04 | integration_connect_, google_oauth_, google_vault_callback_ |
| 05 | integration_renewal_, google_refresh_, google_connection_lifecycle_ |
| 06 | integration_gateway_, vault_workflow_http_, vault_workflow_mcp_, vault_authoring_ |
| 07 | integration_portability_, integration_service_contract_, vault_google_e2e_ |

Every SQL-bearing batch edits the init schema, uses a freshly created disposable database,
runs migrations, regenerates SQLx metadata, and executes the runtime SQL tests. Never mutate a
development/production database for these checks. Export TEST_DATABASE_URL explicitly and do not
enable ALLOW_MISSING_DATABASE_URL.

Local setup example, after provisioning a disposable PostgreSQL instance and suitable local role:

~~~sh
vault_plan_db="open_bots_vault_google_$(date +%s)_$$_test"
createdb "$vault_plan_db"
export DATABASE_URL="postgres://$(whoami)@localhost:5432/$vault_plan_db"
export TEST_DATABASE_URL="$DATABASE_URL"
export SQLX_OFFLINE=true
SQLX_OFFLINE=false cargo sqlx migrate run
SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets
~~~

The final CI-equivalent environment uses PostgreSQL 18.6 and the query-statistics settings in
[CI](../../.github/workflows/ci.yml). Configure a dedicated instance; do not change a shared
server's settings. Cache locked Rust/Node dependencies and browser binaries before offline or
network-isolated checks. Clean up only the explicitly created test resources after recording
results and confirming no process still uses them.

## Batch 07C — Operator/user documentation and future extraction gate

Document actual shipped configuration and route behavior. Reuse the currently read Google client
ID/secret pair and existing credential encryption key configuration; new keys require implemented
startup validation before they appear in deployment examples.

Document how to add a provider/app: implement its typed adapter and authorization identity key,
declare auth/config/capability/destination/injection and renewal policies in the registry, add
contract/contender tests, and expose only the resulting metadata in owner UI. Explain the
separation of Openbot login, external authorization, private account custody, and company grants.
Supported production methods are static bearer, registered static API-key header, and Google
OAuth; test fixtures do not expand that support list. Existing connection operation registration
remains authoritative; a Composio-style action catalog, triggers, and general agent tool exposure
are separate future work.

Cover callback registration, test users, consent scopes, Workspace-admin app controls, account
versus company binding ownership, key escrow/rotation, private metadata, provider errors, local
disconnect versus Google revocation, and reconnect recovery. Explain that Google's external
Testing mode limits these refresh grants to seven days, and do not promise permanent grants.
[Google token lifecycle](https://developers.google.com/identity/protocols/oauth2#expiration).

Public launch documentation must identify restricted-scope verification and any applicable
server-side security assessment as provider prerequisites, not startup checks.
[Google scope and verification requirements](https://developers.google.com/workspace/gmail/api/auth/scopes).
Describe which mailbox data reaches model steps, retention/deletion behavior, and compliance with
[Google's Workspace user-data policy](https://developers.google.com/workspace/workspace-api-user-data-developer-policy).

Expose sanitized state and bounded observations for cache hits, refresh attempts/outcomes,
lock timeouts, grant revocations, and reconnect-required entries. No token bytes, ciphertext,
full requests, or mailbox contents belong in those observations.

A separately authorized live smoke check may connect personal/Workspace test mailboxes and send
only to explicitly approved recipients. Record real provider acceptance metadata without secrets.
If credentials, admin approval, preview enrollment, or public verification are unavailable, report
that limitation explicitly; scripted CI results are not evidence of live provider approval.

Document a future extraction checklist, without deploying or implementing the remote server now:

1. Move integration account/grant/attempt persistence, cipher/key rotation, provider registry,
   renewal coordination, credential injection, and authenticated HTTP/MCP sessions/transports
   together. Keep Openbot login, company/workflow/action/approval state in Openbot. Replace only
   the host-facing gateway composition after the remote protocols pass the same conformance cases.
2. Authenticate service clients and bind each to an owner namespace. Specify bounded protocol
   versions, TLS, credential/key custody, operator access, request/result retention, and service
   limits. Never introduce a token-export API to keep old transports working.
3. Implement a required remote DispatchAuthority adapter against live host state and bind its
   decision to the exact action/request/grant versions. Specify the dispatch linearization and
   revocation acknowledgement protocol, including partitions and racing membership/grant changes.
   Long-lived signed tickets and cached membership cannot replace live checks. Authority/service
   outages fail closed before sending; already dispatched effects may still complete.
4. Make grant updates and host user/company/membership teardown explicit, idempotent commands.
   Use durable outbox/inbox or equivalent recoverable coordination where database atomicity ends.
   Test crash/partition recovery and denial while reconciliation is pending; do not assume shared
   tables, foreign keys, or distributed transactions provide the protocol.
5. Preserve host action reservation, cancellation/lease loss, and receipt reconciliation across
   the network. Persist/reconcile dispatch identity and observed outcomes so a lost gateway
   response cannot prompt an unsafe second send. Bound cancellation acknowledgement and session
   cleanup; a request ID alone is not external idempotency or exactly-once execution.
6. Run protocol conformance and actual multi-process/partition/competing-dispatch tests before
   enabling RemoteIntegrationGateway. Provision a non-root service image and CI/deployment gates
   for both processes. Reset affected schemas/payloads when needed; do not add dual reads/writes,
   token-export bridges, data backfills, or staged compatibility routes.

For 07C, verify local Markdown links, the reset notice and startup-note inventory for every plan
file, registry/configuration claims against shipped code, and the final CI command inventory.
Record actual commands/results. No implementation gate is considered passed merely by writing it.

## Batch 07D — Final full-suite gate

Run once after all stages are integrated. Reconcile the current CI file first: scripts/check.sh
currently omits some CI/browser/lease gates and defaults migrations to a development URL.
It is not sufficient alone. Include all integration families and the module/service-contract
boundary checks added by this plan in CI's inventory. Finish documentation before running this
final gate so it validates the completed implementation.

With the disposable database variables above:

~~~sh
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
~~~

Execute CI's query-statistics activation assertions and all fixture-backed browser gates as well:

~~~sh
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
~~~

Add focused browser assertions for the new Vault owner forms, grant consent, and reconnect states.
Record the actual commands rather than advertising a future script before it exists. If a resource
bound must increase, retain an early-failure test for approaching it in CI. Do not raise the stock
stack-budget threshold merely because this new call chain overflows; shrink the future/frame chain.

## Completion evidence

Record the implementation revision, environment, migrations/metadata results, each command and
exit status, nonzero test counts, concurrency outcomes, frontend/browser results, and unavailable
checks. Rerun affected checks after fixes; repeat the full gate when their impact warrants it.

Acceptance: a private reusable vault, owner-authorized workflow connections, automatic serialized
Google renewal, and manual read/prepare/send examples work together. Credentials remain internal
to the runtime, routine renewal preserves valid business approvals, revocation blocks later
dispatch, and the complete required verification gate has passed. Non-Google/static portability
and wire-contract checks pass without provider-specific workflow branches or token export.
LocalIntegrationGateway is the shipped composition; remote deployment remains explicitly deferred.
