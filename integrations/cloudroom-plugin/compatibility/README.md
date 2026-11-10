# Cloudroom compatibility evidence

The integration targets the source revision and package versions in [manifest.json](manifest.json).
The manifest separates observed behavior from unavailable runtime proof. It does not describe a
released Openbot connector. The public plugin SDK at this revision is `0.6.50`; npm returned E404
for that version. Build the pinned Cloudroom source rather than installing an unrelated BB release.

The public-SDK probe installs through Cloudroom's CLI and uses `BbPluginApi`. It runs a new server
and host daemon against a new disposable profile, validates secret/database persistence and
background cancellation on reload, and characterizes scheduled create/send mutations. It crashes
the server after SDK acceptance but before plugin mapping, reconstructs the profile, and recovers
from positive metadata without submitting again. An independent Cloudroom CLI edits the same queue.
Chromium checks the built Cloudroom UI against the real API and sees the same queued Codex thread.
An installed hook parks probe-origin dispatch; completed probes delete their scheduled prompts.
An explicitly authorized native smoke run separately exercised Claude Code, Codex and OpenCode
in disposable projects using existing local logins. All three produced correlated completed turns
and appeared in the real UI. Nonempty event pagination/replay passed; Claude/Codex history survived
a server restart. Real UI approval denial and question submission recovered native turns. A public
thread-wide stop interrupted a running Codex turn. Linux remains unverified.

Use Node `24.21.0`, git, the source's pinned `pnpm@10.34.9`, this repository's locked JavaScript
dependencies (`npm ci`), and Playwright Chromium (`npx playwright install chromium`):

```sh
git clone --depth 1 --branch core-v0.1.98 https://github.com/davidondrej/cloudroom.git /tmp/cloudroom-openbot-proof
node integrations/cloudroom-plugin/compatibility/run-probe.mjs /tmp/cloudroom-openbot-proof /tmp/cloudroom-openbot-evidence
```

The runner checks the exact commit and applies the reviewed optional local-onboarding and public SDK cancellation patches
identified in the manifest before installing dependencies with `--frozen-lockfile`. It rejects
other tracked edits, untracked GUI inputs and patch/hash tampering. Its source guard has regression tests:
`node --test integrations/cloudroom-plugin/compatibility/check-source.test.mjs`.
It builds the public SDK, declarations, CLI and UI, and serves the built UI through Vite's preview
proxy. It binds services to loopback, retains their handles, and terminates them on success or
failure. It leaves the explicitly
reported disposable profile for inspection. It never reads or changes a user's Cloudroom profile.
The probe uses `/tmp/openbot-cloudroom-p01-workspace` only for scheduled, non-executed messages.

The native probe requires `OPENBOT_CLOUDROOM_NATIVE_SMOKE=1` in its server environment. Its
`native-*` commands operate only on threads recorded in its own disposable journal. Use the tested
native CLI versions in the manifest. The local OpenCode 2.1.4 installation stalled at ACP session
creation; an isolated `opencode-ai@1.19.5` executable completed the smoke through the same public SDK.
Do not replace the user's installed CLI. Select an advertised model and reasoning level explicitly
(the tested OpenCode selection was `openai/gpt-6-luna` / `low`); discovery does not establish provider
entitlement. `prove-native-ui.mjs` and `prove-native-stop.mjs` exercise real UI submission and active
stop. `verify-native-evidence.mjs <native-evidence-directory>` checks retained captures; those checks
do not replace live provider execution.

The frozen Rust execution policy carries the reasoning level explicitly. Each advertised model
lists its bounded supported reasoning levels; selection validation refuses a level absent from that
model, including `none` for the tested OpenCode model. Reasoning participates in the immutable
create-command digest. A catalog-refresh success references its catalog observation sequence and
does not manufacture a native thread identity.

Observed at the pinned revision:

- Two local `spawn` requests with the same `requestId` create different threads.
- Two queued `send` requests with the same `requestId` create two queue entries.
- Metadata supplied to `spawn` is readable through the public plugin metadata API.
- A server crash after SDK acceptance and before the plugin saves its thread mapping can be
  reconciled from matching operation metadata in this positive case. This proves no negative
  lookup or automatic retry guarantee.
- Secret settings and the plugin-owned SQLite database survive plugin reload. The background
  signal aborts while the database is still available for its final durable write.
- A later server restart preserves the profile's secret, journal, and host identity.
- The public stop API accepts a thread identity. It provides no atomic targeted-turn cancellation.
- Follow-ups queued behind an undispatched first message require an explicit model; otherwise
  the local API reports that no execution model has been stored yet.

A plugin must save a dispatch intent before mutation, never blindly retry an ambiguous call, and
recover only from positive matching identity evidence. A bounded listing without a match cannot
establish absence. Idle/latest-output observations cannot establish completion of a particular
Openbot turn. Missing journal state requires recovery rather than automatic new dispatch. An
Openbot deadline ends its wait; it does not prove a disconnected native process stopped.

Retained execution evidence lives under
[`plan/cloudroom-local/phases/p01/docs/logs`](../../../plan/cloudroom-local/phases/p01/docs/logs).
The source checkout is disposable; the manifest hashes the upstream contracts used for decisions.
