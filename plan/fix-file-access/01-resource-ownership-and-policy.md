# 01 — Resource ownership and policy

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Start from the running Doop revision and its applicable repository instructions. Trace resource
creation, references, storage, readers, and deletion before editing. The [README](README.md) anchors
are evidence for the inspected upstream version, not a complete inventory of our deployment.

## Batch 01A — Enumerate every read and creation surface

Use exhaustive symbol/path searches and caller tracing for frame images, preview/thumbnail caches,
uploaded images/fonts/files, upload tickets, screenshots, PDF exports, prototype/player views,
Community copies, downloads, WebSocket/MCP responses, and direct object-storage/static URLs.
Include HEAD, ranges, conditional requests, redirects, alternate origins, and generated HTML/CSS
asset references. Record which surfaces expose bytes, identifiers, or metadata before auth.

Inventory disk/bucket modes, bucket ACLs, CDN/proxy behavior, rendered-preview storage, and HTML
export references. Verify uploads and background import/agent jobs retain a validated canvas
identity. Enumerate ordinary ownerless/user-library asset behavior instead of assuming every
asset already carries `canvasId`.

**Focused checks:** reproduce unauthenticated image/asset access with synthetic private data in
two workspaces, including one warmed cache. Add failing `tests/privateFileAccess.test.ts` cases
for the known bypasses. Record the deployed revision, routes, storage modes and executed counts.

## Batch 01B — Put ownership in the resource layer

Use one authoritative private-resource policy based on the existing canvas access owner. Resolve
a frame's parent from stored state, not a caller-supplied workspace/canvas ID. For managed company
canvases require authenticated durable membership/ownership and prohibit public-link/Community
access; do not leave public media enabled as a separate sharing policy.

Make each private uploaded asset belong to one canvas with an enforced relationship. Require
validated canvas authority at creation/upload/import and ensure upload tickets cannot change that
identity. Unknown or missing parent ownership fails closed. Reset incompatible existing state;
do not backfill ownership by scanning arbitrary HTML or preserve ownerless access fallbacks.

An asset ID in model-authored HTML is not proof of permission. Validate references on writes and
again when materializing content for a reader/renderer. Copying private content between canvases
requires authorized source read and destination edit, creates a destination-owned asset identity,
and preserves destination access policy. Do not build a shared-private-asset ACL graph or silently
adopt foreign assets based on their URL. Trace font libraries and Community/import copies too;
if those private producers exist, update them to the same ownership rule.

Reuse existing schema/storage owners, constraints, transaction boundaries and domain types.
Delete cascades/retention must not orphan publicly readable blobs. Keep any intentionally bundled
public application assets separate by construction from company-uploaded files.

**Focused checks:** new `tests/privateAssetOwnership.test.ts` database-backed cases for validated
upload tickets, missing/foreign parents, forged HTML/CSS asset references, source/destination copy
authorization, and owner deletion. Use competing upload/copy/delete claimants where new locking
or ownership transitions are introduced; sequential mocks do not establish the protocol.

## Exit criteria

The inventory is complete for the deployed source. All private byte producers establish a valid
parent canvas, and the common read policy has no ID-possession, ownerless, or public-sharing bypass.
