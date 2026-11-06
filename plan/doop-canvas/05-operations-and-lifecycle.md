# 05 — Operations and lifecycle

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependencies

Depends on 02–04 and the file-access fix. Operate the integration as a separately deployed Doop
service, using our accepted revision and ordinary Openbot MCP connections. Vendor source changes
belong in the actual Doop checkout/fork; this planning repository must not imply they are already
present or vendor an unrelated source tree merely to record the plan.

## Batch 05A — Deployment and setup runbook

Record the accepted Doop commit, lockfile/toolchain, image digest, private storage configuration,
canonical HTTPS origin, database/backup location, Chromium availability, and applied access fixes.
Run production services as non-root; retain renderer sandboxing wherever supported and document
any narrowly necessary platform capability. Pin newly introduced git dependencies and build inputs.

Use only configuration keys read by the deployed applications. Trace them before publishing
examples; separate build-time values from runtime secrets. Do not invent Doop configuration flags
or claim Openbot startup validates remote permissions, private buckets, or bot exclusivity unless
the implementation actually performs those checks. Setup/health probes must say what they tested.

Cover synthetic setup, identity verification, catalog review, fixed workspace configuration,
agent/workflow publication, explicit human invitations, and enabling use only after file-access
acceptance. Do not enable confidential production use on the unpatched upstream image.

**Focused checks:** deployment/runbook dry run against a disposable pinned Doop instance; startup
and transport-policy regressions for any actual configuration changes. Verify non-root execution,
private storage, source/image reproducibility, and absence of secrets in generated examples/logs.

## Batch 05B — Rotation, disconnect, and data ownership

Rotate the company's bot key through existing encrypted MCP credential replacement and key
rotation inventories. Confirm old keys are revoked in Doop and new dispatch uses current
authority; neither a cached MCP session nor an old publication may bypass revocation. Report
credential revision changes according to existing approval/invalidation contracts.

Disabling an Openbot connection stops new authorized calls; it is not proof that accepted remote
work was cancelled. Explain timeouts, partial canvases, uncertain creates, and manual reconciliation.
Re-enable only after verifying the same company identity/workspace, without automatic replay.

Disconnect does not implicitly delete company documents. Remote deletion is an explicit
authorized operation with a documented owner, confirmation, and recovery policy. Explain Doop's
workspace-deletion behavior: inspected upstream detaches canvases into their owners' personal
spaces. Do not call workspace deletion secure document erasure. Deprovision humans/bots and clean
up canvases/assets/backups deliberately under the retention policy.

Document restore with a reset/re-provision policy for incompatible schema/payload changes. There
are no legacy URL decoders, dual credential stores, data backfills, or staged compatibility mode.
Use structured identifiers/outcomes for troubleshooting without storing prompts, bearer headers,
raw HTML, media bytes, or private URLs containing credentials in logs/metrics.

**Focused checks:** `doop_lifecycle_` tests for rotation, disabled/revoked access, restore,
connection replacement, uncertain work, and disconnect without remote deletion. Race rotation or
revocation with active dispatch using real competing workers. Test Doop membership removal and
media revocation through the independent file-access suite.

## Exit criteria

The runbook states supported behavior, access boundaries, remaining limitations, and exact tested
versions. Operators can rotate/revoke access and reconcile partial work without crossing companies
or losing documents through an implicit disconnect/delete coupling.
