# 02 — Connected-account storage and vault management

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [01](01-contracts-and-ownership.md). Deliver module-owned account and encrypted
credential persistence, useful for static credentials before Google-specific behavior is added.

## Batch 02A — Persistence and envelope context

Add integration_accounts to migrations/20260817000000_init_schema.sql. Store the metadata and
encrypted credential fields from 01 in the same account row so credential refresh and account
status share one lock; CredentialStore remains a separate narrow interface, not necessarily
another table. Implement owner-namespace/subject/key uniqueness, bounded metadata, positive
revisions, supported auth-method/schema checks, and provider-defined authorization uniqueness.
For Google, enforce the owner/subject/client identity key through explicit constrained columns or
a canonical adapter-produced identity key; do not hard-code Google as the global account shape.
A disconnected account has no usable token envelope; a reconnect_required account may retain
its encrypted bundle for identity and diagnostics but can never supply a usable credential.

Use a distinct integration_account credential context bound to immutable owner namespace/subject,
account ID, provider/config identity, and auth method/schema. Do not substitute a user UUID into
a company credential context. Copying ciphertext to a different owner, account, provider, or
credential type must fail authentication. Retain the existing envelope format, key-version
handling, and fail-closed behavior.

Implement bounded owner-scoped metadata listing, creation, conditional replacement, disconnect,
and deletion. List/detail projections select neither envelope nor raw secret. Credential reads
belong to a separate narrow port. Count enforcement and creation must be one transaction, with
a real same-owner competing-create test at the cap.

Management writes use expected revisions. Return a typed conflict rather than silently replacing
a concurrent refresh or owner change. Generic editing is restricted to static credentials;
registered provider adapters exclusively write their own managed payloads. Every persisted JSON
decode is fallible and bounded. Reconnect preserves the account ID only under the identity and
revision rules in 04; provider/config/auth-method substitutions require a new account.

Keep account repositories and all credential SQL inside integrations persistence adapters.
Openbot routes, workflow persistence, and connection handlers consume ports and opaque references;
they do not join or select from integration tables. Owner namespace/subject are stable client
references, not a dependency on the users table. The trusted local adapter maps verified UserId
to the configured Openbot client namespace and owner subject. Company IDs belong to host grants,
not private account ownership. Do not infer authority from the existence of an account row.

AuthConfig/AppDefinition/AuthProvider registrations are code/config-owned in this release; do not
add an auth-config CRUD table or dynamic endpoint configuration. The use-grant table is added in
03 with internal account FKs. Any local host reference constraints are deployment details; no
module query or contract may depend on a cross-module join to enforce host authority.

## Batch 02B — Key lifecycle and deletion

Include vault rows in credential inventory/status, keyset pagination, rotation, convergence
reports, unavailable-key handling, and the operator runbook. Preserve existing rotation CAS:
a rotation based on old ciphertext must not overwrite an owner replacement or refresh.
Rewrapping does not change authorization revision or unnecessarily restart an MCP session.

Define deletion with use grants before exposing the destructive operation. Bound accounts must
first be disconnected and their grants invalidated/disabled in the same module transaction. Use
internal FKs to prevent dangling active grants. Stale host references must resolve as denied,
never fall back to connection-owned credentials.

02 defines the lifecycle and account-level denial; complete grant-aware destructive operations
and expose them for bound accounts only after 03 adds the grant schema. Until then, management
must not claim that company attachment is available. Include connect-attempt encryption/rotation
inventory when 04 adds its sensitive verifier persistence.

Openbot user/company/membership lifecycle adapters call explicit idempotent revocation and teardown
ports. User deletion removes that owner's accounts and grants; company deletion revokes only its
grants. A private account used by another company survives. Host membership/deletion state remains
part of the live dispatch guard, so failed cleanup cannot leave usable authority. Record durable
teardown intent before destructive host deletion and retain it until cleanup completes; exercise
restart/retry recovery, not only the successful request path. Keep this bounded lifecycle work
in the host's existing supervised execution facilities, not a detached task or refresh daemon.
Do not assume database cascades can implement the future remote lifecycle.

Do not add a separate service or background token-refresh worker. Reuse current PostgreSQL and
the credential cipher. Plan and regenerate SQLx metadata even when runtime queries generate no diff.

## Batch 02C — Owner management API and UI

Add authenticated owner routes and a small profile Vault/Connected apps section. Users can list
labels/status, create or replace static credentials, remove an account, and see which company
connections use it. Route handlers call IntegrationGateway management ports. Proposed
route names are implementation targets, not existing public API:

    GET    /ui/profile/vault
    POST   /ui/profile/vault
    PUT    /ui/profile/vault/{entry_id}
    DELETE /ui/profile/vault/{entry_id}

The entry_id in these UI routes identifies a connected account, including a static-secret
account; it does not expose a separate raw credential store. Connect routes in 04 use app/config
IDs resolved from the server registry. Do not expose provider endpoint URLs or client secrets as
editable browser fields.

Use existing session, CSRF/origin protection, form-error, and stale-revision conventions. Secret
inputs render blank. Responses contain metadata and configured/not-configured status only.
There is no browser endpoint for reading a saved raw secret. Reject foreign entry IDs without
revealing another owner's labels or accounts.

UI code offers replacement, not readback. Tests and diagnostics must not record submitted values.
The UI can identify this as server-encrypted storage; do not promise end-to-end encryption.

## Focused checks and acceptance

Add vault_storage_, vault_management_, and vault_rotation_ families. Cover SQL constraints,
create/update/read round trips, revision conflicts, owner substitution, malformed encrypted JSON,
account/company deletion, durable teardown restart/retry, duplicate managed identity, cap
competitors, and real rotation versus replacement/refresh races. Include key retirement,
different owner namespaces, workspace identities without email, and no-secret serialization/error
assertions. Verify host consumers cannot bypass account ports with direct integration-table reads.

Batch checks: 02A uses vault_storage_ and the envelope tests; 02B uses vault_rotation_ and lifecycle
cases in vault_storage_; 02C uses vault_management_ plus affected profile route/page tests. Apply
the migration/SQLx checks below for each SQL-bearing batch; do not run the full suite per batch.

On a freshly migrated disposable database, run:

~~~sh
SQLX_OFFLINE=false cargo sqlx migrate run
SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets
cargo test --locked --offline --lib vault_storage_
cargo test --locked --offline --lib vault_management_
cargo test --locked --offline --lib vault_rotation_
cargo test --locked --offline --lib credentials::envelope::tests
cargo test --locked --offline --lib credential_rotation
cargo check --locked --offline --all-targets
~~~

List each new filter first and record nonzero selected/executed counts. TEST_DATABASE_URL must
explicitly identify the disposable test database; see 07 for setup and isolation rules.

Acceptance: a user can save and replace a reusable secret, another user cannot inspect it, all
new encrypted rows participate in operator rotation, and deletion cannot leave usable dangling
bindings.
