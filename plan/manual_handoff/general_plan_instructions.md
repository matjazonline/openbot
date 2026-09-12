# Manual handoff for outside replies — shared instructions

Read this before any phase file. The line references and facts here are what every phase edits
against.

Today **every** outside reply to an existing thread runs the channel's agent, unconditionally. The
only gate is `MessageDisposition::{Answer, FileOnly}` (`src/application/transport/ingress.rs:149-154`),
and `Answer` always produces a task. There is no way for a team to say "hold customer replies on
this channel until one of us looks".

After this change a company — and, per channel, an override — chooses between:

- **`Automatic`**: today's behaviour, unchanged, and the migration default.
- **`ManualHandoff`**: an eligible outside reply is **filed** on the thread as the customer message
  it is, no agent task is created, and a durable **Needs instruction** handoff appears in the shared
  attention queue. The team then tells the agent what to do, replies themselves, or dismisses it.
  Opening the thread or reading a notification never clears it; only an explicit action does.

This plan implements [`plan/message-confirmation-ideas/manual-handoff.md`](../message-confirmation-ideas/manual-handoff.md),
which was written but never built. It consumes the audience, internal-note, response-draft,
task-ownership and operational-attention contracts in
[`plan/general-improvements/ideas/README.md`](../general-improvements/ideas/README.md) rather than
defining competing versions of them.

| Phase | File | Goal |
|---|---|---|
| 1 | [`phase1.md`](phase1.md) | `ExternalReplyHandling::{Automatic, ManualHandoff}`: company column, nullable channel override, pure resolver, port, settings page, JSON/form round trip. No behaviour change |
| 2 | [`phase2.md`](phase2.md) | The hold gate: eligibility as a pure decision, `thread_handoffs` + `thread_handoff_events`, the generation model, and the held reply committing atomically with no task |
| 3 | [`phase3.md`](phase3.md) | `needs_instruction` projects into `AttentionItem`; claim / reassign / priority / due as versioned, idempotent, audited commands |
| 4 | [`phase4.md`](phase4.md) | Generate draft, send draft, edit and send, dismiss — reusing `AskOwnerToAct`/`StartAgentTask` and `ResponseDraft`, fenced by `thread_handoff_runs` |
| 5 | [`phase5.md`](phase5.md) | Mailbox badges and banner, and the SSE wake-up that keeps them current |

Each phase compiles, tests green, and is landable on its own. Phase 1 adds configuration nothing
reads yet. Phase 2 is where behaviour changes, and that change is contained in
`prepare_channels`/`CommitPlan::build` plus one new table write inside the existing inbound commit.

## Deviation from the shape suggested by the audit

The audit suggested four phases, with the `AttentionItem` projection folded into the UI phase. This
plan puts the projection and the responsibility commands in Phase 3, **before** the draft actions,
for one reason: after Phase 2 a held reply exists with nowhere to be seen except the thread itself.
Phase 3 makes it visible and claimable through the queue that already exists, which makes Phase 2 +
3 a shippable increment ("held replies are queued for the team") without any of Phase 4's drafting
machinery. Phase 4 then reuses exactly the same projection for `draft_ready`.

---

## Behaviour, before and after

| Inbound | Today | After, channel on `Automatic` | After, channel on `ManualHandoff` |
|---|---|---|---|
| Outside reply to an existing thread, `To: support@` | agent runs, reply sent | unchanged | filed as a customer message; no task; **Needs instruction** handoff |
| Outside sender opens a **new** thread | agent runs | unchanged | unchanged — new conversations are never held |
| Teammate's reply (`CompanyMembership` is not `None`) | agent runs | unchanged | unchanged — the team is not "outside" |
| `support.quiet@`, or `[[quiet]]` in the body | filed, no agent | unchanged | unchanged — an explicit `FileOnly` is already what a hold would ask for |
| Reply that closes an outreach the platform is awaiting | recorded, waiting task resumes | unchanged | unchanged — correlated outreach/quorum replies are never held |
| Passive Cc'd channel the body does not name | filed only | unchanged | unchanged — that channel was never going to answer |
| `To: support@, billing@`, support on `ManualHandoff`, billing on `Automatic` | one task for both | unchanged | billing's task runs; support's copy is held. One message, one handoff |
| Second outside reply while the first is still held | second task | unchanged | the handoff takes a **new generation**; the old one can no longer be acted on |

## What the team sees — worked example

Setup: company `acme`, default `Automatic`. Channel `support` overridden to `ManualHandoff`; channel
`billing` left on inherit. Thread "Invoice 4471" already exists in `support`, with Ana
(`ana@client.com`) as a participant, and she is not a member of `acme`.

1. Ana replies to the thread. Ingest classifies her as outside (`CompanyMembership::None`), sees an
   existing thread, sees that `support` would have answered, sees no `FileOnly` marker and no
   outreach correlation, and resolves `support`'s effective policy to `ManualHandoff`.
2. One transaction writes: the canonical message (audience `external_conversation`, entry kind
   `conversation` — a held message is still the customer's message), its thread association, its
   provider mapping, **and** a `thread_handoffs` row in state `needs_instruction` with a fresh
   generation UUID. **No `background_tasks` row.**
3. The mailbox shows a durable **Needs instruction** badge on the thread row, and a banner in the
   open thread naming the channel team as responsible, the age, `Normal` priority and no due time.
   The shared attention queue shows the same item under **Unassigned**.
4. Bo claims it. Responsibility becomes Bo; the handoff version advances; an immutable event records
   the claim with Bo's principal and the command UUID.
5. Bo writes an internal note — "quote her the 30-day terms, don't offer the discount" — and presses
   **Generate draft**. That goes through the existing `StartAgentTask` use case: one fenced agent
   task, correlated to this handoff generation through a `thread_handoff_runs` row keyed by task id.
   The handoff enters `drafting`, which is visible progress but **not** actionable work, so it leaves
   the attention queue.
6. The agent completes. Because the run is a handoff draft run, it writes a `ResponseDraft` version 1
   rather than an outbound canonical message, and the handoff moves to `draft_ready` — actionable
   again.
7. Bo edits two sentences and presses **Send**. That creates draft version 2 (immutable; version 1 is
   superseded), publishes it under the channel's `ExternalResponseReview` policy, and resolves the
   handoff generation when the logical external delivery is durably enqueued.
8. Ana replies again a day later. A **new** generation opens in `needs_instruction`. If Bo's browser
   still had step 6's page open, pressing **Send** there now fails with a conflict naming the newer
   generation instead of sending a reply written for the previous message.

## Naming: `thread_handoffs` is not `manual_handoffs`

**A table called `manual_handoffs` already exists and is a different feature.** It is a generic,
explicitly human-created work item — "someone please do this" — created through
`POST /api/companies/{id}/handoffs` (`src/adapters/http/routes/attention.rs:242-274`,
`NewManualHandoff` at `src/domain/entities/attention.rs:261-275`, table at init migration lines
2452-2476). It carries a free-text `title` and `next_action`, has no generation, no source message,
no state machine beyond `open`/`resolved`/`withdrawn`, and is not caused by anything inbound.

This plan's concept is inbound-triggered, generation-fenced, one-per-thread, and derives its title
from the thread. It is a **different table**: `thread_handoffs`. Whoever implements this must not
"reuse the existing handoff table" — it does not fit, and the version/responsibility semantics
would collide.

The collision is not only the table name. These are already taken by the *other* feature and must
**not** be reused for this one:

| Already means `manual_handoffs` | This plan uses |
|---|---|
| `AttentionSourceKind::Handoff` / `'handoff'` (`src/domain/entities/attention.rs:65-101`) | `AttentionSourceKind::ThreadHandoff` / `'thread_handoff'` |
| `AttentionWakeSource::Handoff` (`src/infra/events.rs:86-94`) | `AttentionWakeSource::ThreadHandoff` |
| `attention_source_events.source_kind = 'handoff'` (init migration 1871) | its own `thread_handoff_events` table — see below |
| `notify_attention_changed('handoff')` trigger argument (init migration 5811) | `notify_attention_changed('thread_handoff')` |
| `notification_from_attention_source()`'s `'handoff'` branch (init migration 595-607) | nothing: notifications are out of scope |
| `task_ownership_events.handoff_instruction` (init migration 3329) — the note attached to a task **transfer** | untouched |

`response_drafts.source_handoff_generation` (init migration 2881,
`src/domain/entities/response_draft.rs:242`) looks like it belongs to this plan and **does not**.
Despite the name it is already written, on both review paths, with the id of the latest *task
ownership transfer* event — `src/adapters/persistence/task/operations.rs:1499-1526` and
`:1777-1802` — read back through `DRAFT_COLUMNS`
(`src/adapters/persistence/response_review.rs:53-58`), carried across an edit
(`src/adapters/http/routes/ui_response_reviews.rs:339`) and asserted by
`src/adapters/persistence/task/tests.rs:920-923`. So it is the
`task_ownership_events.handoff_instruction` sense of "handoff", not this plan's generation. **Phase 4
does not touch it and adds no foreign key to it**; a draft is correlated to a handoff generation
through `thread_handoff_runs.task_id = response_drafts.task_id`. See Phase 4 §4.0.

## Repo-specific schema convention — the README's release gate does **not** apply

The shared README says "use additive migrations and staged rollouts. Do not rewrite a migration
applied to any persistent environment." **That line does not apply to this repository**, whose
actual convention overrides it: there is exactly one squashed init migration, schema changes edit it
in place, and both databases are recreated from scratch. See `src/AGENTS.md` for the full binding
style rules this plan inherits.

```sh
dropdb --if-exists mail_agents && createdb mail_agents
dropdb --if-exists mail_agents_test && createdb mail_agents_test
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo sqlx migrate run
```

Everything else in the README's release gates **does** apply: existing automatic-send, quiet-message
and outreach-timeout behaviour stay as migration defaults (`Automatic` for every existing and new
company), and no index is added without a representative query plan.

`migrations/20260817000000_init_schema.sql` is `pg_dump`-shaped. Keep it that way or the next dump
diff is unreadable:

- new columns go at the **end** of the table's column list (ordinal order), before the constraints;
- `CONSTRAINT` lines inside `CREATE TABLE` are **alphabetical** — `channels_external_reply_handling_override_check`
  sorts *before* `channels_external_response_review_override_check`;
- tables, functions, indexes, triggers and foreign keys each live in their own alphabetically
  ordered section with the `-- Name: … Type: … --` banner comments;
- a new `CHECK` value on an existing constraint means editing that constraint's line in place, not
  adding a second constraint.

After any SQL change, regenerate the offline query cache and commit it
(`src/AGENTS.md`, "Regenerate sqlx query metadata after touching SQL"):

```sh
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo sqlx prepare -- --all-targets
```

Every query this plan adds is a runtime `sqlx::query`, so a typo is a runtime error, not a build
error. **Every changed statement needs a test that executes it.**

## DB-test convention

DB tests in this repo share **one live database** and run in parallel. Whole-table counts are racy
and will fail under `cargo test -j`. Every assertion in every test section of this plan must be
scoped to ids the test itself created:

- count with `WHERE company_id = $1` for a company the test made, never `SELECT COUNT(*) FROM
  thread_handoffs`;
- assert a handoff by `(company_id, thread_id)`, a generation by its own UUID, an event by
  `(company_id, command_id)`;
- leave no claimable task behind. Fixture channels without agents give tasks no owner, so no worker
  claim can take them — Phase 4's draft-run tests must either use such a channel or complete the
  task they enqueue.

## Cross-cutting bounds and rules every phase inherits

**Tenant scoping.** Every new table is keyed `(company_id, …)` and every foreign key is composite
through the company, matching `manual_handoffs_thread_fk` (init migration 6642-6643):
`FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES threads(company_id, channel_id, id)`.
Every new query filters on `company_id` **and** on a visible-channel list, as
`src/adapters/persistence/attention.rs` does throughout. Every new route gets a cross-company test
and a restricted-channel test.

**Version fencing.** `thread_handoffs.version` is monotonic and every mutating command carries both
`expected_version` and `expected_generation`. A command whose generation is not the row's current one
fails with `AppError::Conflict` naming the current generation — never silently no-ops, and never
mutates a newer generation. The model to copy is `change_source_attributes`
(`src/adapters/persistence/attention.rs:642-823`): lock, check the recorded command, check the
version, write, append the event, commit.

**Idempotency.** Every command carries a client-supplied `command_id`. The first write records a
SHA-256 fingerprint of the command's semantic fields; a replay with the same id and the same
fingerprint returns the recorded outcome, and the same id with different parameters is a
`Conflict`. This is exactly the shape of `existing_event`
(`src/adapters/persistence/attention.rs:498-528`) and `ask_owner_to_act`
(`src/adapters/persistence/task/instructions.rs:263-289`). Authorization context
(`visible_channel_ids`) is excluded from the fingerprint, as `AttentionSourceCommand` already does
with `#[serde(skip)]`.

**Audit.** Every transition appends one immutable `thread_handoff_events` row inside the same
transaction, with typed actor data (`actor_kind` ∈ `system|human|agent`, plus the principal when
there is one) and the from/to state and version. Immutability is enforced by a
`BEFORE UPDATE OR DELETE` trigger modelled on `attention_source_events_are_immutable()` (init
migration 43-56).

**Bounded text.** `thread_handoffs` stores **no** free text. Its attention title is the thread's
subject and its `next_action` is a `CASE` over the state, both derived in the projection query —
there is no `title`/`next_action` column to bound, unlike `manual_handoffs`. The only new bounded
text is `thread_handoff_events.failure_reason`, capped at 2048 bytes by a `CHECK`, matching
`manual_handoffs_next_action_check`. Note text for **Generate draft** reuses
`MAX_INTERNAL_NOTE_BYTES` (65 536, `src/domain/entities/internal_note.rs:13`) and draft bodies reuse
the `response_drafts_body_check` cap (262 144). No new limit is invented where one exists.

**Bounded fan-out.** At most one handoff per thread and at most one thread per channel per message,
so the holds one commit can write are bounded by `MAX_THREAD_ASSOCIATIONS` (20,
`src/application/transport/ingress.rs:54`). The commit request carries them in a `BoundedVec` with
that bound, like `associations` and `outreach_transitions` already do.

**Durable payloads carry identifiers only.** Nothing about a handoff goes into
`InboundTaskPayloadV1`. A draft run finds its handoff through a `thread_handoff_runs` row keyed by
`(company_id, task_id)` and reloads the handoff from persistence. This is the README's "durable task
payloads contain identifiers and delivery facts only" and it is also what preserves
stale-generation fencing: a worker that snapshotted the generation could not notice it had been
replaced.

**SSE is a wake-up only.** Every new `pg_notify` payload is identifiers only, capped well under the
8000-byte limit, and readers re-query. Subscribe before querying; reconnect and lag both
reconcile by re-querying. Phase 5 adds no new PostgreSQL channel — it routes through the existing
`attention_changed` channel (`src/infra/events.rs:43`).

**Do not fold the hold into `MessageDisposition`.** It is tempting to make a held message
`FileOnly` and be done. It is wrong: `src/adapters/persistence/thread/inbound.rs:776` turns
`FileOnly` into audience `InternalOnly` and entry kind `Note`, which would reclassify a paying
customer's email as a private internal note, hide it from the customer-visible history, and exclude
it from the reply the agent eventually writes. A held reply is an ordinary
`external_conversation` / `conversation` message whose **task** was not created. The hold is a
separate axis.

**Handoff responsibility vs. task ownership.** These are related and distinct, and the plan must not
conflate them:

- A **handoff** is responsible-to either the channel team (`responsible_principal_id IS NULL`) or one
  claimed human principal. It exists before any task does.
- A **task** has its own `TaskOwner`/`ownership_version` (`src/domain/entities/task.rs:83-88`;
  `plan/message-confirmation-ideas/implementations/task_ownership_and_transfer_1.md`), and it governs
  who may act on the run.
- So a handoff can be unclaimed while the draft task it eventually starts has an agent owner; and a
  handoff claimed by Bo does not make Bo the task's owner. **Generate draft** goes through the
  existing note/instruction use cases precisely so ownership fencing stays where it already is:
  `AskOwnerToAct` refuses unless the task's owner is an agent and the caller passed the current
  `expected_ownership_version` (`src/adapters/persistence/task/instructions.rs:146-165`).
- Terminal draft-task failure returns only the **matching** `drafting` generation to
  `needs_instruction`. A task transfer or a lost lease never changes who the handoff is
  responsible-to.

## Out of scope

- **Notification email and push.** Deliberately not here. It belongs to
  [`plan/general-improvements/ideas/actionable_notifications_8.md`](../general-improvements/ideas/actionable_notifications_8.md),
  whose own "Dependencies and sequencing" section says to implement it *after* notes and manual
  handoff have stable transitions. Concretely: this plan does **not** touch
  `notification_from_attention_source()` (init migration 595-607), does not add
  `'thread_handoff'` to `notifications_source_kind_check` or
  `notification_events_source_kind_check` (init migration 2727, 2755), and writes no
  `notifications` rows. Because the new events live in `thread_handoff_events` rather than in
  `attention_source_events`, the existing notification trigger never fires for them — the
  out-of-scope boundary is enforced by the schema rather than by discipline.
- **SLA, business-hours, escalation and digest policy.** Priority and due time are stored and
  settable; nothing acts on them automatically. The README asks for those defaults to be chosen from
  measured data, not invented.
- **Retrofitting `ExternalResponseReview`.** Its channel override cannot currently be cleared back
  to "inherit" through `ChannelPersistence::update` because of the `COALESCE` at
  `src/adapters/persistence/channel.rs:532`, and it has no JSON read-back. Phase 1 does not make
  that bug worse and does not fix it either; see Phase 1 §1.5.
- **A second draft concept.** `ResponseDraft` already has immutable versions, status, recipient and
  transport snapshots, evidence and reviewer fields (`src/domain/entities/response_draft.rs:233-256`).
  Phase 4 reuses it. Do not design another.
- **Holding new conversations, outreach replies, or team messages.** Explicitly unchanged; the
  eligibility rule in Phase 2 §2.2 is the whole decision and it is a pure function with a table of
  cases.
- **Per-principal unread state.** A handoff is shared-mailbox state. Counts come from the attention
  projection, never from notification read state, and opening a thread changes nothing.

## End-to-end verification

Run at the end of **every** phase:

```sh
cargo fmt --all
cargo fmt --all -- --check
SQLX_OFFLINE=true cargo build --all-targets          # offline compilation against .sqlx/
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo test
cargo clippy --all-targets -- -D warnings
```

After any phase that touched SQL (1, 2, 3, 4):

```sh
dropdb --if-exists mail_agents && createdb mail_agents
dropdb --if-exists mail_agents_test && createdb mail_agents_test
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo sqlx migrate run
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo sqlx prepare -- --all-targets
git diff --stat .sqlx/      # must be non-empty, and committed
```

Competing-claimant cases that must be green before any mutation surface is considered done — each
one a DB test scoped to its own company:

| Case | Expected |
|---|---|
| The same inbound message delivered twice | one message, one handoff, one generation, no task |
| Two concurrent **Generate draft** commands on one generation | one `thread_handoff_runs` row, one task; the loser gets a `Conflict` |
| An outside reply racing a team instruction on the same thread | either order commits; the handoff ends in exactly one state and its generation matches its `source_message_id` |
| A draft task completing against a generation that has since been replaced | the write is refused; the newer generation stays `needs_instruction` |
| **Edit and send** racing **Send draft** | one publication, one logical delivery, one canonical message |
| **Send** racing **Dismiss** | exactly one wins; the loser gets a `Conflict` naming the current version |
| Two teammates claiming an unclaimed handoff | one claim; the second is a `Conflict` on `expected_version` |
| A command replayed with the same `command_id` | the recorded outcome, no second write |
| The same `command_id` with different parameters | `Conflict` |
| Every new route, called for another company's handoff id | `NotFound`, never a leak of existence |
| Every new route, called by a principal with no `view` grant on the channel | `NotFound` |

**Stock-stack budget.** Phase 4 is the only phase that changes worker-path code (the dispatch
completion branch that writes a draft instead of publishing). `src/AGENTS.md` caps this: the
task-worker → dispatch → agent-runner chain is currently 336 KiB and must not grow materially. Run
`scripts/stack-frames.sh` before and after Phase 4 and record both numbers in the commit message.
Extract synchronous helpers rather than adding `async fn` levels; the branch that decides
draft-vs-publish is a pure decision and belongs in a non-`async fn`.

**Manual end-to-end check** (after Phase 5), server on `:3001`:

1. Set the company default to `Automatic`, override `support` to `ManualHandoff`, leave `billing`
   on inherit. Confirm the settings page shows `support` as "Manual handoff (channel override)" and
   `billing` as "Automatic (inherited)".
2. Open a thread in `support` as an outsider and reply into it (a SendGrid-shaped POST to
   `/webhooks/email/sendgrid`, or the in-app compose as a non-member).
3. The mailbox thread row shows **Needs instruction**; the open thread shows the banner with
   "Channel team", the age, `Normal`, no due date. The task board shows **no** new task. The
   attention queue shows the item under **Unassigned**.
4. Reload — the badge is still there. Open and close the thread — still there.
5. Claim it, set priority `High`, then write a note and press **Generate draft**. The badge becomes a
   non-actionable **Drafting** mark and the item leaves the attention queue. The task board shows one
   task.
6. When the run finishes, the badge becomes **Draft ready** and the item is back in the queue, now
   under **My work**.
7. Press **Edit and send**, change a word, send. The deliveries page shows exactly one reply; the
   handoff is gone from the queue and from the thread row.
8. Reply from outside again, then replay the same Message-ID. One new generation, nothing new
   enqueued or sent.
9. Flip `support` back to `Automatic` and reply from outside. The agent runs immediately and no
   handoff is created.
