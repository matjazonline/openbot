# Local Doop integration tests

Use the reusable runner for synthetic Doop compatibility, isolation and Openbot setup
checks. `acceptance` runs both required probe families against disk and bucket storage.
Each run provisions fresh accounts and disposable data; setup and dependencies
are reused. The diagnostic modes use embedded PostgreSQL. The private deployment mode
below uses the production image, native PostgreSQL and real local TLS. No production
accounts or credentials are required.

## One-time setup

Requirements: Python 3.9+, Node 22+, npm, Git and a POSIX host. Native rendering needs
installed Chrome/Chromium. Image tests also need Docker with Buildx and a Linux kernel
supporting unprivileged user namespaces. The first setup/build needs network access.

```sh
python3 scripts/doop-test.py setup --managed --directory "$HOME/.cache/openbot-doop-test"
```

Use a new directory. Setup installs Bun 1.3.10 and frozen dependencies with general
lifecycle scripts disabled; only Bun's pinned platform selection installer runs.
`integrations/doop/source.json` identifies the exact upstream commit, lockfile, MCP SDK,
reviewed patch and full source fingerprint. The managed checkout is reconstructed and
verified from that package. Existing pristine setups can use `prepare-managed --directory`.
A managed checkout has its own frozen dependency directory and lock identity; it never
borrows the pristine checkout's dependencies. Incomplete, stale or previously symlinked
managed setups require a new setup directory.
A changed package needs a new setup directory. There is no fallback to an unverified source.

## Repeated native checks

```sh
python3 scripts/doop-test.py run --managed \
  --directory "$HOME/.cache/openbot-doop-test" --output /tmp/doop-native-01
```

Use a new output directory each time. Runs install no dependencies and do not reuse
accounts, credentials, ports or data. The native server receives an already-reserved
loopback socket. A test-only preload observes fresh ownerless seed IDs without changing
authentication, authorization or application data. Pass `--chrome /absolute/path/to/chrome`
when auto-detection cannot find your installed browser; its normal sandbox stays enabled.

Normal signup, workspace, membership and agent-key APIs provision two human/bot pairs.
Only the fresh bots' welcome canvases are deleted. A private control pipe configures their
managed-company policy and permits two bounded restarts. Credentials remain in probe memory.
Private deployment acceptance also permits one quiesced database restore before the ordinary
restart, retaining the same private asset store; its report records three app restarts in total.
The service excludes inherited database URLs, payment/model keys, proxies and Node preloads.
Cleanup affects only this run's process group and temporary data, including startup failure
and interruption. Reusable dependencies and evidence remain.

The full suite checks own and foreign access, private sharing and Community restrictions,
managed-bot invitations/roles/moves/claims, competing forbidden changes, joined WebSocket
revocation and reconnect, old design-sync keys and actual frame/edge immutability across
restart. It also exercises authenticated MCP discovery, guide/profile, fixed-workspace canvas
creation, frame writes/reads, and real PNG/PDF rendering. Upstream without `--managed` is
available for compatibility diagnostics; its full isolation suite intentionally fails.

## Container checks

```sh
python3 scripts/doop-test.py image --managed \
  --directory "$HOME/.cache/openbot-doop-test" --output /tmp/doop-image-01
python3 scripts/doop-test.py run --managed \
  --directory "$HOME/.cache/openbot-doop-test" --image /tmp/doop-image-01/image.json \
  --output /tmp/doop-container-01
```

The exported OCI archive has retained manifest/configuration/archive digests. Build inputs
include pinned Node/Bun images, a signed dated Debian archive and the frozen lockfile.
The build context excludes ignored dependencies, environment files and graph metadata.
Each run verifies the source/archive/image identity, then starts its own labeled container
and private volume with a read-only application, UID 1000, zero capabilities,
no-new-privileges and the explicit Chromium seccomp profile. It proves parent chroot denial
and actual Chromium namespace/seccomp sandbox support before creating accounts.
A bounded loopback TCP relay preserves HTTP/WebSocket bytes and a stable local origin across
restarts. Only owned containers/volumes/tunnels are removed. See the packaged Doop
`deploy/README.md` for runtime configuration; production HTTPS is an operator deployment task.

## Repeat private deployment acceptance

Provision the same pinned service images used by the tests before running offline:

```sh
docker pull postgres:18.6-alpine@sha256:123b17998ccaf47e53b502e8c6a1e79afd8f579c6f3eec83c563da026f957395
docker pull dxflrs/garage:v2.5.0@sha256:677e5b758e4425a325b811c425c026bf35a4bf42f7ddbaa773de3f4fd779a5a5
python3 scripts/doop-test.py acceptance \
  --directory "$HOME/.cache/openbot-doop-test" --image /tmp/doop-image-01/image.json \
  --ui-chrome /absolute/path/to/chrome \
  --openbot-test-binary /absolute/path/to/current-open_bots-libtest \
  --openbot-test-database "$TEST_DATABASE_URL" \
  --output /tmp/doop-acceptance-01
```

Private acceptance requires the current compiled helper and owned PostgreSQL authority described
below, including all three exact ignored tests. The command runs `all` in disk and bucket
storage, then `e2e` in disk and bucket storage, with fresh accounts and resources each time.
Every run must use the same verified source, image and immutable helper. Cleanup must complete
before the next run starts; failure stops the command. `acceptance.json` retains all four
report hashes, cleanup receipts and artifact identities. Use a new output directory each time.
For focused debugging, use `run --managed --suite e2e --private-deployment disk` with the
same image/helper/database/browser options; `--suite all` selects privacy and lifecycle checks.
Both families are required for complete acceptance. The
setup directory must be shared with the selected Docker daemon. Each run creates a
protected `.private-run-*` directory there, outside the checked source. PostgreSQL,
the application and optional Garage instance have independent owned storage and a
private Docker network. Only the application backend and test ingress use loopback
bindings; the private bucket fixture also has a loopback endpoint for unsigned controls.

Native HTTPS clients validate the generated local CA and hostname. The isolated UI
browser allows only the exact generated server SPKI. This does not install a trusted
root or establish public DNS/certificate coverage. The production renderer keeps its
own sandbox and receives no browser cookies or MCP keys. Application storage uses its
actual `BUCKET`, `ACCESS_KEY_ID`, `SECRET_ACCESS_KEY`, `ENDPOINT` and `REGION` keys.

Required checks cover known private image/font bytes and metadata, warmed caches,
anonymous/foreign/bearer denial, the current backend address after restart, raw bucket
denial, forged cross-parent materialization, late upload publication after deletion,
actual editor pixels/fonts, rendered multipage PDF pixels/glyphs, the normal UI PDF
download, bounded MCP previews and authenticated full exports, and current revocation.
The runner refuses a successful report that omits a required path. The private probe
has a separate fixed 96-request budget; the original probe retains its 160-request bound.
Broader upload/copy/render/cache race protocols are exercised by the native Doop suite.

The `e2e` family publishes the normal document editor, complete guide Skills and shipped
workflow for two separate companies. Real approvals and competing workflow workers create,
edit, read back and deliver each document. Worker reconstruction must reuse known creates.
A separate normally published create deliberately loses a successful real Doop receipt;
it must remain unknown for manual reconciliation while independent work makes progress.
Bot keys reopen and export the generated documents, and ordinary humans reopen them and use
the normal PDF controls. Known foreign document/frame/media IDs and signed-out browsers
must fail. Known private images/fonts are added through normal human APIs after the bot
edits, with that enrichment recorded separately. Model replies use a deterministic local
provider fixture; paid inference and model visual review remain explicitly unexercised.

Within one agent step, an identical frozen operation and its arguments reuse the committed
action snapshot, including read operations. A new model call ID alone does not refresh it.
The integrated test checks actual accepted edits, then performs fresh bot-key MCP reads
after workflow completion and before any human enrichment. A stale pre-edit snapshot or
replacement that leaves the old text behind fails this required consumer check.

Both families retain the 80-second native helper and 180-second probe deadlines. Required
groups must all execute successfully; missing, duplicate, unexercised or wrong-family
reports fail. The separate `doop-integration` CI job also runs the complete managed Doop
service gate before building one image and running all four acceptance profiles. Doop service
checks receive a clean environment without inherited database URLs or provider credentials.

Cleanup records intent before external creation and verifies the exact engine, labels,
names and resource IDs. It stops this run's probe/browser and application before their
dependencies. An uncertain creation or removal retains protected recovery state and
`cleanup-pending.json`; a single empty Docker query cannot establish that creation ended.
After the original runner has exited, reconcile the retained run with:

```sh
python3 scripts/doop-test.py cleanup-private \
  --runtime-dir /absolute/path/from/cleanup-pending.json \
  --output /tmp/doop-private-recovery-01
```

Use the same Docker daemon and unchanged reviewed helper/source. Recovery refuses
unconfirmed creation, an active original runner, unrelated/reused process identity or
a replaced resource. Resolve an uncertain provider operation before retrying recovery;
do not prune other resources or infer success from absence alone. Reusable caches,
the source worktrees, OCI archive and evidence survive successful cleanup.

## Include live Openbot connection setup

First compile the Rust libtest binary with the repository's usual locked offline build.
Use Cargo's JSON `compiler-artifact` message to locate its executable; require exactly one
`adapters::persistence::doop_live_setup_tests::doop_live_company_setup` in its `--list` output.
Copy that executable to a private, versioned path and retain its SHA256 before running it:
later Cargo builds may relink the executable under `target/`. Then add these options to either
native or image `run`:

```sh
--openbot-test-binary /absolute/path/to/open_bots-libtest \
--openbot-test-database "$TEST_DATABASE_URL"
```

Private deployment acceptance requires these options. Its compiled binary must also list exactly
one `adapters::persistence::doop_live_setup_tests::lifecycle::doop_lifecycle_live_company_keys`.
It must also list exactly one
`adapters::persistence::workflow::tests::mcp_tests::doop_setup_tests::document_tests::live_tests::doop_e2e_live_company_documents`.
An older binary matching zero tests cannot pass. That runner supplies
the generated CA to the isolated libtest helper. The helper accepts the matching
HTTPS loopback origin and adds that CA to its own test client while retaining
hostname and chain verification. This test-only trust setting is absent from the
production client; it does not modify the host trust store.

The PostgreSQL URL must point to an owned disposable test server with database-creation
rights. The ignored helper creates, migrates and drops its own database. Credentials travel
only on private child stdin into the normal company MCP connection/encryption owners.

The `all` profile also mints new bot keys through normal Doop APIs, verifies each old key before
revoking it, and proves the old keys fail afterward. A separate strict lifecycle helper warms a
real SDK session, reuses the same frozen workflow binding through credential replacement,
disable/re-enable and disconnect, then verifies encrypted credentials are removed. All three helpers
retain the existing 64 KiB stdin and 16 KiB combined-output bounds; they have separate success
markers and records. Tokens and human cookies never enter checked-in fixtures or model context.
The private control performs one same-schema dump/reset/restore inside its positively owned
PostgreSQL container with the app stopped, retaining the same private asset store. It verifies
exact asset bytes, canvas identity, current permissions and old-key revocation afterward. This
does not test recovering a separately lost object store. See [the operations runbook](doop-operations.md)
for backup consistency, recovery cutoffs, rotation and manual reconciliation.
The runner creates a verification canvas through each company's human account. Two real Openbot
companies each create a `doop` connection, discover and explicitly review read operations, then
publish, bind and execute the shipped read-only identity workflow through normal workers.
It verifies the exact bot, workspace and known canvas; user input cannot supply those identities.
The real guarded client is observed for four total tool calls and six catalog discoveries, and
all sessions must close. Foreign connection reads, token replacement and token access are denied.
`setup-record.json` retains nonsecret identities, intended humans, source identity, audience,
observed counts and the successful verification workflow/version/binding/run IDs and result.
See [company setup and catalog review](doop-connections.md).
Diagnostic native/image runs without helper flags report setup as NOT EXERCISED. Private
profiles and `acceptance` require the current helper and owned database explicitly; a failing
helper or zero selected tests fails the run. Helper output and duration are bounded;
credentials are never persisted as fixtures.

## Evidence and fast checks

`report.json` records source/image identities, individual checks and restart count. Normalized
JSON captures exclude credentials. Startup diagnostics are bounded and redact generated keys.
The exact image/source and editing preconditions are recorded in `integrations/doop/README.md`.
Diagnostic origins use loopback HTTP. Private deployment origins use the local TLS
scope described above. Neither establishes a separate production deployment's policy.

```sh
python3 scripts/tests/doop-test.py
node --test scripts/tests/doop-probe.mjs
cargo test --locked --offline --lib doop_contract_ -- --list
cargo test --locked --offline --lib doop_contract_
cargo test --locked --offline --lib doop_binary_
```

CI runs the fast runner/probe/Rust regressions. Live Doop image checks require the setup above
and are separate evidence. Fixture replay uses the existing authenticated guarded transport;
fixtures alone do not prove live compatibility. Generic model observations omit whole binary
MCP carriers, including sibling copies, before workflow extraction and model checkpoints/history.
They explicitly report that visual inspection was not performed. Original bounded action receipts
and child output remain intact; oversized raw MCP results retain the existing refusal/unknown
and no-reissue behavior. No discovery, result, checkpoint or response limit is raised.
