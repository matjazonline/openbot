# AH-P01-D1 / r3 — creator provenance must not obstruct principal revocation

Architecture gate reopened for a contradicted lifecycle assumption. All D1/r2 decisions remain except this precise principal lifetime representation. Original phase01 requires current stream rights, and D1/r2 explicitly prohibits tying retained session identity to ephemeral membership in a way that prevents revocation. No dependent correction implemented until independent gatePASS.

## Actual contradictory evidence

The complete targeted affected gate (`history-and-affected-runtime-05.json/.log`, selected164) reports161PASS/3FAIL. All28history testsPASS, including real9→0→9 audience identity. Three existing native revocation cases fail23503 at commit on `agent_history_sessions_company_id_creator_principal_id_fkey`:

- `workflow_action_authority_revocation_contender_blocks_then_rechecks`, action_tests.rs213–242: a genuine revoker deletes a principal while holding company lock, then current authority must refuse after it commits.
- `workflow_action_authority_sql_scope_actor_and_current_membership`, uses persistence/test_support.rs577–625 membership revocation helper.
- `workflow_admit_sql_actor_revocation_blocks_new_replay_and_alias`, admission_authority_tests.rs52–65, genuine commit blocks new admission/replay/source alias.

The existing production member-removal path can convert a principal to external and retain its UUID, but the existing schema and authority protocol also support a principal's actual deletion, including membership cascade. The new required FK fails at commit and prevents those revocations. This is a regression from our source; tests/authority/removal behavior must not be weakened or changed to suit history.

## Narrow corrected representation

`agent_history_sessions` retains two distinct facts:

1. `creator_principal_id` stays an immutable NOT NULL historical principal UUID snapshot, proven at initial creation from the genuine root actor/admitted event. It is not a live FK that can pin deletion. `creator_user_id`, tenant, session UUID and association remain immutable and retain actual actor provenance.
2. `live_creator_principal_id` is a nullable tenant-scoped `(company_id, principal_id)` FK to principals, initially exactly equal to that snapshot. `ON DELETE SET NULL (live_creator_principal_id)` allows actual principal deletion without deleting session, conversations, root mappings, future exchanges, or queued/active pins. No automatic reassignment to a later principal; no legacy/backfill.

Native insert guard requires BOTH creator principal fields equal the currently genuine person principal/user for the admitted root. A narrowly guarded UPDATE permits only clearing the non-null live link when the old referenced principal actually no longer exists in that tenant, with every immutable snapshot/association/time/identity field unchanged. All other updates still fail23514. Manual clearing while the principal exists and retargeting to a different principal both fail. This allows the FK's actual cascade and genuine company teardown without turning a general update into a relationship override.

Current access continues through actual persisted execution actor, current account/company membership and exact association/agent authority. Historical creator UUIDs grant no authority. Session reuse still resolves only the same creator/caller/tenant/exact association under normal authenticated manual admission. Principal deletion revokes current use; retaining its audit identity does not preserve membership or a capability. Native FK clearing does not alter stream/session identities, creator snapshots or content stamps, and cannot discard failed/active input audit. No new conversation tables, lifecycle queues, public API or override mode.

## Correction verification

Reset only owned p01 DB pair after schema change, fresh migrations, actual SQLx prepare/check. Add focused native session proof: real member/principal revocation commits, historical principal/user/session and default mapping survive with null live link, current admission/identity use is refused, direct live-link clearing/retargeting and creator-snapshot changes refuse. Exercise public application revocation path and native deletion as appropriate. Rerun all164 targeted affected tests (especially the three original failed current-authority cases), plus the new focused tests, all-target offline compile/Clippy/fmt. No full suite or bound increase. Then whole phase01 independent code review; this architecture gate is not code acceptance.
