# 04 — Results and human access

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependencies

Depends on 03. Deliver a private editable canvas link through existing workflow/run result and
message surfaces. Confidential acceptance also requires the completed [file-access fix](../fix-file-access/README.md).
Do not build a new document library, public file host, or browser-side MCP proxy for this release.

## Batch 04A — Validate and publish the result

Derive the canvas URL from the configured canonical Doop origin and the verified canvas ID; do
not trust an arbitrary tool-provided URL, HTML link, redirect, or model-authored origin. Check the
result against the intended workspace through the authenticated connection before final success.
Reuse domain identity/URL validation and existing result schemas; do not guess foreign ownership
from the shape of an ID or absence from one paginated list.

Return the canvas identity, approved origin/link, title, and accurate completion/review status.
Keep the existing run's access level, provenance, and recipient/egress checks on summaries,
notifications, and replies. A private link's title and surrounding text can still disclose data.
Reject content whose source audience is narrower than the Doop workspace's approved membership.
Do not relax Openbot access levels or auto-invite a message recipient so a link will work.

Use existing message rendering and link sanitization with escaping and accessible labels. Do not
embed public thumbnails or automatically fetch previews from unvalidated tool output. Missing
Doop membership is a clear access/setup error rather than a request to turn on public sharing.

**Focused checks:** `doop_result_` tests for forged/foreign URLs and IDs, incorrect workspace,
escaping, access-level propagation, narrower source audiences, failed final read, and delivery to
unauthorized recipients. Run affected result/message/browser fixture tests.

## Batch 04B — Human opening and export

Document explicit human account invitation to the company's Doop workspace, normal Doop sign-in,
and opening/editing the returned canvas. Openbot company membership and Doop membership are
separate until a future authorized synchronization feature exists. Explain edit rights rather
than promising a read-only workspace role that upstream does not provide.

Verify the actual pinned deployment can export a private canvas to a multipage PDF through its
authorized Doop UI. Keep PDF bytes out of normal LLM output and workflow summaries. If raw MCP
PDF resource forwarding is not useful within existing budgets, do not add a storage subsystem:
the supported release path remains the canvas link and Doop's authenticated download.

**Focused checks:** a real-browser acceptance with human A/B: A opens A's document and exports a
PDF including private images/fonts; B and a signed-out browser cannot open A's canvas or fetch
its media. Repeat after membership removal, including warmed previews and browser revalidation.

## Exit criteria

The user receives a working private canvas link with honest status and no new access grant.
Authorized people can edit/export; strangers cannot use media URLs to bypass canvas permissions.
