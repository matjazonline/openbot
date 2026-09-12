# Phase 5 — The mailbox badge, the thread banner, and the wake-up that keeps them current

Read [`general_plan_instructions.md`](general_plan_instructions.md) first, then
[`phase2.md`](phase2.md), [`phase3.md`](phase3.md) and [`phase4.md`](phase4.md). Everything this
phase renders already exists in the database and is already reachable through
`thread_handoffs_for_threads` (Phase 3 §3.4) and the four action routes (Phase 4 §4.7).

**Goal.** A held thread is obvious without opening it, and stays obvious: a durable **Needs
instruction** badge on the thread row, a banner in the open thread naming who is responsible, how
long it has waited, its priority and its due time, with Phase 4's actions folded into it — and both
of them updating live through the `attention_changed` wake-up the Phase 3 trigger already publishes.

**This phase touches no SQL.** No migration edit, no new statement, no `.sqlx/` regeneration. If a
change here seems to need a query, it is a Phase 3 read that was missed — add it there, not here.

**Files touched**

| File | Change |
|---|---|
| `src/infra/events.rs` | `MailboxEvent::is_thread_handoff_in_channel` |
| `src/adapters/http/routes/live_updates.rs` | `mailbox_thread_wake_ups`, `mailbox_channel_wake_ups` |
| `src/adapters/http/routes/ui.rs` | `page_handoffs`; the two streams; `render_message_pane` |
| `src/adapters/http/pages/thread_handoff_marks.rs` | **new**: the row badge, its slot and its SSE event name |
| `src/adapters/http/pages/thread_handoffs.rs` | `thread_handoff_banner`, absorbing Phase 4's action row |
| `src/adapters/http/pages/mailbox.rs` | `ThreadRowMarks.handoff`; `MailboxPage`/`ThreadColumn`/`MessagePane` fields; the banner slot in `message_pane` |
| `src/adapters/http/pages/mod.rs` | register the module |
| `src/adapters/http/pages/tests.rs` | render tests |
| `README.md` §3.7 | one sentence: what the team sees |

---

## 5.1 The row badge is its own mark, not a `ThreadActivity`

The thread row already has one live mark: `thread_activity_slot`
(`src/adapters/http/pages/thread_activity.rs:35-46`), rendered into the row at
`src/adapters/http/pages/mailbox.rs:2144` and replaced over SSE through its own per-thread event
name, `activity-{thread_id}` (`:44-46`).

**Do not add `ThreadActivity::NeedsInstruction`.** `ThreadActivity` is derived from a
`background_tasks` row (`ThreadActivity::from_task`, used by `list_thread_work_summary` at
`src/adapters/persistence/task/operations.rs:389-466`), and a held thread has **no task** — that is
the entire point of Phase 2. A variant there would have to be synthesised from outside the task
table, and every consumer of `ThreadActivity` — the schedules page badge
(`schedule_run_activity_badge`, `thread_activity.rs:109-131`), the collaboration strip
(`:55-106`), the task board — would silently start rendering a state that has no task behind it.

So the handoff gets a **second, independent slot** beside the activity slot. A new module,
`src/adapters/http/pages/thread_handoff_marks.rs`, mirroring `thread_activity.rs` function for
function:

```rust
/// What a thread row says about its handoff. Absent when the thread has no open handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadHandoffMark {
    pub state: ThreadHandoffState,
    /// Whether anybody has claimed it. The row says "unclaimed" louder than it says "claimed".
    pub claimed: bool,
}

/// The compact mark for a thread row. Empty string for `None`, which is what clears the slot.
pub fn thread_handoff_mark(mark: Option<ThreadHandoffMark>) -> String { … }

/// An independently replaceable handoff mark within a live thread row.
pub fn thread_handoff_slot(thread_id: Uuid, mark: Option<ThreadHandoffMark>) -> String { … }

/// The SSE event name carrying one thread's handoff mark.
pub fn thread_handoff_event(thread_id: Uuid) -> String {
    format!("handoff-{thread_id}")
}
```

Rendering rules, all of which are decisions rather than taste:

- **`needs_instruction` is a text badge, not a glyph.** Every other row mark is an icon
  (`thread_activity_mark` uses `Icon::DotFill`, `Icon::Hourglass`, …). This one carries a **word** —
  `Needs instruction` at `badge-warning badge-xs`, shortened to `Needs input` if it does not fit at
  the column's width — because it is the only mark that asks the reader to do something and an
  unlabelled dot has never once communicated that. `title` carries the long form for the tooltip.
- **`draft_ready` is `Draft ready` at `badge-info badge-xs`**, and **`drafting` is `Drafting` at
  `badge-ghost badge-xs` with no pulse**. `drafting` is deliberately the quietest of the three: the
  general file calls it "visible progress but not actionable work", and it is out of the attention
  queue, so a row that shouted would contradict the queue.
- **Terminal states render nothing.** `thread_handoffs_for_threads` only returns open handoffs, so
  `None` is the normal case and the empty string is what clears a slot after a send or a dismiss.
- **No `animate-pulse` anywhere.** The activity mark pulses while a task runs
  (`thread_activity.rs:14-18`) because that state is transient and self-clearing. A handoff is
  durable and waits for a person; a pulsing badge that never stops is noise.
- `escape_html_text` on every interpolation, `icon(…)`-free markup, and the slot's
  `hx-target="this" hx-swap="innerHTML"` copied exactly from `thread_activity_slot` so the two
  slots behave identically.

`ThreadRowMarks` (`mailbox.rs:2083-2091`) gains `pub handoff: Option<ThreadHandoffMark>` — it is
already a struct rather than positional arguments for exactly this reason, and its doc comment
(`:2080-2082`) says so. `thread_row_fragment` renders the new slot **before** the activity slot at
`:2125-2129`, so the order reading left to right is "what the team must do" then "what the machine is
doing" then the timestamp.

`ThreadColumn` (`:195-206`) and `MailboxPage` (`:177-192`) each gain
`handoffs: &'a HashMap<Uuid, ThreadHandoffMark>`, beside `activity`, with the same "threads with
nothing in flight are absent" contract. `thread_row` (`:2013-2025`) fills the field from it.

In `src/adapters/http/routes/ui.rs`, a `page_handoffs` helper beside `page_activity` (`:329-340`):

```rust
async fn page_handoffs(
    handoffs: &ThreadHandoffUseCases,
    company_id: Uuid,
    visible_channel_ids: &[Uuid],
    threads: &[Thread],
) -> AppResult<HashMap<Uuid, ThreadHandoffMark>>
```

One call to Phase 3's `thread_handoffs_for_threads` mapped into marks. It takes
`visible_channel_ids` and passes it through rather than trusting the thread list, because the page
handler proved channel visibility for the *selected* channel and the mark query must prove it for
every row it answers — the general file's "every new query filters on `company_id` **and** on a
visible-channel list".

## 5.2 The banner in the open thread

`src/adapters/http/pages/thread_handoffs.rs` (created in Phase 1 for the settings page) gains:

```rust
/// The handoff banner for an open thread: who owns it, how long it has waited, and what can be
/// done about it. Absent when the thread has no open handoff.
pub fn thread_handoff_banner(view: &ThreadHandoffBannerView<'_>) -> String;

pub struct ThreadHandoffBannerView<'a> {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub handoff: &'a ThreadHandoff,
    /// Who is responsible, already resolved to a label — `None` renders "Channel team".
    pub responsible_label: Option<&'a str>,
    /// The signed-in reader, for the claim/act decision.
    pub viewer_principal_id: Option<PrincipalId>,
    pub viewer_manages_tasks: bool,
    /// The draft this generation has, when it has one — Phase 4's run row.
    pub draft: Option<ThreadHandoffDraftRef>,
    pub as_of: DateTime<Utc>,
    pub error: Option<&'a str>,
}
```

What it renders, in one `alert`-shaped strip above the composer:

- a **state line**: `Needs instruction`, `Drafting…`, or `Draft ready`, in the same words and colours
  as the row badge, so a reader who saw the badge recognises the banner;
- **who is responsible**: the label, or `Channel team` when `responsible_principal_id` is `NULL` —
  the same fallback string the attention projection uses
  (`src/adapters/persistence/attention.rs:169`), because two different words for one state is how a
  support conversation goes wrong;
- **the age**, from `generation_opened_at` against `as_of`, formatted by the **existing**
  `age_label` (`src/adapters/http/pages/attention.rs:192-203`) — make it `pub(crate)` and call it.
  A second age formatter would drift from the queue's within a month;
- **priority** and **due time**, using `priority_class` (`attention.rs:205-211`) for the badge and
  the queue's `Due %Y-%m-%d %H:%M UTC` / `No due time` wording (`:147-150`);
- **the actions**, which is Phase 4's `thread_handoff_actions` **moved into this markup** rather than
  rendered separately. Phase 4 §4.7 put the buttons in one function precisely so this phase relocates
  markup and changes no behaviour. After this phase `message_pane` calls the banner and the banner
  calls the actions.
- `hx-post` targets, `expected_version` and `expected_generation` come from `handoff` — every button
  carries both, and a button rendered from a stale page therefore fails loudly instead of acting on
  the wrong generation. That is the user-visible half of the whole fencing design and it is why the
  banner takes the entity rather than a flattened view model.

Two conditional behaviours:

- **Unclaimed** (`responsible_principal_id IS NULL`): the primary button is **Claim**, posting to
  Phase 3's command route; `Generate draft` is not rendered at all, because Phase 4 §4.2 refuses it
  on an unclaimed handoff and a button that always fails is worse than no button. A one-line hint
  says why.
- **Claimed by somebody else**, viewer is not a manager: the banner is **read-only** — state,
  responsibility, age, priority, due — with no buttons. A manager additionally gets **Reassign**.
  And per Phase 4 case 22, **Send is offered only to the assigned reviewer**, which for a handoff
  draft is the responsible principal: a manager who is not that principal sees the draft's existence
  but not a Send button, because `execute_command` would refuse them
  (`src/adapters/persistence/response_review/commands.rs:61-69`).

`MessagePane` (`mailbox.rs:209-228`) gains `handoff: Option<&'a ThreadHandoff>` (Phase 4 §4.7
already added it) plus `handoff_responsible_label: Option<&'a str>` and
`handoff_draft: Option<ThreadHandoffDraftRef>`. `message_pane` renders the banner in a **named,
swappable container** beside the existing activity strip (`:2256`):

```html
<div id="thread-handoff" sse-swap="handoff" hx-target="this" hx-swap="innerHTML">{banner}</div>
```

Put it **above** `#thread-activity` and below `#message-scroll`, and assert the order in a test as
`the_message_pane_has_a_slot_for_the_activity_strip` (`pages/tests.rs:2935-2968`) already does for
the activity strip — for the same reason given in that test's comment: `#message-scroll` swaps
`beforeend`, so anything inside it ends up interleaved with the messages.

`render_message_pane` (`src/adapters/http/routes/ui.rs:407-493`) populates the three fields from
Phase 3's reads. It already resolves `viewer_principal_id` (`:420`) and `viewer_manages_tasks`
(`:421-422`), so the banner needs no new authorization lookup — the channel was proved readable by
the caller before `render_message_pane` was reached.

## 5.3 The wake-up

Phase 3's trigger already publishes `{company_id, channel_id, source_kind: "thread_handoff",
source_id}` on the existing `attention_changed` channel; the listener already parses it into
`MailboxEvent::AttentionChanged(AttentionScope)` (`src/infra/events.rs:320-321`, `:70-76`). Nothing
new is published in this phase — the general file's "Phase 5 adds no new PostgreSQL channel" is a
statement about this section.

Today both mailbox streams **discard** it:
`src/adapters/http/routes/ui.rs:845` and `:1002`, each with the comment "wake_ups filters these out
before they reach this stream", because `thread_wake_ups` and `channel_wake_ups`
(`src/adapters/http/routes/live_updates.rs:34-52`) match only messages and activity.

**Add new predicates; do not widen the existing ones.** `thread_wake_ups` is also the simulation
stream's (`src/adapters/http/routes/channel.rs:1138`) and `channel_wake_ups` is also the schedule-run
stream's (`src/adapters/http/routes/ui_schedules.rs:455`). Neither renders a handoff, so widening
those two functions would wake two unrelated streams into a pointless re-query on every claim. So:

```rust
// src/infra/events.rs, beside is_activity_in_channel (:182-184)
/// True for a thread-handoff change anywhere in `channel_id`.
pub fn is_thread_handoff_in_channel(&self, channel_id: Uuid) -> bool {
    matches!(
        self,
        MailboxEvent::AttentionChanged(scope)
            if scope.channel_id == channel_id
                && matches!(scope.source_kind, AttentionWakeSource::ThreadHandoff)
    )
}
```

```rust
// src/adapters/http/routes/live_updates.rs
/// The mailbox column's wake-ups: messages, activity, and handoffs.
pub(super) fn mailbox_channel_wake_ups(events, label, channel_id) -> … {
    wake_ups(events, label, move |event| {
        event.is_message_in_channel(channel_id)
            || event.is_activity_in_channel(channel_id)
            || event.is_thread_handoff_in_channel(channel_id)
    })
}

/// The open thread's wake-ups. The handoff term is channel-scoped, not thread-scoped: see below.
pub(super) fn mailbox_thread_wake_ups(events, label, thread_id, channel_id) -> … { … }
```

**Why the handoff term is channel-scoped even in the thread stream.** `AttentionScope`
(`src/infra/events.rs:70-76`) carries `company_id`, `channel_id`, `source_kind` and `source_id` —
and `source_id` is the **handoff id**, not the thread id. The stream cannot tell whether the change
was on its own thread without a query, and narrowing the payload would mean changing
`notify_attention_changed` (`migrations/20260817000000_init_schema.sql:1028-1068`), which is shared
by six other sources and would put SQL in this phase for a latency optimisation. The cost of the
channel-wide term is one `thread_handoffs_for_threads` call per handoff change per open thread, and
its result is an identical `innerHTML` swap when nothing relevant moved. That is the "SSE is a
wake-up only… readers re-query" contract, and it is the right trade here. Say so in a comment on the
predicate, so the next reader does not "fix" it by inventing a thread-scoped channel.

**`thread_column_stream`** (`ui.rs:870-1023`) then changes in three places:

1. `mailbox_channel_wake_ups` replaces `channel_wake_ups` at `:890`.
2. a `stale_handoffs: HashSet<Uuid>` beside `stale_badges` (`:898-901`), seeded from
   `first_page_thread_ids` (`:343-353`) the same way `initial_stale_badges` is at `:894` — subscribe
   first, then snapshot, so a change during the snapshot query stays buffered and one before the
   subscription is recovered by the snapshot itself. That ordering comment at `:891-893` covers both.
3. a block before the `stale_badges` block that drains `stale_handoffs`, calls
   `thread_handoffs_for_threads` **once** for the drained ids, and yields one
   `thread_handoff_event(id)` event per id carrying `thread_handoff_mark(map.get(&id).copied())` —
   an absent thread yielding the empty payload that clears the badge, exactly as the activity block
   does at `:908-916`.
4. the `AttentionChanged` arm at `:1002` becomes
   `stale_handoffs.extend(first_page_thread_ids(…).await?)`, and the `Wake::Lagged` arm (`:1007-1016`)
   extends **both** sets from the one `first_page_thread_ids` call it already makes.

Re-sending every first-page handoff slot on every handoff change in the channel is deliberate: it is
bounded by `STREAM_BATCH_LIMIT` (50, `:710`), it is one query, the swap is idempotent, and it needs
no per-connection memory of what was last sent. A diff against the last-sent map would be less
traffic and more state; if profiling ever asks for it, it is a local change inside this block.

Streamed rows must arrive with the badge already on them, the same way they already do for activity
(`:931-948` and its comment): `thread_row_fragment`'s `ThreadRowMarks.handoff` is filled from the
same map for the rows the `pending_rows` block yields. A row that streamed in without its badge
would blink from held to plain until the next handoff change, which is the bug that comment was
written about.

**`thread_message_stream`** (`ui.rs:740-861`) changes in two places:

1. `mailbox_thread_wake_ups` replaces `thread_wake_ups` at `:765`, taking `channel.id` as well;
2. a `pending_handoff` flag beside `pending_activity` (`:770-771`), **starting `true`** so a connect
   renders the current banner rather than leaving a reconnected thread with a stale one; its block
   re-reads this thread's handoff and yields one `handoff` event carrying the banner HTML — state,
   not an append, so no `id:` and no cursor, exactly as the activity event at `:788-793`. The
   `AttentionChanged` arm at `:845` sets the flag.

The banner rendered into an SSE event and the banner rendered with the page must come from **one**
function with identical arguments, which is what the `message_bubble_chat` comment at `:2654-2658`
established as this codebase's rule: "a bubble that streams in is indistinguishable from one that
came with the page". A render test asserts the two strings are equal for one fixture.

## 5.4 Documentation

`README.md` §3.7, one short paragraph after Phase 2's: a held reply shows a durable **Needs
instruction** badge on the thread row and a banner in the thread naming who is responsible; opening
the thread, reading it, or reloading the page never clears it; only claiming, sending or dismissing
does. That last sentence is the product promise from the general file's "Per-principal unread state"
out-of-scope note, and it belongs where a support engineer will read it.

---

## Tests

**Render tests** (`src/adapters/http/pages/tests.rs`), no database:

1. `thread_handoff_mark`: `None` is the empty string; `needs_instruction` contains the words and
   `badge-warning`; `draft_ready` contains `badge-info`; `drafting` contains `badge-ghost` and **not**
   `animate-pulse`. A handoff-shaped subject is escaped.
2. `thread_handoff_slot` carries `sse-swap="handoff-{thread_id}"`, `hx-target="this"` and
   `hx-swap="innerHTML"`, and the slot for one thread does not mention another thread's id.
3. `thread_row_fragment` with `ThreadRowMarks { handoff: Some(needs_instruction), activity:
   Some(Working), .. }` renders **both** marks, with the handoff slot before the activity slot, and
   the row still renders when either is `None`.
4. `thread_column` / `mailbox_page` render the badge for a thread present in the `handoffs` map and
   nothing for one absent from it.
5. `thread_handoff_banner`: an unclaimed `needs_instruction` handoff renders `Channel team`, the age
   from `generation_opened_at`, `Normal`, `No due time`, a **Claim** button, a **Dismiss** button
   and **no Generate draft**. Claimed by the viewer: **Generate draft** and **Dismiss** appear.
   `drafting`: no buttons and the quiet state line. `draft_ready` claimed by the viewer: **Send**,
   **Edit and send**, **Dismiss**.
6. Claimed by somebody else, viewer is not a manager: no buttons at all, and the responsible label
   is the other principal's. As a manager: **Reassign** appears and **Send** does not.
7. Every button carries the handoff's `expected_version` **and** `expected_generation` as hidden
   fields — assert the literal values, because this is the assertion that fails if someone
   "simplifies" the form later.
8. `message_pane` contains `id="thread-handoff" sse-swap="handoff"`, the container sits **after**
   `#message-scroll`'s closing `</div>` and **before** `#thread-activity`, and the page still
   renders with `handoff: None` (the container present and empty, so the first streamed banner has
   somewhere to land — mirroring `#no-messages` at `:2200-2203`).
9. The banner string rendered inline equals the banner string the stream would send for the same
   fixture.
10. The queue and the banner agree: for one handoff fixture, `age_label` and `priority_class`
    produce the same text and class in `attention_card` and in the banner — a test that fails if
    somebody reintroduces a second formatter.

**Event tests** (`src/infra/events.rs`):

11. `is_thread_handoff_in_channel` is true for an `AttentionChanged` with a matching `channel_id` and
    `AttentionWakeSource::ThreadHandoff`; false for another channel, for
    `AttentionWakeSource::Handoff` (the *other* feature — this is the collision test the general
    file's naming table asks for), and for a `MessageCommitted` in the same channel.
12. The existing `the_two_event_kinds_are_matched_separately` test (`:436-460`) gains the handoff
    case, so a future refactor cannot make one predicate answer for another.

**Stream tests** (beside the existing stream tests for these routes):

13. A handoff change in the channel yields one `handoff-{thread_id}` event per first-page thread,
    with a non-empty payload for the held thread and an empty one for the others.
14. A handoff resolved by a send yields an **empty** payload for that thread — the badge clears
    without a page reload. This is the general file's manual step 7 as an automated test.
15. A row that streams in for a held thread arrives with its badge already rendered.
16. `Wake::Lagged` re-seeds both the activity and the handoff sets and re-sends both, with no stale
    badge left behind.
17. The open-thread stream emits the banner on connect, and again after a claim, with the
    responsibility changed.
18. A handoff change on a **different** thread in the same channel re-renders the open thread's
    banner unchanged — asserting the accepted cost from §5.3 rather than pretending it does not
    exist.
19. The simulation stream (`channel.rs:1105-1173`) and the schedule-run stream
    (`ui_schedules.rs:442-476`) are **not** woken by a handoff change. This is the regression guard
    for "add new predicates, do not widen the existing ones", and it is the test a future
    simplification will break.

**Manual end-to-end.** The general file's nine-step manual check, run in full against `:3001`. It is
the only place the whole feature is exercised as a user experiences it, and Phase 5 is the phase
where it must pass end to end.

## Done when

- [ ] No SQL was touched: `git diff --stat migrations/ .sqlx/` is empty for this phase.
- [ ] `ThreadActivity` gained no variant: `grep` for `NeedsInstruction` finds only
      `ThreadHandoffState`.
- [ ] The row badge is its own slot with its own `handoff-{thread_id}` event, rendered before the
      activity slot, and clears to empty when the handoff closes.
- [ ] The banner renders responsibility, age, priority and due time using the attention page's own
      `age_label` and `priority_class`, and carries `expected_version` and `expected_generation` on
      every button.
- [ ] Phase 4's action row now lives inside the banner and exists in exactly one function.
- [ ] The two mailbox streams use the new `mailbox_*_wake_ups`; `thread_wake_ups` and
      `channel_wake_ups` are unchanged and their other two callers are proven unaffected.
- [ ] The badge and the banner survive a reload, opening and closing the thread, and a reconnect;
      only claim, send and dismiss change them.
- [ ] Cases 1-19 pass, and the manual end-to-end check in the general file passes by hand.
- [ ] `cargo fmt --check`, `SQLX_OFFLINE=true cargo build --all-targets`, `cargo test` and
      `cargo clippy --all-targets -- -D warnings` are green.
