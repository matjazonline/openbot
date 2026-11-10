# Pinned Doop integration

Upstream: https://github.com/kgoedecke/doop (AGPL-3.0). `managed.patch` contains the
source delta. Materialize with `scripts/doop-test.py setup --managed`; see
[repeatable checks](../../docs/doop-testing.md). This package preserves upstream licensing.

Configure, review and verify a company connection using the
[operator guide](../../docs/doop-connections.md), [minimal catalog](catalog.json) and ordinary
[read-only verification workflow](verify-company.workflow.json).
Operate rotation, disconnect, backups and retention using [the runbook](../../docs/doop-operations.md).

- Base revision: `01584b34f6cadcf21767127280cc6e320a6d748a`.
- Full source fingerprint (base, paths, modes and bytes): `e0c063c43c5bc70018eb8e09d0add0d32ef35159fff843bc9646da7ebe4c4464`.
- Patch SHA256: `4621d91534b24db5e5cb35f535bda9d579c808120eddcf3876ad67d793ed298c`.
- Managed lock SHA256: `9fc8b3c110b390a9c46975da45c3a297090096703f9d9721dcea19f84dad28e3`.
- Bun 1.3.10; MCP SDK 1.30.0; negotiated protocol 2025-03-26.
- Tested linux/arm64 OCI manifest: `sha256:425a2d2e718285f11fd9b18bf110dd173c689d0f0378a94f5ebe4f9872e52d43`.
- Image configuration: `sha256:1f301a9894e089a8011846ec974bf423115a5357c730e74bd4961a2ccbacc225`.
- OCI archive SHA256: `7e85e67b892e8b0c523f089cbe2b8a4612861634e9ed5b7c5a64efba721e046d`.

The retained image was built from these exact source inputs. Rebuilding can produce a different
OCI digest because build-layer metadata is not claimed byte-for-byte deterministic. Every build
records its actual digest. Use the retained archive for exact replay; source and dependencies
remain pinned. The private deployment tests use run-local CA/hostname-verified HTTPS
and an isolated UI browser with the exact server SPKI. This establishes the disposable
local ingress scope; a production origin requires its own deployment verification.

The current image passed 42 disk and 43 private-bucket check groups, with no omitted
checks. Each made 145 base requests and three restarts, including one database restore;
direct private checks made 91
requests on disk and 93 on bucket storage. It covers real private pixels/fonts, rendered
multipage PDFs, an actual normal UI PDF download, bounded MCP exports, alternate backend
and unsigned bucket denials, current revocation and a held upload/deletion race.
An invited human edits through the normal UI; removing their membership closes the idle
canvas connection and clears its already-painted private frame without a later edit.
Known media also refuses conditional revalidation and the browser cannot reopen the canvas.
UID 1000, CapEff=0, NoNewPrivs=1, Seccomp=2, read-only application and active Chromium
PID/network namespace and seccomp sandboxes were verified. The parent's chroot syscall is denied.
Both runs used the same immutable Rust helper. Each created two normal Openbot company
connections and explicitly reviewed, published and ran two verification workflows:
four MCP calls and six discoveries. A second native helper made 12 read calls across
the same two companies' frozen verification bindings: retired-key refusal, current-key
replacement, disable/re-enable, disconnect and encrypted credential removal. Preexisting
remote canvases and private assets remain after disconnect and same-schema native
PostgreSQL restore with the same disk volume or Garage bucket. Current/foreign/signed-out
authority and old-key refusal are checked afterward. This exercises database restoration
with retained assets, rather than restoring a separately lost object store.
The unchanged Doop source retains its accepted complete service evidence: frozen
installation, formatting, type checking, lint, documentation checks, 2,711 tests and
the production build. Its 17 skipped tests explicitly require the shared macOS system
clipboard; all 233 private-file and revision cases ran without skips across native,
controlled and unit fixtures. A fixture-only explicit peer assertion corrected a type-check
error; type/format/lint/docs and its three real idle-revocation cases were rerun on the
current package. All production bytes match the source of the complete service test run.

Production Node is 22.23.4 and the pinned Debian Chromium/sandbox packages are
156.0.8078.79-1~deb12u1. Host checks used Node 26.5.0 and Chrome 156.0.8078.80.
These are synthetic local checks of the exact pinned package above. Independent review covered
results and human access, the private-file prerequisite, operations and lifecycle, and the final
combined integration. Historical review records are retained separately from the published source;
the [repeatable test guide](../../docs/doop-testing.md) describes the actual checks and their limits.
Fixture replay does not establish a separate deployment's policy or model visual inspection.

Managed-company policy is server-owned `MANAGED_COMPANY_WORKSPACES`, validated at startup against
current users, membership, canvas ownership, invitations and public state. Current membership
is checked before owner/direct-invite access and each live delivery. Managed bots cannot gain
foreign/personal ownership, invitations or workspaces. Private canvases cannot enable public links
or enter Community. Design-sync key mint/list and ingest are disabled for managed company canvases,
including retained keys after restart. Instance/database operators control this configuration and
retain trusted override authority; ordinary workspace members cannot override it. Intended humans
have workspace edit rights. See materialized `docs/managed-company.md`.

## Current schema contract

The provider publishes Draft2020-12 tool schemas using the pinned package's explicit Zod4
subpath and public SDK registration handles. SDK validation/dispatch retain ownership.
Registration names are fixed; rename is refused before mutation. The pinned converter loses
fixed-tuple cardinality, so tuple definitions are refused before registration/schema updates.
Current Doop definitions use no tuples. `create_canvas` and `get_canvas` declare authenticated
structured canvas/workspace identity. Refresh requires explicit grant/review after schema drift.
Openbot company setup uses two published read-only verification workflows through normal
connection, encryption, review and worker owners. Its optional live helper must be
explicitly selected; omission is reported as NOT EXERCISED rather than a passing check.
Full private deployment acceptance requires both the normal setup and lifecycle helpers.

## Captured contracts and bounds

Actual discovery advertises 48 tools. The complete catalog, profile, text/structured/error
results, PNG image and embedded PDF carrier are captured under `src/adapters/mcp/fixtures/doop`.
The complete static `get_guide` is retained as `guide.json` in the runner output and
the checked-in replay fixtures. The three ordered document-guide Skills preserve
all 29,947 UTF-8 bytes at section boundaries, within each existing 16 KiB Skill bound.
The editor retains its five selected tools and receives the full guide through those
published Skills. The curated operation selection in `catalog.json` is unchanged;
refresh changed discovered contracts and explicitly review them before publication.

Actual private screenshot/PNG/JPG and small/large PDF captures come from the same
image run; separate provenance records exact inputs, hashes and call arguments.
Dense media uses bounded truthful JPEG previews, and full downloads require current
normal browser authority. Openbot leaves those locations inert. Existing transport,
per-field 16 KiB and action-result 64 KiB bounds remain. Oversized raw receipts retain
unknown/refusal and no-reissue behavior. Smaller binary receipts remain durable, while
workflow/model data becomes explicit omission metadata. Binary transport does not
claim visual review by a text model.

## Editing preconditions

These are the actual discovered mutator schemas, not authority inferred from annotations.
A read-then-write loop alone does not prevent a concurrent overwrite. Provider write outcomes
without an idempotency key or definitive receipt must use existing reconciliation.

| Tool | Explicit precondition in MCP |
| --- | --- |
| `create_canvas` | None in the discovered schema. |
| `set_guidelines` | None in the discovered schema. |
| `create_tokens` | None in the discovered schema. |
| `set_tokens` | None in the discovered schema. |
| `set_token_themes` | None in the discovered schema. |
| `save_decision` | None in the discovered schema. |
| `set_status` | None in the discovered schema. |
| `reply_to_comment` | None in the discovered schema. |
| `resolve_comment` | None in the discovered schema. |
| `get_feedback` | None in the discovered schema. |
| `report_issue` | None in the discovered schema. |
| `share_feedback` | None in the discovered schema. |
| `set_canvas_pages` | Required `expectedPages`, compared synchronously with the ordered current pages. |
| `create_canvas_page` | None in the discovered schema. |
| `set_page_background` | None in the discovered schema. |
| `set_prototype_start` | None in the discovered schema. |
| `create_frame` | None in the discovered schema. |
| `set_frame_html` | None in the discovered schema. |
| `upload_asset` | None in the discovered schema. |
| `generate_image` | None in the discovered schema. |
| `import_webpage` | None in the discovered schema. |
| `append_frame_html` | None in the discovered schema. |
| `edit_frame_html` | Required `old_str` must match exactly once in current HTML before synchronous replacement. |
| `update_frame` | None in the discovered schema. |
| `restore_frame_version` | None in the discovered schema. |
| `delete_frame` | None in the discovered schema. |

`set_frame_html` and streaming replacement do not provide a revision parameter. REST
`expectedUpdatedAt` behavior is not an MCP editing guarantee. Later editing operations must use
verified targeted preconditions or add conflict support in the actual Doop owner.

## Document authoring recipe

Phase03 supplies [the ordinary create workflow](create-document.workflow.json),
[published editor instructions](document-editor.md) and
[an isolated scripted sample](create-document.sample.json). Follow
[document setup and recovery](../../docs/doop-documents.md).
The result is a draft with visual review not performed. Use synthetic data until the separate
private-file-access acceptance gates pass.
