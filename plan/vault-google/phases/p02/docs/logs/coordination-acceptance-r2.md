# Plan coordination: vault-google

- Authority: /Users/mac02/.codex/worktrees/1133/openbot/plan/vault-google/COORDINATION.md
- Writer: /root, worktree /Users/mac02/.codex/worktrees/1133/openbot
- Revision: 2

## Plan-wide instructions

- VG-I01 / r1: Implement p01, accept it, then implement p02 and stop. No phases 03–07 implementation. No compatibility work, commits, deployment, or production resets.
- VG-I02 / r1: User confirmed root is gpt-6.1-sol / xhigh. Root authors; independent B uses gpt-6-astra / high with no inherited history.
- VG-I03 / r1: Focused checks during implementation. The explicitly selected workflow requests asking before the full suite; retain the full-suite gate as pending until requested.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | /root | /Users/mac02/.codex/worktrees/1133/openbot | phases/p01/docs/TASK.md; phases/p01/docs/HANDOFF.md |
| p02 | /root, sequential after p01 acceptance | same worktree | phases/p02/docs/TASK.md; phases/p02/docs/HANDOFF.md |

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

