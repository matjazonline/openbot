# 03 — Outbound connection and durable protocol

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [01](01-contracts-and-cloudroom-compatibility.md) and
[02](02-company-machines-and-pairing.md). Connect the plugin to any Openbot replica and make
commands/results recoverable without treating the socket as durable storage.

## Batch 03A — Authenticated connection and replica ownership

Implement `GET /v1/local-machines/connect` with WebSocket upgrade and a machine credential in the
authorization header, never the URL. Add Axum's `ws` feature at the HTTP adapter boundary. Verify
credential generation, machine state, installation identity, protocol version and bounded hello
before accepting commands. Reject unknown versions; coordinate a plugin update, not dual decoders.

Use one active connection generation per machine, acquired/renewed through a PostgreSQL lease.
A reconnect replaces the previous generation transactionally. Reads/writes and acknowledgments
check installation, credential generation, connection generation and unexpired ownership. Closing
an old socket may not unregister or mark the replacement offline. Presence is observed/expiring
state, separate from administrative enabled/revoked state and agent execution state.

The socket owner polls that machine's durable command rows in bounded batches. `LISTEN/NOTIFY`
may wake it, but missed notifications and another replica accepting an HTTP request must not lose
work. Do not require sticky sessions, a distributed in-memory registry, or a new message broker.
No database transaction stays open during a socket write. A send after ownership loss can be
ambiguous; stable operation IDs and the plugin journal remain necessary even with server fencing.

Supervise reader, writer, heartbeat and lease tasks, retaining handles and propagating shutdown.
Stop admission on shutdown, cancel/close the socket, join loops within a bounded drain, and leave
short recoverable leases. Enforce frame, connection, queue and database-work bounds from phase 01.

## Batch 03B — Durable command delivery

Use a versioned closed envelope containing installation/connection identity, operation ID,
operation kind, immutable payload digest, deadline and bounded typed payload. The server derives
company/machine identity from authentication rather than trusting peer-supplied company fields.
Freeze command inputs at admission and use a unique logical invocation key to deduplicate callers.
Same ID with another digest is a conflict, not an edit or a retry.

Before first dispatch, recheck live company/workflow authority, required Openbot approvals,
machine/project enablement and the absolute deadline through the application port. A queued
command whose authority was withdrawn is refused without an SDK effect. Once local acceptance
is possible, withdrawal is a cancellation/reconciliation problem, not proof that dispatch failed.

| Operation observation | Meaning |
| --- | --- |
| Queued | Openbot saved work; no local execution is established |
| Accepted locally | Plugin durably journaled the command; SDK acceptance is still separate |
| Running / waiting for input | Known Cloudroom thread/turn state with matching evidence |
| Succeeded / failed | Matching terminal turn result or definitive local refusal |
| Cancelled / expired | Proven pre-dispatch refusal or confirmed cancellation; late work may remain separately observed |
| Uncertain | SDK submission/outcome cannot be proved; automatic resubmission is forbidden |

Do not create one oversized status enum spanning machine connectivity, command delivery and
Cloudroom thread lifecycle. Model those dimensions separately and make allowed transitions
conditional. A stop/archive control has its own operation ID and names its target session; it
does not overwrite the original prompt's outcome.

Transport delivery is at least once. The plugin persists receipt before acknowledgment; replay
returns the receipt or current observation. An Openbot timeout never means the SDK did not accept
work. Do not reassign a possibly accepted command to a different machine/thread. Cancellation
racing dispatch becomes a stop request or uncertain outcome, not false proof of non-execution.

## Batch 03C — Event/result delivery and reconciliation

Plugin observations carry a stable journal sequence, operation ID, Cloudroom thread/turn IDs when
known, event kind and bounded data. They are untrusted input. Validate correspondence to the
saved machine/session/command, project/provider snapshot and permitted transitions. Positive
SDK identity evidence, rather than a peer's arbitrary `success` string, drives reconciliation.

Commit inbox deduplication, accepted operation state/output and any workflow wake-up together.
Only then acknowledge the highest contiguous durable plugin sequence. Reject sequence reuse with
different content. Results survive an Openbot crash before acknowledgment; replay cannot publish
or complete twice. Chunked output has a digest, total size and expiration; never acknowledge a
terminal output as complete before all required bytes are durable and validated.

After reconnect, reconcile outstanding commands and mapped local threads before dispatching new
effectful commands. A missing local journal/profile or changed installation is recovery-required,
not an empty machine ready to repeat old work. Progress deltas may be coalesced; terminal
receipts/output cannot be silently dropped. Fill event gaps from Cloudroom's public timeline if
the compatibility proof supports it; otherwise expose a gap and current snapshot honestly.

Disconnect lets already-running native work continue. Deadline expiry ends Openbot's wait and
blocks new dispatch, but cannot prove an offline process stopped. Late observations remain audit
evidence and do not resurrect cancelled/expired workflow runs. A stale workflow claimant can never
settle results; only a fresh fenced continuation can consume a committed operation outcome.

**Focused checks:** `cloudroom_connection_`, `cloudroom_protocol_`, `cloudroom_commands_`, and
`cloudroom_observations_`; real WebSocket and PostgreSQL tests, not sequential mocks alone. Race
two replicas for one machine, supersede a live socket, revoke during handshake/renewal, and lose
each command/event acknowledgment. Test slow consumers, malformed/chunked/oversized payloads,
outbox backpressure, old-generation cleanup, polling without notifications, and shutdown leaks.
