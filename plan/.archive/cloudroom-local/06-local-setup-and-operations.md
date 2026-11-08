# 06 — Local setup and operations

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [02](02-company-machines-and-pairing.md) through
[05](05-openbot-management-and-workflows.md). Deliver a reproducible single-company installation
on a Mac and a headless Linux server, including recovery instructions and truthful status.

## Batch 06A — Package and document the supported setup

Publish a versioned plugin artifact with integrity information and a locked compatibility manifest.
Document the exact supported Cloudroom release/build and public plugin installation command after
testing that artifact. Reproduce builds from pinned source/dependencies and retain applicable
upstream Apache/MIT notices. Do not describe the upstream `npx bb-app@latest` launcher as this
fork's verified release; defaults and features differ. Do not execute unpinned remote install
scripts as part of Openbot server startup.

The tested setup instructions must cover:

1. Install Cloudroom's local server/host daemon and the chosen agent CLIs using their supported
   installers. Use the supported Node runtime from the compatibility manifest for headless builds.
2. Run under one unprivileged company OS account; authenticate providers there. The plugin works
   through native local providers, not through Rust Core or Cloudroom's hosted sandbox service.
3. Open a local project, prove a manual native agent task, install the plugin, and pair using the
   Openbot origin and short-lived code. Store secrets outside shell history, scripts and logs.
4. Select advertised projects and create an Openbot local profile. Run an explicit smoke task,
   confirm the same thread in Cloudroom's UI, and prove a follow-up and stop.
5. Arrange startup/restart and verify disconnect, re-pair and backup recovery before relying on
   unattended scheduling. Laptop sleep/shutdown makes the machine unavailable.

Linux: supply a tested systemd unit or wrapper for Cloudroom's supported launcher and host daemon,
with `User`, `WorkingDirectory`, explicit profile/data directory, bounded restart behavior and
graceful shutdown. No root execution, privileged port, Docker socket or graphical desktop is
needed. Provider authentication and PATH must match that service account, not the administrator's
interactive shell. Use journald or bounded rotating logs.

macOS: use the desktop-managed server/daemon or a supported headless LaunchAgent, never both
launchers on the same profile. Both Openbot and the UI must target that same server/profile.
Closing a window is distinct from quitting/stopping services; document observed behavior for the
tested release instead of promising agents survive application exit. Native files/tools stay on
the Mac; no Linux VM is required for this local-server integration.

The local API stays on loopback. Remote Cloudroom UI access is an optional, separately authorized
private network/SSH setup, not a public proxy shipped by this feature. The plugin opens outbound
TLS/WSS to Openbot; no port forwarding or Cloudroom hosted account is needed.

## Batch 06B — Operations, shutdown and retention

Expose durable metrics/status for connected/stale/revoked machines, queue depth/age, accepted
operations, permission waits, uncertain outcomes, reconciliation gaps, backpressure, and last
successful provider task. Distinguish transport availability from provider authentication and
actual inference. Logs use company/machine/operation/thread IDs and redacted errors; never raw
prompts, outputs, pairing codes, credentials, full auth headers or SDK request objects.

Keep Openbot's SQL-backed command/terminal summaries for 90 days after terminal settlement and
while referenced by active workflows or unresolved operations. Detailed progress events expire
after 7 days once acknowledged/settled. Retention workers are paginated, fenced and bounded; they
cannot remove an outstanding command, uncertainty evidence, or the only accepted result needed
by a waiting workflow. Artifact/output deletion follows the same authorized retention policy.

The plugin compacts acknowledged progress, retaining mappings and terminal receipts for at least
the corresponding server reconciliation window. Backpressure at its 64 MiB outbox bound stops
new effects and coalesces progress while reserving space for bounded terminal summaries. Do not
discard unacknowledged terminal output to make room; pause/refuse admission visibly. Cloudroom's
own transcript retention remains its responsibility, separate from the plugin journal.

Back up the Cloudroom profile, provider-managed session state and plugin journal/secret settings
under their existing sensitive-data handling. Coordinate restore so two clones cannot connect as
one installation. A new installation/re-pair never replays old unresolved commands. Document how
to inspect uncertain work in Cloudroom and explicitly accept or abandon it; a new execution is a
new user-authorized operation with provenance, not a hidden retry.

A disconnected machine may keep already-running agents working locally. Disable/revoke blocks
Openbot admission and authentication immediately at the server; stopping offline native processes
requires local access or reconnection. Disconnect/uninstall preserves workspace files and provider
logins. Archive is not delete. Openbot does not remotely wipe the trusted company account.

**Focused checks:** installation/uninstallation smoke with packaged plugin on both platforms;
headless Linux boot/restart without a display; macOS single-profile desktop/headless behavior;
service shutdown during active work; secret redaction; catalog/provider diagnostics; bounded
retention racing an active workflow; outbox exhaustion and restore/re-pair without replay.
