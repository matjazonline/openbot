# Handle separately addressed channels separately — shared instructions

Read this before either phase file.

Today, one inbound message addressed to several platform addresses becomes **one** agent task. That
task runs every answering channel in a single chain and sends **one** combined reply from the first
channel's address. After this change, each **address** gets its own task:

- A `+` address (`target3+target4@`) is still one pipeline and one task, chained exactly as today.
- A separate address (`target1@`, a second `To:` entry, or a Cc'd channel) gets its own run and its
  own reply, sent from its own address.

| Phase | File | Goal |
|---|---|---|
| 1 | [`phase1.md`](phase1.md) | Let one message own several tasks: the schema key, commit/ingest shapes, dispatch reads the lease. No behaviour change |
| 2 | [`phase2.md`](phase2.md) | One task per address: group channels by address when the commit plan is built; file each reply into the other addressed threads as context; update the README |

Each phase compiles, tests green, and is landable on its own. Phase 1 only widens shapes. Phase 2 is
the only phase that changes behaviour, and that change is contained in `CommitPlan::build`.

---

## Behaviour, before and after

| Email | Today | After |
|---|---|---|
| `To: support+sales@` | 1 task: support → sales, one reply from `support@` | unchanged |
| `To: target1@, target3+target4@` | 1 task: target1 → target3 → target4, one reply from `target1@` | 2 tasks. `[target1]`, reply from `target1@`. `[target3 → target4]`, reply from `target3@` |
| `To: support+sales@`, `Cc: billing@`, body does not name billing | 1 task: support → sales; billing only gets the email filed in its history | unchanged |
| `To: support+sales@`, `Cc: billing@`, body says `@billing` | 1 task: support → sales → billing. Billing's prompt carries support's and sales' answers as "Step 1/Step 2". One reply from `support@` with a `[Billing … - CC]` section | 2 tasks. `[support → sales]`, reply from `support@`. `[billing]`, no upstream context, reply from `billing@` |
| `To: support+sales@`, `Cc: sales@` (named) | sales runs once, in the first pipeline | unchanged: a channel named twice keeps its first address |

## What the customer receives — worked example

Setup: channels `support` (copies outsiders, `add_3rd_party` on), `sales`, and `billing` (does not
copy outsiders, `add_3rd_party` off). Ana writes:

```text
From:    ana@client.com
To:      support+sales@acme.mailagents.com
Cc:      billing@acme.mailagents.com, bob@client.com
Subject: Upgrade and invoice

We'd like to move to the Team plan. @billing, can you resend March's invoice?
```

**Today: one email.**

```text
From:    Support <support@acme.mailagents.com>
To:      ana@client.com
Cc:      bob@client.com
Subject: Re: Upgrade and invoice

[Support (support@acme.mailagents.com) - TO]
…support's answer
--------------------------------------------------
[Sales (sales@acme.mailagents.com) - TO]
…sales' answer, written after reading support's
--------------------------------------------------
[Billing (billing@acme.mailagents.com) - CC]
…billing's answer, written after reading support's and sales'
```

Bob receives billing's answer even though billing is set not to copy outsiders: the whole reply
follows support's setting, because support is the first channel.

**After: two emails.** Both reply to Ana's message (`In-Reply-To`), so her mail client shows them in
one conversation.

```text
From:    Support <support@acme.mailagents.com>
To:      ana@client.com
Cc:      bob@client.com                  ← support copies outsiders
Subject: Re: Upgrade and invoice

[Support (support@acme.mailagents.com) - TO]
…support's answer
--------------------------------------------------
[Sales (sales@acme.mailagents.com) - TO]
…sales' answer, written after reading support's
```

```text
From:    Billing <billing@acme.mailagents.com>
To:      ana@client.com
Cc:      (nobody)                        ← billing does not copy outsiders
Subject: Re: Upgrade and invoice

…billing's answer, unlabelled, written from Ana's email alone
```

What changes for the people involved:

- **Ana** gets two emails instead of one. Replying to billing's email reaches billing's thread. Today
  every reply of hers goes to support.
- **Bob** gets what each channel's own setting allows: here one email, support's. If billing also
  copied outsiders, he would get both.
- **Each channel's own team list** (`participant_emails`) is copied on that channel's reply only.
  Today only the first channel's list is copied, whoever answered.
- **Timing is independent.** Either email can arrive first. One can wait for approval or review while
  the other goes out. If billing's agent fails, support's answer still goes out; today nothing is
  sent.
- **Every addressed channel still sees every answer** (Phase 2 §2.2). Billing's reply is also filed
  into the support and sales threads as a context entry, and support's reply into billing's. A
  follow-up to either team is handled knowing what the other said. No mail passes between the
  channels, and filing a reply as context never counts as that channel's own answer.

The same logic for `To: target1@, target3+target4@`: one email from `target1@` with target1's answer
unlabelled, and one from `target3@` with target3's and target4's labelled sections.

## Why

- **The documented grammar already distinguishes the two cases.** README §3.7 promises chaining only
  "in a `+` pipeline". Ingest computes the pipeline step per address: in the example, billing is step 1
  of 1 (`ingest/routing.rs:149-151`). Only the one-task-per-message model flattens every answering
  channel into a single chain. That flattening is why the worker rebuilds billing as step 3 of 3
  (`reload.rs`, from the flat target list).
- **It was never scoped.** The commit that introduced To/Cc execution (`7e35ffa`) already fed every
  earlier output into every later agent, whichever address it came from. Nothing records this as a
  decision.
- **Channels not chained with `+` are coupled anyway:**
  - Prompts carry other teams' draft answers (`upstream_context_for`, `dispatch.rs:1329`).
  - Replies leave from the first channel's address, under that channel's Cc policy (`outbound_cc_for`,
    `dispatch.rs:1514`).
  - If any agent fails, nothing is sent (`dispatch.rs:825`).
  - A structured response is refused whenever a second address is answered (`dispatch.rs:831`).
  - Deleting a later channel deletes the whole combined task (`delete_channel_target_tasks`, init
    migration lines 120-131).
  - Later channels' threads show no task activity, because the activity query keys on
    `task.thread_id` (`task/operations.rs:408`).

## Decisions

- **Per address, not per role.** Decided 2026-09-11: in `To: target1@domain.com,
  target3+target4@domain.com` those are two addresses, and target1 is handled separately from
  target3+target4. A second `To:` address splits exactly like a Cc'd one.
- **`.quiet` stays message-wide.** Decided 2026-09-11: one `.quiet` address still turns the whole
  message file-only, as `policy::fold_disposition` does today, so no address runs.
- **One email per answering address.** Accepted 2026-09-11, after comparing with established tools
  (see "Common practice" below). A company-level "one reply" mode is deferred until a customer asks
  for it.
- **Replies are cross-filed as context.** Decided 2026-09-11: each reply is also filed into the other
  threads the message reached, as a `delegation` entry (Phase 2 §2.2). This keeps the shared
  visibility the established tools keep.
- **Unchanged:**
  - The Cc activation rule: a Cc'd channel runs only when the body names it (`cc_was_mentioned`,
    `routing.rs:668`).
  - Channel dedup: a channel named twice keeps its first address, To before Cc (`routing.rs:188`).
  - One canonical message is filed into every addressed channel's thread.
  - `+` chaining, and the per-step labelled reply inside one pipeline.
  - A channel whose match closes an outreach still resumes the waiting task instead of getting a new
    one.
- **No new column.** The address boundary is derived from `ChannelCandidate::handle`, already loaded
  and documented as "Pipeline steps of one address share it" (`routing.rs:63`). The existing unique key
  **gains** a column rather than a new index being added, so the index count stays the same.
- **Bounded:** tasks per message ≤ answering channels ≤ `MAX_THREAD_ASSOCIATIONS` (20,
  `transport/ingress.rs:54`). The commit request carries the task requests in a `BoundedVec` with the
  same bound.

## Facts the design rests on (verified 2026-09-11)

- **One task per message is enforced in the schema.** `background_tasks_company_source_message_key
  UNIQUE (company_id, source_message_uuid)` (init migration line 3574). `insert_task` names it in
  `ON CONFLICT` (`task/queue.rs:373-374`), which is how a redelivered message gets back the task it
  already has. No foreign key references that key.
- **Only two production producers attach a message source:** the inbound commit (`thread/inbound.rs:663`)
  and the selected-note task (`task/instructions.rs:411`, one fresh note message → one task). Both stay
  unique under `(company_id, source_message_uuid, channel_id)`.
- **Each pipeline's first channel is distinct**, because routing drops a repeated channel. So
  `channel_id`, the task's own channel, is enough to tell one message's tasks apart.
- **The worker already runs a task from its own targets.** `load_inbound_task` rebuilds the matches from
  `task_channel_targets` (`reload.rs:66`). A single-pipeline task therefore runs exactly that pipeline:
  - The reply is from the task's first channel, using the slug the email named for it (`reply_slug`,
    `thread/mod.rs:1709`; `dispatch.rs:1462`).
  - The reply is written only into that task's threads (`agent_reply_write`, `also_in_threads =
    matches[1..]`, `dispatch.rs:1621`).
  - The Cc line follows that channel's own `add_3rd_party` and `participant_emails`.
- **Who gets a reply:** `To` is the sender (`sender_address`, `dispatch.rs:212`). `Cc` is the inbound
  Cc line, minus outsiders when the channel has `add_3rd_party` off, plus the channel's
  `participant_emails` (`outbound_cc_for`, `dispatch.rs:1514`). An "outsider" is any address outside
  the platform domain other than the sender (`is_third_party_address`, `thread/support.rs:192`).
- **Replies can't loop back through Cc.** Egress strips every platform address, and the To and From
  addresses, from the wire Cc (`copied_addresses`, `protocols/email/egress.rs:404`). In-process relay
  applies only when the `To` is a single internal channel (`internal_target`, `egress.rs:423`), and an
  agent reply's `To` is the customer.
- **The already-replied guard works per thread** (`task_worker.rs:986`, `find_outbound_reply_after`,
  `persistence/thread/mod.rs:630`). Threads are per channel and a channel is in only one pipeline, so
  one task's reply cannot make another task skip. The guard also only counts
  `entry_kind = 'conversation'`, which is what lets Phase 2 file sibling replies as `delegation`.
- **A reply is published into threads in two transactional places:** `commit_agent_dispatch`
  (`task/operations.rs:1581`) and review approval `approve_on` (`response_review/commands.rs:242`).
  A third, task-less branch in `commit_dispatch` (`dispatch.rs:1650-1697`) is unreachable in
  production and is deleted in Phase 1.
- **The agent already sees every entry kind.** `list_agent_history` (`thread/views.rs:415`) excludes
  only private notes, and the prompt labels each entry by role, audience, kind and author
  (`agent_runner/prompt.rs:124-150`). Inserting into `thread_messages` fires only
  `thread_messages_notify`, a UI refresh.
- **Dispatch always runs under a lease.** The only production caller of
  `execute_claimed_agent_task_and_dispatch` is `run_task`. `reload.rs:101` sets `task_id: None` and the
  worker immediately overwrites it with the claimed task (`task_worker.rs:1034`). So the running task
  is always `lease.task_id`. `ReplyDeliveryMode::Direct` (no task) is reachable only from tests.
- **Readers that key on the source message:**
  - `task_for_message` (redelivery, `thread/inbound.rs:199`) assumes one task.
  - `fetch_thread_tasks` also filters on `thread_id` (`thread/views.rs:190`), so it already finds each
    thread's own task.
  - The outreach child-task joins (`task/collaboration.rs:552, 636`, `task/controls.rs:245`) find the
    child by `source_message_uuid` alone.
- **The chain board is fine with several root tasks per chain:** it groups by `correlation_id` and only
  counts (`task/board.rs:130-170`). All of a message's tasks share its correlation id.

## Rules every phase inherits

- Schema changes edit `migrations/20260817000000_init_schema.sql` in place, then recreate **both**
  databases:

  ```sh
  dropdb --if-exists mail_agents && createdb mail_agents
  dropdb --if-exists mail_agents_test && createdb mail_agents_test
  DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo sqlx migrate run
  ```

- After any SQL change: `DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo sqlx
  prepare -- --all-targets`. The queries touched here are runtime `sqlx::query`, so a typo is a runtime
  error. Every changed statement needs a test that executes it.
- DB tests share one database. Scope every assertion to ids the test created. Leave no claimable task
  behind: fixture channels without agents give tasks no owner, so no claim can take them.
- Run `cargo test` with `DATABASE_URL` set. `cargo test` and
  `cargo clippy --all-targets -- -D warnings` must be green at the end of each phase.

## End-to-end check (after Phase 2)

With the server on `:3001` and a company holding `support`, `sales` and `billing` channels, each with
an agent, `support` copying outsiders and `billing` not:

1. Send the worked example above. Use the in-app mailbox compose, or a SendGrid-shaped POST to
   `/webhooks/email/sendgrid`.
2. The task board shows one chain (one correlation id) holding two tasks: one owned by support's
   agent, one by billing's.
3. The deliveries page shows two replies:
   - From `support@…`, containing the Support and Sales sections, with `bob@client.com` on Cc.
   - From `billing@…`, containing only billing's answer, with nobody on Cc.
4. Billing's thread shows its own task in the activity strip. It holds billing's reply as a
   conversation entry and support's reply as a context (`delegation`) entry. Support's thread shows
   the mirror image.
5. Repeat with the same Message-ID. Nothing new is enqueued or sent.

## Common practice (checked 2026-09-11)

How established tools handle one email addressed to several team addresses:

- **Zendesk.**
  - Default: one email is one ticket, however many support addresses it names. Zendesk picks the
    ticket's address through "an internal prioritization process", and results may vary.
  - Opt-in, early access, "multi-recipient email tickets": one ticket per unique support address in
    To **or** CC. The tickets are linked by a shared ID plus an internal comment, the end user gets a
    notification per ticket, and each reply threads into its matching ticket. Stated caveat: it "can
    significantly increase ticket and email volume". At most 10 addresses per email.
- **Front.**
  - Default ("smart merge"): one conversation shared across the inboxes addressed. Replies, comments
    and actions in one inbox show in the others, "so that all context stays in one place".
  - Per-inbox setting, "keep actions separate": each team gets its own standalone copy and handles it
    independently.
- **Email between people.** Each addressee who needs to act replies, usually reply-all. A Cc is
  informational unless the body names you.

Where this design sits:
- Today's behaviour matches none of these. Every team answers, the answers are stapled into one
  email from an address the system picked, and later teams read earlier teams' drafts.
- This plan is the split mode, as in Zendesk's multi-recipient tickets and Front's "keep actions
  separate":
  - one unit per address, To and Cc alike;
  - each customer reply goes back to its own unit;
  - siblings are linked by the shared correlation id;
  - visibility across units is kept by cross-filing (Phase 2 §2.2).
- The vendors default to merging to limit volume. Here the Cc-mention rule already does much of that
  work: Zendesk opens a ticket for every Cc'd support address, but a Cc'd channel here only runs when
  the body names it.

Sources:
[Zendesk — email sent to multiple support addresses](https://support.zendesk.com/hc/en-us/articles/4408881694362-What-happens-when-an-email-is-sent-to-multiple-support-addresses),
[Zendesk — multi-recipient email tickets (EAP)](https://support.zendesk.com/hc/en-us/articles/10584454777370-Turning-on-multi-recipient-email-tickets-EAP),
[Front — shared inboxes merge duplicates by default](https://help.front.com/en/articles/2265),
[Front — keep actions separate between shared inboxes](https://help.front.com/en/articles/2026).
