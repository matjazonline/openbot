These rules apply whenever you change the **model**: adding, renaming, retyping or removing a field
on a persisted domain entity (`src/domain/entities/*`), or changing the table behind one. They are
in addition to `src/AGENTS.md`, which still governs style — read the sqlx section there too. The
last section here, on SQL style, applies to any query you touch in this directory, model change or
not.

A one-field change is never one edit. It lands in seven places, and the compiler only finds five of
them. Work the list in order; the two it cannot find are steps 2 and 7.

# Preserve invariants at the database boundary

Application checks are useful error messages, not substitutes for constraints. If a relationship
is tenant-scoped, the tenant identifier must participate in the foreign key. Do not model two facts
such as `(company_id -> companies)` and `(channel_id -> channels)` when the real invariant is
`(company_id, channel_id) -> channels(company_id, id)`. The same applies to join tables: every side
must be proven to belong to the same tenant, rather than independently referencing valid rows.

Before adding a composite foreign key:

- query for existing mismatches and decide how they will be repaired;
- add the required composite `UNIQUE` key on the referenced table when necessary;
- use an additive migration and validate compatibility before removing weaker constraints; and
- add a rollback-only or transaction-scoped database test that demonstrates the invalid
  cross-tenant row is rejected.

Keep tenant identifiers in writes even when they appear derivable. They make scoping explicit and
support composite constraints; derivability alone is not a reason to drop them.

# Multi-step work needs a durable unit of execution

Advancing a cursor, disabling a one-off schedule, creating domain rows, and enqueueing work in
separate transactions is not a reliable workflow. An ordinary `Err` cleanup cannot recover from a
process crash between commits. For work that crosses transactions:

- create a durable run/idempotency record in the same transaction that claims or advances the
  source record;
- give each logical occurrence a stable unique key (for schedules, the schedule plus logical
  slot), not a fresh key generated on every retry;
- make every materialization step resumable and idempotent from that record; and
- treat partial thread/message/task creation as recoverable state, not as something a best-effort
  release function can undo.

When the state changes truly must agree, keep them in one transaction. When they cannot fit in one
transaction, persist the workflow state and resume it.

# Fence leases to an execution, not merely a row

Any claimed/reclaimable operation must prevent a stale worker from completing the replacement
worker's execution. Store a claim generation or execution UUID and require it in every renewal,
completion, failure, and attempt-ledger update. A task id plus attempt number is insufficient when
an expired attempt row can be reopened; worker id alone is also insufficient if worker ids may be
reused.

Preserve the existing atomic claim shape: one `UPDATE ... FROM` selection using `FOR UPDATE SKIP
LOCKED`, deterministic ordering, and a bounded limit. Queue claims are intentionally global rather
than tenant-scoped. Do not add tenant filtering to worker claims unless the worker architecture is
being deliberately redesigned.

# Encode queue state machines as constraints

Status and lease columns are one state machine. Add database checks that make incoherent states
impossible: an in-flight row has an owner and expiry, while pending/terminal rows do not retain
lease data. Recovery and dashboard queries must temporarily classify legacy in-flight rows with
missing lease data as stalled; a predicate such as `lock_expires_at <= CURRENT_TIMESTAMP` does not
match `NULL`.

Audit and repair existing rows before validating a new constraint. Mirror the strongest existing
queue constraint rather than inventing subtly different semantics for each queue.

# Preserve the existing correctness guards

Do not weaken these patterns while refactoring adjacent persistence code:

- completion, failure, payload replacement, and lease renewal stay conditional on the current
  worker owning a live lease;
- approval consumption stays atomic, status-checked, and expiry-checked;
- approval, outreach, task, and outbox changes that must agree stay transactionally grouped;
- message deduplication verifies the canonical content hash before reusing an existing message id;
- dynamic list filters remain bound parameters (for example through `QueryBuilder`), never string
  interpolation; and
- password and confirmation material remains one-way hashed.

When a proposed simplification removes one of these predicates or transaction boundaries, assume
it changes correctness until a concurrency or adversarial database test proves otherwise.

# Treat persisted JSON as untrusted, versioned input

Never `expect` a JSONB value from the database to deserialize. Manual SQL, older application
versions, and imports can all create shapes the current Rust type does not accept. Convert
fallibly, attach row/type context to the `AppError`, and let the caller decide whether one bad row
should fail a request or be quarantined.

Inventory existing shapes before adding a JSON constraint. If the format is stable enough to
constrain, include an explicit version/discriminator and roll the constraint out compatibly. This
is separate from the `#[serde(default)]` rule below: defaults preserve old valid payloads; fallible
decoding protects the process from invalid ones.

# Secrets need their own persistence boundary

Provider credentials must not be ordinary plaintext fields selected into general-purpose domain
entities. New secret storage must use application-boundary envelope encryption (or the project's
designated KMS), record a key version, and expose narrow credential-specific reads. List/detail
projections should omit ciphertext as well as plaintext unless the caller explicitly needs the
secret.

A credential migration requires staged compatibility: support legacy reads, backfill encrypted
values, switch writers/readers, remove plaintext, and rotate the external credentials. Do not log,
serialize into durable task payloads, derive debug output from, or return secret material in broad
entities.

# Change performance only from production evidence

Do not add an index or rewrite pagination solely because a query looks expensive on fixtures.
Collect representative `EXPLAIN (ANALYZE, BUFFERS)` output plus table and index statistics first.
Pay particular attention to correlated subqueries per result row, expressions with the parameter
on the left side of `LIKE`, time-window filters unsupported by index prefixes, and large offset
pagination. Record the before/after plan with the change.

Keep cursor ordering deterministic with a unique tie-breaker (normally timestamp plus UUID). Offset
pagination is acceptable for bounded administrative pages; replace it only when observed offsets
and plans justify the API change.

# 1. Migration

New timestamped file in `migrations/`, never an edit to an installed one —
`20260817000000_init_schema.sql` is the squashed baseline every database has already applied, so
editing it changes a checksum sqlx will reject. The minimal shape is one `ALTER TABLE ... ADD
COLUMN`; the multi-statement shape backfills in an `UPDATE` between the `ADD COLUMN` and the
`SET NOT NULL`.

A `NOT NULL` column needs a `DEFAULT` (or an `UPDATE` backfill before `SET NOT NULL`) — existing rows
must come out valid, because production has rows and your dev database mostly doesn't. State the
default in SQL rather than relying on the application to fill it in; the column is read by queries
that predate your change.

Apply it locally **before** step 7:

```sh
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" sqlx migrate run
```

Then confirm the shape you think you wrote is the shape that exists: `psql "$DATABASE_URL" -c '\d <table>'`.

# 2. The entity field needs `#[serde(default)]` — this is load-bearing

Entities are not only rows. `Company`, `Channel`, `Thread` and `Message` are serialized whole into
the `background_tasks.payload` JSONB column (`durable_ingest_payload`, `thread/mod.rs`) and read back
with `serde_json::from_value::<InboundIngestResult>` in `task_worker.rs`.

**A new field without a serde default makes every already-queued task undeserializable.** The task
does not error loudly; it fails to re-hydrate, and the work silently stops happening. Nothing in the
type system or the test suite catches this.

```rust
#[serde(default = "enabled_by_default")]   // a non-`Default::default()` default
pub enabled: bool,

#[serde(default)]                          // when the type's own default is right
pub alias_slugs: Vec<ChannelSlug>,
```

Same rule for anything reachable from a durable payload — `BounceInfo.disabled_slugs` needed it for
exactly this reason. Removing or renaming a field is the mirror image: old payloads still carry the
old key, so keep it deserializable (or migrate the stored JSON) rather than assuming a clean cutover.

# 3. Row struct and queries move together

For the table's file in this directory, all five of these change or the column is silently dropped:

- the `*Db` struct (`ChannelDb`, `ThreadDb`, `BackgroundTaskDb`, …) — `sqlx::FromRow`, DB-shaped types
  (`String`, `Vec<String>`), not the newtypes;
- the `From<*Db> for <Entity>` impl, which is where newtype conversion belongs;
- the shared `SELECT` const where one exists (`CHANNEL_SELECT`, `THREAD_SELECT`, `MESSAGE_SELECT`) —
  every read path `format!`s a `WHERE` onto it, so one edit fixes all of them;
- the `INSERT` column list, and
- the `UPDATE` `SET` list.

The runtime-query files bind positionally. The column list order and the `.bind()` chain order are
one fact expressed twice, and adding a column in the middle renumbers every `$n` after it. Append
rather than insert, and re-read the whole statement after editing — a swapped pair of same-typed
binds compiles, passes a green `cargo test`, and writes the wrong data.

# 4. Don't grow the positional parameter list — pass a struct

A persistence `create`/`update` that already takes six-plus arguments does not get a seventh. Convert
it to one named params struct; `ChannelWrite` in `src/application/use_cases/channel.rs` is the model
to copy, and it also carries the normalization (`ChannelWrite::normalize`) so every entry point
stores the same shape.

This is the `src/AGENTS.md` "No flag parameters" / "Name your tuples" rule at the persistence
boundary. Adding a bool beside an existing bool is the specific mistake: `create(.., true, false)`
compiles with the arguments swapped.

# 5. Trait impls live outside this directory

Changing a `*Persistence` trait signature breaks every impl, and most of them are hand-written mocks
in test modules elsewhere. Find them before you start, so the size of the change is not a surprise:

```sh
grep -rn "impl ChannelPersistence for" src/
```

`ChannelPersistence` has seven, `CompanyPersistence` nine. They are spread across
`adapters/smtp/server.rs`, `adapters/http/routes/webhooks/sendgrid.rs`,
`application/services/{task_worker,outreach_tool}.rs` and the various `mod tests`.

# 6. Struct literals in fixtures

A new entity field breaks every `Entity { .. }` literal — one `Channel` field hit 44 of them across
14 files. The compiler enumerates these, so drive the edit off `cargo check --all-targets`; they are
mechanical. If a fixture repeats more than a couple of times, hoist it into a helper rather than
adding the field 44 times again next release.

# 7. Regenerate the sqlx cache — and know whether it can tell you anything

```sh
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" \
  cargo sqlx prepare -- --all-targets
```

Run it after any SQL change and commit the result, per `src/AGENTS.md`. But know what it does and
does not verify here, because this directory is split:

- **Compile-time macros** (`query!`, `query_as!`) — `agent.rs`, `company.rs`, `company_invite.rs`,
  `user.rs`. These are checked against the live schema, produce `.sqlx/` entries, and will fail the
  build on a stale cache.
- **Runtime queries** (`sqlx::query`, `sqlx::query_as::<_, T>`) — `approval.rs`, `channel.rs`,
  `task.rs`, `thread.rs`. These are **plain strings**. Nothing checks them at compile time, they
  generate no `.sqlx/` entries, and a wrong column name is a runtime error only.

So an empty `.sqlx/` diff after touching `channel.rs` or `task.rs` is expected and proves nothing.
Run the command anyway — the macro files share the database and a stale cache there either fails the
Fly.io build or, worse, keeps building the old query shape.

# Verifying against a real database

The DB-backed tests in this directory open with:

```rust
let Some(pool) = test_pool().await else { return };
```

`test_pool` (`test_support.rs`) resolves `TEST_DATABASE_URL`, or else redirects `DATABASE_URL` onto
its `_test` sibling, and applies the migrations once per test binary. **Never point these tests at
the development database.** The queue operations they exercise are unscoped on purpose —
`claim_pending_tasks` and `claim_outbox_emails` sweep every row, because that is what a worker does —
so against your dev database a test run competes with any `cargo run` server polling the same queues
twice a second, and `reap_expired_outbox_leases` can mark real deliveries failed.

Create the database once:

```sh
createdb mail_agents_test
```

Then every model change must be run as:

```sh
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo test --lib
```

The command is unchanged; it now lands on `mail_agents_test`. **Run it with the variable set.**
`cargo test` does not read `.env` — only `main.rs` does, at runtime — so an unexported `DATABASE_URL`
is the normal accident, not an exotic one.

Both ways of getting it wrong now fail loudly rather than skipping:

| `DATABASE_URL` | Result |
|---|---|
| set, reachable | the DB-backed tests run |
| set, unreachable | panic naming the fix |
| unset | panic naming the fix |
| unset, with `ALLOW_MISSING_DATABASE_URL=1` | skip, deliberately |

The skip used to be the default, on the reasoning that it let CI run without a database. There is no
CI here, and the silence cost real bugs: the three `thread.rs` tests below that named columns no
migration creates, plus a content-hash mismatch that broke every internal delegation hop and a
`WHERE id = $9` placeholder collision, all sitting behind a green suite. A skipped test is *counted
as passing* and the suite reports the same total either way, so nothing about the output reveals it.
Use `ALLOW_MISSING_DATABASE_URL=1` when you genuinely want the non-DB tests only.

This runs parallel and should stay that way. Every fixture here still shares one database — the test
one — so a new DB-backed test has to be written to tolerate that:

- **Suffix every database-wide unique value** — company slugs, usernames, emails — with
  `Uuid::new_v4().simple()`. Channel slugs are unique per company, so those may stay fixed. A
  `duplicate key value violates unique constraint` failure that vanishes on rerun is a fixture that
  skipped this.
- **Never assert on a global query's totals.** `claim_pending_tasks` polls the entire
  `background_tasks` queue with no company filter, because that is what a real worker does. Assert
  that *your* row was claimed exactly once (filter by id), not that the queue returned one row, and
  keep the claim limit small so the test steals as little as possible from whatever else is running.
- **Do not assert that your row is still `pending`, or still unclaimed.** That asserts no unscoped
  claim ran in between, which is not a property this code has. Either drop the status from the
  assertion — `approval_lookup_is_scoped_and_token_is_consumed_once` counts its notification by
  `idempotency_key` alone — or put the row out of reach first: `claim_outbox_emails` only takes rows
  whose `available_at` has arrived, so pushing that forward excludes it from every claim set without
  touching the columns under test.
- **If you must call a real claim, make it deterministic and give back what is not yours.** Both
  claims order by their due column, so backdating your own row sorts it ahead of every neighbour and
  a `LIMIT 1` then takes precisely it. Where a claim can still catch a foreign row —
  `only_one_worker_queues_an_outbound_send` runs two concurrent claims, so the second is guaranteed
  to — release it back to `pending` with its worker and lease cleared. A test that leaves a
  five-minute lease on someone else's row makes *their* test fail, several files away.

If a test still cannot be isolated, run just that test rather than reaching for `--test-threads=1`
for the whole suite — serialising everything hides the next isolation bug instead of surfacing it.

This blind spot is not hypothetical: `find_outbound_reply_excludes_outreach_outbox_messages` in
`thread.rs` had three INSERTs naming columns that no migration creates (`task_outreaches.target_count`,
`email_outbox.channel_id`, `task_outreach_targets.id`) and failed unnoticed because CI never sets
`DATABASE_URL`.

Finally, extend the round-trip assertion rather than only adding the field. A model change is not
done until a test writes the new value, reads it back, and asserts it survived — create, then update,
then re-read, as `postgres_channel_persistence_works` does for `enabled`. A field that is bound on
`INSERT` but forgotten in `UPDATE` passes every other check.

# Write SQL for the reader, not for the character count

The runtime queries in step 7 have no compile-time check, so the *reader* is the type system.
Optional syntax that a human has to reconstruct is not worth the keystrokes it saves.

**Always write `AS` for a table alias.** `UPDATE email_outbox outbox` (`task.rs:854`) reads as two
table names before it reads as one aliased one; `UPDATE background_tasks AS task` (`task.rs:1171`) —
the same query shape, three hundred lines away — does not. Same rule for `FROM` and `JOIN` targets
and for CTE column lists. Column aliases in the `SELECT` list already use `AS` throughout
(`em.sender::text AS sender`, `CASE ch.access_mode ... END AS participant_emails`); table aliases get
the same treatment.

**Alias to a word, not an initial.** `FROM threads t` / `JOIN email_messages em ON em.id =
tm.email_message_id` forces the reader to hold a decoder ring for the length of the query, and the
ring is per-query — `a` is `agents` in `agent.rs:100` and `accepted` in `company_invite.rs:196`. The
anti-examples are `t`/`tm` (`thread.rs:129,138`), `ch`/`cs`/`cp`/`ca` (`channel.rs:57-83`) and
`i`/`a`/`m` (`company_invite.rs`). The model to copy is `task.rs`: `task_outreaches AS outreach`,
`email_outbox AS outbox`, `task_outreach_targets AS target`.

These aliases leak. The shared `SELECT` consts from step 3 are `format!`ed into a `WHERE`/`JOIN`
clause at thirteen call sites that spell the alias themselves —
`format!("{CHANNEL_SELECT} WHERE ch.id = $1")` — so renaming one is a cross-file edit. Do it when
you're already changing that query, not as a sweep.

**Name the columns instead of `SELECT *`.** `SELECT * FROM human_approvals` (`approval.rs:244, 269,
321, 477`) feeds a `query_as::<_, ApprovalDb>`, so the row struct and the table are coupled by column
name and order with nothing stating either. A shared column const is the fix — `task.rs` already does
this with `OUTBOX_COLUMNS`, and step 3 makes that const the one place you edit per model change.

None of this is licence to reformat a query you aren't otherwise touching, and it is not an argument
against genuinely dense SQL: a `CASE` expression or a window function that carries real logic stays.
The target is syntax that is optional *and* load-bearing for comprehension.
