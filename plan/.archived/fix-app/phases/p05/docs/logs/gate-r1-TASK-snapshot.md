# fix-app p05 — Tenant integrity: TASK

## Identity

- Phase ID: `p05`; plan `plan/fix-app/05-tenant-integrity.md` (+ `plan/fix-app/README.md`).
- Workflow: `implement-opus-reviewed-2_2` (skill dir
  `/Users/mac03/dev/openbot/.claude/skills/implement-opus-reviewed-2_2`).
- Record root: `/Users/mac03/dev/openbot/.worktrees/fix-app-p05/plan/fix-app/phases/p05/docs/`.
- Root = Author (A): Opus 5.5 at `high` effort (set by the manager at spawn; confirmed in the
  assignment). Reviewer (B): Opus 5.5 / `xhigh`, general-purpose subagent, agent ID recorded
  below once spawned.
- Worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p05`, branch `fix-app/p05`, starting commit
  `e6b0a51` (main). No pre-existing changes in the worktree at start.
- Resources (owned by the manager, used by this phase only): Postgres 18 at `127.0.0.1:55405`
  (trust auth), dev DB `mail_agents`, test DB `mail_agents_test`; `.env` sets `DATABASE_URL`.
  Build dir: worktree `target/`. psql/createdb in `/opt/homebrew/opt/postgresql@18/bin`.
- Context: `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75` is set in the environment (verified with
  `printenv`).
- Snapshot exclusions (exact, reused for A/B/root): `plan/fix-app/phases/p05/docs`.

## Coordination

- Authority: `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md` (writer: manager root
  session). Last checked revision: 1.
- Applicable notices: `fix-app-I001 r1` (repo/plan rules: AGENTS.md, no backward compatibility,
  edit only named init-schema statements in place, final gate reset-db + check.sh) —
  acknowledged; `fix-app-I002 r1` (isolation: own worktree, cluster 55405, own target dir) —
  acknowledged; `fix-app-I003 r1` (commit accepted phase on `fix-app/p05`, no COORDINATION.md,
  no push) — acknowledged. No cross-phase notices target p05.
- Pending cross-phase requests: see "Cross-phase notices requested" below.

## Requirements (original)

`plan/fix-app/05-tenant-integrity.md`, batches 1-5, plus its focused checks and final gate.
Applicable instructions: `AGENTS.md`, `src/AGENTS.md`, `src/adapters/persistence/AGENTS.md`,
`src/application/AGENTS.md` (application-layer edits).

Acceptance: every plan batch's constraint exists in the init schema; each new constraint has a
test showing the invalid row is rejected; channel deletion and company deletion behave as today;
focused suites named by the plan pass with nonzero counts; final gate
`./scripts/reset-db.sh --all` then `scripts/check.sh` passes (or gaps named).

## Batching

One architecture gate (D1-D5 below, all schema/ownership decisions plus the member-removal
locking protocol), then one implementation batch covering plan batches 1-5 (the schema edits are
interdependent: batch 5's member FK changes batch 1's removal protocol). One batch review.

## Architecture gate — decisions (revision r1)

Spans re-checked at `e6b0a51`; they match the plan's `4e92edd` spans.

### D1 — Notification deliveries carry their tenant (plan batch 1)

Evidence: four producers write standalone (`purpose='notification'`) deliveries:
notification projection (`persistence/notification.rs` `apply_projection_on`), decision notices
(`persistence/workflow/decisions.rs` `insert_reviewers`), the reserved-address help reply
(`thread/ingest/routing.rs` `send_system_reply`), and rejection bounces
(`thread/mod.rs` `handle_bounce_dispatch`). Every one has a company: a bounce is produced only by
`undeliverable_rejection` (reached only after a selector matched a company, since
`UndeliverableSlugs::unknown/disabled` are recorded only after `matched_company = Some`) or by the
thread-injection check (always `Some(company)`). `BounceInfo.company_slug: Option` is therefore
`Some` on every reachable path.

Decision:
1. `message_deliveries.company_id` becomes `NOT NULL`. `message_deliveries_attribution_check`
   becomes: (channel, message, source binding, destination binding all set) OR (all four NULL AND
   `depends_on_delivery_id IS NULL` AND `external_destination IS NOT NULL` AND
   `purpose='notification'`). The existing `message_deliveries_company_id_fkey ... ON DELETE
   CASCADE` now covers notices.
2. `message_delivery_parts.company_id` becomes `NOT NULL`; its composite FK to its delivery is then
   always checked.
3. `notifications_email_delivery_id_fkey` becomes `(company_id, email_delivery_id) →
   message_deliveries(company_id, id) ON DELETE SET NULL (email_delivery_id)`.
   `workflow_decision_reviewers_notice_fkey` becomes `(company_id, notice_delivery_id) →
   message_deliveries(company_id, id)` (NO ACTION, as today). `workflow_decision_reviewer_guard`
   requires `delivery.company_id = NEW.company_id` instead of `IS NULL`.
4. `notification_from_delivery` skips rows with no channel (standalone notices) instead of rows
   with no company, so a failed notice still never raises a delivery-failure notification (today's
   behaviour; avoids a notice-about-a-notice loop).
5. The standalone dedup index stays global (`transport, idempotency_key WHERE
   destination_binding_id IS NULL`); the absorb lookup in `insert_standalone_delivery_on` also
   filters `company_id`, so a cross-company key collision is an Internal error, never a
   cross-tenant absorb.
6. Rust: `StandaloneDeliveryRequest` and `NewStandaloneDelivery` gain `company_id: Uuid`;
   `BounceInfo` gets a required `company_id: Uuid` and `company_slug: CompanySlug` (not Option);
   `undeliverable_rejection` takes the matched `Company`, and the unreachable "undeliverable with
   no company" case answers `UnknownRecipient` (no bounce) rather than a tenantless bounce.
   `DeliveryRecord` gains a top-level `company_id: Uuid` and `DeliveryAttribution` drops its
   `company_id` (one fact, not two); the delivery worker's per-company fair share groups by
   `record.company_id`, so notices now share their company's slot instead of a `None` slot.
   `DeliveryDb.company_id` / `PartDb.company_id` become `Uuid`.
7. The claim stays global (no change to `claim_deliveries`).
8. **Member-removal protocol (concurrency).** `remove_member`, in its one transaction:
   (a) `SELECT ... FROM company_members WHERE company_id=$1 AND user_id=$2 AND role<>'owner'
   FOR UPDATE` — no row: return Ok (today's owner/absent no-op). This conflicts with the
   `FOR KEY SHARE` a concurrent notification insert takes through D5's new
   `notifications → company_members` FK, so a projection that inserted a notice row has committed
   before we continue, and one that has not yet inserted blocks until we commit and then fails its
   FK (projection retries and finds a non-member).
   (b) demote the principal (as today) — this waits on any decision-reviewer insert holding the
   principal `FOR SHARE` (`workflow_decision_person_snapshot`), so its notice is committed first;
   later reviewer inserts see `kind='external'` and are refused.
   (c) lock (`FOR UPDATE`, `ORDER BY id`) the member's not-yet-cancelled notices that are
   claimable or in flight — via `notifications(company_id, recipient_user_id).email_delivery_id`
   and `workflow_decision_reviewers(company_id, user_id).notice_delivery_id` — and settle them with
   the existing `cancel_locked_deliveries_on(..., RecipientRevoked)`: claimable → `dead_letter`
   superseded with parts buried; in flight → intent only, the holding worker settles it.
   (d) delete the member row (as today; D5's FK cascades its notifications).
   Failure modes: an in-flight notice may still reach the provider (recorded as potentially
   sent, same as workflow cancellation). Test with competing claimants: an uncommitted
   transaction holding a notice for the member while `remove_member` runs; after it commits, the
   notice ends cancelled.

Invariants: no delivery row without a company; a notice's parts carry the same company; deleting a
company removes its notices and parts; a removed member has no claimable notice.
Affected contracts: `StandaloneDeliveryEnqueuer`, `DeliveryComposer::compose_standalone`,
`DeliveryRecord`, `BounceInfo`, `CompanyInvitePersistence::remove_member` (signature unchanged).
Company-scoped delivery dashboard counts (`persistence/dashboard.rs`, `WHERE company_id = $1`)
now include that company's notices (accurate; tenant delivery list and attention reads inner-join
message/binding/channel and are unchanged).

### D2 — A run's binding is proven (plan batch 2)

- Add `workflow_runs_binding_revision_fk (company_id, binding_id, binding_revision) →
  workflow_binding_revisions(company_id, binding_id, revision)`, NO ACTION (same as the run →
  channel FK `workflow_runs_company_id_channel_id_fkey`). App never deletes bindings; channel
  deletion with runs/bindings is refused by `ChannelPersistence::delete` before SQL (unchanged);
  company deletion cascades both in one statement.
- Replace `workflow_message_intent_binding_run` with `(company_id, run_id, binding_id) →
  workflow_runs(company_id, id, binding_id) ON DELETE CASCADE` (keeps that FK's cascade; uses the
  existing `workflow_runs_company_id_id_binding_id_key`). MATCH SIMPLE: refused/failed rows
  (`run_id NULL`) are unchecked, as today.
- Test fixtures that insert runs with invented binding ids get real binding revisions.

### D3 — One channel per row (plan batch 3)

- `channel_bindings`: add `UNIQUE (company_id, channel_id, id)`; replace
  `channel_bindings_company_transport_key (company_id, id, transport)` (only referenced by the
  delivery destination FK) with `UNIQUE (company_id, channel_id, id, transport)`.
- `external_threads`: add `channel_id uuid NOT NULL`; replace its two FKs with
  `(company_id, channel_id, binding_id) → channel_bindings(company_id, channel_id, id)` and
  `(company_id, channel_id, thread_id) → threads(company_id, channel_id, id)`, both
  `ON DELETE CASCADE` (as today). `upsert_external_thread` takes the channel id (both callers have
  it: `ResolvedThread.channel_id`, `scope.channel_id`).
- `message_deliveries`: destination FK becomes `(company_id, channel_id, destination_binding_id,
  transport) → channel_bindings(company_id, channel_id, id, transport)`, source FK becomes
  `(company_id, channel_id, source_binding_id) → channel_bindings(company_id, channel_id, id)`,
  both CASCADE as today; standalone rows (NULL channel/bindings) are unchecked by MATCH SIMPLE.
  The single producer (`DeliveryComposer::compose`) takes both bindings from the channel's own
  active bindings, so no writer change.
- `channel_schedules`: the binding may be **the company's** (`workflow_bindings.channel_id IS
  NULL`) or the schedule's channel's (`schedules.rs incompatibility`, p08 D1.9), so a plain
  `(company_id, channel_id, binding_id)` FK would reject valid company-binding schedules.
  Decision: `workflow_bindings` gets `UNIQUE (company_id, channel_id, id)` (plan) plus generated
  `company_wide boolean GENERATED ALWAYS AS (channel_id IS NULL) STORED` and
  `UNIQUE (company_id, id, company_wide)`. `channel_schedules` gets `binding_channel_id uuid`
  (NULL = the binding is the company's) with `CHECK (binding_channel_id IS NULL OR
  binding_channel_id = channel_id)`, generated `binding_company_wide boolean GENERATED ALWAYS AS
  (binding_channel_id IS NULL) STORED`, and two FKs replacing `channel_schedules_binding_fk`:
  `(company_id, binding_id, binding_company_wide) → workflow_bindings(company_id, id,
  company_wide)` (always checked: proves the schedule's claim about which kind of binding it is)
  and `(company_id, binding_channel_id, binding_id) → workflow_bindings(company_id, channel_id,
  id)` (checked when channel-scoped: proves the channel), both `ON DELETE CASCADE` (as today).
  Writers (`SchedulePersistence::create`/update) set `binding_channel_id` from the binding's
  association read in `check_binding`. A thread-bound binding has a channel, so the DB admits it
  under that channel; the app check still refuses it (plan scope is the channel path).

### D4 — A message is filed under its own thread (plan batch 4)

- `internal_notes`: add `internal_notes_thread_message_fk (company_id, channel_id, thread_id,
  message_id) → thread_messages(company_id, channel_id, thread_id, message_id)` NO ACTION. Keep
  `internal_notes_thread_fk` (its cascade is what removes notes with their thread; the
  immutability trigger permits only that delete) and `internal_notes_message_fk` (audience). Thread
  deletion removes the association and the note in one statement; message deletion stays blocked
  (FK violation, as today via the message FK). Both writers insert the association before the
  note.
- `workflow_message_destinations`: add `channel_id uuid NOT NULL` (the destination thread's
  channel); replace `workflow_message_destination_message` and `..._thread` with one
  `(company_id, channel_id, thread_id, message_id) → thread_messages(...)` NO ACTION (both old FKs
  were NO ACTION).
- `destination_id` (3 kinds: channel, user, reply — fewer than four, so per-kind columns):
  `destination_channel_id uuid` → `channels(company_id, id)`; `destination_user_id uuid` →
  `users(id)` (a member destination's id is the user id; a `company_members` FK would block or
  cascade on member removal while destinations are append-only run facts; tenancy of a user
  destination is already proven by its required `principal_id` FK); `replied_message_id uuid` →
  `thread_messages(company_id, channel_id, thread_id, message_id)` (a reply's source is in the
  reply's own thread — `RouteTarget::ReplyEmail/ReplyInternal` use the source thread). All NO
  ACTION. CHECK `workflow_message_destinations_target_check`: exactly the column matching `kind`
  is set, and a channel destination's `destination_channel_id = channel_id` (the plan opens the
  thread in the named channel). `destination_id` becomes `GENERATED ALWAYS AS
  (COALESCE(destination_channel_id, destination_user_id, replied_message_id)) STORED NOT NULL`, so
  every reader (SQL functions, receipts, reply waits, views) is unchanged and no dual write
  exists. `PlannedDestination` gains `channel_id`.

### D5 — Smaller gaps (plan batch 5)

- `inbound_events`: unique key `(company_id, transport, external_event_key)` replaces
  `(transport, external_event_key)`; the insert's `ON CONFLICT` and the duplicate lookup use the
  tenant key; installation/content-hash mismatch within one company stays a Conflict.
- `notification_events`: add `UNIQUE (company_id, id, source_kind, source_id, action_kind,
  source_generation)`; `notifications` gets one FK on that tuple `ON DELETE CASCADE`, replacing
  `notifications_event_id_fkey` and `notifications_event_identity_fk`.
- `notifications_recipient_member_fk (company_id, recipient_user_id) →
  company_members(company_id, user_id) ON DELETE CASCADE`, replacing
  `notifications_recipient_user_id_fkey` (users → members already cascades).
- `workflow_dispatch_demand` and `workflow_poll_companies`: `company_id → companies(id) ON DELETE
  CASCADE`. `VALID_HINT` (`workflow/fairness.rs`) is not a deleted-company sweep — it checks the
  job is pending/due, the execution open, the run live and unwaited — so nothing is dropped.
- `workflow_message_intent_reply_to` becomes NO ACTION.

## Verification plan

Focused (disposable `mail_agents_test` on 55405 after `./scripts/reset-db.sh --all`):
`cargo test --lib notification_delivery_tenant_`, `run_binding_fk_`, `channel_path_fk_`,
`message_thread_fk_`, `tenant_fk_small_`; then affected suites: `delivery`, `notification`,
`workflow::admission`, `message_intents`, `integration`, `schedules`, `thread`, plus
`company_invite`, `decision`, `inbound_event`, `workflow::message`, `fairness`. Nonzero counts
recorded. Final gate: `./scripts/reset-db.sh --all` then `scripts/check.sh`; `.sqlx` regenerated.

## Status

- Architecture gate r1: pending B review.
- Implementation: not started.

## Check log

(filled per batch)

## Cross-phase notices requested

(filled as found)
