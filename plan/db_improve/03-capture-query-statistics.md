# Capture Query Statistics Safely

## Decision

Keep this work. The database will be cleared, which removes backfill concerns and makes this the
best time to enable collection before data and traffic return. It does not remove the need for an
evidence path: the Thread-Index and `task_attempts.started_at` index candidates recorded in
`docs/query-evidence/2026-09-01-thread-index-and-task-attempts.md`, along with the still-current
items in `05-deferred-until-traffic.md`, cannot be decided from empty fixtures.

The solution is deliberately layered:

1. the operator-only System dashboard is the monitoring surface for collection health and the
   highest-cost normalized queries;
2. `scripts/db-stats.sh` is the detailed statistics investigation surface;
3. privacy-safe application signals capture parameter classes PostgreSQL normalizes away;
4. controlled `EXPLAIN` is the plan investigation surface for a reviewed query; and
5. a deterministic skewed workload supplies pre-traffic shape checks only.

Synthetic evidence can rule a proposed optimization out. Only representative production traffic,
or a staging copy whose distribution is defensible as representative, can rule one in.

## Monitoring and Investigation Surfaces

The global `/ui/dashboard` is the routine monitoring surface. Only an address in
`OPERATOR_EMAILS`, while viewing System scope, can see “Database query health.” The panel shows the
statistics reset time, deallocations, effective collection and logging configuration, and two
deterministic top-five rankings: cumulative execution time and weighted mean execution time with at
least five calls. It deliberately does not invent latency or I/O thresholds before representative
traffic supplies a defensible baseline.

The dashboard is not a query console and exposes no JSON API. Normalized SQL is HTML-escaped,
bounded to 16 KiB, and hidden inside an expandable block; dashboard HTML, fragments, and event
streams use `Cache-Control: private, no-store`. A statistics failure is rendered as an operator-safe
category and does not suppress the queue, runtime, or machine panels. Successful readings are
shared across tabs for 60 seconds with single-flight refresh; failures are retried after 15 seconds.

Use `scripts/db-stats.sh` for an investigation snapshot and the controlled `EXPLAIN` procedure
below for a reviewed statement. Those surfaces carry the broader table/index context and plan
details that do not belong in continuous browser monitoring. Their output is operationally
sensitive and must be reviewed before it is saved or shared.

## Current State

- `runtime_metric_samples` is a process and pool heartbeat. It records machine resource use,
  database acquisition latency, pool occupancy, and memory-provider counters, but no query
  fingerprint, plan, per-query timing, or working-set size.
- `MonitoringService` has tracing and an in-memory store but no Prometheus or OpenTelemetry export.
  Deep-pagination observations and active dashboard streams therefore appear as clearly labelled,
  per-process since-boot signals rather than durable or multi-instance evidence.
- The derived PostgreSQL image preloads `pg_stat_statements`; an additive migration creates the
  extension and CI starts the pinned production PostgreSQL version with the library loaded.
- `scripts/db-stats.sh` reports `pg_stat_statements`, table/index statistics, relation sizes, and
  reset metadata for local or Fly investigations.
- Production SQL is available only through the database machine over Fly's private network.
- `background_tasks_company_updated_idx` now exists. Do not carry forward the stale statement in
  `05-deferred-until-traffic.md` that no usable `updated_at` index exists; refresh the unresolved
  query inventory before generating seed data.
- `pg_stat_statements` normalizes constants and bind values. It can rank a parameterized list query,
  but it cannot reveal whether requests used a shallow or deep offset, company or global scope, or
  a short or long dashboard window.

## Safety and Configuration Rules

### Always-on aggregate statistics

Load `pg_stat_statements` on every database start:

```text
-c shared_preload_libraries=pg_stat_statements
-c compute_query_id=on
-c pg_stat_statements.track=top
-c pg_stat_statements.track_utility=off
```

`top` captures statements sent by sqlx without adding nested trigger and PL/pgSQL noise. Leave
`pg_stat_statements.track_planning` off initially; it can add measurable contention. Do not put the
preloaded module behind a runtime flag: changing it requires a PostgreSQL restart, and disabling it
creates a blind interval in the cumulative history.

Create the extension through a new timestamped additive migration:

```sql
CREATE EXTENSION pg_stat_statements;
```

Never edit `migrations/20260817000000_init_schema.sql`. It is the already-applied squashed baseline
even when application rows are about to be cleared. SQLx owns migration idempotency, so the
migration should fail on unexpected pre-existing state rather than hiding it with `IF NOT EXISTS`.

### Boolean switch for optional slow-query logs

Slow-query statement logging is supplementary diagnostics, not the primary evidence source. Put it
behind one strictly parsed boolean environment key read by the PostgreSQL startup wrapper:

```text
DATABASE_SLOW_QUERY_LOGGING_ENABLED=false
```

- `false` or unset: do not pass a slow-statement logging threshold; PostgreSQL's default remains in
  effect. Keep both parameter limits at zero independently of this switch.
- `true`: add all of the following server arguments:

  ```text
  -c log_min_duration_statement=200ms
  -c log_parameter_max_length=0
  -c log_parameter_max_length_on_error=0
  ```

- Any other value: fail startup with a configuration error. Do not silently interpret typos.

The zero parameter limits are unconditional and load-bearing: sqlx uses the extended query protocol, and PostgreSQL
can otherwise place bind values such as addresses, message content, tokens, or credentials in its
logs. The boolean must be set independently on the database Fly app, defaults to `false`, and takes
effect on database restart. Enable it for a bounded staging or incident-investigation window; do
not make it an undocumented permanent production default.

Implement the conditional argument construction in a small checked startup script included by a
derived PostgreSQL image. Keep the official `postgres:18.6-bookworm` base pinned, and have the
wrapper delegate to the official `docker-entrypoint.sh` so first-boot initialization continues to
work. Do not embed shell conditionals directly in the Fly process command.

### Privacy-safe application evidence

Always emit low-volume structured threshold events for facts PostgreSQL cannot recover after
normalization:

- list endpoint plus offset bucket (`0-99`, `100-999`, `1000+`), with a named
  `deep_pagination_observed` event when the highest bucket is entered;
- dashboard scope class (`company` or `global`) and window class;
- dashboard snapshot duration, returned working-set size, and whether it was truncated; and
- active dashboard SSE connection count.

Do not log query text, bind values, company/user identifiers, message data, or arbitrary request
objects. Keep label values to the fixed sets above so they cannot create unbounded cardinality.
These signals satisfy the repository rule that a deferred optimization leave a named metric or
threshold log capable of reopening the decision.

## Design

### 1. Reproducible PostgreSQL activation

- Replace the direct image reference with a minimal derived image containing only the checked
  startup wrapper.
- Keep `pg_stat_statements` preload arguments unconditional; append slow-query arguments only when
  `DATABASE_SLOW_QUERY_LOGGING_ENABLED=true`.
- Add the extension in a new additive migration.
- Align CI's PostgreSQL service with the production `18.6-bookworm` image and preload the module in
  CI. A migration-only test without the preloaded library does not prove the deployed feature is
  active.
- Document the corresponding local PostgreSQL configuration and restart. `reset-db.sh` recreates
  databases but cannot load a shared library into an already-running server.

After database deployment and application migration, verify all of the following:

```sql
SHOW shared_preload_libraries;
SHOW compute_query_id;
SHOW pg_stat_statements.track;
SELECT extversion FROM pg_extension WHERE extname = 'pg_stat_statements';
SELECT stats_reset, dealloc FROM pg_stat_statements_info;
```

Execute a harmless query and verify that its normalized entry appears in `pg_stat_statements` for
the current database. A successful `CREATE EXTENSION` alone is not activation evidence.

### 2. Investigation snapshot: `scripts/db-stats.sh`

Write one read-only snapshot script with `--local` for `$DATABASE_URL`; the default connects through
`fly ssh console`. Follow the error-handling conventions in `scripts/deploy.sh` and
`scripts/reset-db.sh` and run psql with `-X`, `ON_ERROR_STOP`, and the pager disabled.

The snapshot must print:

1. capture timestamp, PostgreSQL version, database name, relevant settings, database statistics
   reset time, and `pg_stat_statements_info.stats_reset`/`dealloc`;
2. top normalized statements for the current database by total execution time;
3. top statements by mean and maximum execution time, with a minimum call count to suppress
   one-off setup statements;
4. calls, rows, mean/max/total time, shared and temporary block activity, WAL bytes, query ID, and
   a bounded representative query text;
5. `pg_stat_user_tables`, including sequential/index scans, estimated live/dead tuples,
   modifications since analyze, and vacuum/analyze timestamps;
6. `pg_stat_user_indexes`, including scan/read/fetch counts and last scan time; and
7. heap, index, toast, and total relation sizes.

Label `idx_scan = 0` rows as "not observed since statistics reset", never "unused". The script must
not reset production statistics. A separate reset is allowed only as an explicit step in a
disposable benchmark database before replaying a controlled workload.

Statistics output may contain representative SQL text. Write to stdout only, document that saved
artifacts are operationally sensitive, and do not commit raw production snapshots unless they have
been reviewed and sanitized.

### 3. Plan investigation: controlled capture

Document a repeatable command for a reviewed, parameterized query. Prefer a representative staging
copy. If production execution is necessary:

- allow reviewed `SELECT` statements only;
- use a read-only transaction, short `statement_timeout`, and short `lock_timeout`;
- run during a bounded operational window; and
- never accept arbitrary SQL from an untrusted argument or file.

Use this baseline unless a specific investigation needs node timing:

```sql
EXPLAIN (ANALYZE, BUFFERS, WAL, SETTINGS, TIMING OFF)
SELECT ...;
```

`TIMING OFF` retains total execution time and row counts while avoiding per-node clock overhead.
Capture multiple executions to identify warm-cache behavior. Where sqlx may switch between custom
and generic prepared plans, capture both rather than assuming one representative value describes
all tenants or windows.

Each saved plan must include the snapshot timestamp, parameter classes (not sensitive values), row
counts/selectivity, statistics age, PostgreSQL version, and the exact candidate index definition.
Compare the entire join and sort, not only the target table's scan node.

### 4. Deterministic skewed seed and workload

Build a standalone development/benchmark tool that is neither an HTTP route nor included in the
production runtime image. It must refuse production-like hosts and database names and require an
explicit benchmark database.

The tool must accept a scale and deterministic random seed, print both, and populate every required
parent row plus:

- `email_messages` and `thread_messages`, including a few very long threads and a long tail;
- `background_tasks` with uneven company, status, schedule, creation, and update distributions;
- `task_attempts` with realistic attempts-per-task and recent/old `started_at` windows; and
- scheduled runs and list-page histories deep enough to exercise the remaining deferred queries.

Document the mathematical distribution rather than only the final row count. Load atomically,
run `ANALYZE` after the bulk insert, and print verification queries for largest, median, and
long-tail companies/threads plus window selectivity.

Seeding alone is insufficient. Add a workload runner that executes the actual application query
shapes over a fixed matrix:

- small, median, and large companies;
- short and long dashboard windows;
- short, median, and very long threads;
- shallow, medium, and deep offsets; and
- repeated executions after the initial run.

Test candidate indexes on this disposable database or a representative staging copy. Do not add an
acceptance criterion that forces an index scan: for a low-selectivity predicate or small relation,
a sequential scan may be the correct plan.

## Implementation Steps

1. Refresh `05-deferred-until-traffic.md` and the query-evidence links so the workload matrix covers
   only current unresolved decisions.
2. Add the derived PostgreSQL image and checked startup wrapper. Keep `pg_stat_statements`
   unconditional and implement the strict `DATABASE_SLOW_QUERY_LOGGING_ENABLED` boolean.
3. Add a new timestamped migration containing `CREATE EXTENSION pg_stat_statements`; do not edit
   the baseline migration.
4. Align and configure local/CI PostgreSQL to preload the module, then document the required
   restart and deployment order.
5. Add activation verification for settings, extension version, reset metadata, and an observed
   harmless query.
6. Keep `scripts/db-stats.sh` as the detailed local/Fly investigation snapshot, separate from the
   routine dashboard monitoring surface.
7. Keep privacy-safe pagination and SSE usage signals visible as process-local, since-boot evidence
   on the operator dashboard, with bounded field values.
8. Implement the deterministic standalone seeder and representative workload runner.
9. Document safe plan capture, artifact handling, and before/after comparison in `docs/deploy.md`.
10. Run formatting, offline compilation, shell syntax/static checks, migrations, database-backed
    tests, and `scripts/stack-budget.sh` through CI.

## Tests

- The startup-wrapper tests prove unset/`false` omits slow logging, `true` adds the threshold and
  both zero parameter limits, and any other value fails.
- CI starts the pinned production PostgreSQL version with `pg_stat_statements` preloaded, applies
  the additive migration, and proves a harmless query appears in the view.
- Migrations apply both over the existing squashed baseline and from an empty database.
- `scripts/db-stats.sh --local` produces every section, filters statements to the current database,
  records reset metadata, and performs no reset or write.
- Application-signal tests cover each bounded label/bucket, prove the deep-offset threshold, and
  assert that identifiers, bind values, message content, and query text are absent.
- The production Docker image contains only the server binary and cannot invoke the seeder.
- Given the same seed and scale, the seeder reports the same cardinalities and distribution; its
  largest, median, and long-tail verification queries match the rows written.
- The workload runner exercises every current deferred query shape and records its parameter class
  without recording sensitive values.
- Plan-capture documentation uses a reviewed `SELECT`, read-only transaction, and bounded timeouts.

## Acceptance Criteria

- `pg_stat_statements` is active before the cleared database is reopened to data-bearing traffic,
  and its activation is verified rather than inferred from configuration.
- Optional slow-query logging is controlled only by the strict
  `DATABASE_SLOW_QUERY_LOGGING_ENABLED` boolean and never logs bind parameters.
- The operator System dashboard identifies expensive normalized queries and states the
  observation/reset window needed to interpret every counter, while company dashboards remain
  unchanged.
- `scripts/db-stats.sh` and controlled `EXPLAIN` remain explicit investigation workflows rather
  than browser endpoints.
- Deep pagination, dashboard scope/window, working-set truncation, and SSE fan-out are observable
  without logging identifiers or unbounded values.
- A reviewed query can be captured with reproducible plan context and operational safety bounds.
- Synthetic plans are explicitly labeled as shape checks and are never the sole justification for
  a production index or pagination contract change.
- No applied migration is edited, no seeder is shipped in the production runtime image, and CI
  exercises formatting, offline compilation, migrations, the database-backed suite, and the
  stack-budget guard.
