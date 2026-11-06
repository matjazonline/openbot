# Reusable app integrations, per-user vault, and Google/Gmail

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and status

Status: proposed implementation, revised on 2026-11-06. None of the stages below is implemented by
this plan. The existing directory name is retained; the shared architecture covers multiple apps
and authentication methods, with Google/Gmail as the first production OAuth integration.

A user can save reusable private credentials, connect external accounts, and authorize selected
company HTTP or MCP connections to use them. The first release supports static credentials and
personal Gmail or Google Workspace mailboxes.
Humans author the workflows. Mailbox operations execute through ordinary workflow steps; model
steps receive only the message data and business outputs supplied to them.

Workflow steps remain stateless. An integrations module inside Openbot owns connected accounts,
vault persistence, provider authorization, credential renewal, and authenticated HTTP/MCP
execution. It initially uses the existing PostgreSQL database and credential envelope encryption.
Openbot consumes an application-owned IntegrationGateway port through LocalIntegrationGateway.
The module's contract must support a future RemoteIntegrationGateway without exporting tokens
or depending on Openbot workflow/database types. A separately deployed service is not part of
this implementation.

## Implementation order

| Stage | Deliverable | Depends on |
| --- | --- | --- |
| [01 — Contracts and ownership](01-contracts-and-ownership.md) | Provider/app/account/grant contracts, gateway boundary, bounds, and revision semantics | Existing credential and workflow contracts |
| [02 — Vault storage and management](02-vault-storage-and-management.md) | Module-owned account/credential storage, owner API/UI, deletion, and key rotation | 01 |
| [03 — Connection bindings and authority](03-connection-bindings-and-authority.md) | Owner-approved use grants and live dispatch authority | 02 |
| [04 — Providers and Google authorization](04-google-authorization.md) | Shared connect lifecycle, provider adapters, and separate Gmail consent | 02; use 03 for connection attachment |
| [05 — Internal credential lifecycle](05-token-resolution-and-refresh.md) | Provider-specific renewal, concurrency, reconnection, and disconnect behavior | 03, 04 |
| [06 — Gateway execution and authoring](06-workflow-integration-and-authoring.md) | Authenticated HTTP/MCP execution, session handling, and manual Gmail examples | 05 |
| [07 — Portability, verification, and operations](07-integration-verification-and-operations.md) | Non-Google adapter proof, service-contract checks, operating guidance, and final full-suite gate | All earlier stages |

Implement these stages in order. Each stage has cohesive batches, focused checks, and acceptance
criteria. Finish the shared contract before changing transport implementations.

## Module and future service boundary

    Openbot owner UI / deterministic workflow step
      -> Openbot session, company, workflow, approval, and action authority
      -> IntegrationGateway
      -> LocalIntegrationGateway (future: RemoteIntegrationGateway)
      -> connected account + owner-approved use grant
      -> private credential resolution + provider/app destination policy
      -> existing bounded HTTP/MCP transports + final host authority check
      -> external app response / dispatch outcome, never credential bytes

Separate these concepts rather than adding a shared enum variant and workflow branch per vendor:

| Concept | Owns | Example |
| --- | --- | --- |
| AuthProvider | Authorization, identity discovery, refresh/revocation strategy | Google OAuth |
| AuthConfig | Server-controlled provider/client/instance and scope configuration | Production Google client |
| AppDefinition | API destinations, capabilities, and supported auth/injection methods | Gmail or Calendar |
| ConnectedAccount | Private owner, external authorization identity, status, and credential reference | Personal Google account |
| UseGrant | Owner-confirmed use by a particular host connection, app, and destination | Company A's Gmail HTTP connection |
| CredentialStore | Bounded encrypted payloads, narrow reads, locking, and key rotation | PostgreSQL adapter |

Google is an authorization provider; Gmail and Calendar are different apps. A connected account
can serve multiple apps only where its provider authorization and granted capabilities permit it;
each company connection still needs its own use grant. External identities may be users,
workspaces, installations, or resources. Email is optional display/verification data according to
the provider, not a universal account key.

Use module-owned account and grant persistence plus narrow opaque references on existing HTTP/MCP
connections. Auth/provider/app definitions start in a code-owned registry with server configuration;
do not build a general ACL engine, dynamic plugin loader, or app marketplace. Existing connection
registration remains the operation catalog; a new cross-app action/tool catalog is deferred.

Openbot owns login, company membership, workflow provenance, publication, approvals, leases,
cancellation, action reservation, and business-operation replay policy. Integrations owns account
custody, credential lifecycle, grants, provider-controlled destinations, and authenticated transport
execution. A host authority port preserves Openbot's live final dispatch checks without importing
FrozenAction, sessions, SQLx transactions, or other Openbot implementation types into the module's
public contract. See 01, 03, and 06 for the authorization handoff.

Credential resolution is private to integrations. The public boundary offers connect/account/grant
management and authenticated execution; it does not offer resolve_token or raw-secret export.
Provider adapters keep typed credential decoders and renewal policies behind a common lifecycle.
Use the existing transports and their protections rather than building a second HTTP/MCP stack.

The first production OAuth adapter is Google. A static API-key app fixture and a test-only
non-Google OAuth adapter must prove different injection, identity, and rotating-refresh behavior.
These fixtures are not claims of production support for another named vendor. A general workflow
variable store, secret interpolation language, token-export API, separate server, background
refresh scheduler, production second OAuth vendor, and direct agent app tools remain out of scope.

## Decisions carried from the discussion

- Vault ownership is per Openbot user, independent of company ownership and of the connected
  external identity. A user's login email need not match the mailbox they connect. The gateway
  uses a stable client namespace and owner subject derived by the trusted host adapter; company
  sharing is a grant, not a change to private ownership.
- Openbot sign-in and external-app authorization remain different flows, even for the same
  provider. Shared OAuth utilities do not merge callbacks, consent, sessions, or credentials.
- A company does not inherit access to its members' private vaults. The owner explicitly grants
  use to a named connection, destination, and workflow-only execution policy.
- HTTP/MCP steps remain the mailbox execution interfaces. Managed credentials do not become
  direct agent tools. Existing explicitly authorized workflow tools may wrap these workflows.
- Refresh is on demand at actual dispatch. Every execution can resolve the same reference;
  only an adapter whose credential strategy needs renewal contacts a token endpoint.
- A database row lock on the shared connected-account credential serializes refreshes across
  runs and processes. All references to one external authorization share that record.
  Compare-and-swap alone is insufficient to prevent overlapping token-endpoint calls. Provider
  strategies explicitly distinguish reusable and rotating refresh tokens and ambiguous recovery.
- Token cache writes do not change connection policy or invalidate an otherwise current
  approval. Binding, account, scope, secret replacement, and revocation changes do.
- Gmail API over HTTP is the required read/send path. MCP uses the same vault mechanism for
  compatible servers; Google's preview MCP service is not a prerequisite for the HTTP release.
- The local gateway returns provider results and typed effect observations. Request IDs do not
  make an external send idempotent, and gateway timeouts do not justify automatic replay.
- Moving the module into a service must move token injection and authenticated transports with
  its credential store. Stage 07 records the required remote authority/revocation, lifecycle,
  cancellation, and ambiguous-result protocols; deployment is not a transparent wiring change.

## Current repository anchors

Source spans are scoping anchors, not claims that the proposed vault exists:

- [Google sign-in](../../src/adapters/http/routes/user.rs):174–240 currently requests only
  identity scopes and models only the access token.
- [Google configuration](../../src/infra/config.rs):253–270 reads GOOGLE_OAUTH_CLIENT_ID and
  GOOGLE_OAUTH_CLIENT_SECRET as a required pair.
- [Credential contexts](../../src/adapters/persistence/credentials/envelope.rs):96–192 bind
  encrypted payloads to their persisted identity.
- [Credential rotation](../../src/adapters/persistence/credential_rotation.rs) inventories and
  rotates every supported secret store; the new vault must join that inventory.
- [HTTP credential loading](../../src/adapters/persistence/workflow/http.rs):328–369 and
  [last-moment dispatch checks](../../src/adapters/workflow_http/mod.rs):126–138 already load
  scoped credentials in the backend.
- [MCP credential loading](../../src/adapters/persistence/workflow/mcp.rs):296–333 already returns
  an internal token and credential revision.
- [Schema rules](../../src/adapters/persistence/AGENTS.md) require changes to the init schema and
  verification against freshly migrated databases.

Before implementation, refresh graph context and trace the changed symbols and their callers.
Read the relevant AGENTS.md files. Preserve dependency direction and existing action, approval,
cancellation, sample-isolation, and key-rotation protocols.

## Verification discipline

All test prefixes in these files are future test families to add. A filter matching zero tests
is not evidence. For each batch record commands, results, nonzero executed-test counts, and
unavailable checks. Use real competing claimants for new locking and revision protocols.

After SQL changes, apply the init schema to a fresh disposable database and regenerate SQLx
metadata. Use focused tests during implementation; run the complete CI inventory once after all
stages are integrated, following [07](07-integration-verification-and-operations.md). The final
gate includes formatting, offline compilation, migrations, and the database-backed suite.

Every Markdown file in this directory carries the reset policy above and must appear in
NO_BACKWARD_COMPATIBILITY_NOTES in src/main.rs.
