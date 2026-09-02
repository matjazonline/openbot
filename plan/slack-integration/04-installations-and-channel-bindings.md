# Step 4 — Add Installations and Channel Bindings

## Outcome

Make transport interfaces first-class. A company can install a provider account, and each business
channel can expose one or more independently enabled bindings without adding protocol fields to the
`channels` table.

## Additive migration

Create `migrations/20260902010000_add_installations_and_bindings.sql` with:

### `integration_installations`

- `id`, `company_id`, `transport`, `external_tenant_key`, display name, status, granted scopes,
  installed/updated/revoked actor and timestamps.
- Status values `active`, `reauthorization_required`, `revoked`, and `disabled` with a check.
- `UNIQUE (transport, external_tenant_key)` for v1 so one external workspace cannot bridge two app
  companies accidentally. Revisit only with a documented multi-tenant workspace threat model.
- Composite `UNIQUE` keys needed by tenant-scoped child FKs.

### `integration_credentials`

- One narrow table keyed by `(company_id, installation_id, credential_kind)` containing only an
  encrypted envelope and timestamps.
- No token appears on the broad installation entity, list query, debug output, task payload, or
  `AppState`.
- Do not put Slack tokens through the current direct-master-key/AAD-empty format. Either complete
  the preferred KMS-backed or bounded local-envelope decision in
  `plan/security/improve-key-credentials.md` first, or implement that same per-credential DEK
  contract as part of this step. Bind authenticated encryption to company, installation,
  transport, and credential kind; do not reuse empty associated data.
- Extend the credential status/rotation inventory to this table only after it can parse, validate,
  and rewrap the selected envelope format without returning secret material.

### `channel_bindings`

- `id`, `company_id`, `channel_id`, nullable `installation_id`, `transport`, namespace,
  `external_endpoint_key`, display label, access policy, delivery policy, status, disabled reason,
  creator, audit snapshot, and timestamps.
- Email deployment bindings may have no external installation; all Slack bindings must reference an
  active Slack installation. Encode this coherence as a database check.
- Composite FKs prove channel and installation belong to `company_id`.
- Unique active endpoint rules prevent two channels in the same installation from consuming the
  same conversation. Use separate partial unique indexes for installed and deployment transports
  so SQL `NULL` semantics cannot bypass uniqueness.
- Status values `active`, `paused`, `disabled`, and `orphaned`; access/delivery policies are checked
  enums rather than arbitrary JSON strings.

### `binding_audit_events`

- Append-only actor, action, reason, bounded metadata, and timestamp records for link, enable,
  pause, disable, drift, and unlink operations.
- Metadata contains safe IDs and the confirmed access-policy snapshot, never credentials or full
  provider responses.

## Domain and ports

- Add `IntegrationInstallation`, `InstallationStatus`, `ChannelBinding`, `BindingStatus`,
  `BindingAccessPolicy`, and `BindingDeliveryPolicy` to `src/domain/entities/transport.rs`.
- Define application-owned `InstallationPersistence`, `InstallationCredentialStore`, and
  `ChannelBindingPersistence` ports close to the integration/channel use cases.
- Implement narrow SQLx adapters in `src/adapters/persistence/integration.rs`. Credential reads
  return `SecretString` and require exact company/installation/kind scope.
- Add queries for active bindings by channel, exact inbound endpoint lookup, and manager-scoped
  list/detail views. All caller-supplied IDs include company predicates.

## Email backfill

- Create one active email binding per existing channel representing its canonical outbound
  interface; aliases remain adapter routing keys that resolve to that binding.
- Store the resolved canonical inbound address as the endpoint key. Do not remove `channel_slugs`;
  they remain product-level selectors and email aliases.
- Set email binding access policy to `channel_acl` and delivery policy to the behavior currently
  implied by email reply/outreach code.

## Tests

- Cross-company installation/binding/credential inserts fail at the database boundary.
- Two channels cannot actively bind the same installed endpoint.
- Disabling a binding leaves channel/thread/message rows intact and excludes it from ingress and
  delivery queries.
- Generic installation/list projections cannot select ciphertext.
- Credential rotation/status includes integration credentials in bounded batches.
- Email binding backfill creates exactly one binding per channel and is idempotent.

## Acceptance criteria

- A business channel supports zero-to-many protocol interfaces without a new nullable column per
  provider.
- Secret access is narrow, encrypted, tenant-scoped, and covered by the existing rotation process.
- “Encrypted” means the repository's selected envelope-encryption design; the existing `enc:v1`
  pass-through-compatible format is not accepted for new integration credentials.
- Every binding lifecycle mutation is audited and every active-binding invariant is enforced by
  the database as well as the use case.
