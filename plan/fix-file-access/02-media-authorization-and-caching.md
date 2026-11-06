# 02 — Media authorization and caching

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on 01. Enforce resource authority on the actual service/storage paths, including cache
hits. Removing URL fields from tools or hiding download buttons does not fix the underlying read.

## Batch 02A — Gate every private read

Extend the existing Doop authentication and canvas policy rather than implementing a parallel
file ACL service. Apply it before `/i/`, `/a/`, thumbnails and other inventoried private surfaces
perform expensive rendering, return object metadata, or access bytes. Resolve the authoritative
parent and check current access on every request. Use existing MCP identity handling for MCP
operations; do not accidentally make every browser file URL accept a broad admin/service secret.

Use consistent denied/missing behavior to avoid foreign-resource enumeration. Malformed paths,
extension mismatches, absent ownership, revoked membership and expired/revoked credentials fail
closed. Preserve distinguishable internal operational errors without leaking file contents or
existence to unauthorized clients. Bound path parsing, response size, range count and render work.

Enforce the same policy for GET, implicit/explicit HEAD, range/partial responses, conditional
requests, download variants, alternate hosts and redirects. An unauthorized request must never
receive private bytes, a 304 authorizing reuse, content length/type, or an object-store location.
Do not authorize by Referer, guessed high-entropy IDs, origin headers, or blanket CORS settings.

**Focused checks:** `tests/privateFileAccess.test.ts` with real HTTP requests: anonymous, foreign,
authorized, expired session, revoked bot, malformed/missing ID, HEAD/range/download/conditional
variants, and auth/database failure. Assert denial happens before storage fetch or rendering.

## Batch 02B — Keep caches and storage behind the gate

Return `Cache-Control: private, no-store` for private file/image/download responses and remove
public immutable/shared caching. Ensure Express validators, 304 handling, reverse proxies, CDNs,
service workers, and dashboard caches cannot bypass current authorization. Include appropriate
credential variance where relevant; `Vary` alone is not an authorization mechanism.

Server-side rendered-byte caches may be reused internally only after each caller passes the
current policy. Keep render/content revision and ownership in the cache identity or invalidate
correctly when either changes. Membership revocation must not wait for cached-byte expiry.

Keep disk assets outside public static roots. Require private buckets/object ACLs and prevent a
CDN or storage endpoint from exposing objects directly. Do not redirect private downloads to
unrestricted object URLs or issue long-lived bearer URLs. Ensure errors and logs do not echo
credential-bearing URLs or private object metadata. Retain MIME sniffing, `nosniff`, safe download
names, SVG/content security, and existing storage/SSRF bounds.

**Focused checks:** new `tests/privateMediaCache.test.ts`: warm as A, then fetch anonymously/as B;
revoke A and retry; race revocation/deletion with cache fill; test conditional requests and direct
storage URLs. Exercise disk and private bucket implementations with isolated fixtures. Verify
no unauthorized 200/206/304, redirect, bytes, or metadata, including proxy/CDN configuration probes.

## Exit criteria

Known private URLs are insufficient to read files, even on a warmed cache. Storage has no parallel
public path. Authorized reads still work without weakening type, size, CSP, or SSRF protections.
