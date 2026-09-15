# Live open-task counts: one SSE, shown on Channels and Agents

## Context

Operators can't see how much work is queued or running per channel, per agent, or for the whole
company without opening the Tasks board. We want live counts on the Channels and Agents workspaces,
fed by **one** SSE endpoint that both pages reuse rather than a stream per widget.

Decisions (confirmed with user):
- **Buckets: open work only.** Pending · Active (`processing`) · Waiting (`pending_approval` +
  `waiting_for_third_party_reply`). Completed/failed/dead-letter/stopped are not counted.
- **Per agent = tasks the agent currently owns.** Owner principal kind `agent`, joined to
  `principals.agent_id`. Unassigned/human-owned tasks count for channel + company only.
- **Placement:** Channels sidebar rows + company total in the Channels sidebar; Agents sidebar rows +
  company total in the Agents sidebar; the agent pane header, which is visible on both the Settings
  and Channel tabs.
- **Refresh:** an immediate initial snapshot, then at most one refresh per **5 seconds per stream**.
- **Compatibility:** no backward compatibility is required; the database will be reset. Do not add
  fallback implementations or staged migrations to preserve the previous design.

No schema change is currently required, and the wake source already exists. Start with
`background_tasks_company_status_created_idx (company_id, status, …)` and verify the query plan
against representative data before deciding whether an additional index is warranted.

## Design

**Wake source (existing, no trigger change).** The `background_tasks_notify_attention` trigger fires
`attention_changed` on INSERT, DELETE, and UPDATE OF status/owner. The payload carries
`company_id, channel_id, source_kind='task'`. It already arrives as
`MailboxEvent::AttentionChanged(AttentionScope)`. Priority/due edits also fire it. These still cost
a refresh when eligible, even though an unchanged snapshot is not re-sent.

**One aggregate query per refresh**, in addition to authorization and visible-channel reads; all
three scopes folded in Rust:
```sql
SELECT task.channel_id, principal.agent_id, task.status, COUNT(*)::bigint AS count
  FROM background_tasks AS task
  LEFT JOIN principals AS principal
    ON principal.company_id = task.company_id AND principal.id = task.owner_principal_id
   AND task.owner_principal_kind = 'agent'
 WHERE task.company_id = $1 AND task.status = ANY($2) AND task.channel_id = ANY($3)
 GROUP BY task.channel_id, principal.agent_id, task.status
```
`$2` = the four open statuses, derived from `TaskStatus`. `$3` = the viewer's visible channel ids,
the same visibility the Tasks board uses. A count never reveals work the board would hide.

**Stream `GET /ui/task-counts/events?company_id=`**, following the `task_board_stream` pattern:
- Authorize with `load_managed_company`, the same owner/admin gate as both pages.
- Subscribe to wake-ups **before** the first query.
- Each pass verifies the viewer is still a manager and re-reads readable channels through the
  shared application visibility method described below. It then counts, renders one fragment,
  and yields event `task-counts` only if the fragment differs from the last one sent. The first
  pass always sends.
- **Throttle, not debounce:** `TASK_COUNTS_REFRESH_INTERVAL = 5 s`. Track a pending-refresh flag
  and a monotonic next-eligible deadline. After each pass finishes, set that deadline to now + 5 s.
  Matching wakes set the flag; repeated wakes do not extend the deadline. Once pending and eligible,
  run one pass. A wake after an idle period can refresh immediately. Keep only one pass in flight.
- Clear the pending flag when starting a pass, never after it. Keep the subscription alive during
  the query so wakes arriving during the read remain available for the next pass. Do not discard
  queued wakes after querying; the query may have captured state before those changes committed.
- `Wake::Lagged` also requests a full recount through the same five-second gate. It does not bypass
  the throttle. Sustained churn cannot starve the eligible refresh deadline.
- A **60 s recheck tick** requests a pass through that same gate, with missed ticks skipped instead
  of replayed. It reconciles notifications lost during a `PgListener` reconnect and permission
  changes even without a wake. Allow up to 60 s plus the remaining cooldown and query time for this
  fallback. Wakes, lag, and ticks share one pending flag and never schedule duplicate passes.
- Any query error ends the stream. EventSource reconnects and re-authorizes, as the board stream does.
- Load: after its initial snapshot, each connected tab performs at most one aggregate query per
  five seconds under churn, plus the authorization/visibility reads for that pass. Idle streams
  recheck once per minute; this is not an additional refresh outside the throttle. New connections
  each need an initial snapshot, so this is a per-stream limit, not a tenant-wide rate limit.
- Verify history pruning and join cost with representative `EXPLAIN (ANALYZE, BUFFERS)` output;
  filtering for open statuses does not itself prove the database only visits open rows. Record
  `task_counts_refresh_duration_seconds` for the full pass, including authorization/visibility,
  with bounded outcome labels and no per-user/company metric labels.

**Payload.** One HTML fragment of `<template data-task-counts-key="…">badges</template>`:
- `company` is always present; when all counts are zero it renders "No open tasks".
- `channel:{uuid}` and `agent:{uuid}` are present only when non-zero.
- An absent key means none.
- Content is numbers and static labels only, rendered once in Rust.

**Client (reuse = slots + one hidden source).**
- Each workspace aside holds a hidden source element:
  `<div hidden data-task-counts-source hx-ext="sse" sse-connect="/ui/task-counts/events?company_id=…" sse-swap="task-counts" hx-swap="innerHTML">`.
  The aside is never swapped; only `#channel-menu`/`#agent-menu` get OOB-replaced. So the connection
  survives pane and list swaps. `closeLiveStreams` already cleans up `[sse-connect]` on navigation.
- Anywhere a count shows is an empty slot: `<span data-task-counts="agent:{id}">`.
- The `TASK_COUNTS_SCRIPT` applies the source's templates to every matching slot:
  - on `htmx:afterSettle` for the source, the whole document;
  - on `htmx:afterSettle` / `htmx:oobAfterSwap` for any other swapped subtree, the new slots in it.
  - It only applies once the source holds a snapshot (the `company` template exists).
- This is what makes a pane fetched mid-update correct: a freshly swapped agent pane takes the
  latest snapshot from the source, instead of depending on an event that fired before its slot
  existed. Page handlers do **not** query counts.

## Changes

1. **Domain.** New `src/domain/entities/task_counts.rs`; `task.rs` is already ~1,900 lines. Register it in `entities/mod.rs`.
   - `TaskCounts { pending, active, waiting }` with `is_empty()`.
   - `TaskCountRow { channel_id, agent_id: Option<Uuid>, status: TaskStatus, count }`.
   - `TaskCountSnapshot { company, channels: BTreeMap<Uuid, TaskCounts>, agents: BTreeMap<Uuid, TaskCounts> }`
     built by a pure `from_rows`.
   - `TaskStatus::OPEN` (or `TaskCounts::bucket(status) -> Option<…>`) so the SQL status list and
     the fold share one exhaustive match.
   - Keep these types independent of SQLx. `TaskCountRow` is typed input to the pure fold, not a
     database row with `FromRow` derives or string statuses.
2. **Port and wiring.** New `src/application/task_counts.rs` defines the narrow
   `TaskCountsReader: Send + Sync` port with required
   `task_counts(company_id, visible_channel_ids) -> AppResult<TaskCountSnapshot>`.
   No default method: an unimplemented reader must not silently report "No open tasks". Register
   the module and wire `Arc<dyn TaskCountsReader>` into application state and the route extractor
   from the existing Postgres persistence instance. Give tests explicit reader implementations;
   the worker's `TaskPersistence` interface does not grow for this read-only feature.
3. **Persistence.** New `src/adapters/persistence/task/counts.rs`, containing `task_counts_on(pool, …)`
   with the runtime `sqlx::query_as` style of `chain_board_on` (`task/board.rs:412`). Register the
   submodule and implement `TaskCountsReader for PostgresPersistence` here. Decode into a private
   `TaskCountDb` row, fallibly parse its status, then feed typed rows to the domain fold. Propagate
   query/decoding errors; do not turn failures into empty snapshots.
4. **Events.**
   - `MailboxEvent::is_task_attention_in_company(company_id)` in `src/infra/events.rs`: matches
     `AttentionChanged` with `source_kind == Task` only.
   - `task_count_wake_ups(events, label, company_id)` in `routes/live_updates.rs`. Add a new
     predicate rather than widening an existing one, per the guard test there.
   - Keep the pending flag/deadline scheduler beside the counts stream in `ui_task_counts.rs`;
     do not add a generic burst coalescer or change unrelated streams' refresh cadence.
5. **Visible channels, one place.** Add an application-level
   `ChannelUseCases::list_managed_readable_channels(viewer, company_id)` that verifies company
   management on every call and reuses the existing `list_readable_channels` logic. Keep
   `Channel::viewer_access` as the underlying decision, including the owner exemption and admin
   allowlist restrictions. Make `TaskMonitorView::channels` (`ui_tasks.rs:887`) and the new stream
   use this method instead of introducing another visibility implementation in HTTP helpers.
6. **Route.** New `src/adapters/http/routes/ui_task_counts.rs` holding the router, a small extractor
   (company/channel use cases, `TaskCountsReader`, monitoring, `MailboxEvents`, viewer), and the
   stream handler. Merge it in `routes/mod.rs` and extend state/extractor wiring explicitly.
7. **Pages.** New `src/adapters/http/pages/task_counts.rs`, exported from `pages/mod.rs`:
   - `TASK_COUNTS_EVENT`
   - `task_counts_source(company_id)`
   - `task_counts_slot(key)`, with `TaskCountKey::{Company, Channel(Uuid), Agent(Uuid)}` rendering
     the attribute value
   - `task_counts_fragment(&TaskCountSnapshot)`
   - `task_count_badges(&TaskCounts)`: daisyUI `badge-sm`, one per non-zero bucket (ghost = pending,
     info = active, warning = waiting), with an `aria-label`/`title` like "3 pending, 1 active, 2 waiting"
   - `TASK_COUNTS_SCRIPT`, added to `application_javascript()` (`mailbox.rs:848`). The CSP forbids
     inline scripts.

   Slots to add:
   - `channel_settings_page` / `agent_settings_page`: a counts bar under `{header}` ("Open tasks" +
     `company` slot + hidden source).
   - `channel_settings_entry` (`channel_settings.rs:291`): `channel:{id}` slot on the name line.
   - `agent_settings_entry` (`agent_settings.rs:316`): `agent:{id}` slot.
   - `agent_edit_pane` header (`agent_settings.rs:424`): `agent:{id}` slot under the address line.

## Tests

- **Domain (pure):**
  - `from_rows` puts each status in the right bucket.
  - The company total equals the sum over channels.
  - Only agent-owned rows reach `agents`.
  - An empty input gives an empty company total.
- **Events:**
  - A task attention change wakes `task_count_wake_ups`.
  - A thread-handoff or delivery attention change, or one in another company, does not.
  - Test at the predicate, per the `live_updates.rs` convention.
- **Refresh scheduler (paused tokio time):**
  - The first pass is immediate. Subsequent passes never start before five seconds have elapsed
    since the previous pass finished, including after an unchanged snapshot.
  - Continuous bursts and events spaced 150 ms apart both obey that bound; sustained events do
    not postpone an eligible refresh indefinitely.
  - Wakes during cooldown collapse into one pending pass; a wake after cooldown refreshes promptly.
  - Lag and a periodic tick during cooldown obey the same bound, including when they coincide.
  - Slow reads never overlap, missed ticks do not cause catch-up bursts, and a change during a read
    produces a later reconciliation instead of being cleared on completion.
- **Persistence** (`test_pool`; assert only on the test's own company, since the DB is shared):
  - A company with 2 channels and 2 agents gives correct channel/agent/company counts.
  - Exercise every `TaskStatus` against the SQL filter and fold together, including both waiting
    statuses and all excluded terminal statuses, so the Rust/SQL status mapping cannot drift.
  - A channel outside `visible_channel_ids` is excluded from all three scopes.
  - An ownership transfer moves the agent count.
  - Candidate fixture: `release_fixture(..).tasks_in_every_status` in `task/release_tests.rs`;
    hoist it into `test_support` if reused.
- **Stream** (`ui_stream_tests.rs` harness):
  - The first frame reflects current counts.
  - Protect the already-correct subscribe-before-query sequence with a controlled reader barrier:
    capture the initial query result, pause before returning it, commit a task change and publish
    its wake, then release the stale result. Assert the next frame reflects the change after the
    five-second cooldown and before the 60-second fallback. This must fail if subscription is
    moved after the initial query; merely inserting before the first frame is read is insufficient.
  - A status change produces a new frame after the cooldown; an unchanged snapshot sends no frame.
  - Another company's `company_id` is refused.
  - Lag causes full reconciliation; a database change with no published wake is recovered by the
    periodic recheck. These paths still obey the five-second gate.
  - Reconnect sends a fresh snapshot. Revoking manager access ends the stream at its next pass and
    refuses reconnect; revoking a channel view grant removes its counts from every scope. Test the
    owner exemption and restricted admin visibility through the shared application method too.
- **Pages:**
  - Slots carry the right keys.
  - Badges render only non-zero buckets.
  - The company key renders "No open tasks" at zero.
  - The source element has `sse-connect` + `sse-swap="task-counts"`.
- **Browser protocol (automated, run in CI):**
  - Use the shipped HTMX and SSE extension in a browser harness with controlled SSE frames and
    delayed pane responses; markup assertions and mocked HTMX callbacks alone are insufficient.
  - Test snapshot-before-pane and pane-before-snapshot. Both settle to the current source snapshot.
  - Replace each sidebar list OOB and confirm new slots receive existing counts without another event.
  - A nonzero key disappearing clears its previous badges; an empty company snapshot displays
    "No open tasks". Slots remain uninitialized until the first snapshot arrives.
  - Complete competing pane reads out of order and confirm `hx-sync="#agent-pane:replace"` keeps
    the last selected agent visible with the correct count; cover channel pane reads likewise.
  - Pane/list swaps preserve exactly one connection. Reconnect rehydrates slots from the new
    snapshot, and navigation closes the old connection.

## Verification

1. Run the targeted Rust tests and automated browser protocol tests, then the required formatting,
   offline compilation, migrations, and full database-backed suite through CI's existing commands.
   Preserve the stack-budget check. Use `DATABASE_URL=postgres://mac03@localhost:5432/mail_agents`
   for Rust tests (`test_pool` redirects to the `_test` sibling); use `own_database` for any fixture
   that invokes global claims or sweeps. Wire the new browser suite into CI explicitly.
2. `DATABASE_URL=… cargo sqlx prepare -- --all-targets`, per `src/AGENTS.md` (the query is runtime,
   so expect no diff; this does not validate the new runtime query).
3. Capture `EXPLAIN (ANALYZE, BUFFERS)` with representative open backlog, retained terminal history,
   channel counts, and ownership distribution. Record table/index statistics and the plan; verify
   the full-pass duration metric. Add an index only if that evidence warrants it and record the
   before/after plans.
4. Run the server (:3001) and open `/ui/channels` and `/ui/agents` in Chrome:
   - Check that one `/ui/task-counts/events` request is open per page.
   - Enqueue/claim/complete a task and watch the channel row, the agent row/header, and the company
     total change without a reload. Under repeated changes, verify the initial snapshot is immediate
     and subsequent refreshes respect the five-second cooldown.
   - Switch agents while a task changes, and check the new pane shows the current count (the client
     re-apply path).
   - Navigate away and check the stream closes.
   - Check light and dark themes.
