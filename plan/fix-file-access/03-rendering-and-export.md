# 03 — Rendering and export

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on 02. Preserve private image/font rendering in the browser, screenshots, prototypes and
PDF export without restoring anonymous access or giving untrusted frame HTML a powerful session.

## Batch 03A — Authorized resource materialization

Trace existing sandboxed iframe and isolated Chromium asset loading. Opaque-origin frames/fonts
may not send the parent application's session, so putting a cookie gate in front of `/a/` is not
proof that editing/rendering still works. Keep iframe isolation, per-render browser contexts,
SSRF/network restrictions and CSP; do not enable same-origin scripting or share a logged-in
browser profile to recover asset loading.

Materialize referenced private assets through the existing application/renderer owner after
validating the caller, parent canvas, and each reference. Reuse suitable embedding helpers; pass
bounded bytes as data/blob resources or fulfill exact validated render requests from server-side
private storage. Preserve font MIME/CORS requirements without exposing a public asset endpoint.
Do not attach user cookies/bearer headers to arbitrary frame network requests, make an internal
unauthenticated download listener, or interpret arbitrary local URLs as trusted file references.

Give every render a validated canvas context; an export authorized for A cannot load B's assets
from malicious HTML/CSS, imports, redirects or SVG/font references. Keep aggregate asset count,
decoded-byte, frame-size, concurrency and deadline limits. Report missing/denied assets honestly;
do not publish a blank export as a fully verified document.

**Focused checks:** new `tests/privateRendering.test.ts` with real isolated Chromium and browser
iframes: private images/fonts load for A; foreign asset IDs and malicious URL/SVG/CSS references
fail; cookies/keys never enter frame HTML or foreign requests. Cover limit rejection and missing
assets. Verify screenshots, dashboard previews and multipage PDFs with both disk/bucket fixtures.

## Batch 03B — Export consumers and revocation

Update MCP/UI export descriptions and guides that currently promise public hotlinks. PNG/JPG
exports must return authenticated download locations or bounded authorized content supported by
the actual consumer. Retain PDF's parent-canvas checks and embedded-resource/download semantics;
do not replace them with publicly accessible files. A URL returned to an MCP bot is not evidence
that a signed-out human or unrelated HTTP client can fetch it.

Trace exported HTML, prototypes, notifications, OG images, thumbnails and Community features.
Private content must not be published through those alternate consumers. Keep managed company
content private; no public-sharing/token feature is part of this fix. Document unsupported
external hotlink consumers and require an explicit separately authorized publication mechanism
for any future public release.

Remove stale private media from application/service-worker caches on logout, access revocation
or owner deletion where the client can observe it. Each new server request reauthorizes. Explain
the practical limit: bytes already downloaded or copied by an authorized person cannot be recalled.

**Focused checks:** new `tests/privateExport.test.ts` plus existing MCP/preview/PDF/browser suites.
Verify download and embedded-resource shapes, altered tool descriptions, export-after-revocation,
wrong-canvas frame selections, and no public fallback on rendering failure. Run the Doop/Openbot
consumer acceptance with synthetic content so changed URL semantics cannot fail silently.

## Exit criteria

Authorized editing and export remain usable with real images and fonts. No rendering or export
consumer reintroduces a public URL, leaks a browser/session credential, or reads a foreign asset.
