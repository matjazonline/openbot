# Phase 1 — Schema hygiene and tenant-scope doctrine

Four independent Class A changes. None can regress a query plan: one deletes an index that is a
byte-for-byte copy of another, two add a predicate the caller already knows, and one collapses three
copies of a statement into one. Land them together or separately; they do not depend on each other.

Read `general_plan_instructions.md` first, particularly the note that most SQL here is unchecked at
compile time.

---

## 1.1 Drop the duplicate `human_approvals` index

`migrations/20260817000000_init_schema.sql:5136` and `:5143` create the same index twice:

```sql
CREATE INDEX human_approvals_expiry_due        ON public.human_approvals USING btree (expires_at, id) WHERE (status = 'pending'::text);
CREATE INDEX human_approvals_pending_expiry_idx ON public.human_approvals USING btree (expires_at, id) WHERE (status = 'pending'::text);
```

Identical column list, identical order, identical predicate. Postgres maintains both on every
insert and every status transition of an approval, and the planner will only ever use one. Confirmed
by grouping `pg_indexes.indexdef` with the index name stripped — it is the only exact duplicate in
the schema.

**Change.** Delete the `human_approvals_expiry_due` statement and its `-- Name:` comment block
(`:5133`–`:5137`), keeping `human_approvals_pending_expiry_idx`. The kept name matches the
`_idx` convention used by every other partial index in the file; `_expiry_due` is the outlier.

**Before deleting, grep for the name.** `scripts/db-stats.sh` prints index statistics by name, and
`src/adapters/persistence/delivery/tests.rs:1065` shows the repo does assert on
`pg_indexes.indexdef` by index name in at least one place. If `human_approvals_expiry_due` is named
in a test, a doc or a script, that reference moves to the surviving name rather than being deleted.

**Verify.** After `./scripts/reset-db.sh --all`, this must return one row, not two:

```sql
SELECT indexname FROM pg_indexes
 WHERE schemaname = 'public' AND tablename = 'human_approvals'
   AND indexdef LIKE '%(expires_at, id)%';
```

**Regression guard.** Add a test in the same style as the existing index assertion in
`delivery/tests.rs:1065`: group `pg_indexes` by normalised definition across the whole `public`
schema and assert the result is empty. That catches the *next* duplicate rather than only this one,
and it costs one query:

```sql
SELECT tablename, string_agg(indexname, ', ')
  FROM (SELECT tablename, indexname,
               regexp_replace(indexdef, '^CREATE (UNIQUE )?INDEX \S+ ON', 'CREATE \1INDEX ON') AS body
          FROM pg_indexes WHERE schemaname = 'public') d
 GROUP BY tablename, body HAVING count(*) > 1;
```

---

## 1.2 Scope `fetch_thread_tasks` to its company, and bound it

`src/adapters/persistence/thread/views.rs:174`:

```sql
SELECT id, source_message_uuid, correlation_id, task_type, created_at
  FROM background_tasks
 WHERE thread_id = $1
 ORDER BY created_at ASC
```

Two defects, in order of seriousness.

**No `company_id`.** `src/adapters/persistence/AGENTS.md` requires the tenant identifier in any
tenant-scoped read, and every other `background_tasks` read in the codebase carries it. This is not
currently a cross-tenant leak — `thread_id` is a v4 uuid and threads belong to exactly one company —
but the safety rests on uuid uniqueness rather than on a predicate, which is precisely the
substitution that file forbids. It also means the read cannot use any of the seven
`(company_id, …)` indexes; it is served by `background_tasks_thread_idx (thread_id) WHERE thread_id
IS NOT NULL` and nothing else.

**No `LIMIT`.** Every task ever run in the thread is loaded into a `Vec`, on a page render. A long
thread with a scheduled agent on it grows this without bound, and `src/AGENTS.md` requires an
explicit bound at every boundary driven by stored data.

**Change.** Thread `company_id` through `fetch_thread_tasks` from its caller — find it with
`graft callers fetch_thread_tasks` and take the value the caller already holds rather than
re-reading it — and add both the predicate and a bound:

```sql
SELECT id, source_message_uuid, correlation_id, task_type, created_at
  FROM background_tasks
 WHERE company_id = $1 AND thread_id = $2
 ORDER BY created_at ASC
 LIMIT $3
```

`(company_id, thread_id)` has no dedicated index and does not need one: `background_tasks_thread_idx`
still leads, and `company_id` becomes a filter on an already-tiny result.

**Choosing the bound.** Look at what the caller does with the rows before picking a number. If it
renders every task, the bound is a display decision and belongs next to the other page constants; if
it only needs the newest few, invert the sort to `created_at DESC` and take a small `LIMIT`, then
reverse in Rust. Do not invent a number without reading the consumer.

**Test.** Two tasks in the same thread under one company, one task under a second company with a
fabricated matching `thread_id` is not constructible (the composite foreign key forbids it) — so
test the bound instead: insert more tasks than the limit, assert the returned length equals the
limit and that the rows are the expected end of the ordering.

---

## 1.3 Scope the attention feed's delegation branch

`src/adapters/persistence/attention.rs:87`, `ATTENTION_SQL`. Five of the six union branches join
`principals` and `background_tasks` on `(company_id, id)`. The `delegation_decision` branch does not:

```sql
FROM task_outreaches AS outreach
JOIN background_tasks AS task ON task.id = outreach.task_id
```

and neither do two of the anti-joins:

```sql
NOT EXISTS (SELECT 1 FROM task_outreaches AS outreach
             WHERE outreach.task_id = task.id AND outreach.status = 'timeout_pending_approval')
```

```sql
NOT EXISTS (SELECT 1 FROM task_outreach_targets AS target
              JOIN task_outreaches AS outreach ON outreach.id = target.outreach_id
             WHERE target.delivery_id = delivery.id
               AND outreach.status = 'timeout_pending_approval')
```

The outer `WHERE task.company_id = $1` keeps the results correct, so this is a consistency defect
rather than a leak. It still matters: the file's own doctrine is that the tenant identifier
participates in the join, and `task_outreaches` carries the composite key
`task_outreaches_company_task_id_key (company_id, task_id, id)` that makes the scoped join free.

**Change.** Add `AND task.company_id = outreach.company_id` to the delegation join, and
`outreach.company_id = task.company_id` / `target.company_id = delivery.company_id` to the two
anti-joins. Nothing else in the statement moves — Phase 4 rewrites the same query for a different
reason, so either land this first and rebase Phase 4 on it, or fold this into Phase 4 and say so in
the commit. Do not do both.

**Test.** The existing attention tests (`src/adapters/persistence/attention_tests.rs`) already
exercise every branch; the assertion to add is that the returned item set is byte-identical before
and after. If no test currently covers the `delegation_decision` branch, that gap is worth closing
here rather than in Phase 4, where the same branch is being restructured.

---

## 1.4 One outreach tally, not three

The same statement appears verbatim at `src/adapters/persistence/task/operations.rs:226`,
`operations.rs:618` and `task/controls.rs:394`:

```sql
SELECT COUNT(*) FILTER (WHERE status IN ('active', 'responded'))::bigint,
       COUNT(*) FILTER (WHERE status = 'responded')::bigint
  FROM task_outreach_targets WHERE outreach_id = $1
```

All three destructure into an `(i64, i64)` and all three immediately hand the pair to
`required_response_count` or `outreach_progress`, which already live together in
`src/adapters/persistence/task/outreach.rs:17` and `:21`. The tally belongs beside them.

It also lacks `company_id`, for the third time in this phase. The primary key is
`task_outreach_targets_pkey (outreach_id, email)` so the read is indexed either way, but the same
doctrine applies, and a single helper is the cheapest place to fix it once.

**Change.** Add to `task/outreach.rs`:

```rust
/// Targets that still count toward the threshold, and how many of them have answered.
///
/// The only place this pair is derived. All three transition paths -- a reply landing, a control
/// command, and the timeout sweep -- must weigh identical numbers, and three copies of the
/// statement is three chances for them not to.
pub(crate) async fn tally_outreach_targets(
    executor: impl sqlx::PgExecutor<'_>,
    company_id: Uuid,
    outreach_id: Uuid,
) -> AppResult<(i64, i64)>
```

The executor generic matters: the three call sites hold, respectively, a `&mut PgConnection`, a
`&mut Transaction` and a `&mut *tx`. `sqlx::PgExecutor` covers all three without any of them
changing how they manage their transaction. Verify that against the sqlx version pinned in
`Cargo.toml` before writing the signature — if the lifetimes fight, take
`&mut PgConnection` and let the callers reborrow.

**Test.** One test on the helper covering the three states that matter — an `active` target, a
`responded` target, and a target in neither state (so the first count excludes it) — plus the
assertion that a second company's outreach with targets does not contribute. The three call sites
keep their existing behavioural tests unchanged; if any of them changes result, the extraction was
wrong.

---

## Acceptance criteria

- `pg_indexes` reports no two indexes in `public` sharing a normalised definition, enforced by a
  test rather than by inspection.
- `fetch_thread_tasks` carries `company_id` and a bound, and the bound is justified by what its
  caller renders.
- Every join and anti-join in `ATTENTION_SQL` carries `company_id`, matching the five branches that
  already did.
- `grep -rn "FILTER (WHERE status IN ('active', 'responded'))" src/` returns exactly one non-test
  hit, in `task/outreach.rs`.
- `./scripts/reset-db.sh --all` succeeds, `cargo sqlx prepare -- --all-targets` produces no diff
  beyond the statements touched here, and `cargo test` plus
  `cargo clippy --all-targets -- -D warnings` are green.
