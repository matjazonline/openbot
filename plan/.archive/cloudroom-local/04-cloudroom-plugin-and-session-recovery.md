# 04 — Cloudroom plugin and session recovery

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [01](01-contracts-and-cloudroom-compatibility.md) through
[03](03-outbound-connection-and-durable-protocol.md). Deliver a standalone Openbot plugin using
Cloudroom's public plugin SDK, with no local database edits or second native-agent process manager.

## Batch 04A — Plugin lifecycle, configuration and discovery

Keep the plugin in a bounded package such as `integrations/cloudroom-plugin/` in this repository,
with its own lockfile, build/typecheck/test scripts, packaged artifacts, and supported Cloudroom
version constraints. Use only the public `@get-bb/plugin-sdk` surface proven in phase 01. A small
WebSocket dependency is appropriate if the supported runtime cannot supply handshake headers.

Settings include Openbot HTTPS origin, machine name, selected Cloudroom projects and connection
status. Pairing codes are transient secret inputs. Persist machine/claim credentials through
Cloudroom secret settings; keep non-secret mappings and the journal in plugin-owned storage.
The SDK's resolved server/profile is authoritative. Do not hard-code inherited BB ports, require
Electron APIs, or require a graphical desktop for the plugin.

Own the connector through `bb.background.service`, using its cancellation signal. On plugin
reload/shutdown, stop SDK submissions, abort pending network work, flush durable journal writes,
close the socket and await termination. Apply exponential reconnect backoff with jitter and the
phase 01 bounds. An authorization rejection stops retrying until pairing is repaired; network
loss does not reset the journal, installation identity or native thread state.

Discover installed local providers, available model catalogs, projects and supported operations
through SDK queries. The initial provider IDs are `claude-code`, `codex`, and `acp-opencode` when
actually available. Only advertise selected projects on the installation's local host; reject
remote/cloud environment selection. Resolve project IDs to Cloudroom's own workspace facility;
Openbot sends no arbitrary shell executable, working-directory override or provider credential.

Refresh bounded catalog pages at connection and explicit refresh. Availability and authentication
failures are distinct. Replacing project defaults or a provider catalog never silently changes a
frozen command. An unavailable exact provider/model is an actionable refusal, not a fallback.

## Batch 04B — SDK execution and durable local journal

Serialize journal mutation through one plugin-owned dispatcher. KV is suitable for mappings if
its durability is established; a small plugin-owned transactional store is appropriate if needed
for atomic command/outbox transitions. Neither may write Cloudroom's internal SQLite tables.
Prevent overlapping connector instances for the same profile with an OS/process lifetime lock.

For each mutating command:

1. Validate identity/digest, pairing generation, deadline, selected project/provider and capacity.
2. Persist the command before local acceptance. A duplicate with the same digest returns its
   recorded state; a different digest fails. Persist a dispatch-intent marker before the SDK call.
3. For create, use `bb.sdk.threads.spawn` with explicit local environment/provider/model and
   plugin origin/metadata carrying the Openbot operation identity where supported. For follow-up,
   send to the already mapped thread. Honor the exact proven upstream request-ID semantics.
4. Save the returned thread/turn identity and outbox observation before sending it to Openbot.
5. Track the matching turn through public events/timeline until a bounded final observation is
   durable. Keep it until Openbot acknowledges its journal sequence.

Do not hold a local or server transaction across an SDK call. The gap between SDK acceptance and
journal persistence is real: journal `dispatching` is not proof that execution happened or did
not happen. Phase 01 must identify positive reconciliation evidence. If the SDK has no atomic
idempotency/lookup guarantee, recover a found matching thread/turn, otherwise leave the command
uncertain. Absence from a bounded listing and a timeout never authorize another spawn/send.

Stop/archive are deliberately Cloudroom thread-wide controls. Require an explicit manual action
for a broad stop; do not claim it cancels only the original Openbot turn. Automated deadlines use
atomic targeted cancellation only if proven by the SDK. Otherwise end the server wait and show
that the local thread may still run, with an explicit thread-stop action. Preserve files/history.

## Batch 04C — Shared UI, prompts and recovery

Persist `(installation, Openbot session, Cloudroom thread)` mappings and command/turn correlation.
Openbot only controls mapped threads. Importing arbitrary existing Cloudroom threads is deferred;
users can still create/manage their own unrelated threads in Cloudroom.

Cloudroom's queue is the authority for local prompt order. SDK acknowledgments distinguish queued
from started. User messages in its UI may interleave with Openbot work. Associate results with
the originating command and actual turn; `thread.idle` or the latest assistant message alone
cannot complete an Openbot operation. Report manual edits, stopped turns, permission waits,
archival/deletion and provider failures as observed state, without fabricating an Openbot action.

Keep configured native permissions and subagent behavior. V1 surfaces interactions as
`waiting_for_input` with a local Cloudroom thread reference; the user answers in Cloudroom.
Do not auto-approve tools, upload local credential files, or copy Openbot provider keys into the
agent environment. Child sessions created by a native agent stay Cloudroom-owned local work;
they are not automatically separate Openbot sessions or additional authority.

On restart, recover dispatch-intent commands, mapped threads and unacknowledged observations before
new effects. Thread deletion never triggers automatic recreation. Never use provider `--last`
semantics or change working directory/model to make a missing identity appear recoverable.

**Focused checks:** plugin unit/SDK fixture tests for every durable cut point, duplicate commands,
provider/project refusal, UI interleaving, prompts queued behind human turns, permission waits,
reload and journal loss. Include two connector instances and competing duplicate deliveries.
Run locked plugin typecheck/build/test commands and a fake-server WSS integration. Real-agent
start/send/stop/resume on macOS and Linux is separate phase 07 acceptance, not proven by fixtures.
