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

# Carry the discriminator; never re-assert it as a literal

The same rule one column over. The memory tables are keyed on a discriminator plus an id:
`(company_id, provider)` on `memory_provider_connections`, `(provider, remote_database_id)` on
`memory_remote_resource_lifecycles` and `memory_cleanup_jobs`. A query that already has the
discriminator in scope — returned by a CTE, read off the job row, held in
`companies.memory_provider` — and then filters on the literal `'hydradb'` instead compiles, passes
every test, and is wrong the moment a second value exists.

Three shapes, all found in one sweep:

- **The wrong row is updated.** `complete_provisioning` matched the lifecycle on
  `completed_job.provider`, dropped it from `RETURNING`, then updated `... AND connection.provider
  = 'hydradb'`. A second provider's job would flip the first provider's connection to `ready`.
- **A no-op is misread as a lost lease.** That same predicate matching nothing returns no row,
  which the worker reads as "another execution owns this" — so the job retries until its budget is
  gone.
- **Teardown is silently skipped.** `delete_company_with_cleanup` selected lifecycles `WHERE
  provider = 'hydradb'`, so a company holding any other provider's remote data would be deleted
  with no lifecycle retirement and no cleanup job. The remote data is orphaned and nothing left
  behind records that it exists.

A discriminator in a `WHERE`, a `RETURNING`, or a `.bind()` comes from the row you just read or
from a parameter. A literal is acceptable only when you are deliberately scoping to one value, and
then say why in a comment. When a CTE matches on a discriminator, return it and join the outer
statement on it rather than restating the value.

A single-variant enum makes all of this unverifiable. While `MemoryProviderKind` had only
`Hydradb`, every one of these sites was a tautology and the suite was green. Treat "this enum has
one variant today" as a reason for more care in the query, not less, and do not expect a test to
catch you.

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

For remote resources, the execution fence must survive deletion of its owning row. Transactionally
set durable desired state to `absent`, detach the lifecycle row, and enqueue reconciliation before
deleting the owner. Provisioning completion must match the current generation and desired state;
cleanup may accept `404` only after the bounded provider-operation quiescence deadline has passed.

# Encode queue state machines as constraints

Status and lease columns are one state machine. Add database checks that make incoherent states
impossible: an in-flight row has an owner and expiry, while pending/terminal rows do not retain
lease data. Recovery and dashboard queries must temporarily classify legacy in-flight rows with
missing lease data as stalled; a predicate such as `lock_expires_at <= CURRENT_TIMESTAMP` does not
match `NULL`.

Audit and repair existing rows before validating a new constraint. Mirror the strongest existing
queue constraint rather than inventing subtly different semantics for each queue.

When a domain decision is implemented in both Rust and SQL, keep one generated definition or add a
database-backed equivalence matrix that exercises every case. A `debug_assert!` only checks the
path and build modes that happen to execute it, so it is not a sufficient drift guard.

Audited state transitions carry typed reason and actor data explicitly. Never recover an audit
reason by parsing free-text errors or silently substitute a specific reason when none is known;
unclassified transitions remain observably unknown. Notification triggers fire only for material
state changes, not for writes that leave the notified state unchanged.

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

A bounded result set does not imply bounded database work. Apply time and status eligibility meant
to prune retained history before aggregation or large joins, then verify the pruning with
representative `EXPLAIN` output. Replace per-parent query loops with bounded batch reads.
Operational views must cap their working set and visibly report truncation rather than silently
returning partial data.

When an optimization is deliberately deferred pending production evidence, add a named duration or
working-set metric, or a threshold log, that can supply the evidence needed to revisit it.

# 1. Migration

A migration is immutable as soon as it has been applied to any persistent development database.
Refine it with a new timestamped follow-up migration; never edit the applied file or rewrite SQLx
checksums. `20260817000000_init_schema.sql` is the squashed baseline every database has already
applied. The minimal shape is one `ALTER TABLE ... ADD COLUMN`; the multi-statement shape backfills
in an `UPDATE` between the `ADD COLUMN` and the `SET NOT NULL`.

A `NOT NULL` column needs a `DEFAULT` (or an `UPDATE` backfill before `SET NOT NULL`) — existing rows
must come out valid, because production has rows and your dev database mostly doesn't. State the
default in SQL rather than relying on the application to fill it in; the column is read by queries
that predate your change.

Apply it locally **before** step 7:

```sh
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" sqlx migrate run
```

Then confirm the shape you think you wrote is the shape that exists: `psql "$DATABASE_URL" -c '\d <table>'`.

# 2. Nothing but identifiers goes into a durable payload

`background_tasks.payload` used to hold whole `Company`, `Channel`, `Thread` and `Message` values,
read back with `serde_json::from_value::<InboundIngestResult>`. That made every queued row a
snapshot of the domain model: a field added without a serde default made every already-queued task
undeserializable — silently, since the task did not error, it simply stopped re-hydrating — and a
task that *did* re-hydrate replayed configuration as it was hours earlier.

It now carries `InboundTaskPayloadV1`: company, channel, thread, source message, correlation, and
the three delivery facts no row holds (hop count, traced channels, reply-delivery choice). The
worker reloads everything else with tenant-scoped queries. The version tag is structural, so a
payload written by a deployment this process does not know fails to decode rather than being read
as V1 with defaulted fields.

**So: do not add an entity, a parsed provider message, or raw provider content to a payload.** If a
worker needs a fact, either store it on a row and reload it, or state why it is a property of the
delivery rather than of the message and add it to the versioned payload deliberately.

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
- **Leave no claimable residue.** A test that creates a globally claimable queue row must complete
  it, delete its owner when safe, or move it beyond the claim horizon before releasing the test
  guard. A passing assertion is insufficient if the test leaves due work that another test can
  claim. Deterministically sort the test row first, verify its stable identifier after claiming,
  and return any accidentally claimed foreign row to its prior claimable state.
- **Establish the baseline before calling a failure pre-existing.** Because the fixtures share one
  database, a test that leaves residue fails *other* tests, in other files, on later runs. The
  symptom — a different set failing each run, with the occasional clean one — is indistinguishable
  from inherited flake, and "this suite is just flaky" is the wrong conclusion almost every time.
  `git stash` and run the same filter three or four times before attributing anything to anyone.
  A deterministic green baseline means the flake is yours, and the cause is usually the residue
  rule above: a new test left one `pending` `memory_provisioning_jobs` row, `claim_provisioning_job`
  is table-wide, and three unrelated tests failed intermittently because of it.

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
