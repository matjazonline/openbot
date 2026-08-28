# Index Thread-Index Lookups and Task Attempts

## Goal

Remove two index gaps that are wrong at any table size, so neither depends on production
statistics: an Outlook thread-stitching query that no index can serve in its current form, and a
queue table that carries no indexes at all.

## Current Risk

### Thread-index lookup cannot use an index

`find_thread_by_thread_index` (`src/adapters/persistence/thread.rs:430-458`) resolves Outlook
thread stitching on inbound mail, used when `In-Reply-To`/`References` fail:

    {THREAD_SELECT}
    JOIN thread_messages tm ON tm.thread_id = t.id
    JOIN email_messages em ON em.id = tm.email_message_id
    WHERE t.channel_id = $1
      AND em.thread_index IS NOT NULL
      AND $2 LIKE em.thread_index || '%'
    ORDER BY length(em.thread_index) DESC, tm.created_at DESC
    LIMIT 1

`email_messages.thread_index` has no index of any kind — the only occurrence of the name in
`migrations/20260817000000_init_schema.sql` is the column declaration at line 415. Adding a plain
btree would not help either: the wildcard is on the *column* side, so there is no range to scan.
This is a full scan by construction, not by cardinality, and it runs per inbound message.

### `task_attempts` has no indexes

`migrations/20260817000000_init_schema.sql:649-671` declares the table with a primary key on `id`
and `task_attempts_task_attempt_key UNIQUE (task_id, attempt_number)`, and zero `CREATE INDEX`
statements. Every other queue table in the schema has purpose-built indexes.

Three dashboard queries filter it by `attempt.started_at >= CURRENT_TIMESTAMP - make_interval(...)`:
`LATENCY_BODY` (`src/adapters/persistence/dashboard.rs:159-178`), `ATTEMPT_STATS_SQL` (`:240-258`),
and `RETRY_RATE_BODY` (`:261-278`). The dashboard re-reads on a five-second tick per connected tab
(`routes/ui_dashboard.rs:56`), so this is three sequential scans of the attempt
ledger every five seconds per operator tab.

## Design

Turn the prefix match into an equality match against a computed candidate set. A MAPI
`Thread-Index` is base64 of a 22-byte header plus one 5-byte block per reply, so every ancestor
index is derivable from the incoming value without touching the database.

- `ThreadIndex` (currently a bare `string_newtype!` at
  `src/domain/entities/value_objects.rs:126`) gains a method returning the ancestor chain:
  base64-decode, take byte prefixes of length 22, 27, 32, … up to the full length, re-encode each
  as the base64 string form actually stored in the column.
- It returns an empty chain for anything that is not valid base64 or is shorter than 22 bytes.
- The query predicate becomes `em.thread_index = ANY($2)` over that array, keeping
  `ORDER BY length(em.thread_index) DESC, tm.created_at DESC LIMIT 1` so longest-ancestor-wins
  semantics are unchanged.
- When the chain is empty, fall back to the existing `LIKE` form, so malformed or non-Outlook
  values behave exactly as they do today.
- Add a partial index on `thread_index`, matching the style of `email_messages_in_reply_to_idx`
  (migration line 424).
- Add an index on `task_attempts (started_at)`.

Both indexes are edited directly into `migrations/20260817000000_init_schema.sql` — the database is
empty and the repo already squashes migrations (commit `4968194`), so there is no reason to carry
an additive migration into launch.

The wider dashboard index question stays out of scope; see `05-deferred-until-traffic.md`. The
point of the `task_attempts` index is that a table with no indexes is a defect on its own terms.

## Implementation Steps

1. Add the ancestor-chain method to `ThreadIndex` in `src/domain/entities/value_objects.rs` with
   unit tests over real captured headers.
2. Rewrite the predicate in `find_thread_by_thread_index`
   (`src/adapters/persistence/thread.rs:430-458`) to bind the candidate array, keeping the `LIKE`
   fallback branch for an empty chain.
3. Add to `migrations/20260817000000_init_schema.sql`, beside the existing index blocks:
   - `CREATE INDEX email_messages_thread_index_idx ON email_messages (thread_index)
      WHERE thread_index IS NOT NULL;`
   - `CREATE INDEX task_attempts_started_at_idx ON task_attempts (started_at);`
4. `./scripts/reset-db.sh --all`, then regenerate `.sqlx` metadata for the changed query per
   `src/AGENTS.md`.

## Tests

- Ancestor derivation over a parent, a first reply, and a nested reply from real captured
  `Thread-Index` headers.
- Ancestor derivation returns empty for a non-base64 value and for one shorter than 22 bytes.
- Persistence: a nested index still resolves to the thread created by its grandparent.
- Persistence: a malformed index takes the `LIKE` fallback and returns the same row it does today.
- Schema replays cleanly from scratch against an empty database.

## Acceptance Criteria

- Thread stitching returns identical results to the current implementation on every existing test.
- The stitching query can use an index; no query path scans `email_messages` to match a prefix.
- `task_attempts` carries an index covering the dashboard's `started_at` window filters.
