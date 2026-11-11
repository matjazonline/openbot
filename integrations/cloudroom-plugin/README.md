# Openbot local agents plugin

This plugin connects one company-owned Cloudroom installation to Openbot using outbound WSS.
Cloudroom owns agent processes, workspaces, permissions, prompt order and history. Provider
credentials stay with the local CLIs. macOS and headless Linux are supported by the implementation;
real Linux acceptance has not been exercised.

Use Node 24.21.0 and npm 11.19.0. The supported Cloudroom artifact is the fixed revision and
**both** patches in [compatibility/manifest.json](compatibility/manifest.json), not an npm
release. The public SDK 0.6.50 declarations in `types/` come from that exact artifact, with their
license and hash manifest. A stock build without the SDK cancellation patch is unsupported.

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
