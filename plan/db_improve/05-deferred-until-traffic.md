# Deferred Until There Is Traffic

## Goal

Record the performance concerns from `plan/db_improvements.md` that cannot be decided yet, with
enough detail that whoever picks them up does not have to re-derive the analysis — and with an
explicit bar for when they become actionable. This file also carries the evidence tooling that the
statistics work left unbuilt, so it is the single place where a deferred query decision and the
machinery needed to reopen it are recorded together.

## Why These Are Held

The original audit refused to act on these because the test database had no meaningful cardinality.
Collection has since been turned on — `pg_stat_statements` is preloaded by the derived PostgreSQL
image, the extension is created by `migrations/20260901000000_enable_pg_stat_statements.sql`, the
operator-only "Database query health" panel and `scripts/db-stats.sh` read it, and `docs/deploy.md`
carries the activation-verification and controlled-`EXPLAIN` procedures. What collection cannot do
is manufacture cardinality: **the project is not in production**, so the counters are accruing
against near-empty tables and there is still nothing worth an `EXPLAIN`.

Each item below is a real trade-off rather than an oversight. Acting on any of them from synthetic
data risks paying a permanent cost — a write-amplifying index, a UI contract change — for a problem
that may not exist at real scale, or missing the one that does.

**The bar for picking these up:** either production has run long enough for `pg_stat_statements` to
rank these queries against everything else, or the skewed seeder and workload runner described under
"Evidence Tooling Still To Build" exist and reproduce a distribution someone is willing to defend as
representative. Neither exists today, so real traffic is currently the only evidence path. A seeded
plan is enough to rule a change *out*, never enough to rule one *in*.

Record every plan captured, before and after, in this file.

## 1. Schedule-Run Listing: Correlated Subqueries Per Row

`list_schedule_runs` (`src/adapters/persistence/schedule.rs:661`):

    SELECT thread.id AS thread_id, task.id AS task_id, thread.channel_id, thread.subject,
           task.status AS task_status, task.lock_expires_at,
           (SELECT message.clean_text_body
            FROM thread_messages AS association
            JOIN messages AS message
              ON (message.company_id, message.id) = (association.company_id, association.message_id)
            WHERE association.thread_id = thread.id
              AND message.direction = 'outbound'
            ORDER BY association.created_at DESC, association.id DESC
            LIMIT 1) AS latest_response,
           (SELECT COUNT(*)::bigint
            FROM thread_messages AS association
            WHERE association.thread_id = thread.id) AS message_count,
           thread.created_at, thread.updated_at
      FROM background_tasks AS task
      JOIN threads AS thread ON thread.id = task.thread_id
     WHERE task.task_type = 'scheduled_agent_run'
       AND task.payload->>'schedule_id' = $1
     ORDER BY thread.created_at DESC, thread.id DESC
     OFFSET $2 LIMIT $3

At 15 rows per page (`PAGE_SIZE`, `routes/ui_schedules.rs:246`) this is 30 extra probes per page
load, not thousands — which is why it is deferred rather than urgent.

`message_count` is well served by `thread_messages_thread_created_idx (thread_id, created_at, id)`
(`migrations/20260817000000_init_schema.sql:511`). `latest_response` is the weaker one:
`thread_messages_outbound_thread_idx (thread_id, email_message_id, created_at DESC)
WHERE direction = 'outbound'` (`:515`) puts `email_message_id` between the leading column and the
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

One `background_tasks` index now leads with `updated_at` in second position:
`background_tasks_company_updated_idx (company_id, updated_at DESC)`
(`migrations/20260817000000_init_schema.sql:712`). It was added for the Kanban board's recency arm,
not for the dashboard, and it only partly fits the dashboard's shapes. The other candidates are keyed
on `run_at`, `lock_expires_at`, `wait_expires_at`, or `(company_id, status, created_at DESC, id DESC)`.
Affected queries in `src/adapters/persistence/dashboard.rs`:

- `THROUGHPUT_BODY` (`:130`) — `updated_at >= CURRENT_TIMESTAMP - make_interval(...)` plus a status
  set, scoped by `($1::uuid IS NULL OR company_id = $1)`. Because `company_id` leads the index, the
  System-scope (`NULL`) form cannot use it at all, and the `IS NULL OR` shape is a poor match even
  for the company-scoped form.
- `QUEUE_DEPTH_BODY` (`:199`) — filters and joins on both `created_at` and `updated_at`
- `OUTSTANDING_SQL` (`:289`) — `ORDER BY …, task.updated_at DESC, task.id DESC LIMIT $2`; the index
  supplies the company-scoped sort but carries no `id` tie-break column

The author's own comment at `dashboard.rs:195-198` records that queue-depth leans on
`(company_id, status, created_at DESC, id DESC)` to bound the CTE, not to serve the `updated_at`
filter.

(The related `task_attempts` gap is *not* resolved. That table carries only the
`task_attempts_task_attempt_key UNIQUE (task_id, attempt_number)` btree (`init_schema.sql:816`);
the `started_at` candidate is still at the evidence gate and is tracked in §4.)

**Two options, and the choice is a genuine trade-off.** Widening or adding an index — a bare
`(updated_at)`, or reworking the scope predicate so `(company_id, updated_at DESC, id DESC)` is
usable — is the obvious move, but `background_tasks` takes a write on every claim, lease renewal,
and completion, and `updated_at` changes on each, so such an index is maintained far more often than
one on `created_at`. The alternative is to cache the dashboard snapshot for one tick across
connected tabs: `dashboard_snapshot(company, window)` (`routes/ui_dashboard.rs:259`) is still read
per request, and the stream re-reads every five seconds *per tab* (`TICK`, `ui_dashboard.rs:63`), so
a shared snapshot removes most of the read pressure with no index and no write cost. The 60-second
single-flight cache that already exists in `src/application/services/database_query_health.rs`
covers only the query-health panel, not the snapshot, but it is the pattern to copy.
**Evaluate the cache first** — it scales with operator count rather than data size and may make the
index unnecessary.

If the cache is implemented, it needs a test that company-scoped and operator-wide views stay
isolated: a cached snapshot must never leak one company's rollup into another's view.

**Sensitive to:** the write rate on `background_tasks`, and the number of simultaneous dashboard
viewers — measure both, not just the read plan.

## 3. Offset Pagination on Three List Pages

- `list_company_tasks_page` (`src/adapters/persistence/task/operations.rs:1276`) — `TaskFilterQuery`
  exposes `page`/`limit` (`routes/task.rs:58`), `TaskFilter::offset()` computes `(page-1)*limit`
  (`domain/entities/task.rs:1086`, `DEFAULT_PAGE_SIZE = 50` at `:1054`, `MAX_PAGE_SIZE = 100` at
  `:1055`), pager rendered by `task_pager` (`pages/task_monitor.rs:274`)
- `list_company_outbox_page` (`task/operations.rs:1322`) — `OutboxFilter` at
  `domain/entities/outbox.rs:136`
- `list_schedule_runs` (`schedule.rs:661`) — `routes/ui_schedules.rs:159-178`, `PAGE_SIZE = 15`

All three sort by `(created_at, id)` with a matching-direction `id` tie-breaker, so results are
already stable — no duplicates and no skipped rows across pages. The only defect is that a deep
offset walks and discards rows before returning any.

**The pattern to reuse already exists here.** `src/adapters/persistence/thread.rs` does keyset
pagination with `(timestamp, id)` cursor types — `MessageCursor` over `created_at` and
`ThreadCursor` over `updated_at`, both generated by the `timestamp_id_cursor!` macro in
`src/domain/entities/cursor.rs:71` and `:75` (grep for the macro, not for a struct) — used by
`list_threads_updated_after` (`thread.rs:443`) and `list_messages_after` (`thread.rs:657`), with
resume and tie-break semantics covered by tests at `thread.rs:1326`, `:1387`, and `:1436`. Applying
it is mechanical; these queries are already ordered on exactly the columns the predicate needs.

The cost is not in the SQL. Page numbers are part of the UI contract — query strings, pager links,
SSE URLs — and cursors remove the ability to jump to page N. Prefer converting only the SQL while
keeping the page-number UI, carrying a cursor for next/previous and falling back to offset for a
direct jump.

**Sensitive to:** whether anyone pages deep at all. `deep_pagination_observed` already counts
`1000+` offsets per endpoint and surfaces on the operator dashboard (see "Collection Gaps"); if that
counter stays at zero once there is traffic, close this item without writing code.

## 4. Index Candidates Awaiting Evidence

Two index candidates were deliberately not shipped because no plan could justify them. The reasoning
is recorded in `docs/query-evidence/2026-09-01-thread-index-and-task-attempts.md`; they are listed
here so the workload matrix has a stated target.

- **Partial `email_messages (thread_index)`.** The column exists (`init_schema.sql:476`) with no
  index. The `Thread-Index` predicate was changed to a bounded `text[]` of binary ancestors using
  `= ANY($2)`, which makes a btree access path possible but does not show that one improves the
  complete channel-scoped join.
- **`task_attempts (started_at)`.** Only the `(task_id, attempt_number)` unique btree exists
  (`init_schema.sql:816`), so recent/old attempt windows have no access path of their own.

Neither has passed the evidence gate. Before either is proposed, retain the complete before/after
plans, buffer activity, relation and index sizes, write rate, and window selectivity for the
scope/window matrix, and record them in this file.

**Sensitive to:** `email_messages` and `task_attempts` cardinality and the selectivity of a single
`thread_index` value or `started_at` window — both are indistinguishable from noise at current size.

## Evidence Tooling Still To Build

`pg_stat_statements` ranks normalized statements, but it cannot distinguish a shallow offset from a
deep one, a company scope from a global one, or a short dashboard window from a long one. Neither
can it supply cardinality the database does not have. Closing any item above from anything other
than production traffic requires the tooling below, and none of it exists yet: there is no `[[bin]]`
in `Cargo.toml`, no `src/bin/`, and no `examples/`, `xtask`, or `tools` directory.

### Deterministic skewed seeder

Build a standalone development/benchmark tool that is neither an HTTP route nor included in the
production runtime image. It must refuse production-like hosts and database names and require an
explicit benchmark database.

The tool must accept a scale and a deterministic random seed, print both, and populate every
required parent row plus:

- `email_messages` and `thread_messages`, including a few very long threads and a long tail;
- `background_tasks` with uneven company, status, schedule, creation, and update distributions;
- `task_attempts` with realistic attempts-per-task and recent/old `started_at` windows; and
- scheduled runs and list-page histories deep enough to exercise §1 and §3.

Document the mathematical distribution rather than only the final row count. Load atomically, run
`ANALYZE` after the bulk insert, and print verification queries for largest, median, and long-tail
companies/threads plus window selectivity.

### Workload runner

Seeding alone is insufficient. Add a runner that executes the actual application query shapes over a
fixed matrix, scoped to the four items in this file:

- small, median, and large companies;
- short and long dashboard windows, in both company and System scope;
- short, median, and very long threads;
- shallow, medium, and deep offsets; and
- repeated executions after the initial run.

Test candidate indexes on this disposable database or a representative staging copy. Do not add an
acceptance criterion that forces an index scan: for a low-selectivity predicate or a small relation,
a sequential scan may be the correct plan. The runner must record the parameter *class* it used, not
the values.

### Plan capture

`docs/deploy.md` carries the controlled `EXPLAIN` recipe — reviewed `SELECT` only, read-only
transaction, bounded `statement_timeout` and `lock_timeout`,
`EXPLAIN (ANALYZE, BUFFERS, WAL, SETTINGS, TIMING OFF)`. Each plan saved here must include the
snapshot timestamp, parameter classes (not sensitive values), row counts and selectivity, statistics
age, PostgreSQL version, and the exact candidate index definition. Compare the whole join and sort,
not only the target table's scan node, and capture both custom and generic prepared plans where sqlx
may switch between them.

## Collection Gaps

Bounded, low-cardinality signals capture the parameter classes PostgreSQL normalizes away. Two exist
and are the pattern to follow — `MonitoringService` counters and gauges with `&'static str` labels,
never tracing events carrying request data:

- offset bucket per list endpoint (`0-99`, `100-999`, `1000+`) plus `deep_pagination_observed`
  (`src/domain/monitoring.rs:110-150`), surfaced on the operator dashboard; and
- `active_dashboard_sse_connections` (`routes/ui_dashboard.rs:438-468`, decremented on `Drop`).

Three are still missing:

- dashboard scope class (`company` or `global`) — `DashboardScope` resolves it but records nothing;
- dashboard window class — `DashboardWindow` is passed through `dashboard_snapshot` unrecorded; and
- dashboard snapshot duration, returned working-set size, and whether it was truncated —
  `dashboard_snapshot(company, window)` (`ui_dashboard.rs:259`) is neither timed nor gauged.

Keep label values to fixed sets so they cannot create unbounded cardinality. Never record query
text, bind values, company or user identifiers, message data, or arbitrary request objects. These
signals are what let a deferred decision above be reopened without guesswork; without them, §2 and
§3 cannot be closed from evidence at all.

One reporting gap belongs to `scripts/db-stats.sh`: its index section prints raw `idx_scan` values.
Rows with `idx_scan = 0` must be labelled "not observed since statistics reset", never "unused" —
the counter is relative to the reset time the same snapshot prints.

## Verification Gaps

- No test covers the PostgreSQL startup wrapper. `deploy/postgres/entrypoint.sh` implements the
  strict three-way `DATABASE_SLOW_QUERY_LOGGING_ENABLED` case, but `scripts/tests/` holds only
  `deploy.sh` and `credential-key-rotation.sh` and CI runs no shell syntax or static check. Prove
  unset/`false` omits slow logging, `true` adds the threshold while both parameter limits stay zero,
  and any other value fails.
- CI starts stock `postgres:18.6-bookworm` and configures it with `ALTER SYSTEM`, so the derived
  image and its wrapper are never exercised by the activation probe that follows.
- No test asserts `scripts/db-stats.sh --local` emits every section, filters to the current
  database, and performs no reset or write.
- No test asserts the production image contains only the server binary. The property holds today
  only because no seeder exists; `cargo build --release --locked` would pick up a future `[[bin]]`
  automatically, so the guard must land with the seeder.
- No negative assertion proves the bounded signals carry no identifiers, bind values, message
  content, or query text. Safety is currently by construction only.
- Local development PostgreSQL is Homebrew 16.14 with an empty `shared_preload_libraries`, so the
  query-health panel renders `ExtensionUnavailable` locally while CI and production run 18.6 with
  the module loaded. Any local plan capture is therefore against a different major version than
  production — note it on every artifact, or align the local server first.

## Acceptance Criteria

- No change from this file lands without a captured plan justifying it, recorded here.
- Each item is either implemented with before-and-after plans, or explicitly closed with the
  evidence that made it unnecessary.
- Synthetic plans are explicitly labelled as shape checks and are never the sole justification for
  a production index or a pagination contract change.
- No seeder or workload runner is shipped in the production runtime image, and a test enforces it.
