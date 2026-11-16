# Company local Cloudroom installation

The supported runtime is Cloudroom revision `d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815`
plus all patches in `compatibility/manifest.json`, public SDK 0.6.50, Node 24.21.0,
pnpm 10.34.9 and npm 11.19.0. This is a source build, not the upstream
`npx bb-app@latest` release. It uses Cloudroom's native local providers. The separate
Rust Core/VPS launcher and hosted sandboxes are outside this integration.
Linux installation, systemd boot and native provider acceptance remain **NOT EXERCISED**.

## One-step installation

`scripts/install-local.mjs` runs the build, packaging, installation and registration
steps of the next two sections in order, as the same unprivileged account. From this
plugin directory:

```sh
node scripts/install-local.mjs --cloudroom /absolute/cloudroom --profile /absolute/private-company-profile --plugin /absolute/openbot-plugin
```

Before building it reports every mismatched pin (Node, npm, pnpm) and every missing
git, `c++`, `tar` or (unless `--skip-tests`) OpenSSL together, and it refuses root.
It runs `npm ci`, `npm run typecheck`, then `npm test` and `npm run test:guards`
unless `--skip-tests`. It clones the pinned revision only when `--cloudroom` does not
exist; an existing checkout is left as it is for `prepare-runtime` to verify.
`--plugin` must be absent or empty, because Cloudroom runs a `path:` plugin from that
directory in place. The three directories must be separate. Artifacts go to
`artifacts/` here unless `--artifacts` names another absolute directory.

The tarball is compared with the integrity file the same packaging run wrote. That only
detects a damaged archive. Pass `--trusted-integrity /absolute/copy.integrity.json` to
also require the SHA-256 of an independently trusted copy.

It then starts `services/run.mjs` on the profile, installs and reloads the plugin
through the Cloudroom CLI with inherited `BB_*`/`ROOM_*` variables removed, and stops
the wrapper. `--keep-running` leaves the wrapper in the foreground until Ctrl-C instead.
`--server-port`, `--daemon-port` and `--ui-port` override 4310, 4311 and 4312. A
failure after the wrapper starts stops it before the installer exits.

It does not install or log in to provider CLIs, change plugin settings or accept a
pairing code. It finishes by reporting which provider CLIs are on PATH, the remaining
steps from "Providers, pairing and a first task", and the command that starts the
profile again. It installs no LaunchAgent or systemd unit.

## Build and verify the artifact

Use one unprivileged company OS account with a C/C++ toolchain, git and the pinned
runtimes. Do all builds and dependency installation before enabling unattended services.
From this plugin directory:

```sh
npm ci
npm run typecheck
npm test
git clone https://github.com/davidondrej/cloudroom /absolute/cloudroom
git -C /absolute/cloudroom checkout --detach d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815
node scripts/prepare-runtime.mjs /absolute/cloudroom
npm run package -- /absolute/artifacts
```

`prepare-runtime` verifies the source/patch identity, checks pnpm's exact version,
uses the upstream frozen lockfile, and builds the SDK/declarations, server, host daemon, CLI and local UI.
Its `.openbot-runtime.json` stamp identifies the prepared source and runtime. It does
not attest provider login or native task success. Startup verifies that stamp and source
identity and performs no dependency download, remote script execution or auto update.

Keep `bb-plugin-openbot-local-0.2.0.tgz` and its `.integrity.json` together. Compare the
tarball's SHA-256 to an independently trusted copy of the integrity file. For example:

```sh
shasum -a 256 /absolute/artifacts/bb-plugin-openbot-local-0.2.0.tgz
mkdir /absolute/openbot-plugin
tar -xzf /absolute/artifacts/bb-plugin-openbot-local-0.2.0.tgz -C /absolute/openbot-plugin --strip-components=1
cd /absolute/openbot-plugin
npm ci --omit=dev
```

The packaged npm shrinkwrap locks runtime dependencies including the native profile lock;
compile them under the service account on each target OS. Apache/MIT upstream notices
are in `compatibility/` and `types/`. Cloudroom supplies the plugin-owned SQLite API.
Do not copy native binaries from a Mac to Linux or treat fixture checks as Linux acceptance.

## Start one profile

Choose a new private profile and three distinct free ports above 1023. The wrapper
starts the pinned source server and native host daemon plus the built UI, all on
127.0.0.1. Default ports are 4310 (API), 4311 (daemon), 4312 (UI):

```sh
node /absolute/openbot-plugin/services/run.mjs --source /absolute/cloudroom --profile /absolute/private-company-profile
```

Wait for the redacted `ready` status. In another terminal use the same account/profile:

```sh
BB_DATA_DIR=/absolute/private-company-profile ROOM_SERVER_URL=http://127.0.0.1:4310 node /absolute/cloudroom/gui/apps/cli/dist/index.js plugin install path:/absolute/openbot-plugin --yes
BB_DATA_DIR=/absolute/private-company-profile ROOM_SERVER_URL=http://127.0.0.1:4310 node /absolute/cloudroom/gui/apps/cli/dist/index.js plugin reload openbot-local
```

Open `http://127.0.0.1:4312`. Both the UI and Openbot must address this same
Cloudroom installation. Keep APIs on loopback; optional SSH/private network access
requires separate authorization and configuration. The plugin's only Openbot
connection is outbound HTTPS/WSS; it requires no hosted account or inbound forwarding.

The wrapper holds one profile lock, refuses root and unsafe profile permissions,
and stops its whole owned group on a component failure. SIGINT/SIGTERM starts a
five-second graceful drain followed by bounded forced shutdown. A stopped socket,
server or wrapper does not establish that a native agent stopped. Use explicit
thread stop and inspect its matching native turn when that proof is required.
The plugin bounds each SDK wait to five seconds. A slow stop/archive response, including
release of an already idle provider runtime, can therefore remain uncertain even if Cloudroom
later completes it. Inspect the mapped thread before deciding on another explicit control; the
plugin never retries that uncertain mutation automatically.
Provider startup can fail before a native turn is accepted, including when a login is missing.
The plugin reports uncertainty in that case and retains the original request for reconciliation;
it does not manufacture a failed turn or resend the prompt. Inspect Cloudroom's thread and the
provider login before deciding on a new explicit operation.
Wrapper output contains component/state only. Raw child output is suppressed;
Cloudroom's own diagnostics under the private profile remain sensitive upstream data.

On macOS use this headless wrapper **or** a desktop-managed launcher for a profile.
Do not run both. The supplied LaunchAgent template needs absolute Node, plugin, source,
profile and provider PATH substitutions; install it only in the company account's
`~/Library/LaunchAgents` after testing. Closing a browser window leaves the headless
wrapper running; quitting/stopping the wrapper shuts down its owned services. No
Electron app or Linux VM is needed. Desktop app exit behavior is not attested by this wrapper.
The template discards status output; when debugging, run it interactively.

Linux's `services/cloudroom-local.service` specifies `User=cloudroom`, explicit
working directory/profile/PATH, unprivileged ports, journald, ten-second restart delay,
five starts per five minutes and a fifteen-second stop budget. Adjust the example
paths to the prepared installation. Authenticate and install provider CLIs as that
account, then enable the unit using normal system administration. Do not use root,
a Docker socket or a graphical desktop. This template has static checks only on macOS.

## Providers, pairing and a first task

Install the native provider CLIs using their supported installers under the company
account. The compatibility matrix exercised Claude Code 2.1.319, Codex 0.166.0 and
OpenCode 1.19.5; see `compatibility/README.md` for the isolated OpenCode pin. Consult
the providers' official installation/login instructions. Run `claude --version`,
`codex --version`, `opencode --version`, then their login commands in that account.
The service PATH must contain those same executables. Catalog availability is not
authentication, entitlement or successful inference.

Create a disposable local project in Cloudroom and run a manual native task first.
Then open the installed plugin's settings, set the Openbot HTTPS **origin**, machine
name, and selected project IDs. Generate a short-lived pairing code in Openbot's
company settings and enter it into the plugin's transient secret setting. Never put
codes or provider credentials in shell history, unit files or logs. Reload the plugin.
Use its registered CLI commands through Cloudroom:

```sh
BB_DATA_DIR=/absolute/private-company-profile ROOM_SERVER_URL=http://127.0.0.1:4310 node /absolute/cloudroom/gui/apps/cli/dist/index.js plugin run openbot-local status
BB_DATA_DIR=/absolute/private-company-profile ROOM_SERVER_URL=http://127.0.0.1:4310 node /absolute/cloudroom/gui/apps/cli/dist/index.js plugin run openbot-local projects
```

Select advertised projects in Openbot, save a local profile with an advertised
provider/model/reasoning level, and submit an explicit small smoke task. Confirm the
same operation marker, thread and accepted turn in Cloudroom's UI; send a follow-up
and an explicit thread stop. Permissions are answered in Cloudroom. Thread-wide stop
can affect human work in that thread; no targeted-turn cancellation is promised.
Manual JSON start/control/catalog requests require a UUIDv7 request ID issued within
the last 24 hours (at most two minutes in the future). Exact saved requests replay
their original result; IDs cannot be reused with changed payloads or resurrected
after retention. HTML forms generate these IDs for you.

## Status and pressure

Openbot shows connected/stale/revoked state, queue depth and oldest age, accepted
operations, permission waits, uncertain outcomes, reconciliation/backpressure and
last successful native task. Plugin `status` shows transport/recovery/provider
discovery and journal counts/bytes/reservations/admission pauses. Treat these as
separate facts. Neither a connected transport nor a successful model catalog proves
native authentication or inference. Laptop sleep and shutdown make it unavailable.

SQL summaries remain for at least 90 days after physical settlement and while needed
by active/prepared/unresolved work. Acknowledged settled progress detail can compact
after seven days, keeping deduplication identity. Unknown physical outcomes stay
retained. Bounded collection and workflow-tree cleanup preserve live references.
Plugin acknowledged terminal records remain at least 90 days; old session mappings
retire only after a current authenticated server confirms their permanent absence.
Cloudroom transcript retention is separate and remains Cloudroom's responsibility.

The plugin has a 64 MiB outbox and 4096 command/mapping bounds. It reserves terminal
space before effects, coalesces progress under pressure, preserves unacknowledged
terminal output and pauses new admission visibly. ACK drain and existing-work
reconciliation continue. Do not delete the journal to relieve pressure. Resolve
uncertainty with local inspection and allow acknowledged old records to retire.

## Backup, recovery and removal

Stop services before taking a coordinated snapshot of Openbot SQL, the entire private
Cloudroom profile (including plugin SQLite/WAL, secret settings and host identity)
and provider-managed session state. Include the plugin's adjacent `openbot-native-bundles`
cache in that same snapshot. Configured mappings with missing or changed cache contents require
recovery; copying thread metadata cannot restore capability ownership. Treat all of it as sensitive. Never start two
clones with one installation identity. A coordinated same-point restore may retain
known mappings. A SQL-only rollback requires revoking affected installations and a
new installation/re-pair; an unchanged observation cursor does **not** prove that
newer plugin mapping history matches older SQL. Lost/changed journals fail closed.
Keep the original profile for inspection; do not import arbitrary thread IDs.

Disconnect and restart before enabling schedules. Confirm that the same installation
recovers acknowledged history and that lost acknowledgments cause reconciliation,
not another mutation. Re-pairing uses a fresh installation/journal identity and never
replays old unresolved work. Inspect uncertain operations in Cloudroom, then explicitly
accept or abandon them through the authorized workflow resolution path. Any new
execution is a separate user-authorized operation with provenance, never a hidden retry.

Disable/revoke in Openbot blocks admission/authentication immediately, but offline
native processes may continue until local intervention or reconnection. Archive is
not deletion. To remove the plugin, first resolve/inspect active work, stop its
service, revoke its machine and use the same-profile public CLI:

```sh
BB_DATA_DIR=/absolute/private-company-profile ROOM_SERVER_URL=http://127.0.0.1:4310 node /absolute/cloudroom/gui/apps/cli/dist/index.js plugin remove openbot-local
```

Cloudroom removal deletes plugin settings/secrets/schedules; retain a protected backup
before removal if investigation is needed. It does not wipe project files or provider
logins. Unload the company LaunchAgent or stop/disable the systemd unit before removing
the wrapper. Preserve local work. Rollback across state/protocol changes means a reset
and new pairing, not a compatibility shim or replay of unresolved commands.
