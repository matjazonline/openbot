# Openbot local agents plugin

This plugin connects one company-owned Cloudroom installation to Openbot using outbound WSS.
It requires Openbot logical bridge protocol 2; chunk framing remains version 1.
Cloudroom owns agent processes, workspaces, permissions, prompt order and history. Provider
credentials stay with the local CLIs. macOS and headless Linux are supported by the implementation;
real Linux acceptance has not been exercised.

Use Node 24.21.0 and npm 11.19.0. The supported Cloudroom artifact is the fixed revision and
all patches in [compatibility/manifest.json](compatibility/manifest.json), not an npm
release. The public SDK 0.6.50 declarations in `types/` come from that exact artifact, with their
license and hash manifest. A stock build without SDK cancellation and required agent configuration is unsupported.

From this directory:

```sh
npm ci
npm run typecheck
npm run test:guards
npm test
npm run build
npm run package -- /absolute/artifacts
```

`fs-ext` uses a native process-lifetime lock and requires a C/C++ build toolchain. Tests also
compile `better-sqlite3` and use OpenSSL for a disposable trusted TLS certificate. The plugin
uses the host's public plugin-owned SQLite storage at runtime. It never edits Cloudroom tables.
The package includes `dist/server.js`; the Cloudroom plugin host supplies `BbPluginApi`.
The archive includes a runtime npm shrinkwrap, and its companion integrity file records SHA-256.
Packaging repeats the build and archive step and rejects differing bytes. Follow
[OPERATIONS.md](OPERATIONS.md) for the pinned source build, public CLI install/remove,
single-profile service templates, backup/restore and uncertainty handling. Its one-step
`node scripts/install-local.mjs` runs that build, packaging and registration for one profile.

In the plugin's Cloudroom settings, set **Openbot HTTPS origin**, **Machine name**, **Selected
project IDs** (one per line), and the transient secret **Pairing code** generated in Openbot.
Use `openbot-local projects` through Cloudroom's registered plugin CLI to list local project
IDs, and `openbot-local status` for connectivity, recovery and provider availability. Native
authentication and inference are separate from catalog discovery. Only the persistent primary
local host and selected local projects are eligible. There is no executable, path or remote-host
override. Changes to origin, selected projects or the managed secret stop the service until reload.

The managed connection credential and pending claim live in Cloudroom secret settings. Do not
edit or copy them to another origin/profile. Pairing writes its complete recovery candidate before
network requests; a lost response resumes the same claim. A paired secret with a lost or changed
journal fails closed. Keep the profile, journal and secret state together. Automatic re-pairing,
identity reset, arbitrary thread import and destructive history cleanup are not supported.
If an initial pairing is definitively rejected, its input and rejected candidate are cleared;
enter a fresh Openbot pairing code and reload the plugin to retry. Incomplete responses keep the
original candidate and must recover that same claim. Rejection never resets an initialized journal.

Choose a published company agent in the Openbot Local Agents profile to apply its full
instructions, skill directories and native permission mode. Native-only agents can leave their
hosted model unset; the profile chooses the local model. Publication freezes the instructions,
files and permissions used by new work. An existing session keeps its admitted revision when
the profile or publication changes. Source or grant revocation still blocks new configuration.
Published company MCP tools are available through the native `openbot_company_mcp` tool on the
pinned Claude Code provider. It lists and describes frozen aliases and schemas, then calls the
existing company connection through a real Openbot workflow. Openbot keeps connection credentials,
current grants and required human approval. Other configured tool kinds and MCP agents on other
providers are refused before native startup; instructions, skills and permission modes remain
available on supported providers.

Integration-owned MCP credentials require a native-only published agent with its hosted model
unset. Configure the local model in the profile.

Company MCP authority is available to the root Claude turn after Openbot acknowledges its
invocation. Native subagents do not receive that authority.

An MCP call waits for its actual approval and result, for at most the originating operation's
remaining deadline or 15 minutes. Stopping the native operation prevents later dispatch. A lost
response reuses the same durable invocation; an uncertain remote effect requires reconciliation
and cannot start another effect in that native session. Later turns can read an existing call's
status with current authority. Full filesystem access does not bypass company MCP approval.

Agent permissions and unconfigured profiles default to **Full access**;
**Auto** and **Accept edits** remain available when the provider and host advertise them. The plugin
passes the frozen mode on both spawn and follow-up. A lower host/provider ceiling or a changed
native permission mode refuses admission instead of silently changing the selected access.

Configured threads require a positive SDK spawn receipt saved in the plugin journal before
provider startup. A lost receipt remains uncertain and cannot be recovered from thread metadata.
The pinned core retains the required plugin owner independently of editable metadata: disabling
or removing that plugin prevents future launch/resume. Instructions are complete, and skill
scripts/assets retain their paths and executable modes. The private immutable cache beside the
journal is bounded to 64 MiB and 128 bundles; full capacity refuses admission. Cache hits check
current server authority and local integrity. Missing or changed files require recovery.

An operation receives a visible `[Openbot operation …]` header. Follow-up prompts use Cloudroom's
queue. Only the exact originating request and accepted native turn can complete the operation;
human messages, idle status and the latest answer do not suffice. Native event cursors can be sparse
after Cloudroom compacts streaming deltas. Final output uses completed message snapshots; gaps
conservatively mark the result truncated. Answer permission prompts in
Cloudroom. Stop/archive explicitly affect the whole mapped thread. Deadlines, disconnects,
service cancellation and credential revocation do not prove the native agent stopped.
If provider startup fails before a native turn is accepted, the operation becomes uncertain.
Inspect the thread and fix the provider login before deciding on a new explicit request.

The plugin retains immutable observations until a contiguous server acknowledgment. It reserves
terminal space before submission, limits native effects to eight, bounds output to 256 KiB and
journal history to 4096 commands/mappings. Unknown native outcomes retain capacity. A full journal
pauses new admission while acknowledgments drain and existing effects reconcile. Missing or changed
durable state requires operator recovery; it never resubmits uncertain work.
Reload/shutdown cancels and joins actual network/SDK waits before releasing the profile lock.

To reproduce SDK cancellation proof against a prepared source build:

```sh
node compatibility/prove-sdk-abort.mjs /absolute/path/to/pinned-cloudroom
```

Fixture tests establish durability/protocol behavior, not real-agent or platform acceptance.
The plan's final integration gate owns native plugin smoke tests and operator installation proof.
