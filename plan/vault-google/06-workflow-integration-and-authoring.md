# 06 — Gateway execution, workflow integration, and authoring

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [05](05-token-resolution-and-refresh.md). Humans can select a vault-backed connection
in existing HTTP/MCP workflow steps. LocalIntegrationGateway owns authenticated execution and
returns bounded app results and effect observations; workflow handlers never receive credentials.
No persistent state or raw-token values are added to the workflow language.

## Batch 06A — Local gateway and authenticated execution

Implement LocalIntegrationGateway using the private resolver, provider/app registry, credential
injectors, and existing bounded HTTP/MCP transports. Treat http_for_action, HttpProvider::invoke,
mcp_use, and McpRuntime as integration anchors. Split host-specific preparation/authority from
credential custody and transport composition; do not duplicate the transport stack. Place new
domain/application/adapter modules according to dependency direction rather than putting a public
abstraction inside persistence or an HTTP adapter.

Openbot keeps action preparation, reservation, approvals, worker fencing, and replay decisions.
Its adapter converts an authorized deterministic action into the gateway invocation from 01 and
maps the gateway outcome back into existing receipt/effect handling. Module code sees only the
opaque host authority reference and shared contract values, not FrozenAction or workflow SQL.
Preserve rechecks after DNS/catalog waits and the current action reservation/approval boundary.
A resolver failure before business dispatch is a typed pre-dispatch failure, not an ambiguous send.

Authorize before credential resolution, resolve outside the short action transaction, then
enter 03's short final dispatch gate to validate account/grant versions and invoke the required
DispatchAuthority host adapter immediately before sending/opening the business call. It checks
binding identity, membership, connection, current policy, approved arguments, worker fencing, cancellation, and live
workflow ownership. Refreshing a token cannot substitute another account, destination, or operation.
The gateway cannot execute from an arbitrary account ID plus caller-supplied company/actor IDs.

Use the same gateway boundary for static bearer, registered API-key header, and managed OAuth
accounts. A new app/provider changes the registry and its adapter, not workflow syntax or shared
HTTP/MCP handlers. Grant policy, app capability requirements, injection, and transport destination
checks apply to every method. Do not pass an arbitrary URL to a generic credentialed proxy.

Reuse existing request/response, argument, header, catalog, session, timeout, concurrency, and
network limits; validate the gateway's DTO projections against those same bounds. Authenticated
headers are inserted only inside the module and are stripped from diagnostics and ordinary
results. Serialize provider data through existing bounded/redacted output handling, never an
internal transport request/session or secret wrapper. Samples use inert gateways and cannot
construct a production authority reference.

## Batch 06B — App destinations, MCP sessions, and effect semantics

Apply a provider/app-controlled destination policy to managed accounts. For Google/Gmail, the
required HTTP destination is the Gmail API HTTPS origin and API path family; deny arbitrary
hosts, redirects, user-supplied authorization headers, and non-Gmail Google APIs. A separate
registered Google app may declare
another API policy; connecting Gmail does not enable it. Existing network/SSRF controls still apply.
The token endpoint and identity endpoint are internal adapter destinations, not workflow targets.

For MCP, permit a Google credential only for an explicitly supported Google server with verified
authentication/scopes and a pinned allowed endpoint. A user-controlled MCP server cannot receive
the raw Google token merely because its URL passes general SSRF checks. Static vault secrets
remain usable with other owner-approved MCP endpoints.

Keep authenticated MCP sessions and their credential bytes inside integrations. Discovery is a
separate typed owner/configuration context authorized through the host adapter; it cannot be used
as a business call or token read. Return only bounded catalog metadata or business tool results.

Give sessions a credential identity that includes account authorization revision, grant identity,
app/destination policy, and credential generation. On refresh, reopen or reauthenticate any
session that would send an obsolete credential. If a
generation changes during catalog discovery, close and rebuild within a bounded attempt count or
yield a safe pre-dispatch retry. Key rewrapping alone must not change this identity.

Do not add a blanket refresh-and-replay wrapper around HTTP/MCP operations. Preserve current
uncertain-effect handling: a timed-out Gmail send may have succeeded, so token renewal does not
justify sending it again. A stable Message-ID is useful correlation, not a Gmail exactly-once API
guarantee. Keep existing action approval and replay contracts authoritative.

Retain explicit effect classification across the gateway. Distinguish rejection before sending,
committed provider result, and uncertain outcome after possible dispatch. Propagate deadlines,
lease loss, and cancellation into actual transport work and close sessions on shutdown. An
invocation/request ID provides correlation and reconciliation; it does not supply provider
idempotency. A future gateway network timeout must preserve this distinction rather than map
every failure to a generic retryable error.

## Batch 06C — Manual authoring and sample behavior

Add a credential-source picker to existing connection management, showing only entries the owner
can grant or metadata for bindings already approved for that company. Other administrators see
configured account/status information within existing connection permissions, never secret
readback or the rest of the owner's vault.

Owner UI calls generic account/connect/grant operations and renders app-specific display metadata
from the registry; adding a provider must not add a provider branch to workflow authoring. Keep
login-provider buttons and app-connect buttons as different UI flows.

Workflow definitions continue referencing HTTP/MCP connection IDs. Publication checks reject
missing/unapproved bindings and direct tool exposure. Samples use scripted responses and inert
credential references; they must not refresh Google tokens or open real MCP sessions.

Provide two manually authored examples using actual supported workflow syntax:

1. Read a supplied Gmail thread/message ID through HTTP, pass bounded selected message data to
   a model step, and return suggested reply text for review.
2. Send an explicitly supplied, reviewed Gmail message through HTTP. The input supplies a valid
   base64url-encoded RFC 5322 MIME message; the workflow sends it with the correct thread fields.

Do not invent an existing MIME/base64 workflow transform or ask a model to generate trusted
encoding. The graph currently shows codecs in adapters, not a general workflow encoding operator.
The send example may take reviewed raw MIME as manual input; automatic construction/encoding can
be added separately if required. Validate fixture encoding with the existing base64 dependency.

Where a compatible MCP server exposes required operations, document its tool catalog and
equivalent example separately. Google currently describes its Gmail MCP server as Developer
Preview, including read and draft operations; do not promise a send tool or require preview
enrollment for the HTTP examples.
[Google MCP documentation](https://developers.google.com/workspace/gmail/api/guides/configure-mcp-server).

## Focused checks and acceptance

Add integration_gateway_, vault_workflow_http_, vault_workflow_mcp_, and vault_authoring_ families
using real authority persistence plus scripted transports. Verify token refresh at execution after approval/wait,
no refresh during publication/sample, correct account selection, destination rejection before
credentials leave the process, no raw-secret output, MCP generation changes during discovery,
and no duplicate business effect on uncertain sends. Exercise static API-key injection and an
OAuth fixture through the same gateway, host-authority denial after every wait, conflicting auth
headers, bounded outcomes, and cancellation/session cleanup. Gateway outputs, workflow receipts,
sample fixtures, and serialized errors must contain no injected credentials.

Batch checks: 06A uses integration_gateway_, vault_workflow_http_, and transport boundary checks;
06B uses vault_workflow_mcp_, gateway effect cases, and workflow_action_approval_; 06C uses
vault_authoring_, sample-boundary checks, and affected UI/browser tests. Apply 02's SQL checks
to changed queries. Do not run the final full-suite gate until 07's portability checks are complete.

~~~sh
cargo test --locked --offline --lib integration_gateway_ -- --list
cargo test --locked --offline --lib integration_gateway_
cargo test --locked --offline --lib vault_workflow_http_
cargo test --locked --offline --lib vault_workflow_mcp_
cargo test --locked --offline --lib vault_authoring_
cargo test --locked --offline --lib workflow_action_approval_
cargo test --locked --offline --lib sample_direct_workflow_tool_child_and_message_have_zero_production_effects
node --test scripts/tests/workflow-samples.mjs
python3 scripts/workflow-sample-boundary.py
~~~

List filters and record counts/results. Run affected connection route/page tests and regenerate
CSS only if UI classes change. Acceptance: a manually authored workflow performs authorized HTTP
mailbox reads/sends, MCP uses the same authenticated gateway boundary, and direct agent tools
cannot acquire those credentials. No provider-specific workflow variant was needed.
