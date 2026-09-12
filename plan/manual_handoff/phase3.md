# Phase 3 — The handoff in the attention queue, and the responsibility commands

Read [`general_plan_instructions.md`](general_plan_instructions.md) first, then
[`phase2.md`](phase2.md): this phase assumes `thread_handoffs`, `thread_handoff_events` and the
generation model already exist and that a held reply already writes one.

**Goal.** A handoff in an **actionable** state appears in the shared attention queue as an
`AttentionItem`, under `AttentionSourceKind::ThreadHandoff`, in the right view for whoever is
responsible; and four commands — **claim**, **release**, **reassign**, **set priority / due** — move
it, each one tenant-scoped, version-**and**-generation fenced, idempotent under a client
`command_id`, and audited by exactly one `thread_handoff_events` row.

After Phase 2 a held reply exists with nowhere to be seen but the thread itself. This phase is what
makes Phase 2 + 3 a shippable increment — "held replies are queued for the team, and the team can
take them" — with none of Phase 4's drafting machinery. That is the reordering the general file's
"Deviation from the shape suggested by the audit" section commits to, and the projection built here
is the *same* projection Phase 4 reuses for `draft_ready` and Phase 5 reuses for the mailbox badge.

**Files touched**

| File | Change |
|---|---|
| `src/domain/entities/attention.rs` | `AttentionSourceKind::ThreadHandoff` |
| `src/domain/entities/thread_handoff.rs` | `ThreadHandoffCommand`, `ThreadHandoffOperation`, `validate` |
| `src/application/thread_handoff.rs` | the port gains `change_thread_handoff`, `get_thread_handoff`, `thread_handoffs_for_threads` |
| `src/application/use_cases/thread_handoff.rs` | forwarding methods plus the read-authorization wrapper |
| `src/adapters/persistence/attention.rs` | a seventh `raw` branch in `ATTENTION_SQL`; the `ThreadHandoff` arm of `item_href`; the `BadRequest` arm of `change_source_attributes` |
| `src/adapters/persistence/thread_handoff.rs` | `change_thread_handoff` and its lock / fingerprint / fence / event sequence |
| `src/adapters/persistence/thread_handoff_tests.rs` | command tests |
| `src/adapters/persistence/attention_tests.rs` | projection tests |
| `src/adapters/http/routes/thread_handoffs.rs` | **new**: the command route and the JSON read |
| `src/adapters/http/pages/attention.rs` | the `ThreadHandoff` label in `attention_card` |
| `src/infra/events.rs` | `AttentionWakeSource::ThreadHandoff` |
| `migrations/20260817000000_init_schema.sql` | `thread_handoffs_notify_attention` trigger |

---

## 3.1 The seventh branch of `ATTENTION_SQL`

`ATTENTION_SQL` (`src/adapters/persistence/attention.rs:87-326`) is one `WITH` over six
`UNION ALL` branches. A seventh goes after the `handoff` branch (`:162-179`) and before `approval`,
which keeps the file in the same order as the `AttentionSourceKind` enum and puts the two handoff
kinds next to each other where a reader will compare them.

```sql
    UNION ALL

    SELECT 'thread_handoff', handoff.id, handoff.company_id, handoff.channel_id,
           handoff.thread_id, NULL::uuid, NULL::uuid, handoff.state,
           handoff.responsible_principal_id,
           CASE WHEN handoff.responsible_principal_id IS NULL
                THEN 'channel_team' ELSE 'principal' END,
           COALESCE(responsible.display_label, 'Channel team'), thread.subject,
           CASE handoff.state
             WHEN 'needs_instruction' THEN 'Tell the agent what to do, reply, or dismiss'
             WHEN 'draft_ready' THEN 'Review the drafted reply and send it'
             ELSE 'Waiting for the drafting run'
           END,
           handoff.business_priority, handoff.business_due_at, NULL::timestamptz,
           handoff.version, handoff.generation_opened_at, handoff.updated_at
    FROM thread_handoffs AS handoff
    JOIN threads AS thread
      ON (thread.company_id, thread.channel_id, thread.id)
       = (handoff.company_id, handoff.channel_id, handoff.thread_id)
    LEFT JOIN principals AS responsible
      ON responsible.company_id = handoff.company_id
     AND responsible.id = handoff.responsible_principal_id
    WHERE handoff.company_id = $1 AND handoff.channel_id = ANY($2)
      AND handoff.state IN ('needs_instruction', 'draft_ready')
      AND ($4 <> 'my_work' OR handoff.responsible_principal_id = $3)
      AND ($4 <> 'unassigned' OR handoff.responsible_principal_id IS NULL)
```

Every column choice here is load-bearing:

- **No new bind parameter.** The statement stays at fourteen, so `bind_attention`
  (`:332-359`) is untouched and its "a test can hold a reference statement to exactly the parameters
  production binds" property still holds.
- **`title` is the thread's subject**, joined rather than stored. The general file's "bounded text"
  rule is why: `thread_handoffs` has no `title` column to bound, escape or keep in step with a
  renamed thread. `threads.subject` is already bounded by ingest (`MAX_SUBJECT_BYTES`,
  `src/application/transport/ingress.rs:38`).
- **`next_action` is a `CASE` over the state**, not a column, for the same reason. The `ELSE` arm
  covers `drafting` and is unreachable from this branch's own `WHERE` — keep it anyway, so a state
  added to the filter later cannot render an empty action.
- **`state IN ('needs_instruction', 'draft_ready')`** is `ThreadHandoffState::is_actionable`
  (Phase 2 §2.5) written in SQL. `drafting` is visible progress, not work, so it leaves the queue —
  which is the general file's worked-example step 5 and the reason the state exists at all. The two
  terminal states are gone for good. A comment above the line must say that the predicate and
  `is_actionable` are two spellings of one rule, and that changing one without the other is the bug.
- **`created_at` is `generation_opened_at`, not `created_at`.** The queue's age column and the
  cursor's `created_at` component must describe *this* wait, not the first time this thread was ever
  held. A thread held three times in a month is not a month-old item. The consequence is that a
  regeneration moves the item in the sort order mid-pagination; that is already true of any item
  whose priority changes and is what `AttentionPage.as_of` and the re-query-on-wake-up contract
  exist for.
- **`task_id` and `correlation_id` are `NULL::uuid`.** A handoff has no task until Phase 4 starts a
  drafting run, and `thread_handoff_runs` does not exist yet. Phase 4 replaces both with a `LEFT
  JOIN` on that table; a comment naming Phase 4 goes on the two `NULL`s so the next reader knows
  they are a stage, not an oversight.
- **`expires_at` is `NULL`.** Nothing expires a handoff. SLA and escalation are explicitly out of
  scope (general file), and a non-null value here would feed `AttentionCursor::for_item`'s
  `due_rank` and silently create an SLA the product never agreed.
- **`version` is `handoff.version`** — the same number every command fences on, so an item a client
  read from the queue can be acted on without a second fetch.
- **The `my_work` / `unassigned` branch filters are copied verbatim from the `manual_handoffs`
  branch** (`:178-179`), and the `ranked` backstop (`:291-305`) is not touched. The comment at
  `:291-295` is the contract: a branch filter may be narrower than nothing but never narrower than
  `ranked`.

`AttentionRow` and its `TryFrom` (`:21-85`) need **no change**: `source_kind` is parsed through
`FromStr`, `responsibility_kind` already handles `channel_team`/`principal`, and the priority parses
through `BusinessPriority`. That is the payoff for reusing `BusinessPriority` in Phase 2.

## 3.2 The source kind and the link

`AttentionSourceKind` (`src/domain/entities/attention.rs:63-101`) gains `ThreadHandoff` with
`as_str()` `"thread_handoff"` and the matching `FromStr` arm. Put the variant **after** `Handoff`.
Two notes:

- The derived `Ord` is not used for pagination — `after_cursor` (`:306-319`) compares `source_kind`
  as *text* in SQL, so `'thread_handoff'` sorts after `'task'` and `'response_review'` regardless of
  declaration order. `grep` for `source_kind` outside `attention.rs` confirms nothing in Rust sorts
  on the derived order today; it is matched only (`src/adapters/http/pages/attention.rs:151`,
  `src/adapters/http/routes/attention.rs:295`). If that ever changes, the declaration order must be
  made to match the text order.
- `AttentionCursor`'s `Display`/`FromStr` (`:172-220`) split on `~`, and `"thread_handoff"` contains
  none, so cursors round-trip unchanged. Add it to the existing
  `cursor_round_trips_every_sort_component` test as a second case rather than a new test.

`item_href` (`:361-391`) gains a `ThreadHandoff` arm. Unlike the `Handoff` arm above it, `thread_id`
is `NOT NULL` for a thread handoff, so there is no fallback to write:

```rust
AttentionSourceKind::ThreadHandoff => format!(
    "/ui?company_id={}&channel_id={}&thread_id={}",
    item.company_id,
    item.channel_id,
    item.thread_id.expect("a thread handoff always names its thread"),
),
```

Use `expect` with that message rather than a silent fallback: a `NULL` here would mean the schema's
`NOT NULL` was removed, and a link to the wrong place is worse than a panic in a test.

`attention_card` (`src/adapters/http/pages/attention.rs:145-190`) gains one label —
`AttentionSourceKind::ThreadHandoff => "Needs instruction"`. **Not** "Thread handoff": the badge is
read by a person deciding whether to pick the item up, and every other label in that match is
already the work rather than the table. The state is rendered separately at `:169`, so
`needs_instruction` and `draft_ready` are still distinguishable.

## 3.3 The command

One command type for all four operations, in `src/domain/entities/thread_handoff.rs`:

```rust
/// What a responsibility command does. Named, because `Option<Option<PrincipalId>>` is not a
/// vocabulary: "claim", "release" and "reassign" are three intents that all write one column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadHandoffOperation {
    /// Take an unclaimed handoff. Only from `responsible_principal_id IS NULL`.
    Claim,
    /// Give a claimed handoff back to the channel team.
    Release,
    /// Move it to another principal.
    Reassign { to: PrincipalId },
    /// Priority and due time only; responsibility is untouched.
    SetAttributes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadHandoffCommand {
    pub company_id: Uuid,
    pub handoff_id: Uuid,
    pub command_id: Uuid,
    pub expected_version: u64,
    pub expected_generation: Uuid,
    pub operation: ThreadHandoffOperation,
    pub priority: BusinessPriority,
    pub due_at: Option<DateTime<Utc>>,
    pub actor_principal_id: PrincipalId,
    /// Authorization context, not command semantics; omitted from the idempotency fingerprint.
    #[serde(skip)]
    pub visible_channel_ids: Vec<Uuid>,
}
```

Three things this shape gets right, each copied from something that already works:

- **`#[serde(skip)]` on `visible_channel_ids`**, exactly as `AttentionSourceCommand` does
  (`src/domain/entities/attention.rs:285-287`). The fingerprint is a hash of the serialized command
  (§3.4), so an authorization context in it would make the same command replayed by a principal with
  a different channel list look like a *different* command and return a `Conflict` instead of the
  recorded outcome.
- **`expected_generation` alongside `expected_version`.** The version alone is not enough: a
  regeneration bumps the version *and* the generation, so version fencing would already catch it —
  but a claim written against generation A that happens to arrive when the row is at the same
  version under generation B is representable if any future path writes a generation without a
  version bump. Carrying both makes the fence state what it means, and it is what lets the
  `Conflict` message name the current generation, which is what the general file's step 8 promises
  the user.
- **`priority` and `due_at` are always present**, not `Option`, matching `AttentionSourceCommand`.
  A command therefore states the whole attribute pair every time, so "set priority High" cannot
  silently drop a due time somebody else set — the version fence is what makes that safe, and the
  client always has both values from the queue item it read.

`validate(&self) -> Result<(), String>`, in the domain and unit-tested with no database:
`expected_version` is non-zero and `<= i64::MAX`; `expected_generation` is not nil;
`Reassign { to }` where `to == actor_principal_id` is an error naming `Claim` instead (so the
audit's `operation` is honest); `due_at` more than a year out is an error, matching nothing existing
— **omit that last rule** unless a sibling already has it, and it does not. Keep `validate` to the
three real rules.

## 3.4 The persistence command

`change_thread_handoff` in `src/adapters/persistence/thread_handoff.rs`. The model to copy,
statement for statement, is `change_source_attributes`
(`src/adapters/persistence/attention.rs:642-823`): lock, check the recorded command, load and lock
the row, check the fence, authorize, write, append the event, commit. Reuse its helpers rather than
re-deriving them — `lock_attention_source` (`:483-496`), `require_human_principal` (`:422-439`),
`require_channel_principal` (`:441-475`), `actor_is_manager` (`:400-420`) and
`command_fingerprint` (`:393-398`) are all free functions in that module; make them
`pub(crate)` and import them rather than writing a second copy in this file.

In order:

1. `command.validate().map_err(AppError::BadRequest)?`.
2. `let fingerprint = command_fingerprint(&command)?;` — SHA-256 of the serialized command, with
   `visible_channel_ids` skipped.
3. `lock_attention_source(&mut tx, company_id, "thread_handoff", handoff_id)` — the advisory key is
   `"{company}:{kind}:{id}"`, so a `thread_handoff` and a `manual_handoffs` row sharing a UUID
   cannot collide on the lock.
4. **Idempotency**, against `thread_handoff_events` rather than `attention_source_events`:

   ```sql
   SELECT command_fingerprint, to_version FROM thread_handoff_events
    WHERE company_id = $1 AND handoff_id = $2 AND command_id = $3
   ```

   A matching fingerprint returns the recorded `to_version`; a different one is
   `AppError::Conflict("Thread handoff command id was already used with different parameters.")`.
   This is `existing_event` (`:498-528`) with one table changed, so write it as a private
   `existing_handoff_event` in this module with the same shape and the same two error paths.
5. `require_human_principal(&mut tx, company_id, command.actor_principal_id)` and
   `let manager = actor_is_manager(…)`.
6. Load and lock the row:

   ```sql
   SELECT state, generation, responsible_principal_id, business_priority, business_due_at,
          version, channel_id
     FROM thread_handoffs
    WHERE company_id = $1 AND id = $2 AND channel_id = ANY($3)
      AND state IN ('needs_instruction', 'drafting', 'draft_ready')
      FOR UPDATE
   ```

   `None` is `AppError::NotFound("Thread handoff not found.".into())` — the *same* message for a
   handoff in another company, a handoff on a channel the caller cannot view, and a terminal
   handoff. Never a message that distinguishes them: that is the general file's "never a leak of
   existence" and it is why `visible_channel_ids` is in the predicate rather than checked afterwards.

   Note the state filter includes `drafting`, which the projection excludes. A handoff can be
   reassigned or re-prioritised while its draft run is in flight — the item is out of the queue but
   the thread banner is still there, and forbidding it would mean a manager cannot hand over work
   that is mid-run.
7. **Fence**, in this order and with these messages:
   - `generation != command.expected_generation` →
     `AppError::Conflict(format!("This thread received a newer reply; the current handoff generation is {current}. Refresh and try again."))`. Generation first, because it is the more useful
     answer when both are stale.
   - `version != command.expected_version` → the `change_source_attributes` wording at `:724-729`,
     verbatim modulo the noun.
8. **Authorization.** Copy the rule at `:730-738` exactly, which is subtler than it looks:
   - a **manager** (`manages_company_operations`, via `actor_is_manager`) may do anything;
   - a non-manager may act only when the row is *already* theirs (`old_responsible == Some(actor)`);
   - the one exception is the **self-claim**: `Claim` on a row with
     `responsible_principal_id IS NULL` where the actor is claiming for themselves. That is what
     makes an unclaimed item claimable by any teammate who can see the channel.
   - any other case is `NotFound`, not `Forbidden`. A teammate must not learn that a handoff they
     cannot act on exists on a channel they cannot see.
   - `Reassign { to }` additionally calls
     `require_channel_principal(&mut tx, company_id, channel_id, to)`, so a handoff cannot be
     assigned to somebody who cannot open the thread. `create_handoff` does exactly this at
     `:595-598`.
9. **Write**, with the new responsibility derived from the operation and *not* from an `Option`
   round trip:

   ```sql
   UPDATE thread_handoffs
      SET responsible_principal_id = $3, business_priority = $4, business_due_at = $5,
          version = $6, updated_at = CURRENT_TIMESTAMP
    WHERE company_id = $1 AND id = $2 AND version = $7 AND generation = $8
   ```

   `rows_affected() != 1` is `AppError::Conflict` — the `FOR UPDATE` above makes it unreachable, and
   an unreachable check that fires is how a lost fence is discovered rather than tolerated.
   Note `bump_handoff_version_for_responsibility_cleanup` (Phase 2 §2.1) also bumps the version on a
   responsibility change; it only fires when `NEW.version = OLD.version`, so an explicit bump here
   wins and the trigger stays what it is for — the `ON DELETE SET NULL` path.
10. **One event**, in the same transaction, with `operation` derived from what actually changed:
    `claimed` / `released` / `reassigned` / `attributes_changed`. Derive it in a pure `const fn` over
    `(old_responsible, new_responsible)` plus the requested operation, not inline — the
    `change_source_attributes` version of this at `:789-793` is one `if`, and four cases in an `if`
    chain is where an audit log starts lying. `from_state` and `to_state` are both the row's state:
    a responsibility command never changes state, and recording that explicitly is what makes a
    state change without a state event detectable.
11. `tx.commit()`, return the new version.

The other two port methods this phase adds are plain reads:

- `get_thread_handoff(company_id, handoff_id, visible_channel_ids) -> AppResult<Option<ThreadHandoff>>`
  — the same `WHERE` as step 6 without `FOR UPDATE` and without the state filter, so a resolved
  handoff can still be read back by a test or a route that needs to explain itself.
- `thread_handoffs_for_threads(company_id, thread_ids, visible_channel_ids) -> AppResult<HashMap<Uuid, ThreadHandoff>>`
  — keyed by `thread_id`, `WHERE thread_id = ANY($2)`, open states only. Phase 5's badges and banner
  are its only callers and it is written here because it belongs beside the other two statements;
  `list_thread_work_summary` (`src/adapters/persistence/task/operations.rs:389-466`) is the shape to
  copy, including returning a map rather than a `Vec` the caller has to index.

## 3.5 Why `change_source_attributes` is not extended

`change_source_attributes` already rejects four of its six source kinds with a `BadRequest`
(`:643-651`) and `unreachable!()`s them in three `match`es (`:714-718`, `:784-787`). Adding a fifth
arm would mean branching its idempotency store, its lock key, its `UPDATE` and its event insert on
the kind — four conditionals inside a function that is already 180 lines. So:

- `AttentionSourceKind::ThreadHandoff` joins the **rejected** list, with the message
  "Thread handoff responsibility must be changed through its own command." That keeps
  `POST /companies/{id}/attention/thread_handoff/{id}` from half-working, and it keeps
  `attention_source_events_source_kind_check` (init migration `:1871`) untouched — the general
  file's collision table requires both.
- The three `unreachable!()` arms gain `ThreadHandoff` beside the others. They are `unreachable` only
  because the guard above returns first; the compiler is what keeps that true.

This is `src/AGENTS.md`'s "when you edit a function that already breaks one of them, don't extend the
violation", applied to a function whose problem is size rather than correctness.

## 3.6 The routes

A new router, `src/adapters/http/routes/thread_handoffs.rs`, merged in
`src/adapters/http/routes/mod.rs` beside `attention::router()`:

| Route | Body / result |
|---|---|
| `POST /api/companies/{company_id}/thread-handoffs/{handoff_id}` | `{ command_id, expected_version, expected_generation, operation, priority, due_at }` → `{ "version": n }` |
| `GET /api/companies/{company_id}/thread-handoffs/{handoff_id}` | the handoff as JSON, or `NotFound` |

Both authorize through `read_context` (`src/adapters/http/routes/attention.rs:97-125`) — make it
`pub(super)`-visible to this module, as `attention.rs` already exposes it for its own use. It gives
the caller's `principal_id` and `visible_channel_ids` in one place and refuses a non-team member with
`company_not_found()`, which is the behaviour every new route in this plan needs.

`operation` deserializes as `{"kind": "claim"}` / `{"kind": "reassign", "to": "<uuid>"}` through
`ThreadHandoffOperation`'s derived `Deserialize`; an unknown kind is a `BadRequest` from serde, not
a silent default. Contrast `parse_review_override`
(`src/adapters/http/routes/channel.rs:165-178`), which maps an unrecognised string to `None` —
Phase 1 §1.6 already refused to copy that and this phase must not either.

The HTML surface for these commands is **Phase 5's**, and the attention queue page
(`src/adapters/http/pages/attention.rs:116-143`) keeps its current shape: cards with an "Open
source" link, no inline forms. Claiming from the queue page is a deliberate non-goal here — the
queue links into the thread, and the thread is where the banner with the buttons lives. One place
for the actions means one place for the version and generation to come from.

## 3.7 The wake-up

One trigger, in the init migration's trigger section in alphabetical position (beside
`thread_handoffs_bump_cleanup_version` from Phase 2, and after
`task_outreaches_notify_attention` at `:5986`):

```sql
CREATE TRIGGER thread_handoffs_notify_attention
    AFTER INSERT OR DELETE OR UPDATE OF state, responsible_principal_id, business_priority,
        business_due_at, version, generation
    ON public.thread_handoffs FOR EACH ROW
    EXECUTE FUNCTION public.notify_attention_changed('thread_handoff');
```

`notify_attention_changed` (`:1028-1068`) already does the right thing with no change: it reads
`company_id`, `channel_id` and `id` out of `to_jsonb(NEW)`, and only its `response_review` and
`delegation` branches need a table-specific lookup. The payload is
`{company_id, channel_id, source_kind, source_id}` — identifiers only, far under 8000 bytes, exactly
as the general file requires.

`AttentionWakeSource` (`src/infra/events.rs:85-93`) gains `ThreadHandoff`, serializing as
`"thread_handoff"`. Add a case to `parses_the_payload_the_triggers_build` (`:363-425`) with the new
`source_kind`, because that test is the only thing standing between a renamed trigger argument and a
queue that silently stops updating.

**`UPDATE OF … generation` is in the column list** so a regeneration wakes the queue even when the
state, responsibility, priority and due time are all unchanged — which is exactly the case where a
`draft_ready` item becomes a `needs_instruction` one... and in that case the state *did* change.
Keep `generation` anyway: a regeneration from `needs_instruction` to `needs_instruction` changes only
the generation and `version`, and while `version` alone would fire it, a reader of this trigger
should not have to reason that far.

**No notification rows.** `thread_handoff_events` is not `attention_source_events`, so the
`handoff_actionable_notification` trigger (`:5720`) does not fire for any of this, and
`notification_from_attention_source()` is untouched. The general file's out-of-scope boundary is
enforced by the schema here rather than by discipline — say so in a comment above the new trigger.

---

## Tests

**Unit, no database** — `src/domain/entities/thread_handoff.rs` and `attention.rs`:

1. `ThreadHandoffCommand::validate`: zero and `i64::MAX + 1` versions are errors; a nil
   `expected_generation` is an error; `Reassign { to: actor }` is an error naming `claim`; a valid
   claim, release, reassign and set-attributes all pass.
2. The `operation`-derivation `const fn`: `(None → Some(actor))` is `claimed`,
   `(Some(actor) → None)` is `released`, `(Some(a) → Some(b))` is `reassigned`, and an unchanged
   responsibility is `attributes_changed` — including the case where the *requested* operation was
   `Claim` but the row was already theirs, which must audit as `attributes_changed` rather than a
   second claim.
3. `AttentionSourceKind::ThreadHandoff` round-trips `"thread_handoff"`; `"threadhandoff"` and
   `"handoff "` are errors; a cursor carrying it round-trips through `Display`/`FromStr`.
4. The command serializes without `visible_channel_ids`: two commands identical but for the channel
   list produce the same `command_fingerprint`. This is the regression guard for §3.3's
   `#[serde(skip)]`, and it belongs beside the fingerprint helper rather than in a DB test.

**Projection** (`src/adapters/persistence/attention_tests.rs`, using that file's existing
`FeedScope` fixture shape and `insert_pending_review`-style helpers at `:1159-1198`), each scoped to
its own company:

5. A `needs_instruction` handoff with no responsible principal appears in **`unassigned`** and
   **`team_work`**, and not in any principal's `my_work`. Its `title` is the thread's subject, its
   `next_action` is the `needs_instruction` sentence, `priority` is `Normal`, `due_at`, `task_id`,
   `correlation_id` and `expires_at` are `None`, `version` is 1, and `href` is
   `/ui?company_id=…&channel_id=…&thread_id=…`.
6. Claimed by a principal: it leaves `unassigned`, appears in **that** principal's `my_work`, does
   not appear in another principal's `my_work`, and `responsibility_label` is the principal's
   `display_label`.
7. `drafting` appears in **no** view; `draft_ready` appears again with the review sentence;
   `resolved` and `dismissed` appear in none.
8. Channel scoping: a handoff on a channel absent from `visible_channel_ids` is not returned in any
   view, and neither is one in another company — assert by `source_id`, never by counting the page.
9. Renaming the thread changes the item's `title` on the next query, with no write to
   `thread_handoffs`. That is the whole argument for the join, so it gets a test.
10. `generation_opened_at` drives the age: a handoff whose row is old but whose generation was
    opened a minute ago sorts with the recent items, and its `created_at` in the item equals
    `generation_opened_at`.
11. Ordering and paging: a `Urgent` handoff, a `High` task and a `Normal` handoff in one company
    page in priority order, and the cursor from page one continues correctly into page two.
12. The item's `version` equals the row's, and a command run with that version succeeds — the
    round trip a client actually performs.

**Commands** (`src/adapters/persistence/thread_handoff_tests.rs`):

13. **Claim.** An unclaimed handoff claimed by a non-manager teammate with `view` on the channel:
    responsibility becomes theirs, `version` 1 → 2, exactly one new event with
    `operation = 'claimed'`, `actor_kind = 'human'`, the actor's principal, `from_state` and
    `to_state` both `needs_instruction`, `from_version = 1`, `to_version = 2`.
14. **Two teammates claim at once.** One succeeds; the other gets a `Conflict` naming version 2.
    Assert exactly one `claimed` event for that `(company_id, handoff_id)`.
15. **Release and reassign.** A manager releases a claimed handoff (`released`,
    `responsible_principal_id IS NULL`) and reassigns another to a second principal
    (`reassigned`, with both `previous_` and `new_responsible_principal_id` recorded).
16. **Priority and due.** `SetAttributes` to `Urgent` with a due time: the row and the projection
    both move, responsibility unchanged, event `attributes_changed` carrying both the previous and
    the new pair.
17. **Non-manager authorization.** A teammate who is not the responsible principal gets `NotFound`
    for release, reassign and set-attributes on a handoff claimed by somebody else — and the same
    `NotFound` for a handoff in a channel they cannot view, so the two are indistinguishable.
18. **Reassign to an ineligible principal** (no `view` grant on a restricted channel) is `NotFound`
    and writes nothing.
19. **Stale generation.** Claim against the previous generation after a second held reply arrives:
    `Conflict`, its message contains the current generation, the row is untouched and no event is
    written. Then claim with the current generation and succeed — the handoff is still claimable, the
    command just had to be refreshed.
20. **Stale version, current generation**: `Conflict` naming the current version.
21. **Idempotency.** The same command replayed returns the recorded version and writes no second
    event (count by `(company_id, command_id)`). The same `command_id` with a different
    `priority` is a `Conflict`. The same `command_id` with a different `visible_channel_ids` is
    **not** — it returns the recorded outcome.
22. **Terminal handoff.** A `resolved` or `dismissed` handoff refuses every command with `NotFound`,
    and `get_thread_handoff` still returns it.
23. **`drafting` accepts responsibility commands** but the item is absent from every view — the
    "reassign work that is mid-run" case from §3.4 step 6.
24. **`change_source_attributes` rejects `thread_handoff`** with the `BadRequest` message, without
      touching the row.
25. **Cleanup trigger.** Deleting the responsible principal sets the column `NULL` and bumps
    `version`, and a command written against the pre-delete version is then a `Conflict` rather than
    a write onto a claim that no longer exists.

**Routes** (beside the existing attention route tests):

26. `POST /api/companies/{id}/thread-handoffs/{id}` for a handoff in another company is `NotFound`;
    for a channel the caller cannot view, `NotFound`; for a non-team member, the company-not-found
    error.
27. A body with an unknown `operation.kind` is a `BadRequest` and changes nothing.
28. `GET` after a successful claim returns the new responsibility and version.

**Events** (`src/infra/events.rs`): the payload test parses a `"thread_handoff"` wake-up into
`AttentionWakeSource::ThreadHandoff`.

## Done when

- [ ] The seventh `ATTENTION_SQL` branch exists with no new bind parameter, the title joined from
      `threads`, the `next_action` `CASE`, `generation_opened_at` as `created_at`, and the
      `NULL` `task_id`/`correlation_id` commented as Phase 4's seam.
- [ ] `AttentionSourceKind::ThreadHandoff`, its `item_href` arm and its `attention_card` label
      exist; the cursor round-trips it.
- [ ] `ThreadHandoffCommand` carries both `expected_version` and `expected_generation`, skips
      `visible_channel_ids` from the fingerprint, and is validated in the domain.
- [ ] `change_thread_handoff` locks, replays, fences on generation **then** version, authorizes with
      the manager / own-row / self-claim rule, writes once and appends exactly one event.
- [ ] `change_source_attributes` refuses `thread_handoff` with a `BadRequest`, and
      `attention_source_events_source_kind_check` is unchanged.
- [ ] The `thread_handoffs_notify_attention` trigger exists, reuses `notify_attention_changed`
      unchanged, and writes no `notifications` row — verified by a test asserting zero
      `notifications` rows for the company after a claim.
- [ ] Cases 1-28 pass; both databases recreated and `.sqlx/` regenerated and committed.
- [ ] `cargo fmt --check`, `SQLX_OFFLINE=true cargo build --all-targets`, `cargo test` and
      `cargo clippy --all-targets -- -D warnings` are green.
