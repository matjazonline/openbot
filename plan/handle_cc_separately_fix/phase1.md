# Phase 1 — Let one message own several tasks (no behaviour change)

Read [`general_plan_instructions.md`](general_plan_instructions.md) first. The line references and
facts there are what this phase edits against.

**Goal.** Every type, query and constraint can carry N tasks per message, while ingest still asks for
at most one. The suite stays green with unchanged assertions, apart from the renamed fields.

**Files touched**

| File | Change |
|---|---|
| `migrations/20260817000000_init_schema.sql` | unique key gains `channel_id` |
| `src/adapters/persistence/task/queue.rs` | `ON CONFLICT` target |
| `src/application/transport/ingress.rs` | `task` → `tasks`, `task_id` → `task_ids` |
| `src/application/use_cases/thread/ingest/commit.rs` | wrap the one request in the list |
| `src/adapters/persistence/thread/inbound.rs` | `create_tasks`, the channel check, `tasks_for_message` |
| `src/application/use_cases/thread/mod.rs`, `ingest/mod.rs`, `reload.rs` | `InboundIngestResult.task_ids` |
| `src/application/use_cases/thread/dispatch.rs`, `src/application/services/task_worker.rs` | running task from the lease; delete `ReplyDeliveryMode::Direct` |
| `src/application/use_cases/thread/test_support.rs` | in-memory double loops over `tasks` |
| `src/adapters/persistence/task/collaboration.rs`, `task/controls.rs` | child joins name the channel |

---

## 1.1 Schema key

Replace the constraint at init migration line 3574:

```sql
ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_company_source_message_channel_key
    UNIQUE (company_id, source_message_uuid, channel_id);
```

Add a `COMMENT ON CONSTRAINT` saying one source message causes at most one task **per channel**, the
channel being the first of one addressed pipeline. The leading `(company_id, source_message_uuid)`
still serves every lookup the old key served, so this swaps one index for another rather than adding
one. `NULL` sources (schedule tasks) are still not constrained.

`insert_task` (`task/queue.rs:373-374`) becomes
`ON CONFLICT (company_id, source_message_uuid, channel_id) DO UPDATE SET source_message_uuid =
EXCLUDED.source_message_uuid`. The no-op `DO UPDATE` is kept so `RETURNING` still yields the existing
row.

Recreate both databases (see the general file).

## 1.2 Commit request and outcome (`src/application/transport/ingress.rs`)

- `InboundCommitRequest.task: Option<InboundTaskRequest>` →
  `tasks: BoundedVec<InboundTaskRequest, MAX_THREAD_ASSOCIATIONS>`. Doc: one per addressed pipeline, in
  address order. A channel appears in at most one.
- `InboundCommitOutcome.task_id: Option<Uuid>` → `task_ids: Vec<Uuid>`, in the same order.
- `CommitPlan::build` (`ingest/commit.rs:133`) wraps its one request, if any, in the list. Behaviour is
  unchanged.

## 1.3 Commit persistence (`src/adapters/persistence/thread/inbound.rs`)

- `create_task` → `create_tasks`: validate, then call `insert_task` once per request, keeping the
  current per-request body (payload built from that request's first target, `TaskSource::Message`).
  One statement per task is fine here: the list is bounded at 20 and each insert returns its own row.
- Add a pure check before any insert, `each_channel_in_one_task(&[InboundTaskRequest]) -> AppResult<()>`.
  It refuses a channel named by two requests. The new key only covers each task's **first** channel,
  so a repeat in a later position would otherwise be written silently.
- `task_for_message` → `tasks_for_message`, returning every task with that source. Order them by the
  position of each task's `thread_id` in `threads_holding` (association order, already loaded in
  `recognise_redelivery`), so a redelivery returns ids in the order the first delivery created them.
  Don't order by `created_at`: every task in one commit shares the transaction's timestamp.

## 1.4 Ingest result and dispatch

- `InboundIngestResult.task_id` (`use_cases/thread/mod.rs:1956`) → `task_ids: Vec<Uuid>`, meaning
  what this ingest enqueued. `assemble_result` (`ingest/mod.rs:463`) copies `outcome.task_ids`.
  `reload.rs:101` sets `task_ids: Vec::new()`, because reloading a task enqueues nothing.
- Dispatch takes the task it is running from `lease.task_id`, not from the ingest result. Change
  every `ingest.task_id` read in `dispatch.rs` (lines 383, 1002, 1021, 1074, 1078, 1178, 1293, 1438,
  1490, 1653). Where a helper has no lease, pass it the `Uuid`, not the whole lease. Delete the
  worker's `ingest_exec.task_id = Some(task.id)` (`task_worker.rs:1034`).
- `ReplyDeliveryMode::resolve` then always has a task. Delete the `Direct` variant and its
  `message:{id}` source key. Tests that ran dispatch with no task (e.g. `tests.rs:4958`,
  `task_id: ingest.task_id.unwrap_or_else(Uuid::new_v4)`) move onto an enqueued task, which is what
  production does.
- Delete `commit_dispatch`'s task-less branch too (`dispatch.rs:1650-1697`, "A task-less caller
  (direct ingest)…"), for the same reason. With it gone, a reply is published in exactly two places:
  `commit_agent_dispatch` and review approval. Phase 2 §2.2 relies on that.

## 1.5 In-memory double (`src/application/use_cases/thread/test_support.rs:1610-1660`)

The fake `commit_inbound` loops over `request.tasks` the same way and returns `task_ids`. It applies
the same "channel in one task" check, so the in-memory suite cannot drift from the database one.

## 1.6 Joins that treat the source message as a task key

Add `AND child.channel_id = target.internal_channel_id` to the child join in:
- `load_collaboration_tasks` (`task/collaboration.rs:552-557`)
- `load_collaboration_targets` (`task/collaboration.rs:636-641`)
- `revoke_internal_child` (`task/controls.rs:245-248`)

Today these can't match two children: an internally relayed outreach request has exactly one channel
recipient, so it causes one task. The predicate states that invariant in the join, so it doesn't rest
on the old key. `fetch_thread_tasks` already filters by thread and needs nothing.

---

## Tests

- **`inbound_tests.rs`:** a request carrying two task requests creates two tasks. Each has its own
  `task_channel_targets` rows, and `task_ids` comes back in request order.
- **`inbound_tests.rs`:** redelivering that message returns the same two ids in the same order and
  creates no task (count by `company_id`).
- **`inbound_tests.rs`:** a request naming one channel in two task requests is refused, and nothing is
  written (message, thread, task counts all zero).
- **`inbound_tests.rs`:** extend `two_concurrent_deliveries_of_one_message_produce_one_of_everything`
  to a two-pipeline request. There must be exactly two tasks, never four.
- **`task/tests.rs`:** `enqueue_task` twice with the same `TaskSource::Message` on the same channel
  returns one task. On a different channel it inserts a second.
- **`task/tests.rs`:** the three collaboration queries still find the child for an internal target.
  Existing outreach tests cover this; confirm they run the changed statements.
- **Unit:** `each_channel_in_one_task`, accepting and refusing, with no database.
- The whole existing suite passes with only field renames. No behaviour assertion changes in this
  phase.

## Done when

- [x] Constraint replaced in the init migration, both databases recreated, `.sqlx` regenerated.
- [x] `tasks` / `task_ids` everywhere, no `task_id` left on the commit outcome or the ingest result.
- [x] Dispatch reads the running task from the lease, and `ReplyDeliveryMode::Direct` is gone.
- [x] The three child joins name the channel.
- [x] New tests above pass, and `cargo test` + `cargo clippy --all-targets -- -D warnings` are green.

Validation: 2026-09-12 — offline all-target compilation, SQLx preparation, formatting,
`cargo test --locked --offline --all-targets` (1,489 library tests and 5 main tests passed;
22 existing opt-in tests ignored), and Clippy with warnings denied all passed.
The final run used an isolated PostgreSQL cluster on the external workspace volume after
the internal disk filled; both fresh databases accepted all migrations.
