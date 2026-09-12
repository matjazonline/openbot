# Phase 2 — The hold gate, the tables, and the generation model

Read [`general_plan_instructions.md`](general_plan_instructions.md) first, and
[`phase1.md`](phase1.md) after it: this phase assumes `ExternalReplyHandling`, the
`ExternalReplyHandlingPolicy` resolver and the `ThreadHandoffPolicyPersistence` port already exist
and already round-trip.

**Goal.** An eligible outside reply to an existing thread on a `ManualHandoff` channel is **filed as
the customer message it is** and opens a `thread_handoffs` generation in state `needs_instruction`,
in the *same* transaction, with **no `background_tasks` row for that channel**. Everything else about
that message — its audience, its entry kind, its participants, its provider mapping, the other
channels' tasks — is unchanged.

This is the only phase that changes what happens to an inbound message. The change is deliberately
narrow and lives in three places:

1. one pure eligibility decision over values the read-only phases already loaded (§2.2);
2. `CommitPlan::build`, which drops a held channel from `task.targets` and states the hold in the
   request (§2.3);
3. the existing inbound commit, which writes one `thread_handoffs` row plus its opening event
   (§2.4).

Nothing reads the handoff yet except the thread it sits on. Phase 3 puts it in the attention queue;
after this phase a held reply is visible only through the database and through the absence of a task.
That is intentional — it makes the hold testable before any projection or UI depends on it.

**Files touched**

| File | Change |
|---|---|
| `migrations/20260817000000_init_schema.sql` | `thread_handoffs`, `thread_handoff_events`, `thread_handoff_events_are_immutable()`, their keys and composite foreign keys |
| `src/domain/entities/thread_handoff.rs` | `ThreadHandoffState`, `ThreadHandoffGeneration`, `ThreadHandoff`, `HoldDecision`, and the pure `hold_outside_reply` |
| `src/application/transport/ingress.rs` | `InboundHold`, `InboundCommitRequest.holds`, `InboundCommitOutcome.handoff_ids` |
| `src/application/use_cases/thread/ingest/routing.rs` | resolve the effective policy per candidate; carry `hold` on `PreparedChannel` |
| `src/application/use_cases/thread/ingest/commit.rs` | `PreparedChannel.hold`; `CommitPlan::build` excludes held channels from `targets` and builds `holds` |
| `src/application/use_cases/thread/ingest/policy.rs` | nothing — see §2.2 on why the decision does **not** live here |
| `src/application/use_cases/thread/mod.rs` | `ThreadUseCases` gains `Arc<dyn ThreadHandoffPolicyPersistence>` |
| `src/adapters/persistence/thread/inbound.rs` | `create_handoffs` inside `commit_on`, after `create_task` |
| `src/adapters/persistence/thread_handoff.rs` | `open_handoff_generation_on` and the event append |
| `src/adapters/persistence/thread_handoff_tests.rs` | generation, replacement, idempotency and immutability tests |
| `src/adapters/persistence/thread/inbound_tests.rs` | commit-level tests: held reply writes a handoff and no task |
| `src/application/use_cases/thread/tests.rs` | eligibility table as in-memory ingest cases |
| `README.md` §3.7 | the hold as a channel policy, and that a held message is still the customer's message |

---

## 2.1 Schema

Both tables are edited into the squashed init migration in place, in `pg_dump` order and in their
alphabetical section. Recreate both databases afterwards and regenerate `.sqlx/` (general file).

**`thread_handoffs`** — sorts between `thread_handoff_events` and `thread_messages`
(`migrations/20260817000000_init_schema.sql:3385` is `thread_messages`, which is where the new
section goes just before). One row **per thread**, mutated in place as generations open and close:

```sql
CREATE TABLE public.thread_handoffs (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    generation uuid NOT NULL,
    state text DEFAULT 'needs_instruction'::text NOT NULL,
    source_message_id uuid NOT NULL,
    responsible_principal_id uuid,
    business_priority text DEFAULT 'normal'::text NOT NULL,
    business_due_at timestamp with time zone,
    version bigint DEFAULT 1 NOT NULL,
    generation_opened_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    closed_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT thread_handoffs_closure_check CHECK ((((state = ANY (ARRAY['resolved'::text, 'dismissed'::text])) AND (closed_at IS NOT NULL)) OR ((state <> ALL (ARRAY['resolved'::text, 'dismissed'::text])) AND (closed_at IS NULL)))),
    CONSTRAINT thread_handoffs_priority_check CHECK ((business_priority = ANY (ARRAY['normal'::text, 'high'::text, 'urgent'::text]))),
    CONSTRAINT thread_handoffs_state_check CHECK ((state = ANY (ARRAY['needs_instruction'::text, 'drafting'::text, 'draft_ready'::text, 'resolved'::text, 'dismissed'::text]))),
    CONSTRAINT thread_handoffs_version_check CHECK ((version > 0))
);
```

Keys, copying `manual_handoffs` exactly (`:4057-4069`) so composite foreign keys can reference it:

```sql
ALTER TABLE ONLY public.thread_handoffs
    ADD CONSTRAINT thread_handoffs_pkey PRIMARY KEY (id);
ALTER TABLE ONLY public.thread_handoffs
    ADD CONSTRAINT thread_handoffs_company_id_id_key UNIQUE (company_id, id);
ALTER TABLE ONLY public.thread_handoffs
    ADD CONSTRAINT thread_handoffs_thread_key UNIQUE (company_id, thread_id);
ALTER TABLE ONLY public.thread_handoffs
    ADD CONSTRAINT thread_handoffs_company_id_generation_key UNIQUE (company_id, generation);
```

`thread_handoffs_thread_key` is the **"at most one handoff per thread"** rule, enforced by the
database rather than by a `SELECT … FOR UPDATE` that a concurrent commit could interleave with. It is
also what makes the hold write an `INSERT … ON CONFLICT (company_id, thread_id) DO UPDATE` — see
§2.4. `thread_handoffs_company_id_generation_key` is what Phase 4's `thread_handoff_runs` foreign
key hangs off, and it makes "a generation belongs to exactly one thread" a schema fact.

Foreign keys, all composite through the company, in the alphabetical FK section beside
`manual_handoffs_*` (`:6621-6657`):

```sql
ADD CONSTRAINT thread_handoffs_channel_fk FOREIGN KEY (company_id, channel_id)
    REFERENCES public.channels(company_id, id) ON DELETE CASCADE;
ADD CONSTRAINT thread_handoffs_responsible_fk FOREIGN KEY (company_id, responsible_principal_id)
    REFERENCES public.principals(company_id, id) ON DELETE SET NULL (responsible_principal_id);
ADD CONSTRAINT thread_handoffs_source_message_fk FOREIGN KEY (company_id, source_message_id)
    REFERENCES public.messages(company_id, id) ON DELETE CASCADE;
ADD CONSTRAINT thread_handoffs_thread_fk FOREIGN KEY (company_id, channel_id, thread_id)
    REFERENCES public.threads(company_id, channel_id, id) ON DELETE CASCADE;
```

`ON DELETE SET NULL` on the responsible principal is not a convenience: it is the reason
`manual_handoffs` carries `bump_handoff_version_for_responsibility_cleanup()` (`:57-72`, trigger at
`:5804`). Deleting a principal silently changes who a handoff is responsible-to, and a version that
did not move would let a claimed-then-orphaned handoff accept a command written against the claim.
`thread_handoffs` gets the **same** trigger function — reuse it, do not copy it:

```sql
CREATE TRIGGER thread_handoffs_bump_cleanup_version BEFORE UPDATE OF responsible_principal_id
    ON public.thread_handoffs FOR EACH ROW
    EXECUTE FUNCTION public.bump_handoff_version_for_responsibility_cleanup();
```

**`thread_handoff_events`** — the immutable audit log, and the idempotency store. One row per
accepted command, `attention_source_events` (`:1851-1873`) being the model:

```sql
CREATE TABLE public.thread_handoff_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    company_id uuid NOT NULL,
    handoff_id uuid NOT NULL,
    generation uuid NOT NULL,
    command_id uuid NOT NULL,
    command_fingerprint text NOT NULL,
    operation text NOT NULL,
    actor_kind text NOT NULL,
    actor_principal_id uuid,
    from_state text,
    to_state text NOT NULL,
    from_version bigint NOT NULL,
    to_version bigint NOT NULL,
    previous_priority text,
    new_priority text,
    previous_due_at timestamp with time zone,
    new_due_at timestamp with time zone,
    previous_responsible_principal_id uuid,
    new_responsible_principal_id uuid,
    task_id uuid,
    draft_id uuid,
    draft_version integer,
    failure_reason text,
    occurred_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT thread_handoff_events_actor_check CHECK ((((actor_kind = 'system'::text) AND (actor_principal_id IS NULL)) OR ((actor_kind = ANY (ARRAY['human'::text, 'agent'::text])) AND (actor_principal_id IS NOT NULL)))),
    CONSTRAINT thread_handoff_events_failure_reason_check CHECK (((failure_reason IS NULL) OR ((btrim(failure_reason) <> ''::text) AND (octet_length(failure_reason) <= 2048)))),
    CONSTRAINT thread_handoff_events_operation_check CHECK ((operation = ANY (ARRAY['opened'::text, 'regenerated'::text, 'claimed'::text, 'released'::text, 'reassigned'::text, 'attributes_changed'::text, 'draft_requested'::text, 'draft_ready'::text, 'draft_failed'::text, 'resolved'::text, 'dismissed'::text]))),
    CONSTRAINT thread_handoff_events_priority_check CHECK ((((previous_priority IS NULL) OR (previous_priority = ANY (ARRAY['normal'::text, 'high'::text, 'urgent'::text]))) AND ((new_priority IS NULL) OR (new_priority = ANY (ARRAY['normal'::text, 'high'::text, 'urgent'::text]))))),
    CONSTRAINT thread_handoff_events_state_check CHECK ((((from_state IS NULL) OR (from_state = ANY (ARRAY['needs_instruction'::text, 'drafting'::text, 'draft_ready'::text, 'resolved'::text, 'dismissed'::text]))) AND (to_state = ANY (ARRAY['needs_instruction'::text, 'drafting'::text, 'draft_ready'::text, 'resolved'::text, 'dismissed'::text])))),
    CONSTRAINT thread_handoff_events_version_check CHECK (((from_version >= 0) AND (to_version = (from_version + 1))))
);
```

```sql
ADD CONSTRAINT thread_handoff_events_pkey PRIMARY KEY (id);
ADD CONSTRAINT thread_handoff_events_command_key UNIQUE (company_id, handoff_id, command_id);
ADD CONSTRAINT thread_handoff_events_company_fk FOREIGN KEY (company_id)
    REFERENCES public.companies(id) ON DELETE CASCADE;
```

Three deliberate choices, each with a reason a reviewer will ask for:

- **`operation` is the full vocabulary now, not just Phase 2's `opened`/`regenerated`.** A `CHECK`
  value added later means editing that constraint line in place (general file), which means
  recreating both databases again; and the state machine is fully designed here, so writing it once
  is cheaper and it documents the intent. Phases 3 and 4 only start *emitting* the rest.
- **No foreign key to `thread_handoffs`.** Deliberate, and the same choice
  `attention_source_events` makes (`:6157-6161` is its only FK, to `companies`): the audit trail
  must outlive the row it describes, and a `thread_handoffs` row is deleted whenever its thread or
  channel is. The company cascade is what stops it outliving the *tenant*.
- **`from_version >= 0` and `to_version = from_version + 1`.** Copied from
  `attention_source_events_version_check`. It means every version the row ever had has exactly one
  event, so "who moved this handoff from 4 to 5" is answerable and no transition can be written
  twice.

Immutability, modelled on `attention_source_events_are_immutable()` (`:46-56`) — a **new** function
rather than a second trigger on the same one, because the existing function's message names the
other table and a shared function would report the wrong one:

```sql
CREATE FUNCTION public.thread_handoff_events_are_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP = 'DELETE'
       AND NOT EXISTS (SELECT 1 FROM companies WHERE id = OLD.company_id) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'thread handoff events are immutable' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER thread_handoff_events_immutable BEFORE DELETE OR UPDATE
    ON public.thread_handoff_events FOR EACH ROW
    EXECUTE FUNCTION public.thread_handoff_events_are_immutable();
```

**No `notify_attention_changed` trigger in this phase.** The projection does not exist yet, so a
wake-up would name a source no reader can query. Phase 3 adds it, in the same edit that teaches the
feed about `thread_handoff`.

**No discretionary index.** Every Phase 2 read is either the unique key `(company_id, thread_id)` or
the unique `(company_id, generation)`. Phase 3's projection filters
`company_id = $1 AND channel_id = ANY($2)` exactly as the `manual_handoffs` branch at
`src/adapters/persistence/attention.rs:172-179` already does with no index of its own; matching it
keeps the accepted plan and honours the README's "indexes only from a representative query plan".

## 2.2 Eligibility as a pure decision

The whole hold rule is one function over already-loaded values. It goes in
`src/domain/entities/thread_handoff.rs` — the **domain**, beside the resolver Phase 1 put there, not
in `ingest/policy.rs` — because it is a statement about a message and a channel policy rather than
an ingress guard, and because Phase 3's re-open path and Phase 4's tests call it too.

```rust
/// Everything the hold rule looks at, named so a call site cannot swap two bools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutsideReplyFacts {
    /// The channel's *effective* reply-handling policy, already resolved through `COALESCE`.
    pub handling: ExternalReplyHandling,
    /// Whether this message continues a thread that already existed.
    pub continues_existing_thread: bool,
    /// Whether the sender is outside the company — `CompanyMembership::None`.
    pub sender_is_outside: bool,
    /// Whether this channel would have answered: `To`, or a `Cc` the body named.
    pub channel_answers: bool,
    /// Whether the message asked for an answer at all, after the `.quiet` fold.
    pub asks_for_an_answer: bool,
    /// Whether this copy closes an outreach the platform was awaiting.
    pub closes_outreach: bool,
}

/// Whether this channel's copy of an outside reply waits for the team.
pub const fn holds_outside_reply(facts: OutsideReplyFacts) -> bool { … }
```

The body is the conjunction of all six, with `handling.holds_outside_replies()` (Phase 1 §1.2) as
the first term and `!facts.closes_outreach` as the last. Write it as one `matches!`-free `&&` chain,
`const`, no `self`, no `async` — `src/AGENTS.md`, "pure decisions, separately".

Every line of the general file's behaviour table is a case of this function, and each row below is a
unit test with no database:

| Facts | Held? | Why |
|---|---|---|
| `ManualHandoff`, existing thread, outside sender, answers, asks | **yes** | the whole point |
| `Automatic`, everything else identical | no | today's behaviour is the default and stays it |
| new conversation (`continues_existing_thread == false`) | no | a first contact has nothing to hold *against*; the agent's greeting is the fastest useful answer and there is no history for a teammate to instruct from |
| teammate's reply (`sender_is_outside == false`) | no | the team is not "outside"; a teammate who wants a hold writes an internal note instead |
| `asks_for_an_answer == false` (`.quiet`, `[[quiet]]`) | no | an explicit `FileOnly` already produces no task; a handoff would ask the team to act on a message that asked nobody to |
| `closes_outreach == true` | no | the reply satisfies an outreach and resumes a waiting task; holding it would strand that task |
| `channel_answers == false` (passive Cc) | no | that channel was never going to answer, so there is nothing to hold |

Note what is **not** in `OutsideReplyFacts`: the thread's turn count, the sender's trust, the
attachment count, the body. A fact that does not appear cannot be accidentally depended on, and each
of those is already handled elsewhere in the pipeline.

## 2.3 Wiring the decision into the read-only phases

**Where the policy is read.** `prepare_channels`
(`src/application/use_cases/thread/ingest/routing.rs:496-612`) already resolves each candidate's
thread, its participants and its `answers` flag in one loop over `resolved.candidates`
(`:529-606`). The effective policy joins that loop: after `answers` is computed at `:594-597`, ask
the port for this channel's policy and evaluate `holds_outside_reply`.

`ThreadUseCases` gains `thread_handoff_policy: Arc<dyn ThreadHandoffPolicyPersistence>` beside
`task_persistence` (`src/application/use_cases/thread/mod.rs:403-438`). **Not** `Option<Arc<…>>`: a
deployment that forgot to wire it would answer every held customer immediately, which is exactly the
failure mode the `deliveries` field's own comment (`:420-423`) refuses for the same reason.

Three details that are easy to get wrong:

- **The sender's membership is already loaded.** `prepare_channels` builds `sender_context` at
  `:503-505` through `DirectoryCache::access_context`, and `PrincipalAccessContext.membership`
  (`src/domain/entities/participant.rs:264-267`) is the `CompanyMembership`. `sender_is_outside` is
  `!context.membership.is_team()` — do not re-query, and do not use `candidate.access.trusted`,
  which is a channel-ACL verdict and is `true` for an explicitly listed outsider.
- **`continues_existing_thread` is the `existing` binding**, i.e. `matches!(target,
  ThreadTarget::Existing(_))` inside the same loop iteration (`:536-565`). It must be read from the
  target, not from "the subject looks like a reply".
- **`closes_outreach` is `outreach.is_some()` *after* `authorize_thread_injection` folded in the
  correlated match** (`:546`), not the `resolved.outreach_by_channel` lookup at `:530-533`. Reading
  the earlier value would hold a reply that a correlated outreach had just authorised, and strand
  the waiting task — which is the bug the comment at `:543-545` describes for the pre-canonical
  path.

`asks_for_an_answer` is **not** available per channel inside that loop: the disposition is folded
message-wide in `CommitPlan::build` (`commit.rs:79-86` → `policy::fold_disposition`). So
`prepared.channels[i].hold` carries the five channel-shaped terms and `CommitPlan::build` applies
the sixth. Model it as a small enum rather than a bool so the reason survives to the logs and the
tests:

```rust
/// Why this channel's copy is, or is not, being held. Stated per channel in the read-only phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HoldDecision {
    Hold,
    /// The channel holds outside replies, but this message is not an eligible one.
    NotEligible,
    /// The channel answers automatically.
    Automatic,
}
```

**`CommitPlan::build`** (`commit.rs:63-147`) then changes in exactly two places:

1. the `targets` filter at `:103-111` gains one term —
   `channel.answers && channel.outreach.is_none() && !holds(channel, disposition)` — where `holds`
   is a private `fn` in this module taking the already-computed `HoldDecision` and the folded
   `disposition`. Keep it a free function so it is unit-testable without building a `CommitPlan`.
2. a new `holds: BoundedVec<InboundHold, MAX_THREAD_ASSOCIATIONS>` field, built in the same
   `iter().filter_map()` shape the `outreach_transitions` block already uses at `:112-127`.

`InboundHold` (`src/application/transport/ingress.rs`, beside `InboundOutreachTransition` at
`:643-646`) is identifiers only:

```rust
/// One channel's copy of an eligible outside reply that must wait for the team.
///
/// Named by channel, not by thread, for the same reason `InboundTaskTarget` (`:624-627`) is: the
/// thread may not exist until the commit writes it. The commit resolves the pair from the
/// association it just wrote, so a hold with no association is unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboundHold {
    pub channel_id: Uuid,
    /// The handoff row id to use when this thread has no handoff yet. A fresh UUID per commit;
    /// an existing row keeps its own id and only takes the new generation.
    pub handoff_id: Uuid,
    /// The generation this hold opens. New on every held message, including a second hold on a
    /// thread whose previous generation is still open.
    pub generation: Uuid,
}
```

`MAX_THREAD_ASSOCIATIONS` (20, `ingress.rs:54`) is the right bound and not a new one: at most one
handoff per thread and at most one thread per channel per message, so the holds one commit can write
are bounded by the associations it writes. `BoundedVec::parse` gives the same "refused at the
boundary, not at the `INSERT`" property `associations` has at `commit.rs:132`.

Add `holds` to `InboundCommitRequest` (`:660-682`) and to `CommitPlan::request()`
(`commit.rs:149-162`), and add `handoff_ids: Vec<Uuid>` to `InboundCommitOutcome` so
`assemble_result` and the tests can name what was opened without a second query.

**The disposition stays as it is.** Do not fold the hold into `MessageDisposition` — the general
file's "Do not fold the hold into `MessageDisposition`" section explains why in terms of
`inbound.rs:776` and `:792-803`, and this phase is where the temptation is strongest. A held message
takes the `Answer` disposition, audience `external_conversation` and entry kind `conversation`,
exactly like an unheld one. The only thing the hold changes is `task.targets`.

**One message, one handoff, several channels.** A message addressed to a held `support` and an
automatic `billing` produces: one canonical message, two associations, **one** task (billing's) and
**one** handoff (support's thread). If every answering channel is held, `targets` is empty and
`commit.rs:133`'s existing `(disposition.answers() && !targets.is_empty())` guard already makes
`task` `None` — no change needed there, which is worth a comment at the call site so the next reader
does not add a redundant check.

## 2.4 The commit: one statement group, no task

`commit_on` (`src/adapters/persistence/thread/inbound.rs:81-122`) gains one call, **after**
`create_task` at `:111` and before `create_deliveries`:

```rust
let handoff_ids = create_handoffs(tx, request, &threads, stored.id).await?;
```

Ordering matters and is not arbitrary: the handoff's `source_message_id` foreign key needs the
stored message (`:105`), its thread foreign key needs the resolved threads (`:101`), and putting it
after `create_task` keeps the statement order of the whole commit stable so a lock-ordering
deadlock cannot be introduced between two concurrent inbound commits on one thread.

`create_handoffs` mirrors `create_task`'s shape (`:623-672`): build the channel → thread map, refuse
a hold whose channel has no association with `AppError::Internal` (the same treatment
`task_targets` at `:678-698` gives a target with no association — it is a planner bug, not a user
error), then call into the handoff module once per hold.

`open_handoff_generation_on(tx, …)` lives in `src/adapters/persistence/thread_handoff.rs` beside
Phase 1's statements and is **two statements, no read**:

```sql
INSERT INTO thread_handoffs (
        id, company_id, channel_id, thread_id, generation, state, source_message_id
) VALUES ($1, $2, $3, $4, $5, 'needs_instruction', $6)
ON CONFLICT (company_id, thread_id) DO UPDATE
   SET generation = EXCLUDED.generation,
       state = 'needs_instruction',
       source_message_id = EXCLUDED.source_message_id,
       version = thread_handoffs.version + 1,
       generation_opened_at = CURRENT_TIMESTAMP,
       closed_at = NULL,
       updated_at = CURRENT_TIMESTAMP
RETURNING id, generation, version, (version = 1) AS opened
```

- **`ON CONFLICT … DO UPDATE` rather than select-then-branch.** The unique key does the locking, so
  two commits racing on one thread serialise on the row instead of on an advisory lock, and the
  loser sees the winner's version. A `SELECT … FOR UPDATE` first would need the row to exist.
- **`responsible_principal_id` is deliberately *not* reset.** A thread Bo claimed and then received
  a second reply on stays Bo's: responsibility is about who is looking after this conversation, and
  a new customer message is not a reason to un-assign it. `business_priority` and `business_due_at`
  are kept for the same reason. The general file's step 8 depends on this — the *generation* moves,
  the *responsibility* does not.
- **`state` returns to `needs_instruction` from any state**, including `drafting` and `draft_ready`.
  That is the fencing rule: a draft written for the previous message is no longer the answer to this
  thread, and Phase 4's `Send` on the stale generation must fail. It fails because the generation
  UUID moved, not because the state did — the state is what the team sees.
- **`RETURNING … (version = 1) AS opened`** tells the caller whether this was a first hold or a
  replacement, which is the only thing the event row needs to choose between `opened` and
  `regenerated`.

Then one event, in the same transaction:

```sql
INSERT INTO thread_handoff_events (
        company_id, handoff_id, generation, command_id, command_fingerprint, operation,
        actor_kind, from_state, to_state, from_version, to_version
) VALUES ($1, $2, $3, $4, $5, $6, 'system', $7, 'needs_instruction', $8, $9)
```

- `actor_kind` is `'system'` with a `NULL` principal. An inbound message is not a person acting, and
  `thread_handoff_events_actor_check` is what makes that unrepresentable as anything else.
- `command_id` is the **canonical message id**, not a fresh UUID. That is what makes the whole write
  idempotent against a redelivery that somehow reaches this code path twice: the unique key
  `(company_id, handoff_id, command_id)` refuses the second event. The fingerprint is the SHA-256 of
  `{handoff_id, generation, source_message_id}` via the same helper shape as
  `command_fingerprint` (`src/adapters/persistence/attention.rs:393-398`).
- In practice the redelivery never gets here: `commit_on` returns from `recognise_redelivery` at
  `:96-99` before any write, so the same inbound message delivered twice produces one message, one
  handoff, one generation and no task. Test it anyway (case 8 below) — the guarantee is worth an
  assertion, and `recognise_redelivery` is not the only path that could grow a second call.

**`effective_reply_handling_on` is not called here, and that is a change from Phase 1 §1.4.** Phase
1 expected the commit to re-read the effective policy inside the transaction. It must not, for a
reason that only becomes visible once the hold exists: the plan has *already* dropped the channel's
task target, so a commit that re-decided and disagreed would have to either write a handoff the
policy no longer wants or file a customer's reply with neither a task nor a handoff. Both are worse
than honouring the decision the read-only phase made, and the commit module's own contract
(`commit.rs:1-6`, "nothing here awaits… the plan is built from values the earlier phases already
loaded") says the decision belongs upstream. A policy flip during an in-flight ingest therefore
resolves as "whichever policy was in force when the message was routed", which is the answer a
support engineer would give anyway.

Leave the statement in place: it is written, prepared and covered by Phase 1's own test, it is one
`pub(crate) async fn`, and it is the read any later transaction-scoped caller will want — the
settings page and Phase 5's banner both answer "why is this channel behaving like that" through
`reply_handling_policy`, which is the pool-based twin. If a reviewer would rather delete it, that is
a one-line removal plus one test — but do not delete it *and* leave Phase 1's "done when" claiming
it exists.

## 2.5 Domain types

`src/domain/entities/thread_handoff.rs` (created in Phase 1) gains:

```rust
/// Where one generation of a thread handoff has got to.
///
/// Actionability is a property of the state, not a column: `needs_instruction` and `draft_ready`
/// are work somebody must do, `drafting` is visible progress, and the two terminal states are
/// neither. Phase 3's projection asks this rather than re-listing states in SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadHandoffState {
    NeedsInstruction,
    Drafting,
    DraftReady,
    Resolved,
    Dismissed,
}

impl ThreadHandoffState {
    pub const fn as_str(self) -> &'static str { … }
    /// Whether this state is work the team must act on. `drafting` is not.
    pub const fn is_actionable(self) -> bool {
        matches!(self, Self::NeedsInstruction | Self::DraftReady)
    }
    /// Whether a new outside reply may still change this generation.
    pub const fn is_open(self) -> bool {
        !matches!(self, Self::Resolved | Self::Dismissed)
    }
}
```

plus `FromStr` (error `format!("invalid thread handoff state '{value}'")`) and the entity:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadHandoff {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub generation: Uuid,
    pub state: ThreadHandoffState,
    pub source_message_id: CanonicalMessageId,
    pub responsible_principal_id: Option<PrincipalId>,
    pub priority: BusinessPriority,
    pub due_at: Option<DateTime<Utc>>,
    pub version: u64,
    pub generation_opened_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

`BusinessPriority` is reused from `entities::attention` (`src/domain/entities/attention.rs:15-61`) —
it already has `ALL`, `sort_rank`, `FromStr` and `Display`, and the projection in Phase 3 sorts on
the same ranks. Do not define a second priority enum.

`ThreadHandoff` is **not** added to any durable task payload. The general file's "durable payloads
carry identifiers only" rule and Phase 4's stale-generation fencing both depend on that; a worker
holding a snapshotted generation could not notice it had been replaced.

## 2.6 Documentation

`README.md` §3.7, one new paragraph next to the disposition rules:

- a channel may be set to `Manual handoff`, in which case an **outside reply to an existing thread**
  that the channel would have answered is filed and waits for the team;
- a held message is an ordinary customer message on the thread — same audience, same entry kind,
  same participants — and the only difference is that no agent task was created;
- new conversations, teammates' messages, quiet messages and outreach replies are never held;
- a second outside reply while a hold is open replaces the generation, and anything written for the
  previous one can no longer be sent.

Update the doc comment on `InboundCommitRequest` (`ingress.rs:654-658`) — "a message visible without
its task is a message no agent will ever answer" is now true only for an unheld message, and the
holds are what make the held case legible instead of looking like a lost task.

---

## Tests

**Unit, no database, no mocks** — `src/domain/entities/thread_handoff.rs`:

1. `holds_outside_reply` for all seven rows of §2.2's table, each as its own named test rather than
   a loop, so a failure names the rule that broke.
2. Flipping *only* `handling` from `ManualHandoff` to `Automatic` on the held case flips the result,
   and flipping any *one* of the other five terms also flips it — a small matrix that proves the
   conjunction has no redundant term. A term that can be removed without failing this test is a term
   that should not be in `OutsideReplyFacts`.
3. `ThreadHandoffState`: `as_str`/`FromStr` round-trip all five; `is_actionable` is true for exactly
   `needs_instruction` and `draft_ready`; `is_open` is false for exactly the two terminal states;
   `"open"`, `"handoff"` and `""` are errors.

**Pure, in `ingest/commit.rs`** — the `holds` helper and the `targets` filter, built from a
hand-made `Vec<PreparedChannel>` with no I/O:

4. A message to a held `support` and an automatic `billing`: `targets` is `[billing]` and `holds` is
   `[support]`, in that order.
5. Every answering channel held: `targets` is empty, `task` is `None`, `holds` has one entry per
   channel.
6. `disposition` folded to `FileOnly` (a `.quiet` address anywhere): `holds` is **empty** and `task`
   is `None`. This is the case where holding would be wrong and the two independent gates could
   easily both fire.
7. A held channel that also closes an outreach: not held, and still excluded from `targets` by the
   existing `outreach.is_none()` term.

**Application-layer ingest** (`src/application/use_cases/thread/tests.rs`, beside the existing Cc
ingest cases): each of the general file's behaviour-table rows driven end to end through the
in-memory committer, asserting the number of tasks and the number of holds in the captured request.

**Persistence** (`src/adapters/persistence/thread_handoff_tests.rs`), each scoped to a company it
creates — the general file's DB-test convention applies to every assertion:

8. **Held reply, first generation.** One `thread_handoffs` row for `(company_id, thread_id)` in
   `needs_instruction` with `version = 1`, `responsible_principal_id IS NULL`,
   `business_priority = 'normal'`, `closed_at IS NULL`, and its `source_message_id` equal to the
   committed canonical id. Exactly one `thread_handoff_events` row for that `(company_id,
   handoff_id)` with `operation = 'opened'`, `actor_kind = 'system'`,
   `actor_principal_id IS NULL`, `from_version = 0`, `to_version = 1`.
9. **No task.** `SELECT count(*) FROM background_tasks WHERE company_id = $1` is 0 for the held
   commit's company, and `InboundCommitOutcome.task_id` is `None`. Scoped to the test's own company;
   never a whole-table count.
10. **Second reply replaces the generation.** A second held commit on the same thread leaves exactly
    one `thread_handoffs` row, with a *different* `generation`, `version = 2`, `state` back to
    `needs_instruction`, and `source_message_id` equal to the *second* message. Two events, the
    second with `operation = 'regenerated'`, `from_version = 1`, `to_version = 2`.
11. **Responsibility and attributes survive a replacement.** Set
    `responsible_principal_id`/`business_priority` directly, commit a second held reply, and assert
    both are unchanged while the generation moved.
12. **A replacement from a non-terminal state.** Set `state = 'drafting'`, hold again, and assert the
    row is back in `needs_instruction` with a new generation. Then set `state = 'resolved'` with a
    `closed_at`, hold again, and assert `closed_at` is `NULL` again — the `closure_check` would
    reject the row otherwise, so this is the test that proves the `DO UPDATE` clears it.
13. **Redelivery.** The same inbound message committed twice: one message, one handoff, one
    generation, one event, no task. `CommitDisposition::Duplicate` on the second.
14. **Mixed message.** One commit with a held `support` and an automatic `billing`: one canonical
    message, two `thread_messages` rows, one `background_tasks` row whose target is billing's
    channel, one `thread_handoffs` row on support's thread.
15. **Two concurrent commits on one thread.** Two transactions each opening a generation on the same
    thread: both succeed, the row ends at `version = 2` with one of the two generations, and there
    are two events — the unique key serialised them rather than raising a duplicate-key error out of
    an ingest.
16. **Immutability.** `UPDATE thread_handoff_events SET operation = 'claimed'` and
    `DELETE FROM thread_handoff_events` both raise; the error is the new message, not
    `attention_source_events`'s.
17. **Constraints.** Direct writes rejected: an unknown `state`; `version = 0`;
    `state = 'resolved'` with `closed_at IS NULL`; `state = 'needs_instruction'` with a
    `closed_at`; `actor_kind = 'human'` with a `NULL` principal; `actor_kind = 'system'` with one; a
    `failure_reason` of 2049 bytes; `to_version <> from_version + 1`; a second row with the same
    `(company_id, handoff_id, command_id)`; a second handoff for one `(company_id, thread_id)`; two
    handoffs sharing a `generation`.
18. **Tenant scoping.** A handoff whose `thread_id` belongs to another company is rejected by
    `thread_handoffs_thread_fk`; deleting the thread cascades the handoff away and leaves its events
    (no FK); deleting the *company* removes the events too.
19. **A hold naming a channel with no association** is `AppError::Internal` and the whole commit
    rolls back — no message, no thread, no mapping. Model it on
    `a_commit_that_fails_at_the_task_leaves_no_thread_message_or_mapping`
    (`src/adapters/persistence/thread/inbound_tests.rs:887-936`), which already proves the rollback
    property for the task equivalent.

**Existing assertions.** Only tests that assert "an outside reply to an existing thread creates a
task" on a channel this phase puts on `ManualHandoff` may change, and no fixture is switched to
`ManualHandoff` casually: the migration default is `Automatic`, so a correctly written existing test
cannot notice this phase at all. If one does, say which and why in the PR.

## Done when

- [ ] `thread_handoffs` and `thread_handoff_events` exist in the init migration in `pg_dump` order,
      with the closure/state/version/actor checks, the three unique keys, the composite foreign keys,
      the reused cleanup-version trigger and the new immutability trigger; both databases are
      recreated and `.sqlx/` is regenerated and committed.
- [ ] `holds_outside_reply` is a `const fn` over `OutsideReplyFacts` in the domain, with the seven
      table rows and the one-term-at-a-time matrix as unit tests.
- [ ] `PreparedChannel.hold` is set in `prepare_channels` from the *folded* outreach match and the
      sender's `CompanyMembership`, and `CommitPlan::build` applies the disposition term.
- [ ] An eligible held reply commits a canonical `external_conversation` / `conversation` message,
      its associations, its provider mapping and one `thread_handoffs` row — and no
      `background_tasks` row — in one transaction.
- [ ] A second held reply takes a new generation, keeps responsibility and attributes, and leaves
      exactly one row per thread.
- [ ] `MessageDisposition` is unchanged: `grep` for `FileOnly` shows no new occurrence in
      `inbound.rs` or `policy.rs`.
- [ ] `InboundTaskPayloadV1` is unchanged: `grep` for `handoff` in
      `src/application/transport/` finds only `InboundHold` and `InboundCommitRequest.holds`.
- [ ] Cases 1-19 pass. README §3.7 and the `InboundCommitRequest` doc describe the hold.
- [ ] `cargo fmt --check`, `SQLX_OFFLINE=true cargo build --all-targets`, `cargo test` and
      `cargo clippy --all-targets -- -D warnings` are green.
