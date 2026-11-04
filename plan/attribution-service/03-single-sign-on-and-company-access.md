# 03 — Single sign-on and company access

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [02](02-independent-application-foundation.md) and phase 01's SSO decision. Users enter
attribution on its own origin, authenticate centrally, and use an attribution-owned session.
Openbot remains the authority for accounts, companies, membership, and roles.

## Batch 03A — Deliver login without shared sessions

1. Integrate the selected standards-based SSO arrangement using maintained protocol components.
   Specify issuer, registered confidential client, exact callback URLs, audience, state, nonce,
   PKCE, token expiry, key rotation, and supported logout/revocation behavior. Existing Openbot
   browser authentication is not itself a supported OIDC provider; implement the chosen bridge
   explicitly rather than repurposing an Openbot session token as a cross-app bearer token.
2. Attribution owns login/callback/logout routes and an opaque, revocable local session. Use
   Secure, HttpOnly, host-only cookies with suitable SameSite handling; never set a shared parent
   domain cookie. Rotate the local session at login and enforce idle/absolute expiry.
3. Keep provider tokens and client credentials server-side, encrypted where recoverable storage
   is required. Authentication codes are one-use and short-lived; validate redirects and return
   paths. The browser never receives a merchant key or service administration key.
4. Cover CSRF on state-changing routes, central response security headers, session fixation,
   login rate limits, local logout, account-disable/security-change revocation, and upstream
   expiry. Reuse Openbot's existing account/security owner for any new identity endpoint.

Focused checks: `attribution_sso_` and `attribution_session_` cover code replay with competing
redeemers, wrong issuer/audience/nonce/state, callback injection, expired identity, cookie scope,
CSRF and security-change revocation. Exercise the actual browser redirect flow across two origins,
including an already logged-in user and a user who must authenticate. Run affected Openbot session
and revocation regressions, plus offline compilation, when changing that shared infrastructure.

## Batch 03B — Enforce current company access and separate administration

1. Add the minimum authenticated Openbot identity/company-access API needed by attribution.
   Identity evidence must bind the requesting user and the attribution client; a service secret
   plus an arbitrary submitted `user_id` is not proof of that user's login. Return stable IDs and
   explicit permissions, using Openbot's existing membership use cases.
2. For the MVP, check current account status and relevant company permissions on every protected
   dashboard/API request made under a user session. Use bounded HTTP calls and fail closed with
   a distinct retryable authority-unavailable response when Openbot cannot be reached. Do not
   silently authorize from cached roles or equate an outage with a deleted user.
3. Attribution's company selector lists only authorized companies. Resolve every requested object
   under that active company even when the user belongs to several companies. Map roles to the
   read/link-management/integration-administration permissions agreed in phase 01.
4. Provision local company configuration idempotently from trusted identity/company responses.
   The shared `company_id` is its key. A local record cannot confer membership or recreate a
   suspended/deleted company. Display names are non-authoritative snapshots only.
5. Attribution owns its own navigation and settings shell. Account/company/member management
   links open Openbot settings with validated return navigation. Openbot may add an Attribution
   navigation link. Do not embed the dashboard or proxy its pages through Openbot.

Focused checks: `attribution_company_access_` for another tenant's IDs on every resource path,
multi-company users, lost membership during a session, role downgrade, revoked account, timeout,
forged actor IDs, and concurrent first-use provisioning. Assert attribution cannot connect to the
Openbot database. Verify both browser applications keep separate session cookies.

## Batch 03C — Propagate company lifecycle without a shared database

1. Define authenticated, idempotent company suspension/deletion messages with monotonic lifecycle
   versions. Openbot durably records the notification with its company change and retries bounded
   deliveries. Attribution applies the lifecycle change transactionally and cannot revive a
   tombstoned company from a delayed provisioning/login response.
2. A suspension disables local credentials and public installations according to the agreed
   policy; deletion starts the defined retention/purge process. Repeated lifecycle messages have
   one effect. Use a narrowly scoped service credential that cannot submit sales or read reports.
3. Reconcile company status periodically through the authenticated API and enforce a maximum
   status freshness deadline agreed in phase 01. After that deadline fail closed for tenant
   operations, including merchant/public ingestion. Merchant requests need no per-event Openbot
   call while local status is valid. Document the exact bounded outage/revocation behavior.
4. Keep delivery/reconciliation work supervised and durable with bounded retry/deadline policy.
   Alerts surface undelivered changes and expired status; local counters are insufficient.

Focused checks: `attribution_company_lifecycle_` with competing lifecycle deliveries, stale versions,
delete versus first-use provisioning, crash after commit before acknowledgment, missed delivery
repaired by reconciliation, and a simulated prolonged Openbot outage. Record observed revocation
bounds and error behavior. Use isolated databases for global sweeps/worker assertions.

Completion: moving attribution to another server does not move user/company administration or
change any session semantics. An Openbot outage has a documented effect, not a hidden dependency.
Record batch commands and counts here; final full-suite gate is in [10](10-integration-verification-and-pilot.md).
