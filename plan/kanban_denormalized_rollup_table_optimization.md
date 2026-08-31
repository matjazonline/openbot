# Deferred — denormalized `task_chains` rollup table

**Status: deferred. Do not build until the trigger condition below is met and measured.**

Prerequisite: `plan/fix_kanban.md` Phase 3a has shipped and its `EXPLAIN` baseline is recorded.

---

## Context

The Tasks board (`list_task_chain_board`, `src/adapters/persistence/task.rs`) computes a six-column
Kanban by aggregating `background_tasks` and `email_outbox` per `correlation_id` on every render.
Renders are driven by SSE wake-ups, which are driven by writes, so cost scales with concurrent
viewers.

Phase 3a bounded that aggregate by **working set** — unresolved chains plus chains active in the
window — instead of by total history, which is unbounded because `background_tasks` is never pruned
(`dashboard.rs:186`).

This document covers the next step *if* the working set itself grows large enough that aggregating
it per render costs real time. It was deliberately not done first, for a reason worth restating:

**The live query is the rollup's specification, its trigger body, and its only verification oracle.**
Building the rollup while the live aggregate still exists means it can be validated against a known-
good computation over the same data. Replacing the live query with a table would leave any *future*
change to the rollup with nothing to validate against. Going live-query → rollup is materially
cheaper than going rollup-v1 → rollup-v2, and that asymmetry is why this is sequenced second, not
skipped.

## Build it when

The board query is slow **after** Phase 3a — i.e. a single company's working set is large enough
that per-render aggregation is measurably expensive at the concurrency the board actually sees.

Compare against the `EXPLAIN (ANALYZE, BUFFERS)` baseline recorded in the Phase 3a commit. Growth in
*total history* does not justify this; Phase 3a already handles that. Only growth in the working set
does.

**The concrete signal.** Phase 3a adds a tripwire to `list_task_chain_board`:

```
WARN Task board projection is slow  elapsed=… company_id=… chains=…
```

`chains` is the size of the set the pushdown selected — the working set. Read the line as:

| Observation | Reading |
|---|---|
| Line never appears | Phase 3a is sufficient. Do not build this. |
| Appears, `chains` small, `elapsed` high | Not a working-set problem. Check pool contention (`database_acquire_duration_ms`) and viewer concurrency first. |
| Appears, `chains` large and rising | This is the case this document exists for. Build it. |

If the line is absent because nobody deployed the tripwire, that is not evidence of health — verify
it is present before concluding anything.

## Do not build it if

- The board is rarely open, or open by one operator at a time. The rollup adds cost to the queue's
  hot write path to save work on a read nobody is making.
- The channel-filter semantics are still unsettled (see "Open question" below). A stored key freezes
  them; the live query does not.

---

## Design

```sql
CREATE TABLE task_chains (
    company_id     UUID NOT NULL,
    correlation_id UUID NOT NULL,
    stage          TEXT NOT NULL,

    -- the counts from TaskChainCounts, minus expired_processing (see constraint 1)
    total_tasks      BIGINT NOT NULL,
    pending          BIGINT NOT NULL,
    processing       BIGINT NOT NULL,
    pending_approval BIGINT NOT NULL,
    waiting_reply    BIGINT NOT NULL,
    completed        BIGINT NOT NULL,
    failed           BIGINT NOT NULL,
    dead_letter      BIGINT NOT NULL,
    stopped          BIGINT NOT NULL,
    total_deliveries BIGINT NOT NULL,
    delivery_pending BIGINT NOT NULL,
    delivery_sending BIGINT NOT NULL,
    delivery_sent    BIGINT NOT NULL,
    delivery_failed  BIGINT NOT NULL,

    -- lease expiry is evaluated at READ time from this, never stored as a count
    min_lock_expires_at TIMESTAMPTZ,

    retry_count      BIGINT NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL,
    last_activity_at TIMESTAMPTZ NOT NULL,

    PRIMARY KEY (company_id, correlation_id),
    FOREIGN KEY (company_id) REFERENCES companies(id) ON DELETE CASCADE
);

CREATE INDEX task_chains_board_idx
    ON task_chains (company_id, stage, last_activity_at DESC, correlation_id);

-- Which channels a chain touches. Serves both the channel filter and the read-time name join.
CREATE TABLE task_chain_channels (
    company_id     UUID NOT NULL,
    correlation_id UUID NOT NULL,
    channel_id     UUID NOT NULL,
    PRIMARY KEY (company_id, correlation_id, channel_id),
    FOREIGN KEY (company_id, channel_id) REFERENCES channels(company_id, id) ON DELETE CASCADE
);

CREATE INDEX task_chain_channels_channel_idx
    ON task_chain_channels (company_id, channel_id, correlation_id);
```

### Three constraints a naive version gets wrong

**1. `expired_processing` must not be stored.** It is

```sql
COUNT(*) FILTER (WHERE status = 'processing' AND lock_expires_at <= CURRENT_TIMESTAMP)
```

— it changes with **no write at all**, as a lease simply lapses. And `ChainStage::derive`
(`src/domain/entities/task.rs:729`) routes `expired_processing > 0` straight to **Needs Attention**.
A stored count would leave a chain sitting in "Running" until `reap_expired_task_leases` happens to
rewrite the row: a stale-board bug the current live query does not have.

Store `min_lock_expires_at` and evaluate at read time:

```sql
-- in the board read
(chain.min_lock_expires_at IS NOT NULL
 AND chain.min_lock_expires_at <= CURRENT_TIMESTAMP) AS has_expired_processing
```

This also means **`stage` cannot be fully stored** — the `needs_attention` rung depends on a
time-dependent value. Two options:

- **B1 (recommended):** store `stage` as the *time-independent* stage, and let the read override to
  `needs_attention` when `min_lock_expires_at <= now()`. One extra `CASE` at read time, and the
  stored column still drives the index.
- **B2:** derive `stage` entirely at read time from the stored counts, using `ChainStage::derive`.
  Simpler and drift-proof, but loses `stage` from the index, so column partitioning and per-column
  limits happen after the scan.

Prefer B1 unless the read-time `CASE` complicates the window functions more than it saves.

**2. Do not store `channel_names`, `agent_names`, or `title`.** All three are join-derived:

- `channel_names` / `agent_names` come from `channels` and `channel_agents`. A channel rename or an
  agent reassignment would fan out to every chain row that ever touched it.
- `title` is `(array_agg(COALESCE(NULLIF(thread.subject,''), task.task_type) ORDER BY created_at,
  id))[1]` — the first task's thread subject, which can change under you.

`task_chain_channels` carries the channel set; join to `channels`/`agents`/`threads` at read time.
Those joins are small and indexed, and the same table answers the channel filter.

**3. Recompute per chain on write; do not apply deltas.** The trigger runs the same aggregate the
board runs, scoped to one `correlation_id` (bounded by chain size, ≤200 tasks):

```sql
CREATE FUNCTION refresh_task_chain() RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO task_chains (company_id, correlation_id, stage, total_tasks, ...)
    SELECT ... FROM background_tasks
     WHERE company_id = NEW.company_id AND correlation_id = NEW.correlation_id
     GROUP BY company_id, correlation_id
    ON CONFLICT (company_id, correlation_id) DO UPDATE SET ...;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
```

Drift becomes impossible by construction, and the cost is an indexed aggregate over one chain rather
than an O(1) counter update — a trade worth making for a derived table that must not lie.

### Share the SQL between board and trigger

The board body and the trigger body are the same aggregate with a different `WHERE`. Compose them
from one constant, following the pattern already used at `src/adapters/persistence/dashboard.rs:222-228`:

```rust
const CHAIN_ROLLUP_SELECT: &str = r#"SELECT company_id, correlation_id, ... FROM background_tasks"#;
const CHAIN_ROLLUP_GROUP:  &str = r#" GROUP BY company_id, correlation_id"#;

static CHAIN_ROLLUP_ONE: LazyLock<String> = LazyLock::new(|| {
    format!("{CHAIN_ROLLUP_SELECT} WHERE company_id = $1 AND correlation_id = $2{CHAIN_ROLLUP_GROUP}")
});
```

One definition, two call sites, no chance of the trigger and the oracle disagreeing about what a
count means.

### Triggers to attach

Mirror the existing `*_notify_chain` set, since they already identify every table whose change can
alter a chain's rollup:

| Table | Fires on |
|---|---|
| `background_tasks` | `AFTER INSERT OR UPDATE OF status, retry_count, lock_expires_at, channel_id` |
| `email_outbox` | `AFTER INSERT OR UPDATE OF status` (guarded by `OLD.status IS DISTINCT FROM NEW.status`) |
| `background_tasks` | `AFTER DELETE` — remove or recompute the chain row |

`task_chain_channels` is maintained from the `background_tasks` trigger (insert the channel on first
task, and on `AFTER DELETE` remove channels no task still references).

Note the interaction with `plan/fix_kanban.md` Phase 3c: the trigger that refreshes the row is the
natural place to fire `pg_notify('task_chain_changed', ...)` from, replacing the current
`task_status_events` trigger for that purpose — one notify per rollup change instead of one per
underlying row.

---

## Absorbs Phase 6

If `stage` becomes a stored column with one writer, `plan/fix_kanban.md` **Phase 6** (one source of
truth for stage derivation) largely dissolves:

- The SQL `CASE` in `staged` and `ChainStage::derive` stop being two independent implementations.
- The `debug_assert_eq!` at `task.rs:353` goes away.
- `ChainStage::derive` keeps exactly one production role under option B1: computing the stored value
  inside the trigger, or the read-time `needs_attention` override.

Revisit Phase 6's scope if this document is ever executed. Do **not** do both independently.

---

## Reconciliation

The retained Phase 3a aggregate is the oracle:

```sql
-- must return zero rows
SELECT correlation_id, stage, total_tasks, pending, ... FROM (<live board aggregate>) AS live
EXCEPT
SELECT correlation_id, stage, total_tasks, pending, ... FROM task_chains WHERE company_id = $1;
```

Run it as a DB-backed test after driving a chain through every transition the queue supports:
enqueue, claim, fail-and-retry, approval request and accept, outreach start / reply / timeout /
extend, operator stop and resume, lease expiry and reap, delivery queue through to sent and failed.

Production reconciliation is deliberately **not** included. Recompute-on-write cannot drift, so a
background sweep would be verifying an invariant the design already guarantees. Add one only if the
test above ever fails for a reason other than a bug in the trigger itself.

---

## Backfill and rollout

1. Add both tables and the trigger functions to the squashed init migration in place, per the
   project convention; recreate both DBs.
2. Backfill in the same migration by running the live aggregate across all companies:
   `INSERT INTO task_chains SELECT ... FROM (<live aggregate, no company filter>)`.
3. Keep `list_task_chain_board` as-is behind the read switch until the reconciliation test is green.
4. Switch the board read to `task_chains` joined to `task_chain_channels` + `channels` + `agents`.
5. **Keep the live aggregate in the codebase** as the oracle. It is not dead code; it is the test.

Rollback is switching the read back — the table is derived, so nothing is lost.

---

## Open question to settle before building

Whether a channel-filtered board should show whole-chain counts (current behaviour, pinned by
`task_chain_board_groups_by_correlation_and_keeps_complete_chain_under_channel_filter`: filter on
Second Channel, `total_tasks == 2`, `channel_names == ["First Channel", "Second Channel"]`) or only
that channel's tasks.

- **Whole-chain (current):** rollup stays keyed by `(company_id, correlation_id)`, `stage` stays
  stored, `task_chain_channels` handles selection and names. This is the design above.
- **Channel-scoped:** rollup key becomes `(company_id, correlation_id, channel_id)`, the
  all-channels view sums across rows, and `stage` must be derived at read time from the summed
  counts — giving back most of the Phase 6 absorption.

The live query changes this in a few lines. A stored key does not. **Settle it before executing this
document, not before Phase 3a.**

---

## Done when

- [ ] Working-set growth measured against the Phase 3a baseline justifies the change.
- [ ] Channel-filter semantics settled.
- [ ] `task_chains` + `task_chain_channels` in the init migration; both DBs recreated.
- [ ] Aggregate SQL shared between board and trigger via one `LazyLock` composition.
- [ ] `expired_processing` evaluated at read time from `min_lock_expires_at`; no stale "Running".
- [ ] No channel/agent/title text stored; all joined at read time.
- [ ] Reconciliation test green across every transition the queue supports.
- [ ] Live aggregate retained as the oracle, with a comment saying why it is not dead code.
- [ ] `plan/fix_kanban.md` Phase 6 re-scoped or closed as absorbed.
