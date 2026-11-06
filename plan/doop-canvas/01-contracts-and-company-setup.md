# 01 — Contracts and company setup

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependencies

Define the tested boundary before changing Openbot. Read the [shared decisions](README.md) and
[file-access dependency](../fix-file-access/README.md). Use disposable companies and synthetic data.

## Batch 01A — Trace the deployed contract

Record the actual Doop source revision, image digest, MCP protocol/SDK versions, and deployment
origin. Compare them to the inspected upstream revision in the README; do not assume a current
GitHub README describes our running instance. Pin the final accepted source revision and image.

Probe authenticated initialization, discovery, `get_guide`, `get_profile`, `list_canvases`,
`create_canvas` with `workspace_id`, reading/editing frames, and `export_frame`. Capture bounded,
redacted schemas/results as fixtures. Trace screenshot image blocks and embedded PDF resources
through Openbot's existing MCP result and agent paths. Keep large binary results out of the model
context; a rejected oversized PDF does not block the canvas-link release.

Check schemas against current discovery/operation limits. Preserve existing bounds; remove
unneeded tools/content or use compact reads before considering a justified limit change with
an early-failure CI test. Record whether revision/conflict checks exist for each editing tool.

**Focused checks:** new `doop_contract_` Rust fixtures for actual discovery/result shapes, plus a
counted local MCP fixture using the real transport. List tests before running and reject zero
matches. Separately record the real-Doop probe; fixtures alone are not compatibility acceptance.

## Batch 01B — Provision two isolated company identities

For each company, an authorized operator creates one workspace, one dedicated non-admin bot
account, and an agent key owned by that bot. Use normal supported Doop setup/key-minting flows;
do not write authentication rows directly or retain a shared instance-admin credential in Openbot.

Record a secret-free setup record: Openbot company, Doop origin, workspace ID, bot user ID,
connection ID, reviewed revision, intended human members, and approved data audience. Keep the
workspace ID in trusted published workflow configuration alongside its company resource binding;
do not add a separate mapping table when these existing owners suffice.

Managed bots must not join another workspace, receive unrelated direct canvas invitations, or
own unrelated personal canvases. In Doop, enforce the managed bot/workspace association at the
membership, invitation, share, and move boundaries; ordinary company owners must not be able to
add another company's managed bot. Reuse existing workspace/access owners for a small guard if
the deployed revision cannot express this restriction. Document trusted operator override access.
Do not mistake a one-time setup check for enforcement of later changes.

Private company workspaces must prohibit anonymous/link-wide access and Community publication.
Keep these restrictions coherent with the file-access plan's private-media policy. Invite only
the intended human audience and explain that upstream workspace membership currently grants edit.
Cross-company human membership, if deliberate, must not merge bot identities or credentials.

**Focused checks:** real Doop tests with bot A/B and human A/B: own-workspace read/edit succeeds;
foreign IDs, foreign workspace creation, managed-bot cross-invitation and cross-workspace moves
fail. Verify revocation, direct-share restrictions, and private sharing cannot be overridden by
an ordinary workspace member. All management probes use disposable identities.

## Exit criteria

The compatibility record distinguishes verified behavior, needed Doop changes, and unavailable
checks. Both company mappings and audience restrictions are reviewable without exposing keys.
There is no unresolved account-wide path that grants a managed bot another company's private work.
