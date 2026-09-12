# Guided team-member removal — shared instructions

Read this before any phase file.

Today, removing a team member demotes their principal (`kind` → `external`, `user_id` → `NULL`) and
deletes their `company_members` row. A DB trigger,
`principals_release_owned_tasks_on_demotion`, silently releases every task they still owned to
Unassigned so the demotion does not trip `background_tasks_owner_principal_fk`.

After this change, live work is **presented, decided once, and resolved explicitly** as part of the
removal, the way a task transfer already requires a mandatory handoff instruction rather than moving
work silently. The trigger stays, demoted from "the mechanism" to "the safety net".

The admin takes **one** decision — **who** takes over everything the departing member holds,
always a named person or agent — and one click applies it to every item and then removes them. The
mechanism underneath is per item: one version-fenced, idempotent, audited command per task and per
ask. Tasks are transferred; open asks are **re-asked at the same taker**, not cancelled.

| Phase | File | Goal |
|---|---|---|
| 1 | [`phase1.md`](phase1.md) | The audit-trail FK tolerates demotion; the trigger is documented as a fallback. No behaviour change for callers |
| 2 | [`phase2.md`](phase2.md) | The pre-check (`member_work_at_stake`), the guard, and the one-decision batch that applies a handover and then removes |
| 3 | [`phase3.md`](phase3.md) | The Team pane asks the one question and removes in one click |

Each phase compiles, is clippy- and fmt-clean, and is landable on its own.

The two product-owner corrections of 2026-09-12 — asks redirected rather than cancelled, and a
mandatory-taker selection — are folded into the phases they change (2 and 3) rather than added as a
phase of their own: they alter what those phases build, not what order anything lands in.

---

## Directives this plan implements

1. **Never delete a principal row.** Already true — removal only demotes `kind`. Every historical
   audit reference to that principal must keep working *forever*, including references written
   after the demotion. This is why the audit FK in Phase 1 gains `ON UPDATE CASCADE` instead of
   being dropped or made nullable.
2. **No silent auto-unassignment.** When someone being removed still has live work, the admin is
   shown it and must state where it goes. The removal does not proceed until nothing is left at
   stake — and if applying the decision fails on any item, it does not proceed at all.
3. **The taker is always somebody.** *(correction, 2026-09-12)* There is no "unassign all" option:
   the one selection names a person or an agent, and nothing else. Work with no eligible taker
   refuses the removal outright (`MemberWorkAtStake::NO_ELIGIBLE_OWNER`) rather than falling back
   to leaving it to nobody. A plain removal with **no** work at stake never reaches this selection
   and is completely unaffected.
4. **Open asks are redirected, not cancelled.** *(correction, 2026-09-12)* A question waiting on the
   departing person is re-asked at the chosen taker through a new
   `DelegationOperation::ReassignPersonTarget` — the old target superseded, a correlated new one
   created, both histories kept. Cancelling would silently drop a question somebody is waiting on.

## The investigation that shaped the scope (done 2026-09-12)

### Only three FKs can block a demotion

`kind` is part of exactly three foreign keys into `principals(company_id, id, kind)`
(every other FK is on `(company_id, id)` and is untouched by a `kind` change):

| FK | init-migration line | What it is | Treatment |
|---|---|---|---|
| `background_tasks_owner_principal_fk` | 6193 | live ownership | **Category A** — explicit resolution (Phase 2) |
| `delegation_control_commands_actor_fk` | 6417 | append-only audit trail | `ON UPDATE CASCADE` (Phase 1) |
| `task_outreaches_creator_fk` | 7329 | outreach provenance | **neither** — see below |

### Category C: `task_outreaches.created_by_principal_id` needs no mechanism

Two facts, both read out of the current tree:

- **The creator of an outreach is always an agent.**
  `task_outreaches_creator_shape_check` (init migration 3302) is
  `(created_by_principal_id IS NULL AND created_by_principal_kind IS NULL) OR
  (created_by_principal_id IS NOT NULL AND created_by_principal_kind = 'agent')`.
  A team member's principal is `kind = 'person'`, so **a person can never be an outreach creator**.
  Removing a member therefore cannot cross `task_outreaches_creator_fk` at all, and there is no
  "outreach this person created" for the pre-check to return.
- **The creator field is not pure provenance either.** `authorize`
  (`src/adapters/persistence/task/controls.rs:112-141`) computes
  `is_creator = actor.kind == "agent" && outreach.created_by_principal_id == actor
  && task.owner_principal_kind == "agent" && task.owner_principal_id == actor`, and
  `DelegationAuthority::OwningAgent` is exactly that conjunction. So the frozen creator id *is*
  compared literally — but only together with **current** task ownership. An agent that loses
  ownership of the parent task loses `OwningAgent` authority on its own outreaches immediately;
  it never keeps authority through provenance alone.

Consequences, and why the plan does neither of the two candidate treatments here:

- **No `DelegationOperation::ReassignOutreachOwner`.** There is nothing to reassign: authority
  already follows current task ownership, and the only principals in the column are agents, who
  are never demoted.
- **No `ON UPDATE CASCADE` on `task_outreaches_creator_fk` either.** Cascading would have to write
  `created_by_principal_kind = 'external'`, which the shape check above forbids — so the CASCADE
  would require widening "an outreach creator is an agent" into "an outreach creator is an agent or
  an external", weakening a real invariant to serve a case that cannot occur. The FK keeps
  `ON DELETE RESTRICT` and no `ON UPDATE` clause.

### Category B is *not* an outreach target naming a channel

`task_outreach_targets` names either an internal **channel** (`target_kind = 'internal_channel'`,
`internal_channel_id`) or an **external identity**
(`external_transport`/`external_namespace`/`external_subject`) — never a principal
(init migration 3252-3274). So:

- An *internal* target that a departing person is somehow behind is behind it through the **child
  task** that target spawned (`revoke_internal_child`, `controls.rs:229-297`, joins
  `background_tasks` by `source_message_uuid`). If that child task is owned by them and still
  running, it is already a **category A** row in the pre-check, and transferring it moves who is
  expected to answer. No separate item, no separate mechanism.
- An ask that names **the person** is an `external` target whose identity triple matches one of
  their `participant_identities` rows — which every person principal has
  (`create_person_principal_on`, `src/adapters/persistence/participant.rs:260`). That join is what
  category B in this plan actually selects.

`DelegationOperation::ReassignInternalTarget` is guarded to old targets of kind
`internal_channel` (`controls.rs`, the `kind != "internal_channel"` check in its `apply_operation`
arm), so it cannot reassign a person-addressed ask.

### Redirecting a person-addressed ask (correction, 2026-09-12)

Cancelling drops a question somebody is waiting on, so the flow now **redirects** instead.

**Investigation: can an agent be the new target?** No. `participant_identities.principal_id` is a
plain FK to `principals(company_id, id)`, so the *schema* permits an agent's rows — but nothing
ever writes one. `create_agent_principal_on` (`src/adapters/persistence/participant.rs`) inserts
into `principals` only; the two writers of `participant_identities` are
`create_person_principal_on` (a person's verified account mailbox) and
`resolve_or_create_external_identity_on` (an observed outsider, principal kind `external`). An
agent principal therefore has **no contact identity at all**, and a person-addressed outreach
target is matched purely on `(transport, namespace, subject)` — so there is nothing to re-address
the question to, and nothing that would route a reply back.

The alternative ("fold it into the agent's ordinary dispatch flow") is what
`ReassignInternalTarget` already is, and it names a **channel**, not an agent — an agent is reached
by mail to a channel it is assigned to. Turning "who takes this work over" into "and which channel
should the question go to" is a second decision, which the product directive forbids. Out of scope,
with no speculation invented in its place.

**Resolution.** The one shared candidate list is **narrowed to humans whenever any ask is at
stake** (`member_removal::takers_for`). An agent may still inherit tasks; it is simply not offered
while a person's question is in the batch. If that empties the list, the removal is refused — which
is the correct outcome of the mandatory-taker directive, not a gap.

**A new variant rather than a relaxed guard.** `DelegationOperation::ReassignPersonTarget
{ outreach_id, target_id, new_principal_id }` mirrors its sibling's shape — version-fenced,
idempotent by `command_id` + fingerprint, audited in `delegation_control_commands`, old target
`superseded`, new target created and correlated by `replaces_target_id` — but is its own variant
because every part of it is the other's mirror image rather than its parameter:

- the payload names a **principal**, which `new_channel_id` cannot express;
- the guard is inverted (`target_kind = 'external'` instead of `'internal_channel'`);
- the replacement's identity kind is `External`, not `InternalChannel`;
- `revoke_internal_child` has no analogue — an externally addressed question spawns no child task;
- "pick a different channel" becomes "pick a different person", compared on the identity triple.

`delegation_control_commands_operation_check` in the init migration gains
`'reassign_person_target'` so the audit row is a distinct, readable value. An `OwningAgent` may not
issue it: re-addressing a named person's question at another named person is a manager's decision,
and is listed beside `CancelOutreach`/`ProceedWithPartial`/`StopTask` in `authorize`.

Quorum follows for free: `tally_outreach_targets` counts only `active` and `responded`, so the
superseded target stops being able to close the outreach while keeping its request, its delivery
and any late reply it still draws.

### What "live work" means, precisely

- **Category A**: `background_tasks` with `owner_principal_id` = the member's principal and
  `status IN ('pending','processing','pending_approval','waiting_for_third_party_reply','dead_letter')`.
  `completed`, `stopped` and `failed` are done with; `dead_letter` is included because it is a row
  that still needs an owner to act (the attention feed lists it).
- **Category B**: `task_outreach_targets.status = 'active'` whose external identity is one of the
  member's `participant_identities`, on an outreach in
  `status IN ('waiting','timeout_pending_approval')` (the two states
  `CancelTarget` accepts, `controls.rs:626-634`) and a parent task that is not terminal
  (`task_is_terminal`, `controls.rs:783`).

## Decisions

- **One decision, not one per item.** The admin picks a single taker for everything at once, with
  one shared handoff instruction, and a single Remove click applies it. Delegated asks carry no
  *separate* decision: they are re-asked at the same person the tasks went to. What collapsed is
  the admin's decision; the commands underneath are still one per item.
- **The picker offers the intersection of the affected channels' candidates, narrowed to takers.**
  One choice has to be valid for every item it is applied to, so `member_work_at_stake` returns a
  single `owner_candidates` list: what each affected channel allows — a channel is affected by an
  owned task **or** by an ask whose asking task runs there — intersected, minus the departing
  principal, then `takers_for`-narrowed to humans when any ask is present. Pooling them instead
  would offer a candidate half the commands then refuse; a second picker would be a second
  decision. An empty intersection refuses the removal.
- **Versions are read at submission time, not at page-render time.** The batch re-runs the
  pre-check and fences each command on what it just read, so a pane left open for an hour does not
  submit stale `expected_version`s. Each command still gets its own fresh `command_id`.
- **Partial failure is never a partial removal.** Every item is attempted; failures are collected;
  if anything failed, the member stays on the team and the refusal names the items that are still
  theirs (bounded to the first `UnresolvedWork::MAX_NAMED`). Items that did succeed stay resolved,
  so a retry has strictly less to do — and the re-rendered pane, which re-reads the pre-check, is
  exactly the list of what is left. The alternative (remove anyway, skip the failures) is the
  silent unassignment this flow exists to prevent.
- **One removal endpoint in the UI.** `POST …/team/members/{user_id}/removal` carries the decision
  and does the removal. There are no per-item resolution endpoints, and no second UI route that
  removes somebody without saying where their work goes. The classic
  `/companies/{id}/invites` page still calls the bare `remove_company_team_member`, which still
  refuses while anything is at stake.
- **The pre-check is a read; the resolutions are commands of the existing shape.** Category A
  resolves through `TaskOwnershipOperation::Transfer` via `change_task_ownership_on`
  (`src/adapters/persistence/task/ownership.rs`), category B through
  `DelegationOperation::ReassignPersonTarget` via `execute_delegation_command_on`
  (`src/adapters/persistence/task/controls.rs`). Both are version-fenced (`expected_version`),
  idempotent (`command_id` + fingerprint) and audited (`task_ownership_events`,
  `delegation_control_commands`). The redirect is the one genuinely new mutation, and it was built
  as its sibling's mirror rather than as a new pattern.
- **The guard lives in the use case, not in the route.** `remove_company_team_member` runs the
  pre-check itself and refuses with `AppError::Conflict` while anything is at stake, so a future
  caller (a JSON route, a script) cannot walk past it. The batch
  (`remove_company_team_member_with_handover`) finishes by calling *that* method, so the guard also
  re-checks what the batch believes it cleared.
- **The admin acts with the authority they already have.** Transfers and releases go out as
  `TaskOwnershipAuthority::Manager`, cancellations as `DelegationAuthority::CompanyManager`; both
  are satisfied by an owner/admin `person` principal, and neither needed a new authority.
- **Reason codes reuse the existing vocabulary.** `TaskOwnershipReason::OwnerUnavailable` for a
  removal-driven transfer, `DelegationReason::TargetUnavailable` for a redirected ask. The only new
  enum variant anywhere is `DelegationOperation::ReassignPersonTarget` itself.
- **The trigger stays as a safety net.** It is the last line for a code path that reaches the
  demotion without the pre-check — a `remove_member` called directly in a test, or a future caller
  that forgets. Its new comment in the init migration says exactly that.
- **Bounded.** The pre-check lists at most `MemberWorkAtStake::MAX_PER_KIND` (50) items per
  category and reports `truncated`; a truncated result still blocks removal, so the bound cannot
  turn into a way past the guard. One submission therefore resolves at most 50 of each kind: the
  final guard then refuses the removal, the pane re-renders with the next batch, and clicking
  Remove again continues. The pane says that where it reports the truncation.

## Rules every phase inherits

- Schema changes edit `migrations/20260817000000_init_schema.sql` **in place** — no additive
  migrations — then recreate both databases: `./scripts/reset-db.sh --all`.
- After any query change:
  `DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo sqlx prepare -- --all-targets`.
- DB tests share one database and run in parallel. Scope every assertion to ids the test created;
  never assert a whole-table count.
- `cargo build`, `cargo clippy --lib --all-targets`, `cargo fmt` clean at the end of each phase,
  and the phase's own tests run narrowly.
