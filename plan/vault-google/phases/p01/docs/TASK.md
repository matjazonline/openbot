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

- Architecture snapshot: base 769a23aa6ef423e964f8221a8ee6b0743325c6ba; digest 4868f6c61a7b3620e19ecddffc2d982d1f51f95c1d206e6fb7f39b8ac3b5b707. B handle /root/reviewer (gpt-6-astra/high requested, no history). Original snapshot script requires GNU realpath; native macOS invocation failed before hashing. Subsequent invocations prepend /tmp/vault-google-tools (a narrow realpath executable for the script's exact flags); the skill and script remain unchanged. Reviewer uses the same environment.
- Baseline cargo check --locked --offline --all-targets started; logs/baseline-check.log.

## Implementation checkpoint

- VG-P01-D1/r1 passed by B, note VG-P01-R1; immutable decision/coordination evidence under logs. Source implementation underway.
- Added domain/integrations (validated identifiers, metadata, identities, revision table, grant/dispatch decisions) and application/integrations (gateway subports, local composition, bounded invocation, required host authority, narrow private credential/provider/injection ports and code-owned registry).
- Added current typed Google payload validator without network/consent/login behavior or production registration. Static registry supports bearer and a code-owned X-API-Key header. Managed inputs cannot pass through static editing.
- New test families: integration_contract_ (5 tests), vault_contract_ (4), vault_authority_ (4). List/run commands queued; actual results pending. Baseline offline compilation passed (logs/baseline-check.log).

## B1 verification and frozen review input

- Corrected actual family counts: integration_contract_ 4, vault_contract_ 5, vault_authority_ 4. Each was listed before running; all 13 passed with zero failures/ignored tests. Exact commands and output: logs/{integration_contract_,vault_contract_,vault_authority_}{list,run}.log.
- Initial list/build failed on a newtype mismatch in a fixture (CredentialSchema vs ClientSecretReference); fixed the fixture. Retained failure: logs/integration_contract_list-failed-type.log. Successful rerun covers the correction.
- cargo check --locked --offline --all-targets passed before the final bounded-type refinements (logs/check.log); final rerun in progress at review boundary (logs/check-final.log), required before acceptance. Private contracts for later phases emit dead-code warnings; no successful-default operations added.
- cargo fmt --all -- --check, git diff --check, graft build performed; final fmt/whitespace logs follow final check. Graph outputs are local derived cache.
- Frozen artifact: base/review-base/HEAD 769a23aa6ef423e964f8221a8ee6b0743325c6ba; digest bd10fd0dc5972fa61a1977d7322c7eb9507ffa94059ba97893f6de050785d6a0. Fixed exclusions unchanged. Source freeze now active. Coordination revision1 and instructions VG-I01/I02/I03 r1 rechecked; no notices.
- Full CI-equivalent suite command: scripts/check.sh, deferred for the completion question per user-selected workflow.
- Status: implemented; focused tests passed; awaiting independent B1 batch review and final compile result. Not yet accepted/integrated. No p02 implementation started.

## B1 corrections after VG-P01-R2

- B reviewed the whole frozen artifact and saved VG-P01-F1/F2 (both P2). Source thawed only after the full findings were saved.
- F1: static typed-shape detection now recognizes leading JSON whitespace, parses the original value, and validates exact supported shape. Plain strings still follow the adapter's bounded header-value rules.
- F2: operation headers now use an allowlist of ordinary content/conditional headers, shared with static adapter registration so an adapter cannot claim one of those fields for secret injection. X-API-Key and all other adapter-owned headers are denied; nested injection fields remain denied.
- Added two regression tests (integration_contract_ now6; vault_contract_5; vault_authority_4). List+run correction logs pending; rerun all15 focused contract tests together.
- Final previous-artifact alltargets compilation, fmt and whitespace passed (logs/check-final.log, fmt-final.log, diff-check.log). Current correction artifact requires affected tests/compilation and B recheck; not accepted yet.

- Correction results: listed and executed integration_contract_6, vault_contract_5, vault_authority_4; all15 passed, no failures/ignored. Logs: *_correction-{list,run}.log. fmt/whitespace/graft refresh pass. Correction digest48c978f0dbbcaf84db7b4901eac45b6aad1efd33a66f54ed65aef0c83f64de81 (same base/HEAD/exclusions); source frozen for F1/F2 recheck. Alltargets offline compilation rerun queued for this exact artifact. Coordination revision1 still current; no changed notices.

## p01 acceptance

Accepted in phase and integrated in this sequential working tree at 48c978f0dbbcaf84db7b4901eac45b6aad1efd33a66f54ed65aef0c83f64de81. Root rehashed the artifact and rechecked coordination r1; VG-I01/I02/I03 r1 satisfied for current scope. Independent PASS VG-P01-R3; VG-P01-R1 architecture PASS, VG-P01-R2 whole-batch findings F1/F2, both closed by R3. All15 focused tests, exact-artifact alltargets offline compile, fmt, whitespace and graph refresh passed. No open findings. Full scripts/check.sh is pending user completion question, so this is focused verification, not full-suite verification. Immutable prerequisite sources retained in logs/accepted-source.tar.gz plus tracked patch and snapshot-accepted.log. p01 source freeze released for authorized p02; historical acceptance applies to this exact artifact. Next: p02 architecture gate; stop after p02.
