# Fix plan — `kanban` commit (844e85c)

Remediation plan for the review findings on commit `844e85c` ("kanban", 15 files, +2911/-146),
which added the correlation-chain Kanban board, the `task_status_events` ledger, and the
`task_chain_changed` live-update path.

Baseline at time of writing: `cargo build` clean, `cargo test --lib task` → 77 passed.
Every phase below must leave both of those true.

References below prefer file + symbol names over line numbers. Where a line number is retained, it
describes commit `844e85c`; later phases deliberately move enough code that those numbers will not
remain stable.

---

## Shared context

### House rules this work walks into

`src/AGENTS.md` is binding for code written *and* code touched. The rules that bite here:

- **Newtypes / parsed enums over bare strings** — statuses arriving as strings get parsed once at
  the adapter boundary so the match is exhaustive.
- **No flag parameters** — a `bool` that selects behaviour becomes an enum; never a private
  `_inner` with a bool matrix, never a fourth positional parameter.
- **Name your tuples** — any tuple with 3+ elements, or two same-typed elements, becomes a struct.
- **One decision, one place** — extract to a named helper the first time you would write the
  second copy.
- **Don't collapse errors into defaults on authorization paths.**
- **Keep the file and its test module navigable** — split a module at ~1,000 lines; move an inline
  `#[cfg(test)] mod tests` to a sibling file past ~500 lines.
- **Bound work at every external boundary** — advertising a limit without rejecting input that
  exceeds it is not enforcement.
- **Keep `async fn` chains shallow** — prefer extracting a *synchronous* helper; an `async fn` that
  only forwards is pure stack cost.

### Authorized migration exception

The project is pre-release and `migrations/` deliberately holds **one squashed init file**.
Schema changes edit `migrations/20260817000000_init_schema.sql` **in place**; the database is
reset, not migrated.

This plan is an explicit, plan-specific exception to `src/adapters/persistence/AGENTS.md` §1,
which otherwise requires additive migrations. Do not update that guide as part of this work; for
this remediation only, follow the squash-and-reset workflow below.

Editing the init file changes its checksum, so both databases must be recreated:

```sh
dropdb --if-exists mail_agents && createdb mail_agents
dropdb --if-exists mail_agents_test && createdb mail_agents_test
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo sqlx migrate run
```

### sqlx offline cache

The repo mixes runtime-checked queries (`sqlx::query`, `query_as`) with compile-time macros
(`query!`, `query_as!` — `agent.rs`, `company.rs`, `company_invite.rs`). Everything this plan
touches in `task.rs` is runtime-checked, so `.sqlx/` is not strictly affected — but regenerate
anyway after any SQL change, because a stale cache silently builds against the old shape on Fly.io:

```sh
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo sqlx prepare -- --all-targets
```

### Verification loop for every phase

```sh
cargo build
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents_test" cargo test --lib
cargo clippy --all-targets -- -D warnings
cargo fmt
```

DB tests share one database and run in parallel — never assert on whole-table counts, always scope
assertions to the company/task the test created.

### Phase index

| Phase | Goal | Severity | Depends on |
|-------|------|----------|-----------|
| 1 | Pin existing board-card escaping with a regression test | Hardening | — |
| 2 | Fold the additive migration into the squashed init schema | Convention | — |
| 3 | Bound the board query; batch chain detail; scope SSE re-renders | **High** | 2 |
| 4 | Make transition attribution mandatory and row-local | Medium | 2 |
| 5 | Correct transition reasons; make Resume actually resume | Medium | 2, 4 |
| 6 | Enforce SQL/Rust stage parity; make lease-lost explicit | Medium | 2, 4 |
| 7 | Structural cleanup: module split, named tuples, dead code, round trips | Low | 3–6 |

Phases 1 and 2 are independent and can ship immediately. Phase 3 is the largest single win.

---

## Phase 1 — Pin board-card escaping with a regression test

### Goal

Keep operator-supplied channel and agent names escaped on every Kanban card, and leave a focused
test that fails if either output-context escape is removed.

### Current state

`task_chain_card` first joins the names:

```rust
let channels = card.channel_names.join(", ");
let agents = card.agent_names.join(", ");
```

but the same `format!` call already escapes both values at its named arguments:

```rust
channels = escape_html_text(&channels),
agents = escape_html_text(&agents),
```

This is true in commit `844e85c`; the earlier review excerpt stopped before these arguments and
therefore reported a stored-XSS defect that is not present. Channel and agent names are still free
text, so the missing piece is regression coverage, not a production fix.

### Files touched

- `src/adapters/http/pages/tests.rs`

### Design

Do not move or double-apply the existing escape merely to create a production diff. Audit the
remaining interpolations in `task_board.rs` and record the result in the test:

- `task_chain_card` — `title`, `failure_summary` escaped; `short_id` is a UUID slice (safe).
- `chain_timeline` — `event.reason`, `approval.status`, `approval.action_title`,
  `outreach.status` escaped; `delivery.status.label()` is a `&'static str` from an enum (safe).
- `chain_task` — `task_type` escaped; the rest are enum labels and UUIDs (safe).
- `task_board_toolbar` — `list_url` uses `escape_html_attr`; the shared
  `channel_filter_options` helper already escapes `channel.name` and `channel.slug`.

Note that `escape_html_attr` is currently just an alias for `escape_html_text`
(`src/adapters/http/pages/layout.rs:640`), which escapes `& < > " '` — sufficient for both quoted
attribute values and text nodes. No unquoted attributes exist in this file.

### Tests

Keep or add the focused sibling test
`board_cards_escape_operator_supplied_channel_and_agent_names`:

```rust
#[test]
fn board_cards_escape_operator_supplied_channel_and_agent_names() {
    // build a TaskChainCard with:
    //   channel_names: vec!["<script>alert(1)</script>".into()],
    //   agent_names:   vec!["Ops & \"Friends\"".into()],
    let html = /* task_board_page(..) */;
    assert!(!html.contains("<script>alert(1)</script>"));
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(html.contains("Ops &amp; &quot;Friends&quot;"));
}
```

The negative assertion is the one that matters: it fails if the existing interpolation escape is
ever removed. If the focused test is already present in the working tree, retain it rather than
adding a duplicate.

### Done when

- [ ] Existing `task_chain_card` escaping remains unchanged and the whole file is re-audited.
- [ ] `channel_filter_options` is confirmed to escape channel names and slugs.
- [ ] A test asserts the raw `<script>` payload does **not** appear in the rendered board.
- [ ] Existing page tests still pass.

---

## Phase 2 — Fold the migration into the squashed init schema

### Goal

`migrations/20260830000000_task_status_events.sql` (207 lines) is a second migration file. The
project keeps one squashed init file, edited in place, and resets the DB.

### Files touched

- `migrations/20260817000000_init_schema.sql` (edit in place)
- `migrations/20260830000000_task_status_events.sql` (delete)

### Design

Move the 207 lines into the init file as one dependency-ordered task-ledger section. Keeping the
new objects together avoids the contradictory requirement to place triggers beside functions that
appear before the triggers' tables exist.

Current init-file landmarks (line numbers before edit):

| Line | Object |
|------|--------|
| 546 | `CREATE FUNCTION notify_thread_message()` |
| 562 | `CREATE TRIGGER thread_messages_notify` |
| 567 | `CREATE TABLE background_tasks` |
| 678 | `CREATE FUNCTION notify_thread_activity()` |
| 692 | `CREATE TRIGGER background_tasks_notify_activity` |
| 738 | `CREATE TABLE task_attempts` |
| 768 | `CREATE TABLE human_approvals` |
| 809 | `CREATE TABLE email_outbox` |
| 876 | `CREATE TABLE task_outreaches` |
| 910 | `CREATE TABLE task_outreach_targets` |

Placement, after `task_outreach_targets` and its indexes:

1. **`CREATE TABLE task_status_events`** + its two indexes.
   It has FKs to `human_approvals` and `task_outreaches`, so it must follow both. Keep the
   `-- Immutable, metadata-only history …` header comment; it explains why the table holds no
   payload.
2. **`record_task_status_event()` + `background_tasks_record_status_event`** — immediately after
   the table. Keep its current transaction-local-setting comment during the pure fold; Phase 4
   replaces that protocol and comment with the row-local attribution contract.
3. **`notify_task_chain_changed()` + its five triggers** — immediately after the status-event
   trigger, once every referenced table exists. Keep the `-- One small, identifier-only
   notification …` comment.

Then delete the additive file.

Because Phases 5 and 6 also edit this trigger, doing this fold **first** means those phases touch
one file instead of two.

### Verification

```sh
dropdb --if-exists mail_agents && createdb mail_agents
dropdb --if-exists mail_agents_test && createdb mail_agents_test
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo sqlx migrate run
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo sqlx prepare -- --all-targets
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents_test" cargo test --lib
```

Confirm the objects landed:

```sh
psql -U mac03 -d mail_agents_test -c "\d task_status_events"
psql -U mac03 -d mail_agents_test -c \
  "SELECT tgname FROM pg_trigger WHERE NOT tgisinternal ORDER BY tgname"
```

Expect the five `*_notify_chain` triggers plus `background_tasks_record_status_event`.

### Done when

- [ ] `migrations/` holds exactly one file again.
- [ ] Both DBs recreated from scratch; migrate runs clean.
- [ ] `.sqlx/` regenerated and committed.
- [ ] The three DB-backed status-event tests still pass
      (`task_chain_board_groups_by_correlation…`, `status_event_constraints_reject…`,
      `guarded_transitions_emit_only_on_success…`).

---

## Phase 3 — Bound the board query, batch the detail, scope the stream

### Goal

The board query aggregates the company's entire task and outbox history on every render, and every
render is triggered by a write, for every connected viewer. The chain-detail pane costs ~405
sequential round trips. The stream rebuilds that pane on events that have nothing to do with it.

### The defects

**(a) The board's window filter cannot prune anything.** `list_task_chain_board` filters in
`staged`:

```sql
WHERE is_active OR is_unresolved OR last_activity_at >= $3
```

All three are aggregates over `task_rollup`, which has already run `GROUP BY task.correlation_id`
across `WHERE task.company_id = $1` — every task the company has ever had. Postgres cannot push an
aggregate predicate below its own `GROUP BY`, so the filter runs *after* the scan it was meant to
bound. Same for `delivery_rollup` over `email_outbox`.

`background_tasks` is never pruned, and that is deliberate: `dashboard.rs:186` reconstructs queue
depth from the task rows and says so — *"which is only possible because `background_tasks` is never
pruned."* Retention is not available without breaking the dashboard's queue-depth chart.

The board re-renders because a write emitted `task_chain_changed`, so the cost multiplies by the
number of connected viewers.

The same problem was already solved one file over. `QUEUE_DEPTH_BODY`'s `open_tasks` CTE
(`src/adapters/persistence/dashboard.rs:200-212`):

> The `open_tasks` CTE narrows before the join on purpose. The join is buckets x tasks over a table
> with no retention, so without it the 24-hour window scans every task the system has ever run;
> with it the work is bounded by "unfinished, or finished recently", which is what the
> `(company_id, status, created_at DESC, id DESC)` index is for.

**(b) N+1 in `get_task_chain_detail`** (`src/adapters/persistence/task.rs:2174`) — a per-task loop
issuing `list_task_attempts` + `list_task_deliveries`. 1 header + 1 task list + 200×2 + 3 =
**~405 sequential round trips** for a full chain.

**(c) Unscoped SSE fan-out** (`src/adapters/http/routes/ui_tasks.rs:340`) — the selected chain pane
is rebuilt on *any* `task_chain_changed` event in the company.
`MailboxEvent::is_task_chain(correlation_id)` (`src/infra/events.rs:104`) was added for exactly this
gate and has no production caller.

**(d) Frozen window** — `board_filter()` (`ui_tasks.rs:105`) computes `now - 7 days` once and the
stream captures it for the connection's lifetime.

---

### 3a — Push the window filter into `eligible`

**Current selection semantics, precisely.** A chain shows when any of these hold. Note `stopped` is
in **neither** `is_active` nor `is_unresolved`, so stopped chains *do* age off — preserve that.

| Condition | Composed from |
|---|---|
| `is_active` | task `pending`/`processing`/`pending_approval`/`waiting_for_third_party_reply`, or outbox `pending`/`sending` |
| `is_unresolved` | task `failed`/`dead_letter`/`expired_processing`, or outbox `failed` |
| `last_activity_at >= $3` | `MAX(task.updated_at)` or `MAX(outbox.updated_at)` past the cutoff |

`expired_processing` is a subset of `processing`, so it needs no separate arm.

**The change.** Replace the `SELECT DISTINCT` in `eligible` with a `UNION` carrying the same
predicate at row level, then apply the existing channel filter on top:

```sql
eligible AS (
    SELECT correlation_id FROM (
        SELECT correlation_id FROM background_tasks
         WHERE company_id = $1
           AND (status IN ('pending', 'processing', 'pending_approval',
                           'waiting_for_third_party_reply', 'failed', 'dead_letter')
                OR updated_at >= $3)
        UNION
        SELECT correlation_id FROM email_outbox
         WHERE company_id = $1
           AND (status IN ('pending', 'sending', 'failed')
                OR updated_at >= $3)
    ) AS recent
    WHERE $2::uuid IS NULL OR EXISTS (
        SELECT 1 FROM background_tasks AS filtered_task
         WHERE filtered_task.company_id = $1
           AND filtered_task.correlation_id = recent.correlation_id
           AND filtered_task.channel_id = $2
    )
)
```

`UNION` replaces the `DISTINCT`. The outbox arm also closes a real gap: today `eligible` looks only
at tasks, so a chain whose only recent activity is a delivery is selected only incidentally, via the
`staged` post-filter.

**Leave `staged`'s WHERE clause exactly as it is.** With the pushdown it is redundant, and that
redundancy is the point — the two select the same set, so the pushdown cannot change results, only
prune earlier. It is what makes the optimisation testable, and it keeps the aggregate intact as the
specification and oracle that
[`kanban_denormalized_rollup_table_optimization.md`](./kanban_denormalized_rollup_table_optimization.md)
depends on.

**Indexes.** There is no `updated_at` index on either table today (verified against the live
schema). The `OR` resolves as a BitmapOr of two index scans:

```sql
CREATE INDEX background_tasks_company_updated_idx ON background_tasks (company_id, updated_at DESC);
CREATE INDEX email_outbox_company_updated_idx     ON email_outbox     (company_id, updated_at DESC);
```

The status arm is served by the existing `background_tasks_company_status_created_idx`.
`email_outbox` has no `(company_id, status)` index — add one only if `EXPLAIN` on seeded data shows
that arm seq-scanning. Do not add it speculatively.

These go into the squashed init migration in place (Phase 2 should land first so this is one file).

**Instrumentation — make the deferral decision measurable.**

There is no production signal for board cost today, so "we will see how it behaves in production"
does not currently work:

- `init_tracing` (`src/infra/setup.rs`, `init_tracing`) installs a console `fmt` layer only — no
  `with_span_events`, no exporter. The `#[instrument]` attributes on the board routes give span
  *context* on log lines, not durations.
- `runtime_metric_samples` captures machine-level data (RSS, CPU, pool size,
  `database_acquire_duration_ms`) but nothing per-route or per-query.
- The `duration_ms` occurrences in `src/adapters/http/pages/` are rendered task-attempt durations,
  not request timing.

The only existing indicator is indirect and lagging: board queries hogging pool connections would
push `database_acquire_duration_ms` and `pool_active` up, which says something is wrong without
saying what.

Add a threshold log to `list_task_chain_board` while already in this code:

```rust
/// Past this, the per-render projection is worth looking at. Not an SLO — a tripwire for the
/// question `plan/kanban_denormalized_rollup_table_optimization.md` defers.
const BOARD_QUERY_WARN_THRESHOLD: Duration = Duration::from_millis(500);

let started = Instant::now();
let rows = /* the board query */;
let elapsed = started.elapsed();
// Each non-empty stage contributes rows carrying one shared stage_total. Sum one total per stage;
// rows.len() is capped by the display limit and is not the working-set size.
let eligible_chains = rows
    .iter()
    .map(|row| (&row.stage, row.stage_total))
    .collect::<HashMap<_, _>>()
    .into_values()
    .sum::<i64>();
if elapsed > BOARD_QUERY_WARN_THRESHOLD {
    warn!(
        ?elapsed,
        %company_id,
        returned_cards = rows.len(),
        eligible_chains,
        "Task board projection is slow"
    );
}
```

This needs no new infrastructure and turns the deferred document's trigger condition into
something greppable. `eligible_chains`, not the capped returned-card count, is the number compared
with the Phase 3a `EXPLAIN` baseline later.

Deliberately not done here: adding a column to `runtime_metric_samples`. Its schema comment warns
that the table's CHECK constraints are coupled to its column list, so widening it is real friction
for a signal a log line already carries.

---

### 3b — Batch the chain-detail queries

- Add `ChainAttemptDb` using `#[sqlx(flatten)]` so attempts carry `task_id`. Available in sqlx
  0.8.6 and already used by `AccessibleCompanyDb`. `OutboxEntryDb` already carries `task_id`.
- Replace the per-task loop in `get_task_chain_detail` with two `WHERE task_id = ANY($2)` queries:

```sql
SELECT attempt.task_id, attempt.attempt_number, attempt.status, attempt.error,
       attempt.stop_reason, attempt.prompt_tokens, attempt.completion_tokens,
       attempt.result, attempt.started_at, attempt.finished_at, attempt.execution_generation
  FROM task_attempts AS attempt
  JOIN background_tasks AS task ON task.id = attempt.task_id
 WHERE task.company_id = $1 AND attempt.task_id = ANY($2)
 ORDER BY attempt.task_id, attempt.attempt_number

SELECT {OUTBOX_COLUMNS} FROM email_outbox
 WHERE company_id = $1 AND task_id = ANY($2)
 ORDER BY task_id, created_at, id
```

- Group with a **synchronous** helper (`fn group_by_task<T>(...) -> HashMap<Uuid, Vec<T>>`) — a
  non-`async fn` contributes no future frame.
- Bound explicitly and surface truncation rather than silently drawing a partial timeline:

```rust
/// A chain detail pane is an operational view, not an export: past these the pane truncates and
/// says so rather than projecting an unbounded ledger into one HTML response.
const CHAIN_DETAIL_MAX_TASKS: i64 = 200;
const CHAIN_DETAIL_MAX_ATTEMPTS: i64 = 1_000;
const CHAIN_DETAIL_MAX_DELIVERIES: i64 = 1_000;
const CHAIN_DETAIL_MAX_EVENTS: i64 = 200;
const CHAIN_DETAIL_MAX_APPROVALS: i64 = 200;
const CHAIN_DETAIL_MAX_OUTREACHES: i64 = 200;
```

Every bounded query fetches `limit + 1`, trims the sentinel row, and contributes to a single
`truncated: bool` on `TaskChainDetail`; render a visible notice whenever any source was truncated.
Do not infer truncation merely because a result contains exactly the limit. Keep the public
`list_task_status_events` maximum at 200; the detail loader uses an internal event query that may
fetch the 201st sentinel row rather than weakening the public limit.

Result: **7 queries flat** — header, tasks, attempts, deliveries, events, approvals and outreaches —
down from ~405 and independent of the number of tasks returned.

---

### 3c — Scope SSE re-renders and slide the window

```rust
/// A wake means "re-render the selected pane" when it names that chain, or when lag means we
/// cannot tell what we missed.
fn wake_touches(wake: &Wake, selected: Option<CorrelationId>) -> bool {
    match (wake, selected) {
        (Wake::Lagged, _) => true,
        (Wake::Event(event), Some(id)) => event.is_task_chain(id.as_uuid()),
        (Wake::Event(_), None) => false,
    }
}
```

```rust
let mut refresh_selected = true;            // the first pass always paints the pane
let period = Duration::from_secs(60);
let mut window_tick = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
loop {
    let filter = query.board_filter();      // recomputed for every event or timer wake
    match view.board(filter).await { /* yield task-board */ }

    if refresh_selected && let Some(correlation_id) = selected {
        match view.chain(correlation_id).await { /* yield task-chain */ }
    }

    tokio::select! {
        first = changes.next() => {
            let Some(first) = first else { return };
            let mut touched = wake_touches(&first, selected);
            while let Ok(Some(wake)) =
                tokio::time::timeout(Duration::from_millis(75), changes.next()).await
            {
                touched |= wake_touches(&wake, selected);
            }
            refresh_selected = touched;
        }
        _ = window_tick.tick() => {
            // The timer advances terminal_since but does not rebuild an unchanged detail pane.
            refresh_selected = false;
        }
    }
}
```

`Wake::Lagged` must force a refresh — that is the existing "what was missed is unknown, so redo
everything" contract the thread streams implement in their `Wake::Lagged` arm.

**Suppress no-op notifications at the source.** Keep the combined INSERT/UPDATE triggers. At the
top of `notify_task_chain_changed()`, use `TG_OP` inside the trigger function and nested
table-specific checks to return early when an UPDATE leaves `status` unchanged. Apply this to
`email_outbox`, `human_approvals`, and `task_outreaches`; the target-response trigger already uses
`IS DISTINCT FROM`.

Do not put `TG_OP` in a trigger `WHEN` expression, and do not claim this removes genuine
`pending → sending → sent` notifications: those are three material state changes and remain
eligible for the existing 75 ms stream-side coalescing.

**Broadcast pressure.** `TaskChainChanged` now shares the 256-slot broadcast
(`src/infra/events.rs:36`) with the thread streams, so thread subscribers hit `Wake::Lagged` more
often. Measure before acting; if it shows, raise `BROADCAST_CAPACITY` first (the comment already
says it "only needs to be large enough that lag is rare"). Give chain events a separate sender only
if that is not enough — do not do it speculatively.

---

### Tests

- **Equivalence, DB-backed (the important one).** Run the board query with and without the pushdown
  over a seeded company covering every branch: active chain, unresolved chain, chain stopped inside
  the window, chain stopped outside it, chain completed inside, chain completed outside, and a chain
  whose only recent activity is an outbox row. Assert identical card sets. This protects the
  semantics and doubles as the oracle harness Document B reuses.
- **Grouping, DB-backed.** A chain with 3 tasks each having attempts and deliveries; assert every
  attempt and delivery attaches to the *right* task. Association is what breaks when batching, so
  assert per-task, not totals.
- **Truncation.** Cross each of the six detail limits separately; assert the sentinel row is
  removed, `truncated` is set, and the rendered notice is visible. Assert an exactly-at-limit result
  is not marked truncated.
- **Pure, no mocks.** `wake_touches` over the full matrix — `Lagged`/`Event` × selected/none ×
  matching/non-matching.
- **Paused-time SSE test.** Advance Tokio time past the 60-second tick with no mailbox event;
  assert the board is refreshed with a newly computed cutoff and the selected pane is not queried.
- **DB-backed notification test.** A no-op status UPDATE on outbox, approval, and outreach rows
  emits no chain notification; a real status change still emits one.
- Keep `task_chain_board_groups_by_correlation_and_keeps_complete_chain_under_channel_filter`
  green — it pins the channel-filter semantics the pushdown must not alter.

### Measuring

The test DB holds 6 tasks and 1 outbox row — far too small for the planner to show anything. Seed
~50k tasks across ~10k chains, most completed and older than the window, then
`EXPLAIN (ANALYZE, BUFFERS)` the board query. Before: `task_rollup`/`delivery_rollup` row counts on
the order of full company history. After: bounded by working set, BitmapOr over
`background_tasks_company_status_created_idx` and `background_tasks_company_updated_idx`.

**Record both numbers in the commit message.** They are the baseline that decides whether
`plan/kanban_denormalized_rollup_table_optimization.md` is ever needed.

Once deployed, the signal to watch is `Task board projection is slow` in the logs. Rising
`eligible_chains` values mean the *working set* is growing, which is the one thing the pushdown does
not bound and the only thing that justifies the rollup table. `returned_cards` is diagnostic only
and will plateau at the display cap.

### Done when

- [ ] `eligible` selects at row level; `staged`'s filter retained unchanged.
- [ ] Both `updated_at` indexes in the init migration; DBs recreated; `.sqlx/` regenerated.
- [ ] Equivalence test passes across all seven branches.
- [ ] `get_task_chain_detail` issues a fixed 7 queries regardless of chain size.
- [ ] All six detail collections are bounded with limit-plus-one detection; truncation is visible.
- [ ] The chain pane re-renders only on its own chain, or after `Wake::Lagged`.
- [ ] A 60-second timer advances `terminal_since` even when the SSE connection receives no events.
- [ ] `is_task_chain` has a production caller.
- [ ] Outbox, approval, and outreach no-op status writes emit no chain notification.
- [ ] Before/after `EXPLAIN` numbers recorded in the commit message.
- [ ] The slow-query tripwire logs elapsed time, capped returned cards, and true working-set size.

---

## Phase 4 — Make transition attribution mandatory and row-local

### Goal

Delete the four defaulting trait methods, require every stop/resume/failure call to state its
cause, and carry the complete attribution in the same row write that changes task status.

### The defect

The `TaskPersistence` defaults currently have this shape:

```rust
async fn mark_task_failed_with_reason(&self, …, _reason: TaskStopReason) -> AppResult<bool> {
    self.mark_task_failed(lease, error_msg, next_run_at, is_dead_letter).await
}
async fn stop_task_as(&self, id: Uuid, _operator_id: Uuid)   -> …  { self.stop_task(id).await }
async fn resume_task_as(&self, id: Uuid, _operator_id: Uuid) -> …  { self.resume_task(id).await }
async fn resume_task_after_approval(&self, id: Uuid, _approval_id: Uuid) -> …
                                                                   { self.resume_task(id).await }
```

Each default throws away the reason / operator / approval id. These are the **only** path by which
attribution reaches the trigger, so an implementation that forgets to override compiles clean and
writes a permanently wrong audit trail. That is the inverse of the posture the rest of this trait
takes — `commit_agent_dispatch` and the lease renewal both carry explicit "No default:" comments
explaining why a silently-succeeding double is unacceptable.

It also produces a `_as` / `_after_approval` method matrix and `Option<Uuid>` dispatch in
`TaskWorker::stop_task_and_notify` and `TaskWorker::resume_task` — the shape `AGENTS.md` bans under
"No flag parameters".

### Files touched

- `src/adapters/persistence/task.rs`
- `src/adapters/persistence/approval.rs`
- `src/domain/entities/task.rs`
- `src/application/services/task_worker.rs`
- `src/application/use_cases/approval.rs`
- `src/adapters/http/routes/task.rs`
- `src/adapters/http/routes/ui_tasks.rs`
- `migrations/20260817000000_init_schema.sql`
- 6 mock implementors (see below)

### Design

**Replace, don't add.** Use narrow public cause enums for the two operations and one complete
internal actor representation for the trigger context.

Add to `src/domain/entities/task.rs`, next to `TaskTransitionReason`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopActor {
    Operator(Uuid),
    Approval(Uuid),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeActor {
    Operator(Uuid),
    Approval(Uuid),
}

/// Internal representation of all valid trigger actor/source shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransitionActor {
    System,
    Worker(Uuid),
    Operator(Uuid),
    Approval(Uuid),
    Outreach(Uuid),
}

impl TransitionActor {
    pub fn kind(self) -> TaskTransitionActorKind { … }
    pub fn actor_id(self) -> Option<Uuid> { … }      // Worker/Operator when carried directly
    pub fn approval_id(self) -> Option<Uuid> { … }
    pub fn outreach_id(self) -> Option<Uuid> { … }
}
```

Then collapse the trait surface:

```rust
// was: stop_task(id) + stop_task_as(id, operator_id)
async fn stop_task(&self, id: Uuid, actor: StopActor) -> AppResult<BackgroundTask>;

// was: resume_task(id) + resume_task_as(id, op) + resume_task_after_approval(id, approval)
async fn resume_task(&self, id: Uuid, actor: ResumeActor) -> AppResult<BackgroundTask>;
```

And fold the failure path's bool flag away at the same time:

```rust
/// Which side of the retry budget this failure lands on. A bool here reads as
/// `is_dead_letter: false` at the call site and says nothing about why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskFailureOutcome { Retry, DeadLetter }

pub struct TaskFailure<'a> {
    pub lease: TaskLeaseRef,
    pub error: &'a str,
    pub next_run_at: DateTime<Utc>,
    pub outcome: TaskFailureOutcome,
    pub reason: TaskStopReason,
}

// was: mark_task_failed(lease, msg, next, bool) + mark_task_failed_with_reason(…, reason)
async fn mark_task_failed(&self, failure: TaskFailure<'_>) -> AppResult<bool>;
```

No defaults on any of the three. `TaskFailure.lease.worker_id` always becomes
`TransitionActor::Worker`; callers cannot choose a different actor for a fenced worker failure.

**Chosen design — Option B: replace the transaction-local GUCs with row-local attribution.** Add
five nullable columns to `background_tasks` in the squashed init migration:

```sql
transition_reason TEXT,
transition_actor_kind TEXT,
transition_actor_id UUID,
transition_approval_id UUID,
transition_outreach_id UUID
```

Use CHECK constraints matching the event-ledger reason and actor enums, plus a source-shape check
that permits at most one related approval/outreach ID and requires that source ID to match its actor
kind. A null reason requires all four actor/source columns to be null; a non-null reason requires a
non-null actor kind. Add the approval/outreach foreign keys after those referenced tables have been
created. Do not index these columns; they describe the latest intended transition and are not query
dimensions.

Every INSERT or status-changing UPDATE must set **all five** columns in the same statement,
explicitly binding `NULL` for absent values. Never leave a column out of a status-changing `SET`
list: PostgreSQL would copy the previous transition's value into the new row version and the trigger
could not distinguish that stale value from deliberate reuse. Centralize conversion in a
synchronous `TransitionAttribution` struct with named fields, constructed exhaustively from the
typed actor/cause enums.

Change `record_task_status_event()` to read `NEW.transition_*` directly. It writes those values into
the immutable event row, using the deterministic insert/status fallback only when
`NEW.transition_reason IS NULL`; Phase 5 changes the final fallback to `unknown`. Delete both Rust
GUC setters and every `current_setting('mail_agents.task_transition_*', ...)` call.

This returns fenced failure and other single-statement transitions to one database round trip. It
also removes pooled-session state from the attribution protocol. The task row intentionally keeps
the latest transition metadata for inspection; the event ledger remains the history. The accepted
cost is a wider row version on the hot task table and a stricter write contract. It does not add a
second SQL write: the five values travel in the status-changing INSERT/UPDATE that already occurs.

Map the narrow causes in one place:

| Cause | Reason | Internal actor |
|------|--------|----------------|
| `StopActor::Operator(id)` | `OperatorStopped` | `Operator(id)` |
| `StopActor::Approval(id)` | `ApprovalRejected` | `Approval(id)` |
| `ResumeActor::Operator(id)` | `OperatorResumed` | `Operator(id)` |
| `ResumeActor::Approval(id)` | `ApprovalAccepted` | `Approval(id)` |

Add `TaskTransitionReason::ApprovalRejected` and the corresponding schema values in this phase so
the approval stop mapping compiles before Phase 5. Phase 5 removes the remaining raw-action mapping
that currently chooses the wrong reason.

Keep stop predicates cause-specific: an approval rejection may stop only a `pending_approval` task;
an operator may stop the existing operational set. A cause/state mismatch must affect zero rows and
write no ledger event.

**Call-site updates:**

| Site | Change |
|------|--------|
| `TaskWorker` failure path | `mark_task_failed_with_reason(…)` → `mark_task_failed(TaskFailure { … })` |
| `TaskWorker::stop_task_and_notify` | take `StopActor`; delete the `Option<Uuid>` match |
| `TaskWorker::resume_task` | take `ResumeActor`; delete the `Option<Uuid>` match |
| HTTP task routes | pass `StopActor::Operator(user_id)` / `ResumeActor::Operator(user_id)` |
| Approval accept path | pass `ResumeActor::Approval(approval.id)` |
| Approval reject path | pass `StopActor::Approval(approval.id)` |

Inventory every `INSERT INTO background_tasks` and every statement whose `SET` list includes
`background_tasks.status`, including statements in `approval.rs`; each is part of the five-column
protocol even if it previously relied on the trigger's status-only fallback.

Both approval paths currently discard the task-transition result. Change `apply_decision` to return
`AppResult<String>`, propagate the stop/resume error through `process_link_action`, and remove both
`let _ = …` statements. The approval row may already be consumed, so log the approval and task IDs
with the propagated error for reconciliation; do not pretend the side effect succeeded.

**Mock implementors to update (6):**

- `src/adapters/http/routes/webhooks/sendgrid.rs:700`
- `src/adapters/smtp/server.rs:1327`
- `src/application/use_cases/schedule.rs:1064`
- `src/application/use_cases/thread/tests.rs:475`
- `src/application/use_cases/approval.rs:579`
- `src/application/services/task_worker.rs:1889`

Treat the listed lines as commit-`844e85c` anchors; use `impl TaskPersistence for` to locate them
after other worktree changes. Each mock must implement the new signatures explicitly. Consolidating
the materially different mocks is not part of this phase.

### Tests

- Keep `guarded_transitions_emit_only_on_success_and_operator_actions_record_the_user` green — it
  is the regression net for this whole area.
- Add a DB-backed test that an approval-driven resume records `actor_kind = 'approval'` and a
  non-null `related_approval_id` (mirrors the existing `ApprovalRequested` assertion).
- Add the corresponding approval-rejection test: `approval_rejected`, actor kind `approval`, and
  the rejecting approval ID.
- Unit-test all `TransitionActor` mappings, including direct worker attribution.
- DB-backed, exercise every status-changing persistence operation twice and assert the second event
  does not inherit any actor/source column from the first. Include approval → worker and outreach →
  operator transitions, the combinations most likely to expose stale row metadata.
- Schema tests reject mismatched source shapes such as actor kind `approval` without an approval ID,
  both source IDs populated, or `system` with an actor ID.

### Done when

- [ ] `mark_task_failed_with_reason`, `stop_task_as`, `resume_task_as`,
      `resume_task_after_approval` no longer exist.
- [ ] No `TaskPersistence` method that carries attribution has a default body.
- [ ] `is_dead_letter: bool` is gone from the failure path.
- [ ] All five attribution columns are written by every status-changing INSERT/UPDATE.
- [ ] No transition GUC setter or `current_setting('mail_agents.task_transition_*')` remains.
- [ ] Approval accept and reject paths propagate transition failures and carry the approval ID.
- [ ] Worker failures and batch lease loss both produce actor kind `worker` through explicit paths.
- [ ] Consecutive transitions cannot inherit stale actor or related-source values.
- [ ] All 6 mocks updated; full `cargo test --lib` green.

---

## Phase 5 — Correct transition reasons; make Resume actually resume

### Goal

Three reason-mapping defects, and a Resume button that cannot recover a dead-lettered task.

### The defects

**(a) Resume from `dead_letter` is a no-op in practice.** `resume_task_on` and `stop_task_on`
both gained `'dead_letter'` in their allowed status lists in this commit, but resume does not reset
`retry_count`. The worker computes:

```rust
// src/application/services/task_worker.rs:639
let is_dead_letter = dead_letter_now || next_retry >= task.max_retries;
```

with `next_retry = task.retry_count + 1`. A resumed dead-lettered task still has
`retry_count >= max_retries`, so it re-dead-letters on its first failure. The button appears to
work and changes nothing durable.

**(b) An unattributed resume is logged as a worker failure.** The trigger's `CASE` has no arm for
`failed`/`dead_letter` → `pending`, so it falls through to `ELSE 'retryable_failure'`; that reason
maps to actor kind `worker`, usually with no worker ID on this transition. Phase 4 makes operator
and approval attribution explicit rather than teaching the trigger to guess an actor from status.

**(c) Approval action strings are matched loosely.** `src/adapters/persistence/approval.rs:377`:

```rust
let transition_reason = match action {
    "proceed_partial" => TaskTransitionReason::ApprovalAccepted,
    "extend_24h" | "extend_48h" | "extend" => TaskTransitionReason::OutreachExtended,
    "reject" => TaskTransitionReason::OperatorStopped,
    _ => TaskTransitionReason::ApprovalAccepted,
};
```

`"reject"` pairs reason `operator_stopped` with `actor_kind: Approval`, contradicting the trigger's
own mapping (`operator_stopped` → `operator`). And `_ =>` labels *any* unrecognised action as
`approval_accepted` — a typo in a new action string silently records consent that was never given.
This is exactly the bare-string-status problem `AGENTS.md` says to parse into an enum once at the
adapter boundary.

### Files touched

- `migrations/20260817000000_init_schema.sql`
- `src/domain/entities/approval.rs`
- `src/domain/entities/task.rs`
- `src/adapters/persistence/task.rs`
- `src/adapters/persistence/approval.rs`
- `src/application/use_cases/approval.rs`

### Design

**(a) Reset the retry budget on operator resume, not on continuation.**

Resume serves two different intents: an operator retrying failed work, and an approval/outreach
continuation. Only the first should reset the budget.

`resume_task_on` receives `ResumeActor`, converts it to row-local transition attribution, and
derives a local `reset_retry_budget` decision from the typed actor plus the row's old state:

- `ResumeActor::Operator` resets to zero for `failed` and `dead_letter`.
- It also resets a `stopped` task when `retry_count >= max_retries`, covering a dead-lettered task
  that an operator stopped before resuming.
- `ResumeActor::Approval` is accepted only from `pending_approval` and never resets the budget.

Use actor-specific status predicates rather than one broad shared `WHERE`: operator resume accepts
`stopped`, `failed`, and `dead_letter`; approval resume accepts only `pending_approval`. An actor used
against the wrong state returns the existing not-found/conflict error and writes no event.

Keep the decision in one synchronous helper and bind its result into the UPDATE; do not expose a
boolean parameter on the trait. The same UPDATE writes all five transition columns from Phase 4.

`last_error` deliberately remains in place as historical context for the pane. This is the chosen
UX; do not leave a later implementation decision to clear it.

**(b) Stop the fallback from lying.**

Do **not** add a generic `failed`/`dead_letter` → `pending` = `operator_resumed` trigger arm: status
alone does not prove an operator acted. The operator and approval resume statements write
`operator_resumed` and `approval_accepted` into `NEW.transition_reason` explicitly.

`ELSE 'retryable_failure'` asserts a cause it has not established. Add an
`'unknown'` reason to the `task_status_events_reason_check` constraint and to
`TaskTransitionReason`, and use it. A ledger row that says "we did not classify this" is
strictly better than one that says the wrong thing, and it makes unclassified transitions
greppable:

```sql
SELECT from_status, to_status, count(*) FROM task_status_events
 WHERE reason = 'unknown' GROUP BY 1, 2;
```

Tests must scope this query to the company/tasks they create. A whole-table assertion is invalid
because DB tests share one database and run in parallel.

**(c) Parse the approval action once.** Define the shared type beside the approval domain enums so
both the application parser and persistence port can depend on it without reversing dependencies:

```rust
/// Approval actions arrive as strings from a link; parse once so the match is exhaustive and a
/// new action is a compile error rather than a silently-accepted approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuorumTimeoutAction {
    ProceedPartial,
    Extend { hours: u32 },
    Reject,
}

impl FromStr for QuorumTimeoutAction { … }   // "extend_24h" | "extend_48h" | "extend" | …
```

The application already parses link input into `LinkAction`. Represent its quorum-specific branch
with `QuorumTimeoutAction`, pass that type through
`ApprovalPersistence::consume_quorum_timeout_action`, and drive the persistence match from it.
Unknown strings return `AppError::BadRequest` at the existing link-input boundary; the persistence
adapter never parses or guesses. The `_ =>` arms disappear by construction.

Use the `ApprovalRejected` reason added in Phase 4 so `reject` no longer borrows
`operator_stopped` with a mismatched actor kind:

```rust
QuorumTimeoutAction::Reject => TaskTransitionReason::ApprovalRejected,
```

### Tests

- **DB-backed:** dead-letter a task (exhaust `max_retries`), `resume_task(id,
  ResumeActor::Operator(..))`, assert `retry_count == 0`, status `pending`, and that the event
  reason is `operator_resumed` with `actor_kind = 'operator'`.
- **DB-backed:** stop an exhausted task, resume it as an operator, and assert the budget resets.
- **DB-backed:** resume from `pending_approval` via the approval path, assert `retry_count` is
  **unchanged** and the reason is `approval_accepted`.
- **DB-backed:** within each lifecycle test, assert its own company/tasks produced no unexpected
  `reason = 'unknown'` rows.
- **Pure:** `QuorumTimeoutAction::from_str` round-trip, including the unknown-action error.

### Done when

- [ ] Resuming a dead-lettered task actually gets a fresh retry budget; a continuation does not.
- [ ] Resuming an exhausted stopped task also gets a fresh budget for an operator.
- [ ] Resume reasons come from typed row-local attribution, not a status-only trigger guess.
- [ ] The fallback records `unknown`; scoped lifecycle assertions produce none unexpectedly.
- [ ] The typed quorum action crosses the persistence boundary; no duplicate string match remains.
- [ ] `reject` records `approval_rejected` with `actor_kind = 'approval'`.

---

## Phase 6 — Enforce stage parity and make lease-lost explicit

### Goal

The stage rule necessarily exists in Rust and SQL but lacks a release-build parity check; lease
loss is classified by duplicated free-text instead of explicit row-local attribution.

### The defects

**(a) Stage derivation exists in SQL and in Rust.** The production decision is the `CASE` in the
`staged` CTE (`src/adapters/persistence/task.rs`, inside `list_task_chain_board`).
`ChainStage::derive` is a Rust re-implementation used only by a `debug_assert_eq!` in the row
conversion and by the three new unit tests
(`every_individual_chain_state_maps_to_its_operational_stage`,
`chain_stage_precedence_surfaces_mixed_work_and_delivery_failures`,
`completed_chains_require_every_task_and_delivery_to_succeed`).

In release builds nothing checks them against each other. The three tests read like coverage of
board staging and cover the copy production never runs.

**(b) The lease-lost error string is duplicated into the trigger.**
`const LEASE_EXPIRED_ERROR` is matched by literal text in the trigger:

```sql
WHEN OLD.status = 'processing' AND NEW.status = 'pending'
     AND NEW.last_error = 'Task lease expired without the run reporting a result'
    THEN 'lease_lost'
```

Editing the Rust constant silently reclassifies every future lease loss as `retryable_failure`.

### Files touched

- `src/domain/entities/task.rs`
- `src/adapters/persistence/task.rs`
- `migrations/20260817000000_init_schema.sql`

### Design

**(a) Keep each representation in its owning layer and enforce their parity.**

The SQL genuinely needs `stage` as a column — `PARTITION BY stage` drives both the per-column
`ROW_NUMBER()` limit and the `stage_total` count — so it cannot move wholesale into Rust. Keep
`ChainStage::derive` in the domain and the SQL expression in the persistence board module; putting
SQL on the domain type would violate the repository's dependency-direction rule. State plainly
that these are two representations protected by an equivalence test:

```rust
impl ChainStage {
    /// The Rust representation of the board precedence.
    pub fn derive(counts: &TaskChainCounts) -> Self { … }
}

// In adapters/persistence/task/board.rs:
/// The SQL representation used by board window functions. The DB-backed matrix test is the
/// release-build guard that keeps it rung-for-rung identical to `ChainStage::derive`.
const CHAIN_STAGE_SQL_CASE: &str = r#"CASE
    WHEN failed > 0 OR dead_letter > 0 OR stopped > 0
         OR expired_processing > 0 OR delivery_failed > 0 THEN 'needs_attention'
    WHEN pending_approval > 0 THEN 'waiting_approval'
    WHEN processing > 0 OR delivery_sending > 0 THEN 'running'
    WHEN waiting_reply > 0 THEN 'waiting_reply'
    WHEN pending > 0 OR delivery_pending > 0 THEN 'queued'
    WHEN total_tasks > 0 AND completed = total_tasks
         AND delivery_sent = total_deliveries THEN 'completed'
    ELSE 'needs_attention'
END"#;
```

Interpolate `{case}` into the board query via `format!`. This removes the third inline copy from
the query, but does not claim that Rust generates SQL; the matrix below is the drift guard.

**Back it with a real test, not a `debug_assert`.** Add a DB-backed test that pushes a matrix of
`TaskChainCounts` through the actual SQL and compares against `derive`:

```rust
#[tokio::test]
async fn chain_stage_sql_matches_rust_derivation() {
    let Some(pool) = test_pool().await else { return };
    for counts in stage_matrix() {          // every count field at 0 and 1, plus the mixed cases
        let sql: String = sqlx::query_scalar(&format!(
            "SELECT {} FROM (SELECT $1::bigint AS failed, …) AS combined",
            CHAIN_STAGE_SQL_CASE
        )).bind(…).fetch_one(&pool).await.unwrap();
        assert_eq!(ChainStage::from_str(&sql).unwrap(), ChainStage::derive(&counts),
                   "SQL and Rust disagree for {counts:?}");
    }
}
```

This is the test the three existing pure ones were standing in for. Keep them — they document
intent — but this is the one that catches drift.

Then remove the conversion-time `debug_assert_eq!`. With the matrix test in place the assert is
redundant, and a debug-only panic on database data is not the parity mechanism.

**(b) Delete the lease-lost string match rather than syncing it.**

The lease sweep is a set-based UPDATE over rows held by different workers. Option B handles this
without session state: in the same UPDATE, copy each row's current worker into the transition
columns before clearing the lease fields. PostgreSQL evaluates right-hand expressions from the old
row:

```sql
SET transition_reason = 'lease_lost',
    transition_actor_kind = 'worker',
    transition_actor_id = worker_id,
    transition_approval_id = NULL,
    transition_outreach_id = NULL,
    -- existing retry/status fields follow
    worker_id = NULL,
    execution_generation = NULL,
    locked_at = NULL,
    lock_expires_at = NULL
```

Each event therefore records the worker that actually lost that row's lease. No
`TransitionActor::System`, shared batch actor, GUC setter, or trigger fallback is involved.

With that in place, **delete both `NEW.last_error = '…'` arms from the trigger.** The duplication
is gone rather than synchronised.

### Tests

- `chain_stage_sql_matches_rust_derivation` as above — the centrepiece.
- DB-backed: force a lease expiry, run `reap_expired_task_leases`, assert the resulting event has
  `reason = 'lease_lost'` and `actor_id` equal to the worker that held the lease. This is what
  proves the trigger arms are safe to delete.

### Done when

- [ ] Rust and SQL stage representations stay in their owning layers and are protected by a
      DB-backed parity matrix.
- [ ] A DB-backed matrix test compares SQL against Rust; `debug_assert_eq!` removed.
- [ ] The lease sweep sets all five transition columns in its existing set-based UPDATE.
- [ ] Both `last_error` string-matching arms are gone from the trigger.
- [ ] `LEASE_EXPIRED_ERROR` appears only in Rust.

---

## Phase 7 — Structural cleanup

### Goal

Pay down what this commit added on top of existing `AGENTS.md` debt. Do this **last** — Phases 3–6
move a lot of this code, and splitting first means resolving the same changes twice.

### Files touched

- `src/adapters/persistence/task.rs` → `src/adapters/persistence/task/`
- `src/adapters/http/pages/task_board.rs`
- `src/domain/entities/task.rs`
- `src/adapters/http/routes/ui_tasks.rs`

### Items

**(a) Split `persistence/task.rs` — 5,692 lines (this commit added 1,474).**

`AGENTS.md` says split at ~1,000, and move an inline test module past ~500 lines to a sibling.
Proposed decomposition along the phase boundaries already in the file:

| File | Contents |
|------|----------|
| `task/mod.rs` | the `TaskPersistence` trait, shared `*Db` structs and their `TryFrom` impls, `OUTBOX_COLUMNS`, shared consts |
| `task/queue.rs` | enqueue, claim, lease renewal/expiry sweep, complete, `mark_task_failed`, stop/resume |
| `task/board.rs` | `list_task_chain_board`, `get_task_chain_detail`, `list_task_status_events` |
| `task/outreach.rs` | outreach start/response/timeout/extend |
| `task/attempts.rs` | attempt ledger reads and writes |

Keep the row-attribution structs and conversion helpers beside the status-changing queue methods.
Use sibling `*_tests.rs` files for any test module over ~500 lines. The six external mocks have
different state and behavior; update them in Phase 4 but do not claim they can all be replaced by
one shared mock here.

Do this as a **pure move** — no logic changes in the same commit — so the diff is reviewable.

**(b) Name the timeline tuple.** `chain_timeline` currently begins with:

```rust
let mut entries: Vec<(DateTime<Utc>, Uuid, i32, String)> = Vec::new();
```

Four elements, and the sort keys are magic (`10_000` attempts, `20_000` deliveries, `30_000`
approvals, `40_000` outreaches) chosen to sit above real `task_status_events.sequence` values.
Replace with:

```rust
/// One row of the merged chain timeline. `kind` orders entries that share a task and timestamp,
/// and keeps synthetic rows clear of the real `task_status_events.sequence` space.
struct TimelineEntry {
    at: DateTime<Utc>,
    task_id: Uuid,
    kind: TimelineKind,
    sequence: i32,
    html: String,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum TimelineKind { StatusEvent, Attempt, Delivery, Approval, Outreach }
```

Sorting by `(at, task_id, kind, sequence)` derives the ordering from the enum's declaration order
instead of from magic offsets, and removes the "what happens past 10,000 events" question. It also
fixes the current instability where every delivery for a task gets the identical key `20_000`.
Use the real event sequence and attempt number where available; enumerate deliveries, approvals and
outreaches after their existing deterministic `(timestamp, id)` ordering so every key is stable.

**(c) Dead code.** `TaskBoardFilter::with_limit` and `MAX_PER_COLUMN` are never called —
`board_filter()` always takes the default 50 and no query parameter reaches them. Delete both; do
not add a `per_column` wire parameter in this remediation.

**(d) Confirm the row-local attribution result.** Option B is implemented in Phase 4 with five
columns, not left as a choice here. Confirm `mark_task_failed` and other single-statement status
transitions no longer open a transaction solely to call `set_config`; the status and attribution
must land in one UPDATE.

No-op chain-notification suppression belongs solely to Phase 3; do not duplicate its trigger work
in this cleanup phase.

### Done when

- [ ] New `persistence/task/` modules stay near the repository size threshold and oversized inline
      tests moved to sibling files.
- [ ] Other pre-existing oversized files touched by the remediation are recorded as deferred
      structural debt; this phase does not claim repository-wide threshold compliance.
- [ ] `chain_timeline` uses a named struct and an ordering enum; no magic offsets.
- [ ] `with_limit` and `MAX_PER_COLUMN` are deleted.
- [ ] Failure/status transitions write attribution in the same statement, with no GUC round trip.

---

## End-to-end verification

After every phase:

```sh
dropdb --if-exists mail_agents && createdb mail_agents
dropdb --if-exists mail_agents_test && createdb mail_agents_test
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo sqlx migrate run
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo sqlx prepare -- --all-targets

cargo build
cargo clippy --all-targets -- -D warnings
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents_test" cargo test --lib
cargo fmt
```

Manual pass after all phases land (server on `:3001`):

1. Create a channel named `<b>bold</b>`; confirm the board card shows the literal text, not markup.
2. Open `/ui/tasks?view=board` in two browser windows; run a task in an *unrelated* chain; confirm
   only the board column counts move and the selected pane does **not** flicker or re-render.
3. Select a chain, drive it through approval and outreach; confirm the timeline reasons read
   `approval_requested` / `approval_accepted` / `outreach_started`, never `retryable_failure`.
4. Dead-letter a task, hit Resume, confirm it runs again and survives one more failure.
5. Query `task_status_events` for the exercised company/correlation IDs — no unexpected `unknown`
   rows.

### Ledger health query

Worth keeping as an operational check after the whole plan lands:

```sql
SELECT to_status, reason, actor_kind, count(*)
  FROM task_status_events
 WHERE transitioned_at > now() - interval '1 day'
 GROUP BY 1, 2, 3
 ORDER BY 4 DESC;
```

Rows with `reason = 'unknown'`, or an actor_kind that contradicts its reason
(`operator_stopped` + `approval`), mean a call site is still transitioning a task without saying
why.
