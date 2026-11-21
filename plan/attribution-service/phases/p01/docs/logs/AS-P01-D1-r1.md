# Attribution network contracts

These versioned contracts describe the API-first implementation. Native checkout
platform adapters and a real-store pilot are later work. The contract files are
portable wire artifacts; the service does not import Openbot application code.
They do not imply that every endpoint has been implemented yet.

## Ownership and credentials

Openbot owns accounts, company membership, and lifecycle authority. Attribution
owns its local sessions, stores, tracking, commerce ledger, and reports. Every
database and deployment credential is application-specific. Only HTTPS connects
the applications in production; loopback HTTP is restricted to test fixtures.

| Credential | Intended recipient | Authority |
| --- | --- | --- |
| Openbot browser session | Openbot origin only | Existing account login; never accepted by attribution |
| Confidential attribution client secret | Openbot token/status endpoints | Redeem a bound code or obtain company status; cannot select a logged-in user |
| Openbot identity grant | Openbot identity/company endpoints | One user, one attribution client, one live originating Openbot session |
| Attribution browser session | Attribution origin only | Local session plus a live authority check on each protected operation |
| Store API key | Attribution merchant/approved scoped APIs | One company, one store, explicit scopes; no account impersonation |
| Public installation ID | Attribution observation endpoints | Select public configuration; never authorize commerce or management |
| Lifecycle delivery secret | Attribution lifecycle endpoint | Apply company status/version only; cannot create sales or read reports |

Keys are at least 256 random bits, shown once, and stored only as SHA-256
verifiers. Recoverable identity grants and future provider secrets are encrypted
with attribution-owned authenticated encryption and purpose/record-bound associated
data. Token values, authorization headers, callback queries and payload bodies
must not enter request logs. No raw IP address is stored.

## Sign in with Openbot

This is a dedicated OAuth 2.0 authorization-code/PKCE bridge with an explicit
identity API. It is not an OIDC discovery/ID-token provider or a new account system.
Use maintained OAuth client/PKCE and cryptographic components, not custom crypto.
Openbot authenticates with its existing login methods; user IDs come exclusively
from its verified session. No token endpoint accepts a submitted user ID.

1. Attribution `/auth/login` creates a server-side login transaction, random state
   and S256 PKCE verifier/challenge, with a ten-minute expiry. A Secure, HttpOnly,
   host-only SameSite=Lax correlation cookie binds the transaction to the browser.
   An optional return path is a validated attribution-local path, never a URL.
2. Redirect to Openbot `/api/attribution/v1/authorize` with fixed registered
   `client_id`, exact registered `redirect_uri`, `response_type=code`, `state`,
   `code_challenge`, and `code_challenge_method=S256`. Reject all other methods,
   clients and callback URLs before redirecting. The callback is never caller-defined.
3. Openbot obtains a live existing session. If login is needed, a bounded server-side
   continuation survives the normal Openbot login flow; no arbitrary return URL is
   accepted. After authentication it generates a 60-second random code, storing only
   its verifier plus user, session, authentication revision, client, callback,
   challenge, expiry and consumed state. It redirects to the exact callback with
   code and the original state. Responses use no-store and no-referrer.
4. Attribution verifies state/correlation and atomically consumes its login
   transaction. Its backend posts the code, exact redirect URI, and verifier to
   Openbot's fixed token URL, authenticated with its dedicated client secret.
   Openbot atomically consumes the code after verifying client/callback/PKCE and
   the originating session/revision. Competing redeemers cannot both succeed.
5. Openbot returns an opaque identity grant, bound to that user/client/session,
   expiring at the earlier of eight hours or the originating session expiry.
   No refresh token is issued. Attribution verifies the returned identity through
   the fixed identity endpoint, then rotates its own opaque local session.
6. Attribution stores the encrypted identity grant server-side. Its session has
   a thirty-minute idle limit and eight-hour absolute limit, also bounded by grant
   expiry. Cookie `__Host-attribution` is Secure/HttpOnly, Path=/, SameSite=Lax,
   without Domain. Cookies are never interchangeable between applications.
7. Every protected request checks the grant and current company permissions via
   Openbot. Loss of membership, originating session revocation, account deletion,
   or authentication-revision change removes access. Outages return retryable 503,
   not a false account-deleted response. Local logout revokes the attribution
   session and best-effort revokes the upstream grant; local revocation succeeds
   even when Openbot is unavailable.

State-changing browser APIs require the local session's CSRF token in addition
to SameSite cookies. Apply CSP, frame denial, nosniff, no-referrer, and no-store
centrally. Limit login and token endpoints before parsing or database work.

## Company scope and lifecycle

Owner/admin permissions are `read`, `links:manage`, and `integrations:manage`;
members receive `read`; non-members receive none. A user's active company is an
explicit request scope, never a cached global membership grant. Every nested
store/ad/link/click/order/refund lookup includes that company and its parent store.
For user API calls, `X-Company-ID` selects the company to check; it never grants
access. For a store key the company/store come only from the credential; conflicting
body/header/filter IDs are rejected without exposing foreign object details.

Openbot owns a monotonic lifecycle version per company, retained after deletion.
A company lifecycle change and its durable notification commit in one Openbot
transaction. The outbox payload contains only company ID, version, state, and
change time. Delivery uses bounded retries and leases, with persistent unresolved
state and status reconciliation as recovery. No distributed transaction is used.

Attribution serializes provisioning and lifecycle updates on the same local tenant
key. A greater version supersedes a lower one; an equal version with different
state is a conflict. Deleted is terminal, including against later provisioning.
Deletion retains a minimal ID/version tombstone and starts bounded purge. Suspension
blocks all tenant operations while retaining credential metadata; resumed access
requires an explicit newer active version, and manually revoked keys stay revoked.

An authenticated status read supplies current state and version for an already
known external company ID. Each tenant has a `status_valid_until` bounded by five
minutes from the beginning of the successful status request, not its response
arrival. Local clocks/HTTP deadlines must not extend that budget. Stale lifecycle
deliveries cannot refresh freshness. At or after expiry all tenant operations fail
closed with retryable 503 until an authoritative status read succeeds. Status
reconciliation is bounded and supervised; merchants need no per-event authority
call while the local observation remains fresh. User operations always check live
authority even inside this window. Tenant writes take a lock shared with local
lifecycle changes so a completed suspension cannot race with a later accepted write.

## HTTP and retry contract

JSON uses UTF-8, rejects unknown properties and duplicate object keys, and has
route-specific byte/item limits. Maximum JSON body is 256 KiB for merchant events,
16 KiB for management, 8 KiB for observations, and 4 KiB for lifecycle/login forms.
Authenticate merchant/service headers before reading/parsing their body. Dates
are RFC3339 instants with an explicit offset, normalized to UTC. IDs are distinct
domain types even when multiple wire IDs use UUID strings. Ad IDs and click IDs
never substitute for one another. Tracking link codes are opaque random tokens.

Lists use opaque, scope/filter-bound keyset cursors, default 25 and maximum 100
items. Reports require a half-open interval of at most 93 days, a validated IANA
zone and at most 100 result groups per page. Export has a separate 10,000-row cap
and a bounded query deadline; pagination cannot bypass tenant predicates.

| Status | Meaning and client behavior |
| --- | --- |
| 200/201/204 | Durable successful operation or exact replay; never an uncommitted acknowledgement |
| 202 | Event durably stored pending reconciliation; does not mean a paid order exists |
| 302 | Tracking redirect only after click commit; no-store; HEAD never creates a click |
| 400 | Malformed or unsupported request; correct it before retrying |
| 401 | Missing, invalid, expired or revoked credential |
| 403 | Known company/resource but permission or credential scope insufficient |
| 404 | Missing or foreign scoped resource; no foreign details disclosed |
| 409 | Same idempotency/event identity with conflicting normalized content |
| 410 | Expired accepted ingestion horizon or deleted public installation/link |
| 413 | Body or collection bound exceeded |
| 429 | Admission limit reached; retry after Retry-After |
| 503 | Storage, commit outcome or authority unavailable/stale; retry after Retry-After |

Clients retry 429/503 and transport ambiguity with the same operation/event ID
and payload, using exponential backoff with jitter (initial 1 s, capped at 60 s).
Interactive operations have a 10-second overall deadline; authority HTTP calls
have a 2-second connect/total bound and no retries inside an interactive request.
Durable workers own their retries and shutdown. A committed click remains a raw
click when its response is lost; a repeated GET is a new click, not an exact replay.

Error bodies expose a stable code, safe message and correlation UUID, never SQL,
secrets, foreign identifiers, raw payloads or submitted credential values.

## Merchant-neutral checkout and commerce

A shop backend associates browser-supplied consented candidates with its own cart.
It calls `POST /v1/checkouts` using a `checkouts:write` store key at order creation,
with an external checkout ID, creation time, consent, and at most 32 candidate
click IDs. Attribution selects the latest eligible click by server click time
(UUID breaks a time tie), within the same company/store and the inclusive interval
`order_created_at - 30 days <= clicked_at <= order_created_at`, excluding bots and
prefetches. No consent gives explicit unattributed evidence. A later ineligible
candidate never replaces an earlier eligible one. The frozen checkout is immutable;
an exact retry replays it and changed facts under its ID conflict.

The browser cannot call the checkout/commerce path with a public installation ID.
It cannot establish order identity or payment. The merchant API is a trust
boundary: its credential holder attests to payment facts. Future native adapters
must authenticate platform signatures before producing the same normalized facts.

`POST /v1/conversions` accepts paid, refund and cancellation events. Identity is
company/store/external event ID, separate from external order and refund IDs.
Persist a canonical typed-payload fingerprint and immutable normalized envelope.
The envelope and immediate ledger mutation commit in one transaction. Exact replay
returns the existing event state; conflicting payloads return 409. Missing payment
prerequisites remain durably pending, with a bounded leased reconciliation worker.
Paid facts serialize by scoped external order identity, including first creation.
Different event IDs for the same identical paid fact cannot create a second sale.
Inconsistent paid facts become visible conflicts, never overwrites.

Paid orders freeze the selected checkout evidence, dimensions and policy version
at their first accepted paid fact. Later clicks/refunds/replays cannot change it.
Missing, foreign, future, expired, no-consent, bot and unavailable/pruned evidence
produce explicit unattributed reasons without disclosing foreign data. Checkout
creation time must agree with the order fact; mismatch is a reconciliation conflict.

Each order has 1–100 lines with stable external line/product IDs, positive quantity,
actual discounted merchandise, and separate tax/shipping totals. Amounts are integer
minor units, never floating point: USD/EUR/GBP exponent 2, JPY exponent 0. Each amount
and checked aggregate is 0–9,000,000,000,000; quantities are 1–1,000,000. Unsupported
currencies and inconsistent order/line sums are rejected. Different currencies
are never added.

Refunds have their own stable refund ID, 1–100 line allocations, merchandise amounts,
returned quantities (zero is valid), and separate tax/shipping refunds. They cannot
exceed remaining money/quantity balances, even under competing refunds. Refunds
do not decrement historical paid-order counts. Cancellation does not create a sale
or reverse money; a paid order is reversed only through refund facts. Arrival order
does not change the converged ledger. Refunds before payment remain pending.

## Reporting and privacy

Clicks use click time; observations use observation time; paid orders/revenue use
paid time; refunds use occurrence time. Order creation time controls eligibility.
Aggregate clicks, distinct orders, lines and refunds independently before joining.
Keep raw clicks, heuristic eligible clicks, image requests, landing observations,
and viewport observations distinct. No metric claims unobserved platform impressions.
Period conversion rate is attributed paid orders / eligible clicks, may exceed
100%, and is null with no clicks. For sold-product filters omit the rate; advertised
product filters apply separately to click dimensions. Never sum a repeated click
denominator across currency rows. Period net merchandise can be negative.

The browser script does no optional observation or persistent storage until an
explicit consent signal. Withdrawal clears local candidates and prevents further
observations; there is no fingerprinting, cross-device linkage, or customer PII.
Tracking redirects retain minimal click ID/time/dimensions/classifier evidence,
without a browser cookie or raw IP. Script events contain bounded IDs, not full URLs.

Retention periods are explicit deployment configuration with validated relationships:
raw click retention covers the attribution window plus accepted delivery delay;
event deduplication covers the accepted retry horizon; financial/order evidence
covers the supported refund horizon. Reject events outside declared horizons so
pruning cannot make an old delivery new. Freeze retained reporting dimensions before
pruning clicks. Test fixtures declare their own periods; production region, retention
and throughput/SLO choices require the eventual pilot's deployment policy. No
production deployment or destructive reset is authorized by this implementation.
