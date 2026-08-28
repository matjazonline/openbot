# Capture Query Statistics

## Goal

Make it possible to answer the questions `plan/db_improvements.md` deferred. Its instruction was to
collect production `EXPLAIN (ANALYZE, BUFFERS)` plus table and index statistics before changing
indexes or pagination. Nothing in the repository can do that today, and the database is empty, so
there is nothing to measure either.

## Current Risk

- `runtime_metric_samples` (`migrations/20260817000000_init_schema.sql:1194-1247`) is a
  process/pool heartbeat — RSS, CPU, pool size, acquire latency, HydraDB counters — one row per
  machine per ~10s tick, written by `src/adapters/persistence/runtime_metrics.rs:291-317`. It has
  no column for query text, plan, or per-query timing and cannot be reused for this.
- `MonitoringService` (`src/domain/monitoring.rs`, adapters in `src/adapters/monitoring/`) is
  in-process only: a tracing sink plus an in-memory counter store, wired at
  `src/infra/setup.rs:39-42`, exposed as JSON at `/metrics` and `/api/v1/monitoring/stats`
  (`routes/monitoring.rs`). There is no Prometheus or OTel export anywhere in `src/`.
- `pg_stat_statements` is not enabled. `deploy/postgres/fly.toml` starts Postgres with
  `-c listen_addresses=* -c max_connections=100 -c shared_buffers=128MB
  -c effective_cache_size=384MB` and no `shared_preload_libraries`.
- No `EXPLAIN`, `pg_stat_user_tables`, or `pg_stat_user_indexes` query exists in `src/` or
  `migrations/`. `RUST_LOG` (`fly.toml:9`) sets `sqlx=warn`, giving sqlx's built-in 1s
  slow-statement warning and nothing else.
- Ad-hoc SQL against production is `fly ssh console -a mail-agents-db -C
  "psql -U mail_agents mail_agents"` (`docs/deploy.md:313-314`). The database has no public IP and
  is reachable only over the org's 6PN network.

Enabling `pg_stat_statements` requires a database restart. Doing it before launch costs nothing;
doing it after means a deliberate production restart, and the window to observe a system from its
first row is gone either way.

## Design

Four pieces. The first two are configuration, the third a script, the fourth the one that matters
most while the database is empty.

1. **`pg_stat_statements`** — add `-c shared_preload_libraries=pg_stat_statements
   -c pg_stat_statements.track=all` to the Postgres command in `deploy/postgres/fly.toml`, and
   `CREATE EXTENSION IF NOT EXISTS pg_stat_statements` directly in
   `migrations/20260817000000_init_schema.sql` (the database is empty; no additive migration).
2. **Slow-query logging in staging** — `-c log_min_duration_statement=200ms` on the Postgres app,
   so slow statements reach `fly logs` with no application change.
3. **`scripts/db-stats.sh`** — dumps to stdout: top 30 rows of `pg_stat_statements` by
   `total_exec_time`; `pg_stat_user_tables` (seq vs index scans, live/dead tuples);
   `pg_stat_user_indexes` (`idx_scan = 0` finds indexes nothing uses); `pg_relation_size` per table
   and index. A `--local` mode runs against `$DATABASE_URL`; the default goes over
   `fly ssh console`. Follow the conventions in `scripts/deploy.sh` and `scripts/reset-db.sh`.
4. **A dev-only seeder that models skew.** This is the load-bearing piece: with an empty database
   the deferred items in `05-deferred-until-traffic.md` cannot be measured at all, and a seeder
   that fills tables *evenly* would answer the wrong question. What decides those items is
   distribution, not row count — a handful of threads with thousands of messages against a long
   tail with three; a few companies holding most of the tasks; most schedules with no runs and one
   with hundreds. Fill `background_tasks`, `task_attempts`, `thread_messages`, and `email_messages`
   to a target order of magnitude with an explicitly skewed shape, and document the distribution it
   generates so a plan read against it can be interpreted honestly.

A seeded plan is evidence about query *shape*, not about production. Treat it as enough to rule a
change out, never enough to rule one in.

## Implementation Steps

1. Edit the Postgres process command in `deploy/postgres/fly.toml`; note in `docs/deploy.md` that
   the change requires a database restart.
2. Add the extension to the init schema; `./scripts/reset-db.sh --all`.
3. Write `scripts/db-stats.sh`, executable, with the `--local` mode.
4. Write the skewed seeder behind a dev-only entry point, with a row-count argument and a printed
   summary of the distribution it produced.
5. Document in `docs/deploy.md` how to capture a plan for a single query through
   `fly ssh console`.

## Tests

- `scripts/db-stats.sh --local` produces all four sections against a local database.
- The init schema replays cleanly on a database where the extension is already present.
- The seeder is unreachable from a release build or is guarded by an explicit environment flag.
- The seeder's reported distribution matches what it actually wrote (a spot-check query on the
  largest and median thread).

## Acceptance Criteria

- A plan and a statistics snapshot can be captured for any named query without ad-hoc improvisation.
- `pg_stat_statements` is active before the first production deploy, not after.
- The seeded dataset is skewed enough that `EXPLAIN (ANALYZE, BUFFERS)` on the queries in
  `05-deferred-until-traffic.md` distinguishes an index scan from a sequential one.
