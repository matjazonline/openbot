# Phase 4 — Generate draft, send, edit and send, dismiss

Read [`general_plan_instructions.md`](general_plan_instructions.md) first, then
[`phase2.md`](phase2.md) and [`phase3.md`](phase3.md). This phase assumes the generation model, the
event log and the responsibility commands all exist.

**Goal.** The four actions that get a held reply answered, each reusing machinery that already
exists rather than growing a parallel copy of it:

| Action | Reuses | New |
|---|---|---|
| **Generate draft** | `AddInternalNote`, `StartAgentTask` / `AskOwnerToAct` | one `thread_handoff_runs` row, written in the same transaction |
| *(the run finishes)* | `PreparedReviewDraft`, `create_review_draft_on` | one pure decision: draft instead of publish |
| **Send draft** | `ReviewCommand` + `ReviewAction::Approve` | the handoff resolves when the delivery is durably enqueued |
| **Edit and send** | `ReviewAction::Edit` then `Approve` | nothing — two existing commands in order |
| **Dismiss** | — | one command, one event, no message and no delivery |

This is the only phase that touches worker-path code, and the general file's **stock-stack budget**
applies to it: run `scripts/stack-frames.sh` before and after, record both numbers in the commit
message, and keep the draft-versus-publish decision a non-`async fn`.

**Files touched**

| File | Change |
|---|---|
| `migrations/20260817000000_init_schema.sql` | `thread_handoff_runs`, its keys and composite foreign keys |
| `src/domain/entities/thread_handoff.rs` | `ThreadHandoffRun`, `ThreadHandoffDraftOutcome`, `DraftTarget` + the pure `draft_target` |
| `src/domain/entities/internal_note.rs` | `AskOwnerToAct.handoff` and `StartAgentTask.handoff`, both `Option` |
| `src/adapters/persistence/task/instructions.rs` | one insert in `insert_instruction` / `record_started_task_instruction`; the handoff fence |
| `src/adapters/persistence/task/operations.rs` | `commit_agent_dispatch`: the handoff-draft branch and the `draft_ready` transition |
| `src/adapters/persistence/task/queue.rs` (or wherever terminal failure lands) | `draft_failed` returns the matching generation to `needs_instruction` |
| `src/adapters/persistence/response_review/commands.rs` | `approve_on` resolves the handoff generation |
| `src/adapters/persistence/attention.rs` | the `thread_handoff` branch gains `task_id`; the `response_review` branch suppresses handoff drafts |
| `src/adapters/persistence/thread_handoff.rs` | `request_draft`, `dismiss`, `run_for_task_on`, `resolve_generation_on` |
| `src/application/thread_handoff.rs`, `src/application/use_cases/thread_handoff.rs` | the port and façade methods |
| `src/adapters/http/routes/thread_handoffs.rs` | the four action routes |
| `src/adapters/http/pages/thread_handoffs.rs` | `thread_handoff_actions` — the action row Phase 5 folds into the banner |
| `src/adapters/http/pages/mailbox.rs` | render the action row in `message_pane` |
| tests | as below |

---

## 4.0 A correction to the general file

The general file says `response_drafts.source_handoff_generation` (init migration `:2881`,
`src/domain/entities/response_draft.rs:242`) is "a nullable, unused forward hook" that Phase 4
starts populating and gives a foreign key. **That is no longer true and must not be done.** The
column is already written on both review paths, with the id of the latest **task-ownership transfer
event**:

- `src/adapters/persistence/task/operations.rs:1499-1526` (`commit_agent_dispatch`);
- `src/adapters/persistence/task/operations.rs:1777-1802` (`complete_human_task`);
- read back through `DraftDb` / `DRAFT_COLUMNS` (`src/adapters/persistence/response_review.rs:30-58`),
  carried across an edit at `src/adapters/http/routes/ui_response_reviews.rs:339`, and **asserted**
  by `src/adapters/persistence/task/tests.rs:920-923` ("a draft created after manual transfer
  retains the handoff event identity").

So the name means *task* handoff — the `task_ownership_events.handoff_instruction` sense the general
file's collision table already warns about — not this plan's generation. Repurposing it would
silently change an asserted behaviour and would put two unrelated kinds of id in one column, and a
foreign key to `thread_handoffs(company_id, generation)` would immediately fail on existing rows.

**Phase 4 therefore does not touch `source_handoff_generation` at all.** A draft is correlated to a
handoff generation through `thread_handoff_runs.task_id = response_drafts.task_id`, which is
available on every path that matters because a handoff draft is always produced by a task. The
general file has been corrected to say this.

## 4.1 `thread_handoff_runs`

One row per drafting run. It is the fence, the correlation and the idempotency record.

```sql
CREATE TABLE public.thread_handoff_runs (
    company_id uuid NOT NULL,
    task_id uuid NOT NULL,
    handoff_id uuid NOT NULL,
    generation uuid NOT NULL,
    requested_by_principal_id uuid NOT NULL,
    command_id uuid NOT NULL,
    draft_id uuid,
    draft_version integer,
    state text DEFAULT 'running'::text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT thread_handoff_runs_draft_check CHECK ((((draft_id IS NULL) AND (draft_version IS NULL)) OR ((draft_id IS NOT NULL) AND (draft_version > 0)))),
    CONSTRAINT thread_handoff_runs_state_check CHECK ((state = ANY (ARRAY['running'::text, 'drafted'::text, 'failed'::text, 'superseded'::text])))
);
```

```sql
ADD CONSTRAINT thread_handoff_runs_pkey PRIMARY KEY (company_id, task_id);
ADD CONSTRAINT thread_handoff_runs_generation_key UNIQUE (company_id, generation);
ADD CONSTRAINT thread_handoff_runs_task_fk FOREIGN KEY (company_id, task_id)
    REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE;
ADD CONSTRAINT thread_handoff_runs_handoff_fk FOREIGN KEY (company_id, handoff_id)
    REFERENCES public.thread_handoffs(company_id, id) ON DELETE CASCADE;
ADD CONSTRAINT thread_handoff_runs_generation_fk FOREIGN KEY (company_id, generation)
    REFERENCES public.thread_handoffs(company_id, generation) ON DELETE CASCADE;
```

The three keys are the whole concurrency design and each one answers a specific race from the
general file's competing-claimant table:

- **`PRIMARY KEY (company_id, task_id)`** — the worker's only lookup. It is keyed by task because
  that is the one identifier a durable payload carries; nothing about the handoff goes into
  `InboundTaskPayloadV1`, per the general file's "durable payloads carry identifiers only".
- **`UNIQUE (company_id, generation)`** — **at most one run per generation, ever.** This is what
  makes "two concurrent Generate draft commands on one generation" produce one task: the second
  insert violates the key and its whole transaction — task row included — rolls back, so the loser
  gets a `Conflict` and no orphan task exists. Do **not** replace it with a `SELECT … FOR UPDATE`
  and a state check; the unique key is the only version of this that is correct without a lock
  ordering argument.
- **`FOREIGN KEY (company_id, generation)` → `thread_handoffs(company_id, generation)`** — a run
  cannot name a generation that does not exist, and `ON DELETE CASCADE` from a regeneration is
  **not** what happens: a regeneration *updates* the `generation` column, so the foreign key would
  block it. That is deliberate and is resolved in §4.4: a regeneration first marks the open run
  `superseded` and nulls nothing, then the handoff's own `UPDATE` moves the generation, and the
  foreign key is `NOT VALID`-free only because Phase 2's `DO UPDATE` runs in the same transaction.
  **If that ordering proves awkward in implementation, drop `thread_handoff_runs_generation_fk` and
  keep the two other keys** — the `UNIQUE` is the load-bearing one, and a dangling generation in a
  `superseded` run is harmless. Decide this once, in code, and write which you chose in the PR.

No index beyond the keys: every read is by primary key (the worker), by `generation` (the command),
or a join from `response_drafts.task_id` (the projection), and all three are covered.

`state` exists so a run's outcome is legible without joining the task: `running` while the agent
works, `drafted` once a draft exists, `failed` on terminal failure, `superseded` when a newer
generation opened while it ran.

## 4.2 Generate draft

**The precondition: the handoff must be claimed.** `request_draft` is refused unless the handoff's
`responsible_principal_id` is `Some(actor)` — or the actor is a manager acting on a claimed handoff.
This is not ceremony. `create_review_draft_on` takes an `assigned_reviewer: Option<PrincipalId>`
(`src/adapters/persistence/response_review.rs:190-215`) and `execute_command` authorizes **only the
assigned reviewer** (`src/adapters/persistence/response_review/commands.rs:61-82`). Fixing the
reviewer to the responsible principal at draft time is what makes **Send** work later with no change
to the review machinery at all. An unclaimed handoff therefore shows "Claim it first" rather than a
disabled button.

**The note is an existing use case.** If the team writes an instruction, that is `AddInternalNote`
(`src/domain/entities/internal_note.rs:50-74`) through the existing
`POST /ui/internal-notes` route — the handoff adds nothing, and `MAX_INTERNAL_NOTE_BYTES` (65 536,
`:13`) is already the bound. Phase 4 does not invent a handoff-instruction field.

**The run is an existing use case too**, one of two depending on what the thread is doing — exactly
the choice `internal_note_actions` already makes at
`src/adapters/http/pages/mailbox.rs:2609-2616`:

- the thread has no active task → `StartAgentTask` (`internal_note.rs:118-143`), which refuses if
  one appears (`ensure_thread_has_no_active_task`,
  `src/adapters/persistence/task/instructions.rs:348-372`);
- the thread has an **agent-owned** active task → `AskOwnerToAct` (`:86-116`), which refuses unless
  the owner is an agent and `expected_ownership_version` matches
  (`instruction_outcome`, `:146-165`), and requeues a `processing` task
  (`requeue_processing_task`, `:204-255`).

Both commands gain **one optional field**:

```rust
/// The handoff generation this run answers, when the run was started from a handoff.
///
/// `Option` because the note/ask surfaces predate handoffs and must keep working unchanged: a
/// `None` here is "an ordinary agent run", not "a handoff run with a missing id".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoffRunRequest {
    pub handoff_id: Uuid,
    pub generation: Uuid,
    pub expected_version: u64,
}
```

`AskOwnerToAct.handoff: Option<HandoffRunRequest>` and `StartAgentTask.handoff:
Option<HandoffRunRequest>`, both included in `validate()` only to the extent of "a generation is not
nil", and both **inside the command fingerprint** — `instructions.rs:263-271` and `:465-471` build
those `json!` objects by hand, so add the field there. Two `Generate draft` presses with the same
`command_id` but different generations must be a `Conflict`, and that is what puts it in the
fingerprint.

In persistence, both paths gain the same three steps, inside their **existing** transactions:

1. after the existing guards and before the task write, fence the handoff:

   ```sql
   SELECT state, generation, responsible_principal_id, version
     FROM thread_handoffs
    WHERE company_id = $1 AND id = $2 AND thread_id = $3 FOR UPDATE
   ```

   then, in a pure helper, refuse unless `state == needs_instruction`,
   `generation == request.generation`, `version == request.expected_version` and the responsibility
   check above passes. The generation mismatch message names the current generation, as Phase 3 §3.4
   step 7 does — the same sentence, from the same helper.
2. `INSERT INTO thread_handoff_runs (company_id, task_id, handoff_id, generation,
   requested_by_principal_id, command_id) VALUES (…)` — the unique-key collision here is the
   "two concurrent Generate draft" loser, and it must surface as
   `AppError::Conflict("A draft is already being prepared for this reply.")` rather than a raw
   `sqlx` unique-violation. Map it explicitly on the error's constraint name; do not `ON CONFLICT DO
   NOTHING`, which would leave a task running for nobody.
3. move the handoff to `drafting` (`version + 1`) and append one `thread_handoff_events` row with
   `operation = 'draft_requested'`, `actor_kind = 'human'`, the actor, `task_id` set,
   `from_state = 'needs_instruction'`, `to_state = 'drafting'`.

`ask_owner_to_act`'s idempotency record (`task_agent_instructions`, checked at `:274-289`) and
`start_agent_task`'s (`start_agent_task_commands`, `:475-493`) already return the recorded outcome
for a replay, and because the `thread_handoff_runs` insert is in the same transaction, a replay
correctly writes nothing a second time. That is the reason for extending these commands rather than
wrapping them: a wrapper would need its own idempotency store and its own transaction, and the task
and the run row could then disagree.

**The handoff leaves the attention queue** the moment it is `drafting`, because Phase 3's branch
filters `state IN ('needs_instruction','draft_ready')`. No projection change is needed for that.

## 4.3 The worker path: a draft instead of a publication

`commit_agent_dispatch` (`src/adapters/persistence/task/operations.rs:1406-1626`) currently has two
outcomes: park for review when `effective_review_required_on` says so (`:1474-1573`), or publish
(`:1575-1625`). It gains a third input and keeps both existing outcomes intact.

**The decision is pure and lives in the domain**, which is the stack-budget rule from the general
file:

```rust
/// Where an agent's proposed external reply goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftTarget {
    /// Publish it and queue the delivery. Today's default.
    Publish,
    /// Park it for a human reviewer, because the channel requires review.
    Review,
    /// Park it as this handoff generation's proposed reply.
    HandoffDraft,
}

/// `review_required` and `handoff_run` are both already loaded when this is asked.
pub const fn draft_target(review_required: bool, is_handoff_run: bool) -> DraftTarget { … }
```

`HandoffDraft` wins over `Review` when both are true: a handoff draft already has a named human — the
responsible principal, who is also the assigned reviewer — so routing it through the generic review
queue as well would list one piece of work twice. §4.6 makes that single-listing explicit in SQL.

In `commit_agent_dispatch`, immediately after the lease fence at `:1447-1472` (so a lost lease still
writes nothing) and before the review check at `:1474`, one read:

```sql
SELECT handoff_id, generation FROM thread_handoff_runs
 WHERE company_id = $1 AND task_id = $2 AND state = 'running'
```

Then `draft_target(review_required, run.is_some())`, and a `DraftTarget::HandoffDraft` arm that
reuses the review branch's own body almost verbatim:

- the same `DraftPublicationSnapshot::new(…).with_also_in_threads(…)` as `:1496-1498`;
- the same `PreparedReviewDraft::new(…)` as `:1511-1525`, with `author_principal_id` the agent
  principal and `created_by_principal_id` the same, and **`source_handoff_generation` left `None`**
  (§4.0);
- `create_review_draft_on(&mut tx, &draft, Some(responsible_principal))` — the assigned reviewer is
  the handoff's responsible principal, read from `thread_handoffs` in the same transaction. If it is
  `NULL` (a manager released the handoff mid-run), fall back to `None` and let
  `resolve_reviewer_on` (`src/adapters/persistence/response_review.rs:322-…`) pick, exactly as the
  review path does;
- `draft.expires_at` set to a **deliberate** value rather than the `DEFAULT_REVIEW_EXPIRY_DAYS`
  default (`src/application/use_cases/response_review.rs:193`). A handoff draft that expires makes
  `Send` fail with "This review has expired." (`commands.rs:56-60`); §4.5 turns that into a clean
  `draft_failed` transition, so the default is survivable — but say in a comment which value was
  chosen and why, and do not leave it implicit;
- then `UPDATE thread_handoff_runs SET state = 'drafted', draft_id = …, draft_version = 1,
  updated_at = CURRENT_TIMESTAMP WHERE company_id = $1 AND task_id = $2 AND state = 'running'`,
  `rows_affected() != 1` → `Conflict`;
- then the handoff: `state = 'draft_ready'`, `version + 1`, **`WHERE … AND generation = $n`** — the
  generation the *run* named. A zero-row update means the generation was replaced while the agent
  worked, which is the general file's "a draft task completing against a generation that has since
  been replaced" case: **the write is refused** (`AppError::Conflict`), the whole transaction rolls
  back, the newer generation stays `needs_instruction`, and the task retries or dead-letters without
  ever having published anything;
- one `thread_handoff_events` row, `operation = 'draft_ready'`, `actor_kind = 'agent'`, the agent
  principal, `task_id`, `draft_id`, `draft_version`;
- the task goes to `pending_approval` exactly as the review branch does at `:1553-1567`, and the
  function returns `DispatchCommit::PendingReview { draft_id, draft_version }`. **No new
  `DispatchCommit` variant**: the caller's behaviour is identical — stop, do not send, wait for a
  human — and a new variant would mean a new arm in every match on it for no behavioural difference.

**Stack budget.** The new code is one `sqlx::query_as`, one `const fn` call and one branch whose body
is a near-copy of the branch above it. To keep the chain from growing, extract the shared part of the
two branches into a **synchronous** helper that builds the `PreparedReviewDraft` from
`(&commit, author, reviewer)` and returns it, so both arms call it and neither adds an `async fn`
level. `src/AGENTS.md`'s "extract synchronous helpers rather than adding `async fn` levels" is the
rule; the numbers in the commit message are the proof.

## 4.4 When a run does not produce a draft

Three ways a drafting run ends without a `draft_ready`, and each one must leave the handoff in a
state the team can act on:

1. **Terminal task failure** (`dead_letter`, or `stopped` by an operator). Wherever the task reaches
   a terminal non-completed status, add: if a `thread_handoff_runs` row exists for
   `(company_id, task_id)` in state `running`, mark it `failed` and move **only the matching**
   generation back to `needs_instruction` —
   `UPDATE thread_handoffs SET state = 'needs_instruction', version = version + 1 WHERE company_id
   = $1 AND id = $2 AND generation = $3 AND state = 'drafting'` — with a `thread_handoff_events` row
   `operation = 'draft_failed'`, `actor_kind = 'system'`, and the task's error in `failure_reason`
   (bounded at 2048 bytes by Phase 2's `CHECK`; truncate on a char boundary before binding, never
   let the `CHECK` be the truncation). The `AND generation = $3` is the general file's "terminal
   draft-task failure returns only the **matching** `drafting` generation": a failure for an old
   generation must not drag a newer `needs_instruction` handoff backwards or overwrite a
   `draft_ready` one.
2. **A newer outside reply while the run is in flight.** Phase 2's `DO UPDATE` moves the generation;
   the run's own completion is then refused by the generation predicate in §4.3. Phase 2's
   `open_handoff_generation_on` gains one statement for this — mark any `running` run for the
   *outgoing* generation `superseded`, before the handoff's `UPDATE` moves the column — which is
   also what keeps `thread_handoff_runs_generation_fk` satisfiable (§4.1). The task itself is left
   alone: it is an ordinary agent task, it will finish, and its commit will be refused. Do **not**
   try to cancel it; task cancellation has its own ownership rules and a handoff has no business
   reaching into them.
3. **A lost lease or a task transfer.** Neither touches the handoff. The general file is explicit:
   "a task transfer or a lost lease never changes who the handoff is responsible-to". The lease
   fence at `:1447-1472` already makes a superseded execution write nothing, and the handoff stays
   `drafting` until the surviving execution commits or the task dies (case 1).

## 4.5 Send draft, and edit and send

**Send draft** is `ReviewCommand { action: ReviewAction::Approve { rationale: None }, … }` through
the existing `ResponseReviewUseCases::execute`, with `expected_draft_version` from the run row. It
already: locks the draft and review `FOR UPDATE` (`commands.rs:22-43`), refuses a stale or
non-latest version (`:44-55`), refuses an expired review (`:56-60`), authorizes the assigned
reviewer (`:61-82`), writes the canonical message and its associations, enqueues the delivery,
marks the draft `published`, records the publication row and completes the task
(`approve_on`, `:223-320`). Phase 4 adds **one thing** to `approve_on`, at the end and in the same
transaction:

```sql
UPDATE thread_handoffs AS handoff
   SET state = 'resolved', closed_at = CURRENT_TIMESTAMP, version = handoff.version + 1,
       updated_at = CURRENT_TIMESTAMP
  FROM thread_handoff_runs AS run
 WHERE run.company_id = handoff.company_id AND run.handoff_id = handoff.id
   AND run.generation = handoff.generation
   AND (run.company_id, run.draft_id) = ($1, $2)
   AND handoff.state = 'draft_ready'
RETURNING handoff.id, handoff.generation, handoff.version
```

plus the `resolved` event when a row comes back. Three properties this shape has and a naive version
does not:

- **It resolves nothing when the draft is not a handoff draft**, because the join finds no run. So
  every existing review approval is byte-for-byte unchanged, which is what the regression suite
  proves.
- **It resolves only the generation the draft belongs to** (`run.generation = handoff.generation`).
  A draft written for a superseded generation cannot resolve the current one — and in fact it cannot
  be approved at all, because §4.3 refused to create it.
- **It is inside the publication transaction**, so the general file's "resolves the handoff
  generation when the logical external delivery is durably enqueued" is a transactional fact rather
  than a hope. `Send` racing `Dismiss` therefore has exactly one winner: whichever transaction
  commits second finds the state it required (`draft_ready`) already gone and reports a `Conflict`
  naming the current version.

**Edit and send** is `ReviewAction::Edit` followed by `Approve`, both existing, in that order — the
same pair the review UI already performs across two requests
(`src/adapters/http/routes/ui_response_reviews.rs:296-369`, then `:230-248`). The handoff route does
both in one handler with **two** `command_id`s derived from the request's own, so a retry replays
both idempotently. Say plainly in a comment that this is two transactions: if the `Edit` commits and
the `Approve` fails, the draft is at version *n+1* pending and the handoff is still `draft_ready`, so
the banner's Send button simply targets the new version. That is recoverable and visible, which is
better than a bespoke single-transaction edit-and-publish path that would have to duplicate
`create_review_draft_on`'s validation.

`prepare_response_review_edit` (`src/application/use_cases/thread/human_completion.rs:165-285`)
already carries `source_handoff_generation` across an edit (`:283`) — leave that alone; per §4.0 it
is the task-transfer id and copying it forward is correct.

**Expiry.** If `Send` or `Edit` returns the "This review has expired." `Conflict`, the route
converts it into a `draft_failed` transition on the matching generation — run `failed`, handoff back
to `needs_instruction`, `failure_reason` "the drafted reply expired before it was sent" — and reports
a message that tells the user to generate a new draft. Without that, an expired review would leave a
`draft_ready` handoff whose only button can never succeed.

## 4.6 Dismiss, and the projection seam

**Dismiss** is the smallest command in the plan and deliberately writes nothing but the handoff and
its event: `state = 'dismissed'`, `closed_at`, `version + 1`, fenced on
`(generation, version)` and on `state IN ('needs_instruction','draft_ready')`, event
`operation = 'dismissed'`, `actor_kind = 'human'`. No message, no delivery, no task change — the
customer's message stays on the thread exactly as it is, because dismissing is "we are not answering
this through the handoff queue", not "this did not happen". A `drafting` handoff cannot be dismissed:
a run is in flight and the answer to "stop it" is to let it finish or let it fail, per §4.4.

Reuse Phase 3's `change_thread_handoff` skeleton for it — lock, replay, load, fence, write, event —
rather than a second hand-rolled sequence. In practice that means `dismiss` and `request_draft` are
two more `ThreadHandoffOperation`-shaped commands sharing one private `apply_command` in
`src/adapters/persistence/thread_handoff.rs`; if they are not sharing it by the end of this phase,
the file has three copies of the same fence and the review will say so.

**Two projection edits** in `src/adapters/persistence/attention.rs`:

1. the `thread_handoff` branch's `NULL::uuid AS task_id` (Phase 3 §3.1) becomes the run's task:
   `LEFT JOIN thread_handoff_runs AS run ON run.company_id = handoff.company_id AND run.generation =
   handoff.generation` and `run.task_id`. `correlation_id` follows from a join on
   `background_tasks` only if a use appears for it — if nothing reads it, leave it `NULL` and say so
   rather than adding a join for symmetry.
2. the `response_review` branch (`:211-229`) gains one predicate:

   ```sql
   AND NOT EXISTS (
       SELECT 1 FROM thread_handoff_runs AS run
       WHERE run.company_id = draft.company_id AND run.task_id = draft.task_id
   )
   ```

   "a draft belonging to a handoff drafting run is queued as its handoff, not as a review." This is
   the same de-duplication the `task` branch already performs against pending reviews at `:138-147`,
   pointed the other way, and it is what makes `DraftTarget::HandoffDraft` beating
   `DraftTarget::Review` (§4.3) show up as one item instead of two. The `/reviews` page still lists
   the draft — only the attention feed de-duplicates — so a reviewer who prefers that surface keeps
   it.

## 4.7 The action surface

`src/adapters/http/routes/thread_handoffs.rs` (created in Phase 3) gains four POSTs, all
`read_context`-authorized exactly as the command route is:

| Route | Body | Effect |
|---|---|---|
| `POST …/thread-handoffs/{id}/draft` | `command_id, expected_version, expected_generation, note_ids` | `AddInternalNote` is already done; this starts the run |
| `POST …/thread-handoffs/{id}/send` | `command_id, expected_version, expected_generation, draft_version` | `Approve` |
| `POST …/thread-handoffs/{id}/send-edited` | the above plus `subject, body, recipient_to, recipients_cc` | `Edit` then `Approve` |
| `POST …/thread-handoffs/{id}/dismiss` | `command_id, expected_version, expected_generation` | `dismiss` |

Every body carries `expected_version` **and** `expected_generation`, and every route returns the
conflict verbatim from persistence so the message the user sees names the current generation. A
route that silently retried with a refreshed version would defeat the entire fencing design; state
that in a comment on the shared extractor.

`src/adapters/http/pages/thread_handoffs.rs` gains `thread_handoff_actions(&MessagePane<'_>) ->
String`: the four buttons under the open thread, rendered next to `internal_note_actions`
(`src/adapters/http/pages/mailbox.rs:2609-2652`) and driven by the handoff's state —
`needs_instruction` shows **Generate draft** (reusing the existing `#ask-agent-form` note checkboxes
for `note_ids`) and **Dismiss**; `drafting` shows a disabled "Drafting…" with no actions;
`draft_ready` shows **Send**, **Edit and send** and **Dismiss**. It is rendered from `message_pane`
(`:2256-2263`, in the block with the other panels) behind a new
`MessagePane.handoff: Option<&ThreadHandoff>` field, populated in `render_message_pane`
(`src/adapters/http/routes/ui.rs:407-493`) from Phase 3's `thread_handoffs_for_threads`.

**Phase 5 folds this row into the banner** — the same function, moved into the banner's markup, with
the responsibility, age, priority and due time above it. Keep the buttons in one function now so
Phase 5 relocates markup rather than rewriting behaviour, and do not build a second set of buttons
in the banner.

---

## Tests

**Unit, no database:**

1. `draft_target`: all four `(review_required, is_handoff_run)` combinations, with
   `(true, true) == HandoffDraft` called out in its own named test because that is the precedence
   decision.
2. `HandoffRunRequest` in the fingerprints: two otherwise identical `AskOwnerToAct`s differing only
   in `handoff.generation` produce different fingerprints; the same for `StartAgentTask`. A `None`
   handoff produces the same fingerprint the pre-Phase-4 command did — assert the literal hash so a
   future field addition to that `json!` cannot silently change replay behaviour for existing
   commands.
3. The `failure_reason` truncation helper cuts on a char boundary and never exceeds 2048 bytes
   (feed it a multi-byte string whose 2048th byte is mid-character).

**Generate draft** (`src/adapters/persistence/thread_handoff_tests.rs`), each scoped to its own
company, and — per the general file's DB-test convention — each either using a channel with no agent
so the task has no owner a worker can claim, or completing the task it enqueues:

4. **Happy path, no active task.** A claimed `needs_instruction` handoff plus one note: one task, one
   `thread_handoff_runs` row in `running` naming that task and generation, handoff `drafting` at
   `version + 1`, one `draft_requested` event with the task id.
5. **Happy path, agent-owned active task.** The same through `AskOwnerToAct`: no new task, the
   existing one requeued, one run row, the same handoff transition. Assert the requeue by the
   `task_attempts` row `requeue_processing_task` closes at
   `src/adapters/persistence/task/instructions.rs:242-253`.
6. **Unclaimed handoff** is refused, nothing written — no task, no run row, handoff untouched.
7. **Claimed by somebody else**, actor is not a manager: `NotFound`, nothing written. As a manager:
   allowed.
8. **Two concurrent Generate drafts on one generation**: one run row, one task, the loser gets the
   `Conflict` and its task rolled back — assert `SELECT count(*) FROM background_tasks WHERE
   company_id = $1` is 1, scoped to the test's company.
9. **Stale generation** and **stale version**: `Conflict` naming the current value, nothing written.
10. **Replay** with the same `command_id`: the recorded outcome, still one task and one run row.
    Same id with a different generation: `Conflict`.
11. **`drafting` or terminal handoff**: refused.
12. The handoff is **absent from every attention view** while `drafting`, and the thread still
    renders (the queue and the thread are different surfaces).

**The worker path** (`src/adapters/persistence/task/tests.rs`, beside
`review_policy_parks_agent_dispatch_and_private_rejection_feedback_reaches_retry` at `:1154-1297`,
which is the harness to copy):

13. **Draft instead of publish.** A handoff run's `commit_agent_dispatch` returns
    `DispatchCommit::PendingReview`, writes a `response_drafts` version 1 whose `task_id` is the run's
    task and whose reviewer is the handoff's responsible principal, enqueues **no** delivery, moves
    the handoff to `draft_ready`, sets the run `drafted` with `draft_id`/`draft_version`, and writes
    one `draft_ready` event with `actor_kind = 'agent'`.
14. **`source_handoff_generation` is untouched** by the handoff path — it holds whatever the
    ownership-transfer lookup produced (usually `NULL` for an agent-owned dispatch), and never the
    thread-handoff generation. This is §4.0's regression guard.
15. **Review policy on, handoff run**: still one draft, still queued as a handoff, and
    `list_attention` returns exactly one item for that thread — a `thread_handoff`, not a
    `response_review`. The §4.6 predicate is what this tests.
16. **Review policy on, ordinary run**: unchanged — one `response_review` item, no handoff. Prove the
    predicate did not over-reach.
17. **Stale generation at completion.** Open a run, replace the generation with a second held reply,
    then complete the run: the commit is refused, no draft row exists, the newer generation is still
    `needs_instruction`, and the run row is `superseded`.
18. **Terminal failure.** Drive the run's task to `dead_letter`: run `failed`, handoff back to
    `needs_instruction` at `version + 1`, one `draft_failed` event carrying a bounded
    `failure_reason`.
19. **Terminal failure for a stale generation** leaves a newer `needs_instruction` handoff at its own
    version, and leaves a `draft_ready` one `draft_ready`. The `AND generation = $3` predicate is the
    subject.
20. **Task transfer during `drafting`** does not change `responsible_principal_id` and does not
    change the handoff state.

**Send, edit and send, dismiss:**

21. **Send.** One canonical message, one delivery, draft `published`, review `published`, one
    `response_draft_publications` row, task `completed`, handoff `resolved` with `closed_at` and one
    `resolved` event. Then the handoff is in no attention view and `get_thread_handoff` still returns
    it.
22. **Send by a principal who is not the assigned reviewer**: `NotFound` from the existing review
    machinery, handoff untouched. A manager who is the company owner gets the same `NotFound` for
    `Approve` — only `Reassign` is owner-permitted (`commands.rs:65-69`) — so the banner must not
    offer Send to a manager who is not the reviewer. Assert the page does not render the button in
    that case.
23. **Edit and send.** Draft version 2 exists and is `published`, version 1 is `superseded`, exactly
    one delivery for the thread, handoff `resolved`. Replaying the same request writes nothing
    further.
24. **Edit and send racing Send.** One publication, one logical delivery, one canonical message; the
    loser is a `Conflict`.
25. **Send racing Dismiss.** Exactly one wins; the loser's `Conflict` names the current version;
    and if `Dismiss` won there is no delivery anywhere for that thread.
26. **Expired review.** Force `response_reviews.expires_at` into the past, press Send: the handoff is
    back in `needs_instruction` with a `draft_failed` event, the run is `failed`, and no delivery was
    enqueued.
27. **Dismiss.** `dismissed` with `closed_at`, one event, no message, no delivery, no task change;
    the customer's message is still on the thread with its original audience and entry kind. A
    `drafting` handoff refuses dismissal.
28. **Ordinary review approvals are unchanged.** The existing review suite passes untouched; if any
    assertion in it changes, the §4.5 `UPDATE … FROM thread_handoff_runs` is matching rows it should
    not.

**Routes:** each of the four for another company's handoff id → `NotFound`; for a channel with no
`view` grant → `NotFound`; with a mismatched `expected_generation` → the conflict text containing
the current generation; with an unknown field → `BadRequest`.

**Stack frames:** `scripts/stack-frames.sh` before and after, both numbers in the commit message,
and no growth that `src/AGENTS.md` would call material.

## Done when

- [ ] `thread_handoff_runs` exists with its primary key, its `UNIQUE (company_id, generation)` and
      its composite foreign keys; the generation-FK decision from §4.1 is made and recorded in the
      PR; both databases recreated and `.sqlx/` regenerated and committed.
- [ ] `source_handoff_generation` is **not** written by any new code and has no new foreign key;
      `src/adapters/persistence/task/tests.rs:920-923` still passes unchanged.
- [ ] `Generate draft` goes through `AddInternalNote` + `StartAgentTask`/`AskOwnerToAct` with one
      added `Option` field, one added insert, and no new idempotency store.
- [ ] `draft_target` is a `const fn` in the domain with all four combinations unit-tested, and the
      worker path gained no `async fn` level.
- [ ] A completing handoff run writes a `ResponseDraft` and no delivery, and a run whose generation
      was replaced is refused.
- [ ] `Send` resolves only the matching generation, inside the publication transaction, and every
      pre-existing review approval is unaffected.
- [ ] One piece of work appears once in the attention feed: the `response_review` suppression
      predicate is present and case 16 proves it does not over-reach.
- [ ] Cases 1-28 pass, plus the general file's competing-claimant table end to end.
- [ ] `cargo fmt --check`, `SQLX_OFFLINE=true cargo build --all-targets`, `cargo test` and
      `cargo clippy --all-targets -- -D warnings` are green.
