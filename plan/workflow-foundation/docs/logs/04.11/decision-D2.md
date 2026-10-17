# 04.11-D2 — Messaging transaction, receipt and recovery ownership (revision of D1)

No backward compatibility: reset databases for incompatible schemas/payloads; no
backfills, dual paths, legacy decoders or staged rollout.

Status: proposed architecture gate, revision of 04.11-D1 after 04.11-R002 (CHANGES
REQUIRED). Not approved. **D2 is self-contained and supersedes D1 in full**; D1
(`decision-D1.md`, SHA256 `ebb523df…ad83`) is retained unchanged as history. Where a
D2 section keeps D1's decision it says "as D1 §n". Original requirements:
04-actions-http-and-delivery.md ("Action contract", "Execution and uncertainty" 1–7,
"Provider-neutral messaging", "Acceptance"), EXPANSION.md 04.11 and cross-point
preservation, accepted 04.1–04.10 contracts (04.10 accepted at 2be42e3e…bc4c, R005).
User decision: recipients are users and channels; groups rejected. One cohesive batch;
phase05+ and production agent/ingress composition are out of scope.

## 0. Finding dispositions

| Finding | Severity | Disposition | Resolved in |
| --- | --- | --- | --- |
| 04.11-F001 | medium | Adopted. Message kinds join `marker_owned_effect_kind`. Every shared predicate's treatment of the new `message` marker is stated. There is a full outcome table for step and direct tool. Unknown deliveries are marked as needing reconciliation and parked; the evidence command is a justified, documented deferral. | §5 (table §5.1, predicates §5.2, deferral §5.3), tests §8.2 |
| 04.11-F002 | medium | Adopted. At most 128 message markers per execution, refused before any marker/publication write with the distinct terminal `message.action_limit`. Consumption is one action per transaction, ≤20 deliveries, in pages of 8 actions with a persisted cursor and backoff-to-stalled poison handling. | §2.2, §4.2, tests §8.2 |
| 04.11-F003 | low | Adopted. Subject is bounded at 998 bytes as `CanonicalContent::parse` enforces (code-point schema plus a byte-accurate runtime and publication refusal). Destinations are capped at 20, justified against `MAX_THREAD_ASSOCIATIONS`. Sanitization order is stated. | §1.1, tests §8.2 |
| 04.11-F004 (optional) | low | Adopted (low cost). The event-production invariant, lock modes and all producing transitions are stated. The reaper takes parent locks first. A delayed lower-sequence sibling test is added. | §4.1, tests §8.2 |
| 04.11-F005 | medium | Adopted. One canonical message per destination, each with its own RFC identity. Full classification table. `message.send` always opens a new Standalone thread. `message.reply` uses the source's thread and headers. Interplay with existing thread consumers is defined and tested. | §1.2–§1.5, tests §8.2 |
| 04.11-F006 | low | Adopted. Exact focused commands per area, the required test list per finding, a CI-parity final gate (network isolation wrapper, `git diff --check`, stack-budget, lease-budget), the disk plan and reporting of unavailable gates. | §8 |
| 04.11-F007 | low | Adopted. The fence expiry is captured by a database trigger from the live job before the close, and re-checked at COMMIT. A receipt guard derives the result from authoritative delivery states. Action-state transitions are guarded. SQL-only negative tests. | §2.5, §4.3, tests §8.2 |
| 04.11-F008 | low | Adopted. The effect-wait deadline equals the run deadline exactly. There is an explicit effect branch in `resume_on`, effect-specific poll eligibility, and stated expiry/cancel effects on deliveries. A no-hot-repoll test. | §4.4, §6, tests §8.2 |

## 1. Inputs, bounds, routing and classification

### 1.1 Input contract and bounds (F003)

- `message.send` input: `content {subject?, body}`, plus
  `destinations[{kind: user|channel, id: uuid}]`. `group` is removed from the
  catalogue enum and rejected at runtime with `message.rejected`.
- `message.reply` input: `{source_message: uuid, content}`.
- No attachments, no arbitrary addresses, and no provider-specific input. Destination
  ids are UUIDs parsed at runtime. A duplicate `(kind,id)` is refused at runtime, and
  the schema also uses `uniqueItems`.
- **Destination bound** `MAX_MESSAGE_DESTINATIONS = 20`. It is a new registry constant,
  enforced as schema `minItems 1 / maxItems 20` and again at runtime before any write.
  - It is a distinct decision from `MAX_SELECTIONS` (128). Its value matches the
    established per-commit fan-out bound `MAX_THREAD_ASSOCIATIONS = 20`
    (`transport/ingress.rs:55`).
  - One send writes at most 20 threads, 20 canonical messages and 20 deliveries in one
    transaction.
  - The receipt consumer locks at most 20 delivery parents per transaction (§4.2).
- **Subject bound** = the canonical owner `MAX_SUBJECT_BYTES = 998`
  (`transport/ingress.rs:39`). This is stricter than the SQL limit of 2048 bytes.
  - JSON Schema counts code points, so the schema sets `maxLength: 998` as a coarse
    upper bound.
  - The authoritative byte check is `CanonicalContent::parse`, run in the handler
    before the intent/marker transaction. A violation is a pre-marker
    `message.rejected` (Terminal, safe).
  - A literal subject is also byte-checked at publication through registry literal
    access, so a published literal can never fail at runtime.
- **Body**: schema `minLength 1, maxLength MAX_TEXT (16 384 code points ≤ 65 536 bytes)`.
  This is within `MAX_BODY_BYTES = 1 000 000`.
- **Sanitization and digest order**:
  - The frozen arguments and their digest are taken from the bound input, before
    sanitization, as D1 has it.
  - `CanonicalContent::parse` then applies `sanitize_string`, which only strips
    characters, so byte length never grows. The bound is applied after that.
  - A body that is empty after sanitization is refused with `message.rejected`.
  - Sanitization is deterministic, so a replay of the same invocation renders
    identical canonical content. Canonical content is computed from the frozen
    arguments, never from a re-read source.

### 1.2 One canonical message per destination (F005)

Each destination gets its own thread, its own canonical message and, for email, its
own delivery. One canonical message therefore carries exactly one thread association
and at most one RFC Message-ID. That matches `email_message_metadata` PRIMARY KEY
(message_id) and the outreach precedent (`services/outreach_tool.rs:370-446`). D1's
multi-association single message is withdrawn.

| Destination | Thread | Delivery |
| --- | --- | --- |
| `channel` (in-app) | A **new** thread in that company channel. | None. Accepted when the transaction commits. |
| `user` (email) | A **new** thread in the run's explicit channel (`workflow_runs.channel_id`). A run without a channel, or whose channel has no live email binding, is refused pre-marker with `message.rejected`. A first or default channel is never chosen. | One email delivery from that channel's address to the member's current account email, frozen at publication. |
| `message.reply` | The source's own association thread (§1.4). | Email (Received threading) or none (in-app note), per §1.4. |

Delivery identity:

- purpose: new value `workflow`, added to the `message_deliveries_purpose_check`;
- `source_key = invocation:<invocation uuid>`;
- the existing `delivery_key(purpose, source_key, destination)`.

The idempotency key and therefore the Message-ID (`EmailRenderer::message_id_for`, a
pure function of `email:<key>`) are stable across retries and restarts. They are
distinct per recipient, and never derived from an attempt or a fresh UUID. The
composer's predicted provider key is recorded as that message's
`MessageCorrelation::Email` metadata in the publication transaction, as outreach does.
A reply that quotes any one recipient's copy therefore resolves (through the existing
`external_messages`/`email_message_metadata` lookup) to exactly that recipient's
canonical message and thread.

Result:

- The step/receipt result schema becomes
  `{status: accepted|failed, messages: [{destination: {kind, id}, message_id, status: accepted|failed}]}`,
  in destination order.
- `message.reply` has exactly one element, with `destination {kind: "reply", id: source}`.
- The registry's `message_result()` is replaced. There is no compatibility shape.

### 1.3 Classification of written messages (F005)

| Case | Author | Direction | Role | Audience | Entry kind | Correlation / threading |
| --- | --- | --- | --- | --- | --- | --- |
| `send` → channel | `Platform` | outbound | system | `internal_only` | `conversation` (it opens its own thread) | Internal |
| `send` → user | `Platform` | outbound | system | `external_conversation` | `conversation` | Email; **Standalone** (no In-Reply-To or References). Participants are Sender = channel address and To = recipient. |
| `reply` → external email source | `Platform` | outbound | system | `external_conversation` | `conversation` | Email; **Received** {in_reply_to = the source's RFC id, references = the source's recorded references}. Participants are Sender = the source thread's channel address and To = the source's author address. |
| `reply` → internal source | `Platform` | outbound | system | `internal_only` | `note` | Internal. Never email. |

- Role `system` distinguishes workflow automation from agents (phase 06 agents author
  as `Agent`).
- `message_deliveries_message_audience_check` already makes it impossible in the
  database to deliver an `internal_only` message externally.
- The relay trace follows the existing precedent for each threading mode, through the
  existing helper (no new hop rule):
  - Standalone user email is like a scheduled delivery: run channel, hop 0.
  - An external reply continues the source's recorded relay trace, as an agent reply
    does.

### 1.4 `message.reply` source resolution

- The source is the exact canonical UUID. It is looked up tenant-scoped in SQL, never
  by "latest message".
- Association selection:
  - If the run has a thread, the source must be associated with that thread.
  - Otherwise, if the run has a channel, the source must have exactly one association
    in that channel.
  - Otherwise it must have exactly one association in the company.
  - Zero or several matches are refused with `message.rejected`.
- That association's thread and channel are the reply's thread and sender channel.
- Supported sources:
  - An inbound email with a recorded RFC Message-ID gives an email reply to its
    author. The address is the one the existing received-mail reply composer derives;
    implementation reuses that helper and cites it in the batch review (one decision,
    one place).
  - An internal or in-app message gives an in-app `note` reply.
- Refused with `message.rejected`:
  - an outbound source (our own message);
  - a non-email external transport (Slack);
  - an `external_conversation` source without an RFC id.
- The reply headers are the source's own, never `latest_thread_rfc_message_id`. The
  recipient is frozen. A retry never redirects to a newly changed address.

### 1.5 Interplay with existing thread consumers (F005)

- **`message.send` never writes into an existing thread**, so it cannot affect a
  customer thread's
  - `find_outbound_reply_after` (`persistence/thread/mod.rs:637-668`, used by
    `task_worker.rs:990-1010` and `use_cases/thread/dispatch.rs:416-420`), or
  - `latest_thread_rfc_message_id` (`views.rs:587-608`, through
    `use_cases/thread/mod.rs:1025-1041`).

  In its own new thread it is the first entry. A later inbound reply followed by an
  agent answer is neither suppressed (the guard looks only after the trigger), and the
  fallback threads onto the newer inbound id.
- **An external `message.reply`** is a `conversation` entry after the source. When a
  legacy channel-agent task answers that same trigger, the guard treats the workflow
  reply as the answer and skips a second answer. **This is intended**: an explicit
  reply to the exact source answers that turn.

  Later answers without an explicit reply target thread onto the reply's
  Message-ID, which is correct email threading. The predicted id is recorded at
  enqueue, as outreach does, so a delivery that later fails leaves the same
  pre-existing fallback caveat as outreach. That is documented, not changed.
- **An internal `message.reply`** is a `note` with no email metadata. It neither
  suppresses an answer nor changes In-Reply-To.

### 1.6 Authority (as D1 §1, unchanged)

- Explicit trusted built-in ToolSnapshots for message.send and message.reply, selected
  through a message PublicationDirectory, with fixed schemas, side-effect/recovery
  metadata and no connection slot. Publication and binding verify them against the
  built-ins; `supported_publication` stops routing slotless message tools to HTTP.
- Runtime target is `ActionTarget::Local {resource: company_id, resource_kind: messaging}`,
  under the frozen capability ceiling.
- Current company owner/admin authority uses `authorize_company`, read from durable
  owners. Forged policies and snapshots are rejected.
- Frozen stronger approval returns ApprovalRequired without any write (phase 05
  settles approval).
- No messaging grant registry or fabricated connection.
- Recipients:
  - A user destination must be a current member with a principal, locked `FOR SHARE`
    through commit as `authorize_company` does.
  - Its history references the preserved principal row `(company_id, principal_id)`,
    never `company_members`, which `remove_member` deletes or demotes.
  - A channel destination must be an existing channel of the run's company, locked
    `FOR KEY SHARE` in UUID order.

## 2. One logical invocation, admission and atomic publication

### 2.1 Shared service and lock order (as D1 §2)

- `ActionService::prepare_step/prepare_tool` (immutable invocation, digest, model-call
  mapping) and a purpose-specific `MessageDispatch` port reuse
  `PostgresActionDispatch::authorize_on`. Its lock order is company/actor, then fence
  (job/attempt), then run and current owners, then the immutable intent.
- Then: channels and users (§1.6) in UUID order, then new threads, messages and
  deliveries.
- No network I/O inside a transaction.
- Rendering uses `DeliveryComposer` with the injected `TransportRenderers`. Provider
  code never sits in the handler.
- One service backs the workflow handler (`message.send`/`message.reply` step kinds)
  and the direct-tool entry point.
- Effect kind: a new marker/receipt kind `message`, used for every message invocation,
  in-app only included. This replaces D1's split between `local` for in-app and
  `delivery` for email, so the admission bound, guards and predicates key on one kind.

### 2.2 Admission bound (F002)

Every message publication transaction runs these steps in this order:

1. `authorize_on` locks: the attempt row `FOR UPDATE` serializes every dispatch of the
   same execution, including concurrent direct-tool calls under one fence.
2. A replay check. An existing marker for this invocation returns its committed state
   without writing.
3. A bounded probe:
   `SELECT count(*) FROM (SELECT 1 FROM workflow_action_dispatches WHERE company_id=$1 AND execution_id=$2 LIMIT 128) b`.
   If it returns 128, the action is refused. **Nothing has been written at this point**
   (the marker insert is the first write).
4. Writes.

- Refusal is the distinct terminal outcome `message.action_limit`: class Terminal,
  handler safety SafeToRetry, no marker. It is never "unknown".
- Database backstop: a BEFORE INSERT trigger on `workflow_action_dispatches` for
  `effect_kind='message'` repeats the same bounded count and raises
  `ERRCODE='54000'` with message `workflow message action limit`. Rust maps that error
  to `message.action_limit`. Because the marker is the transaction's first write, the
  backstop also refuses before any publication write.
- `MAX_MESSAGE_ACTIONS_PER_EXECUTION = 128` is defined once in Rust, and the SQL
  comment names it.
- **Relation to the proof ceiling.** After a message admission the execution holds at
  most 128 markers. So the 128/129 scans in `workflow_action_retry_safe`
  (`20260930180000:191-194`) and `workflow_action_not_applied_available`
  (`20261001090000:12-16`) never truncate because of messaging.
  - Remote (HTTP/MCP) admission is unchanged accepted 04.9/04.10 behavior. It still
    fails closed through those scans if it ever exceeds 128. This is recorded as a
    pre-existing observation for root, not widened here.
  - The unsupported comment in `action_uncertainty::record` ("per-execution action
    bounds limit this set") is corrected. The real bounds are: message markers ≤128
    per execution, runs bound by frozen `max_steps`, and the INSERT…SELECT is
    set-based.
- Boundary tests: 127 existing markers lets the 128th admit; 128 existing refuses the
  129th, through both the service path and SQL-only.

### 2.3 Publication transaction writes

All of these commit together:

1. the `message` marker;
2. for each destination: a new thread, the canonical message (plus email metadata for
   email) and, for email, the frozen delivery and its part;
3. `workflow_message_actions` (one row);
4. `workflow_message_destinations` (one row per destination);
5. for an all-in-app send: the action receipt `accepted`, written atomically;
6. otherwise, for step origin: `workflow_waits(reason='effect')` plus the run moved to
   `waiting/effect`, plus the exact fenced attempt and job closed by a new
   `completion_job` owner `EffectParked`. The generic `Pure` owner is not reused. The
   wait is not created for direct-tool origin.

There is no second delivery queue or execution lease. In-app-only publication returns
`{status: accepted, …}` only after commit, then normal shared completion advances the
step. That is the existing `complete_io` path, because the fence is still live.

### 2.4 Schema (new migration after `20261017110000`; reset the database if needed)

- `workflow_action_dispatches.effect_kind` and `workflow_action_receipts`: add
  `'message'`. The receipt-kind CHECK gains `(effect_kind='message' AND remote_entry_id IS NULL)`.
- `workflow_message_actions`:
  - PK `(company_id, invocation_id)`. Columns: run_id, execution_id, argument_digest,
    dispatch_id, `origin step|model_tool`, `wait_id` (NOT NULL iff the step parked;
    the id is minted before the action insert and backed by a DEFERRABLE INITIALLY
    DEFERRED FK to `workflow_waits`, which is inserted later in the same transaction),
    `fence_expires_at` (DB-captured, §2.5).
  - Mutable columns: `state pending|uncertain|accepted|failed|stalled`,
    `consumed_through bigint`, `consume_failures smallint ≤5`, `consume_after`,
    `updated_at`.
  - Composite FK `(company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, effect_kind)`
    references the marker.
  - A trigger makes every other column immutable and validates transitions (§4.3).
- `workflow_message_destinations` (append-only):
  - PK `(company_id, invocation_id, ordinal 0..19)`.
  - Columns: `kind channel|user|reply`, `destination_id`, `thread_id`, `message_id`,
    nullable `delivery_id`, nullable recipient `principal_id`.
  - Composite tenant FKs: to the action;
    `(company_id, message_id) → messages`;
    `(company_id, delivery_id, message_id) → message_deliveries(company_id, id, message_id)`
    (new UNIQUE on deliveries);
    `(company_id, thread_id) → threads`;
    `(company_id, principal_id) → principals`.
  - UNIQUE `(company_id, delivery_id)`.
  - CHECK: `kind='channel' ⇒ delivery_id IS NULL`, `kind='user' ⇒ delivery_id and principal NOT NULL`.
- `workflow_message_delivery_events` (append-only):
  - `seq bigint` identity, `(company_id, invocation_id, ordinal)` FK to the
    destination row only (never to the action row, see §4.1), `delivery_id`,
    `status delivered|dead_letter|outcome_unknown`, `failure_class`, `observed_at`.
- `message_deliveries` cancellation reasons: add `workflow_cancelled`,
  `workflow_expired`, `workflow_revoked`, `recipient_revoked`.
- `workflow_waits.failure_code` is unchanged. An effect wait completes (§4.4).
- `workflow_action_reconciliations.effect_kind` CHECK becomes `IN ('remote','message')` (§5.2).
- Indexes:
  - events `(company_id, invocation_id, seq)`;
  - actions `(state, consume_after)` partial on `state='pending'`;
  - destinations `(company_id, delivery_id)`.

### 2.5 Database-derived proofs (F007)

- **Fence capture.** A BEFORE INSERT trigger on `workflow_message_actions`:
  - loads the marker's job and attempt;
  - requires `job.status='processing'`, a matching worker/generation/attempt,
    `attempt.status='processing'` and `job.lock_expires_at > clock_timestamp()`;
  - sets `NEW.fence_expires_at := job.lock_expires_at`.

  A writer-supplied value that is not NULL and differs raises. Inserting after the job
  is closed raises, so the order "action row before close" is enforced. The job and
  attempt stay locked `FOR UPDATE` by `authorize_on` until COMMIT, so only this
  transaction can have closed them afterwards.
- **Deferred commit guard.** `workflow_action_dispatch_commit_guard` keeps its local
  and remote branches byte-for-byte. A new `message` branch requires:
  1. the action row and ≥1 destination row for the marker. Destination messages and
     deliveries are verified by the composite FKs.
  2. If every destination is in-app: the atomic receipt plus the existing live-fence
     predicate.
  3. Otherwise, for origin `model_tool`: the existing live-fence predicate.
  4. Otherwise, for origin `step`:
     - run `waiting/effect` with `run.deadline > clock_timestamp()`;
     - the wait `id=action.wait_id`, `reason='effect'`, `state='waiting'`,
       `deadline = run.deadline`;
     - the job and attempt (exact fence ids) no longer processing;
     - `action.fence_expires_at > clock_timestamp()`.

  A delayed COMMIT past either bound rolls back the marker, messages, deliveries, wait
  and park. Merely writing a closed job is not proof.
- **Effect-wait insert trigger.** `reason='effect'` requires `deadline = run.deadline`,
  NULL event_name, correlation and notification_intent, and a matching action row.
- The receipt guard and action-state guard are in §4.3.

### 2.6 Ambiguous COMMIT, competing claimants, replay (as D1 §2, made explicit)

- An ambiguous COMMIT is settled by a WriteBound read under the original lock order.
  - Committed: return the same message ids and wait (Suspended for an email step,
    Completed for in-app).
  - Definitely absent: retry is permitted.
  - Unresolved: the handler reports `message.interrupted`. `retire_on` then runs under
    the fence and the shared predicate decides from the actual state (§5.1 row 6).
  - A lost acknowledgement never mints a new logical effect.
- Competing claimants create one canonical message set and one delivery set. The
  attempt lock serializes them; the loser observes the marker in the replay check.
- A saved receipt replays without rendering or sending, after current authorization
  when a result is returned to a caller. Background evidence collection needs no
  caller authorization.

## 3. Handler suspension and direct-tool boundary (as D1 §3, clarified)

- The handler outcome is `Completed(WorkflowHandlerResult) | Suspended {wait, invocation}`.
  - The existing Result alias stays for ordinary handlers. Conversion happens at the
    supervisor boundary.
  - `supervise_io` calls a required, defaultless `WorkflowLeases::confirm_suspension(fence, wait)`.
    It is read-only and proves that the fence's attempt was closed by `EffectParked`
    and that the wait belongs to the action, in any wait state (an early receipt may
    already have resumed it).
  - It never calls `complete_io`. An unconfirmed suspension is treated as lease loss
    (no completion).
  - Test doubles state their behavior explicitly.
  - A crash after the park is recovered by the durable wait and poller.
- Direct message tools (port seam; no production agent host until phases 06/08)
  return a `MessageObservation`:
  - `Committed(receipt accepted|failed)`
  - `Pending {invocation, messages}`
  - `Uncertain {invocation, messages}`
  - `ApprovalRequired`
  - `Refused(ActionLimit)`
  - or an error for rejection/interruption.
- Pending never claims provider acceptance or produces successful model output.
- The action row is the durable owner. The receipt consumer (§4) collects outcomes
  regardless of whether the caller is still alive.
- Re-entry with the same model call observes the same invocation and messages.
  Changed arguments are refused.
- A direct tool never completes or parks the enclosing agent execution. That
  execution's interruption semantics stay those of its own kind (§5.1 note). The
  phase 06 saved-call owner must suspend on Pending and resume from the receipt.

## 4. Receipt events, consumption, aggregation and progression

### 4.1 Event production invariant (F004)

- Events are written **only** by an AFTER UPDATE OF status trigger on
  `message_deliveries`, for linked rows, when `OLD.status IS DISTINCT FROM NEW.status`
  and `NEW.status IN (delivered, dead_letter, outcome_unknown)`.
- The UPDATE holds the parent's row lock before `nextval`.
- Every producing transition is a parent status UPDATE:

  | Transition | Function |
  | --- | --- |
  | delivered / retry / dead-letter / unknown | `complete_part` → `aggregate_parent_on` |
  | retry / dead-letter | `fail_delivery` |
  | cancellation settle | `settle_cancelled_on` |
  | unsent supersession | `cancel_task_deliveries_on` and the new workflow cancellation (§6) |
  | begin_part refusal | via the same cancellation helper |
  | lease expiry | the reaper (`queue.rs:566-600`) |
  | stranded cancellation settle | `settle_stranded_cancellations_on` |

- **The reaper is changed to lock its stranded parents `ORDER BY id FOR UPDATE SKIP LOCKED`
  before updating parts.** Today it updates parts without a parent lock and parents in
  scan order. After the change, every producer holds the parent lock before any
  transition, and the consumer's id-ordered locks cannot deadlock with the reaper.
- Event mapping:
  - delivered → accepted;
  - dead_letter (including superseded/cancelled) → failed (known terminal, never
    resent);
  - outcome_unknown → unknown.
- `delivered`, `dead_letter` and `outcome_unknown` have no exit path under D2, so each
  delivery yields at most one event. Email renders exactly one part
  (`protocols/email/egress.rs:386`).
- A duplicate unchanged callback changes no status and adds no event.
- Lock modes:
  - Producers take the parent `FOR UPDATE` and then `KEY SHARE` on the immutable
    destination row (the FK check).
  - The consumer takes run `FOR UPDATE`, the action row `FOR NO KEY UPDATE`, and parents
    `FOR SHARE` in id order. It reads destination rows without locking them.
  - Callback producers (`complete_part`, `fail_delivery`, `settle_*`, the reaper) never
    touch run or action rows. The two workflow producers (workflow cancellation and
    workflow `begin_part` refusal, §6) take run before parent, the consumer's own
    order. No producer locks an action row, so there is no cycle.

### 4.2 Consumer transaction and bounds (F002)

- Discovery is a new shared constant `MESSAGE_RECEIPT_CANDIDATES_SQL`, used by
  `polling::poll_work`, `fair_polling::discover`/`company_query` and diagnostics
  (`inspection`). It selects executions having an action with `state='pending'`,
  `consume_after <= now`, and an event `seq > consumed_through`, with company-fair
  ordering under the existing poll limit.
- This is discovery of existing work, not a queue or lease. It finds actions on live,
  terminal and cancelled runs alike.
- Work kind `PollWork::MessageReceipt` processes at most `MESSAGE_RECEIPT_PAGE = 8`
  actions of that execution, ordered by `(consume_after, invocation_id)`, **one action
  per transaction**. Each transaction locks ≤20 parents and reads ≤41 event rows
  (`LIMIT 2·20+1`; exceeding it stalls the action as a defect).
- Transaction steps:
  1. lock run, then action, then parents (§4.1);
  2. in a new statement, read the maximum seq and the authoritative parent statuses;
  3. aggregate (§4.3);
  4. advance `consumed_through`;
  5. write the state, and the receipt when the action is terminal.
- Remaining work stays discoverable because unconsumed events keep the predicate true.
- Poison handling:
  - A consumer transaction error rolls back.
  - A separate short transaction then increments `consume_failures` and sets
    `consume_after = now + min(2^n s, 300 s)`.
  - The 5th failure sets `state='stalled'`, which is observable, keeps every fact,
    has no receipt and appears in the run audit.
  - A fixed failing set therefore leaves the candidate set. Two consecutive poll
    iterations without advancing time do not reclaim it.

### 4.3 Aggregation, receipt and state guards (F007)

- Per destination:
  - in-app (`delivery_id IS NULL`) → accepted;
  - delivered → accepted;
  - dead_letter → failed;
  - outcome_unknown → unknown;
  - pending/sending/retryable → pending.
- Action rules:
  - any pending → stays `pending`;
  - all terminal with any unknown → `uncertain` (no receipt; unknown dominates
    failed);
  - all accepted → receipt `accepted`;
  - all known terminal with ≥1 failed → receipt `failed`, accepted siblings retained
    in the result.
- **Receipt guard.** A BEFORE INSERT trigger on `workflow_action_receipts` for
  `effect_kind='message'` recomputes the result from destination rows (by ordinal) and
  current delivery statuses. It requires no pending or unknown destination and
  `NEW.result = expected` (exact jsonb). The result is therefore database-derived; the
  Rust value is only a convenience.
- **State guard.** An action UPDATE trigger allows only:
  - pending → accepted/failed, only if the matching receipt exists;
  - pending → uncertain, only if all are terminal with ≥1 unknown;
  - pending → stalled, only at `consume_failures = 5`;
  - and only a monotone `consumed_through`.
- Email acceptance means provider acceptance only, never inbox receipt or read.
- No email verifier is invented, and not-applied is never inferred from a timeout.

### 4.4 Step progression and the effect wait (F008)

- The effect wait's deadline is **exactly** `run.deadline` (insert trigger, §2.5).
  `wait.deadline <= now` therefore coincides with the run-deadline branch handled by
  `maintenance::expire_on`.
- `CANDIDATES_SQL` (`polling.rs:105-111`): the wait branch gains
  `OR (wait.reason='effect' AND EXISTS(receipt for the action whose wait_id = wait.id))`.
  An effect wait without a receipt is rediscovered only at run deadline or when the
  run is terminal, never on every scan.
- `wait_commit::resume_on` handles `reason='effect'` explicitly, before the generic
  event lookup:
  - **No receipt** → `Pending`. The wait is not touched and the event lookup is not
    run.
  - **Accepted receipt** → `advance` with output = the receipt result. The wait is
    completed, the run goes back to running, and `batch_commit::complete` is called
    with `CompletionOwner::Parked`.
  - **Failed receipt** → the wait is completed and the failure goes through the
    existing `recovery::route_failure` writer, the one shared by `retire_on` and
    `route_pending_failure`, with Terminal `message.delivery_failed`. The frozen
    `final_error` route is taken, or the run fails when none is declared. No retry
    budget is consumed.
- The receipt is committed in the consumer's transaction, strictly before the resume
  transaction. An activation-limit or route failure on resume uses the existing wait
  `fail` path and the receipt is retained. Nothing resends.
- For `model_tool` origin the consumer records the receipt only. Progression of the
  enclosing step is the saved-call owner's (phase 06) responsibility.
- On cancelled or terminal runs, receipts are recorded as truth without advancing,
  creating a successor or returning tool output.

## 5. Recovery and reconciliation contract (F001)

### 5.1 Outcome table

`retry_safe` is `workflow_action_retry_safe`. The handler reports SafeToRetry for
every message outcome (shared `action_step_failure`, new `ActionFamily::Message`).
The predicate result then decides.

| # | Outcome | Step: code / class | Effective after `retry_safe` → decision | Run / attempt / wait after | Effect state | Direct-tool observation |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Validation refusal before intent (schema, bounds §1.1, group, duplicate) | `message.rejected` / Terminal | no marker → NULL → Safe → FinalError | attempt closed failed; final_error route or run failed; no wait | none | error; no intent |
| 2 | Routing/authority refusal in the publication transaction (actor demoted, capability revoked, recipient not a member, no run channel/binding, foreign/ambiguous/unsupported source) | `message.rejected` / Terminal | transaction rolled back, no marker → FinalError | as 1 | `prepared` | error; intent without marker |
| 3 | ApprovalRequired (frozen or current) | `message.approval_required` / Terminal | no marker → FinalError (phase 05 replaces this with an approval wait) | as 1 | `prepared` | `ApprovalRequired` |
| 4 | Admission limit (§2.2) | `message.action_limit` / Terminal | no marker → FinalError | as 1 | `prepared` | `Refused(ActionLimit)` |
| 5 | Crash, lease loss or transaction error before COMMIT | `message.interrupted` (or `workflow.interrupted` from recovery `report()`) / Retryable | no marker → NULL → Safe → RetryAt within the attempt budget. The next attempt re-prepares the same invocation and publishes once. | attempt closed interrupted; job pending | `prepared` | error; re-entry with the same model call uses the same invocation |
| 6 | Ambiguous COMMIT | per §2.6 | Committed → rows 8/9; absent → row 5; unresolved → `retire_on` under the fence. A live fence means the commit is absent (no marker), so retry. A lost fence means the park committed: no retirement, the park stands. In-app commit applied with a live job: marker + receipt → true → retry replays the receipt. | per resolved case | per case | error; re-entry observes the committed state |
| 7 | Worker crash after the committed park | — | not reaped (job not processing); durable wait | run `waiting/effect`; attempt closed `EffectParked`; wait waiting | `possible_dispatch` | Pending lost to the caller; re-entry returns Pending or the receipt |
| 8 | Pending (any delivery pending/retryable/sending) | — | step parked; if anything tried to retire it, marker without receipt → false → Reconcile | as 7 | `possible_dispatch` | `Pending` |
| 9 | All accepted | receipt `accepted` | resume → success completion | wait completed; successor or end | `committed` | `Committed(accepted)` |
| 10 | Failed (all known terminal, ≥1 dead_letter) | receipt `failed`; resume → `message.delivery_failed` / Terminal | `route_failure`; never resend. If a control retry ever reopens the step, `retry_safe` sees the receipt → safe, and re-entry replays the same failed receipt (same invocation); it cannot create a new delivery. | wait completed; execution FinalError route or run failed | `committed` | `Committed(failed)` |
| 11 | Uncertain (all terminal, ≥1 outcome_unknown) | — (no receipt) | stays in the effect wait. At run deadline `expire_on` fails the run, expires the wait and `record()` writes a `message` reconciliation row. | run failed at deadline; unknown retained | `needs_reconciliation` | `Uncertain` |
| 12 | Mixed siblings | accepted+pending/retryable → row 8. Only the unsent or retryable delivery re-enters the engine; a delivered sibling is terminal and never re-enters. accepted+failed → row 10 with the accepted sibling in the result. accepted+unknown or failed+unknown → row 11. | | | | |
| 13 | Cancel or deadline while pending (§6) | — | unsent → dead_letter superseded; in-flight keeps its intent. The consumer later records failed, uncertain or accepted truth without advancing. `record()` covers markers unresolved at that moment. | run cancelled or failed | `committed` or `needs_reconciliation` | observation of the truth |
| 14 | Re-entry with an existing pending/uncertain marker (for example a control retry) | `message.unresolved` / Retryable (`PossibleDispatchExists`) | marker without receipt → false → Reconcile; never republished | parked reconciliation | `possible_dispatch` or `needs_reconciliation` | `Pending` or `Uncertain` |

Note on the enclosing agent execution of a direct tool: agent kinds are not
marker-owned, so their interruption default stays EffectOutcomeUnknown.
`retry_safe` overrides it to Safe only when every marker in that execution has a
receipt, as for HTTP/MCP direct tools. Phase 06 owns the saved-call resume. Until
then, an interrupted agent execution holding a pending message parks conservatively,
and its message markers are represented by `record()` (§5.2).

### 5.2 Shared predicates (explicit decisions)

- **`marker_owned_effect_kind`** gains `message.send | message.reply`.
  - Invariant (as for 04.10-F011): the message handler's only effect-bearing write is
    the publication transaction, which always inserts the `message` marker first.
    Provider I/O happens only in the delivery engine, after commit. So "no marker"
    proves "no message and no delivery".
  - The `failure.rs` test asserting `message.send` false is changed deliberately.
  - `recovery.rs` `report()` and `pending_recovery::unknown_effect` then treat message
    kinds without a marker as safe.
- **`workflow_action_retry_safe`**: no code change. For a `message` marker, a receipt
  (accepted or failed) means CONTINUE. Without a receipt it is unsafe, because
  `workflow_action_replay_supported` returns false for non-remote markers
  (`20260930150000`, first check) and `workflow_action_not_applied_available` returns
  NULL (`20261001090000:8`). This is tested in SQL.
- **`workflow_action_effect_states`**: one new branch before `possible_dispatch`. A
  `message` marker whose action state is `uncertain` or `stalled` becomes
  `needs_reconciliation`. Pending stays `possible_dispatch` (accurate: a delivery may
  be in flight). A receipt stays `committed`. The vocabulary and the Rust parser are
  unchanged.
- **`action_uncertainty::record`** and **`workflow_action_reconciliations`**: the
  effect_kind filter and CHECK become `IN ('remote','message')`.
  - On cancel (`controls::cancel_on`), deadline (`maintenance::expire_on`) and
    Reconcile parks (`retire_on`), every receipt-less `message` marker becomes a
    retained reconciliation row.
  - The existing guard (terminal or reconciliation-parked run) is unchanged.
  - A later receipt still takes precedence in the projection.
- **04.7 reconciliation command**: the snapshot of a `message` invocation refuses with
  the distinct `reconciliation.unsupported_effect` before any write. Today it would
  only fail to find a `remote` marker (`action_reconciliation/mod.rs:216`). The
  evidence, coverage and consumption guards stay remote-only.

### 5.3 Documented deferral: evidence path for `outcome_unknown` deliveries

Against phase 04 items 5–6:

- **Item 5 is met.** With safe replay and provider reconciliation unavailable, the
  invocation is marked as needing reconciliation (action `uncertain`, projection
  `needs_reconciliation`, a reconciliation row at deadline, cancel or park). The
  owning step stays parked. Nothing is retried blindly or fabricated.
- **Item 6's command exists, but nothing can feed it here.** The 04.7 command settles
  only through a host-installed `ActionEvidenceVerifier` attestation
  (`reconciliation/verification.rs:133`). Operator assertions record only Unknown
  notes. SMTP has no post-hoc ledger that could prove a specific Message-ID was or
  was not accepted, and no email verifier exists. An operator-asserted not-applied
  would violate "Only a proven not-applied result permits retry".
- "Unknown remains unknown; wait deadlines can fail the run without discarding the
  unresolved invocation" is exactly row 11.
- Building a per-delivery evidence path now would give no settleable outcome and would
  widen the accepted 04.7 machinery (claims, coverage, episodes) for an unusable kind.
- **Open obligation** recorded for root, for the first provider with a queryable
  ledger (for example a Resend API status):
  - a verifier-attested per-delivery path;
  - applied → that delivery `delivered`;
  - final not-applied → only that delivery returns to `retryable`;
  - siblings untouched;
  - run → parent lock order;
  - the new events of §4.1.

  Root may reject this deferral and require the path inside 04.11.

## 6. Cancellation, expiry, current permission and duplicate callbacks

(D1 §5 plus F008 effects)

- A new `cancel_workflow_deliveries_on(tx, company, run, reason)` extends the existing
  helper's unsent-versus-in-flight logic to linked delivery ids. It locks parents in
  id order after run, executions and jobs.
  - It is called by `controls::cancel_on` (`workflow_cancelled`) and
    `maintenance::expire_on` (`workflow_expired`).
  - Unsent pending/retryable deliveries become dead_letter, superseded, known not
    sent.
  - Sending/outcome_unknown deliveries keep the lease and get only the cancellation
    intent, so a late provider acceptance is still recorded.
  - Accepted siblings are never altered.
  - Cancellation cannot promise rollback after a committed `begin_part`.
- **Workflow-linked `begin_part`**:
  - Before marking possible provider entry, it reads the immutable destination link
    without a lock and takes company/actor (as `authorize_company`), then run
    `FOR SHARE`, then the parent. Cancellation takes run then parent and the consumer
    takes run then parents; there is no inversion.
  - It rechecks: the run is live and before its deadline; the actor's current
    authority and messaging policy; the recipient is still a member and principal with
    the current email equal to the frozen address.
  - A refusal cancels the unsent work (`workflow_revoked` / `recipient_revoked` /
    `workflow_expired`) without a provider call.
  - Deliveries that are not workflow-linked keep their existing path.
- **Duplicate callbacks.** `complete_part` gains `AND part.status='sending'`
  (`queue.rs:229-240` has none today). Only a currently sending part takes its first
  outcome. A duplicate callback is a no-op that preserves the first durable acceptance
  and produces no event.
- Existing crash and reaper semantics stay: sending becomes unknown;
  prepared/retryable may retry; delivered never re-enters. No resource limit is
  raised.

## 7. Affected paths (caller tracing before edits: `graft callers <sym> --depth all`)

- **Registry/publication:** `registry/actions.rs`, `schemas.rs` (`content`,
  `message_result`), `registry/mod.rs`, `publication`, `authority::supported_publication`,
  binding readiness.
- **Actions:**
  - `actions/failure.rs` (`marker_owned_effect_kind`, `ActionFamily`,
    `ActionStepOutcome`);
  - a new `MessageDispatch` port;
  - `action_dispatch` and `action_receipts`;
  - `action_uncertainty::record`, `recovery::{report, retire_on, route_failure}`,
    `pending_recovery::unknown_effect`;
  - `action_reconciliation/mod.rs` (refusal).
- **Waits/poller:** `waits::park_on` style writer for effect waits;
  `wait_commit::resume_on`; `completion_job` (new `EffectParked` owner);
  `polling::{CANDIDATES_SQL, poll_work}`; `fair_polling::{discover, company_query}`;
  `inspection` (diagnostics); `supervise::supervise_io` and the `WorkflowLeases`
  implementations (Postgres plus the supervise test port); `worker.rs`.
- **Controls/maintenance:** `controls::cancel_on`, `control_retry::retry_on` (unchanged;
  behavior per rows 10 and 14), `maintenance::{expire_on, retire_jobs}`.
- **Delivery:**
  - `queue.rs`: `begin_part`, `complete_part`, reaper locking;
  - `cancellation.rs`: the new helper and reasons;
  - the delivery worker loop is unchanged.
- **Threads:**
  - `thread::{insert_message_on, associate_message_on}` and a transactional
    `insert_thread_on` extracted from `create_thread` (`thread/mod.rs:277`);
  - email correlation storage;
  - inbound resolution (`thread/external.rs:267-295`) is unchanged and tested.
  - Channel ACL decides what a member's inbound reply to the channel triggers. That is
    unchanged and out of scope.
- **Composition:** a message `WorkflowHandler` adapter (outside the application layer,
  as for HTTP/MCP) with `DeliveryComposer`, the trusted directory and the dispatch port.
- **Docs:** `plan/workflow-foundation/BRIEF-04.11.md` carries the no-BC note and is
  added to `NO_BACKWARD_COMPATIBILITY_NOTES` (`src/main.rs`).
- **CI scripts:** `scripts/lease-budget.sh` gains the new measured operations
  (`message.publish`, `message.consume`) in `REQUIRED_OPS` and its filters
  (`workflow_message_`). No threshold is raised.

## 8. Verification (F006)

Environment for every check:

- `TEST_DATABASE_URL=DATABASE_URL=postgres://workflow0410@127.0.0.1:55450/workflow0410_test`
  (task fixture, system id 7697669855432439122);
- `SQLX_OFFLINE=true` (`false` only for SQLx/migrate);
- `CARGO_BUILD_JOBS=2`;
- `PATH` with `/opt/homebrew/opt/postgresql@18/bin`;
- logs under `plan/workflow-foundation/docs/logs/04.11/`, statuses in
  `gate-status.log`, scripts saved there.

### 8.1 Focused checks during implementation

Each is `cargo test --locked --offline --lib -- <filters>`. New tests use these name
prefixes so the filters are exact.

| Area | Filters |
| --- | --- |
| A registry/publication | `registry:: publication:: workflow_authority workflow_binding message_registry_` |
| B publication transaction, admission, guards (SQL-only included) | `workflow_message_publish_ workflow_message_sql_ workflow_action_dispatch_` |
| C receipts, events, consumer, poison, waits | `workflow_message_receipt_ workflow_message_wait_ wait_ polling fair supervise` |
| D recovery/predicates | `workflow_message_recovery_ workflow_action recovery uncertainty action_reconciliation` |
| E delivery engine | `delivery:: workflow_message_delivery_ cancellation task_release channel_assignment` |
| F thread consumers | `thread:: workflow_message_thread_ find_outbound_reply_after latest_thread_rfc` |
| G stack | `scripts/stack-budget.sh --offline -- workflow_message workflow_action delivery wait` after the handler and supervisor change |

Migrations are applied to the fixture with `SQLX_OFFLINE=false cargo sqlx migrate run`.
If they do not apply cleanly, the task database is reset (no backward compatibility).

### 8.2 Required tests, real PostgreSQL and default parallelism

Every test asserts run, attempt, job and wait state plus message, delivery and receipt
rows, not only send counts.

- **F001:** one test per row of §5.1. In particular:
  - pre-marker crash retries and publishes once;
  - a committed park with a dead worker is not reaped and resumes on its receipt;
  - ambiguous COMMIT, all three resolutions;
  - uncertain at deadline: run failed, wait expired, `message` reconciliation row,
    `needs_reconciliation`;
  - the 04.7 command refuses `reconciliation.unsupported_effect` with state unchanged;
  - `retry_safe` SQL for a message marker without and with a receipt;
  - `marker_owned_effect_kind` unit test;
  - control retry of a failed message step either is refused or, if it reopens the
    step, replays the failed receipt with no new delivery (asserting whichever the
    existing `retry_on` eligibility yields).
- **F002:**
  - admission at 127→128 admits and 128→129 refuses, through both the service and
    SQL-only paths. The refusal writes no marker, message, thread, delivery or wait
    and gives `message.action_limit`, Terminal, FinalError;
  - consumption page: 9 ready actions take two visits;
  - poison: a full page of failing actions over two iterations without advancing time
    is not reclaimed, and reaches `stalled` at 5 failures;
  - per-transaction lock count ≤20.
- **F003:**
  - subject of exactly 998 bytes from 3-byte characters is accepted; 999 is refused;
  - a 4-byte character crossing 998 is refused, and the same character ending at
    exactly 998 is accepted;
  - 998 code points of `€` pass the schema but are refused at runtime before the
    marker;
  - a literal over 998 bytes is refused at publication;
  - 20 destinations accepted and 21 refused (schema and runtime);
  - duplicates and `group` refused;
  - an empty body after sanitization refused.
- **F004:** two sibling deliveries. The lower-sequence commit is delayed (the producer
  is paused while holding its parent) while a higher one commits and the consumer
  runs. The consumer blocks on the parent and then observes both. Also: reaper versus
  consumer run concurrently with no deadlock; a duplicate callback adds no event.
- **F005:**
  - a reply quoting each recipient's copy resolves to that recipient's message and
    thread;
  - a send in a run whose thread is a customer email thread writes nothing there, the
    channel agent's later answer is not suppressed and its In-Reply-To is unchanged;
  - an internal reply note in a customer thread leaves the guard and fallback
    unaffected;
  - an external reply uses the source's own RFC id and references despite a newer
    distractor, its recipient is the source author, and the guard treats it as the
    answer;
  - classification columns asserted for each case of §1.3;
  - stable Message-ID across restart and safe retry;
  - foreign, ambiguous, outbound and Slack sources refused.
- **F007 (SQL-only negatives):**
  - forged `fence_expires_at` raises;
  - an action inserted after the job close raises;
  - a delayed COMMIT past the captured expiry, and past the run deadline, rolls back
    everything;
  - wrong-tenant, wrong-message and wrong-delivery links are rejected by FKs;
  - a premature accepted receipt (one delivery pending), a failed receipt with an
    unknown delivery, and a result jsonb mismatch are all rejected;
  - forged state transitions are rejected.
- **F008:**
  - the effect-wait deadline equals the run deadline;
  - two consecutive poll scans without a receipt yield no candidate for that
    execution;
  - the generic `resume_on` with no receipt returns Pending and does not touch the
    wait;
  - deadline and cancel supersede unsent deliveries and keep in-flight ones;
  - a late acceptance after cancel records the receipt without a successor.
- **Carried from D1 §6:**
  - competing dispatchers and model-call re-entry; changed arguments refused;
  - injected rollback at message, delivery, link and wait writes;
  - provider acceptance resumes only after every destination;
  - crash after send and the reaper; restart gives no duplicate provider send;
  - cancel-first, begin-first and revoke-first orderings; stale fences;
  - event before observer, and observer downtime;
  - competing consumers;
  - current-result authorization on direct replay;
  - the direct Pending → receipt seam without completing the enclosing step;
  - regressions for shared wait, supervisor, poll and control behavior.

### 8.3 Final combined phase 04 gate

Run once after implementation, in order. Logged in `logs/04.11/final-gates.sh`, with
statuses in `final-gate-status.log`.

1. `cargo fmt --all -- --check`
2. `git diff --check`
3. `scripts/tests/transport-boundary.sh && scripts/transport-boundary-check.sh`
4. `scripts/tests/credential-key-rotation.sh` and `scripts/tests/deploy.sh`
5. `cargo fetch --locked` (only if the lockfile changes); `cargo check --locked --offline --all-targets`
6. `cargo clippy --locked --offline --all-targets -- -D warnings`
7. Fresh migrations: recreate `workflow0410_fresh_test`, then
   `SQLX_OFFLINE=false DATABASE_URL=…/workflow0410_fresh_test cargo sqlx migrate run`.
   Check schema equivalence with
   `pg_dump -s` (fresh against fixture, diff empty after normalization), as in 04.10.
8. `SQLX_OFFLINE=false cargo sqlx prepare -- --locked --offline --all-targets`, then
   `cargo sqlx prepare --check -- --all-targets`
9. `cargo test --locked --offline --all-targets --no-run`
10. `scripts/test-network-isolation.sh cargo test --locked --offline --all-targets`
    (full real-database suite; the macOS `sandbox-exec` profile allows localhost, so
    the fixture is reachable)
11. `scripts/test-network-isolation.sh scripts/stack-budget.sh --offline` (full lib
    suite at 2048 KiB)
12. `scripts/test-network-isolation.sh scripts/lease-budget.sh --offline` (including
    the new required operations)
13. `graft build`

Not replicated locally: CI's npm, CSS, Playwright and SSE steps (no frontend change in
this batch; run them only if frontend files change) and the `pg_stat_statements` probe
step (CI container configuration). Both are reported as not run.

Then: an independent whole-batch review of implementation and combined phase 04
effects, and root digest/evidence acceptance. Every failed gate and its diagnosis is
retained. A failed stock or lease gate is fixed by shrinking the chain or operation,
never by raising `STACK_BUDGET_KIB` or a bound without a recorded reason.

### 8.4 Resources and reporting

- Disk has about 3.5 GiB free. Before the first build, the regenerable project
  artifacts are removed with `cargo clean -p mail_agents` (about 14G; third-party
  dependencies and the registry are kept). `df -h` is checked before steps 9–12.
- The fixture starts only when needed, after identity verification, and is stopped
  with `pg_ctl -D /private/tmp/workflow0410-20261017 -m fast stop` at checkpoints.
- A gate that cannot run (disk, sandbox, missing tool) is recorded as **unavailable**
  with its reason, never as passed. Completion is not claimed while any required gate
  is failed or unavailable.

## 9. Out of scope and obligations kept explicit

- Phase 05: approval settlement.
- Phase 06: saved-call suspend/resume for direct Pending; agent-execution recovery.
- Phase 08: production host composition and bypass removal.
- Phase 09: UI.
- The §5.3 verifier-attested delivery evidence path for a provider with a ledger.
- Groups (rejected by the user).
- Remote HTTP/MCP admission beyond 128 (pre-existing; observation only).
