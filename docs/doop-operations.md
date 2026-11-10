# Operate a company Doop deployment

Doop runs separately from Openbot. Deploy the reviewed managed source and private-file fixes,
then use Openbot's ordinary company MCP connections. Intended humans join the Doop workspace
explicitly. Openbot company membership does not invite them. Start with synthetic data and run
the deployment checks before enabling confidential use.

## Supported artifact and runtime

[The source record](../integrations/doop/source.json) and [integration evidence](../integrations/doop/README.md)
identify the accepted package. The upstream base is `01584b34f6cadcf21767127280cc6e320a6d748a`;
the managed fingerprint is `e0c063c43c5bc70018eb8e09d0add0d32ef35159fff843bc9646da7ebe4c4464`.
The patch SHA256 is `4621d91534b24db5e5cb35f535bda9d579c808120eddcf3876ad67d793ed298c`;
the lock SHA256 is `9fc8b3c110b390a9c46975da45c3a297090096703f9d9721dcea19f84dad28e3`.
Use frozen dependencies with Bun 1.3.10 and MCP SDK 1.30.0.

The retained linux/arm64 OCI manifest is
`sha256:425a2d2e718285f11fd9b18bf110dd173c689d0f0378a94f5ebe4f9872e52d43`;
the configuration digest is `sha256:1f301a9894e089a8011846ec974bf423115a5357c730e74bd4961a2ccbacc225`.
Its exported archive SHA256 is `7e85e67b892e8b0c523f089cbe2b8a4612861634e9ed5b7c5a64efba721e046d`.
Retain and verify the archive for exact replay. Rebuilds record their actual image digest;
layer metadata is not promised to reproduce byte for byte.

This image uses Node 22.23.4 and Debian Chromium 156.0.8078.79-1~deb12u1. Run as UID 1000,
with a read-only application, zero capabilities, no-new-privileges and the packaged Chromium
seccomp profile. Keep writable data separate. Retain Chromium's namespace and seccomp sandboxes;
the tested parent process cannot chroot. Use an outer HTTPS proxy rather than granting root
privileges for a low port. The image's materialized `deploy/README.md` and `deploy/` configuration
are the source for mounts and sandbox details. Restrict database and object-storage networking.

## Runtime configuration and ownership

These are application runtime settings, not values baked into the image or checked into Git.
Supply secrets through the deployment's private secret source. Keep canonical URLs consistent
with the externally reachable HTTPS origin and review proxy configuration separately.

| Application | Keys actually read | Meaning |
| --- | --- | --- |
| Doop | `NODE_ENV=production`, `PORT` | Production behavior; the normal port defaults to 4400. |
| Doop | `DATABASE_URL` | PostgreSQL connection. Without it, Doop uses embedded PGlite under `./data/pg`; the private deployment acceptance uses PostgreSQL 18.6. |
| Doop | `BETTER_AUTH_SECRET`, `BETTER_AUTH_URL` | Private session secret and canonical origin; production requires both. |
| Doop | `SECRETS_ENCRYPTION_KEYS` | Comma-separated `id:base64` entries, each 32 decoded bytes; first entry is active. Retain needed old decryption keys. Production can start without this setting and warn; startup does not guarantee stored provider credentials are encrypted. Configure it deliberately. |
| Doop | `MANAGED_COMPANY_WORKSPACES` | Operator JSON array of `{ "workspaceId": "WORKSPACE_ID", "botUserId": "BOT_ID" }`. Startup validates configured identities, roles, memberships, workspace canvases and private policy. |
| Doop | `REQUIRE_EMAIL_VERIFICATION` | Defaults to true. The disposable fixture explicitly disables verification for its synthetic accounts. |
| Doop | `BUCKET`, `ACCESS_KEY_ID`, `SECRET_ACCESS_KEY`, `ENDPOINT`, `REGION` | Actual object-storage settings; region defaults to `auto`. Keep the bucket private and test unsigned denial. Without bucket storage, preserve the disk data mount. |
| Openbot | `CREDENTIAL_ENCRYPTION_KEYS` | Comma-separated positive `version:base64` entries, each 32 decoded bytes. Keys protect company MCP credentials in the existing credential owner. |
| Openbot | `CREDENTIAL_ENCRYPTION_ACTIVE_VERSION` | Explicit active version present in that keyring. Adding a higher version alone does not activate it. Never reuse a version with different bytes. |

The two applications have different encryption keyring formats. Openbot's keyring does not
rotate a Doop agent key, and Doop's provider-secret keyring does not replace an Openbot credential.
Openbot startup does not validate a remote private bucket, workspace exclusivity or all remote
permissions. Verify these against the exact running deployment.

## Provision and enable

Follow [connection setup](doop-connections.md), [document setup](doop-documents.md) and
[repeatable deployment testing](doop-testing.md). The trusted setup record contains the origin,
Doop bot ID, fixed workspace ID, human-created verification canvas ID, Openbot company UUID and
connection UUID. Record the intended human audience and its approval.

Configure the managed workspace, invite the humans, mint the bot key through Doop's normal
account settings, and store it through the existing encrypted MCP credential form. Discover and
review complete input/output schemas, grant the recipe's explicit operations, and publish the
read-only identity workflow. Verify the expected bot, managed workspace and known canvas before
publishing the editing agent and create-document workflow. Keep trusted identity binding values
outside model-selected titles, URLs and prompts. Review again after schema changes; discovery
does not silently expand grants.

Before use, test own/foreign/signed-out human access, current media authority, browser opening,
PNG/PDF rendering, membership removal, key revocation and retained private storage. The reusable
fixture tests disk and bucket storage with the same actual image and native PostgreSQL. Its
local CA and exact browser SPKI establish only its disposable loopback HTTPS scope. Verify public
DNS, public certificates, ingress, backups and permissions separately for a production deployment.

## Rotate a bot key

1. Pause new runs using the affected connection and identify already accepted remote actions.
   Record current connection and credential revisions using the normal company connection page.
2. Mint a new key for the same Doop bot through its normal account settings. Keep it in a private
   credential source, never a command argument, workflow, Skill or model prompt.
3. Replace the Openbot credential through the normal credential form/API using its current
   `expected_revision`. A stale revision must conflict; fetch current state before another attempt.
   Replacement advances the connection and credential revisions. Review the identity again.
4. Explicitly revoke the previous key in Doop. Replacing Openbot's encrypted value does not revoke
   the remote key. Prove a formerly valid old key is refused and the replacement verifies the same
   bot/workspace/canvas. A cached SDK session or frozen publication cannot grant stale authority.
5. Resume only after verification. Rotation does not authorize replaying an uncertain create.

For suspected compromise, disable the connection and revoke the exposed Doop key first, then
provision and verify a replacement. Do not retain an exposed key merely to avoid downtime.

Openbot envelope-key maintenance is separate. Back up the database and required keyring securely,
quiesce writers, configure old plus new versions, explicitly select the new active version, then
run the existing inventory commands in that configured environment:

```sh
open_bots credentials rotate
open_bots credentials status --require-version 2
```

`2` is an example version; use the actual intended version. These commands report aggregate,
secret-free outcomes. Incomplete rotation or invalid/unavailable required versions return failure.
Company MCP credentials participate in the existing tenant-qualified rotation inventory. Check
convergence before updating writers and retiring a decryption key. Keep keys needed by retained
backups, or reset and re-provision incompatible disposable state.

## Disable, reconnect and reconcile

Disable a company connection through its normal revision-checked edit. This stops new authorized
calls; an action already accepted by Doop may continue. Revoking a grant or deleting a connection
does not prove that an in-flight remote operation was cancelled. Re-enable only after verifying
the same bot, fixed workspace and intended human audience. Never automatically replay old work.

Deleting an Openbot connection tombstones its identity, removes its encrypted credential and
revokes its reviewed operations. It does not send remote document deletions. A new connection
with the same `doop` slug has a different UUID; publish/review/bind it explicitly. Frozen bindings
to the old UUID must not fall back to the replacement.

When a create times out or loses its response after remote entry, preserve the existing unknown
outcome and reconciliation state. An authorized human inspects the expected Doop workspace,
matches the run/action time and intended document, and decides whether to keep, repair or
explicitly delete the partial canvas. Record the decision and definitive identifiers. Follow
Openbot's normal reconciliation path; do not blindly retry creation or manufacture a success
receipt from a similar title. Targeted edits still require their actual write preconditions.

## Backup and restore

Back up Doop's database, its disk assets or private object store, and all necessary application
keyrings and deployment configuration as one consistent recovery set. Back up Openbot's database
and needed credential keys separately with their matching identities. Encrypt backup access,
restrict readers and apply the company's retention policy. A database dump alone cannot recover
an independently lost or changed asset store.

The disposable private fixture exercises one same-schema PostgreSQL dump/drop/create/restore
after stopping its application. It uses only the positively identified, labeled container ID,
keeps a mode-0600 dump inside that container under the fixed 16 MiB fixture bound, stops on the
first uncertain tool outcome and retains the same disk volume or Garage bucket. It checks
canvas identity, exact private asset bytes, current/foreign/signed-out permissions and old-key
refusal afterward. This is a database restore with retained assets, not full object-store backup
restoration or a general production restore command. Its control accepts no caller database,
container, path or DSN. Use the owned cleanup/recovery procedure on failure.

For production recovery, stop writers, restore a consistent matching database/assets/keyring set,
then inspect its recovery cutoff before enabling traffic. Restoring an older backup can restore
old memberships or agent keys: later revocations are not magically preserved. Revoke/rotate keys,
reconcile membership and review uncertain actions against the intended cutoff. Verify both
companies' identity and isolation, human access, media and render paths before resuming.

**No backward compatibility.** Reset and re-provision when schema or payload changes do not apply
cleanly. Do not add backfills, legacy decoders, dual credential stores or a staged compatibility
mode to carry old rows forward.

## Retention and troubleshooting

The company owns document retention. Assign a human owner to authorized remote deletion, obtain
confirmation through the normal Doop operation, and document its backup recovery policy.
Openbot disconnect alone retains documents. Upstream Doop workspace deletion detaches canvases
into their owners' personal spaces; it is not secure document erasure. Managed company workspaces
deny that deletion while bound. Deprovision humans/bots and dispose of canvases, assets and
backups deliberately under the retention policy, including any required managed configuration
change. Removing workspace membership is an access operation, not proof of physical erasure.

Use structured identifiers and bounded outcomes for diagnostics, for example:

```json
{
  "company_id": "COMPANY_UUID",
  "connection_id": "CONNECTION_UUID",
  "run_id": "RUN_UUID",
  "outcome": "manual_reconciliation_required"
}
```

Keep bearer headers, prompts, raw HTML, media bytes, session cookies and credential-bearing URLs
out of logs and metrics. Retain only the approved audit identifiers, revisions, artifact hashes
and action outcomes. A successful synthetic probe establishes its named paths on its recorded
artifact; it does not establish a text model's visual review or another deployment's permissions.
