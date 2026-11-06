# 04 — Verification and deployment

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on 01–03. Prove confidentiality against the actual pinned Doop deployment and record an
accepted source revision/image for the consuming [Openbot plan](../doop-canvas/README.md).
Publishing these requirements does not claim that the defect has already been fixed.

## Batch 04A — Focused and adversarial acceptance

The named test files are new requirements, not tests already present in Openbot or upstream.
Run from the Doop checkout; list selected cases and record nonzero executed counts. Based on the
inspected package's existing Vitest script, focused commands after adding those files are:

```sh
bun run test tests/privateAssetOwnership.test.ts
bun run test tests/privateFileAccess.test.ts tests/privateMediaCache.test.ts
bun run test tests/privateRendering.test.ts tests/privateExport.test.ts
```

Use synthetic private canvases/assets for companies A/B, separate bots and human accounts,
anonymous clients, real HTTP/WS/MCP paths, and Chromium. Use isolated PostgreSQL with fresh schema
and both disk/private bucket storage. Do not silently skip unavailable databases/browsers/storage.
Assert both allowed behavior and denial, including actual byte/metadata absence and render counts.

Cover known URLs, foreign/absent parents, managed sharing restrictions, forged HTML references,
upload/copy ownership, expired/revoked credentials, GET/HEAD/range/conditional/download variants,
warm server/browser/proxy caches, object-storage origins and redirects. Test revocation or deletion
competing with active upload/copy/render/cache fill; assert the chosen authorization point and
resource ownership rather than promising retroactive recall of bytes already delivered.

The real browser must still display private images/fonts and export accurate multipage PDFs.
The renderer cannot use a foreign asset as a confused deputy. No UI/MCP/export fallback restores
public access. Capture redacted, reproducible proof against the final image, not just mocks.

## Batch 04B — Final Doop service-wide gate

Run focused checks while implementing; run the entire service gate once after integration.
Reconcile these existing package script names against the accepted revision. Pin Bun, Node,
Chromium and dependency lockfile, provision dependencies before network-restricted checks, and
record exact commands, versions, results, executed counts, and unavailable checks.

```sh
bun install --frozen-lockfile
bun run format:check
bun run typecheck
bun run lint
bun run doc-lint
bun run test
bun run build
```

Explicitly verify fresh PostgreSQL migration/startup for changed resource constraints, persistence
after restart, production image/non-root operation, and real browser/renderer integration. Add the
new ownership, access, cache and renderer tests to Doop CI. Keep resource/renderer-budget signals
when raising any bound; do not accept unlimited asset embedding as the fix.

If the implementation changes Openbot, run its focused affected checks and the final full CI gate
in [doop-canvas/06](../doop-canvas/06-integration-and-verification.md) once after integration.
Doop-only changes do not falsely require, or count as evidence from, an untouched Rust service.

## Batch 04C — Storage/cache cutover and accepted deployment

Deploy a source/image revision containing the fix, with private disk/bucket routing and no direct
public storage/CDN path. Inventory and purge existing shared/CDN caches, remove obsolete public
object exposure, and reset incompatible database/blob state under the repository reset policy.
Do not retain a legacy public URL handler or dual public/private serving mode.

Changing cache headers cannot recall publicly cached copies. If the affected instance has ever
held sensitive real data, identify the exposure and obtain the appropriate operator's handling
decision before destructive cleanup; do not claim a patch or ID rotation erased downloaded copies.
Test clean private state with new identities/objects after cache/storage cutover. Never point
disposable destructive tests at the live company's database or bucket.

Verify the actual ingress, alternate origins, object storage and warmed-cache paths externally
after deployment. Keep confidential integration disabled until those probes pass. Do not roll
back to an image with public media serving; restore only a known secure revision, reset/re-provision
incompatible state, or keep the integration unavailable.

Record the accepted Doop commit/image, applied schema, proxy/storage policy, source changes,
runtime verification, remaining limitations and operator runbook. The Openbot integration consumes
that exact acceptance evidence. Repeat affected tests after fixes and the full gate when warranted;
refresh the relevant repository graph after substantial implementation.
