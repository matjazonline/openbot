# Phase 3 — Dashboard: scope predicate and snapshot cache

The operator dashboard runs eight unbounded aggregates over the two largest tables, sequentially,
once per five-second tick, **per connected tab**. Three of those eight scan `task_attempts`, which
has no index on the column they filter and no retention sweep anywhere in the codebase.

This phase does two things. The first is a Class A correctness fix to a predicate that can silently
widen from one company to all of them. The second is a Class B change that removes most of the read
pressure without adding a single index — which is what makes it the right answer to the
`task_attempts` gap that `plan/db_improve/05-deferred-until-traffic.md` §4 has been holding at the
evidence gate since 2026-09-01.

---

## 3.1 The `($1::uuid IS NULL OR company_id = $1)` scope predicate

Seven statements in `src/adapters/persistence/dashboard.rs` select their scope this way —
`TASK_QUEUE_SQL:45`, `TASK_PRESSURE_SQL:58`, `DELIVERY_QUEUE_SQL:69`, `DELIVERY_PRESSURE_SQL:82`,
`THROUGHPUT_BODY:132`, `QUEUE_DEPTH_BODY:201`, `ATTEMPT_STATS_SQL:242`, `RETRY_RATE_BODY:263` — so
that one statement serves both the per-company view and the operator-wide (`None`) view.

Under a **custom** plan, Postgres substitutes the parameter, folds `$1 IS NULL` to false, and the
predicate reduces to `company_id = $1`:

```
Index Only Scan using background_tasks_company_status_created_idx
  Index Cond: (company_id = '…'::uuid)
```

Under a **generic** plan, the parameter is not known when the plan is built, so the `OR` survives
and becomes a filter applied after the scan:

```
Bitmap Index Scan on background_tasks_company_status_created_idx
  Filter: (($1 IS NULL) OR (company_id = $1))
```

Both plans were captured on 2026-09-10 against the scratch schema database with
`plan_cache_mode = force_generic_plan` for the second. **This is a shape claim, not a timing claim** —
what it establishes is that the generic plan reads every company's rows and discards the ones that
do not match, and that a company-scoped dashboard request can therefore do operator-wide work.

sqlx prepares these statements, and Postgres switches a prepared statement to a generic plan once it
has seen enough executions and the generic cost looks competitive. A dashboard on a five-second tick
reaches that threshold in under a minute. The behaviour is therefore not hypothetical, it is merely
undated — nobody can say from here which side of the switch a given deployment is on, which is
exactly why the predicate should not be able to express the wrong thing at all.

**Change.** Stop encoding the scope choice in the SQL. Two statements per aggregate, selected in
Rust:

```rust
const TASK_QUEUE_COMPANY_SQL: &str = r#"
    SELECT status, COUNT(*)::bigint AS count
      FROM background_tasks
     WHERE company_id = $1
     GROUP BY status ORDER BY status"#;

const TASK_QUEUE_GLOBAL_SQL: &str = r#"
    SELECT status, COUNT(*)::bigint AS count
      FROM background_tasks
     GROUP BY status ORDER BY status"#;
```

The company form is then indexable under either plan mode, and the global form is honestly a full
aggregate because that is what it is asking for. Eight aggregates × two forms is sixteen constants,
which is more text than the current file carries; the bucketed ones already compose from `SLOTS_CTE`
plus a body (`:225`–`:228`), so extend that composition with a scope fragment rather than writing
sixteen literals by hand.

**Do not reach for `company_id = COALESCE($1, company_id)`.** It is shorter and it is worse: it
still cannot use the index in the global case, and in the company case it makes the predicate
non-sargable in a way that is harder to spot than the `OR`.

**Test.** For each aggregate, one test asserting the company-scoped form returns only the company's
rows when a second company has data, and one asserting the global form includes both. The existing
`retry_rate_counts_only_attempt_numbers_above_one_and_stays_company_scoped`
(`dashboard.rs:755`) is the pattern; the isolation half of it is the half that matters here.

---

## 3.2 Cache the snapshot across tabs

`dashboard_snapshot` (`src/adapters/persistence/dashboard.rs:325`) runs its eight aggregates
sequentially and is called once per request from `src/adapters/http/routes/ui_dashboard.rs:259`.
`TICK` is five seconds (`ui_dashboard.rs:63`) and the SSE loop re-reads on every tick
(`:403`), per connection. `active_dashboard_sse_connections` (`:446`) already gauges how many
connections there are, so the multiplier is measurable in production the day this ships.

Three of the eight — `ATTEMPT_STATS_SQL:242`, `LATENCY_BODY:161`, `RETRY_RATE_BODY:263` — filter
`attempt.started_at >= CURRENT_TIMESTAMP - make_interval(...)` against a table whose only indexes are
`task_attempts_pkey (id)` and `task_attempts_task_attempt_key (task_id, attempt_number)`. There is
no access path for a `started_at` window, and no `DELETE` targets `task_attempts` anywhere in
`src/`, so the table only grows.

**`plan/db_improve/05-deferred-until-traffic.md` §2 already prescribes the fix and ranks it above
the index**: cache the snapshot for a tick across connected tabs, because it "scales with operator
count rather than data size and may make the index unnecessary". This phase implements that
recommendation. The index candidate stays at the gate; Phase 6 records why.

**The pattern to copy is in this repository.** `DatabaseQueryHealthService`
(`src/application/services/database_query_health.rs:147`) holds `cache: Mutex<Option<CachedReading>>`
with `SUCCESS_TTL = 60s` and `FAILURE_TTL = 15s` (`:16`, `:17`), and the comment at `:145` records
the design decision that matters: **one mutex deliberately covers both the check and the refresh**,
so a hundred waiting readers cause exactly one refresh. Copy that, including the reasoning.

**What differs.** The query-health cache is a single slot; the dashboard is keyed. The key is
`(Option<Uuid>, DashboardWindow)` — `DashboardWindow` (`src/domain/entities/dashboard.rs:19`) is
`Copy + PartialEq + Eq` and has exactly three presets (`PRESETS`, `:26`), so the key space is
`companies × 3` and a `HashMap` is the right container. Derive `Hash` on `DashboardWindow`; it is a
two-field integer struct and the derive is free.

**TTL.** One tick, not sixty seconds. The dashboard's contract is "live", and a snapshot older than
the tick that requested it would show operators stale queue depths during an incident. Five seconds
of TTL with single-flight already collapses N tabs into one read, which is the entire win; a longer
TTL buys progressively less and costs credibility. Take `TICK` from one place so the cache and the
SSE loop cannot drift apart.

**The isolation test §2 demands, restated because it is the one that matters.** A cached snapshot
must never serve one company's rollup to another company's view, and the global view must never be
served to a company request or vice versa. Write it as: populate two companies with distinguishable
data, read company A, read company B, read global, assert three different results, then re-read all
three inside the TTL and assert each still gets its own. A cache keyed on the window alone, or on
`Option<Uuid>` compared by `is_some()`, passes every other test in the suite and fails this one.

**Failure handling.** Follow the query-health service and cache failures too, briefly. A database
that is refusing connections should not receive eight aggregate queries per tab per tick on top of
whatever is already wrong with it.

**Where it lives.** `DatabaseQueryHealthService` is in `src/application/services/`, and the
dashboard cache belongs beside it rather than in the persistence adapter — the adapter's job is to
answer the question, the service's job is to decide how often it is worth asking. That also keeps
the cache out of the `DashboardPersistence` trait (`dashboard.rs:35`), so tests can still drive the
uncached path directly.

---

## 3.3 What to do about the eight sequential round trips

`dashboard_snapshot:330` carries a comment justifying eight statements rather than one join, and it
is right — these are unrelated aggregates and joining them would be a cross join nobody could index.
But sequential is not the same decision as unjoined: the eight are independent reads and could run
concurrently on eight pool connections with `tokio::try_join!`.

**Do this only if 3.2 does not land, or if it lands and the snapshot is still slow.** With the cache
in place the snapshot is computed once per tick for the whole fleet of tabs, at which point eight
sequential round trips is an irrelevant cost paid by one caller — and taking eight pool connections
at once to save it is a worse trade, because it competes with the workers that actually need them.
Recording it here so the option is known, and so nobody does both without noticing they are pulling
in opposite directions.

---

## Implementation notes (2026-09-11)

What landed differs from the text above in these places. The reasons are recorded so the next
reader does not "fix" the code back to match the plan.

- **The company is always the last parameter.** The global form then simply binds one parameter
  fewer. That renumbered the templates: `SLOTS_CTE` now uses `$1` (bucket width) and `$2`
  (window span), `ATTEMPT_STATS_SQL` takes its window as `$1`, and `OUTSTANDING_SQL` its limit as
  `$1`. Each template marks one `{scope}` slot, and `ScopedSql::new` builds both forms from it. A
  unit test asserts the company form adds exactly one parameter, numbered last.
- **"No statement contains `IS NULL OR`" is read as "no statement tests a *parameter* for NULL".**
  `lock_expires_at IS NULL OR …` and the delivery lease checks are column null tests. The
  persistence `AGENTS.md` requires them, so that legacy in-flight rows count as stalled. The guard
  test matches `$n [::type] IS [NOT] NULL`.
- **The isolation tests are two table-driven tests rather than twenty.**
  `every_company_form_reads_only_its_own_company` and `every_global_form_reads_both_companies`
  measure all ten statements against the same fixture. Company B holds twice what company A holds.
  Every failing aggregate is reported, not just the first. Every fixture row is out of reach of the
  unscoped claims and sweeps: tasks on a channel with no agent, finished tasks set to `failed` so
  no notification is enqueued, and due deliveries blocked behind a parked root. As a check, three
  company predicates were broken by hand, and the test named exactly those three.
- **`DashboardPersistence` moved into `services::dashboard_snapshot`.** The service consumes the
  port, and `src/application/AGENTS.md` puts ports where they are consumed. `AppState` now carries
  `dashboard_snapshots: Arc<DashboardSnapshotService>` in place of the persistence handle, and the
  `FromRef` for the trait is gone, so no handler can bypass the cache.
- **One mutex per view, not one for the service.** The check-and-refresh-under-one-lock reasoning is
  copied, but the lock is per `(company, window)` slot. A single lock would put every company's
  refresh in one queue, and one slow operator rollup would stall every company's page.
- **Freshness runs from when a reading's queries were issued, not from when they finished.** The
  query-health service measures from completion, which is harmless with a 60-second TTL. With a
  TTL of one tick, the SSE interval's fixed schedule puts each tick inside the previous reading's
  TTL by however long that reading took. A lone tab would then refresh only every other tick.
  `a_lone_tab_gets_a_fresh_reading_on_every_tick` fails if this is changed. Measuring from the
  issue time alone would break single-flight when a reading takes longer than a tick. So a stale
  reading still answers any caller who was already waiting when it came back
  (`callers_queued_behind_a_slow_reading_reuse_it`).
- **Failures are cached for one tick, the same as successes.** Query health uses 15 s against 60 s.
  One tick is already brief, and a shorter TTL would allow more than one failing read per view per
  tick. `AppError` now derives `Clone`, so every caller served from a cached failure gets the same
  error the refresher got.
- **Idle slots are swept when a new view is first read.** Otherwise the map would keep one stale
  snapshot for every company and window read since boot. The sweep drops any slot that nobody
  holds and whose reading is no longer fresh. Such a slot can never be served again.
- **3.3 was not done**, because the cache landed. The comment in the adapter's
  `dashboard_snapshot` now says why the reads stay sequential.
- **Out of scope, noted:** in the operator view, `load_runtime_snapshot` still reads
  `runtime_metric_samples` once per tick per tab. This phase covered only the eight aggregates.

## Acceptance criteria

- No statement in `dashboard.rs` contains `IS NULL OR`; scope is chosen in Rust and each form is
  separately indexable or honestly global.
- Company-scoped and global forms are covered by isolation tests for every aggregate.
- `dashboard_snapshot` is served from a keyed, single-flight cache with a TTL equal to the SSE tick,
  and the cache lives in `src/application/services/`.
- A test proves two companies and the global scope never see each other's cached snapshot.
- No index is added. `task_attempts (started_at)` remains at the gate, and Phase 6 records the
  updated case for it now that the read pressure has been removed.
- `cargo sqlx prepare -- --all-targets` regenerated; `cargo test` and
  `cargo clippy --all-targets -- -D warnings` green.
