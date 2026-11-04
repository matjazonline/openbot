# 05 — Links and durable clicks

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [04](04-merchant-setup-and-api-credentials.md). A permitted user or management client
creates an ad-specific link; a visitor receives a distinct durably stored click and a correct redirect.

## Batch 05A — Create campaigns, ads and links

1. Add company/store-scoped campaigns, ads, links, placement metadata, and optional advertised
   product references. An ad ID identifies an ad; a link code identifies a placement link; each
   click ID identifies one accepted redirect request. Use opaque unguessable tracking identifiers.
2. Implement `POST /v1/links` and the bounded list/detail/update/disable APIs required by the
   attribution-owned link UI. Include idempotency for retried creation, with a scoped key and
   payload fingerprint; a reused key with a different payload conflicts.
3. Validate destination URLs against active store-approved domains at creation and redirect time.
   Store a canonical destination. Reject reserved tracking parameter collisions; merge the click
   parameter using a URL parser while preserving unrelated parameters, duplicates and fragments.
4. Keep historical attribution stable when names, destination or advertised product metadata
   change: record the relevant ad/link revision or dimension snapshot with the click. Disabling
   a link prevents new clicks without erasing historical reporting.

Focused checks: `attribution_links_` for permission/scoped-object checks, identical and conflicting
creation retries, competing idempotency requests, metadata history, disabled links, and query/fragment
preservation. Execute real database constraints, not only in-memory authorization mocks.

## Batch 05B — Commit clicks before redirecting

1. Implement `GET /r/{code}`. Resolve the active company/store/link, allocate a per-request click
   ID, timestamp it server-side, and durably commit before returning HTTP 302 with
   `Cache-Control: no-store`. Apply privacy/referrer headers and disable CDN caching for tracking.
2. Treat separate GET requests as separate raw clicks, including link scanners and browser
   retries. HEAD/health probes must not create clicks. Do not deduplicate ordinary clicks by IP,
   fingerprint, or visitor identity; there is no reliable browser delivery event ID on this route.
3. Store a bounded bot/prefetch classification and classifier version with the click. Report raw
   and filtered counts separately; heuristic filtering is not a guarantee of human activity.
   Avoid raw IP retention and full user-agent/referrer collection unless explicitly justified.
4. Implement phase 01's failure contract: database/commit uncertainty returns a retryable 503 and
   no tracked redirect; unknown/disabled codes return an appropriate non-redirect error. Record
   committed clicks even if the client disconnects after commit; receipt of a page is not assumed.
5. Bound per-tenant and public endpoint load, connection concurrency, database pool waits and
   request latency. Trust forwarded client information only from configured trusted proxies.

Focused checks: `attribution_redirect_` for one committed row per GET, fresh IDs, 302/header/URL
semantics, HEAD, bots, revoked domains/company, storage failure, unknown commit outcome, and client
disconnect. Use concurrent requests against real PostgreSQL and prove no duplicate IDs or tenant
crossing. Measure the initial redirect latency against the phase 01 pilot target.

Completion: a link can be created, clicked, and inspected from attribution while Openbot contains
no click storage or tracking route. Record batch commands/counts/results and regenerate owning SQLx
metadata. Full-suite gate: [10](10-integration-verification-and-pilot.md).
