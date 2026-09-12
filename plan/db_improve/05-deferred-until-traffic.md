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
image, the extension is created by `migrations/20260817000000_init_schema.sql`, the
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

**The cache landed** (`plan/db_audit/phase3.md`): `DashboardSnapshotService`
(`src/application/services/dashboard_snapshot.rs`) answers one reading per view per
`DASHBOARD_TICK`, and the isolation test above exists. No index was added. What that did and did not
settle is recorded in §4a — it weakened the `task_attempts (started_at)` case rather than closing
it, and left the `background_tasks` shapes listed above exactly as they were.

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

### 4a. `task_attempts (started_at)`: what Phase 3 changed, and the retention gap beside it

Recorded 2026-09-12 from `plan/db_audit/phase6.md` §6.3. The audit found nothing that changes this
candidate's *status*, but Phase 3 changed its **urgency**, and that belongs in the record.

Before Phase 3, three statements scanned `task_attempts` on every five-second tick of every
connected dashboard tab: `ATTEMPT_STATS_SQL`, `LATENCY_BODY` and `RETRY_RATE_BODY`
(`src/adapters/persistence/dashboard.rs`). After it, `DashboardSnapshotService`
(`src/application/services/dashboard_snapshot.rs`) answers one reading per *view* per
`DASHBOARD_TICK`, so those three run once per tick for the whole fleet of tabs watching that view.

That is exactly the outcome §2 predicted when it said to "evaluate the cache first — it scales with
operator count rather than data size and may make the index unnecessary". The read pressure is now
proportional to tick rate and to the number of distinct views (companies being watched × windows
picked, plus the operator rollup), not to viewer count. **So the index's case is weaker than it was,
not stronger.** The remaining question is narrower than the one this section was opened with:
whether a single scan per tick of an unpruned, ever-growing table is acceptable at real cardinality.

**The retention gap is the other half of the same decision.** No statement in `src/` deletes from
`task_attempts` — `grep -rn "DELETE FROM task_attempts" src/` returns nothing. Rows leave only by
cascade from `background_tasks` (`task_attempts_task_id_fkey ... ON DELETE CASCADE`), and
`background_tasks` is never pruned by design: the dashboard's queue-depth reconstruction depends on
that, as its own comment records (`src/domain/entities/dashboard.rs:217`). So the table only grows,
and every dashboard window query pays for the whole accumulated history regardless of the window it
asks for.

A retention sweep modelled on `delete_retained` (`src/adapters/persistence/inbound_event/mod.rs:515`
— batched, ordered by `(processed_at, id)`, backed by a matching partial index) may be a better
answer than an index. The two interact in both directions: a table with retention needs a smaller
index, and a table with an index prunes faster. **Neither should be decided without the other on the
table.**

**What would close this:** the three dashboard statements timed at real `task_attempts`
cardinality, with and without a retention horizon in place, plus the table's write rate — one
`task_attempts` row per claim, so it is written at queue rate, not at human rate.

## 5. Task Claim: Per-Company Slices (landed, `plan/db_audit/phase5.md`)

Not deferred — recorded here because it widens an index on the hottest table, and this file's rule
is that every plan captured for such a change lives here.

**What changed.** `claim_pending_tasks` (`src/adapters/persistence/task/operations.rs`) no longer
ranks the whole due backlog with `ROW_NUMBER() OVER (PARTITION BY company_id …)`. A recursive
loose index scan enumerates companies with pending agent work, a `LATERAL` slice takes at most
`$1` rows from each, and the final `ORDER BY company_round, run_at, created_at, id … LIMIT $1` is
unchanged. The claim's index was widened in place:

    -- before
    CREATE INDEX background_tasks_pending_ready_idx ON public.background_tasks
        USING btree (run_at, created_at, id) WHERE (status = 'pending'::text);
    -- after
    CREATE INDEX background_tasks_pending_ready_idx ON public.background_tasks
        USING btree (company_id, run_at, created_at, id)
        WHERE ((status = 'pending'::text) AND (owner_principal_kind = 'agent'::text));

`pg_indexes` counts 16 indexes on `background_tasks` before and after. The new predicate is
narrower, so the index is maintained on a subset of the writes the old one was: the same writes,
minus those to person-owned and unassigned pending tasks.

**Environment for every capture below.** Local Homebrew PostgreSQL 16.14; CI and production run
18.6 (see "Verification Gaps"). No timing was taken anywhere: `COSTS OFF`, and `TIMING OFF,
SUMMARY OFF` wherever `ANALYZE` was used. Captured 2026-09-11.

### Shape check against an empty database

`mail_agents_schema_audit`, built from the migration (before: `HEAD`; after: the edited file), no
`ANALYZE`. Custom and generic plans were identical in both, so one of each is shown.

Before:

    Update on background_tasks task
      CTE claimable
        ->  Limit
              ->  LockRows
                    ->  Sort
                          Sort Key: ranked.company_round, ranked.run_at, ranked.created_at, task_1.id
                          ->  Nested Loop
                                ->  Subquery Scan on ranked
                                      ->  WindowAgg
                                            ->  Sort
                                                  Sort Key: background_tasks.company_id, background_tasks.run_at, background_tasks.created_at, background_tasks.id
                                                  ->  Index Scan using background_tasks_pending_ready_idx on background_tasks
                                                        Index Cond: (run_at <= CURRENT_TIMESTAMP)
                                                        Filter: ((owner_principal_kind = 'agent'::text) AND (SubPlan 1))
                                                        SubPlan 1
                                                          ->  Nested Loop  (principals_pkey, channel_agents_pkey)
                                ->  Index Scan using background_tasks_pkey on background_tasks task_1
                                      Index Cond: (id = ranked.id)
      ->  Nested Loop
            ->  CTE Scan on claimable
            ->  Index Scan using background_tasks_pkey on background_tasks task
                  Index Cond: (id = claimable.id)

After:

    Update on background_tasks task
      CTE pending_company
        ->  Recursive Union
              ->  Limit
                    ->  Index Only Scan using background_tasks_pending_ready_idx on background_tasks task_1
              ->  WorkTable Scan on pending_company previous
                    Filter: (company_id IS NOT NULL)
      CTE claimable
        ->  Limit
              ->  LockRows
                    ->  Sort
                          Sort Key: slice.company_round, slice.run_at, slice.created_at, task_2.id
                          ->  Nested Loop
                                Join Filter: (slice.id = task_2.id)
                                ->  Seq Scan on background_tasks task_2
                                      Filter: ((status = 'pending'::text) AND (owner_principal_kind = 'agent'::text) AND (run_at <= CURRENT_TIMESTAMP))
                                ->  Nested Loop
                                      ->  CTE Scan on pending_company
                                            Filter: (company_id IS NOT NULL)
                                      ->  Subquery Scan on slice
                                            ->  Limit
                                                  ->  WindowAgg
                                                        ->  Index Scan using background_tasks_pending_ready_idx on background_tasks task_3
                                                              Index Cond: ((company_id = pending_company.company_id) AND (run_at <= CURRENT_TIMESTAMP))
                                                              Filter: (SubPlan 3)
                                                              SubPlan 3
                                                                ->  Nested Loop  (principals_pkey, channel_agents_pkey)
      ->  Hash Join
            Hash Cond: (task.id = claimable.id)
            ->  Seq Scan on background_tasks task
            ->  Hash
                  ->  CTE Scan on claimable

What the shape shows: before, the only `Limit` sits above a `WindowAgg` and two `Sort`s over every
due row. After, each company's rows come from an index scan in `run_at` order under its own
`Limit`, with no sort between them, so the window is bounded by the slice. The loose scan is an
`Index Only Scan` on the widened index. (The `Seq Scan`s are what the planner picks for a table
with no rows; with data it probes `background_tasks_pkey`, below.)

### Shape check against a synthetic seed

`mail_agents_claim_seed`, built from the same migration, then seeded with triggers and foreign keys
off (`session_replication_role = replica`), then `ANALYZE`d. **Not representative of production.**
It exists to count rows and buffers per plan node. Distribution: one company with 20,000 due agent
tasks spread over an hour, plus 1,000 not yet due; 59 companies with one to three due tasks each,
newer than most of that backlog; 3,000 pending person-owned tasks and 600 pending unassigned ones,
days old; 42,000 completed; 8 processing. Each claim ran inside a rolled-back transaction.

| | Before, limit 1 | Before, limit 4 | After, limit 1 | After, limit 4 |
|---|---|---|---|---|
| Rows the candidate scan produced | 20,217 | 20,217 | 60 (one per company) | 122 |
| Channel-assignment `EXISTS` probes | 20,217 | 20,217 | 60 | 122 |
| Person-owned/unassigned rows stepped over | 3,600 | 3,600 | 0 (not in the index) | 0 |
| Join back to `background_tasks` | `Seq Scan`, 66,726 rows | same | pkey probe × 60 | pkey probe × 122 |
| Buffers, whole statement (custom plan) | 42,959 | 43,056 | 876 | 1,357 |
| Buffers, generic plan | — | 44,712 | — | 1,356 |
| Companies served, in order | — | 1, 60, 59, 58 | — | 1, 60, 59, 58 |

The batch is identical — the fairness result is unchanged, only the work to reach it.

Two supporting checks on the same seed:

- **The new statement against the old index** (limit 4): 56,404 buffers — worse than the old
  statement. Each of the 60 slices combined the company's `unsettled_owner` entries with a bitmap
  scan of all 23,817 `run_at <= now` entries of the old index. The widening is what makes the
  rewrite pay.
- **`SELECT DISTINCT company_id … WHERE status = 'pending' AND owner_principal_kind = 'agent'`**
  plans as `Unique` over an `Index Only Scan` of all 21,118 pending agent rows to find 60 companies.
  That is why the enumeration is recursive rather than `DISTINCT`. PostgreSQL has no loose scan for
  `DISTINCT`, and 18's skip scan needs a condition on a later index column, which a bare
  `DISTINCT` does not have — the reading `plan/db_audit/phase2.md` recorded from the 18 docs.

### Other readers of the old leading column

Every non-test statement with a `status = 'pending'` predicate on `background_tasks`, found by grep
on 2026-09-11:

- `TASK_PRESSURE_SQL`'s `due_now` (`dashboard.rs`) and `census_stuck_work`'s `queue_overdue`
  (`task/operations.rs`) — `COUNT(*) FILTER (…)` aggregates over the table or a company;
  neither used the partial index.
- `OUTSTANDING_SQL` (`dashboard.rs`) — its pending arm is one side of an `OR`. Captured before and
  after, both forms: empty database, `Seq Scan` (global) and `background_tasks_company_channel_created_idx`
  (company), unchanged; seeded, `Parallel Seq Scan` (global) and a `BitmapOr` of
  `background_tasks_company_status_created_idx` and `background_tasks_unsettled_owner_idx`
  (company), unchanged. It never read `background_tasks_pending_ready_idx`.
- `board.rs`'s `MIN(run_at) FILTER (WHERE status = 'pending')` — an aggregate over an already
  selected chain.
- `CLAIM_TASK_SQL` (`task/queue.rs`) and the `notify_agent_instruction` trigger — by primary key.
- `attention.rs`, `board.rs`, `operations.rs` status lists — `status IN (…)`, which a
  single-status partial index cannot serve.

### A double claim, found while capturing

The old statement could re-claim a task another worker had claimed and committed after its
snapshot: the pending check lived only in the ranking subquery, and a `FOR UPDATE` recheck
re-evaluates only conditions on the locked table. Reproduced on the seed with two `psql` sessions,
the second delayed by a materialized `pg_sleep` between its snapshot and its locking step: before,
both sessions returned task `8fc003b3…` and it ended up held by the second worker; after, the
second skipped it and took `398acf3f…`. The rewrite repeats the pending and owner checks against
the locked row. `a_claim_never_takes_a_task_another_worker_claimed_after_its_snapshot`
(`task/claim_tests.rs`) pins it deterministically, and fails against the old statement.

### Follow-up: fewest running first

Recorded in `plan/db_audit/phase5.md`, "Follow-up". A candidate's round now adds its company's live
`processing` count (`in_flight`). The count is read through
`background_tasks_processing_lease_idx`. This used the same seed, with company 1's 8 processing
rows given live leases:

| | Phase 5 statement | Follow-up |
|---|---|---|
| Companies served, limit 4 | 1, 60, 59, 58 | 60, 59, 58, 57 |
| `in_flight` | — | `Index Scan using background_tasks_processing_lease_idx`, 8 rows |
| Buffers, limit 1 / limit 4 / limit 4 generic | 876 / 1,357 / 1,356 (table above) | 879 / 1,360 / 1,362 |

This is a scheduling change, not a performance one. The capture shows that it costs one index scan
over live leases, which is bounded by the number of busy worker slots.

### What would reopen it

The claim now costs one index descent per company with pending agent work, plus at most `$1` rows
from each. The number to watch once there is traffic is how many companies have pending agent work
at once; if it reaches the thousands, `claim_pending_tasks` should rank in `pg_stat_statements` and
this record is where the comparison starts.

## 6. Principal Removal: A Set-Based Trigger (landed) and 23 Unindexed Foreign Keys

Recorded 2026-09-12 from `plan/db_audit/phase6.md`. One change landed; three decisions are held
here. Nothing in `src/` runs `DELETE FROM principals` — principals go away by cascade from two
interactive, user-initiated operations:

- **Removing a company member.** `remove_member` (`src/adapters/persistence/company_invite.rs`)
  demotes the principal to `external` and then deletes the `company_members` row, so the release
  runs from `principals_release_owned_tasks_on_demotion` (`BEFORE UPDATE OF kind`).
- **Deleting an agent.** `DELETE FROM agents WHERE id = $1` (`src/adapters/persistence/agent.rs`)
  → `principals_company_agent_fk … ON DELETE CASCADE`, so the release runs from
  `principals_release_owned_tasks` (`BEFORE DELETE`).

Neither can commit unless the release has cleared every task the principal owned:
`background_tasks_owner_principal_fk` is `(company_id, owner_principal_id, owner_principal_kind)
→ principals(company_id, id, kind) ON DELETE RESTRICT`, with no `ON UPDATE CASCADE`, so the
demotion violates it just as the delete does.

**Environment for every capture in this section.** Local Homebrew PostgreSQL 16.14; CI and
production run 18.6 (see "Verification Gaps"). `mail_agents_schema_audit`, built from
`migrations/20260817000000_init_schema.sql` — never from the development database, which has
drifted. `COSTS OFF`; no timing was taken anywhere. Captured 2026-09-12. **These are shape checks,
not measurements.**

### 6.1 Landed: the release trigger is set-based

`release_tasks_for_removed_principal()` used to open a `FOR UPDATE` cursor over every task the
principal had ever owned — **no status predicate, so every task, including ones completed months
ago** — and loop. Per row it issued an `UPDATE background_tasks`, conditionally an `UPDATE
task_attempts`, and an `INSERT INTO task_ownership_events`. `background_tasks` carries eight row
triggers and `task_ownership_events` three, so a principal with 5,000 lifetime tasks generated on
the order of fifteen thousand statements and their trigger cascades inside one transaction, holding
`FOR UPDATE` on all of them.

It is now one `WITH` statement: a locking `owned` CTE, a set `UPDATE background_tasks` that returns
each task's pre-release status, ownership version and execution generation, a set `UPDATE
task_attempts` fed from that, and one `INSERT … SELECT` of the ownership events. Row triggers still
fire once per row — that is PostgreSQL, not the statement shape — but the plpgsql loop, the per-row
planning and the cursor are gone, and the row set is computed, planned and locked once.

**Class A: strictly fewer statements for the same result. No index was added.** The three values the
loop read off each row are carried forward rather than re-read, which is what keeps the result
identical: `task_attempts` is fenced on the generation the task is *losing*, and each ownership
event's `sequence` is that task's own `ownership_version + 1`, not a running counter over the
released set.

`removing_a_member_releases_every_task_they_owned_in_every_status`,
`deleting_an_agent_releases_every_task_its_principal_owned` and
`a_principal_owning_no_tasks_is_removed_without_an_ownership_event`
(`src/adapters/persistence/task/release_tests.rs`) were written against the loop and pass unchanged
against the set form. They own a task in each of the eight statuses, each at a *different*
ownership version, so a running-counter mistake in the event sequence fails; and they leave one
attempt row on a superseded generation and one on a task that is not running, so a rewrite that
fences too widely fails too.

#### The row set has no access path, before or after

The trigger's selection is the finding, and the rewrite does not change it. Empty scratch database:

    LockRows
      ->  Index Scan using background_tasks_company_updated_idx on background_tasks task
            Index Cond: (company_id = '…'::uuid)
            Filter: (owner_principal_id = '…'::uuid)

— a scan of the whole tenant's tasks, filtered.
`background_tasks_unsettled_owner_idx (company_id, owner_principal_id) WHERE status IN (…unsettled…)`
looks like it covers this and cannot: the selection carries no status predicate, so the planner
cannot prove the partial index applies. This is the same fact as §6.2 below, one table over.

#### The statement shape, seeded

`mail_agents_schema_audit`, seeded with triggers and foreign keys off
(`session_replication_role = replica`) and then `ANALYZE`d: 20,000 `background_tasks` rows in one
company, 5,000 `task_attempts`. **Not representative of production** — it exists to show which join
shapes the planner reaches for at two selectivities, nothing more.

A principal owning ~1,818 of the 20,000 (9%):

    Insert on task_ownership_events
      CTE owned
        ->  LockRows
              ->  Seq Scan on background_tasks task
                    Filter: ((company_id = '…'::uuid) AND (owner_principal_id = '…'::uuid))
      CTE released
        ->  Update on background_tasks task_1
              ->  Hash Join
                    Hash Cond: ((owned.company_id = task_1.company_id) AND (owned.id = task_1.id))
                    ->  CTE Scan on owned
                    ->  Hash
                          ->  Seq Scan on background_tasks task_1
      CTE fenced
        ->  Update on task_attempts attempt
              ->  Nested Loop
                    ->  CTE Scan on released released_1
                          Filter: (released_status = 'processing'::text)
                    ->  Index Scan using task_attempts_task_attempt_key on task_attempts attempt
                          Index Cond: (task_id = released_1.id)
                          Filter: ((status = 'processing'::text)
                                   AND (released_1.released_generation = execution_generation))
      ->  Subquery Scan on "*SELECT*"
            ->  CTE Scan on released

A principal owning 5 rows — the same statement, same statistics, only the parameter changed:

      CTE released
        ->  Update on background_tasks task_1
              ->  Nested Loop
                    ->  CTE Scan on owned
                    ->  Index Scan using background_tasks_pkey on background_tasks task_1
                          Index Cond: (id = owned.id)
                          Filter: (owned.company_id = company_id)

What the shapes show: the write-back to `background_tasks` is a primary-key probe per released task
when the set is small and one hash join when it is large, and the attempt fencing is one index
probe per *released* task rather than a statement per *owned* task. The loop had no choice about
either — it planned a fresh `UPDATE … WHERE id = $1` per row. Nothing here is a timing claim; what
changed is the statement count, which is `3 × N` before and `3` after.

### 6.2 Deferred: 23 inbound foreign keys with no index, and why partial indexes cannot help them

After the release trigger runs, PostgreSQL enforces every inbound reference to the principal row
being deleted. `principals` has **27 inbound foreign keys. Four have an index that can serve the
referential check; 23 do not**, and each of those is a full scan of the child table. Counted from
the migration on 2026-09-12 with this catalog query — keep it, it is the expensive part of this
record:

    -- An index serves a referential check when it is total (see below) and its leading key
    -- columns cover every column of the foreign key.
    WITH inbound AS (
        SELECT child.relname AS child_table, reference.conname AS constraint_name,
               reference.conrelid AS child_oid, reference.conkey AS fk_columns,
               array_length(reference.conkey, 1) AS fk_width
          FROM pg_constraint AS reference
          JOIN pg_class AS child ON child.oid = reference.conrelid
         WHERE reference.contype = 'f'
           AND reference.confrelid = 'public.principals'::regclass
    ), served AS (
        SELECT inbound.*,
               (SELECT string_agg(pg_get_indexdef(candidate.indexrelid), ' | ')
                  FROM pg_index AS candidate
                 WHERE candidate.indrelid = inbound.child_oid
                   AND candidate.indpred IS NULL
                   AND (candidate.indkey::int2[])[0:inbound.fk_width - 1] @> inbound.fk_columns
               ) AS serving_index
          FROM inbound
    )
    SELECT child_table, constraint_name,
           (SELECT string_agg(attribute.attname, ', ' ORDER BY ordinality)
              FROM unnest(fk_columns) WITH ORDINALITY AS fk(attnum, ordinality)
              JOIN pg_attribute AS attribute
                ON attribute.attrelid = child_oid AND attribute.attnum = fk.attnum) AS fk_columns
      FROM served
     WHERE serving_index IS NULL
     ORDER BY child_table, constraint_name;

(`indkey::int2[]` is zero-based; slicing it `[1:width]` silently drops the leading column and
reports four served constraints as unserved. The four that *are* served —
`channel_principal_grants`, `messages`, `participant_identities`, `thread_principals` — are the
check that the query is slicing correctly.)

The 23 unserved constraints, as the query returns them:

| Child table | Foreign key | Columns |
|---|---|---|
| `background_tasks` | `background_tasks_owner_principal_fk` | `company_id, owner_principal_id, owner_principal_kind` |
| `channels` | `channels_preferred_reviewer_fk` | `company_id, preferred_reviewer_principal_id` |
| `delegation_control_commands` | `delegation_control_commands_actor_fk` | `company_id, actor_principal_id, actor_kind` |
| `human_approvals` | `human_approvals_approver_principal_fk` | `company_id, approver_principal_id` |
| `internal_note_tombstones` | `internal_note_tombstones_actor_fk` | `company_id, actor_principal_id` |
| `internal_notes` | `internal_notes_author_fk` | `company_id, author_principal_id` |
| `manual_handoffs` | `manual_handoffs_creator_fk` | `company_id, created_by_principal_id` |
| `manual_handoffs` | `manual_handoffs_resolver_fk` | `company_id, resolved_by_principal_id` |
| `manual_handoffs` | `manual_handoffs_responsible_fk` | `company_id, responsible_principal_id` |
| `notification_events` | `notification_events_actor_fk` | `company_id, actor_principal_id` |
| `notifications` | `notifications_recipient_fk` | `company_id, recipient_principal_id` |
| `response_draft_publications` | `response_draft_publications_publisher_fk` | `company_id, published_by_principal_id` |
| `response_drafts` | `response_drafts_author_fk` | `company_id, author_principal_id` |
| `response_drafts` | `response_drafts_created_by_fk` | `company_id, created_by_principal_id` |
| `response_drafts` | `response_drafts_reviewer_fk` | `company_id, reviewer_principal_id` |
| `response_drafts` | `response_drafts_updated_by_fk` | `company_id, updated_by_principal_id` |
| `response_reviews` | `response_reviews_decider_fk` | `company_id, decided_by_principal_id` |
| `response_reviews` | `response_reviews_notification_actor_fk` | `company_id, notification_actor_principal_id` |
| `response_reviews` | `response_reviews_reviewer_fk` | `company_id, reviewer_principal_id` |
| `task_agent_instructions` | `task_agent_instructions_actor_fk` | `company_id, requested_by_principal_id` |
| `task_approval_waits` | `task_approval_waits_company_id_owner_principal_id_fkey` | `company_id, owner_principal_id` |
| `task_harness_runs` | `task_harness_runs_company_id_owner_principal_id_fkey` | `company_id, owner_principal_id` |
| `task_outreaches` | `task_outreaches_creator_fk` | `company_id, created_by_principal_id, created_by_principal_kind` |

(`plan/db_audit/phase6.md` calls this "twenty-one" and then lists twenty-two names. The query says
23, of which `background_tasks_owner_principal_fk` is the one the release trigger has already
emptied by the time the check runs — so 22 scans that find nothing plus one that scans what the
trigger just cleared. Trust the query, not the prose.)

#### Partial indexes never satisfy foreign-key enforcement

Write this on the wall. It is the single most load-bearing fact in this record and it is not
obvious from reading the migration. A referential check issues, in effect,
`SELECT 1 FROM child AS x WHERE $1 = x.company_id AND $2 = x.<principal_column> FOR KEY SHARE OF x`
— **it carries no status predicate**, so the planner cannot prove any partial index's predicate
holds, and will not use one.

`response_drafts.reviewer_principal_id` is the instructive case: it *has* an index that looks like
a perfect fit, and that index is useless here. Captured on the empty scratch database with
`enable_seqscan = off`, so the planner had every incentive to reach for an index:

    -- the check the foreign key actually runs
    LockRows
      ->  Index Scan using response_drafts_scope_idx on response_drafts x
            Index Cond: (company_id = '…'::uuid)
            Filter: ('…'::uuid = reviewer_principal_id)

    -- the same query with AND x.status = 'pending_review' added
    LockRows
      ->  Index Scan using response_drafts_reviewer_pending_idx on response_drafts x
            Index Cond: ((company_id = '…'::uuid) AND (reviewer_principal_id = '…'::uuid))
            Filter: (status = 'pending_review'::text)

`response_drafts_reviewer_pending_idx (company_id, reviewer_principal_id, created_at, id) WHERE
status = 'pending_review'` serves the second form exactly and cannot be considered for the first.
The check falls back to `response_drafts_scope_idx`, leading on `company_id` with the reviewer as a
filter — a scan of the tenant's drafts. The same reasoning disqualifies
`background_tasks_unsettled_owner_idx` in §6.1 and `response_drafts_one_pending_version_idx`.

#### Why no index is added here, and what would change that

Twenty-three indexes on tables that take writes on every note, draft, review, notification and
outreach is a large permanent cost for a path that runs when somebody removes a teammate or deletes
an agent. The write-rate arithmetic also has to include what "Understood, no change proposed" in
`plan/db_audit/general_plan_instructions.md` records: `background_tasks` already carries eight row
triggers and sixteen indexes, so anything added there is maintained on every claim, lease renewal
and completion.

The method, in order:

1. Land the set-based trigger. **Done** — §6.1.
2. Time the two delete paths against a database with a realistic principal history, which is
   exactly what the skewed seeder under "Evidence Tooling Still To Build" is for. The seeder's
   distribution needs one addition for this: lifetime tasks per principal, with skew, and a long
   tail of principals who own nothing.
3. Add indexes **only** for the tables the timing shows dominate, and only after capturing the
   plan. Expect that to be a handful, not 23 — the child tables differ by orders of magnitude in
   size, and a scan of an empty `internal_note_tombstones` costs nothing.

**Sensitive to:** rows per child table, not to how many foreign keys there are. Twenty-three scans
of small tables is cheaper than one scan of a large one.

### 6.3 Open question for whoever owns the ownership model

The release has no status predicate, so it walks every task the principal ever owned rather than
the open ones. Adding `AND status <> 'completed'` — or restricting to the unsettled set
`background_tasks_unsettled_owner_idx` already names, which would make that partial index usable and
change the cost from lifetime tasks to open tasks — is **not a free predicate**. It changes what the
system records: a completed task would keep pointing at a principal row that no longer exists, which
`background_tasks_owner_principal_fk` forbids. So the predicate implies a decision about whether
historical ownership is retained on the live row, and therefore whether that foreign key becomes
`ON DELETE SET NULL` with a denormalised label beside it.

The argument for narrowing: `task_ownership_events` already preserves the whole ownership history
independently, including `previous_owner_label`, so the live column need not. The argument against:
every board, dashboard and attention query that groups by owner would stop seeing the history it
sees today, and the demotion path (a person leaving the team) is exactly where "who used to own
this" is most likely to be asked.

**Do not decide this in review.** It is recorded here because the set-based rewrite helps under
either answer, so nothing was blocked on it. What would settle it: the product decision about
historical ownership, and — if the answer is "narrow it" — a capture showing
`background_tasks_unsettled_owner_idx` serving the narrowed selection.

## 7. `manual_handoffs` Has No Query-Supporting Index

Recorded 2026-09-12 from `plan/db_audit/phase6.md` §6.2. Same environment as §6.

`manual_handoffs` carries exactly two indexes, both identity keys — `manual_handoffs_pkey (id)` and
`manual_handoffs_company_id_id_key (company_id, id)`. The attention feed's handoff branch
(`src/adapters/persistence/attention.rs`) reads it by
`company_id + channel_id = ANY($2) + status = 'open'`, plus the responsibility filter Phase 4 pushed
into the branch. Captured on the empty scratch database:

    Index Scan using manual_handoffs_company_id_id_key on manual_handoffs handoff
      Index Cond: (company_id = '…'::uuid)
      Filter: ((channel_id = ANY ('{…}'::uuid[])) AND (status = 'open'::text))

Every read walks every handoff row the tenant has ever created, open or resolved, and filters.

**The candidate, and the case for it, made in advance so it can be decided quickly.**

    CREATE INDEX manual_handoffs_open_channel_idx ON public.manual_handoffs
        USING btree (company_id, channel_id) WHERE (status = 'open'::text);

The partial form is probably better than the plain `(company_id, channel_id, status)`: the feed only
ever reads open handoffs, and an open handoff is by definition a small and roughly constant
fraction of accumulated history, so the index stays the size of the working set rather than the
size of the table.

The usual objection to a new index — write amplification — is **weak here, and that is what makes
this the strongest of this section's candidates**. A handoff row is written when a human escalation
happens and updated when it resolves. That is a human-rate event, not a queue-rate one, unlike
`background_tasks` (written on every claim, lease renewal and completion) or `task_attempts` (one
row per claim). So the write cost is close to nothing and the read benefit grows with accumulated
handoff history.

The other readers a candidate has to be checked against, all of them found by grep on 2026-09-12:

- `src/adapters/persistence/attention.rs`, the handoff branch of the feed and the
  responsibility-census `UNION ALL` arm — both `company_id + channel_id = ANY + status = 'open'`.
  These are what the candidate is for.
- the two priority/reassignment reads in the same file —
  `company_id + id + channel_id = ANY + status = 'open' FOR UPDATE`. Served by
  `manual_handoffs_company_id_id_key` already; the candidate neither helps nor hurts them.
- `notification_from_attention_source` (`src/adapters/persistence/notification.rs`) —
  `company_id + id FOR SHARE`. Same: already an identity probe.

So the candidate serves two shapes and leaves three alone, which is the cleanest case in this file.

**It is still Class C and still does not ship.** "Handoffs are rare" is an assumption about a
product that has no users yet, and this gate exists precisely to stop assumptions like that from
becoming permanent schema. **What would close it:** the count and growth rate of `manual_handoffs`
rows, the open-to-resolved ratio, and a before/after plan of the feed's handoff branch at that
cardinality. If handoffs turn out to be created by automation rather than by people, the write-rate
argument above collapses and this reverts to an ordinary trade-off.

## Message-to-task correctness fix (2026-09-12)

The explicitly authorized replacement for the global 800-candidate task lookup ships with a
matching scope-and-order index. This fixes incorrect task links when newer follow-ups evict the
preferred task, while retaining the 200-message page bound. The
[before/after evidence](../../docs/query-evidence/2026-09-12-message-task-lookup.md) records synthetic
custom and generic plans, table/index sizes, and regression coverage. These are correctness and
query-shape checks, not production performance evidence; the traffic-dependent decisions above
remain deferred.

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
- `task_attempts` with realistic attempts-per-task and recent/old `started_at` windows;
- scheduled runs and list-page histories deep enough to exercise §1 and §3; and
- lifetime owned tasks per principal, with skew and a long tail of principals owning nothing, plus
  child rows on the tables §6.2 lists — without those, the two principal-removal paths cannot be
  timed at all.

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
