# Vault Google p01 — contracts and ownership

- Workflow: implement-astra-reviewed-2-2.
- Original requirement: implement ../../../../01-contracts-and-ownership.md, then p02, then stop.
- Record root: /Users/mac02/.codex/worktrees/1133/openbot/plan/vault-google/phases/p01/docs.
- Root/A: /root, gpt-6.1-sol / xhigh confirmed by user. B: pending, requested gpt-6-astra / high, independent context.
- Worktree: /Users/mac02/.codex/worktrees/1133/openbot. Detached HEAD.
- Starting commit / review base / current HEAD: 769a23aa6ef423e964f8221a8ee6b0743325c6ba.
- Preserved pre-existing changes: none (clean startup).
- Coordination authority: /Users/mac02/.codex/worktrees/1133/openbot/plan/vault-google/COORDINATION.md; checked revision 1; VG-I01/I02/I03 r1 acknowledged. No cross-phase notices or requests.
- Exclusions (fixed for all snapshots): plan/vault-google/COORDINATION.md; plan/vault-google/phases/p01/docs; plan/vault-google/phases/p02/docs. Only workflow records excluded.
- Prerequisites: existing credential/transport/action contracts at starting commit. No other phase artifact required.
- Owned paths: new domain/integrations and application/integrations modules and registration; p01 contract tests. No transport/storage behavior changes.
- Resources: same checkout; checks serialized; no test database needed for p01. Host context occupancy/75% compaction controls unavailable; defaults with durable records.
- Status: architecture gate pending; implementation not started; not accepted or integrated. Current digest pending.

## Batch B1 — entire phase (01A–01D)

Acceptance is the original phase plan: distinct provider/app/config/account/owner/grant identities, bounded typed payloads and metadata, management vs execution vs private credentials, explicit host authority and deterministic-step provenance, revision semantics, fail-closed registry/auth.

### Architecture gate VG-P01-D1 / r1

1. Domain types live in domain::integrations (no application/framework imports). Application-owned IntegrationGateway is a composition of ManagementGateway, ConnectGateway, GrantGateway, ExecutionGateway. LocalIntegrationGateway composes injected implementations; each correctness method is required. Later-stage features are interfaces only in p01, with explicit unavailable implementations only where p02 composition requires them.
2. Public metadata has no secret/envelope field. Secret payload wrappers use secrecy::SecretString and deliberately do not implement Serialize; errors are fixed enums without submitted material. Validated identifiers deserialize through their constructors. Account/config/provider/auth-method/schema identity is immutable. Registry implementations own payload validation/canonical authorization identity; core registry dispatch has no Google enum branch. Static bearer and fixed registered API-key header adapters explicitly accept string or supported JSON shapes. Managed payload writes are separate private operations, never generic JSON management input.
3. Bounds: IDs/key 128 bytes; owner/external components 256; decrypted credentials 16384 bytes only for the future vault context; metadata 4096 bytes with a typed allowlisted shape; 64 scopes × 256 bytes, aggregate credential bound still applies; 128 accounts/owner and grants/account; list max 50. UTF-8 bytes are the measure. Unknown/auth/schema mismatch rejects.
4. AccountRevision, AuthorizationRevision, GrantRevision, CredentialGeneration are distinct positive version types. One revision transition table drives owner replacement/reconnect/disconnect, refresh, rewrap, and grant change semantics. Rewrap preserves all logical versions; routine refresh increments account revision and generation, not authorization revision. Disconnect denies credentials and increments authorization; grant invalidation is checked via account authorization revision.
5. Owner management accepts an opaque verified client/owner context, constructed at the trusted adapter boundary, never deserialized from browser/workflow arguments. Grant consent requires owner confirmation and current owner+administrator authority from a required host authority port. Dispatch carries typed snapshots and bounded operation arguments, digest/correlation/dispatch IDs, deadline, cancellation identity, and opaque host reference. Local cancellation is separate. No credentials, SQL, FrozenAction, sessions, or framework types in public DTOs. Operation inputs reject authorization headers/injection instructions.
6. One pure authority decision checks owner/client/account/grant/binding/app/destination/revisions/status/workflow-only deterministic provenance; current persisted host facts arrive from required DispatchAuthority immediately before business dispatch in later execution. No actual execution, Google OAuth, grants schema, or workflow integration in this phase. A scheduled deterministic step with explicit grant may pass; direct/model tools and sealed samples deny.

Failure modes: unknown registry IDs/method/schema, owner/client swaps, stale versions, unsupported secret shape, count/size excess, missing owner confirmation/current authority, disconnected/reconnect-required credentials and post-dispatch cancellation all fail closed with sanitized typed results. Public execution outcome distinguishes pre-dispatch denial from committed/uncertain; cancellation after dispatch cannot imply safe retry.

Verification: list each new family first, then nonzero integration_contract_, vault_contract_, vault_authority_ tests. Cover static/managed adapter validation, workspace identity without email, namespaces, metadata serialization, scope+aggregate boundaries, revision table, authority matrix and denial port. Run cargo check --locked --offline --all-targets and fmt. Full CI suite deferred per explicitly selected workflow, ask at completion.

## Evidence and decisions

- Repository graph consulted before sources; original plans and applicable root/src/application guides read.
- Final focused checks, immutable artifact identity, review note IDs and acceptance: pending.
