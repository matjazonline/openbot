# AS-P03-D1/r2 — SSO and company authority implementation decision

Revision r2 for independent architecture recheck of AS-P03-G1-F1; no dependent implementation yet.
Implements accepted p01 D1/r2 + D2/r1, phase03, AS-C001/r1 and AS-C002/r1 under
coordination revision4. No backward compatibility; edit init schemas and use fresh
owned test databases. Production resets are not authorized.

## Ownership and integration

Openbot remains the account/session/membership authority. Add a cohesive application
identity/lifecycle port and its PostgreSQL implementation, HTTP bridge routes and
optional infrastructure composition. Its existing SessionAuthority supplies the
verified user/session, and existing company membership semantics determine roles.
Keep private framework/storage types out of the application port.

Openbot bridge configuration is optional, so unrelated Openbot deployments keep
starting without attribution. Configuring it requires a validated attribution
public origin, confidential-client secret and lifecycle-delivery secret. Client ID
is the contract's constant `attribution`; callback and delivery URLs are fixed paths
on the registered origin, not arbitrary independent URL knobs. Root composition
adds an optional bridge to AppState and a supervised worker to ApplicationWorkers;
main retains and joins its task alongside existing owners. No shared database,
password, browser-session signing key or private source crosses to attribution.

Attribution owns application/session/tenant services, HTTP authority adapter,
PostgreSQL ports and its settings/navigation shell. Its configured Openbot origin
is fixed for all outbound calls; production HTTPS, loopback HTTP only for tests.
No redirects from authority calls; total/connect deadline2s, bounded response body,
strict typed payloads, and no automatic retry inside interactive requests.
Production `serve` must have valid identity and encryption configuration; migration
tooling only needs storage. Focused foundation fixtures will provide explicit test
settings rather than silently exercise an unconfigured authentication mode.

## Codes, continuations and grants in Openbot

Add owned tables for bounded login continuations, one-use authorization codes and
opaque identity grants. Tokens have at least32 random bytes from a maintained OS
random facility. Store SHA256 verifiers, never raw codes/grants. Use maintained
`oauth2` PKCE types and SHA256 implementation; constant-time verifier comparisons.
Newtypes distinguish codes, grants, client identifiers and callback/return bindings.

The authorize endpoint validates client, exact registered callback, response_type,
state length/alphabet and S256 challenge before any redirect. It accepts no user ID.
Already authenticated users proceed through existing SessionAuthority. Otherwise,
store a ten-minute continuation and set a Secure/HttpOnly/host-only correlation
cookie. Redirect to existing Openbot login. All existing browser login methods land
at `/`; its existing index handler is the one continuation seam. With a valid
pending cookie it resumes the fixed bridge path; otherwise retain existing
invitation/onboarding/home routing. Do not change individual password/social login
credential logic or invent a provider return URL parameter. Continuation consumption
and code creation commit together; expired or replayed continuations never mint code.

Codes bind user, originating session, authentication revision, client, callback,
PKCE challenge, 60-second expiry and consumed state. Grants bind the same actor/
session/revision/client and expire at min(parent expiry, now+8h), without refresh.
Use a composite session reference (session_id,user_id,auth_revision), supported by
a unique session key, so mismatched actor/session rows cannot be persisted.

Redemption preloads only immutable binding metadata, then locks account, originating
session and code in that order, consistent with account security/deletion ownership.
It rechecks every binding and live expiry after acquiring locks, atomically consumes
the code and inserts the grant. Invalid PKCE/client attempts do not consume it;
competing valid redeemers have exactly one success. Lost exchange responses require
a new login transaction; grants orphaned by an ambiguous response expire normally.

Every identity/company request checks grant expiry/revocation, current parent session
and account revision. Existing account security mutations and deletion therefore
invalidate grants without separately maintaining a cached account status. Openbot
currently has no general account-disabled flag/UI; this bridge does not create a
second account authority. The concrete revocation proof covers real session revoke,
password/email security changes and account deletion, as accepted p01 specifies.
Future account-disable behavior must invalidate that same authority.

Credential classes remain distinct. Only Basic confidential-client authentication
can redeem codes/read company status; bearer identity grants access identity/company
membership APIs and their own idempotent revocation. The Openbot cookie is used only
on authorize and the owner/admin status mutation, never as a cross-application token.
Rate-limit authorize/token/resume before body/database work with bounded shared
admission and a fixed refill budget; existing login admission remains unchanged.

## Attribution sessions and browser boundaries

Login transactions last10min and bind a random state/correlation secret, encrypted
PKCE verifier and validated attribution-local return path. Reject absolute,
scheme-relative, backslash/control-character and encoded redirect escapes. Callback
must match the browser cookie and state, then consume the transaction atomically
before one code exchange. Validate the grant through the fixed identity endpoint.

Rotate into a new opaque local session, revoking any presented old local session in
the same local transaction. Session lifetime is30min idle and8h absolute, bounded
by grant expiry. Cookie `__Host-attribution`: Secure, HttpOnly, SameSite=Lax, Path=/,
no Domain. Store only its verifier. Store the recoverable grant and random CSRF
material in authenticated ciphertext with purpose+record-ID AAD and key version;
use maintained XChaCha20-Poly1305, not custom encryption. Login verifier encryption
has a different purpose and record identity. Key ring has a configured active key
and bounded prior decryption keys; no plaintext fallback.

Every protected dashboard/API request checks current upstream identity; an active
company additionally requires current membership/permissions. Missing membership,
revocation and unavailable authority are distinct; outages return retryable503 and
never authorize via cached roles. Authenticated writes require the session CSRF
token as well as same-origin/SameSite protection. HTML embeds the CSRF token only
after authentication, escapes display values, and uses local scripts/assets/CSP.
Logout is the documented exception to upstream liveness: local cookie+CSRF revokes
locally first, followed by bounded best-effort upstream grant revocation.

Attribution settings links use fixed validated Openbot origins/routes and open a
separate tab, keeping the attribution page as the return location. Service return
paths remain local and validated. No arbitrary external return target is accepted,
and no account/company administration or page proxy is implemented in attribution.

Client/lifecycle key rotation supports a current verifier plus a previous verifier
with an explicit expiry, not indefinite fallback. Encryption key versions support
normal session-key rotation; this is not legacy data compatibility. Invalid secret
lengths/encodings, incomplete registrations/key rings or non-HTTPS remote origins
fail startup without echoing the values.

## Current company permissions and local tenant state

Use current Openbot membership owner/admin => read, links:manage,
integrations:manage; member => read. Identity company lists are bounded keyset pages
(default25, maximum100); cursors bind requesting user/client/filter context. Use
the same membership semantics as existing company use cases rather than a second
cached role table. Every service resource lookup/writer takes the selected company;
no header or submitted user ID can confer authority.

Local tenants have external company ID, optional display snapshot, lifecycle
version/state/change time, status_valid_until and reconciliation lease/backoff data.
Provisioning and lifecycle update serialize on that exact row: insert-if-absent,
then lock/recheck. Greater versions supersede lower ones, equal version/different
state conflicts, and deleted is terminal even against later active/provisioning
responses. Display fields never confer membership. Deletion immediately clears
this phase's display/configuration data, leaves only the minimal tombstone and a
durable purge request for later owned commerce tables; bounded commerce purge is
implemented with those owners in phase09, not an empty worker scaffold here.

Freshness is at most5min from the start of a successful authoritative status/access
request, never response arrival. Each request captures a process-local monotonic
Instant before any database/network wait, and obtains a sequence-generated durable
observation number plus database request-start time. A successful active observation
creates a process-local ticket for that company/version with deadline start+5min.
Every operation requires BOTH that ticket and the persisted active row/version/
status_valid_until; tenant writers check the monotonic deadline again after taking
the tenant lock, immediately before their mutation. DB wall-clock rollback cannot
extend the monotonic ticket. A backward clock can shorten availability, never extend
authority. Only a successful trusted status/access read can produce a ticket;
notification delivery cannot. Lower observation numbers cannot overwrite a newer
observation or renew freshness, regardless of wall-clock order.

Tickets are not persisted or reconstructed from wall timestamps. Restarted or other
processes must obtain their own successful authoritative read before using a tenant;
a bounded 1024-entry process cache may evict tickets, with eviction treated as cold.
On cold/expired access the existing bounded request admission permits one status
refresh attempt, then 503 if authority is unavailable. No per-event Openbot call is
needed while a local ticket remains valid. Reconciliation refreshes the owning
process's tickets; leases do not confer freshness on other processes. Concurrent
reads can each create a ticket only when the locked persisted version/state still
matches the response; the newer monotonic deadline wins for the same version. All
operations still compare the current locked row so suspend/delete invalidates any
cached ticket. User requests always make their required live authority calls.

Deterministic tests use an injected clock at the freshness owner to move database/
wall timestamps backward while monotonic time advances past five minutes, asserting
503 and no write; restart/second-instance tests reuse persisted fresh timestamps
without tickets and require a successful new authority read. Also prove late response
arrival, stale sequence delivery, ticket eviction, and lock wait past deadline cannot
extend access. Tokio paused time supplies the monotonic clock in focused tests; no
production configurable freshness budget is introduced.

Provide the persistence-owned locked-tenant operation for future tenant writes,
checking active state/freshness after acquiring the row lock. Lifecycle writes take
the same lock. Tests prove a suspension which commits first prevents a later write;
the writer cannot check before waiting and then commit from stale authorization.

## Durable lifecycle in Openbot and bounded reconciliation

Openbot owns a minimal lifecycle table without a company foreign key, so deletion
tombstones survive. Use database triggers, matching existing auth-revision/session
revocation ownership patterns, to create initial active state on company insertion
and terminal state on company deletion. A lifecycle-state trigger enqueues a typed
outbox row in the same transaction. This covers both existing direct company and
owner-account deletion paths without duplicating hooks or bypassing their existing
integration/memory cleanup. Reusing a deleted company ID cannot recreate active state.

The owner/admin status command locks the live company and current membership, checks
expected_version, then changes attribution state. No-op state writes have no new
notification; stale expected_version is409. It does not globally suspend Openbot.
State/version transitions and changed_at are database constrained/generated. Every
outbox payload is immutable company ID/version/state/change time, unique per version.

Outbox claim is an atomic bounded SKIP LOCKED transition with a fresh execution UUID,
30s lease and attempt increment. All acknowledgments/failures require the live
execution fence. Attempts cap at12; expired last attempts become durable terminal
failures. Retry backoff is capped at60s; poison rows leave the immediate claim set.
Delivery has a2s HTTP deadline and at most4 supervised in-flight requests. Shutdown
cancels actual HTTP work, joins tasks and leaves only short recoverable leases.
Undelivered/failed/expired rows supply persistent operational backlog/alert evidence.

Attribution reconciliation similarly claims bounded due tenants with execution
fences, bounded requests/backoff and joined shutdown. It refreshes successful reads
before their freshness deadline and leaves unavailable/expired tenants observable
and fail-closed. An authoritative deleted status repairs missed lifecycle delivery.
No per-merchant-event Openbot call is needed while local status remains fresh.

## Verification and current limitation

TASK AC1–AC6 maps original requirements to focused runtime/browser/DB assertions.
Use real competing code redeemers, continuation consumers, lifecycle writers,
provisioners and leased workers. Exercise actual account-deletion owner and rollback,
commit-before-ack loss, lost delivery repaired by status reads, prolonged authority
outage, downgrade/membership loss, cross-company IDs, cookie separation/CSRF and
clock-start freshness. Service standalone tests use a contract-faithful HTTP server.
Separately, p03 browser tests mount the actual Openbot identity and existing login
handlers/use cases/session persistence with real PostgreSQL and the actual attribution
server, on two distinct hosts/origins. They exercise already-authenticated and
password-login continuations, not fabricated-cookie-only journeys. This focused
composition does not load unrelated agent/email/workflow runtimes. Full production
two-application integration is additionally required in phase10.

Run affected Openbot session/security/company/account cleanup tests, both apps'
format/offline checks and fresh migrations/metadata. The untouched root baseline
currently fails61 E0659 errors from an unrelated reconciliation helper exported via
glob imports; exact log in TASK. A one-line visibility correction is proposed to the
user because the workflow prohibits silently fixing unrelated code. No approval yet;
keep the limitation explicit and continue independent architecture/service work.
