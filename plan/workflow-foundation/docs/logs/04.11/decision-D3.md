# 04.11-D3 — Delta over D2: message delivery reconciliation, consumer bound, backstops

No backward compatibility: reset databases for incompatible schemas/payloads; no
backfills, dual paths, legacy decoders or staged rollout.

Status: proposed architecture gate, revision after 04.11-R003 (CHANGES REQUIRED for
D2). Not approved.

**D3 is an explicit delta.** The effective decision is D2
(`decision-D2.md`, SHA256 `1dcd1a9e99e57cd32e15bf095490961722f185153c66945cd82d29bd4d943a44`)
with the sections listed below replaced or extended. Every D2 section not named here
applies unchanged. D2 and D1 are retained unchanged as history.

Root ruling (2026-10-17): the D2 §5.3 deferral is rejected, and the verifier-backed
reconciliation path is in 04.11 scope. The user has not reduced scope.

| D3 section | Effect on D2 |
| --- | --- |
| §0 | replaces D2 §0 in full |
| §A | replaces the "Database backstop" bullet of D2 §2.2 (F009) |
| §B | replaces item 4 of the "Deferred commit guard" bullet of D2 §2.5 (F009) |
| §C | extends D2 §2.4 (schema for F010) |
| §D | replaces D2 §4.1 in full (F004 premise under F010) |
| §E | replaces D2 §4.2 in full (F010 discovery, F011 bound) |
| §F | replaces the "State guard" bullet of D2 §4.3 |
| §G | replaces D2 §5.1 rows 11–13 and adds rows 15–21 |
| §H | replaces the "04.7 reconciliation command" bullet of D2 §5.2 and D2 §5.3 in full (F010) |
| §I | extends D2 §6 (begin_part proof check) |
| §J | extends D2 §7 and replaces its "CI scripts" bullet (F011) |
| §K | extends D2 §8.1–§8.3 (F009–F011 checks) |
| §L | replaces D2 §9 in full |

## 0. Finding dispositions (replaces D2 §0)

| Finding | Severity | Disposition | Resolved in |
| --- | --- | --- | --- |
| 04.11-F001 | medium | Resolved at decision level in D2 (R003). Its evidence-path remainder moved to F010, now resolved in §H. | D2 §5.1–§5.2; D3 §G, §H |
| 04.11-F002 | medium | Resolved at decision level in D2 (R003). The backstop is tightened by F009 (§A). | D2 §2.2, §4.2; D3 §A, §E |
| 04.11-F003 | low | Resolved at decision level in D2 (R003). Unchanged. | D2 §1.1 |
| 04.11-F004 (optional) | low | Adopted in D2. Its premise is restated for F010: `delivered` and `dead_letter` stay final, and `outcome_unknown` now exits only through evidence settlement or an expired-proof refusal. The event bound and lock modes are restated. | D3 §D |
| 04.11-F005 | medium | Resolved at decision level in D2 (R003). Unchanged. | D2 §1.2–§1.5 |
| 04.11-F006 | low | Resolved at decision level in D2 (R003). Extended with F009–F011 checks. | D2 §8; D3 §K |
| 04.11-F007 | low | Resolved at decision level in D2 (R003). The receipt guard stays stable under F010 because receipts are written only when no destination is pending or unknown, and `delivered`/`dead_letter` are final. | D2 §2.5, §4.3; D3 §D, §F |
| 04.11-F008 | low | Resolved at decision level in D2 (R003). Unchanged. | D2 §4.4, §6 |
| 04.11-F009 (optional) | low | Adopted. The admission trigger locks the fenced attempt row before counting. The deferred step-origin guard requires the attempt's `EffectParked` close cause. A concurrent SQL-only test is added. | §A, §B, §K |
| 04.11-F010 | medium | Adopted. Message invocations go through the existing 04.7 `ActionReconciliationService` and `ActionEvidenceVerifier` port, with one subject per `outcome_unknown` destination delivery (at most 20). Applied marks that delivery delivered. Final not-applied makes only that delivery retryable on a live run, and supersedes it on a terminal run or when attempts are exhausted. Unknown changes nothing. Tests use a test-host verifier; no production verifier is claimed. | §C, §D, §G, §H, §I, §K |
| 04.11-F011 | low | Adopted. The consumer runs under `WriteBound` `message.consume` with the workflow worker's `LeasePolicy`. Leading locks are defined for consume (run, action, parents) and publish (`authorize_on` plus destination-owner locks). Delay and contention tests are marked `Injected` and kept out of the lease filters. No threshold is raised. | §E, §J, §K |

## A. Admission backstop (replaces the "Database backstop" bullet of D2 §2.2; F009)

- A BEFORE INSERT trigger on `workflow_action_dispatches` for `effect_kind='message'`
  first locks the fenced attempt row:
  `SELECT 1 FROM task_attempts WHERE task_id=NEW.job_id AND attempt_number=NEW.attempt_number AND execution_generation=NEW.execution_generation AND worker_id=NEW.worker_id FOR UPDATE`.
  The Rust path already holds this lock through `authorize_on`, so it does not wait.
- Only then does the trigger repeat the bounded count
  (`LIMIT 128` over the execution's dispatches). If the count is 128 it raises
  `ERRCODE='54000'`, `workflow message action limit`.
- Two SQL-only writers under the same fence are therefore serialized at the first
  marker insert. The second waits until the first commits, then counts it.
- Writers under different fences of the same execution cannot coexist: only one live
  attempt exists per execution.

## B. Step-origin close identity (replaces item 4 of the deferred-guard bullet of D2 §2.5; F009)

For origin `step`, all of these must hold at COMMIT:

- run `waiting/effect`, with `run.deadline > clock_timestamp()`;
- the wait `id=action.wait_id`, `reason='effect'`, `state='waiting'`,
  `deadline=run.deadline`;
- the exact fenced job and attempt rows are closed **with the `EffectParked` close cause**.
  This is the value the new `completion_job` owner writes into the attempt's existing
  close fields; the column is chosen to fit `task_attempts`. Any other close cause is
  rejected.
- `action.fence_expires_at > clock_timestamp()`.

`confirm_suspension` (D2 §3) checks the same close cause, so the Rust and SQL proofs
agree.

## C. Schema additions for reconciliation (extends D2 §2.4; F010)

All in the same new migration; reset the database if it does not apply cleanly.

- **Evidence subject.** `workflow_action_evidence` gains nullable
  `(delivery_ordinal smallint, delivery_id uuid)`, either both set or both NULL.
  - Composite FK
    `(company_id, invocation_id, delivery_ordinal, delivery_id) → workflow_message_destinations(company_id, invocation_id, ordinal, delivery_id)`,
    with a new UNIQUE on destinations.
  - The evidence commit guard (latest definition) is extended:
    - `effect_kind='message'` ⇔ delivery subject set;
    - message evidence has no coverage rows;
    - message evidence requires `observed_at >= part.request_started_at` of the
      subject part;
    - remote evidence keeps every existing check unchanged.
- **One-use reopen consumption.** New immutable table `workflow_message_delivery_reopens`:
  - columns: `company_id`, `id`, `evidence_id`, `delivery_id`, `attempt_count`,
    `valid_until`, `created_at`;
  - UNIQUE `(company_id, evidence_id)`, so a proof reopens at most once;
  - composite FKs to the evidence and the destination delivery.
  - A BEFORE INSERT guard locks the run, then requires:
    - the evidence is `final_not_applied` and `grant_eligible`, unexpired, for the
      same invocation and delivery;
    - the delivery is currently `outcome_unknown` with this `attempt_count`.
- **Delivery provenance.** `message_deliveries` gains nullable `settled_evidence_id`
  (FK to evidence) and a `reopen_id` (FK to the reopen row).
- **No unauthorized exit from `outcome_unknown`.** A BEFORE UPDATE trigger on
  `message_deliveries` rejects any status change away from `outcome_unknown` unless it
  is one of three:
  - `→ delivered` with `settled_evidence_id` naming applied evidence for this
    delivery;
  - `→ retryable` with `reopen_id` naming a reopen of this delivery at the current
    `attempt_count`;
  - `→ dead_letter` with `settled_evidence_id` naming final-not-applied evidence for
    this delivery.

  It also rejects any status change away from `delivered` or `dead_letter`. The same
  rules apply to `message_delivery_parts`. A part transition `retryable →
  outcome_unknown`, which is new and used only by §I, is accepted only with class
  `reconciliation_proof_expired`, for a parent carrying a `reopen_id`, while the part
  has started no request since that reopen.
- **New failure classes** in `valid_delivery_failure_class`:
  - `reconciled_not_applied` (on the reopened row);
  - `not_applied_exhausted` (final not-applied with no attempts left);
  - `reconciliation_proof_expired` (§I).
- **Request-digest encoding.** The reconciliation command's request-digest encoding
  gains a versioned `target` field for both kinds. No compatibility decoding; reset
  the database.
- `workflow_action_reconciliations` (first-uncertainty observation): its guard keeps
  remote markers unchanged. For `message` markers it is allowed in any run state,
  because the delivery engine, not the run, owns the effect. The consumer inserts the
  observation when an action enters `uncertain`. `record()` still covers cancel,
  deadline and Reconcile parks (D2 §5.2).

## D. Event production invariant (replaces D2 §4.1; F004 under F010)

- Events are written only by an AFTER UPDATE OF status trigger on `message_deliveries`
  for linked rows. It fires when `OLD.status IS DISTINCT FROM NEW.status` and either
  `NEW.status IN (delivered, dead_letter, outcome_unknown)` or
  `OLD.status = 'outcome_unknown'` (an exit, including reopen to `retryable`).
- The UPDATE holds the parent row lock before `nextval`.
- Producing transitions, all of them parent status UPDATEs:

  | Transition | Function |
  | --- | --- |
  | complete / aggregate | `complete_part` → `aggregate_parent_on` |
  | retry / dead-letter | `fail_delivery` |
  | cancellation settle | `settle_cancelled_on` |
  | stranded cancellation settle | `settle_stranded_cancellations_on` |
  | unsent supersession | `cancel_task_deliveries_on` and the workflow cancellation helper (D2 §6) |
  | begin_part refusals | D2 §6 and §I |
  | lease expiry | the reaper, which locks parents `ORDER BY id FOR UPDATE SKIP LOCKED` first (D2 §4.1, retained) |
  | evidence settlement | the new delivery helper `settle_unknown_delivery_on` (§H) |

- **Premise, restated.**
  - `delivered` and `dead_letter` remain final. R003 checked every `UPDATE message_deliveries`
    site: none redrives them, and the §C trigger now forbids it.
  - `outcome_unknown` is **no longer final**. It exits only through evidence
    settlement: applied → `delivered`; final not-applied → `retryable` or
    `dead_letter`.
  - Besides the existing request-started entry (the reaper's `sending →
    outcome_unknown`), a reopened delivery re-enters `outcome_unknown` only through the
    expired-proof refusal of §I.
  - The §C triggers enforce both the exit rule and this new entry rule.
  - Receipts are written only when no destination is pending or unknown (D2 §4.3), so
    every receipt rests on final statuses and the receipt guard's recomputation stays
    stable.
- **Event bound.**
  - Each entry into `outcome_unknown` needs either a provider request start (bounded by
    the delivery's frozen `max_attempts`) or an expired consumed reopen.
  - Each exit needs one authorized command with a fresh verified proof. A proof is
    consumed at most once (UNIQUE `evidence_id`).
  - So per delivery, events ≤ `2·max_attempts + 2` plus 2 per authorized
    reopen-and-expiry cycle. Storage is bounded by authorized commands, never by an
    automatic loop.
  - The consumer reads only `max(seq)` (§E), never the event rows, so the count does
    not bound any read.
- **Lock modes.**
  - Callback producers (`complete_part`, `fail_delivery`, `settle_*`, the reaper) take
    only the parent `FOR UPDATE` and the FK `KEY SHARE` on the immutable destination
    row.
  - Workflow cancellation, workflow `begin_part` refusals and evidence settlement take
    company/run before the parent. Settlement order is company → run `FOR UPDATE` →
    action `FOR NO KEY UPDATE` → parent `FOR UPDATE`.
  - The consumer takes run `FOR UPDATE` → action `FOR NO KEY UPDATE` → parents
    `FOR SHARE` in id order, and does not lock destination rows.
  - No producer locks an action row ahead of a run, so there is no cycle.

## E. Consumer transaction, discovery and bound (replaces D2 §4.2; F010, F011)

- Discovery is the shared constant `MESSAGE_RECEIPT_CANDIDATES_SQL`, used by
  `polling::poll_work`, `fair_polling::{discover, company_query}` and `inspection`. It
  selects executions having an action with:
  - `state IN ('pending','uncertain')` (an evidence exit can move `uncertain` back);
  - `consume_after <= now`;
  - `EXISTS(event seq > consumed_through)`.

  Selection is company-fair under the existing poll limit. An `uncertain` action with
  no new event is not selected, so there is no hot re-poll. `accepted`, `failed` and
  `stalled` are never selected.
- Work kind `PollWork::MessageReceipt` processes at most `MESSAGE_RECEIPT_PAGE = 8`
  actions per visit, ordered `(consume_after, invocation_id)`, **one action per
  transaction**.
- **Bound (F011).** Each consumer transaction runs under
  `WriteBound::start("message.consume", policy)`, where `policy` is the workflow
  worker's configured `LeasePolicy` (the policy its claims use). That gives
  `attempt_bound = min(lease/3, 2 s)` and the matching `resolution_bound`.
  - The consumer is idempotent: `consumed_through` is monotone and the aggregate is
    recomputed from authoritative state. An ambiguous COMMIT is therefore resolved by
    a read-only re-read of `consumed_through` and state within `resolution_bound`.
    Otherwise it is left to rediscovery, never retried in a loop.
- **Leading locks (F011)**, in order: run `FOR UPDATE`, action `FOR NO KEY UPDATE`,
  required parents `FOR SHARE` in id order (≤20).
  - The phase recorder's `locks_held()` is called immediately after the last parent
    lock.
  - The gated work phase is everything after that: one `max(seq)` aggregate, ≤20
    status reads, aggregation, the state/cursor update, and an optional receipt and
    observation insert.
  - Waits on a paused producer's parent lock therefore fall in the leading phase,
    which is reported but not gated.
- Steps: (1) the leading locks; (2) in a new statement, `max(seq)` for the invocation
  and the authoritative parent statuses; (3) aggregate (D2 §4.3); (4) advance
  `consumed_through`; (5) write state; (6) write the receipt when terminal and the
  first-uncertainty observation when entering `uncertain`.
- Poison handling: unchanged from D2. The error transaction rolls back; a separate
  short transaction sets `consume_failures+1` and
  `consume_after = now + min(2^n s, 300 s)`; the 5th failure sets `stalled`.
- The D2 "LIMIT 41 events / stall on overflow" rule is withdrawn, because the consumer
  never reads event rows.

## F. State guard (replaces the "State guard" bullet of D2 §4.3)

- Every write of `workflow_message_actions.state` must equal the aggregate derived in
  SQL from current destination statuses:
  - `pending` if any destination is pending;
  - else `uncertain` if any is unknown;
  - else `accepted`/`failed`, only when the matching receipt exists.
- Consequently `uncertain → pending | accepted | failed | uncertain` is allowed only
  as evidence changes statuses.
- `accepted` and `failed` are final, because the receipt is immutable and its
  statuses are final.
- `stalled` is written only at `consume_failures = 5` and is final for the consumer.
  Evidence may still change a stalled action's delivery truth; the stalled state
  remains observable for operators.
- `consumed_through` is monotone.

## G. Outcome table changes (replaces D2 §5.1 rows 11–13; adds rows 15–21)

| # | Outcome | Step: code / class | Effective decision | Run / attempt / wait after | Effect state | Direct-tool observation |
| --- | --- | --- | --- | --- | --- | --- |
| 11 | Uncertain (all terminal, ≥1 `outcome_unknown`) | — (no receipt) | Action `uncertain`; first-uncertainty observation inserted; the step stays in its effect wait while the run is live. Each unknown delivery can be reconciled (rows 15–21). Without reconciliation, at run deadline `expire_on` fails the run, expires the wait and `record()` covers the marker; the unknown is retained and stays reconcilable (row 18). | run `waiting/effect`, later failed at deadline | `needs_reconciliation` | `Uncertain` |
| 12 | Mixed siblings | — | accepted + pending/retryable → row 8 (only unsent or retryable deliveries re-enter the engine). accepted + failed → row 10 with the accepted sibling in the result. Any unknown → row 11. Reconciliation touches only its subject delivery. | per row | per row | per row |
| 13 | Cancel or deadline while pending (D2 §6) | — | Unsent → `dead_letter` superseded. In-flight keeps its intent. Later truth is recorded without advancing. Unknown deliveries stay reconcilable under terminal rules (row 18). `record()` covers markers unresolved at that moment. | run cancelled or failed | `committed` or `needs_reconciliation` | observation of truth |
| 15 | Reconcile Applied, live run | — | Subject → `delivered` (`settled_evidence_id`) → event → consumer re-aggregates. All accepted → receipt `accepted` → resume (row 9); otherwise stays pending, uncertain or failed. | resumes per aggregate | `committed` once receipted | `Committed(accepted)` on re-entry |
| 16 | Reconcile FinalNotApplied, live run, attempts left, no cancellation intent | — | Reopen consumed; **only the subject** goes to `retryable` (`reconciled_not_applied`) → event → action `pending`. The engine resends it with the same idempotency key and Message-ID; siblings untouched. Later rows 8–11 apply to the resend. | run `waiting/effect` | `possible_dispatch` | `Pending` |
| 17 | Reconcile FinalNotApplied, attempts exhausted | — | Subject → `dead_letter` (`not_applied_exhausted`), known not sent → destination failed → row 10 once no unknown remains. No limit raised. | per aggregate | per aggregate | per aggregate |
| 18 | Reconcile on a terminal or cancelled run, or with a cancellation intent | — | Applied → `delivered` truth; the receipt may be derived; no advancement. FinalNotApplied → `dead_letter` superseded (`workflow_cancelled`/`workflow_expired`); never resent. | unchanged terminal | `committed` or `needs_reconciliation` | observation of truth |
| 19 | Reconcile Unknown; VerifiedReference with no installed verifier; unauthorized or cross-company actor; stale snapshot; wrong or non-unknown subject; expired proof at settlement | — | UnknownRecorded or the existing typed refusal / `Blocked`; no delivery change. A stale snapshot is refused with no grant. | unchanged | unchanged | unchanged |
| 20 | Finality breach: Applied attested for a delivery whose consumed reopen proof was observed after the attested acceptance | — | Subject recorded `delivered` (truth) plus an `workflow_action_evidence_conflicts` fact (evidence = applied, contradictory = the consumed proof; the existing CHECK is satisfied). `workflow_action_has_evidence_conflict` then vetoes further reopen grants for the execution, and `retry_safe` for it. | unchanged | `committed` once receipted (conflict shown separately) | per aggregate |
| 21 | Reopened delivery whose proof expired before provider entry (§I) | — | No provider call; subject goes back to `outcome_unknown` (`reconciliation_proof_expired`) → event → action `uncertain`. A fresh proof is required. | unchanged | `needs_reconciliation` | `Uncertain` |

## H. Message delivery reconciliation (replaces D2 §5.2 "04.7 reconciliation command" bullet and D2 §5.3; F010)

### H.1 Same service, same authority

- Message invocations use the existing `ActionReconciliationService::reconcile`
  (04.7). That covers:
  - authorization: owner/admin preflight through `WorkflowOperation::Reconcile`, the
    actual run association, transactional reauthorization under company/principal
    locks, and resource access through the application-owned resource policy port,
    extended for the Local `messaging` resource (owner/admin, as `authorize_company`);
  - the idempotent `(company, command_key)` command table, request digest, exact
    replay and revision conflict;
  - the two-transaction snapshot → verify → settle flow with no locks held during
    verification, the five-second verification deadline, DB-clock checks, proof
    validity (≤24 h), the evidence revision bump and the command receipt.
- No new command, port or authority path is added.
- `ReconcileActionCommand` gains
  `target: ReconciliationTarget::{Remote, MessageDelivery { ordinal, delivery }}`,
  included in the request digest. `marker` names the `message` dispatch for message
  targets. A target kind that differs from the marker's effect kind is refused.

### H.2 Subjects and snapshot

- A subject is exactly one destination delivery of the invocation that is currently
  `outcome_unknown`. There are at most 20 per invocation (`MAX_MESSAGE_DESTINATIONS`).
  Anything else is refused as the existing non-disclosing missing/typed refusal.
- Snapshot transaction:
  - locks: company/principal → run `FOR UPDATE` → action `FOR NO KEY UPDATE` →
    subject parent `FOR SHARE`;
  - restores the frozen action through the existing scoped frozen-intent restore, in
    any run state;
  - reads `DeliverySubject`: delivery and part ids, ordinal, frozen recipient address,
    predicted Message-ID (provider key), part body digest ("what a reconciliation
    lookup can compare", `protocols/email/egress.rs:393-395`), source binding and
    transport, `attempt_count`, `request_started_at`, `max_attempts` and cancellation
    intent.
- Coverage digest: SHA-256 over a canonical, versioned encoding of invocation, marker,
  ordinal, delivery, part, Message-ID, recipient, digest, `attempt_count` and
  `request_started_at`.
- `ReconciliationSnapshot` carries
  `subject: SubjectFacts::{Remote { entries }, Delivery(DeliverySubject)}`. The remote
  fields and semantics are unchanged.

### H.3 Verifier contract for delivery subjects

- It is the same `ActionEvidenceVerifier` port. The registration is
  `EvidenceVerifierRegistration::approve` over the trusted built-in message
  ToolSnapshot and the Local messaging target, with `provider` = the email transport
  provider id. That provider must equal the subject's transport, and it is installed
  only at trusted host composition.
- **Applied**: an authentic provider observation that a message with exactly this
  Message-ID, to exactly this recipient, with this part digest was accepted. It carries
  the provider's acceptance time, which is required for the §G row 20 breach check,
  and an optional provider key. There is no recovered output; message receipts are
  derived in SQL (D2 §4.3).
- **FinalNotApplied**: final non-application **and quiescence of every request ever
  made for this delivery**. All `attempt_count` requests carry the same Message-ID, and
  none of them may still become accepted.
  - Absence now, a timeout, SMTP session state, an expired local lease or the absence
    of a bounce cannot satisfy it.
  - A verifier that cannot prove old requests permanently unable to apply returns
    Unknown.
- **Unknown** otherwise. Errors propagate.
- With no installed registration, a VerifiedReference is refused (the existing
  unsupported-registration rejection) and an UnknownNote records Unknown.
- **No production verifier is installed**, as for MCP (BRIEF-04.10:28). Tests inject
  a database-backed test-host ledger verifier through the real service
  (BRIEF-04.10:60 precedent).

### H.4 Settlement

Settlement takes the same locks as the snapshot, but the parent `FOR UPDATE`. It
re-reads every snapshot fact and the coverage digest; any difference is a stale-proof
refusal with no grant. Then it acts on the effective disposition through the delivery
helper `settle_unknown_delivery_on` (delivery module, one decision, one place):

- **Unknown** → UnknownRecorded. No delivery change.
- **Applied** → evidence row, then the part and parent become `delivered`
  (`delivered_at` = DB now, `provider_message_key` = attested or predicted key,
  `settled_evidence_id`), emitting an event. This is allowed on live and terminal runs.
  Applied never downgrades. Outcome `AppliedRecorded { receipt: false }`; the consumer
  derives the action receipt.
- **FinalNotApplied** requires an unexpired proof and no evidence conflict for the
  execution. Otherwise the outcome is `Blocked { ProofExpired | EvidenceConflict }`
  with the evidence retained.
  - Live run (`queued|running|waiting`, deadline > now), no cancellation intent, and
    `attempt_count < max_attempts`: a reopen row is inserted (consuming the proof).
    The part and parent become `retryable` (`available_at` = now,
    `reconciled_not_applied`, `reopen_id`), emitting an event. Outcome
    `Scheduled { receipt_only: false }`.
  - Terminal, cancelled or past-deadline run, or a cancellation intent: `dead_letter`
    superseded (`workflow_cancelled` or `workflow_expired`), `settled_evidence_id`,
    event. Outcome `NotAppliedRecorded`.
  - Attempts exhausted: `dead_letter` (`not_applied_exhausted`),
    `settled_evidence_id`, event. Outcome `NotAppliedRecorded`.
- **Finality breach** (§G row 20) → outcome `EvidenceConflict`, with the delivered
  truth and the conflict fact committed together.

What settlement never does:

- reopen a workflow job;
- write an action receipt;
- complete a step.

Step progression stays with the consumer and the effect wait (D2 §4.4). A
`model_tool`-origin agent execution parked for reconciliation is not reopened by this
path; that remains phase 06's saved-call owner (D2 §5.1 note).

The shared predicates are unchanged for remote markers. `workflow_action_not_applied_available`
and `workflow_action_retry_safe` remain remote-only proof consumers. Message proofs
are consumed only by reopen rows, never by the remote entry protocol.

## I. begin_part reopen-proof check (extends D2 §6)

- For a workflow-linked part whose parent carries a `reopen_id`, and which has started
  no provider request since that reopen (`part.request_started_at` is NULL or earlier
  than `reopen.created_at`), `begin_part` additionally requires the consuming proof's
  `valid_until > DB now`. This runs after the D2 §6 rechecks, under the same company
  → run `FOR SHARE` → parent order.
- If the proof has expired, there is no provider call. The part and the claimed parent
  go back to `outcome_unknown` (`reconciliation_proof_expired`). The parent's lease
  fields are cleared, as `settle_cancelled_on` does. The §C entry rule permits this only
  for that reopen, and the transition emits an event (§G row 21).
- This matches 04.7's "an expired proof before enter prevents the call".

## J. Affected paths (extends D2 §7; replaces its "CI scripts" bullet)

- **Reconciliation:**
  - `actions/reconciliation/{contracts.rs, service.rs, verification.rs}`: target,
    subject facts, registration check;
  - persistence `action_reconciliation/{mod.rs, facts.rs, settlement.rs}`: the
    snapshot and settle branch for deliveries; remote branches unchanged;
  - the evidence commit guard (latest migration definition);
  - the resource policy port: messaging resource.
- **Delivery:** `delivery/` gains the `settle_unknown_delivery_on` helper; `begin_part`
  (§I); new failure classes.
- **CI scripts (replaces the D2 bullet):** `scripts/lease-budget.sh`:
  - `REQUIRED_OPS` gains `message.publish` and `message.consume`;
  - `FILTERS` gains `workflow_message_publish_` and `workflow_message_receipt_` only;
  - no threshold (`LEASE_BODY_BUDGET_PCT` or any bound) changes.
- **`message.publish` bound and leading locks (F011).** It runs under the request's
  `LeasePolicy` through `WriteBound` (the `authorize_on` path). Its leading locks are:
  - the `authorize_on` locks: company/principal, fence job/attempt, run, current
    owners, intent;
  - then the destination-owner locks: channels `FOR KEY SHARE`, members/principals
    `FOR SHARE`, in UUID order.

  `locks_held()` is called after the last of these. The work phase covers the replay
  check, the admission probe, the inserts and the immediate triggers. The deferred
  guards run in COMMIT, which is reported, not gated.

## K. Verification additions (extends D2 §8.1–§8.3)

- **§8.1 area H, reconciliation:**
  `cargo test --locked --offline --lib -- workflow_message_reconcile_ action_reconciliation mcp_reconciliation`.
- **§8.2 F010** (test-host ledger verifier through the real `ActionReconciliationService`):
  - applied on a live run resumes the step only after all destinations are accepted;
  - applied on a terminal run records truth without a successor;
  - final not-applied with an accepted sibling: only the subject is resent, with the
    same Message-ID and key, and the sibling's parts, attempts and provider calls are
    unchanged;
  - final not-applied with attempts exhausted gives `dead_letter`;
  - final not-applied on a cancelled run, and on a run past its deadline, supersedes
    the delivery with zero provider calls;
  - unknown leaves the delivery unchanged;
  - VerifiedReference without an installed verifier is refused;
  - non-admin, cross-company and wrong-association actors are refused;
  - stale snapshot (the delivery changed between snapshot and settle) is refused with
    no grant;
  - exact command replay; different bytes under the same key give IdempotencyConflict;
  - finality-breach conflict, after which a further reopen is refused;
  - proof expiry before `begin_part` returns the delivery to unknown with no call;
  - competing settle, consumer and `begin_part`, and settle racing the workflow
    cancellation, all marked `Injected`;
  - SQL-only negatives:
    - an exit from `outcome_unknown` with no evidence or reopen;
    - an exit from `delivered` or `dead_letter`;
    - a reopen from expired, non-final or foreign evidence;
    - a double consumption;
    - message evidence with coverage rows;
    - remote evidence with a delivery subject.
- **§8.2 F009:**
  - two SQL-only connections under the same fence at 127 existing markers: exactly one
    message marker commits and the other raises `54000`;
  - the deferred guard rejects a step-origin park whose attempt close cause is not
    `EffectParked`.
- **§8.2 F011:**
  - the lease-budget run records `message.publish` and `message.consume` events (the
    script fails if they are missing);
  - every deliberate-delay or contention test (paused producer from D2 F004, delayed
    COMMIT from D2 F007, the competing races above) runs on a current-thread runtime
    under `recorder::Injected::mark()`, is named with the `workflow_message_race_`
    prefix, and is outside the lease filters.

  The gate therefore cannot be pushed over its budget by construction.
- **§8.3 step 12** runs the updated `scripts/lease-budget.sh` unchanged otherwise. A
  failure is fixed by shrinking the operation, never by raising a threshold.

## L. Out of scope and obligations kept explicit (replaces D2 §9)

- Phase 05: approval settlement.
- Phase 06: saved-call suspend/resume for direct Pending; agent-execution recovery,
  including reopening a `model_tool`-origin agent execution after message receipts.
- Phase 08: production host composition and bypass removal.
- Phase 09: UI.
- A production email evidence verifier (none installed, as for MCP; the verified path
  and its tests are in 04.11).
- Groups (rejected by the user).
- Remote HTTP/MCP admission beyond 128 (pre-existing; observation only).
