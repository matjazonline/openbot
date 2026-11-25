# 01 — Tracking host and consent settings

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on the implemented attribution service and on decisions D2 and D3 in the
[README](README.md). A store can register a tracking host on its own domain with one DNS record,
the host gets its certificate automatically, the service knows which host a request arrived on,
and one tested function answers "does this request need consent before a visitor cookie?". No
cookie is set in this phase.

## Batch 01A — Register and verify a tracking host

1. Add `attribution_tracking_hosts` to the one migration, scoped by company and store: id, host,
   `verified_until`, `revoked_at`. A host is unique across the whole deployment, and a store has
   at most one active host.
2. Accept a host only when it is exactly one label below an active approved domain of the same
   store, or below that domain without a leading `www.`. So `go.shop.example` is accepted for
   `shop.example` and for `www.shop.example`. This guarantees the host is same-site with the shop
   without a public-suffix dependency. Reuse the service's canonical host rules and refuse the
   attribution deployment's own host.
3. Verify with one DNS record: the host must have a CNAME pointing exactly at the deployment's
   tracking target, named by a new configuration key `ATTRIBUTION_TRACKING_CNAME_TARGET` (a
   hostname validated at startup; while it is unset, registering a tracking host is refused).
   No TXT record is needed: the host sits under a domain this store already proved it controls,
   and the CNAME shows that domain's owner pointed the host at the deployment. Use the existing
   resolver path and time limits, and re-check on the approved-domain schedule. A host whose
   CNAME stops pointing at the target, or whose parent domain loses verification or is revoked,
   stops being active.
4. Add `POST`, `GET` and `DELETE /v1/tracking-hosts` and a settings section, behind
   `integrations:manage`, with the service's idempotent management receipts and audit rows.
5. When a store has an active tracking host, link listings and the settings page show each
   link's address on that host. Links on the shared address keep working.

Focused checks: `attribution_tracking_host_` for accepted and refused shapes (sibling domain,
two labels below, the bare approved domain, another store's domain, the deployment's own host,
internationalized names), registration refused while the target key is unset, and against the
DNS test resolver: a correct CNAME, a CNAME to another target, an address record instead of a
CNAME, a CNAME removed after verification, and parent revocation; plus permission denial and two
stores competing for one host ending with one owner.

## Batch 01B — Bind routes to the host a request arrived on

1. Read the request host from the `Host` header the proxy forwards, canonicalize it, and classify
   it once per request as an active tracking host of one store, or as anything else. The service
   does not check the host today, so "anything else" keeps today's behavior exactly.
2. On a tracking host serve only `GET /r/{code}`, the browser script, `/pixel.gif`, `/v1/events`
   and the consent routes added in [02](02-visitor-cookie-and-consent.md). Everything else is not
   found, including sign-in, the dashboard and every management route.
3. On a tracking host, a link, installation or event that belongs to another store is not found.
4. On any other host everything behaves as today, and the consent routes are not found.
5. A missing, repeated or malformed `Host` is never a tracking host.
6. Certificates come automatically. Add a route, served only on hosts that are not tracking
   hosts, that answers 200 when a hostname is an active tracking host and 404 otherwise. A proxy
   with on-demand certificates (for example Caddy's on-demand TLS with its `ask` check) calls it
   before requesting a certificate on the first TLS connection for a host. The answer reveals only
   what the host's public DNS record already shows; bound it like the other public routes.
   Replace the sample proxy with one that does this, keeping access logs off, `X-Forwarded-For`
   cleared and, from 01C, the country header overwritten. Document the operator's certificate
   account and the certificate authority's rate limits.

Focused checks: `attribution_host_routing_` as a table of host class × route asserting status and
that no response on a tracking host sets or reads a dashboard cookie; a foreign store's link on a
tracking host; missing, repeated and malformed hosts; unchanged behavior for every existing route
on every other host; and the certificate check answering for active, unverified, revoked, unknown
and malformed hostnames and absent on tracking hosts.

## Batch 01C — Store consent setting and the requirement rule

1. Add to `attribution_stores`: `visitor_consent` (`required` or `by_region`, default `required`)
   and `consent_exempt_countries` (at most 64 distinct ISO 3166-1 alpha-2 codes, upper case,
   empty unless the mode is `by_region`). Constrain both in the schema.
2. Changing either needs `integrations:manage` from a signed-in user; a server key cannot change
   them. Turning on `by_region` requires an explicit acknowledgement in the request and writes an
   audit row naming the actor.
3. Add an optional configuration key `ATTRIBUTION_COUNTRY_HEADER`: the name of the request header
   in which the trusted reverse proxy puts the visitor's country. Validate the name at startup.
   When it is unset, every visitor's country is unknown. Document it in the README and
   `config.env.example`, and state that the proxy must overwrite any value a client sends.
4. A header value counts only when it is exactly two ASCII upper-case letters and appears once.
   Anything else is unknown. Never log the value.
5. Write one pure function that takes the store's setting and list, the country or unknown,
   whether `Sec-GPC: 1` is present, and whether `ob_optout` is present, and returns whether
   consent is required. It returns "not required" only when the mode is `by_region`, the country
   is on the list, and neither opt-out signal is present. Every later phase calls this function;
   nothing re-derives the rule.
6. Add a country lookup to the sample proxy and document two supported sources: a CDN's country
   header, or the proxy's own IP-to-country module with a database the operator keeps current.
   Name the database's license and update duty in the deployment document.

Focused checks: `attribution_visitor_region_` as an exhaustive table over the rule's inputs,
including unset header configuration, lower-case, three-letter, repeated and empty values;
`attribution_store_consent_` for schema bounds, key denial, the acknowledgement and its audit
row, and clearing the list when the mode returns to `required`.

Completion: hosts are registered, verified and routed, and the consent rule exists and is tested,
with no visitor cookie yet. Record commands, counts and results here; regenerate SQLx metadata;
update the contract. Full-suite gate: [03](03-reports-retention-and-final-verification.md).
