# 05 — Tenant integrity

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Part of [fix-app](README.md). No prerequisites, so it can run alongside 01 and 02. It overlaps 01
on member removal and the principal/message foreign keys, and 02 on delivery code: whichever merges
second rebases and reruns both plans' focused tests.

## Outcome

Every relationship the application treats as "same company" or "same channel" is proven by the
database, following the rule in `src/adapters/persistence/AGENTS.md` ("Preserve invariants at the
database boundary"). Nothing outlives its company. The application keeps its checks as friendly
error messages; the constraints are the guarantee.

For each new constraint:
- test that the invalid row is rejected;
- test that channel deletion and company deletion still behave as they do today.

## Batches

### 1 — Notification emails get a tenant

**Today:**
- The CHECK `message_deliveries_attribution_check` (`init_schema.sql:3786`) requires `company_id
  IS NULL` for `purpose = 'notification'`.
- Those deliveries hang off single-column FKs: `notifications.email_delivery_id` (`:9522`, `SET
  NULL`) and `workflow_decision_reviewers.notice_delivery_id` (`:10305`).
- Effect: delete a company, or remove a member, while a decision notice is pending, and the email
  still goes out. The recipient address and the decision content then stay in
  `message_delivery_parts` forever.

**Change:**
- Notification deliveries carry a `NOT NULL company_id`, with composite FKs that cascade.
- `message_delivery_parts.company_id` (nullable at `:3807`) becomes `NOT NULL`. With MATCH SIMPLE,
  its composite FK (`:9425`) is skipped whenever it is NULL.
- Member removal (`remove_member`, `src/adapters/persistence/company_invite.rs:409`) cancels the
  removed member's pending notification deliveries in the same transaction.
- The claim stays global by design.

**Tests, `notification_delivery_tenant_*`:**
- Deleting a company removes its pending notification deliveries and their parts. Use
  `own_database`, because this asserts whole-table state.
- Removing a member cancels their pending notices.
- A part whose `company_id` differs from its delivery's is rejected.

### 2 — A run's binding is proven

**Today:**
- `workflow_runs.binding_id` and `binding_revision` have no FK, and the run's unique key
  `(company_id, id, binding_id)` (`:7300`) is the only thing tying them down.
- `workflow_message_intent_bindings` references its binding (`:10495`) and its run (`:10503`)
  separately. So an `admitted` row can name binding B1 while its run belongs to B2.

**Change:**
- FK `workflow_runs(company_id, binding_id, binding_revision) → workflow_binding_revisions`.
- Replace the intent-binding run FK with `(company_id, run_id, binding_id) →
  workflow_runs(company_id, id, binding_id)`.
- For `ON DELETE`, take the behaviour of the existing run → channel FK, so deleting a channel
  behaves as it does today. Application code never hard-deletes bindings.

**Tests, `run_binding_fk_*`:**
- A run naming an unknown binding revision is rejected.
- An intent-binding row naming a different binding from its run is rejected.
- Deleting a channel with runs still behaves as before.

### 3 — One channel per row

**Today:** these tables reach a channel by two paths that can name different channels. Only the
application checks them (`workflow/schedules.rs:219`, `:313`; `thread/external.rs:200-230`).

| Table | First path | Second path |
|---|---|---|
| `external_threads` | its binding (`:9265`) | its thread (`:9273`) |
| `message_deliveries` | `channel_id` (`:9361`) | its destination binding (`:9385`) and source binding (`:9417`) |
| `channel_schedules` | `channel_id` (`:9121`) | its workflow binding (`:9137`) |

- Example: a Slack thread on channel A's binding is mapped to a thread in restricted channel B, so
  B's replies go out to A's endpoint.

**Change:**
- Add `UNIQUE (company_id, channel_id, id)` on `channel_bindings` and on `workflow_bindings`.
- Carry `channel_id` where it is missing, and use composite FKs.

**Tests, `channel_path_fk_*`:** a cross-channel row is rejected for each of the three tables.

### 4 — A message is filed under its own thread

**Today:**
- `internal_notes` references its message (`:9337`) and its thread (`:9353`) separately.
- `workflow_message_destinations` does the same (`:10471`, `:10559`).
- `workflow_message_destinations.destination_id` (`:5269`) is polymorphic by `kind` and has no FK.

**Change:**
- FK both tables to `thread_messages(company_id, channel_id, thread_id, message_id)`. The
  destinations table needs a `channel_id` for this.
- For `destination_id`, add one nullable FK column per kind, with a CHECK that exactly the column
  matching `kind` is set. Do this only if fewer than four kinds exist; otherwise record why it stays
  polymorphic.

**Tests, `message_thread_fk_*`:** a note or destination naming a message from another thread is
rejected.

### 5 — Smaller gaps

- **Per-tenant inbound dedup.**
  - Today the key is `UNIQUE (transport, external_event_key)` (`:5998-6000`).
  - Two companies connected to the same Resend account each receive the event under their own
    authenticated scope. The second is rejected as a conflict
    (`src/adapters/persistence/inbound_event/mod.rs:189-196`), so its mail is lost.
  - Make the key `(company_id, transport, external_event_key)`. The scope and payload check stays,
    for replays within one company.
- **One FK for a notification's event.** `notifications.event_id` (`:9530`) is a single-column FK
  beside the composite FK on the source tuple (`:9537`). Add the matching `UNIQUE` on
  `notification_events` and use one composite FK.
- **A notification's recipient must be a member.** FK `notifications(company_id,
  recipient_user_id) → company_members` with `ON DELETE CASCADE`, consistent with
  `notification_withdraw_deleted_principal`.
- **Two tables with no parent.**
  - `workflow_dispatch_demand` (PK `company_id`, `:6980`) and `workflow_poll_companies` (`:5356`)
    have no FK to `companies`.
  - Add `ON DELETE CASCADE`.
  - Then check whether the lazy `VALID_HINT` sweep (`src/adapters/persistence/workflow/fairness.rs:67-73`)
    exists only for deleted companies, and drop that part if so.
- **A CHECK that `SET NULL` violates.**
  - `workflow_message_intents.reply_to_message_id` is `ON DELETE SET NULL` (`:10527`), but
    `workflow_message_intent_reply_settled` (`:5340`) requires it once settled.
  - Use `NO ACTION`.
  - Prove channel deletion and company deletion still succeed: both cascade the intents in the same
    statement, and `NO ACTION` is checked at statement end.

**Tests, `tenant_fk_small_*`:**
- The same event key is stored for two companies, and rejected as a conflict within one company.
- A cross-tenant notification event, or a non-member recipient, is rejected.
- Company deletion leaves no demand or poll rows.
- Channel deletion with a settled reply intent succeeds.

## Focused checks

```sh
createdb mail_agents_fix_app_05
SQLX_OFFLINE=true TEST_DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_fix_app_05" \
  cargo test --locked --offline --lib notification_delivery_tenant_
# repeat with run_binding_fk_, channel_path_fk_, message_thread_fk_, tenant_fk_small_
dropdb mail_agents_fix_app_05
```

Run `./scripts/reset-db.sh --all` after every init-schema edit. After each batch, rerun the suites
for the tables it touched:
- `delivery` and `notification` after batch 1;
- `workflow::admission` and `message_intents` after batch 2;
- `integration`, `schedules` and `thread` after batch 3.

Confirm a nonzero test count for each filter.

## Final gate

Run `./scripts/reset-db.sh --all`, then `scripts/check.sh` once. Record the commands and results,
and name any gate that could not run.
