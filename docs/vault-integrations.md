# Private integration accounts and Gmail operations

Openbot ships `LocalIntegrationGateway`: account custody, consent, renewal and
authenticated HTTP/MCP transports run in the application process. Production
methods are static bearer, the registered static API-key header, and Google OAuth
for Gmail HTTP. The workspace OAuth, second Google app and custom API-key app in
tests are extension fixtures. They do not enable production vendors or Gmail MCP.
The registered API-key app permits HTTP only; MCP consent is rejected before a
session opens. Static bearer and separately registered OAuth MCP app fixtures
exercise the same workflow syntax without adding a provider-specific handler.

## Ownership and connection use

Open `/ui/profile/vault` while signed in. Save a static credential or connect
Gmail. The external mailbox can differ from your Openbot login; Google subject
and OAuth client identify its authorization, rather than matching login email.
Names and optional addresses are private metadata. Forms accept credentials but
never return them; existing secret inputs remain blank. A company administrator
cannot browse another user's private accounts or recover their token.

Connecting an account grants no company access. In an HTTP/MCP connection's
credential-source page, choose the app, review the company and exact destination,
and confirm use with your own account. The grant is workflow-only and constrained
by the registered app and connection operations. Review the selection again after
a connection or account authorization changes. One account can support several
consented connections and companies without copying its credential.

Use [the Gmail read/prepare and reviewed-send examples](gmail-workflows.md).
The read operation and separately protected send operation should be narrowly
registered before consent. Business approval covers the frozen operation,
arguments and account authorization; routine access-token renewal preserves that
approval. Changing consent, account, operation or membership does not.

Direct generic agent tools, nested model tools and sealed samples cannot borrow
workflow vault grants. Samples use scripted responses without contacting Google.
Operation registration remains authoritative. A general action catalog, triggers
and arbitrary agent access to private accounts are separate future work.

## Configuration and Google setup

Configure secrets through your deployment's secret store, not checked-in files.

| Key read by the application | Behavior |
| --- | --- |
| `GOOGLE_OAUTH_CLIENT_ID`, `GOOGLE_OAUTH_CLIENT_SECRET` | Both nonempty or both absent; a partial pair fails configuration. The same pair is reused for Google login and integration authorization. Absence leaves Gmail connection unavailable. |
| `APP_DOMAIN_NAME` | Integration callback base. An explicit URL is used as supplied; otherwise local hosts use HTTP and other hosts HTTPS. Startup rejects an unsafe integration callback URL. |
| `CREDENTIAL_ENCRYPTION_KEYS` | Comma-separated positive `version:base64-key` entries; each key must decode to 32 bytes. Empty entries, duplicate versions and invalid keys fail loading. |
| `CREDENTIAL_ENCRYPTION_ACTIVE_VERSION` | Positive version present in that key ring; new encryption uses it. |
| `AUTH_ATTEMPT_SECRET` | Independent authentication-attempt signing secret, at least 32 bytes and different from `JWT_SECRET`. Keep it stable when rotating session signing keys. |
| `RUST_LOG` | Application tracing filter. Native MCP SDK targets are additionally suppressed to prevent raw protocol debug records from exposing peer content. |

For a deployment with `APP_DOMAIN_NAME=https://openbot.example`, register the
exact redirect `https://openbot.example/auth/integrations/google/callback` on the
Google web OAuth client. This is distinct from the login redirect
`/auth/google/callback`; register both if using both flows. Development HTTP
callbacks are limited to the accepted local hosts. Do not add a separate,
unimplemented integration-client environment variable.

Enable the Gmail API and configure the consent screen with the requested scopes
and approved test users. The Gmail app maps `gmail.read` to `gmail.readonly` and
`gmail.send` to `gmail.send`, alongside identity scopes. Missing consent is shown
as missing permissions; reconnect explicitly to request it. Cache renewal cannot
silently broaden scopes. Workspace administrators may block the OAuth client or
requested access; arrange approval through the organization's app-access controls.

Google external apps in Testing mode normally receive refresh tokens that expire
after seven days for these Gmail permissions. Expiry, revocation and provider
policy can require reconnect; this is not a permanent grant.
[Google token lifecycle](https://developers.google.com/identity/protocols/oauth2#expiration).

Gmail readonly is restricted and send is sensitive. Public launch must satisfy
Google's applicable OAuth verification and, for server handling of restricted
data, security-assessment requirements. These are provider prerequisites, not
checks performed by Openbot startup.
[Gmail scope requirements](https://developers.google.com/workspace/gmail/api/auth/scopes).

Startup validates local configuration and registered policy. It does not prove
Google client ownership, consent-screen publication, Workspace admin approval,
scope verification or live provider availability. Scripted CI acceptance proves
none of those external approvals. A live smoke check requires separate
authorization, test mailboxes and explicitly approved recipients; record only
sanitized provider acceptance IDs. No live-account check is needed for CI.

## Recovery and observations

Owner pages expose bounded states such as ready, disconnected and reconnect
required. Callback failures distinguish denial, expiry, missing permissions,
missing refresh access, identity mismatch, configuration and admin blocking;
provider error bodies are not displayed. Retry a temporary failure after its
cooldown. For reconnect-required accounts, reconnect the intended mailbox and
explicitly confirm new connection use when the old authorization was invalidated.
An already consumed callback must start a new connect attempt.

Disconnect removes local usable credentials and invalidates use grants. It does
not call Google's revocation endpoint or remove access from other applications.
Revoke access in the Google account's connected-app controls when that is the
intended outcome, then reconnect explicitly if needed. A local deletion removes
the private account and dependent use grants; it does not retract a message that
was already sent. Company/member teardown first denies host authority and records
durable cleanup intent so interrupted cleanup can resume.

Tracing target `open_bots::integrations` emits only fixed `event`/`outcome` labels:
`cache_hit`, `refresh_attempt`, `refresh_outcome`, `lock_timeout`, `grant_revoked`,
`reconnect_required`, and `resolution_failed`. Refresh `response_received` means
only that a response was validated far enough to continue; `committed` is emitted
after the local write is confirmed. Neither acknowledges business dispatch.
Outcomes include cancellation, cooldown, pending rotation and ambiguous commit.
There are no account/provider dimensions, token bytes, ciphertext, full requests,
mailbox contents or arbitrary provider errors in these lifecycle observations.
Do not turn HTTP/MCP wire logging on to diagnose a credential failure.

Refresh coordination uses the private account lock across gateway pools. Strict
rotating providers additionally commit pending state before exchange: an uncertain
response never licenses reuse of a potentially consumed refresh token. Reconnect
may be required after interrupted rotation. A lost business response is an
uncertain effect; inspect the native action receipt and reconciliation state
before considering another send. Request IDs do not supply provider idempotency.

## Data custody, model processing and retention

Credential envelopes are encrypted with immutable account/owner/provider/schema
context. Keep key escrow separate from database backups and restrict operator
access to both. The application must decrypt at dispatch time; database
encryption does not prevent a privileged process/key custodian from doing so.

Mailbox data returned by a workflow is ordinary workflow output, not a vault
secret. In the read/prepare example only the selected first message snippet and
IDs are bound to the configured agent; tokens never enter its prompt. Broader
workflow bindings can disclose more mailbox content to that model provider.
Review those bindings and the provider's data-use terms before authorizing a
mailbox. Use Workspace data only for disclosed, consented user-facing features;
do not use it to train general-purpose models, sell it or use it for advertising.
Meet applicable access, security, deletion and Limited Use requirements.
[Workspace user-data policy](https://developers.google.com/workspace/workspace-api-user-data-developer-policy).

Run outputs, action results, conversation/history records and database backups
may retain the selected content after execution. Disconnecting or deleting a
vault account does not purge those host records. This feature adds no automatic
mailbox-output TTL or provider-side deletion. Operators must document their
retention window, model-provider handling, access restrictions, backup expiry
and deletion procedure, and apply authorized deletion to host records as well as
private accounts. Do not promise that revoking a token deletes prior model input.

For key rotation, distribute old and new key versions to every writer, activate
the new version, run `open_bots credentials rotate`, then require
`open_bots credentials status --require-version NEW_VERSION` to succeed. Resolve
unavailable/malformed rows and CAS conflicts before retirement. Retain old keys
for the backup/recovery window, then verify before and after removing them. See
[the existing rotation helper](../scripts/credential-key-rotation.sh); it deploys
Fly Machines and must only be invoked with deployment authorization. Status and
rotation include private integration accounts through their module inventory.

## Adding a provider or app

Implement a typed credential adapter with schema/payload validation, injection
policy, capability checks and the required cache-update validator. Define an
authorization identity key appropriate to its client/workspace/installation;
do not require email when the provider does not. Register provider, auth config,
app capabilities, destinations, injection and renewal/session/revocation policy
together. Select reusable or strict rotating renewal from the provider's actual
contract. New consent facts require explicit authorization, not cache updates.

Add malformed-method/schema/destination, scope drift, foreign-owner, redaction,
real competing-resolver, cancellation and ambiguous-response tests. Exercise
HTTP/MCP through the same gateway and syntax. Expose only registered sanitized
metadata in owner UI; never add a token-return API or provider branches to shared
workflow handlers. The extension fixtures demonstrate custom scopes, a workspace
identity with no email, different client namespaces and a fixed non-Authorization
API-key header. They remain test-only.

## Service extraction gate

The bounded version-1 in-memory wire proof is not a deployed remote service.
Its 128 KiB frame limit can reject a larger locally valid catalog/page; callers
receive an explicit limit failure. It serializes no verified owner, host lease,
Tokio cancellation token or credential-resolution result. Trusted local adapters
retain authority and cancellation context; decoding identifiers never grants it.

Before enabling a production `RemoteIntegrationGateway`:

1. Move private account/grant/attempt persistence, cipher/rotation, registry,
   renewal and authenticated HTTP/MCP sessions/transports together. Keep login,
   company/workflow/action/approval state in Openbot. Replace only host gateway
   composition after conformance passes.
2. Authenticate service clients and bind owner namespaces. Specify bounded
   protocol versions, TLS, limits, key custody, operator access and request/result
   retention. Never bridge the split by exporting tokens.
3. Implement a required remote `DispatchAuthority` adapter against live host
   state, bound to exact action/request/grant versions. Define dispatch
   linearization and revocation acknowledgement under partitions and racing
   membership changes. Cached membership or long-lived tickets do not replace
   live checks; outages deny pre-dispatch work. An entered effect may complete.
4. Make grant updates and user/company/member teardown explicit idempotent
   commands. Add recoverable outbox/inbox coordination across databases and test
   denial while cleanup is pending. Shared foreign keys and distributed
   transactions are not the protocol.
5. Preserve action reservation, cancellation/lease loss and receipt
   reconciliation across the network. Persist/reconcile dispatch identities and
   observed outcomes; bound cancellation acknowledgement and session cleanup.
   A lost response must not cause an unsafe second send or an exactly-once claim.
6. Pass actual multi-process, partition and competing-dispatch tests before
   enabling remote composition. Provision a non-root service image and CI gates
   for both processes. Reset affected schemas/payloads when needed; do not add
   backfills, compatibility decoders, dual writes or token-export bridges.

## Implemented verification commands

Run with explicit disposable `DATABASE_URL` and `TEST_DATABASE_URL`,
`SQLX_OFFLINE=true`, and `ALLOW_MISSING_DATABASE_URL` unset. Never aim tests at a
development/production database. The plan's [database setup and command inventory](../plan/.archive/vault-google/07-integration-verification-and-operations.md)
and [CI](../.github/workflows/ci.yml) cover provisioning and broader checks.

```sh
python3 scripts/tests/integration-boundary.py
python3 scripts/integration-boundary.py
cargo test --locked --offline --lib integration_portability_ -- --list
cargo test --locked --offline --lib integration_service_contract_ -- --list
cargo test --locked --offline --lib vault_google_e2e_ -- --list
scripts/test-network-isolation.sh cargo test --locked --offline --lib integration_portability_
scripts/test-network-isolation.sh cargo test --locked --offline --lib integration_service_contract_
scripts/test-network-isolation.sh cargo test --locked --offline --lib vault_google_e2e_
```

The source gate walks production modules, checks custody table access and public
DTO dependencies, and rejects serializable private results/runtime proofs or
their inclusion in gateway replies. Mutation tests exercise its rejection paths;
runtime sentinel tests separately cover actual provider replies, receipts and
logs. CI lists the new families before executing its database-backed suite.
The Vault browser fixture command and actual browser script are registered in CI.
Consult phase evidence for commands actually run and gaps; documentation does
not assert that a full suite or live provider acceptance has passed.
