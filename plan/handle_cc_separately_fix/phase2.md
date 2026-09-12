# Phase 2 — One task per address

Read [`general_plan_instructions.md`](general_plan_instructions.md) first. This phase assumes
[`phase1.md`](phase1.md) has landed: the commit already accepts several task requests per message.

**Goal.** Two changes:
- **The behaviour change** (2.1). It is contained in the commit plan. Everything downstream already
  runs a task from its own targets (see "Facts" in the general file).
- **Cross-filing** (2.2). Each published reply is also filed, as context, into the other threads
  the source message landed in. Every addressed channel keeps seeing every answer, as it would under
  reply-all.

**Files touched**

| File | Change |
|---|---|
| `src/application/use_cases/thread/ingest/commit.rs` | group answering channels by address, one task request per group |
| `src/adapters/persistence/thread/` (beside `associate_message_on`) | new `file_in_sibling_threads_on` |
| `src/adapters/persistence/task/operations.rs` | `commit_agent_dispatch` calls it after the reply's own associations |
| `src/adapters/persistence/response_review/commands.rs` | `approve_on` calls it after the reply's own associations |
| `README.md` §3.7 | describe the per-address rule, the separate replies, and the cross-filing |
| `src/application/transport/ingress.rs` | `InboundTaskRequest` doc comment |
| tests (see below) | new cases; deliberate updates to tests that encoded the combined reply |

---

## 2.1 Group answering channels by address (`ingest/commit.rs:101-111`, `:133`)

Build the targets with today's filter (`channel.answers && channel.outreach.is_none()`), then split
them into consecutive runs sharing `channel.candidate.handle`. Each run becomes one
`InboundTaskRequest`, in the order `prepared.channels` already has (To before Cc, address order). Put
it in a pure helper, `pipelines(&[PreparedChannel]) -> Vec<Vec<InboundTaskTarget>>`, unit tested
without I/O.

Group by `handle`, not by `step.index == 0`. A pipeline's first step can be missing: routing may have
dropped it as a repeat or refused it, and the rest of that address must not be joined onto the
previous pipeline.

The message-wide gate stays as it is: no task at all when `disposition.answers()` is false. That
includes a `.quiet` address anywhere on the message, which by decision stays message-wide.

## 2.2 File each reply into the other addressed threads, as context

**Why.** Once the tasks are split, support's thread would never show billing's answer. A follow-up
that support handles could then contradict billing. The tools this design follows keep related
conversations visible to every team addressed: Front merges them into one shared conversation, and
Zendesk links its split tickets (see "Common practice" in the general file). This is the in-platform
equivalent of reply-all, without sending any mail between channels.

**What.** When a task's reply is published, also associate that reply with every thread the task's
source message was filed into, except the reply's own threads, with `entry_kind = 'delegation'`. That
covers:
- the sibling tasks' threads, and
- the threads of channels that only had the message filed, e.g. a Cc'd channel the body didn't name.

**How.** One statement, derived entirely from existing rows (no new column):

```sql
INSERT INTO thread_messages (
        id, company_id, channel_id, thread_id, message_id, created_at, entry_kind
)
SELECT gen_random_uuid(), filed.company_id, filed.channel_id, filed.thread_id, $3,
       CURRENT_TIMESTAMP, 'delegation'
  FROM background_tasks AS task
  JOIN thread_messages AS filed
    ON filed.company_id = task.company_id
   AND filed.message_id = task.source_message_uuid
 WHERE task.company_id = $1 AND task.id = $2
   AND NOT filed.thread_id = ANY($4::uuid[])
ON CONFLICT (channel_id, message_id) DO NOTHING
```

- `$2` is the task that produced the reply, `$3` the reply's canonical id, and `$4` the reply's own
  threads (`reply.message.thread_id` plus `also_in_threads`).
- It is bounded by the source message's thread count, at most `MAX_THREAD_ASSOCIATIONS`.
- A scheduled task has no source message, and a selected-note task's source lives only in its own
  thread, so for both the statement inserts nothing.

Put it in `src/adapters/persistence/thread/` as `file_in_sibling_threads_on(tx, company_id, task_id,
reply_id, own_threads)`, beside `associate_message_on`. Call it inside the existing transaction of
both publish paths:
- `commit_agent_dispatch`, right after the loop over `also_in_threads` (`task/operations.rs:1581`).
- `approve_on`, right after its loop (`response_review/commands.rs:242`). A reviewed reply is
  cross-filed when it is approved, never while it is only a draft.

The third place a reply is associated today, `commit_dispatch`'s task-less branch
(`dispatch.rs:1650-1697`), is deleted in Phase 1 (§1.4).

**What it must not do.**
- **Count as an answer.** The already-replied guard only counts `entry_kind = 'conversation'`
  (`find_outbound_reply_after`, `persistence/thread/mod.rs:630`). So a sibling's reply filed as
  `delegation` cannot make a task skip. Test it (case 13).
- **Start work.** An association never enqueues a task: only ingest does. Its only side effect is the
  `thread_messages_notify` trigger, which refreshes the UI for that thread.
- **Coordinate first answers.** A sibling that runs before this reply exists won't see it in that
  run. That's accepted: cross-filing is for follow-ups. Answers that must build on each other are
  what the `+` pipeline is for.

**What the agent sees.** No prompt change is needed.
- `list_agent_history` reads every entry kind except private notes (`thread/views.rs:415-445`).
- The prompt labels each entry with its role, audience, entry kind and author
  (`agent_runner/prompt.rs:124-150`).
- So support's agent sees billing's answer as an `Agent`, `Delegation` entry authored by billing's
  agent, distinct from its own `Conversation` replies.

## 2.3 Documentation

README §3.7:
- "Multi-Workflow Ingestion": each addressed channel or `+` pipeline runs as its own task, whether it
  was in `To` or in `Cc`.
- Replace "Response Concatenation & Outbound Filtering". Outputs are concatenated **within one `+`
  pipeline** and sent from that pipeline's first channel. Separately addressed channels reply
  separately, each from its own address, under its own Cc policy. Keep the sentence about stripping
  platform addresses.
- "Cumulative Upstream Context Sharing" is now accurate as written. Add that it never crosses
  addresses.
- New bullet: every reply is also filed, as a context entry, into every other thread the message
  reached, so each addressed channel's history shows every answer. No mail is sent between channels.
- Add a short version of the worked example from the general file.

Update the doc comment on `InboundTaskRequest` (`ingress.rs:630-639`). It becomes one addressed
pipeline, not "the channels the run drives".

---

## Tests

Routing and grouping (application layer, in-memory, `use_cases/thread/tests.rs`, next to the existing
Cc tests around line 3850):

1. `To: support+sales@` → one task, targets `[support, sales]`.
2. `To: support+sales@`, `Cc: billing@`, body `@billing …` → two tasks: `[support, sales]` and
   `[billing]`, in that order.
3. Same without the mention → one task; billing's thread holds the message.
4. `To: target1@, target3+target4@` → two tasks: `[target1]` and `[target3, target4]`.
5. `To: support+sales@`, `Cc: sales@` with the mention → one task. Sales stays in the first pipeline.
6. Unit, `pipelines`: a pipeline whose first step was dropped stays its own group.
7. `To: support+sales@`, `Cc: billing.quiet@` → no task at all. `.quiet` stays message-wide.

Dispatch and worker (the harness used by `external_reply_tests.rs` / `inter_channel_tests.rs`),
using the worked example (`support` copies outsiders, `billing` does not; Cc includes
`bob@client.com`):

8. Run both tasks:
   - Billing's prompt has no `--- Step` block. Its reply goes out from `billing@`, into billing's
     thread only, unlabelled, with no Cc.
   - Support's reply goes out from `support@`, labelled per step, into the support and sales threads,
     with `bob@client.com` on Cc.
   - Two deliveries in total.
9. Case 4's second task replies from `target3@` with target3's and target4's labelled sections.
   Target1's reply is unlabelled, from `target1@`.
10. Run support's task first: billing's already-replied guard does not fire, and billing still
    answers.
11. Make billing's agent fail: support's reply is still delivered, and only billing's task retries.

Persistence (`inbound_tests.rs`):

12. A commit built from case 2's plan: each thread's `thread_activity` row names its own task, and
    `match_task_id` in billing's thread view resolves billing's task.

Cross-filing (2.2). Each of these executes the new statement against the database; it is a runtime
query, so nothing else checks it:

13. Case 2, support's task published first:
    - Billing's thread already holds support's reply as a `delegation` entry.
    - Billing's task still runs, because the guard ignores `delegation`.
    - After both publish, support's and sales' threads hold billing's reply as `delegation`.
    - Each thread's own reply stays a `conversation` entry.
14. Case 3 (billing only filed): billing's thread holds support's reply as `delegation`, and billing
    has no task.
15. With the response review policy on, support's reply is cross-filed when it is approved, and not
    while it is still a draft.
16. A single-address message: no `delegation` rows are written.
17. Publish the same reply twice (a superseded run re-committing, or a repeated approval). There is
    still one `delegation` row per sibling thread (`ON CONFLICT DO NOTHING`).
18. Unit test on the prompt renderer, no database: a history holding another agent's `delegation`
    entry renders with the `Delegation` label and that agent's name.

Existing tests that encoded the combined behaviour will fail. Update them deliberately, not in bulk.
Find them by running the suite: candidates are any multi-address ingest asserting a single task, a
combined labelled reply across addresses, or `channel_matches.len()` across addresses.
`test_pipeline_address_chaining_execution` is one `+` address and must pass unchanged. If it doesn't,
the grouping is wrong.

## Done when

- [ ] `CommitPlan::build` emits one task request per address, through a pure, unit-tested helper.
- [ ] Both publish paths cross-file the reply as `delegation`, in their existing transaction.
- [ ] Cases 1-18 pass. Tests that changed are listed in the PR with the reason for each.
- [ ] README §3.7 and the `InboundTaskRequest` doc describe the new rule and the cross-filing,
      including the worked example.
- [ ] The end-to-end check in the general file passes by hand.
- [ ] `cargo test` + `cargo clippy --all-targets -- -D warnings` are green.
