# 05 — Private credential resolution and provider renewal

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [03](03-connection-bindings-and-authority.md) and [04](04-google-authorization.md).
A private module resolver supplies credentials only to the integrations injector/transport and
owns all persistent state and coordination. Workflow steps and IntegrationGateway consumers do
not acquire tokens or credential database access. Google is the first production renewal strategy.

## Batch 05A — One bounded resolution path

Implement one private resolver for a connected account and use grant. Static accounts use a
registered shape/injection validator and narrow credential read; managed accounts delegate expiry,
refresh, credential generation, and recovery decisions to their provider strategy. Resolve when
an operation actually executes, including resumed/reclaimed runs, rather than at publication or
approval creation.

For a Google account:

1. Validate the caller's connection grant and execution context without decrypting unrelated rows.
2. Begin a dedicated bounded persistence transaction and lock that account with SELECT FOR UPDATE.
3. Re-read provider/config/method/schema, status, identity, authorization revision, and encrypted
   payload under the lock.
4. Reuse a cached access token with at least 60 seconds of remaining lifetime.
5. Otherwise refresh through the Google adapter, preserve an omitted refresh token, and save the
   new token/expiry atomically before committing.
6. Return the token only to the private injector, with authorization identity and token generation.
   Release the refresh lock before entering the final dispatch gate and remote business dispatch.

The lock is on the shared account credential, not a run, process, HTTP connection, MCP connection,
or app definition. Gmail and another permitted app sharing that authorization use the same lock.
A second worker waits and rechecks after the first commits. All references to the same Google
authorization must use that entry. Never use a read/unlock/network/write sequence: it recreates
the original duplicate-refresh window.

The persistence adapter owns the lock. A resolver adapter composes that persistence path with
the provider token transport behind the module's private credential-resolution port. Database
transaction and lock types never escape the adapter boundary. Do not hold company, workflow-run,
or existing short action-authority transaction locks across the Google request. Establish and
test a consistent lock order with management, binding operations, and the final dispatch gate.

Start with a 5-second maximum entry-lock wait and a 10-second provider request bound, both capped
by the caller's remaining deadline. Apply compatible transaction/idle bounds in the dedicated
resolver path; do not globally raise workflow persistence or pool limits. Lock timeout returns a
typed retryable outcome. Provider response and payload sizes stay within 01's bounds.

## Batch 05B — Google refresh protocol and failures

The adapter posts a form to the fixed Google token endpoint with grant_type=refresh_token and the
server-held OAuth client credentials. It accepts a validated access token and lifetime, retaining
the existing refresh token when no replacement is returned.
[Google renewal protocol](https://developers.google.com/identity/protocols/oauth2/web-server#offline).

Cache renewal increments entry revision and token generation, while preserving authorization
revision and connection policy. Entry replacement/reconnect changes authorization revision.
Envelope rewrapping changes neither logical grant identity nor token generation.
Reject a newly issued token that cannot meet the required remaining-lifetime margin; do not
enter an immediate refresh loop on short or malformed lifetimes.

Classify errors without storing provider response bodies:

- Revoked/expired grant: commit reconnect_required for the same current authorization identity.
- Temporary provider failure/rate limit: preserve the grant, return retryable failure, and store a
  bounded shared refresh retry time so different workflow runs cannot hammer the failing endpoint.
- Invalid client configuration: surface an operator configuration failure; do not mislabel every
  user's account as revoked.
- Malformed/oversized response or unavailable encryption key: fail closed without a partial write.

Keep network attempts bounded and use existing workflow retry/backoff. Do not promise exactly-once
refresh across a crash between Google's response and the database commit. A replacement worker
re-reads durable state and resolves again; an unusable grant requires reconnect. This Google
adapter is not a generic solution for every provider's refresh-token rotation protocol. The
renewal interface must explicitly select no-renewal, reusable-refresh, or provider-specific
rotating-refresh behavior; an unknown strategy fails closed.

Cancellation reaches the actual HTTP request and rolls back/releases the lock. No detached
refresh task survives worker shutdown. An ambiguous database commit is re-read before making
another state decision; a stale error must not invalidate a newer successful reconnect.

## Batch 05C — Provider differences and ambiguous rotation

Do not require refresh tokens for non-expiring API keys or providers with no renewal protocol.
Do not apply Google's omitted-refresh-token rule to every provider. A provider declares its
accepted token replacement rules, error classification, safe recovery, revocation scope, and
whether access-token changes require rebuilding a session.

Extend 04's non-Google test adapter with strict refresh-token rotation, exercised end to end in
07. Before its token request, durably record a pending renewal attempt ID, credential generation,
and bounded attempt deadline under the account lock. A second transaction reacquires that lock,
verifies the same pending attempt, unexpired deadline, and current authority, then
holds the lock across the bounded provider request and credential write. A competing resolver
seeing a pending attempt must not make a token request; it may wait/re-read within its own bounded
deadline or return retryable pending status. The durable marker survives cancellation or rollback
after an ambiguous response; it is not erased as if no attempt occurred.

For the strict fixture, a pending attempt past its durable deadline with no committed replacement
requires reconnect rather than retrying the potentially consumed refresh token. Do not classify
an active claimant as interrupted merely because another resolver observed its marker. A live claimant must
recheck its attempt under the lock before contacting the endpoint, so a concurrent disconnect or
recovery decision prevents a delayed request. Success atomically replaces credentials, advances
generation, and clears the marker. This is conservative recovery, not exactly-once refresh.
Real adapters may implement better recovery only when their provider documents it and tests
prove it. Production Google keeps its reusable-refresh path; it does not inherit this protocol
merely because both use OAuth2.

Keep provider-specific renewal state typed and bounded inside credential persistence. The
gateway sees sanitized retry/reconnect status, never refresh attempts containing secrets.

## Batch 05D — Disconnect and reconnection lifecycle

Disconnect, reconnect completion, owner replacement, and deletion coordinate through the same
account lock. Disconnect increments authorization revision, clears cached usable tokens and
pending renewal authority, and invalidates bound dispatch authority locally. It succeeds locally
even if the external provider is unavailable.
Reconnect restores the private entry; bindings carrying its old authorization revision remain
unusable until the owner confirms them through the existing binding flow. Show this state in the
UI rather than silently restoring every company's access. Old business approvals remain stale.

Make local disconnect and provider-wide revocation distinct owner actions. Google's revocation
endpoint can affect the project's combined grant, so do not automatically revoke the Google
grant when removing one company binding. If provider revocation is offered, disclose its scope,
use the fixed endpoint with bounded calls, and keep local denial effective on failure.
[Google revocation behavior](https://developers.google.com/identity/protocols/oauth2/web-server#tokenrevoke).

Deleting a binding leaves the private entry usable by other owner-approved connections. Never
log tokens, ciphertext, request forms, or raw provider errors in these lifecycle operations.

## Focused checks and acceptance

Add integration_renewal_, google_refresh_, google_refresh_race_, and google_connection_lifecycle_
families with real PostgreSQL transactions, separate pools/services, and counted scripted token
endpoints.

Required competing cases: expired-token HTTP versus HTTP and HTTP versus MCP requests produce one
refresh; an independent user refreshes without being blocked by another user's row; timeout/crash
releases the lock; refresh versus disconnect/reconnect/delete cannot restore old authority; key
rotation CAS cannot overwrite refreshed tokens. Use barriers/notifications, not timing guesses.
Also test valid-token reuse, omitted refresh token, short lifetimes, shared cooldown, cancellation,
lost response, malformed data, missing keys, and ambiguous commit recovery. integration_renewal_
covers no-renewal API keys, app-shared credential locking, strict rotation competitors, durable
pending-attempt crash recovery, and invalidation before a delayed claimant reaches the endpoint.

Batch checks: 05A uses integration_renewal_ and valid-cache Google cases; 05B uses google_refresh_;
05C uses the strict-rotation cases in integration_renewal_; 05D uses google_connection_lifecycle_
and vault_rotation_. Apply 02's SQL checks after query changes.

~~~sh
cargo test --locked --offline --lib integration_renewal_ -- --list
cargo test --locked --offline --lib integration_renewal_
cargo test --locked --offline --lib google_refresh_
cargo test --locked --offline --lib google_connection_lifecycle_
cargo test --locked --offline --lib vault_rotation_
~~~

List filters and record counts/results; google_refresh_ must include the race family. Apply 02's
SQL checks after query changes. Acceptance: competing runs share a cached token through one
bounded refresh, and loss of authority remains effective through every delayed response.
