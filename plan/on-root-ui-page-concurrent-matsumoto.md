# Live message column on `/ui` via SSE

## Context

`/ui` is a three-column mailbox (channels → threads → messages). The message column
(`#detail-pane` / `#message-scroll`, rendered by `pages::message_pane` in
`src/adapters/http/pages/mailbox.rs:692`) is only refreshed when the user acts: opening a
thread, sending a reply, or clicking the ⟳ button (`mailbox.rs:719`).

But messages arrive from paths the browser never triggers. Inbound SMTP
(`src/adapters/smtp/server.rs`) and the SendGrid webhook
(`src/adapters/http/routes/webhooks/sendgrid.rs`) both end in `finalize_ingest`
(`src/application/use_cases/thread/ingest.rs:824`), which **enqueues** an `email_agent_dispatch`
task rather than replying inline; the `TaskWorker` polls every 3s and writes the agent's reply at
`src/application/use_cases/thread/dispatch.rs:658` — in a different task from any HTTP request.
Approval notes (`src/application/use_cases/approval.rs:311`) and outreach outbound
(`src/application/use_cases/thread/mod.rs:362`) are the same story. An open thread silently goes
stale until reloaded.

(The `/ui` compose/reply path is the exception: `execute_simulation` runs the agent inline, so its
response already contains the reply. Everything else is invisible to the browser.)

There is no SSE, no polling, and no `LISTEN`/`NOTIFY` in the codebase today; refresh is entirely
manual (the ⟳ buttons) or implicit after a send.

Goal: when a message is persisted, every browser with that thread open **appends** it within ~a
second, with no polling, without disturbing a half-typed draft in the composer.

## Does Postgres have a change-subscription mechanism? Yes.

The app is on Postgres (`sqlx` with the `postgres` feature, `Cargo.toml:9`), so
**`LISTEN` / `NOTIFY`** is available, and `sqlx` ships `sqlx::postgres::PgListener` as its async
client. Key properties that make it the right fit here:

- A `NOTIFY` issued inside a transaction is delivered **only on commit** — no "message announced
  but not yet visible to readers" race.
- It works **across processes and machines** sharing the database, unlike a purely in-process
  `tokio::sync::broadcast`. That matters on Fly.io, where more than one machine can serve `/ui`
  while the SMTP listener or task worker that wrote the row runs elsewhere.
- Payload is capped at 8000 bytes, so it must carry **IDs only**, never message bodies.

It is *not* a durable queue: a client not connected at NOTIFY time misses that event permanently.
The design below makes that harmless — every event is a *hint to run a cursor query*, never the
data itself, so a missed notification is recovered by the next one or by reconnect.

## Design

`NOTIFY` (DB trigger) → one process-wide `PgListener` task → `tokio::sync::broadcast` fan-out →
per-connection axum SSE stream → each new message rendered as a chat bubble and **appended** to
`#message-scroll` by htmx's SSE extension.

The stream is driven by a **cursor**, not by the event payload. On every wake-up the handler asks
"what messages in this thread are newer than the last one I sent?" and emits those. That single
mechanism covers three cases at once: normal arrival, `broadcast` lag (several missed events
collapse into one query returning all of them), and reconnect after a dropped connection.

Appending rather than swapping `#detail-pane` is what preserves the composer draft, the caret, and
the user's scroll position.

### 1. Migration — notify on insert

New migration `migrations/2026XXXXXXXXXX_thread_message_notify.sql`. An `AFTER INSERT` trigger on
`thread_messages` (the association table written in `create_message`,
`src/adapters/persistence/thread.rs:407`):

```sql
PERFORM pg_notify('thread_message', json_build_object(
    'thread_id',  NEW.thread_id,
    'channel_id', NEW.channel_id,
    'company_id', NEW.company_id
)::text);
```

A trigger, not a Rust-side emit, because every writer is then covered including future ones, the
notify is bound to the same transaction as the row, and `create_message` does not even have
`company_id` in Rust — it derives it in SQL from `threads` (`thread.rs:382`). The existing schema
already uses triggers this way (`delete_orphan_email_message`, `init_schema.sql:231`).

No sqlx query text changes, so no offline-data regeneration is expected; re-run the prepare step
if the build asks (`src/AGENTS.md`).

### 2. New persistence query — messages after a cursor

**`MessageCursor` newtype** in `src/domain/entities/value_objects.rs`, alongside the existing
`CompanySlug` / `ChannelSlug` / `MessageId` built on the `string_newtype!` macro. It has real
parsing and ordering rules of its own, which is exactly the case `src/AGENTS.md` calls for a
newtype. It encodes `(created_at, id)` — matching the existing index
`thread_messages_thread_created_idx (thread_id, created_at, id)` — with `parse` / `to_string`
mirroring the thread-list cursor helpers in `src/adapters/http/routes/channel.rs:175`. Reuse that
encoding style rather than inventing a second one.

**Trait method** on `ThreadPersistence` (`src/application/use_cases/thread/mod.rs:96`, next to
`create_message`):

```rust
async fn list_messages_after(
    &self,
    thread_id: Uuid,
    after: Option<&MessageCursor>,
    limit: usize,
) -> AppResult<Vec<Message>>;
```

Postgres impl next to `list_messages_by_thread_id` (`src/adapters/persistence/thread.rs:491`),
reusing the same `MESSAGE_SELECT` const (`thread.rs:132`):

```sql
{MESSAGE_SELECT}
WHERE tm.thread_id = $1
  AND ($2::timestamp IS NULL OR (tm.created_at, tm.id) > ($2, $3))
ORDER BY tm.created_at ASC, tm.id ASC
LIMIT $4
```

`after = None` means "from the start of the thread" — correct for a thread whose pane rendered
empty. Note this is ascending with no reversing wrapper, unlike `list_messages_by_thread_id`,
which fetches the newest 200 descending and flips them.

A `ThreadUseCases::get_thread_messages_after` wrapper goes beside `get_thread_history`
(`mod.rs:500`). Every existing `ThreadPersistence` test double needs the new method — there are
five: `src/adapters/smtp/server.rs:1040`, `src/adapters/http/routes/webhooks/sendgrid.rs:457`,
`src/application/use_cases/approval.rs:671`, `src/application/services/task_worker.rs:757`,
`src/application/use_cases/thread/tests.rs:303`.

### 3. Listener + fan-out (new module `src/infra/events.rs`)

- `ThreadMessageEvent { thread_id, channel_id, company_id }` — `Uuid` fields, `Deserialize`.
- `MessageEvents`, wrapping a `broadcast::Sender<ThreadMessageEvent>` (capacity ~256), `Clone`.
- `spawn_thread_message_listener(pool, events, shutdown_rx)`: `PgListener::connect_with(&pool)`,
  `listen("thread_message")`, loop on `recv()`, parse the JSON payload, `send` on the broadcast.
  `PgListener::recv()` reconnects and re-issues `LISTEN` automatically; log and back off on
  parse/connect errors rather than killing the task.
- Spawned in `src/main.rs:37` next to the existing `TaskWorker` / `SmtpServer` spawns, reusing the
  same `shutdown_rx.resubscribe()` pattern.

### 4. AppState wiring

Add `pub events: MessageEvents` to `AppState` (`src/adapters/http/app_state.rs:17`) plus the
matching `impl FromRef<AppState> for MessageEvents` — the file's established pattern (lines
30–88). Construct it in `src/infra/setup.rs::init_app_state` so `main.rs` hands the same instance
to both the listener task and the router.

### 5. SSE endpoint — `GET /ui/events`

New handler in `src/adapters/http/routes/ui.rs`, registered in `router()` (`ui.rs:38`) as
`.route("/ui/events", get(thread_events))`. Query is `ThreadQuery` (`ui.rs:70`) plus
`after: Option<String>`. Auth needs no new wiring: `ui::router()` is merged inside the group
carrying `route_layer(middleware::from_fn(auth::require_auth))`
(`src/adapters/http/routes/mod.rs:33`), and `EventSource` sends the session cookie same-origin.

- **Authorize once at connect** by reusing `channel_use_cases.get_company_channel(...)` and
  `load_channel_thread(...)` (`ui.rs:147`), exactly as `message_pane_fragment` does (`ui.rs:331`).
  `404` if either fails.
- **Starting cursor**: the `Last-Event-ID` request header if present (the browser sets it
  automatically on reconnect from the last `id:` we sent), otherwise the `after` query param that
  `message_pane` baked into the connect URL, otherwise `None`. Preferring `Last-Event-ID` is what
  closes the reconnect gap; preferring `after` over `None` is what closes the gap between page
  render and connect.
- **Stream**: subscribe to the broadcast, filter to `event.thread_id == thread.id` (after the
  authorization check, so no other user's ids are ever observable), and on each hit call
  `get_thread_messages_after(thread.id, &cursor, LIMIT)`. For each returned message emit an
  `axum::response::sse::Event` with `.id(cursor_of(message))`, `.event("message")`, and `.data(
  message_bubble_chat(message))`; advance the local cursor. `RecvError::Lagged` → run the query
  anyway; `RecvError::Closed` → end the stream.
- Also run the query **once immediately on connect**, before waiting on the broadcast, so a
  message that landed during the connect handshake is not stranded until the next one arrives.
- `Sse::new(stream).keep_alive(KeepAlive::default())` — the comment heartbeat keeps the Fly.io
  proxy from dropping an idle connection.

`message_bubble_chat` (`src/adapters/http/pages/mailbox.rs:778`) must become `pub(crate)` so the
route can render one bubble; it stays the single definition of how a message looks, used by both
the full-pane render and the stream. Multi-line HTML is fine — axum's `Event::data` emits one
`data:` line per newline, which the browser rejoins.

Add `tokio-stream` (feature `sync`, for `BroadcastStream`) to `Cargo.toml`; it and `futures` are
currently only *transitive* deps in `Cargo.lock`. `axum` 0.8's SSE support is in the default
feature set and `tokio` is already on `features = ["full"]`.

### 6. Client side — htmx SSE extension

In `ui_layout` (`src/adapters/http/pages/mailbox.rs:251`), beside the existing htmx tag
(`mailbox.rs:262`):

```html
<script src="https://unpkg.com/htmx-ext-sse@2.2.3/sse.js"></script>
```

In `message_pane` (`mailbox.rs:692`), the connection goes on `#detail-pane` — which every existing
pane swap already replaces, so the `EventSource` is torn down and rebuilt with no lifecycle JS —
and the *swap* goes on `#message-scroll`:

```html
<section id="detail-pane" hx-ext="sse"
    sse-connect="/ui/events?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}&after={cursor}"
    class="flex flex-1 flex-col bg-base-100">
  ...
  <div id="message-scroll" sse-swap="message" hx-swap="beforeend"
       class="flex-1 space-y-1 overflow-y-auto px-6 py-4">
```

`{cursor}` is the `MessageCursor` of `pane.messages.last()`, omitted when the thread is empty.

Two touch-ups in `MAILBOX_SCRIPT` (`mailbox.rs:186`):
- The "This thread has no messages yet." placeholder (`mailbox.rs:700`) must be removed on the
  first append — otherwise it sits above the live messages.
- The existing `htmx:afterSettle` handler (`mailbox.rs:237`) fires only when `#message-scroll`
  itself is swapped in, so it will not catch a `beforeend` append. Add a small handler that
  scrolls to the bottom **only if the pane was already near the bottom**, so a live message never
  yanks the view away from someone reading back through history. It must not touch
  `#thread-composer` focus — leaving the caret alone is the point of this design.

`empty_detail_pane` (`mailbox.rs:680`) is untouched — with no thread open there is nothing to
stream.

## Tests

Following the repo's existing patterns.

**Persistence** (`src/adapters/persistence/thread.rs`, beside the `create_message` tests at
`thread.rs:658`): seed a thread with three messages, then assert `list_messages_after` with
`None` returns all three ascending; with the first message's cursor returns the last two; with the
last message's cursor returns empty; and that two messages sharing a `created_at` are ordered and
split correctly by `id` (the tie-break the cursor comparison depends on). One test that a
`LISTEN`ing connection receives a `thread_message` notification after `create_message` commits,
and none if the transaction rolls back.

**Value object** (`src/domain/entities/value_objects.rs`): `MessageCursor` round-trip
encode → parse, ordering matches `(created_at, id)` ordering, and malformed input is rejected
rather than silently treated as `None`.

**Event payload** (`src/infra/events.rs`): the trigger's `json_build_object` shape deserializes
into `ThreadMessageEvent`; a malformed payload is an error, not a panic.

**Page rendering** (`src/adapters/http/pages/tests.rs`, the 1915-line string-assertion suite):
`message_pane` emits `hx-ext="sse"` and an `sse-connect` URL carrying the last message's cursor;
`#message-scroll` carries `sse-swap="message"` and `hx-swap="beforeend"`; a thread with no
messages emits `sse-connect` **without** an `after` param; and the bubble that
`message_bubble_chat` renders standalone is byte-identical to the one inside the full pane for the
same message (this is what keeps the SSE payload and the page in sync).

**Route**: `/ui/events` returns `404` for a thread that belongs to another user's company —
the same fail-closed assertion the existing `/ui/messages` tests make.

## Out of scope (worth a follow-up)

The thread column and channel sidebar still do not live-update, so a message into a *different*
thread needs a manual refresh. The event already carries `channel_id`, so a second `sse-swap`
targeting `#thread-list` is the natural next step once the message column is proven.

## Verification

1. `cargo build`, `cargo clippy --all-targets`, `cargo fmt`, `cargo test`.
2. `sqlx migrate run` against the dev database; confirm the trigger with `\d+ thread_messages`.
3. Two-window manual test: open `/ui` on a thread in one window; send into that thread from a
   second window (`POST /ui/reply`, or the Simulator at
   `/companies/{id}/channels/{id}/simulate`). The first window must **append** the new bubble with
   no full-pane flash.
4. **The draft test** — the reason for this design: type a half-finished message into the composer
   in the first window, then trigger a message from the second. The draft text, the caret, and the
   scroll position must all survive.
5. Cross-process delivery: send real inbound mail through SMTP (or the SendGrid webhook) and let
   the `TaskWorker` produce the agent reply ~3s later. That writer is a different task from any
   HTTP handler — the case a process-local channel alone would not cover.
6. Reconnect: with the thread open, kill the connection (DevTools offline toggle, or restart the
   server). On reconnect the browser sends `Last-Event-ID`; any message written during the outage
   must appear exactly once, with no duplicates of what was already on screen.
7. Network tab: `/ui/events` stays open as `text/event-stream`, shows periodic keep-alive
   comments, and closes when the thread is switched.
