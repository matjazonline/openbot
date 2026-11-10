# Plan coordination: vault-google

- Authority: /Users/mac02/.codex/worktrees/1133/openbot/plan/vault-google/COORDINATION.md
- Writer: /root, worktree /Users/mac02/.codex/worktrees/1133/openbot
- Revision: 5

## Plan-wide instructions

- VG-I01 / r1: Implement p01, accept it, then implement p02 and stop. No phases 03–07 implementation. No compatibility work, commits, deployment, or production resets.
- VG-I01 / r2: User now authorizes reconciling pushed channel p05, then implementing vault p03, p04 and p05 sequentially with independent acceptance. Stop after p05 and log next steps. Supersedes r1 stop-after02 scope; no p06–p07 implementation, commits, push, deployment or production resets.
- VG-I02 / r1: User confirmed root is gpt-6.1-sol / xhigh. Root authors; independent B uses gpt-6-astra / high with no inherited history.
- VG-I03 / r1 (superseded by r2): Focused checks during implementation. The explicitly selected workflow requests asking before the full suite; retain the full-suite gate as pending until requested.
- VG-I03 / r2: Focused checks per phase. The user-supplied repository AGENTS.md explicitly requires the full verification suite once the entire authorized task is implemented, including formatting, offline compilation, migrations, and database-backed tests. That explicit repository rule overrides the selected skill's default ask-only completion policy. Run the full gate once after03–05, recording failures/unavailable checks; no full suite after each phase. This corrects root's earlier policy interpretation and adds no source/runtime requirement.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | /root | /Users/mac02/.codex/worktrees/1133/openbot | phases/p01/docs/TASK.md; phases/p01/docs/HANDOFF.md |
| p02 | /root, sequential after p01 acceptance | same worktree | phases/p02/docs/TASK.md; phases/p02/docs/HANDOFF.md |

| integration/channel-p05 | /root | same worktree | integration/channel-p05/docs/TASK.md; HANDOFF.md |
| p03 | /root | same worktree | phases/p03/docs/TASK.md; HANDOFF.md |
| p04 | /root, sequential after03 | same worktree | phases/p04/docs/TASK.md; HANDOFF.md |
| p05 | /root, sequential after04 | same worktree | phases/p05/docs/TASK.md; HANDOFF.md |

## Cross-phase notices

### VG-C001 / r1

- State: active
- Origin: p02, /root; phases/p02/docs/TASK.md D1/r3 and VG-P02-F4.
- Targets: p04, p05, p07 (future phases; no implementation authorized by this notice).
- Instruction/change: registered CredentialAdapter implementations must explicitly validate routine cache updates through the required validate_cache_update port. Provider identity and permission changes require the explicit consent/reconnect authorization transition; stable token/cache updates preserve authorization revision. Core storage has no vendor-specific permission branch or secret fingerprint. Actual provider renewal must also satisfy05's rotating-token/ambiguous-outcome protocol; a local write denial does not make an external refresh safe to repeat.
- Dependencies/order: accepted p02 producer artifact e74fd0b7bb6eeec83c21a0da777a92f8f51f2f102894b517b312b10fd87fe601, before adding provider/renewal adapters.
- Evidence/review: VG-P02-D1/r3, architecture PASS VG-P02-R4, implementation PASS VG-P02-R5; private.rs, google_credentials.rs, integrations/accounts.rs and cache_update_tests.rs.
- Verification: permission drift denies or uses explicit renewed consent; routine cache-only changes preserve authorization revision; compare facts under the existing credential lock; provider-specific and real storage tests required.
- Responses: p02 producer satisfied (phases/p02/docs/TASK.md acceptance); p04/p05/p07 pending registration. Published after producer review; no target phase has started.

### VG-C002 / r1

- State: active
- Origin: p02, /root; phases/p02/docs/TASK.md D1/r2 and original02B deferral.
- Targets: p03 (future phase; this notice does not authorize starting it).
- Instruction/change: current company/membership teardown completes the no-binding lifecycle because grant creation is explicitly unavailable. Before enabling attachment, extend account deletion, host lifecycle hooks and the per-intent module transaction with grant invalidation and internal reference constraints. Preserve unrelated private accounts and keep stale host references denied. Do not enable grants while retaining the current no-binding cases unchanged.
- Dependencies/order: accepted p02 artifact above, before company attachment is enabled.
- Evidence/review: original02B, VG-P02-D1/r2, VG-P02-R3/R5 whole-batch and correction coverage; integrations/lifecycle.rs and accounts.rs.
- Verification: bound-account/company/member deletion and retry/restart cannot leave usable grants; unrelated private accounts survive. Extend the existing lifecycle/cap/rotation test families as relevant.
- Responses: p02 producer satisfied for its explicitly unbound scope (TASK.md acceptance); p03 pending registration. No p03 implementation started.


### VG-C003 / r1

- State: active after VG-P03-AG1-R1 PASS of D1/r2 SHA a33eb50f9ae21e674f8bd20edb989032400ade440a79df70635c42b8db21e15b.
- Origin: p03 /root, phases/p03/docs/TASK.md and immutable D1/r2.
- Targets: p03, p04, p05, p06, p07. No expansion beyond user-authorized03–05.
- Contract: exact owner consent and workflow-only grants; authorization snapshot in frozen actions excludes payload/cache revision. Required host-first authority callback composes short persisted host locks then module account/grant checks before business dispatch. Existing synchronous user retirement takes host then module locks; never hold module locks while entering host authority, and never hold host locks during refresh/discovery. Resolver releases account locks before final gate. Opaque refs require exact persisted action/fence/approval/provenance checks and cannot be used by Agent/ChannelFinal/sample origins.
- Lifecycle: pending/failed teardown denies stale grants; late cleanup and removal/readdition cannot invalidate later fresh consent or revive old grants. Owner revocation independent of company admin.
- Producer: architecture accepted only; implementation pending. p03 acknowledges/r1. p04/p05/p06/p07 pending accepted producer.
- Verification: real competing revocation/refresh/deletion tests, source-kind and stale approval checks; original phase03 requirements remain binding.
