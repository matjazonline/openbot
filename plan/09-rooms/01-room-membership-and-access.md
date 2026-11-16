# 01 Room ownership, membership and access

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A unit manager can create a room and choose its people and agents. The roster, ownership and
access checks agree in one commit. Continues in [02](02-mentions-and-addressed-messages.md).

## Batch 01A Channel kind and roster

1. Extend the init schema's `channels` and `Channel` with a kind, `desk` or `room`. Keep the
   non-null owning unit supplied by 04. Kind and owning unit are fixed for a room after creation;
   moving a conversation's scope and billing owner is not a room settings operation in this plan.
   Existing desk writes retain their rules. Address uniqueness is still 03's company namespace.
2. Add `room_members (company_id, channel_id, principal_id, created_by, created_at)` with composite
   tenant foreign keys and one row per room/principal. Guards require a room and an active company
   person or agent; external/system principals are refused. A person must have current root
   membership, including its live membership-instance checks from 03. Bound the roster to 64
   members and its agents to the existing `MAX_MESSAGE_BINDINGS_PER_CHANNEL` (currently 8).
3. The roster is the sole source of membership. Materialize `view` and `participate` grant rows
   with a new `room_membership` provenance in the same write, using the existing grant machinery.
   Refuse direct configuration of other grant provenances on rooms. Never infer membership from
   old email allowlists. Database guards protect kind, tenant, count and grant consistency when a
   caller bypasses the use case; deletion of a roster entry revokes its grants atomically.
4. Extend department-grant synchronization from 04 to select desks only. Room access mode is
   `members`, with external participant expansion disabled. Refuse public-room and allowlist
   writes through generic channel APIs too. The unit's members and adjacent leads do not receive
   room grants. A creator receives membership only by being included in the creation command.

Focused checks: `room_schema_` for kind/tenant/principal/limit guards and reset creation;
`room_grants_` for exact roster-derived grants, department edits and bypassed writes. Keep the
existing desk policy matrix in the focused run to prove its grants still work.

## Batch 01B Policy, management and revocation

1. Extend the existing channel policy owner with a kind-aware decision; routes, ingress, histories,
   files, search, SSE and notifications call it. For a person, current company membership plus a
   room grant is required; the company owner retains the existing exemption. Agent participation
   requires its own live room grant and runtime authority. A room manager sees configuration but
   cannot read messages without the same read authority.
2. Creation, roster edits, settings and deletion use 06's `manages_unit` on the owning unit,
   including any sponsor authority actually implemented by 08. Agent actors do not administer.
   Use ordinary channel use cases/ports with room-specific commands rather than HTTP-only guards.
   Audit roster and settings changes through the existing administration audit path.
3. Membership changes bump `settings_revision`. Serialize admission and publication against the
   same settings/authority gate. Removing a participant revokes pending notices; removing an
   agent disables its room binding and cancels its queued/active turns through the mechanisms
   added in 03. Removing a human also stops discussions initiated under that membership. Account
   removal and principal retirement use this path; rejoin never revives old work or old notices.
4. Trace the actual operation-specific account, company, principal and channel lock order before
   adding room writes. Preserve 03's identity and account prelocks; no model/provider work runs
   inside those transactions. A policy race either commits under the old authority or refuses,
   with publication rechecking current authority after provider work.
5. New members may read retained history within their content-level ceiling. Membership does not
   raise a level or widen a message's audience. Member removal ends future access; already received
   content cannot be recalled. Deletion remains conservative when messages, runs or bindings are
   retained, and unit/account/company teardown must include room-owned rows.

Focused checks: `room_authorization_` covers people, agent principals, owner, non-member managers,
foreign tenants and every read surface; `room_membership_competing_` races add/remove with ingress,
notification delivery and turn admission; `room_retirement_` covers removal/rejoin and teardown.
Run affected authorization families after both batches, using the README's nonzero-count protocol.
