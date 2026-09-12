# Bounded message-to-task matching

Captured 2026-09-12 on local PostgreSQL 16.14, in a disposable database. These are synthetic
correctness and query-shape checks, not production latency measurements; CI and production use
PostgreSQL 18.6. The user explicitly authorized this lookup and its supporting index to fix lost
task matches while keeping reads bounded. This does not close the unrelated traffic-dependent
index decisions in `plan/db_improve/05-deferred-until-traffic.md`.

## Problem and preserved behavior

The old query fetched the newest 800 candidate tasks across an entire displayed message page.
Rust then preferred an exact source-message match, or a main run sharing the correlation. More
than 800 newer follow-ups could evict both correct answers before that preference was applied.
The overflow regression was run against the old implementation and failed with the wrong task ID.

The replacement pairs the canonical ID and correlation of each displayed message using aligned
arrays. Two lateral probes return at most one task for that message: first the exact source match,
then, only if absent, the best correlation match. Both paths retain company and thread scope.
Fallback ordering remains main run first (`email_agent_dispatch` or `scheduled_agent_run`), newest
creation time second, and ascending UUID for ties, preserving the previous stable-sort behavior.

The page limit stays 200. An explicit guard rejects an oversized input to the task lookup. The
query returns at most 200 rows, with at most two task lookups per displayed message, in one round
trip. It does not impose a candidate-history limit that can discard the winner. The index supports
first-match probes; PostgreSQL still chooses the physical plan, as detailed below.

## Fixture and statistics

The capture used one company and thread with 11,202 tasks:

- one older exact-source task with a different correlation;
- one main run and 1,200 newer follow-ups on one hot correlation;
- 10,000 tasks on separate, unrelated correlations.

The three input messages exercised exact matching, hot-correlation fallback, and no match. The
old candidate predicate selected 1,202 of 11,202 tasks before its limit. Statistics were collected
with `ANALYZE background_tasks` immediately after loading. A subsequent capture also ran
`VACUUM (ANALYZE) background_tasks` to model retained history with visibility-map coverage. The
many singleton correlations make the average correlation cardinality small despite the hot
correlation. This matters when testing
parameterized plans; a uniform fixture did not expose the planner's preference for the old index.

Relation sizes from `pg_relation_size`, with `pg_class.reltuples = 11202` for the task table:

| Relation | Bytes |
| --- | ---: |
| `background_tasks` heap | 4,833,280 |
| Existing `background_tasks_company_source_message_key` | 180,224 |
| Existing `background_tasks_correlation_idx` | 1,204,224 |
| New `background_tasks_thread_correlation_match_idx` | 1,490,944 |

## Before and after plans

Captured with `EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF)` on prepared statements, once with
`plan_cache_mode = force_custom_plan` and once with `force_generic_plan`. Default scan/sort planner
settings were used. The excerpts below omit timings and random fixture identifiers.

The old custom plan read 800 rows through `background_tasks_company_created_idx`, filtered by
thread and the source/correlation predicate, and stopped. It hit 34 shared buffers but returned
the wrong candidate set. The old generic plan sorted the entire matching history:

```text
Limit (actual rows=800 loops=1)                     Buffers: shared hit=62
  Sort (actual rows=800 loops=1)
    Sort Key: created_at DESC, id DESC
    Bitmap Heap Scan (actual rows=1202 loops=1)
      Filter: company_id = $1 AND thread_id = $2
      BitmapOr
        Bitmap Index Scan background_tasks_company_source_message_key (rows=1)
        Bitmap Index Scan background_tasks_correlation_idx (rows=1201)
```

The replacement query without the new index read the hot correlation and sorted it before
`LIMIT 1`. An initial expression index without `INCLUDE (task_type)` still selected the old
correlation index and sort. Including the expression's input enabled an index-only projection;
both final custom and generic plans used this shape:

```text
Nested Loop Left Join (actual rows=3 loops=1)
  Nested Loop Left Join (actual rows=3 loops=1)
    Function Scan on rendered (actual rows=3 loops=1)
    Limit (actual rows=0 loops=3)
      Index Scan background_tasks_company_source_message_key
        Index Cond: company_id = $1 AND source_message_uuid = rendered.canonical_id
        Filter: thread_id = $2
  Limit (actual rows=0 loops=3)
    Result
      One-Time Filter: task.id IS NULL
      Index Only Scan background_tasks_thread_correlation_match_idx (actual rows=0 loops=2)
        Index Cond: company_id = $1 AND thread_id = $2
                    AND correlation_id = rendered.correlation_id
        Heap Fetches: 0
```

The original generic capture hit 14 shared buffers; the custom capture hit nine and read five.
The repeated capture after an explicit vacuum hit 14 shared buffers in both modes. The displayed
zero-row averages are rounded across loops: one exact probe and one fallback probe each found a
task. The fallback index ran twice, skipping the message with an exact match. Neither plan sorted
retained history. Heap fetches depend on visibility-map state; zero is not a runtime guarantee.

The freshly loaded fixture also exposed a limitation: before vacuum established visibility-map
coverage, both plan modes could prefer the old correlation index followed by a sort, even with
the new covering index. The three-message capture read 1,201 hot-correlation rows; a full-page
test repeated that scan for its correlated messages. Vacuuming the same data restored the ordered
index-only probes in both modes. The index and `LIMIT 1` therefore bound returned data and support
efficient lookup, but do not guarantee a hard ceiling on physical database work under every
statistics/visibility state. Correctness is independent of which plan PostgreSQL selects.

## Index and regression coverage

The additive migration `20260912010000_index_thread_task_matches.sql` creates:

```sql
CREATE INDEX background_tasks_thread_correlation_match_idx
    ON public.background_tasks (
        company_id, thread_id, correlation_id,
        (task_type IN ('email_agent_dispatch', 'scheduled_agent_run')) DESC,
        created_at DESC, id ASC
    )
    INCLUDE (task_type)
    WHERE thread_id IS NOT NULL;
```

Exact matching continues to use the existing company/source-message unique key. The old correlation
index remains because other queries use it. The new index costs storage and write maintenance on
thread-associated tasks. It is a normal transactional index build and can block writes to the task
table while the migration runs; deployment timing must account for table size.

`src/adapters/persistence/thread/task_lookup_tests.rs` covers:

- older exact and main-run matches behind more than 800 newer follow-ups, through all three readers;
- main-run preference, creation-time ordering, deterministic UUID ties, and unmatched messages;
- company and thread isolation for both lookup paths, plus empty input;
- a full 200-message input over the same 11,202-task distribution after explicit vacuum, checking
  result cardinality, the matching fallback index, absence of a history sort, and skipped fallback
  for an exact match.

Run the database-backed regression with `cargo test --locked --offline --lib task_lookup`, setting
`DATABASE_URL` to a local test database. The fixtures create and drop isolated migrated databases.
The plan test explains the production SQL constant itself with `ANALYZE, BUFFERS, FORMAT JSON`.
It guards this selective lookup's intended plan, not the universal use of indexes on small tables.

Validation on 2026-09-12: all four new regressions passed; the full database-backed library suite
passed with 1,485 tests and 22 ignored. All migrations applied to a fresh disposable database and
the stored index definition was inspected. SQLx metadata regeneration produced no cache changes.
Formatting, offline compilation of all targets, and Clippy with warnings denied passed.
