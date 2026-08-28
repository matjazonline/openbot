# Deferred Until There Is Traffic

## Goal

Record the three performance concerns from `plan/db_improvements.md` that cannot be decided yet,
with enough detail that whoever picks them up does not have to re-derive the analysis — and with
an explicit bar for when they become actionable.

## Why These Are Held

The original audit refused to act on them because the test database had no meaningful cardinality.
That has not changed: **the project is not in production and the database is empty**, so there is
nothing to `EXPLAIN` at all.

Each of the three is a real trade-off rather than an oversight. Acting on any of them from
synthetic data risks paying a permanent cost — a write-amplifying index, a UI contract change —
for a problem that may not exist at real scale, or missing the one that does.

**The bar for picking these up:** `03-capture-query-statistics.md` is in place, and either
production has run long enough for `pg_stat_statements` to rank these queries against everything
else, or the skewed seeder reproduces a distribution someone is willing to defend as
representative. A seeded plan is enough to rule a change *out*, never enough to rule one *in*.

Record every plan captured, before and after, in this file.

## 1. Schedule-Run Listing: Correlated Subqueries Per Row

`list_schedule_runs` (`src/adapters/persistence/schedule.rs:661-687`):

    SELECT t.id AS thread_id, task.id AS task_id, t.channel_id, t.subject,
           task.status AS task_status, task.lock_expires_at,
           (SELECT clean_text_body FROM thread_messages tm
              WHERE tm.thread_id = t.id AND tm.direction = 'outbound'
              ORDER BY tm.created_at DESC, tm.id DESC LIMIT 1) AS latest_response,
           (SELECT COUNT(*)::bigint FROM thread_messages tm
              WHERE tm.thread_id = t.id) AS message_count,
           t.created_at, t.updated_at
      FROM background_tasks AS task
      JOIN threads AS t ON t.id = task.thread_id
     WHERE task.task_type = 'scheduled_agent_run'
       AND task.payload->>'schedule_id' = $1
     ORDER BY t.created_at DESC, t.id DESC
     OFFSET $2 LIMIT $3

At 15 rows per page (`routes/ui_schedules.rs:211`) this is 30 extra probes per page load, not
thousands — which is why it is deferred rather than urgent.

`message_count` is well served by `thread_messages_thread_created_idx (thread_id, created_at, id)`
(migration line 450). `latest_response` is the weaker one:
`thread_messages_outbound_thread_idx (thread_id, email_message_id, created_at DESC)
WHERE direction = 'outbound'` (line 454) puts `email_message_id` between the leading column and the
sort column, so it narrows by `thread_id` but gives no free sort.

**Candidate fix, cheapest first.** Reorder that partial index to
`(thread_id, created_at DESC, id DESC) WHERE direction = 'outbound'`; if that turns
`latest_response` into a single index probe, nothing else is needed. Only if it does not, replace
both subqueries with a `LATERAL` join plus a grouped count. Before reordering, grep for what else
the current index shape serves — `email_message_id` in second position suggests a lookup that
would regress.

**Sensitive to:** messages per thread, and its skew. A few very long threads matter more here than
a uniformly large table.

## 2. Dashboard Time Windows vs. Index Shape

No `background_tasks` index has `updated_at` as a usable column. The existing ones are keyed on
`run_at`, `lock_expires_at`, `wait_expires_at`, or `(company_id, …, created_at DESC, id DESC)`
(migration lines 567-596). Affected queries in `src/adapters/persistence/dashboard.rs`:

- `THROUGHPUT_BODY` (`:130-146`) — `updated_at >= now() - interval` plus a status set
- `QUEUE_DEPTH_BODY` (`:199-218`) — filters and joins on both `created_at` and `updated_at`
- `OUTSTANDING_SQL` (`:289-318`) — `ORDER BY …, task.updated_at DESC, task.id DESC LIMIT $2`

The author's own comment at `dashboard.rs:195-198` records that queue-depth leans on
`(company_id, status, created_at DESC, id DESC)` to bound the CTE, not to serve the `updated_at`
filter.

(The related `task_attempts` gap is *not* held — a table with no indexes at all is wrong on its own
terms and is handled in `01-index-thread-index-and-task-attempts.md`.)

**Two options, and the choice is a genuine trade-off.** Adding an index on `(updated_at)` or
`(company_id, updated_at DESC, id DESC)` is the obvious move, but `background_tasks` takes a write
on every claim, lease renewal, and completion, and `updated_at` changes on each — so that index is
maintained far more often than one on `created_at`. The alternative is to cache the dashboard
snapshot for one tick across connected tabs: the dashboard re-reads every five seconds *per tab*
(`routes/ui_dashboard.rs:56`), so a shared snapshot removes most of the read
pressure with no index and no write cost. **Evaluate the cache first** — it scales with operator
count rather than data size and may make the index unnecessary.

If the cache is implemented, it needs a test that company-scoped and operator-wide views stay
isolated: a cached snapshot must never leak one company's rollup into another's view.

**Sensitive to:** the write rate on `background_tasks`, and the number of simultaneous dashboard
viewers — measure both, not just the read plan.

## 3. Offset Pagination on Three List Pages

- `list_company_tasks_page` (`src/adapters/persistence/task.rs:1783-1822`) — `TaskFilterQuery`
  exposes `page`/`limit` (`routes/task.rs:57-75`), `TaskFilter::offset()` computes
  `(page-1)*limit` (`domain/entities/task.rs:370-415`, `DEFAULT_PAGE_SIZE = 50`,
  `MAX_PAGE_SIZE = 100`), pager rendered at `pages/task_monitor.rs:262-292`
- `list_company_outbox_page` (`task.rs:1829-1868`) — `OutboxFilter` at `routes/ui_outbox.rs:74-90`
- `list_schedule_runs` (`schedule.rs:661-687`) — `routes/ui_schedules.rs:155-172`, `PAGE_SIZE = 15`

All three sort by `(created_at, id)` with a matching-direction `id` tie-breaker, so results are
already stable — no duplicates and no skipped rows across pages. The only defect is that a deep
offset walks and discards rows before returning any.

**The pattern to reuse already exists here.** `src/adapters/persistence/thread.rs` does keyset
pagination with `(timestamp, id)` cursor structs — `ThreadCursor { updated_at, id }` and
`MessageCursor { created_at, id }`, used by `list_threads_updated_after` (`thread.rs:440-470`) and
`list_messages_after` (`thread.rs:648-680`), with resume and tie-break semantics covered by tests
at `thread.rs:1165-1346`. Applying it is mechanical; these queries are already ordered on exactly
the columns the predicate needs.

The cost is not in the SQL. Page numbers are part of the UI contract — query strings, pager links,
SSE URLs — and cursors remove the ability to jump to page N. Prefer converting only the SQL while
keeping the page-number UI, carrying a cursor for next/previous and falling back to offset for a
direct jump.

**Sensitive to:** whether anyone pages deep at all. The first thing to check is production access
logs; if deep pages are never requested, close this item without writing code.

## Acceptance Criteria

- No change from this file lands without a captured plan justifying it, recorded here.
- Each item is either implemented with before-and-after plans, or explicitly closed with the
  evidence that made it unnecessary.
