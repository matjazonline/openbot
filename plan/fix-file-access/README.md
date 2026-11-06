# Fix Doop private file and image access

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Problem, scope, and status

Status: proposed implementation, 2026-11-06. This plan fixes the confidentiality gap in our
self-hosted **Doop service**, not an established defect in Openbot's own file routes. Implementation
belongs in the actual Doop checkout/fork, with its accepted revision/image recorded here.

At inspected upstream revision [`01584b34f6cadcf21767127280cc6e320a6d748a`](https://github.com/kgoedecke/doop/tree/01584b34f6cadcf21767127280cc6e320a6d748a),
private canvases coexist with unauthenticated `/i/<frameId>.png|jpg` frame images and
`/a/<assetId>.<ext>` uploaded assets. Knowing a URL is sufficient to retrieve bytes. Frame images
are cached publicly for 60 seconds; uploaded assets advertise a year of public immutable caching.
The canvas/MCP access check therefore does not establish confidentiality for those files.

Fix resource ownership, enforce read authority before serving any private bytes or cached result,
and preserve authorized canvas editing, screenshots, fonts/images, and PDF export. The first
deployment is private: anonymous sharing, public hotlinks, and Community publication of managed
company canvases are disabled. Do not add public bearer download tokens as a shortcut.

This is an independent prerequisite for confidential use of [the Doop integration](../doop-canvas/README.md).
No real company content may be sent to the affected deployment until this plan's acceptance passes.

## Implementation order

| Group | Deliverable | Dependency |
| --- | --- | --- |
| [01 — Resource ownership and policy](01-resource-ownership-and-policy.md) | Exhaustive surface inventory and canvas-bound private assets | Pinned running Doop revision |
| [02 — Media authorization and caching](02-media-authorization-and-caching.md) | Authorized GET/HEAD/download/revalidation and private storage | 01 |
| [03 — Rendering and export](03-rendering-and-export.md) | Private asset loading without weakening sandbox or renderer isolation | 02 |
| [04 — Verification and deployment](04-verification-and-deployment.md) | Adversarial two-company tests, full gate, cache/storage cutover | 01–03 |

## Source anchors and invariants

All upstream links below refer to the inspected revision; locate exact corresponding symbols in
the deployed revision before editing. This repo does not currently contain that Doop source.

- [Public routes](https://github.com/kgoedecke/doop/blob/01584b34f6cadcf21767127280cc6e320a6d748a/server/index.ts#L418):
  `/i/` and `/a/` return bytes without the canvas read gate.
- [Canvas policy](https://github.com/kgoedecke/doop/blob/01584b34f6cadcf21767127280cc6e320a6d748a/server/access.ts):
  `canvasRole`, `hasDurableCanvasAccess`, and membership/sharing ownership.
- [Assets](https://github.com/kgoedecke/doop/blob/01584b34f6cadcf21767127280cc6e320a6d748a/server/assets.ts):
  upload tickets, optional canvas ownership, storage, references, and remote image fetching.
- [Storage](https://github.com/kgoedecke/doop/blob/01584b34f6cadcf21767127280cc6e320a6d748a/server/storage.ts),
  [renderer](https://github.com/kgoedecke/doop/blob/01584b34f6cadcf21767127280cc6e320a6d748a/server/screenshot.ts),
  and [PDF export](https://github.com/kgoedecke/doop/blob/01584b34f6cadcf21767127280cc6e320a6d748a/server/pdfExport.ts)
  are part of the boundary; changing Express routes alone is insufficient.
- [MCP exports](https://github.com/kgoedecke/doop/blob/01584b34f6cadcf21767127280cc6e320a6d748a/server/mcp.ts#L1439)
  currently describe image URLs as public, while PDF export embeds bytes behind a canvas gate.

Every private resource has an authoritative parent canvas. A URL, random ID, filename, MIME type,
Referer, CORS header, or cache hit is not authorization. Session/bot authority and current canvas
access must be evaluated before metadata or bytes are disclosed. Untrusted HTML cannot turn a
foreign asset ID into an authorized reference. Failure to load authority is an error, never public
access. Trusted operators remain able to access infrastructure; downloaded copies cannot be revoked.

Keep every Markdown file here registered in `NO_BACKWARD_COMPATIBILITY_NOTES` in Openbot's
`src/main.rs`. Every group records focused checks; the last group owns the final service-wide gate.
