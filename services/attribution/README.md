# Attribution service

Independent Rust/Axum/PostgreSQL application. Run all commands in this directory.
It has its own Cargo workspace, lockfile, SQLx metadata, migrations and embedded
assets. No Openbot crate, private source, session secrets or database are used.
The service API is store-neutral; native store adapters are deferred. Contracts
for the Openbot login bridge and merchant API are in [contract/](contract/README.md).

The service exposes its own login, company selector and settings shell,
`GET /v1/session`, `GET /v1/companies`, authenticated lifecycle ingress,
`GET /healthz`, `GET /readyz`, and embedded assets. Health returns 204 without database access; readiness
returns 204 when the database identity marker is accessible, otherwise 503.
Merchant setup and scoped API keys are available at `/v1/stores`, `/v1/domains`,
`/v1/installations` and `/v1/credentials`, and in the company settings page.
Campaigns, ads and links are available at `/v1/campaigns`, `/v1/ads`, `/v1/links`
and in the same settings page. `GET /r/{code}` records and redirects clicks.
Website observations, the consent-gated browser SDK and merchant checkout freeze are
available; see [website integration](WEBSITE.md). Merchant `POST /v1/conversions`
durably ingests payments, refunds and cancellations. Scoped user APIs expose paid
orders, applied refunds and safe event diagnostics at `/v1/orders` and
`/v1/merchant-events`; administrators can retry unresolved events. See the
[commerce contract](contract/README.md) for exact money, attribution, retry and
reconciliation semantics. Reports and CSV exports are available at `/v1/analytics`
and `/v1/analytics/export`, with the independent dashboard at `/reports`. See
[reporting](REPORTING.md) for filters, financial clocks and limits.

## Configure and run

Rust 1.95.0 and PostgreSQL 18 are used in CI. Set environment variables explicitly;
the application does **not** load `.env` files. [config.env.example](config.env.example)
documents the actual keys. Provide credentials via your secret manager.

| Key | Behavior |
| --- | --- |
| `ATTRIBUTION_DATABASE_URL` | Required PostgreSQL connection URL; dedicated nonprivileged database owner |
| `ATTRIBUTION_PUBLIC_ORIGIN` | Required HTTPS origin, no credentials/path/query/fragment; HTTP allowed only on loopback |
| `ATTRIBUTION_BIND_ADDR` | Socket address, default `127.0.0.1:8091`; image default `0.0.0.0:8091` |
| `ATTRIBUTION_DB_MAX_CONNECTIONS` | Pool size 1–32, default 5 |
| `ATTRIBUTION_EVENT_MAX_AGE_DAYS` | Required integer 1–3650; original event and paid time delivery horizon |
| `ATTRIBUTION_REFUND_WINDOW_DAYS` | Required integer 1–3650; maximum refund occurrence age relative to payment |
| `ATTRIBUTION_CLICK_RETENTION_DAYS` | Required 1–10000; at least 30 + event delivery days + 1; also checkout receipt lifetime |
| `ATTRIBUTION_OBSERVATION_RETENTION_DAYS` | Required 2–10000; measured from receipt |
| `ATTRIBUTION_FINANCIAL_RETENTION_DAYS` | Required 1–10000; at least refund window + event delivery days + 1 |
| `ATTRIBUTION_EVENT_RETENTION_DAYS` | Required 1–10000; at least event delivery days + 1; unresolved/pending work remains protected |
| `ATTRIBUTION_AUDIT_RETENTION_DAYS` | Required 1–10000; measured from creation |
| `ATTRIBUTION_OPENBOT_ORIGIN` | Required for serve; registered Openbot HTTPS origin (loopback HTTP for tests) |
| `ATTRIBUTION_CLIENT_SECRET` | Required for serve; unpadded base64url of 32 random bytes, registered with Openbot |
| `ATTRIBUTION_LIFECYCLE_SECRET` | Required for serve; distinct 32-byte secret for lifecycle delivery only |
| `ATTRIBUTION_SESSION_KEYS` | Required for serve; JSON `{"active":1,"keys":[{"version":1,"key":"base64url"}]}`; 1–4 unique positive versions, each a 32-byte key |
| `ATTRIBUTION_PREVIOUS_LIFECYCLE_SECRET` / `ATTRIBUTION_PREVIOUS_LIFECYCLE_SECRET_EXPIRES_AT` | Optional pair; previous delivery secret accepted until a specified RFC3339 expiry within 24 hours at startup |
| `RUST_LOG` | Tracing filter, default `attribution_service=info` |

Provision distinct roles and databases for Openbot and attribution. Revoke PUBLIC
access to **both** databases and do not grant either role membership in the other.
For example, as the database administrator (set each role's password separately):

```sql
CREATE ROLE attribution LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE DATABASE attribution OWNER attribution;
REVOKE ALL ON DATABASE attribution FROM PUBLIC;
-- Apply the same PUBLIC revocation to the independently provisioned Openbot DB.
-- Do not grant cross-application CONNECT, schema/table access or role membership.
```

Startup checks the current role is the database's nonprivileged owner. It refuses
to initialize a database with foreign public tables, validates its own identity
marker when present, and runs its own migrations before listening. It does not
audit the other application's database grants; operators must provision both
boundaries. Use PostgreSQL TLS settings in the connection URL for remote databases.
The HTTP listener expects an HTTPS reverse proxy in production.

```sh
cargo fetch --locked
cargo run --locked --offline -- migrate
cargo run --locked --offline -- serve
```

No backward compatibility: incompatible schemas require an operator-managed
database reset. Startup never drops or resets existing data automatically.

Fixed safety budgets: 64 accepted connections, 32 concurrent non-health requests,
256 KiB request body, 10 s handler deadline, 15 s connection idle deadline and
120 s connection lifetime. Saturation returns retryable 429 for request admission;
connection admission waits for an available slot. SQL statements time out after
5 s and locks after 2 s; pool acquisition is bounded to 2 s. Shutdown stops
acceptance, drains for 5 s, closes remaining IO (1 s), cancels and joins
reconciliation (5 s maximum), then closes the pool (2 s).
Runtime and test thread stacks use the stock 2 MiB budget. These are implementation
bounds, not approved production traffic/retention targets.

Errors and request traces omit URLs, query strings, bodies, peer addresses and
credentials. Responses include a UUID request ID, no-store, CSP, nosniff,
no-referrer and framing restrictions; HTTPS origins add HSTS.

## Verify independently

Install SQLx CLI 0.8.6, PostgreSQL 18 client tools (`psql`, `pg_dump`, `pg_restore`),
Node 24, Python 3.11+, and the contract environment described
in [contract/README.md](contract/README.md). Supply
`ATTRIBUTION_TEST_ADMIN_URL` naming the `postgres` maintenance database on a
disposable test server. This admin must create/drop roles and databases. Never use
a production server. Tests allocate UUID-named `as_test_*`/`as_verify_*` resources
and clean only resources they created. Missing database access fails the checks.

```sh
cargo fetch --locked
npm ci --prefix browser
browser/node_modules/.bin/playwright install chromium
scripts/verify.sh foundation     # focused foundation checks
scripts/verify.sh                # entire service gate
scripts/verify.sh prepare        # regenerate .sqlx after intentional SQL changes
python3 scripts/extracted-build.py
```

`ATTRIBUTION_CONTRACT_PYTHON` can name the prepared contract Python interpreter;
`PYTHON` selects the Python interpreter for `verify.sh`. `CARGO_TARGET_DIR` can
name an isolated build cache. The verification gate migrates a fresh owned DB,
checks SQLx metadata against it, checks formatting, compiles offline, runs Clippy,
checks the resolved dependency boundary, and inventories nonzero tests. The full gate
executes database tests and three browser journeys with external network access denied,
validates asset syntax and OpenAPI contracts, and exercises real backup restoration and
representative load bounds. Linux requires sudo plus iptables/ip6tables for the network
guard, plus standard Linux group-management and setpriv tools. The guard scopes rules to
a temporary command group so the CI supervisor keeps its network access; a real socket
regression checks this on Linux. macOS uses sandbox-exec. Extraction copies only this directory and runs the same
gate without parent Cargo configuration. Browser and Python tools remain explicitly
installed dependencies: `ATTRIBUTION_PLAYWRIGHT_MODULE` may point to the installed
Playwright module; extraction supplies the source service's installed module by default.

The boundary checker enforces a single local package with targets under `src/`;
local wrapper crates/custom targets outside that audited layout are rejected. It
rejects outside path dependencies even through wrappers,
Openbot imports/configuration, outside source/assets and literal SQL references
outside service migrations and an explicit PostgreSQL catalog list. This is a
source regression guard; database permissions enforce access at runtime.

```sh
docker build --tag attribution-service-ci .
python3 scripts/container-smoke.py
```

The image runs as UID 10001 on port 8091 with its assets embedded in the binary.
The smoke check uses only its own containers/network; it exercises read-only,
unprivileged startup, migrations, assets, database loss and graceful shutdown.
`ATTRIBUTION_TEST_IMAGE` optionally names the image under test.


## Openbot login and company authority

Configure Openbot with `OPENBOT_ATTRIBUTION_ORIGIN` (this service's exact origin),
`OPENBOT_ATTRIBUTION_CLIENT_SECRET`, and `OPENBOT_ATTRIBUTION_LIFECYCLE_SECRET`.
The callback and delivery paths are fixed by the published contract. Optional
`OPENBOT_ATTRIBUTION_PREVIOUS_CLIENT_SECRET` and
`OPENBOT_ATTRIBUTION_PREVIOUS_CLIENT_SECRET_EXPIRES_AT` support a maximum 24-hour
rotation overlap. These settings are optional as a group in Openbot; incomplete
registration fails startup. Both applications reject reuse of client and lifecycle
secrets across credential classes, including rotation keys. Openbot retains its
existing account/session authority.

Recoverable grants and PKCE verifiers use versioned XChaCha20-Poly1305 envelopes
bound to purpose and record ID. Keep prior decryption keys while their sessions
remain live (maximum eight hours). Browser cookies are host-only, Secure/HttpOnly,
SameSite=Lax; no shared parent-domain cookie or browser-visible provider grant exists.
Sign-out revokes locally before a bounded best-effort upstream revoke, then clears
the cookie and redirects to `/`. A same-target retry validates CSRF against the
retained session record and completes the same redirect. Expired/revoked HTML
sessions return to a sign-in page; APIs retain 401 and authority outages retain 503.

Every user operation checks live Openbot identity and relevant membership. Owner/admin
permissions are read/manage; member permission is read. Authority transport is bounded
to two seconds and 128 KiB per response, does not follow redirects, and returns
retryable 503 on outage. The schema-derived maximum-company-page regression fails
at 80% of that response budget, before another wire-format increase exhausts it.
Tenant ingestion additionally needs active local lifecycle state and a status
observation no older than five monotonic minutes from request start. Restarted
instances and evicted observations must refresh before serving that tenant. Database
wall-clock rollback cannot extend this interval. A notification cannot refresh it.

Openbot commits lifecycle transitions and an immutable outbox entry together,
including direct company deletion and owner-account deletion. Delivery has four
concurrent slots, two-second HTTP deadlines, 30-second fenced leases, twelve maximum
attempts and at most 60-second retry backoff. Exhausted rows remain in durable failed
state. Service reconciliation uses four slots and fenced 30-second leases; successful
reads schedule another read in two minutes, failures back off to 60 seconds. Suspension
blocks tenant operations; deletion is terminal and leaves a minimal tombstone plus
purge request. Commerce purge and operational views are added with those owners.

Both supervised workers probe durable operational state on startup and every 30
seconds, with a two-second probe deadline. Their structured operator-log events are:

| `component` | Alert fields |
| --- | --- |
| `attribution_lifecycle_health` (Openbot) | `undelivered=true` for pending/in-flight changes; `failed=true` for exhausted deliveries |
| `attribution_company_status_health` (attribution) | `expired=true` for active tenants without current durable status; `reconciliation_failed=true` for unresolved refresh failures |

An affected probe emits WARN; a healthy probe emits INFO with those fields false,
so successful repair clears the signal. `available=false` emits WARN if storage or
the probe deadline prevents observation. Configure the operator log monitor to
alert on these fields (and missing probe events for process liveness), and use the
all-false event for recovery. These are persisted-state probes, not process-local
counters; restart does not clear an outstanding failure. They expose no global
monitoring route to company users and include no credentials or company names.
These signals do not configure a hosted paging destination; operator-log access
and alert routing belong to the deployment.

Startup validates local configuration and storage, but does not probe Openbot or
promise an available upstream login. `/readyz` currently reports storage readiness.
Production region, retention and load targets remain deployment decisions.

The real-browser integration fixture is separate from standalone unit checks. Install
`npm ci --prefix browser` and `npx --prefix browser playwright install chromium`, build
the service binary, then run the owning Openbot test with `--ignored --nocapture`:
`attribution_sso_browser_real_openbot_password_and_existing_session`.
It requires `ATTRIBUTION_TEST_BINARY`, the service maintenance fixture URL and the
Openbot test database configuration. `ATTRIBUTION_PLAYWRIGHT_MODULE` may select an
isolated installed Playwright module; otherwise it uses `browser/node_modules`.
The fixture exercises the real password form and session authority across two origins,
then existing-login continuation, cookie separation, rotation and upstream revocation.

## Stores, approved domains and merchant credentials

Company owners/admins manage setup; members can read it. Every user request checks
live Openbot membership. Select the company with `X-Company-ID`; mutations also
require the session CSRF token, exact Origin, `Idempotency-Key` UUID and the original
`Idempotency-Created-At` RFC3339 time. Keep key/time/payload unchanged on retry. The
24-hour retry window, future skew of 30 seconds and metadata-only secret replay are
specified in the network contract. Receipts and audit records commit with the change;
receipts are retained at least 48 hours. A lost secret response must be recovered by
revoking the returned credential and issuing a new one with a new operation identity.

A store uses an IANA timezone and exact domain approvals. Add the DNS hostname,
then publish the returned `_attribution.<host>` TXT value. Verification checks that
value and all returned A/AAAA addresses through fixed Cloudflare public recursive
DNS servers, with a two-second total deadline and no system search suffixes or hosts
file. DNS access must be permitted from the deployment. The service never fetches a
submitted destination. Both address families must resolve successfully or explicitly
have no records; a timeout in one family cannot be hidden by a public answer in the
other. Private, special-purpose and tunnel address ranges are rejected.

Domains are normalized with IDNA, matched exactly and limited to 240 ASCII bytes so
the challenge name fits DNS's 253-byte bound. IP literals, local/special hostnames,
userinfo, non-HTTPS destinations and ports other than 443 are rejected. Approval
expires after 30 days; keep the TXT record and reverify before expiry. Revocation is
terminal for that domain record. Re-add requires a new proof and installation.
Existing installations fail closed when their store/domain is disabled, expired or
revoked. DNS proofs are public configuration, not merchant secrets.

Public installation IDs enable only `image_load`, `landing_page_load` and
`visible_placement` observations. They never authorize sales, refunds, reports or
configuration. API keys stay on the merchant server. They carry one company/store
and explicit scopes: `conversions:write`, `checkouts:write`, `links:read`,
`links:write`, `analytics:read`. Keys use 256 random bits and are stored only as
SHA-256 verifiers; listing, receipts and audits contain no recoverable secret.

Creation accepts optional expiry. Rotation copies scopes/label/original expiry to
a replacement and caps the old key's expiry at the requested overlap (0–86400
seconds). A credential can rotate once. Retrying that same operation returns the
replacement metadata without extending overlap; revoking the replacement does not
restore the original. Key metadata exposes creation, expiry, revocation and last
committed use. An administrator's live user session can always issue a replacement.

Merchant requests authenticate before reading the body, use local credentials and
cached company freshness, then recheck key/store/company under the tenant lock in
the mutation transaction. Last use commits with that mutation. A write already
holding the lock may finish before revocation; after revocation commits, no later
write can use that key. The same lock orders store changes and lifecycle updates.
A stale authority observation returns 503. Setup and merchant admission allow eight
concurrent requests per tenant, in addition to the existing process bounds. The
registry retains only active requests and has a 1024-tenant hard bound.

Run phase-four checks with PostgreSQL and the pinned browser tooling:

```sh
cargo test --locked --offline --lib setup
cargo test --locked --offline --lib attribution_setup_browser_ -- --ignored --nocapture
```

The second command requires Playwright Chromium. It exercises the real setup forms,
HTTP/session boundary, PostgreSQL, and a local DNS server; its upstream identity
provider is a fixture. The separate Openbot browser test verifies the actual login
bridge. These fixtures do not establish public DNS availability or deployment policy.

## Links and durable redirects

Owners/admins manage campaigns, ads and placement links; members can inspect them.
Merchant keys use `links:read`/`links:write` only within their store. Campaign/ad
management uses the live user session. The management retry contract above applies
including to link updates. Campaign/ad names and advertised product references can
be edited; each click retains its original dimension and destination snapshot.
Omitting `advertised_product_id` in an ad update leaves it unchanged; `null` clears it.

Links use a random 24-character code, separate from ad and per-request click UUIDs.
Destinations must match an active, exact approved store domain at creation, update
and redirect. The reserved decoded query key `ob_click` is rejected. A successful
redirect adds one fresh click UUID while preserving other query pairs and fragments.
Disabled campaigns, ads or links prevent new clicks and retain existing history.

Each GET commits its own raw click before returning 302 with no-store/no-referrer.
A storage error or uncertain commit acknowledgement returns retryable 503 without
Location. The database can retain a committed click when that response is lost;
a repeated GET is a new raw click. HEAD returns 204 for an eligible link and creates
no click or Location. CDN/proxy configuration must bypass caching for `/r/` and
preserve these response headers.

Only booleans `is_bot`, `is_prefetch` and classifier version 1 are retained with the
click. Classification inspects at most 512 bytes of User-Agent for `bot`, `crawler`,
`spider`, `headless`, `preview`, `curl/`, `wget/`, and Purpose/Sec-Purpose for
`prefetch`, case-insensitively. These are heuristics, not human verification; raw
counts include every accepted GET. No raw user agent, referrer or IP is stored,
and forwarded IP headers confer no authority. There are 16 public redirect slots
and 8 shared operations per tenant, within the global limits above; overload is 429.

Focused tests use `cargo test --locked --offline --lib catalog`. They include a
PostgreSQL wire relay that discards an actual successful COMMIT acknowledgement,
a real HTTP client disconnect, competing writes and a local 100-request concurrency-8
latency fixture with p95 below 1000 ms. This is a regression budget, not a production
SLO. The ignored `attribution_setup_browser_real_forms_and_postgres` test also covers
catalog forms, response-loss retry and navigation to a local HTTPS merchant fixture
(requires `openssl` to generate an ephemeral test certificate);
the redirect and click commit run through the real service and PostgreSQL.

Operations, independent backup/restore, secret provisioning and network deployment are documented in [operations.md](operations.md).
