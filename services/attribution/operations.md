# Operating the attribution service

No backward compatibility. Incompatible schema changes require an explicit operator-managed
reset. Startup applies the one owned migration; it never resets a configured database.
Production region, retention choices, load targets and recovery objectives need deployment-owner
approval. The example's 38/30/400/8/90 retention days, delivery 7 days and refund 365 days are
verification values, not approved production policy.

## Retention and repair

Raw clicks use `clicked_at`; observations and event envelopes use `received_at`; audit and
management receipts use `created_at`. Financial expiry uses the later of order receipt and
payment. All comparisons retain a row exactly at the cutoff. Cleanup can run later than the
cutoff due to backlog; these are minimum lifetimes, not deletion deadlines. Pending, unresolved
and leased merchant events protect their orders and referenced checkout/click evidence. Terminal
merchant envelopes retain their configured lifetime even if financial expiry would occur earlier.

Orders already carry frozen attribution dimensions. Raw click deletion cannot recalculate their
attribution or money. Raw counts can shrink as their retention expires, so comparisons over old
periods must account for differing retention windows. Expired financial facts are hidden as a
whole before child rows are removed. Refund and order business identities last with that fact;
after contractual expiry and deletion, the service does not promise historical identity lookup.
Original event timestamps beyond the delivery horizon are rejected before idempotency lookup.
An expired checkout can no longer provide attribution for a newly delivered payment.

Management idempotency receipts last at least 48 hours for the 24-hour original-request horizon.
Checkout receipts follow click retention. Deleted tenants deny access immediately; cleanup removes
owned rows in dependency order, with a permanent minimal company/version/deleted-state fence.
Company-independent user sessions remain subject to their own session lifetime and cannot grant
access to a deleted company. Downloads are streamed; no server export cache needs purging.

Maintenance claims at most eight tenants, advances their schedule before attempting work, and
retires or deletes at most 100 data rows in one tenant transaction (plus scheduling metadata). SQL has a two-second deadline and the worker
has a five-second batch deadline. Busy/failed tenants retry on the committed schedule; other
tenants remain eligible. Successful work schedules another pass after one second, idle tenants
after sixty. Shutdown cancels in-flight transactions and joins the owned workers. No detached
cleanup job or deployment-specific scheduler is required. Do not raise limits to hide slow work.

Structured logs contain fixed operation/outcome labels, UUID request/event correlation, status,
latency, delivery lag, committed attribution reason, pending/unresolved counts and oldest age,
company freshness, and cleanup progress/failure. They omit merchant identifiers, URL/query,
headers, request bodies, IPs and secret-bearing database errors. Protect logs at the platform
boundary and set platform log retention separately. `/healthz` reports process liveness;
`/readyz` checks storage readiness; company freshness is a separate structured operator signal. There is no public global metrics API.
Owner/admin can inspect unresolved merchant events and request retry through the dashboard;
members can inspect them. Retrying preserves event identity, tenant checks and bounded leases.

## Independent backup and restoration

Back up only the attribution database with a PostgreSQL client of the same major version or
newer. Use a protected libpq service file/password file; commands below deliberately use a
service name rather than a password-bearing URL. Restrict backup files to the operator.

```sh
umask 077
pg_dump --dbname=service=attribution_backup --format=custom --no-owner --no-acl --file=attribution.dump
pg_restore --dbname=service=attribution_fresh_restore --no-owner --no-acl --exit-on-error attribution.dump
```

`attribution_fresh_restore` must point to a separately provisioned, empty database owned by its
own non-superuser application role. Revoke PUBLIC CONNECT on both applications' databases and
leave each role unprivileged. Never use `--clean`, restore over a live database, or reset an
existing database as part of this procedure. Preserve the dump's schema and application binary
identity together. Verify migrations/schema identity before serving the restored instance.

Keep the complete versioned `ATTRIBUTION_SESSION_KEYS` ring in the operator's separate secret
backup, alongside the bridge client and lifecycle credentials. The database dump contains
ciphertext and key verifiers, not a recoverable copy of those secrets. Retain needed old encryption
keys during an intentional rotation. Losing them invalidates encrypted sessions/cursors even if
the ledger survives. An Openbot backup is a separate operation with separate credentials.

Before cutover, verify exact orders/refunds, merchant credential verification, encrypted-session
access, a pending event's resumed settlement, terminal tenant fences and denial of cross-database
connections. Stop writers on the old instance, take the final consistent dump, restore and verify,
then switch the private database connection. Keep the stable public origin. Do not run divergent
old/new databases behind the same origin. Recovery point is the last successful consistent dump;
recovery time includes restore, startup, reconciliation and verification. Local fixture measurements
are recorded by `attribution_recovery_real_dump_restore_auth_ledger_pending_and_isolation`; they
are not production RPO/RTO commitments. Verify the actual production backup store and restore
throughput before approving those objectives.

## Deployment boundary

Build with `docker build -t attribution-service services/attribution` or from an extracted service
directory. The pinned-base image runs UID/GID 10001 and listens on unprivileged port 8091. Use a
read-only root filesystem, drop capabilities and enable `no-new-privileges`. Provision all required
keys from `config.env.example` through a protected environment file/secret injector; the application
reads environment variables, not arbitrary secret-file paths. Never copy real keys into an image.
The service directory includes its lockfile, migration, assets, contracts and verification scripts.

TLS termination belongs to the outer ingress. Forward the original Host, and never use a forwarded
identity header. Attribution does not trust `X-Forwarded-For` or proxy-supplied user/company data.
Only its configured HTTPS `ATTRIBUTION_PUBLIC_ORIGIN` determines canonical browser URLs. Configure
`ATTRIBUTION_OPENBOT_ORIGIN` to the public HTTPS authority; Openbot's `OPENBOT_ATTRIBUTION_ORIGIN`
must name the same stable attribution origin. The callback is `/auth/callback`; Openbot receives
`/api/attribution/v1/authorize`, `/api/attribution/v1/resume` and the other `/api/attribution/v1/*` routes, and delivers to
`/internal/v1/company-lifecycle`. Keep database ports and application plaintext ports private.
Apply network ACLs so the apps can reach each other only through the documented HTTPS bridge.
No shared application filesystem, schema or Rust crate is needed.

Use the OS/container trust store for outbound HTTPS, including an enterprise CA when necessary.
The app does not offer an insecure TLS flag. Its fixed authority transport limits are documented
in the contract; the service request deadline is ten seconds, report SQL two seconds, and DB
acquisition/connect bounds are fixed in the PostgreSQL adapter. Configure ingress budgets to
allow these responses, with an outer finite deadline and bounded body size. Do not rewrite URLs
or buffer redirects in a way that obscures retryable HTTP 503 from uncertain click persistence.

Moving compute preserves the public origin and merchant API. Moving the database additionally
requires the verified restore/cutover procedure above. New logins and current user-authorized
actions require live Openbot authority. Existing merchant/public operations require a status
observation no older than five minutes, checked against both a local monotonic clock and durable
state. A cold process has no local freshness ticket: it must reconcile before serving a tenant.
Outage does not turn the service into an independent account authority.

`deploy/compose.yaml` is a single-service deployment example. Set `ATTRIBUTION_IMAGE` to the
built image's immutable registry digest; it is a Compose variable, not an application setting.
Copy `config.env.example` to `deploy/service.env`, replace every example credential, set both
HTTPS origins, and use `postgres://attribution:<URL-encoded application password>@database/attribution`
for its database URL. Provision `deploy/secrets/database-admin`, `database-application`,
`tls-certificate.pem` and `tls-private-key.pem` outside version control. The application password
must match the database URL. Grant the unprivileged ingress process read access to its mounted
TLS key through the secret provisioning mechanism; do not make production private keys public.
Replace the example server name in `nginx.conf`. The database has no published port. Fresh-volume
initialization creates the application role; re-running initialization is not a password-rotation
mechanism. Observe `/readyz` over ingress before routing live traffic and `/healthz` for liveness.
The Compose example has no automatic restart or reset loop; use the deployment platform's bounded
restart policy and diagnostics. A company-status authority failure does not make liveness fail.

The root `scripts/attribution/topology.py` fixture runs actual Openbot and attribution artifacts
in two local network layouts. It uses an ephemeral trusted CA and a Chromium pin for that exact
fixture key; production TLS verification remains enabled. The same trusted application images
run in both layouts. Public DNS proof is covered by the service DNS fixture; this local topology
explicitly seeds the approved domain state and does not claim public DNS ownership. Its outage
check forces durable freshness expiry and restarts the process; exact monotonic five-minute
boundary behavior is separately exercised by the service clock tests. Native store adapters and
a real-store pilot remain outside this API-first implementation.
