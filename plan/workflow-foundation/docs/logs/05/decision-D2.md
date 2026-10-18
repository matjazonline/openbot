# Decision 05-D2 — phase 05 architecture gate, revision of D1 after 05-R001

Status: **proposed by A, awaiting B**. Author A (implement-opus-reviewed-2.1), 2026-10-18.

D2 **supersedes** [D1](decision-D1.md) (`4e3a46ebb03137ba5c86e1b3d64e1d3b0abf756c54eaa59efc8cee8301707931`).
It is an **explicit delta**: §1 gives every D1 section's status, D2 states each replaced section in
full, and D1 text not marked "kept" no longer applies.

- **Base.** HEAD `feefe3e` plus the accepted batch S tree (digest `03ca0bea…cbae`). Line numbers
  refer to this tree. Supporting evidence is in [investigation.md](investigation.md) (`484af70e…975e`,
  cited as I§n). Its §6 plan text is superseded by D2 §15.
- **Constraints.** No source edits and no cargo were run for D2; all checks were read-only code
  inspection (graft and grep). Schema changes edit
  `migrations/20260817000000_init_schema.sql` in place (plan 05:10-14; persistence AGENTS §1 as
  amended), with no follow-up migrations, backfills, shims or populated-upgrade tests.

## 0. Finding dispositions (05-R001)

| Finding | Severity | Disposition | Where |
| --- | --- | --- | --- |
| 05-F001 timeout indistinguishable in-graph | medium | **Fixed, choice (a).** `decision.reviewer_unavailable` fails the run (pending failure, never routed), so a decision's `error` route means only timeout or invalid input. The successor receives `null`, stated plainly; the false "data.map can map the code" claim is withdrawn. The timeout code is durable as the audit event `decision.timed_out` on the execution, together with the `expired` decision wait and, unrouted, the run's `terminal_execution_id`. Phase 07 obligation recorded; two 05A tests added | §4.3, §7.1, §7.3, §14 |
| 05-F002 drop external/credential design | medium | **Fixed.** Every external and credential item is removed, with no seams: no `kind` column, no credential column or table, no authority variant, no external lock steps or races. The reviewer item is only `{"kind":"user","id":<uuid>}`. Deferral and owner: the user decided on 2026-10-18 that email-link approvals are **dropped with the legacy approval code** (member-only), recorded in NOT_PLANNED. Every plan-text amendment is listed, including REPLACEMENT-MAP §4 and I§6. §7–§9 re-confirmed for members only | §2, §4, §5, §8, §9, §15 |
| 05-F003 notice transport and audit collision | medium | **Fixed.** Notices are standalone `notification` deliveries on the existing engine (`insert_standalone_delivery_on`), one per reviewer, enqueued **inside the 05A park transaction**. There is no new port, ledger, deliverer or scheduling. The `decision_notice_failed` run event is dropped: failure is per reviewer on the delivery row, linked from the reviewer row. Pending notices after settlement are left to send, which is harmless. Content rule kept | §10 |
| 05-F004 company row `FOR NO KEY UPDATE` | low | **Fixed.** `authorize_reviewer_on` takes `companies FOR SHARE` plus member ⋈ principal `FOR SHARE`. D1's §6 sentence is corrected. A test shows that commands on two runs of one company do not serialize | §8 |
| 05-F005 event bound and stale events | low | **Fixed.** Producers serialize per run on `pg_advisory_xact_lock` before counting, which makes the 128 bound exact; consumers never take that lock. Run-scope semantics are stated: correlation must carry the iteration identity. Competing-producers and stale-event tests added | §11 |
| 05-F006 05B direction-level | low | **Fixed.** The 05B decision addendum is §16, part of this gate. It resolves I§5.1–5.3 against the code, enumerates the one-wait lookups from `graft grep`, and decides approver eligibility: owner/admin at park, rechecked at submit. 05B code starts only after B accepts D2 including §16, and any deviation found in 05B returns to B first | §3, §16 |
| 05-F007 clarifications | low | **Fixed**, all five items. (1) A recursive static-root check over every `Binding` variant. (2) A deadline already past at park is `workflow.invalid_input` through the existing pending path, never a park. (3) The park-time failure writer is `pending_recovery::settle` with a new non-routed variant. (4) Plans 05–10 are added to `NO_BACKWARD_COMPATIBILITY_NOTES` (user-approved) as the first 05A step. (5) `validate_completion` is called from the new `application::workflow::decisions` module | §4.2, §7.1, §15, §7.2 |

## 1. D1 section status

| D1 § | Status in D2 |
| --- | --- |
| §0 summary | **Replaced** by §0 and §2–§16 |
| §1 scope and batches | **Replaced** by §3 |
| §2 language | **Replaced** by §4 |
| §3 schema | **Replaced** by §5 |
| §4 persistent ownership | **Kept**, except: the reviewer row carries `notice_delivery_id`; the audit list is §6 here; there are no credentials or notice rows |
| §5 transactions | **Kept** for §5.3 (comments), §5.6 (pointer to events) and §5.2 steps 1, 3, 4, 6, 8 and 9. **Replaced** for §5.1 (park), §5.2 steps 2, 5 and 7, §5.4 (timeout) and §5.5 (approval); see §7 and §16 |
| §6 concurrency | **Replaced** by §8 |
| §7 authorization | **Replaced** by §9 (members only) |
| §8 external credentials | **Removed** (Q6; user decision 2026-10-18) |
| §9 action approval | **Kept as direction** (accepted by B), detailed and partly replaced by §16 |
| §10 notification intents | **Replaced** by §10 |
| §11 events | **Kept**, amended by §11 |
| §12 failure codes and bounds | **Replaced** by §12 |
| §13 affected contracts | **Replaced** by §13 |
| §14 verification | **Replaced** by §14 |
| §15 notes list | **Replaced** by §15 |
| §16 open questions | **Closed** by the user answers (§2) and B's judgement (05-R001); none remain open |

## 2. User decisions incorporated

All were given directly to root on 2026-10-18 and are recorded in TASK.md.

- **Q1.** Reviewers are an explicit list of 1–16 company members; the first valid, authorized
  response wins.
- **Q2.** A timeout fails Terminal `decision.timed_out` over the declared `error` route; with no
  route, the run fails.
- **Q6.** No external (email-only) reviewers in phase 05; only company members review.
- **Q8.** Production wiring stays in phase 08. Phase 05 proves behaviour through direct calls and
  restart tests and does not claim production reachability.
- **Email-link approvals** (user decision, 2026-10-18).
  - Email-link approvals are dropped with the legacy approval code, and approvals become member-only.
  - Phase 08 removes the legacy approvals with no link replacement, so non-member `approver_email`
    approvals stop working.
  - REPLACEMENT-MAP §4 is amended accordingly (§15).
- **Notes list.** Plans 05–10 are added to `NO_BACKWARD_COMPATIBILITY_NOTES` as the first step of
  05A, together with the plan-text amendments.
- **Q3, Q4, Q5, Q7.** A's recommendations stand as B judged them: Q3 is re-decided per 05-F003 in
  §10; Q4, Q5 and Q7 are accepted as direction.

## 3. Scope and batches (replaces D1 §1)

**05A — decision core, member reviewers, notices and waits** covers 05.1, 05.2, the member part of
05.3, and 05.4. It is executed in this order:

1. **The first edit** (docs plus a one-place source list):
   - the plan-text amendments in §15;
   - `NO_BACKWARD_COMPATIBILITY_NOTES` gains `plan/workflow-foundation/05-…` through `10-…`, plus
     `BRIEF-05.md` if root asks for the brief.
2. **Language** (§4).
3. **Schema** (§5.1).
4. **Park, timeout, cancel and run expiry** for `decision.human` (§7), with **per-reviewer notice
   deliveries in the park transaction** (§10).
5. **Commands:** `SubmitDecision`, `AddComment` and `ReadDecision` for members (§7, §9).
6. **The run-scoped event inbox** (§11).
7. **Housekeeping:** help text, the example, and `pending_recovery`'s kind list.

**05B — action approval** covers the rest of 05.3, then the phase 05 gate. It is built per the §16
addendum and includes:

- the action `approval` input;
- the approval park in the dispatch seam;
- settlement that reopens the job;
- the approved-dispatch check;
- per-reason wait uniqueness.

**Why split there.**

- 05A is end-to-end verifiable on its own: a workflow with `decision.human` parks, notifies, is
  submitted, times out or is cancelled, and continues; events resume waits.
- 05A's end state is the stable contract 05B builds on: the decision model, the settlement
  transaction, the lock order, the command semantics and the park-time notice enqueue.
- With credentials gone and notices in 05A, 05B is purely the phase 04 dispatch-seam batch. B
  judged that seam right to isolate under a focused review.

## 4. Language (replaces D1 §2)

### 4.1 `decision.human`

| Input | Rule |
| --- | --- |
| `proposal` (required, any JSON) | The reviewed artifact |
| `reviewers` (required) | Replaces `reviewer`. An array of 1–16 **unique** items, each exactly `{"kind":"user","id":<uuid>}`, the same shape and id semantics as 04.11 destinations (`registry/actions.rs:119`). `group` and slug ids are removed (`registry/schemas.rs:24-29`). No other kind exists |
| `deadline` (required, RFC3339) | Effective deadline is `min(deadline, run.deadline)` (see §4.2 for park-time handling) |
| `data_schema` (literal) | Unchanged |
| `feedback_required` (literal) | Unchanged |

- **Routes.** `choices` is required, and `error` is optional (§4.3).
- **Submitted output** is unchanged: `{choice, data, feedback}`, all required. Feedback may be empty
  unless the choice is listed in `feedback_required`.

### 4.2 The static-reviewer rule, and the deadline at park (05-F007 items 1 and 2)

- **Static reviewers.** A new registry check walks the `reviewers` binding tree **recursively** over
  every `Binding` variant (`domain/workflow/context/mod.rs:17-40`).
  - Variants: `Literal`, `Reference`, `Object`, `Array`, `Default{reference, fallback}`, `Concat`,
    `Compare{left, right}`, `In{value, items}`, `Exists`, `And`, `Or`, `Not`.
  - It accepts only `Literal` leaves and references whose root is `Params`
    (`ContextReference::root_name()`, `context/mod.rs:109`; roots at `:59-65`). Any `Input`, `Step`,
    `RunId` or `ParentRunId` reference anywhere is a compile diagnostic.
  - The walk is a domain helper on `Binding` (it visits every reference), so the registry does not
    re-implement the tree.
  - Run `params` are the admin-configured binding snapshot (B verified this:
    `admission_binding.rs:44-70`), so assignment stays deterministic and out of the model's reach.
- **Deadline at park.** A deadline that is unparsable, or no longer in the future at park time,
  never parks. It settles through the existing pending path as `workflow.invalid_input`
  (`pending_recovery::settle(InvalidInput)`, `pending_recovery.rs:47-96`).
  - That variant routes when the step is activated and declares `error`, otherwise it fails the run,
    exactly as for other invalid inputs.
  - This is the same handling as today's `wait.event` deadline check (`waits.rs:190-194`).

### 4.3 What a decision's `error` route carries (05-F001)

- **The writer.** `route_failure` commits `FinalError` with `output: null` (`recovery.rs:212-219`),
  and no binding root carries a failure code. An error-route successor of a decision therefore
  receives **`null` and no code**.
- **What reaches the route.** Under choice (a), a `decision.human` error route is reached only by:
  - **`decision.timed_out`** (§7.3);
  - **`workflow.invalid_input`**: invalid frozen inputs, or a deadline already past at park (§4.2).
  `decision.reviewer_unavailable` never routes; it fails the run (§7.1). In-graph, the error route
  means "timed out, or the decision could not be opened from its inputs", and the two cannot be told
  apart there.
- **Where the code is durable.** The audit event `workflow_run_events.event_kind =
  'decision.timed_out'` is written for that execution (one per execution, matching
  `workflow_run_event_execution_kind`). The failure code is the event kind, as `settle` does today
  (`pending_recovery.rs:88`). The decision wait is `expired` and the execution is committed
  `final_error`; with no error route, the run is `failed` with `terminal_execution_id` set to the
  decision execution.
- **Phase 07 obligation** (recorded for plan 07; not a phase 05 deliverable):
  - The draft-and-review child's `choice: "timeout"` and the parent's "timeout: declared timeout
    route" (`07-…:52-63`) must come from the **child run's terminal failure code**.
  - The child's decision declares no `error` route, so a timeout fails the child with
    `terminal_execution_id` pointing at the decision execution and the `decision.timed_out` audit
    event on it.
  - 07.1's durable child completion must carry that code to the parent; alternatively, 07 makes a
    language change. It cannot come from `data.map`, which receives `null`.
- **05A tests.**
  - `workflow_decision_timeout_error_route_successor_receives_null`: the successor runs with `null`
    input from the decision.
  - `workflow_decision_timeout_without_error_route_fails_run`: the run is `failed`,
    `terminal_execution_id` is the decision execution, and the audit event `decision.timed_out` is
    present.

### 4.4 Help, example, and the action `approval` input (05B)

- **Help text.** `help.rs:71-73` becomes "Durable review deadline required; never automatically
  accepts. Reviewers are an explicit static list of company users; first valid authorized submission
  wins. Timeout fails Terminal decision.timed_out through the declared error route."
- **Example.** `examples.rs:87-89` uses `"reviewers":[{"kind":"user","id":"00000000-0000-0000-0000-000000000006"}]`.
- **The action `approval` input** (05B, §16): `approval: {reviewers: <same schema and static rule>,
  deadline: <RFC3339 text>}`, optional on `http.request`, `tool.call`, `mcp.call`, `message.send`
  and `message.reply`.
- **Disclosure** is unchanged from D1: message `content` fields are strings, so mapping the decision
  object fails at compile time. A compile test pins it.

## 5. Schema (replaces D1 §3)

### 5.1 05A

**`workflow_decisions`.**

```text
company_id, run_id, execution_id, id uuid; wait_id uuid
subject_kind text CHECK IN ('business','action_approval')        -- 05A writes 'business' only
proposal_digest text NULL CHECK ^[0-9a-f]{64}$                    -- business
action_invocation_id uuid NULL, argument_digest text NULL, operation_digest text NULL  -- 05B (§16)
revision bigint NOT NULL DEFAULT 1 CHECK > 0
created_at timestamptz DEFAULT clock_timestamp()
submitted_at, submitted_by (principal), command_key (1..128), command_fingerprint (^[0-9a-f]{64}$),
choice (1..140), feedback (≤ 65536 B), data jsonb (≤ 1 MiB)
CHECK subject shape (business ⇔ proposal_digest set and action fields NULL; approval ⇔ the reverse)
CHECK submission: all seven submission columns NULL, or all set
PK (company_id, id); UNIQUE (company_id, run_id, execution_id)
FK (company_id, run_id, execution_id, wait_id) → workflow_waits(company_id, run_id, execution_id, id) ON DELETE CASCADE
FK (company_id, submitted_by) → principals(company_id, id)
```

- The approval subject columns are created in 05B together with their FKs (§16); 05A ships
  `subject_kind` with the single value it writes.
- Under no backward compatibility, 05B edits the CHECK and adds the columns in place, so 05A carries
  no unused nullable columns. *(This replaces D1's 05A list.)*
- Trigger `workflow_decision_immutable` allows only the one no-submission → submission transition,
  with `revision` going from 1 to 2.

**`workflow_decision_reviewers`.**

```text
company_id, decision_id, ordinal smallint (0..15), principal_id uuid, notice_delivery_id uuid
PK (company_id, decision_id, principal_id); UNIQUE (company_id, decision_id, ordinal)
FK (company_id, decision_id) → workflow_decisions ON DELETE CASCADE
FK (company_id, principal_id) → principals(company_id, id)
FK (notice_delivery_id) → message_deliveries(id) ON DELETE SET NULL
```

- The rows are immutable and are written in the park transaction.
- There is no `kind` column (05-F002): every reviewer is a member's person principal.
- `notice_delivery_id` is set in the park transaction. The FK cannot include the tenant, because
  standalone deliveries have `company_id` NULL by design (init 5036). It follows the
  `notifications.email_delivery_id` precedent (`ON DELETE SET NULL`, init 12265). Immutability
  applies to every other column.

**`workflow_decision_comments`.** Kept from D1 §3.1 unchanged.

**`workflow_wait_events`, made run-scoped.** Kept from D1 §3.1 (the producer lock is §11).

**`workflow_waits`.** Kept from D1 §3.1: the run-scoped `consumed_event_id` FK, and
`check_workflow_wait_commit` extended to `decision` plus the consumed-event equality check.

**Removed from D1:** `workflow_decision_credentials`, `workflow_decision_notices` and
`submission_credential_id`.

### 5.2 05B

These are specified in §16.

- **The approval subject on `workflow_decisions`:** the three columns, the CHECK value
  `action_approval`, and the FKs to `workflow_action_intents`, binding both digests.
- **`workflow_waits`:** per-reason uniqueness plus a partial unique "one `waiting` per execution".
- **`task_attempts.stop_reason`:** `approval_parked`.
- **`check_workflow_fenced_completion`:** an approval-park branch.

## 6. Persistent ownership (D1 §4 kept, with these edits)

- **Lifecycle and submission.** The wait row is the lifecycle owner, and the decision row holds the
  frozen assignment and the single submission. Both are kept from D1. The artifact identity is
  `(decision id, proposal_digest)`.
- **Audit events** in `workflow_run_events` are each unique per execution and kind:
  - `decision_opened`;
  - `decision_submitted`;
  - **`decision.timed_out`** (the failure code as kind, §4.3);
  - **`decision.reviewer_unavailable`** (written by `settle`, §7.1);
  - `workflow.invalid_input` (existing);
  - in 05B, `action_approved` and `action_rejected`.
  - **`decision_notice_failed` is dropped** (05-F003): a per-reviewer outcome lives on the delivery
    row.
- **Notices.** The engine's `message_deliveries` row owns delivery truth: status, attempts, failure
  class, dead letter. The reviewer row's `notice_delivery_id` links each reviewer to theirs.

## 7. Transactions (replaces D1 §5.1, §5.2 steps 2, 5 and 7, and §5.4)

### 7.1 Park (05-F007 item 3)

The worker dispatches `decision.human` to the new port method `park_decision(scope, &DecisionNotices)`
(§10), followed by `resume_wait`. `is_wait` includes `decision.human`.

1. Run `waits::lock` (run → execution → job), then replay an existing wait.
2. Check the run state and deadline.
3. Activate with `pending_recovery::activate`; invalid input settles there.
4. Read the spec from the frozen inputs. A deadline that is unparsable or not in the future becomes
   `settle(InvalidInput)`, and nothing is parked (§4.2).
5. Resolve the reviewers with no locks (§9.3). If any listed reviewer is unresolvable or ineligible,
   call **`pending_recovery::settle(PendingFailure::ReviewerUnavailable)`**:
   - a new variant with code `decision.reviewer_unavailable`, **not** in `settle`'s routed set
     (`pending_recovery.rs:66-76`);
   - so the run is set `failed` with `terminal_execution_id`, the pending job `failed`, and an audit
     event with that code;
   - nothing is parked, and `recovery::route_failure` (the `Recovered` completion owner) is never
     used here.
   This is choice (a) of 05-F001: a configuration failure is never presented as a business timeout.
6. Insert the wait (`reason='decision'`, `deadline = min(input, run)`, registration intent), then the
   decision, then the reviewers.
   - Then, per reviewer, compose and insert the notice delivery and set `notice_delivery_id` (§10).
7. Run `completion_job::finish(Pure)`, set the run to `waiting/decision`, and audit
   `decision_opened`.

### 7.2 SubmitDecision, the changed steps (05-F007 item 5)

- **Step 2, locks.** The authority is `authorize_reviewer_on` (§8, §9.1), then the run lock, the
  association, execution → job, the wait `FOR UPDATE`, and the decision `FOR UPDATE`.
- **Step 5, reviewer check.** The actor's person principal must be one of the frozen reviewers;
  otherwise `NotFound`.
- **Step 7, validation.** The new module `application/workflow/decisions.rs` exposes
  `pub(crate) fn prepare_submission(bundle, activation, submission) -> AppResult<PureCompletion>`.
  - It checks that the choice is declared, builds `{choice, data, feedback}`, checks it against the
    run's context limits, and calls `validate_completion(bundle, activation, output,
    RouteSelection::Choice(choice))`.
  - `validate_completion` is `pub(super)` in `application/workflow/batch.rs:115`, which makes it
    visible to every module under `application::workflow`.
  - This mirrors `waits::prepare` (`application/workflow/waits.rs:117-124`), which already calls it.
    The persistence settlement calls `decisions::prepare_submission`. On `BadRequest` the
    transaction rolls back and the decision stays open.
- **Unchanged from D1:** steps 1 (read without lock), 3 (replay), 4 (open check), 6 (stale check),
  8 (the writes, with `batch_commit::complete(Parked)` boxed) and 9 (the deferred commit guard).

### 7.3 Timeout (replaces D1 §5.4)

In `resume_on`, for a `decision` wait whose deadline has passed while the run deadline has not:

1. set the wait to `expired` and the run to `running`;
2. audit **`decision.timed_out`**;
3. call `route_parked_failure`, the renamed `route_effect_failure` (`recovery.rs:292-334`), with
   Terminal `decision.timed_out`. It takes the declared `error` route, so the successor receives
   `null` (§4.3), or fails the run and sets `terminal_execution_id`.

It is woken by the existing candidate clause `wait.deadline <= clock_timestamp()`. `saved` is
generalized to all committed routes.

- **Cancel and run expiry** are unchanged: `cancel_on` and `maintenance::expire_on`.
- **Retry.** A routed or unrouted timeout is not retryable (`workflow_control_retry_safe` requires a
  failed attempt); a test pins this.

## 8. Concurrency (replaces D1 §6; 05-F004)

**Lock order:**

1. `companies FOR SHARE`, then `company_members ⋈ principals FOR SHARE`, both in
   `authorize_reviewer_on`;
2. `workflow_runs FOR UPDATE`;
3. association rows (channel, grant, thread `FOR SHARE`, `association.rs:6-61`);
4. execution, then job `FOR UPDATE`;
5. wait `FOR UPDATE`;
6. decision `FOR UPDATE`.

**What the company lock serializes**, correcting D1's sentence:

- **Controls and dispatch** take the company row `FOR NO KEY UPDATE` (`authorize_company`,
  `authority.rs:14-19`; dispatch through `action_authority.rs:35-52`).
- **Decision commands** take it `FOR SHARE`. They therefore wait behind a control or dispatch that
  holds the company row, and a control or dispatch waits behind them. Decision commands do **not**
  serialize with one another, because share locks are mutually compatible. They also stay compatible
  with the workers' FK `KEY SHARE`.
- **Owner and role changes** need `FOR NO KEY UPDATE` or a row update or delete. They stay blocked
  by these shared locks for the duration of a decision command, so the authority check cannot go
  stale before commit.
- **No cycle is possible**, because every path that takes the company row takes it first. Workers
  (park, timeout, resume) take the run first and read authority without locks at park.

**Races** (D1's table, members only):

| Race | Outcome |
| --- | --- |
| Submit vs submit (two reviewers, or two keys) | One `Applied`, the other `Conflict`; exactly one successor |
| Same key, same payload | `Replayed` |
| Same key, different payload | `Conflict` |
| Submit vs timeout | The first commit wins; a submit crossing the deadline is refused under the lock or by the deferred guard |
| Submit vs cancel | The first commit wins |
| Comment after settlement | `Conflict` |
| Event producers | Per §11 |
| Approve vs approval expiry vs cancel (05B) | Run lock; the first commit wins |
| Notice delivery claims | The delivery engine's own lease protocol (04), unchanged |

**Removed from D1:** the credential race, the notice-deliverer race, and the external lock steps.

**Competing-claimant tests** (root `AGENTS.md`):

- **05A:**
  - two reviewers submitting;
  - submit vs timeout;
  - submit vs cancel;
  - **N event producers at the 127/128 bound** (§11);
  - **two runs in one company**: run A's submission blocks behind a held run lock while run B's
    submission commits (05-F004).
- **05B:** approve vs expiry.

## 9. Authorization (replaces D1 §7; members only)

### 9.1 Submission

1. `authorize_reviewer_on` (`companies FOR SHARE`; member ⋈ person principal `FOR SHARE`; any
   role) returns the principal, the role and whether the actor is the owner.
2. The run lock.
3. `authorize_association` for the run's channel or thread. A channel-less run requires owner or
   admin.
4. The principal must be one of the frozen reviewers.

Missing, foreign or inaccessible records return `NotFound`. A revocation conflicts with the shared
locks: it is either visible to the check or waits until the settlement commits.

### 9.2 Comments and reads

Comments and reads use steps 1–3; any member with run visibility may comment or read. Comments and
feedback are visible only under run/thread visibility.

### 9.3 Park-time eligibility

There are no locks; submission rechecks. A reviewer `user` must:

- be a company member with a person principal;
- be able to see the run's association, by the `reviewer_is_eligible_on` semantics
  (`response_review.rs:375-413`, which B found consistent with `authorize_association`);
- be owner or admin on a channel-less run.

Anything else means `decision.reviewer_unavailable` (§7.1).

### 9.4 Cross-company access

Every statement is keyed by `company_id`. A decision id used under another company returns
`NotFound`.

**Re-confirmation (05-F002).** With members only, §7–§9 hold: authority is `authorize_reviewer_on`
plus the association and nothing else. No path takes a lock outside the §8 order.

## 10. Notices on the delivery engine (replaces D1 §8 and §10; 05-F003)

- **Mechanism.** In the park transaction (§7.1 step 6), for each reviewer:
  1. read the member's account address (`users.email`, init 5978-5981);
  2. compose a standalone `notification` delivery;
  3. insert it with `delivery::enqueue::insert_standalone_delivery_on(tx, …)`
     (`delivery/enqueue.rs:144-175`);
  4. store the resulting `Created`/`Absorbed` id in `workflow_decision_reviewers.notice_delivery_id`.
- **Precedent and atomicity.** This is exactly how the notification projection enqueues its email
  inside its own transaction (`notification.rs:995-1004`). The notice is durable and atomic with the
  open decision. Park is replay-safe: a re-park returns the existing wait before any insert, and the
  idempotency key absorbs duplicates anyway.
- **Composition.** The new concrete type `application::workflow::decisions::DecisionNotices` (no
  trait, one implementation) wraps the existing `DeliveryComposer` and `Arc<AppConfig>`. It renders
  through `DeliveryComposer::compose_standalone` (`application/transport/compose.rs:191-222`), as
  `NotificationWorker::compose_email` does (`notification_worker.rs:167-211`).
  - Rendering is pure (no I/O), so it is safe inside the transaction. A rendering error is internal:
    the park rolls back and is retried, and a test pins that deterministic content renders.
  - The `WorkflowWorker` holds `DecisionNotices` and passes it to `park_decision`. This adds no new
    port trait, ledger, deliverer or scheduling.
- **The delivery's fields.**

  | Field | Value |
  | --- | --- |
  | Purpose | `DeliveryPurpose::Notification` |
  | From | `notifications@{app_domain_name}`, as the precedent does |
  | To | The member's address, with standalone threading and no relay |
  | `correlation_id` | `workflow_runs.correlation_id` |
  | `source_key` | `workflow-decision:{decision_id}:principal:{principal_id}` |
  | Link | `{AppConfig::public_base_url()}/ui/workflow-decisions/{decision_id}` (`infra/config.rs:534-542`, derived from `APP_DOMAIN_NAME` and `secure_cookies`) |

  The authenticated route is phase 08/09; the link is a page reference, not a credential.
- **Content rule** (kept). The subject names the workflow and step. The body gives the company (and
  channel, if any), the deadline, the decision id and the link. There is **no proposal and no
  comments**.
- **Delivery failure** is the engine's own state (attempts, failure class, dead letter) on that
  reviewer's row. It never approves and never changes the decision. There is no run event, which
  fixes the unique-index collision with `workflow_run_event_execution_kind`.
- **After settlement**, a pending notice is not cancelled. A late email that links to a settled
  decision is harmless (B accepts this): the page shows the decision as settled.
- **Notification preferences** are not consulted, because an assignment is explicit and time-bound.
  Preference integration and in-app queues are phase 09 projections over the decision rows.
- **Replies.** Inbound replies to a notice are ordinary ingress, and no producer matches decision
  waits (§11), so a reply never settles anything (R9).

## 11. Event inbox amendments (D1 §11 kept, amended; 05-F005)

- **An exact bound under concurrent producers.**
  1. `record_event` first takes
     `pg_advisory_xact_lock(hashtextextended('workflow_wait_events:' || company_id || ':' || run_id, 0))`.
     The precedent is `enqueue_actionable_notification_event`, init 734-790.
  2. Then, in fresh statements under READ COMMITTED, it checks the run, deduplicates the id, counts
     the unconsumed events and inserts.
  3. Producers to one run are therefore strictly serialized, and each sees the previous producer's
     committed row. The count of unconsumed events can only overestimate, never underestimate, so
     at most 128 unconsumed events are ever stored; the 129th is a `Conflict`.
  - **Consumers** (`resume_on`/`advance`) never take the advisory lock. A consumption committing
    concurrently only lowers the count.
  - **Blocking.** The producer's insert takes the FK `KEY SHARE` on the run row, so it waits at most
    for one worker transaction (≤ 1000 ms timeouts). No worker takes the advisory lock, so there is
    no cycle, and producers to different runs never block one another. A hash collision only
    over-serializes.
  - **Test** (`workflow_wait_event_bound_competing_producers`): with 127 unconsumed events, 8
    producers behind a barrier yield exactly 1 `Recorded` and 7 `Conflict`, and the final unconsumed
    count is 128.
- **Run-scope semantics, now stated.**
  - An unconsumed event in a run satisfies the **next** `waiting` wait in that run with the same
    event name and correlation, regardless of when it arrived. Arrival before the waiting execution
    exists is the point (event-before-park). Events are consumed oldest first (`created_at, id`).
  - A workflow that waits for the same event name more than once in a run **must put the iteration
    identity into the correlation**, for example the triggering send's message id. Otherwise a stale
    event satisfies a later wait. Execution scoping used to prevent this; run scoping deliberately
    does not.
  - Unconsumed events stay until the run is gone, and they count against the bound.
  - **Test** (`workflow_wait_event_stale_event_matches_only_its_correlation`): three sequential waits
    with one event name and correlations `x`, `y`, `x`. A surplus `x` event recorded during the first
    wait is not consumed by the `y` wait and is consumed by the third, `x`, wait.
- **Everything else in D1 §11 is kept:** literal event names with one schema per name; ingress
  results `Recorded`/`Duplicate`/`Conflict`/`Unmatched`/`BadRequest`; candidate wake on unconsumed
  matching events restricted to `reason='event'`; atomic single consumption; decision waits never
  matched; the old execution-scoped signal API replaced.

## 12. Failure codes and bounds (replaces D1 §12)

**Terminal codes.**

| Code | Behaviour |
| --- | --- |
| `decision.timed_out` | Routed over `error`, else the run fails |
| `decision.reviewer_unavailable` | Never routed; the run fails |
| `<family>.approval_rejected`, `<family>.approval_expired` (05B) | Routed per the action failure writer |
| `<family>.approval_required` | Keeps its meaning (§16) |

**Bounds.** Each gets an at-limit and an over-limit test:

| Bound | Limit |
| --- | --- |
| Reviewers | 16 (so at most 16 notice deliveries per park) |
| Comments | 1000 per decision, 16384 characters each |
| Feedback | `MAX_TEXT` |
| Data | The run's context limits |
| Events | 128 unconsumed per run, exact (§11); payload 1 MiB; name 128; correlation 256 |
| Notice delivery attempts | The engine's existing `MAX_DELIVERY_ATTEMPTS`, unchanged |

No existing limit is raised. **Removed:** the credential bound, the notice-attempt bound and the
notice batch bound.

## 13. Affected contracts (replaces D1 §13)

### 05A

- **Docs:** the §15 plan files.
- **Notes list:** `src/main.rs` (`NO_BACKWARD_COMPATIBILITY_NOTES`, list entries only).
- **Registry:** `registry/families.rs`, `schemas.rs`, `help.rs`, `examples.rs`, plus registry
  tests.
- **Domain:** the reference walk on `Binding` (`domain/workflow/context/mod.rs`).
- **Compiler:** the static-root diagnostic and the event-name/schema rule, plus compile tests.
- **Worker:** `worker.rs` (`decision.human` → `park_decision`, holding `DecisionNotices`).
- **Application:** `application/workflow/waits.rs` (event port; route selection) and the new
  `application/workflow/decisions.rs` (commands, `prepare_submission`, digests via canonicalization
  shared from `actions/freeze.rs`, `DecisionNotices`).
- **Persistence:**
  - `persistence/workflow/waits.rs` (`park_on`/`park_decision`, `is_wait`, `record_event` with the
    advisory lock);
  - `wait_commit.rs` (decision branch, generalized `saved`, event consumption);
  - `polling.rs`;
  - `pending_recovery.rs` (the `ReviewerUnavailable` variant; `decision.human` in the kind list at
    `:141-144`);
  - `recovery.rs` (the rename);
  - new `persistence/workflow/decisions.rs` and its tests.
  - It calls `delivery::enqueue::insert_standalone_delivery_on`, which is already `pub`.
- **Schema:** the init schema.
- **Pinned bodies:** the `workflow/tests.rs` function-body pins, if they cover the changed guard.
- **Event-port test moves:** `wait_guard_tests.rs`, `control_wait_order_tests.rs`, `wait_tests.rs`,
  `wait_deadline_tests.rs`, and the polling and fairness tests.
- `graft build`.

### 05B

The dispatch seam, the action failure codes, the registry `approval` input, the wait uniqueness
lookups, the fenced-completion guard and the attempt stop reason, all enumerated in §16. There are
**no** email, credential or notice modules.

## 14. Verification plan (replaces D1 §14)

**Environment and logging** are as in D1: `SQLX_OFFLINE=true`, `CARGO_BUILD_JOBS=4`, and the fixture
URL. After each init edit, the fixture databases are dropped and recreated and the migration is run.
Results are logged under `logs/05/`, and a check that cannot run is reported, not claimed.

### 05A focused checks

| Change | Command (`cargo test --locked --offline --lib …`) |
| --- | --- |
| Language | `registry::`, `compiler::` (static-root walk, reviewer shape, event-name rule, disclosure compile test) |
| Decisions | `workflow_decision_` (park, submit, comment, timeout, cancel, notices, races, auth, stale, invalid, restart) |
| Waits and events | `workflow_wait_`, `wait_guard`, `wait_deadline`, `control_wait_order` |
| Candidate SQL | `polling`, `fairness` |
| Delivery-engine regression | `delivery::`, `notification` (shared enqueue) |
| Shared resume | `workflow_message_` |
| Init pins | `adapters::persistence::workflow::tests` |

Each step also runs fmt, `git diff --check` (excluding the reviewer-owned file), strict Clippy, and
`cargo sqlx prepare --check` after schema edits.

**05A batch gate:** `--lib adapters::persistence::workflow` and `--lib application::workflow` under
`scripts/test-network-isolation.sh`.

### 05B focused checks

`workflow_action_approval_`, plus the seam regressions:

- `workflow_action_dispatch`, `workflow_http`, `workflow_mcp`, `workflow_message_`;
- `action_receipt`, `action_reconciliation_runtime`;
- `lease_`, `completion_` (the fenced-completion guard).

**05B batch gate:** the same workflow subsets.

### Final phase 05 gate

As in D1: fmt, diff, the transport and credential/deploy script tests, offline check, strict Clippy,
the fresh-schema dump diff, `sqlx prepare` and `--check`, `--no-run`, the network-isolated full
suite, the stack and lease budgets, and `graft build`, all recorded in `logs/05/gate-status.log`.

### Acceptance mapping (plan 05:87-92, line 89 as amended in §15)

| Acceptance | Tests |
| --- | --- |
| Comments don't advance; feedback only where mapped | `workflow_decision_comment_*`; the compile disclosure test |
| Competing reviewers, duplicate submissions, timeout/submit, cancel/submit | `workflow_decision_race_*`, including the two-runs-one-company test |
| Revoked membership, non-member and non-reviewer refusal, cross-company, stale artifacts | `workflow_decision_auth_*` (revocation in both orders; a non-member and a member who is not a reviewer get `NotFound`), `workflow_decision_cross_company_*`, `workflow_decision_stale_*` (digest, revision, late round on a settled decision) |
| Invalid edit, missing feedback or unknown choice fail without settlement | `workflow_decision_invalid_*` |
| Timeout routing | `workflow_decision_timeout_error_route_successor_receives_null`, `workflow_decision_timeout_without_error_route_fails_run`; `workflow_decision_reviewer_unavailable_fails_run_unrouted`; `workflow_decision_past_deadline_at_park_is_invalid_input` |
| Durable precise notices; failure never approves | `workflow_decision_notice_*`: one delivery per reviewer committed with the park and absent when the park rolls back; content has no proposal or comments; a dead-lettered delivery leaves the decision open; a late delivery after settlement is harmless |
| No approved action dispatches different arguments | `workflow_action_approval_digest_*` (05B) |
| Restart and event-before-park | `workflow_wait_event_before_park_*`, `workflow_wait_event_bound_competing_producers`, `workflow_wait_event_stale_event_matches_only_its_correlation`, `workflow_decision_restart_*` |

## 15. Plan-text amendments and the notes list (05-F002, 05-F007 item 4)

These are applied as the first 05A edit. They supersede I§6.

**The phase 05 plan, `05-human-decisions-and-waits.md`:**

1. **Line 18.** "an authorized reviewer or reviewer group" → "an explicit list of 1–16 company members
   as reviewers".
2. **Lines 63-64.** "Support an explicit reviewer or eligible company reviewer group, with the first
   valid authorized response settling the task." → "Reviewers are an explicitly named list of company
   members — no reviewer groups, no membership-derived pools and no external reviewers; the first
   valid authorized response settles the task."
3. **Lines 68-69.**
   - Delete "Support existing email-only reviewers through scoped, expiring, single-use decision
     credentials. Links and authenticated routes use the same decision command."
   - Insert "Reviewers act only through authenticated member routes; email-only (non-member)
     reviewers and link credentials are not supported (user decision 2026-10-18)."
   - Keep the rest of the paragraph (durable precise notification intents; replies never settle;
     delivery failure never approves).
4. **Line 89 (acceptance).** "Test revoked membership, external decision credentials, cross-company
   access, and stale artifacts." → "Test revoked membership, non-member and non-reviewer refusal,
   cross-company access, and stale artifacts."

**EXPANSION, `EXPANSION.md:323` (05.3):**

5. "Deterministic eligible reviewers, current access, expiring single-use external credentials,
   durable precise notifications; …" → "Explicit member reviewers (no groups, no external
   reviewers), current access, durable precise notifications; action digest subject separate from
   business review. No eligible reviewer fails."

**NOT_PLANNED, `NOT_PLANNED.md`:**

6. **Line 17.** "One assigned reviewer or eligible group with first valid response." → "An explicit
   list of member reviewers with first valid response."
7. **A new row.** "Email-only (non-member) reviewers and email-link decision or approval credentials |
   Dropped with the legacy approval code (user decision 2026-10-18). Decisions and approvals are
   member-only; phase 08 removes legacy `approver_email` approvals with no link replacement, so
   non-member approvals stop working."

**REPLACEMENT-MAP §4, `REPLACEMENT-MAP.md`:**

8. **Row 72**, middle column. "deterministic eligible approver, frozen reviewed operation/artifact,
   scoped single-use link, deadline, response and audit" → "deterministic member approver
   (owner/admin, rechecked at submit), frozen reviewed operation/artifact, deadline, response and
   audit".
9. **Row 72**, gate. "05, 08. Move pending/answered links and restart-safe decisions before removing
   harness-specific approvals." → "05, 08. Move restart-safe member decisions before removing
   harness-specific approvals; legacy email-link approvals, including non-member `approver_email`,
   are removed with no replacement (user decision 2026-10-18)."
10. **Row 74.** "Link and queue pages project the human decision" → "Queue pages project the human
    decision", and "preserve notification atomicity and token access tests" → "preserve
    notification atomicity and member authorization tests".

**Investigation and briefs:**

11. **Investigation §6.** Superseded by this list. `investigation.md` stays unchanged as a hashed
    record.
12. **`BRIEF-02.2.md:84`.** Left as history; this list records the supersession.

**The notes list (user-approved).**

- `src/main.rs` `NO_BACKWARD_COMPATIBILITY_NOTES` gains `plan/workflow-foundation/05-human-decisions-and-waits.md`,
  `06-agent-context-memory-and-capabilities.md`, `07-child-workflows-and-review-revisions.md`,
  `08-triggers-and-application-replacement.md`, `09-authoring-and-operations-ui.md` and
  `10-verification-and-cutover.md`.
- Each carries the pre-release note at line 10.
- `BRIEF-05.md` is added too if root asks for a brief; it then opens with the note.

## 16. 05B decision addendum (05-F006)

This addendum was checked against the code at this base. 05B code starts only after B accepts it.
An implementation-time deviation from it returns to B before the code that depends on it.

### 16.1 Who may approve

- **Eligibility.** Approvers declared in the step's `approval.reviewers` must be **owner or admin**
  at park and must be able to see the run's association.
  - At **submit**, `authorize_reviewer_on` (§8) is rechecked and requires role owner/admin, plus
    `authorize_association` and reviewer membership.
  - The reason: the protection comes from admin-level operator policy rows
    (`workflow_http_operations`, `workflow_mcp_operations.approval_required`, `mcp.rs:100-113`), so
    only an admin may lift it for one invocation.
- **Fail closed.** A protected call that declares no `approval`, whose approvers are not all
  eligible, or that has no attempt left (§16.2), keeps today's Terminal
  `<family>.approval_required` through the action failure writer (`actions/failure.rs:66-90`).
  Nothing parks.

### 16.2 Reopening a parked job (I§5.1, resolved)

- **The park.** It happens in the fenced dispatch transaction: the attempt is closed
  `approval_parked` and the job goes `processing` → `completed`.
  - The deferred `workflow_fenced_completion_guard` (`check_workflow_fenced_completion`, init 215-243,
    trigger 11192) today admits that transition only for a committed route or an `effect_parked`
    effect wait.
  - It gains a third branch: the attempt is `completed`/`approval_parked`, the run is
    `waiting/decision`, and a `waiting` wait with `reason='decision'` exists whose
    `workflow_decisions` row has `subject_kind='action_approval'` for this execution.
  - `task_attempts.stop_reason` gains `approval_parked` (init 5577).
  - The supervisor's suspension check gets an approval counterpart to `confirm_suspension`
    (`message_receipts.rs:357-384`).
- **Approve reopens** the job `completed` → `pending` with `run_at = now` and `retry_count`
  unchanged (the precedent is `control_retry.rs:37`).
  - `workflow_action_capture_schedule` (init 2974) fires only on `failed` → `pending`, so it writes
    no witness.
  - `workflow_action_confirm_claim_retirement` (init 3171) fires only on `pending` → `failed`.
  - Neither blocks nor needs to fire.
  - With no witness, `workflow_action_pending_claim_episode` is NULL, so
    `reconciliation_budget_eligible` passes (`lease_claim.rs:180-215`).
- **Attempt numbering.**
  - A claim takes attempt `retry_count + 1` and requires `retry_count < max_retries`
    (`lease_claim.rs:43, 64-69`).
  - The park attempt has consumed one. **Precondition at park: `retry_count < max_retries`**, so an
    attempt remains; otherwise Terminal `approval_required` with no park.
  - The approved dispatch is therefore the next attempt, and the `exhausted` candidate (`lease.rs:410`)
    cannot fire on a reopened job.
  - On re-claim, `pending_recovery::activate` returns the existing activation, and
    `actions.rs::prepare` (17-47) returns the existing intent `replayed`, with the same invocation and
    digests because the frozen inputs are immutable.

### 16.3 The intent commits before the park (I§5.2, resolved)

- `ActionService::prepare_tool` → `persistence::workflow::actions::prepare` commits the intent in its
  own transaction (`actions.rs:22, 45`) before any `dispatch_*`.
- Every family goes through it: `http_runtime.rs:62`, `mcp_runtime.rs:108`, `message_runtime.rs:68`.
- So the approval decision's FK to `workflow_action_intents` is satisfiable inside the dispatch
  transaction.
- **The protected branch changes in exactly these places**, from `End::Rollback(…ApprovalRequired)` to
  a committing `End::Commit(ApprovalParked)` when, and only when, §16.1 allows a park:
  - `action_dispatch.rs:197` (local);
  - `action_dispatch.rs:404-408` (`reserve_remote`);
  - `message_publish.rs:301` (`publish_on`);
  - `message_publish.rs:433` (`observe`).
  Otherwise the rollback and Terminal stay as today.
- **MCP.** The session open is skipped while the call is protected (`mcp_runtime.rs:155, 251`).
  `mcp_use` (`mcp.rs:296-332`) gains the approval lookup's result, so a protected call with a
  matching approval opens the session. The authoritative check stays in `authorize_prelocked_on` at
  reservation.
- **The approved-dispatch check** goes in `authorize_prelocked_on` (`action_dispatch.rs:85-146`)
  after it locks the intent.
  - The decision must match on execution, invocation, `argument_digest` and `operation_digest`, with
    `choice='approve'` and a completed wait.
  - The marker's `current_policy` records `approval_required: true` and the decision id, so
    `workflow_action_replay_supported` (init 3694-3740) refuses automatic replay, and an approval
    yields at most one dispatch.
  - `return_authorized` (`action_receipts.rs:313-359`) shares this seam and is covered by the 05B
    regressions.
- **The schema binding.** The approval columns and the
  `(company_id, run_id, execution_id, action_invocation_id, argument_digest)` FK go to the intent's
  existing UNIQUE key (init 8753). A new
  `UNIQUE (company_id, run_id, execution_id, id, argument_digest, operation_digest)` on intents, with
  an FK to it, binds `operation_digest`. Both digests are database-bound.

### 16.4 One-wait-per-execution lookups (I§5.3, enumerated)

The inventory comes from `graft grep "workflow_waits" --in src` and `grep workflow_waits` over the
init schema at this base, with test files excluded. Changes:

- `UNIQUE (company_id, run_id, execution_id)` (init 9288-9289) becomes
  `UNIQUE (company_id, run_id, execution_id, reason)`;
- a partial unique index allows one `waiting` wait per execution.

**Production Rust.**

| Site | Change |
| --- | --- |
| `waits::row` (`waits.rs:131-137`) | Takes a reason, or returns the `waiting` wait; replay paths pass the step's reason |
| `waits::park_on` replay (`waits.rs:159`) | Passes `event`/`timer`; `park_decision` passes `decision` |
| `wait_commit::resume_on` (`:15`) | Selects the `waiting` wait. When none is waiting, it replays only from the execution's committed route |
| `wait_commit` `advance` (`:149`), `effect_failed` (`:93`), `expire` (`:180`), `fail` (`:262`) | Updates by execution with `state='waiting'`. Already correct under the partial unique index; `reason` is added for clarity |
| `message_receipts::conflicted_effect_park_on` (`:292`) | Adds `reason='effect'` |
| `message_publish::park_on` insert (`:259`) | Correct under per-reason uniqueness |
| `controls::cancel_on` (`:175`), `maintenance::expire_on` (`:34`) | Run-wide `state='waiting'`; unchanged |
| `waits::select_due` (`:299-306`) | Joins waiting waits; unchanged |
| `polling::CANDIDATES_SQL` (`:114`) | `LEFT JOIN … state='waiting'`, at most one row under the partial index; unchanged |
| `fairness::VALID_HINT` (`:85`) | `NOT EXISTS`; unchanged |

**SQL.**

| Site | Change |
| --- | --- |
| `check_workflow_fenced_completion` (init 233) | Its effect-wait `EXISTS` is unchanged; it gains the approval branch (§16.2) |
| `workflow_action_dispatch_commit_guard` (init 3262) | Joins by `action.wait_id`; unchanged |
| `workflow_message_effect_wait_guard` (init 4057) | Unchanged |
| FK `workflow_message_action_wait` (init 13593) | By wait id; unchanged |

**Tests** that assume one wait per execution (for example `message_test_support.rs:319` and the
`["workflow_waits"][0]` snapshot assertions) change only where an approval test creates two waits.

05B re-runs `graft callers waits::row` and `graft grep "workflow_waits"` before its first edit, and
records any site not in this list for B.

### 16.5 Approval lifecycle

The rest of D1 §9 is kept as accepted direction:

- the subject is bound by FK to the frozen intent with both digests;
- the fixed choices are `approve | reject`, with `data` `{}` and optional feedback;
- **reject** and **expiry** fail Terminal `<family>.approval_rejected` and
  `<family>.approval_expired` through `route_parked_failure`;
- the approval wait's deadline is `min(approval.deadline, run deadline)`;
- there is no automatic replay, and at most one dispatch per approval;
- an accepted business decision never implies an approval.

Approver notices reuse the §10 park-time enqueue. An approved `message.send` then parks for its
effect, which is its second wait (§16.4).

## 17. Remaining open points

There are none for B or the user. Two items are recorded for the plans that own them:

- the phase 07 timeout obligation (§4.3);
- the phase 08 routes (`/ui/workflow-decisions/{id}`) and production wiring (Q8).
