# 01 — Integration contracts and ownership

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Define the reusable integrations domain, public gateway, and host authorization boundary before
changing storage or workflow execution. This stage establishes contracts; it adds no Gmail calls.

## Batch 01A — Provider, app, and connected-account contracts

Introduce domain newtypes for account ID, provider/app/config IDs, use-grant ID, owner namespace,
owner subject, key, revision, and authorization revision. Keep secret wrappers separate from
ordinary metadata. Follow the repository's domain/application/adapter dependency direction.

Define AuthProvider, AuthConfig, AppDefinition, ConnectedAccount, UseGrant, and CredentialStore as
described in the README. AuthProvider handles the external authorization lifecycle; AppDefinition
handles API destinations and capabilities. AuthConfig is a server-controlled configuration identity
and references client secrets privately. It is not the user's external account or a company
connection. A Google account may support Gmail and another Google app without duplicated refresh
credentials, but only after the necessary consent and separate use grants.

Start with a code-owned registry, not a dynamic provider description language. Shared contracts
use provider/app IDs and authentication methods, not google_oauth variants. Each registered
adapter validates its own typed credential payload; arbitrary JSON cannot claim a managed kind.

A connected account and its private credential record have these logical fields:

| Field | Meaning |
| --- | --- |
| id, owner_namespace, owner_subject | Stable account identity and private owner supplied by the trusted client |
| key | Immutable, owner-unique lookup name, such as google/personal or crm/api-key |
| provider_id, auth_config_id | Authorization provider and immutable server-controlled client/instance identity |
| auth_method, credential_schema | Static secret/API key or OAuth authorization-code method; adapter-owned current payload schema |
| external_identity | Provider-defined subject/workspace/installation/resource identity, separate from the local owner |
| envelope | Encrypted bounded value; only narrow credential reads select it |
| revision | Changes on logical payload/status writes; supports conditional management updates |
| authorization_revision | Changes when identity, scopes, owner-authorized secret, or use permission changes |
| status | ready, reconnect_required, or disconnected |
| metadata, created_at, updated_at | Bounded owner-visible account/status information and timestamps |

A static secret value may be a UTF-8 string or bounded JSON. A typed static app adapter accepts
only its supported shape and injection method, initially bearer or a registered API-key header.
Unsupported structured secrets cannot be serialized into a header by guessing.

Managed credentials are decoded by the registered provider adapter. The Google payload contains
provider account identity, OAuth client identity, granted scopes, refresh token, cached access
token, expiry, token generation, and optional shared refresh retry time. Other adapters may have
no refresh token, rotating tokens, or workspace/installation identities. Generic editing cannot
create or replace managed bundles. Metadata excludes tokens and ciphertext. A credential_schema
is a current typed discriminator, not permission to add legacy decoders or schema backfills.

Start with code-owned bounds: 128 bytes per key or registry ID, 16 KiB per decrypted credential,
4 KiB of account metadata, 64 granted scopes of at most 256 bytes each, 128 accounts per owner,
128 use grants per account, and 50 results per list page. Bound owner/external identity components
to 256 bytes and validate canonical authorization keys through the adapter. Validate keys once,
treat them as data, and enforce bounds on every writer, decoder, and list. The 16 KiB allowance
applies only to the new vault encryption context; retain existing credential limits and add
boundary tests rather than raising their global limit. Enforce aggregate payload bounds as well
as field bounds; a maximum-length scope list does not override the credential envelope limit.

Each adapter defines an authorization identity key and its uniqueness requirements. For Google,
use local owner, Google subject, and OAuth client; do not impose that tuple on workspace- or
installation-based providers. Different labels, apps, and company connections reference one
authorization instead of copying its tokens. Provider API permissions are recorded facts, not
proof that Openbot has authorized their use.

## Batch 01B — Gateway and module boundary

Define IntegrationGateway in the consuming application layer, with cohesive management and
execution subports where needed. LocalIntegrationGateway composes module services; it is not a
new HTTP server. Its conceptual operations are:

    begin_connect(owner, app_id, auth_config, requested_capabilities, safe_return_target)
    get_connect_attempt(attempt_id)
    list_accounts(owner, page)
    create_or_replace_static_account(owner, typed_input, expected_revision)
    disconnect_account(account_id, expected_revision)
    create_or_revoke_use_grant(owner_confirmation, connection_binding)
    execute_http(authorized_invocation)
    call_mcp(authorized_invocation)

Provider callback completion is a separate adapter-facing operation bound to the stored connect
attempt; browser query parameters never establish owner or grant authority. UI routes adapt
verified sessions into commands. A future server authenticates a client before trusting its
owner namespace; accepting a caller-supplied owner ID is never authorization.

All management operations require verified owner/client context, including attempt polling,
account detail/disconnect/deletion, and grant operations, even where the conceptual signatures
above abbreviate it. IDs alone never authorize a read or write. An account is the owner-facing
vault entry; get/delete/detail and grant inspection are management subport operations even when
not individually listed above.

Management results are bounded metadata, status, revision conflicts, or sanitized connect URLs.
Execution inputs contain opaque account/grant and host-binding references, expected authorization
versions, app/operation identity, bounded arguments, request digest, correlation/dispatch identity,
deadline, cancellation context, and an opaque host-authority reference. Neither execution input
nor execution output carries credentials, cookies, SQLx transactions, FrozenAction, workflow
leases, or framework types. Runtime cancellation stays in a local execution context; the wire
projection uses the deadline and opaque cancellation/dispatch identity described in 07. Operation
arguments must not supply authorization headers or arbitrary credential injection instructions.

Execution results retain bounded provider data and typed pre-dispatch/committed/uncertain outcomes
compatible with existing action semantics. A timeout or cancellation after dispatch is not a
safe-retry result. Resolve-token operations and secret-bearing results exist only behind private
module ports; they are neither gateway DTOs nor ordinary workflow outputs.

The integrations core has no dependency on Openbot login/session, company, or workflow storage.
Host adapters translate Openbot IDs and provide a required DispatchAuthority port. After refresh,
DNS, catalog discovery, and other waits, it rechecks persisted host authority against the exact
invocation immediately before business dispatch. No correctness operation gets a silently
successful default implementation. Versioned, bounded wire projections and conformance fixtures
in 07 prove the boundary without deploying RemoteIntegrationGateway in this release.

## Batch 01C — Ownership and binding authority

An authenticated owner manages their entries. Owner identity comes from the verified session,
never a user_id accepted from a request or workflow argument.

An owner may grant use to a named company connection only while both the owner and the acting
company administrator have the required current company authority. The ordinary path is an
owner who can edit that connection. If another administrator created it, the owner separately
confirms the binding; administrator access alone cannot supply this confirmation.

A UseGrant records the account and owner, trusted client/opaque host binding, app, approved
destination, account authorization revision, grant revision, and workflow-only use. The company
connection stores an opaque grant reference and its approved version. This narrow grant is the
owner-approved binding, not a second general ACL system. It authorizes that connection's registered
operations under existing company policy; explain this scope in the confirmation UI. Destination
changes, entry substitution, or broader execution exposure require fresh owner consent.
Company administrators never gain raw-secret reads.

At dispatch, check the binding, current owner membership, connection enablement, workflow/action
authority, and the entry's status. Runs use the recorded connection grant, not whichever user's
vault happens to match the run actor. Scheduled runs can use a valid explicit grant; leaving a
company revokes that company's use without deleting the owner's other entries.

Only actual deterministic HTTP/MCP workflow steps may use these new bindings. Derive this from
persisted execution kind and server-created action provenance. A model step invoking a generic
HTTP/MCP tool is a different entry path, even when it belongs to a workflow run, and is rejected.
An explicitly published workflow tool may invoke a workflow containing authorized steps.

## Batch 01D — Revisions, secrets, and private ports

Keep these version identities separate:

- Account revision orders cache and management writes.
- Authorization revision identifies consent, account, scope, and secret replacement.
- Grant revision identifies connection/app/destination delegation and revocation.
- Credential generation identifies authentication changes relevant to a live transport session.
- Existing connection/catalog revisions continue to identify destination and operation policy.

Routine access-token refresh and envelope rewrapping preserve authorization revision. Reconnect,
disconnect, owner secret replacement, binding changes, and removal increment the relevant
authorization/policy identity and invalidate stale authorization snapshots. Never put token
bytes or ciphertext into action digests, workflow definitions, approval artifacts, or task payloads.
Envelope-only rewrapping uses ciphertext CAS without changing the logical entry revision or
Google token generation; it must still lose to a newer logical writer.

Define private module ports for credential storage, provider authorization/renewal, and typed
injection. Their secret-bearing results are not serializable gateway data. Transport and
persistence implementations remain adapters. Neither a new provider nor a new app should require
a workflow-language variant or vendor branch in HTTP/MCP workflow handlers.

## Focused checks and acceptance

Add future test families integration_contract_, vault_contract_, and vault_authority_ for
provider/app/account separation, typed static/managed payloads, key/value/count bounds,
metadata serialization, legal states, revision behavior, and a matrix of owner, other user,
company admin, scheduled workflow, direct tool, and sealed sample requests. Include gateway
serialization/redaction, unknown registry IDs, cross-namespace IDs, workspace identities without
email, and denial by the host authority port. Fail closed on unsupported auth methods.

Batch checks: 01A uses integration_contract_ and vault_contract_; 01B uses integration_contract_;
01C uses vault_authority_; 01D uses both contract families and cargo check. List each new family
before its first execution and record nonzero counts and results per batch.

Run each filter after first listing its selected tests:

~~~sh
cargo test --locked --offline --lib integration_contract_ -- --list
cargo test --locked --offline --lib integration_contract_
cargo test --locked --offline --lib vault_contract_ -- --list
cargo test --locked --offline --lib vault_contract_
cargo test --locked --offline --lib vault_authority_ -- --list
cargo test --locked --offline --lib vault_authority_
cargo check --locked --offline --all-targets
~~~

Acceptance: types make user/entry/company identity swaps difficult; the authority matrix and
revision table have one implementation home; secrets cannot enter broad metadata or workflow
outputs. Record actual commands, counts, and results before starting 02.
