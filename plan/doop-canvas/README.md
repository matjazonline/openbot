# Company-scoped Doop canvas documents

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and status

Status: proposed implementation, 2026-11-06. Publishing this plan does not implement the integration
or establish that our deployed Doop instance passes the requirements below.

An Openbot agent creates and updates a document on our self-hosted Doop instance. A human opens the
private canvas, collaborates in Doop, and can export a PDF there. Each company uses its own Doop
workspace, dedicated bot account, and company-bound MCP credential. The initial deliverable is a
private canvas link; Word processing, DOCX output, and storing PDF attachments in Openbot are outside
this plan.

Use existing MCP transport, credential encryption, operation registration, workflow resources,
agent publication, approvals, and dispatch authority. Do not introduce another agent runtime,
transport stack, generic integration gateway, or document database for this vendor.

**Confidentiality prerequisite:** implement and verify [the separate file-access fix](../fix-file-access/README.md)
before using real company content. Workspace access alone does not protect Doop's public frame
images and uploaded-asset URLs. This plan consumes that fix; it does not duplicate its implementation.

## Implementation order

| Group | Deliverable | Dependencies |
| --- | --- | --- |
| [01 — Contracts and company setup](01-contracts-and-company-setup.md) | Pinned Doop compatibility proof, workspace/bot ownership, audience contract | Existing Openbot MCP support |
| [02 — Company MCP connections](02-company-mcp-connections.md) | Company connection setup, credential custody, reviewed tool catalog | 01 |
| [03 — Workflow and agent authoring](03-workflow-and-agent-authoring.md) | Fixed-workspace creation and bounded agent editing through existing actions | 02 |
| [04 — Results and human access](04-results-and-human-access.md) | Validated canvas result, private link, explicit human membership | 03; file-access fix for confidential acceptance |
| [05 — Operations and lifecycle](05-operations-and-lifecycle.md) | Reproducible deployment, rotation, revocation, recovery, deletion | 02–04; file-access fix |
| [06 — Integration and verification](06-integration-and-verification.md) | Two-company acceptance and final full-suite gate | All previous groups and file-access fix |

Each numbered file is one implementation group with its own focused checks and exit criteria.
Groups 01–03 may use disposable synthetic content while the independent file-access fix proceeds.
Real company content is blocked until both plans pass their confidentiality gates.

## Fixed ownership and security decisions

- One Doop instance may host several companies. Workspace membership is application access control,
  not separate infrastructure or protection from trusted instance/database operators.
- Doop agent keys are account-scoped. Two keys minted by one account with several workspaces do
  not isolate companies. A managed bot identity belongs to exactly one company workspace; its
  credentials are never shared between companies.
- Openbot determines the company from authenticated/run authority. Models and external tool
  results cannot choose another company's connection, credential, workspace binding, or audience.
- Canvas creation is a deterministic workflow action with a fixed workspace ID. The agent gets
  the resulting canvas ID and only the reviewed editing/reading tools it needs.
- Human Doop membership is explicit. An Openbot session or a generated URL does not grant Doop
  access. There is no automatic SSO or membership synchronization in this release.
- Every workspace member can edit its canvases in the inspected upstream version. Initially allow
  only inputs approved for that entire workspace audience; reject narrower Openbot content rather
  than silently widening access. Doop memberships and Openbot audiences must be reconciled by setup.
- Non-idempotent external calls use existing uncertain-outcome/reconciliation behavior. An MCP
  annotation, workflow retry, or request ID does not make canvas/frame creation exactly once.

## Evidence and existing owners

Upstream source inspected at [`01584b34f6cadcf21767127280cc6e320a6d748a`](https://github.com/kgoedecke/doop/tree/01584b34f6cadcf21767127280cc6e320a6d748a).
Recheck the actual deployed revision before implementation:

- [Doop access policy](https://github.com/kgoedecke/doop/blob/01584b34f6cadcf21767127280cc6e320a6d748a/server/access.ts):
  canvas owner, explicit invitations, workspace membership, and optional link access.
- [Doop MCP](https://github.com/kgoedecke/doop/blob/01584b34f6cadcf21767127280cc6e320a6d748a/server/mcp.ts#L444):
  `list_canvases`, `get_profile`, `create_canvas`, frame editing, screenshots, and PDF resources.
- [Doop agent-key schema](https://github.com/kgoedecke/doop/blob/01584b34f6cadcf21767127280cc6e320a6d748a/server/db/schema.ts#L273):
  account identity rather than a workspace scope.
- [Connection entity](../../src/domain/entities/mcp.rs):100–113 and
  [credential/use-case ports](../../src/application/use_cases/mcp.rs):82–123 already qualify
  connections and tokens by company.
- [MCP transport](../../src/adapters/mcp/mod.rs):216–255, 274–294 owns calls, error envelopes,
  resource content, and redaction. Its existing handling of resource content is not proof that
  downstream model/browser paths can safely consume a large screenshot or PDF.
- [Workflow MCP authority](../../src/adapters/persistence/workflow/mcp.rs):26–37, 152–186 and
  [operation contracts](../../src/application/workflow/mcp.rs):68–98 own live scoped dispatch.

The proposed [vault/integrations plan](../.archive/vault-google/README.md) is not a prerequisite. Use the
credential owner actually present when implementing; if that plan has landed, reuse its approved
credential reference rather than introducing a second secret store.

Before edits, consult graft, trace affected symbols/callers, and read applicable AGENTS.md files.
Record exact commands, nonzero selected/executed test counts, and unavailable checks per batch.
Run focused checks during implementation and the complete final gate once after integration.
Register every Markdown file here in `NO_BACKWARD_COMPATIBILITY_NOTES` in `src/main.rs`.
