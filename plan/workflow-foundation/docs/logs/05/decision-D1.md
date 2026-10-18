# Decision 05-D1 — architecture gate for phase 05

Status: **proposed by A, awaiting B**. Author A (implement-opus-reviewed-2.1), 2026-10-18.

- **Base.** HEAD `feefe3e`, which equals the frozen batch S tree, plus root's user-approved AGENTS and
  startup-log edits. The evidence is in [investigation.md](investigation.md), cited below as I§n.
- **Scope.** This gate covers the whole of phase 05 (EXPANSION 05.1–05.4) and proposes two batches.
  No source has been written.
- **Schema.** Every schema change edits `migrations/20260817000000_init_schema.sql` in place, and the
  fixture and development databases are then reset. That follows plan 05:10-14 and
  `src/adapters/persistence/AGENTS.md` §1 as root amended it on the user's approval. There are no
  follow-up migrations, backfills, shims or populated-upgrade tests.

## 0. Decisions at a glance

| # | Decision | Section |
| --- | --- | --- |
| 1 | Two batches. **05A**: the decision core, member reviewers, comments, timeout/cancel, and the run-scoped event inbox. **05B**: external credentials, decision notices, action approvals, and the phase 05 gate | §1 |
| 2 | `decision.human` takes `reviewers`, an explicit list of 1–16 users or existing email-only participants. It is static: a literal or binding parameters, never step output | §2 |
| 3 | The timeout is Terminal `decision.timed_out` over the declared `error` route; with no route, the run fails. There is no new route kind | §2, §5.4 |
| 4 | New tables `workflow_decisions`, `workflow_decision_reviewers` and `workflow_decision_comments`; the wait row stays the single lifecycle owner | §3, §4 |
| 5 | Every decision transition runs under the run lock in the controls' order: authority → run → execution → job → wait → decision → credential | §6 |
| 6 | Current authority is rechecked inside the settling transaction under `FOR SHARE` locks, and failures are non-disclosing `NotFound` | §7 |
| 7 | External credentials are a 256-bit random token; only its SHA-256 is stored, and it is minted per send, never written to a durable payload. GET never consumes; the submit consumes it atomically | §8 |
| 8 | An action approval is a decision whose subject is one frozen intent (invocation, argument digest, operation digest). It is checked inside `authorize_prelocked_on`. Expiry and rejection fail Terminal, and approved actions are never auto-replayed | §9 |
| 9 | Decision notices are one durable intent per reviewer, written in the park transaction and delivered by a claimable direct sender. Delivery failure never approves | §10 |
| 10 | Events are run-scoped and persisted before any wake. A literal event name with one schema is validated at ingress. Consumption is single and atomic, and decision waits are never matched | §11 |

## 1. Scope and batch split

**05A — decision core and waits** covers 05.1, 05.2, 05.3 for member reviewers, and 05.4.

- **Language** (§2).
- **Schema:** decisions, reviewers and comments; the event inbox reshape; the deadline guard.
- **Runtime:** park, timeout, cancel and run expiry for `decision.human`.
- **`DecisionCommands`:** `SubmitDecision`, `AddComment` and `ReadDecision` for members.
- **Event inbox:** `record_event`, run-scoped consumption, candidate polling.
- **Housekeeping:** help text, the example, `pending_recovery`, the plan-text amendments (I§6) and
  the `NO_BACKWARD_COMPATIBILITY_NOTES` addition (§15).

**05B — external reviewers, notices and action approval** covers the rest of 05.3, then the phase 05
gate.

- **Credentials:** issue, read and redeem.
- **Notices:** the notice ledger and deliverer for all reviewers.
- **Action approval:** the action `approval` input, the approval park in the dispatch seam, settlement
  that reopens the job, the approved-dispatch check, and per-reason wait uniqueness.

**Why split, and why there.**

1. 05A is end-to-end verifiable on its own. A published workflow with `decision.human` parks, is
   submitted, times out or is cancelled, and continues; events resume waits. Its tests do not depend
   on 05B.
2. The stable contract at the end of 05A is the persisted decision model (§3), the settlement
   transaction and lock order (§5–§6), and the command semantics: expected revision, artifact digest,
   command-key replay, non-disclosing errors.
3. 05B extends that contract without reshaping it.
   - Credential redemption adds a `DecisionAuthority::Credential` variant to the same command and
     settlement writer.
   - Notices hang off `workflow_decision_reviewers`.
   - Action approval adds a subject kind and reuses park, settle, timeout and route.
4. 05B changes the phase 04 dispatch seam (`authorize_prelocked_on`, `local`, `reserve_remote`,
   message publication, MCP) and adds secret handling. Those are the highest-risk 04.7/04.11 seams
   and need a focused review that is not mixed with the wait-core rewrite.
5. One batch would touch roughly 30 files across registry, compiler, wait core, dispatch, delivery
   and email, plus about 50 new tests. Its review would be larger than 04.11's.

The wait-core files (`waits.rs`, `wait_commit.rs`, `polling.rs`) change in both batches. In 05A the
change is the decision branch and the event inbox; in 05B it is per-reason uniqueness. Root may
instead move per-reason uniqueness into 05A, which would make 05A's wait diff final. A recommends
keeping it in 05B, next to its only consumer, an approved `message.send` that then parks for its
effect.

## 2. Language contract

### `decision.human` (05A)

`registry/families.rs:89-116` and `schemas.rs:24-29` change as follows.

- **`proposal`** (required, any JSON) is the reviewed artifact.
- **`reviewers`** (required) replaces `reviewer`. It is an array of 1–16 unique items, each either
  `{"kind":"user","id":<uuid>}` or `{"kind":"external","email":<address ≤ 320 bytes>}`. `group` is
  removed.
  - The `user` id is a user UUID, as in the 04.11 `message.send` destinations
    (`registry/actions.rs:119`).
  - A registry check limits the binding to a `Literal` or to references rooted at binding
    parameters. It rejects `input`, step-output and context roots (the roots are visible in
    `ContextReference`, `domain/workflow/context/mod.rs:53-58`).
  - This keeps assignment deterministic and out of the model's reach (R7). Templates set reviewers
    per binding through parameters.
- **`deadline`** (required, RFC3339) is unchanged. The effective deadline is
  `min(deadline, run.deadline)`, and it must be in the future at park.
- **`data_schema`** (literal) and **`feedback_required`** (literal list of declared choices) are
  unchanged.
- **Routes.** `choices` is required, as today. `error` is optional and receives
  `decision.timed_out` and `decision.reviewer_unavailable` (§12).
- **Submitted output** is unchanged: `{choice, data, feedback}`, all required. Feedback may be empty
  unless the choice is listed in `feedback_required`.

**Timeout over the `error` route** (the alternative is a dedicated `timeout:` route key).

- Plan 05:76 asks for "an explicitly declared route, or fail the run". The `error` route is
  explicitly declared, and the shared writer (`recovery.rs:292-334`) already does "route or fail,
  set `terminal_execution_id`" for a parked execution.
- A dedicated route would need a second output shape (there is no choice to report) that downstream
  type projection does not support.
- Plan 07's child maps the failure code to `choice: "timeout"` with `data.map`.
- B may choose the dedicated route instead; that costs a route-grammar, domain and replay change.

**Disclosure (R6).** Comments are never part of any output. Message `content` fields are strings, so
mapping the whole decision object into `message.send` fails at compile time. A 05A compile test pins
this.

### Help, example and plan text (05A)

- `help.rs:71-73` becomes "Durable review deadline required; never automatically accepts. Reviewers
  are an explicit static list of users or existing email-only participants; first valid authorized
  submission wins. Timeout fails Terminal `decision.timed_out` through the declared error route."
- The example at `examples.rs:87-89` moves to the new shape.
- The plan-text amendments are listed in I§6.

### Action approval input (05B)

`http.request`, `tool.call`, `mcp.call`, `message.send` and `message.reply` gain an optional
`approval: {reviewers: <same schema and static rule>, deadline: <RFC3339 text>}`.

- It is used only when the call is protected at dispatch.
- If a protected call declares no `approval`, it keeps today's Terminal
  `<family>.approval_required` (fail closed, R7).

## 3. Schema (init file, edited in place)

### 3.1 05A

**`workflow_decisions`.**

```text
company_id, run_id, execution_id, id uuid; wait_id uuid
subject_kind text  CHECK IN ('business','action_approval')   -- 05A writes 'business' only
proposal_digest text NULL  CHECK ^[0-9a-f]{64}$               -- business
action_invocation_id uuid NULL, argument_digest text NULL, operation_digest text NULL  -- 05B
revision bigint NOT NULL DEFAULT 1 CHECK > 0
created_at timestamptz DEFAULT clock_timestamp()
submitted_at, submitted_by (principal), submission_credential_id (05B, NULL for members),
command_key (1..128), command_fingerprint (^[0-9a-f]{64}$), choice (1..140), feedback (≤ 65536 B),
data jsonb (≤ 1 MiB)
CHECK subject shape: business ⇔ proposal_digest set and action fields NULL; approval ⇔ the reverse
CHECK submission: all of {submitted_at, submitted_by, command_key, command_fingerprint, choice,
      feedback, data} NULL, or all set; submission_credential_id only when submitted
PK (company_id, id); UNIQUE (company_id, run_id, execution_id)
FK (company_id, run_id, execution_id, wait_id) → workflow_waits(company_id, run_id, execution_id, id) ON DELETE CASCADE
FK (company_id, submitted_by) → principals(company_id, id)
```

Trigger `workflow_decision_immutable` (BEFORE UPDATE) allows only one transition: from no submission
to a submission, with `revision` going from 1 to 2. Nothing else ever changes.

**`workflow_decision_reviewers`.**

```text
company_id, decision_id, ordinal smallint (0..15), principal_id uuid, kind text CHECK IN ('member','external')
PK (company_id, decision_id, principal_id); UNIQUE (company_id, decision_id, ordinal)
FK (company_id, decision_id) → workflow_decisions ON DELETE CASCADE; FK (company_id, principal_id) → principals
```

The rows are written at park, and a trigger makes them immutable.

**`workflow_decision_comments`.**

```text
company_id, decision_id, sequence bigint > 0, author_principal_id, command_key (1..128), body text
(1..16384 chars), created_at
PK (company_id, decision_id, sequence); UNIQUE (company_id, decision_id, author_principal_id, command_key)
FK decision ON DELETE CASCADE; FK (company_id, author_principal_id) → principals
```

The rows are immutable (BEFORE UPDATE raises). They go only through cascade deletes.

**`workflow_wait_events`, reshaped to be run-scoped.**

```text
company_id, run_id, id, event_name, correlation, payload, created_at, consumed_execution_id uuid NULL
PK (company_id, id) (producer dedup, unchanged); UNIQUE (company_id, run_id, id)
UNIQUE (company_id, consumed_execution_id) WHERE consumed_execution_id IS NOT NULL
FK (company_id, run_id) → workflow_runs ON DELETE CASCADE
FK (company_id, run_id, consumed_execution_id) → workflow_executions(company_id, run_id, id)
INDEX (company_id, run_id, event_name, correlation, created_at, id) WHERE consumed_execution_id IS NULL
```

The existing size CHECKs stay. The immutability trigger allows only `consumed_execution_id` going
from NULL to a value, once.

**`workflow_waits`.**

- The `consumed_event_id` FK becomes `(company_id, run_id, consumed_event_id)` → events
  `(company_id, run_id, id)`.
- `check_workflow_wait_commit` (init 365-378) changes in three ways:
  - it applies when `notification_intent IS NOT NULL OR reason = 'decision'`;
  - it checks the per-wait deadline for `reason IN ('event','decision')`;
  - on completion with `consumed_event_id`, it asserts that the event's `consumed_execution_id`
    equals the wait's execution.
- The `notification_intent` comment at `waits.rs:197` is reworded. The column remains the
  registration fact, and the per-recipient ledger in §10 refers to it by `wait_id`.

### 3.2 05B

**`workflow_decisions`, approval subject.**

- Add an FK `(company_id, run_id, execution_id, action_invocation_id, argument_digest)` →
  `workflow_action_intents(company_id, run_id, execution_id, id, argument_digest)`, using the
  existing UNIQUE key at init 8753.
- Add a UNIQUE key `(company_id, run_id, execution_id, id, argument_digest, operation_digest)` on
  intents, and an FK to it, so both digests are bound by the database.

**`workflow_decision_credentials`.**

```text
company_id, decision_id, id, reviewer_principal_id, token_hash text UNIQUE CHECK ^[0-9a-f]{64}$,
issued_at, expires_at (= decision deadline), consumed_at NULL
FK (company_id, decision_id, reviewer_principal_id) → workflow_decision_reviewers
```

- Immutable except `consumed_at` going from NULL to a value.
- At most 8 rows per decision and reviewer, enforced under the notice claim lock (§10).

**`workflow_decision_notices`.**

```text
company_id, decision_id, reviewer_principal_id, id, state CHECK IN ('pending','sent','failed','withdrawn'),
attempt_count 0..8, available_at, claimed_until NULL, last_error_class NULL
CHECK IN ('transport','recipient_unavailable','internal'), sent_at NULL, created_at
PK (company_id, id); UNIQUE (company_id, decision_id, reviewer_principal_id); FK → workflow_decision_reviewers
```

**`workflow_waits`, per-reason uniqueness.**

- `UNIQUE (company_id, run_id, execution_id)` becomes `UNIQUE (company_id, run_id, execution_id,
  reason)`.
- A partial unique index allows one `waiting` wait per execution.
- Every lookup that assumes one row changes: `waits::row`, message receipt and effect-wait queries,
  `workflow_message_effect_wait_guard`, the polling join, and replay.

**`task_attempts.stop_reason`** gains `approval_parked`.

## 4. Persistent ownership

- **The wait row** is the single lifecycle owner for every decision:
  - `waiting` means open;
  - `completed` means submitted (business), or approved/rejected (approval);
  - `expired` means timeout or run deadline;
  - `cancelled` means a run cancel.
  The decision row never duplicates this state.
- **The decision row** owns the frozen assignment facts and the single authoritative submission:
  who, when, through which command or credential, and the choice, feedback and data.
  - For business decisions, the execution's `committed_output` `{choice, data, feedback}` is written
    from the submission in the same transaction by the existing completion writer.
  - Original agent outputs stay immutable under the existing triggers (`preserve_workflow_*`).
- **The artifact identity** is `(decision id, proposal_digest)`. `proposal_digest` is the SHA-256 of
  the canonical frozen `proposal`, using the canonicalization and digest in `actions/freeze.rs`,
  moved to a shared helper. The proposal value itself stays in the decision execution's immutable
  frozen inputs. Its version is the digest. A new round (phase 07) is a new decision with its own id.
- **Comments** are owned by `workflow_decision_comments`; they never touch run events or outputs.
- **Audit.** `workflow_run_events` records `decision_opened`, `decision_submitted`,
  `decision_timed_out`, `action_approved`, `action_rejected` and `decision_notice_failed`. Each is
  one event per execution, matching the existing unique `(execution, event_kind)`.
- **Credentials and notices** (05B) belong to the decision.
- **Not touched.** Legacy `human_approvals` and `response_review*` are retired in phase 08
  (REPLACEMENT-MAP 63-65, 72-74, 83).

## 5. Transactions

Every command runs in one transaction with the `lock_timeout` and `statement_timeout` of 1000 ms that
`waits::lock` already sets. `batch_commit::complete` is boxed, as `advance` does, to keep the stack
shallow.

### 5.1 Park (worker; `decision.human` added to `is_wait` and to the worker's wait dispatch)

1. Run `waits::lock` (run → execution → job), then replay an existing wait.
2. Check the run state and deadline.
3. Activate (`pending_recovery::activate`) and read the spec from the frozen inputs: deadline,
   reviewers, proposal.
4. Resolve and check every reviewer (§7.3). Any failure means Terminal
   `decision.reviewer_unavailable` through `recovery::route_failure` (route or fail the run), and
   nothing is parked. Invalid input keeps the existing `PendingFailure::InvalidInput` path.
5. Insert the wait (`reason='decision'`, `deadline = min(input, run)`, registration intent), then the
   decision, then the reviewers (in 05B, then the notices).
6. Run `completion_job::finish(Pure)`, set the run to `waiting/decision`, and audit
   `decision_opened`.

### 5.2 SubmitDecision

Input: company, decision id, command key, expected revision, expected artifact digest, choice,
feedback, data.

1. Read the decision by `(company, id)` with no lock, to learn the run, execution and subject; a miss
   is `NotFound`.
2. Take the authority locks (§7.1), then the run lock, the association locks, execution → job, the
   wait (`FOR UPDATE`) and the decision (`FOR UPDATE`).
3. **Replay check.** If a submission exists with the same actor, the same command key and the same
   fingerprint (SHA-256 of the canonical command), return `Replayed` with the saved route and
   successor. If a submission exists otherwise, return `Conflict`.
4. **Open check.** The wait must be `waiting` and the run `waiting`. `clock_timestamp()` must be
   earlier than both the wait deadline and the run deadline. Otherwise `Conflict` (closed).
5. **Reviewer check.** The actor's principal must be one of the frozen reviewers of the expected kind;
   otherwise `NotFound`.
6. **Staleness check.** `expected_revision = revision` and `expected_artifact = proposal_digest`;
   otherwise `Conflict` (stale).
7. **Validation.** The choice must be declared, and the output must pass
   `validate_completion(bundle, activation, output, RouteSelection::Choice(choice))`
   (`application/workflow/batch.rs:115`), which covers the data schema, the feedback rule and
   lengths, plus the run context limits. On failure, return `BadRequest` and roll back: nothing is
   settled and the decision stays open (R5).
8. **Writes.**
   - Record the submission: `submitted_at = clock_timestamp()` and `revision = 2`.
   - Complete the wait, set the run to running, and call `batch_commit::complete(Parked)` (route
     `choice:<name>`, successor and job).
   - Audit `decision_submitted`.
   - The activation-limit outcome matches `advance`: the submission is recorded, the wait is
     `failed/activation_limit`, and the run fails.
9. **At commit**, the deferred guard (§3.1) refuses a commit that crosses either deadline. The SQLSTATE
   23514 maps to `Conflict` (closed).

The result is `Applied{revision, route, successor}` or `Replayed{…}`. The settlement and its
continuation commit together (R5).

### 5.3 AddComment

The comment command takes the same locks as the submit. Then:

- the wait must be `waiting`, or the command returns `Conflict`;
- the actor needs run visibility, but need not be a reviewer (§7.2);
- the same author and key with the same body is a replay; the same author and key with a different
  body is a `Conflict`;
- the decision may hold at most 1000 comments (one more is a `Conflict`);
- the next `sequence` is `max+1` under the lock.

A comment never changes the run, the wait or the decision `revision`, so it cannot restart
execution or invalidate a reviewer's expected revision (R4).

### 5.4 Timeout, cancel and run expiry

- **Timeout.** In `resume_on`, for a `decision` wait whose deadline has passed while the run deadline
  has not:
  1. set the wait to `expired` and the run to `running`, and audit `decision_timed_out`;
  2. call `route_parked_failure`, the renamed `route_effect_failure`, with Terminal
     `decision.timed_out`. An approval uses `<family>.approval_expired` instead (§9).
  It is woken by the existing candidate clause `wait.deadline <= clock_timestamp()` (`polling.rs:126`).
  Before the deadline the wait is `Pending`.
- **Replay.** `saved` is generalized to every committed route: `success`, `choice:`, `final_error`.
- **Cancel.** Unchanged: `cancel_on` cancels every waiting wait under the run lock, and later submits
  return `Conflict`.
- **Run expiry.** Unchanged: `maintenance::expire_on`.
- **Retry.** A routed decision timeout is not retryable, because `workflow_control_retry_safe`
  requires a failed attempt. A test pins this.

### 5.5 Approval park, settlement and approved dispatch (05B)

This is §9.

### 5.6 Event record and consume (05A)

This is §11.

## 6. Concurrency

**Lock order** (the controls' order, `controls.rs:64-129`):

1. authority rows: `companies FOR NO KEY UPDATE` and `company_members ⋈ principals FOR SHARE` for
   members; for externals, principal, identity and grant `FOR SHARE`;
2. `workflow_runs FOR UPDATE`;
3. association rows (channel, grant, thread `FOR SHARE`);
4. execution, then job `FOR UPDATE`;
5. wait `FOR UPDATE`;
6. decision `FOR UPDATE`;
7. credential `FOR UPDATE`.

Workers (park, timeout, resume) take the run lock first and read authority without locks at park.
Shared locks never conflict with shared locks, so this order adds no cycle with controls.

**Winners.** The run lock serializes every transition, so the first commit wins.

| Race | Outcome |
| --- | --- |
| Submit vs submit (two reviewers, or two keys) | One `Applied`, the other `Conflict`; exactly one successor |
| Same key, same payload, twice | `Applied`, then `Replayed` (same result) |
| Same key, different payload | `Conflict` |
| Submit vs timeout | Whichever commits first. A submit crossing the deadline is refused under the lock or at commit by the deferred guard. A timeout after a submit finds the wait completed and replays |
| Submit vs cancel | Cancel first: the submit gets `Conflict`. Submit first: the cancel cancels the successor normally |
| Comment vs settlement | A comment after settlement gets `Conflict`; a comment before it never affects settlement |
| Credential redeemed twice (05B) | One consumes and settles; the second is a replay or a non-disclosing `NotFound` |
| Approve vs approval expiry vs cancel (05B) | Run lock; the first commit wins |
| Notice deliverers (05B) | `FOR UPDATE SKIP LOCKED` plus `claimed_until`: one sender per claim window |
| Producers with the same event id | `INSERT … ON CONFLICT DO NOTHING`, then compare: `Duplicate` or `Conflict` |
| Event vs consumer | The consumer takes the oldest unconsumed match `FOR UPDATE` under the run lock. A concurrent insert is either visible or found by the next poll; it is never lost |

**Competing-claimant tests** (root `AGENTS.md`). Each uses a barrier or the R006 RunHold harness so
both sides are in flight:

- 05A: two reviewers submitting, submit vs timeout, submit vs cancel, two event producers with one
  consumer.
- 05B: two credential redemptions, two notice deliverers, approve vs expiry.

## 7. Authorization

### 7.1 Member submission

The actor is an authenticated user ID supplied by trusted ingress (phase 08 routes).

1. `authorize_reviewer_on`: the same locks as `authorize_company`, but any role passes. It returns
   the actor's person principal and whether the actor is owner or admin.
2. The run lock.
3. `authorize_association` for the run's channel or thread (`FOR SHARE`, including allowlist grants
   and the owner exception). A run with no channel requires owner or admin.
4. The frozen reviewers must include the principal.

Missing, foreign or inaccessible records all return `NotFound` (the lifecycle contract,
`authorization.rs:73-90`). Revoking membership or access conflicts with the shared locks, so a
revocation is either visible to the check or waits until the settlement commits (R7, R12).

### 7.2 Comments and reads

Comments and reads use steps 1–3; any member with run visibility may comment or read. Comments and
feedback are visible only under run/thread visibility, and external reviewers can neither read nor
write comments.

### 7.3 Park-time assignment check

This check is configuration validation, not an authority grant, so it takes no locks and causes no
order inversion. Submission rechecks with locks.

- **`user`.** The user is a company member with a person principal, and can see the run's
  association: the `reviewer_is_eligible_on` semantics, moved next to the workflow authority code.
  On a channel-less run the user must be owner or admin.
- **`external`.** Exactly one `external` principal in the company has an email
  `participant_identities` row for the address (normalized, status not `disabled`). It also holds a
  grant on the run's channel. External reviewers are refused on channel-less runs.

Any unresolved or ineligible listed reviewer fails the step with `decision.reviewer_unavailable`
(fail closed). No reviewer is dropped silently (R7).

### 7.4 External submission (05B)

See §8. The authority locks are the external principal, the identity and the channel grant
(`FOR SHARE`), followed by the run lock and the rest of the order in §6.

### 7.5 Cross-company access

Every statement is keyed by `company_id`. A credential's company comes from its row and must match
the decision through composite FKs. A decision ID used under another company returns `NotFound`.

## 8. External single-use credentials (05B)

- **Token.** 32 bytes from `ring::rand::SystemRandom`, base64url-encoded with the prefix `wd1.`.
  - The database stores only `sha256(token)` as hex, `UNIQUE`. The token's 256 bits of entropy make
    a key unnecessary.
  - No secret is stored, so the envelope-encryption rule for stored secrets does not apply. The same
    holds for `confirmation_code_hash`.
  - The token is a newtype with a redacted `Debug`. It is never logged, never written to a durable
    payload (AGENTS secrets rule), and never returned except in the email.
- **Scope and lifetime.** A credential is scoped to one decision and one external reviewer, and
  `expires_at` is the decision deadline. It works only while the wait is `waiting`. A new round is a
  new decision, so a late link cannot settle it (R5).
- **Issuance.** A credential is minted per notice send attempt (§10), in the claim transaction, at
  most 8 per reviewer and decision. Earlier credentials stay valid until one is consumed, the
  decision settles, or the deadline passes. A duplicate email after an ambiguous send is therefore
  harmless.
- **Read (GET).** `ReadDecision{Credential}` validates the credential and returns the proposal,
  choices, data schema, feedback rules, deadline, revision and digest. It returns no comments and
  **never consumes**, so link scanners cannot settle anything; the legacy flaw was settling on GET.
- **Submit (POST).** `SubmitDecision{Credential}` uses the §5.2 path with the §7.4 authority. It
  locks the credential `FOR UPDATE`, requires it unconsumed and unexpired, and sets `consumed_at` and
  `decision.submission_credential_id` in the settling transaction.
  - If settlement is invalid, everything rolls back and the credential stays usable for a corrected
    submission. Single use means exactly one successful submission (R8).
  - A retry with an already consumed credential is a `Replayed` result only when its fingerprint
    matches the saved submission; anything else is `NotFound`.
- **Link.** The link is `{https when secure_cookies else http}://{APP_DOMAIN_NAME}/workflow-decisions/credential/<token>`,
  never hard-coded `http`. It uses config the application already reads (I§2.6). The routes belong
  to phase 08, which must serve them with `Cache-Control: no-store` and `Referrer-Policy: no-referrer`
  and must not log the path; that is an 08 obligation recorded here.

## 9. Action-approval subject vs business decisions (05B)

- **Subjects.**
  - Business: `proposal_digest`, the declared choices, the data schema.
  - Approval: one frozen intent (`action_invocation_id`, `argument_digest`, `operation_digest`). The
    database binds it to `workflow_action_intents`. The choices are fixed `approve | reject`, `data`
    must be `{}`, and feedback is optional.
  - An accepted business decision never creates or implies an approval. An approval references one
    invocation and grants nothing else (R10, REPLACEMENT-MAP 72).
- **Park.** This is in the fenced dispatch transaction.
  1. `authorize_prelocked_on` (`action_dispatch.rs:85-146`) returns `approval_required`, and no
     approval exists for the locked intent.
  2. If the step declares `approval`, insert the wait (`reason='decision'`, deadline the min of the
     approval deadline and the run deadline), the decision with the approval subject, the reviewers
     and the notices.
  3. Close the attempt with `approval_parked`, mark the job `completed`, set the run to
     `waiting/decision`, and audit. Return `ApprovalParked`, a new variant of the local, remote and
     message results. The supervisor confirms the exact close, as `confirm_suspension` does for
     `effect_parked`.
  The same pattern is used by `message_publish.rs:252-276`. With no `approval` declared, the call
  keeps Terminal `<family>.approval_required`.
- **Settlement.**
  - **approve:** record the submission, complete the wait, set the run to running, reopen the job
    (`completed` → `pending`, `run_at = now`; the precedent is `control_retry.rs:37`), and audit
    `action_approved`.
  - **reject:** complete the wait, then `route_parked_failure(<family>.approval_rejected)`.
  - **expiry:** set the wait to `expired`, then `route_parked_failure(<family>.approval_expired)`.
    Protected actions therefore default to rejection on expiry.
  - Parking requires a remaining attempt (`retry_count < max_retries`); otherwise the call fails
    Terminal `approval_required`.
- **Approved dispatch.** Inside `authorize_prelocked_on`, after it locks the intent, a protected call
  is allowed only if an approval decision exists for the same execution with the same invocation,
  argument digest and operation digest, `choice = 'approve'` and a completed wait.
  - The dispatch marker's `current_policy` records `approval_required: true` and the decision id, so
    `workflow_action_replay_supported` (init 3694-3740) still refuses automatic replay. An ambiguous
    outcome goes to 04.7 reconciliation, so an approval yields at most one dispatch.
  - The run actor's current access is still rechecked (existing). A mismatch, or an approval that is
    missing after one was used, fails Terminal `approval_required`; nothing is re-requested.
  - The intent is immutable and both digests are bound by FK, so no approved action can dispatch
    arguments other than those reviewed (R12).
- **Messages.** An approved `message.send` with an email destination then parks for its effect. That
  is its second wait, which needs §3.2's per-reason uniqueness.
- **Reconciliation.** The 04.7 path is unchanged: a parked approval has no dispatch evidence.

## 10. Notification intents (05B)

- **Intent.** One `workflow_decision_notices` row per reviewer is inserted in the park transaction,
  so the intent is durable and atomic with the open decision. It names the decision, run, execution
  and reviewer precisely (R9).
- **Deliverer.** `deliver_decision_notices(limit ≤ 16)`:
  1. **Claim transaction.** Select `pending` rows due and unclaimed, `FOR UPDATE SKIP LOCKED`.
     - If the wait is not `waiting`, mark the row `withdrawn`.
     - Otherwise increment `attempt_count`, set `claimed_until = now + 60 s` and `available_at` by
       backoff, read the recipient address now (the member's account email, or the external
       identity's address), and for an external reviewer mint the credential (§8).
  2. **Send** outside the transaction through the application port `DecisionNoticeSender`, with a
     30 s timeout. Its adapter is a `MailTransport` sender like `SmtpConfirmationSender`
     (`mail.rs:150-187`), using `SMTP_FROM_ADDRESS` with no reply-correlation headers.
  3. **Result transaction.** Success means `sent`. A failure at `max_attempts` (8) means `failed`
     plus audit `decision_notice_failed`; otherwise the claim is released.
  - A crash after the send repeats the email with a new credential (bounded).
- **Content.** The workflow and step title, the deadline, the decision id and the link: an
  authenticated decision page for members, or the credential link for externals. No proposal
  content and no comments go in the email, so forwarding discloses nothing.
- **Never approval.** A failed or withdrawn notice leaves the decision open until it is submitted,
  times out or is cancelled. Notices come from a no-reply address. Inbound replies are ordinary
  ingress, and no event producer matches decision waits (§11; BRIEF-03.6), so replies never settle
  anything (R9).
- **Why a direct sender rather than the delivery engine** (alternatives considered):
  - The delivery engine stores *rendered* parts durably (I§2.3), so a credential link would persist
    in plaintext.
  - The existing `notification_events`/`notifications` outbox cannot express a decision, a
    channel-less run or an external recipient, and it refuses workflow jobs.
  - The direct sender follows the project's precedent for one-time secrets (confirmation codes) and
    gives every reviewer one ledger.
  - Member notices could use the delivery engine, but that would be a second state machine with no
    decision-relevant benefit.
  - In-app queues and pages are phase 09 projections over the decision rows (REPLACEMENT-MAP 83).
- **Wiring.** Scheduling the deliverer, the sweeper and the event producer in `serve` is phase 08
  (I§3 G11). Phase 05 proves them through direct calls and restart tests.

## 11. Timer and event correlation, including event-before-park (05A)

- **Scope.** Events are scoped to the run. A producer supplies company, run, event id, name,
  correlation and payload. It never supplies an execution, so an event can arrive before the waiting
  execution exists or parks (R11, G10).
- **Ingress.** `record_event` returns one of:

  | Result | When |
  | --- | --- |
  | `Recorded` | A new event is stored |
  | `Duplicate` | Same id, identical content |
  | `Conflict` | Same id, different content |
  | `Unmatched` | The run is missing, foreign, terminal or past its deadline, or no `wait.event` step in the run's bundle declares this event name |
  | `BadRequest` | The payload is invalid against that event's schema |

  - `Unmatched` tells the caller (an 08 ingress adapter) to handle the message as ordinary ingress
    (plan 05:82-83, 08:44-46).
  - A new compile rule makes `wait.event`'s `event` a literal. All steps that declare one event name
    must share a single `payload_schema`, so ingress can validate as `signal_on` does today.
  - At most 128 unconsumed events per run; one more is a `Conflict`. The run row gets a plain read,
    not a lock. An event recorded just before a concurrent cancel is orphaned, bounded, and cascades
    with the run.
- **Persist before signal.** The committed row is the signal. The candidate clause (`polling.rs:127-130`)
  becomes "an unconsumed event for the same run, name and correlation exists", restricted to
  `wait.reason = 'event'`. There is no listener or NOTIFY, so a restarted worker finds the event.
- **Consumption.** In `resume_on`, under the run lock, take the oldest unconsumed matching event
  `FOR UPDATE`, then `prepare` it (payload revalidated). `advance` sets `consumed_execution_id` and
  `wait.consumed_event_id` in one transaction. With the UNIQUE indexes this means at most one event
  per wait and one wait per event, so a duplicate signal cannot resume twice. An invalid payload
  still fails the wait with `invalid_output`, as today.
- **Decision waits never match.** They have no event name, and both the clause and the consumption
  query require `reason = 'event'` (BRIEF-03.6:15).
- **Timers** are unchanged: durable deadline, candidate wake, no sleeping worker.
- **The `WorkflowSignal`/`record_signal` API** is replaced; execution scoping disappears. Tests move
  to the new port, with no backward-compatibility layer.

## 12. Failure codes and bounds

**New Terminal codes:** `decision.timed_out`, `decision.reviewer_unavailable`,
`<family>.approval_rejected` and `<family>.approval_expired`, for the http, tool, mcp and message
families. `<family>.approval_required` keeps its meaning: protected with no `approval` declared, a
digest mismatch, or no attempt left.

**Bounds.** Each bound gets an at-limit and an over-limit test, which is the signal that fails if it
is ever approached:

| Bound | Limit |
| --- | --- |
| Reviewers | 16 |
| Comments | 1000 per decision, 16384 characters each |
| Feedback | `MAX_TEXT` |
| Data | The run's context limits |
| Events | 128 unconsumed per run; payload 1 MiB; name 128; correlation 256 |
| Credentials | 8 per reviewer and decision |
| Notice attempts | 8 |
| Notice batch | 16 |

No existing limit is raised.

## 13. Affected contracts (blast radius)

### 05A

- **Registry:** `registry/families.rs`, `schemas.rs`, `help.rs`, `examples.rs`, plus the registry
  tests.
- **Compiler:** the static-root check and the event-name/schema rule, plus compile tests.
- **Worker:** the dispatch in `worker.rs`.
- **Application:** `application/workflow/waits.rs` (event port; route selection in `prepare`) and a
  new `application/workflow/decisions.rs` (commands, validation, digests, shared canonicalization
  moved from `actions/freeze.rs`).
- **Persistence:** `persistence/workflow/waits.rs` (`park_on`, `is_wait`, `record_event`),
  `wait_commit.rs` (decision branch, generalized `saved`, event consumption), `polling.rs`,
  `pending_recovery.rs:141-144` (adds `decision.human`), `recovery.rs` (the rename), and a new
  `persistence/workflow/decisions.rs` with its tests.
- **Schema:** the init schema.
- **Pinned bodies:** the `workflow/tests.rs` INIT_SCHEMA/function-body pins, if they cover the
  changed guard.
- **Existing tests that move to the new event port:** `wait_guard_tests.rs`,
  `control_wait_order_tests.rs`, `wait_tests.rs`, `wait_deadline_tests.rs` and the polling and
  fairness tests.
- **Other:** `src/main.rs` notes list (§15); `graft build`.

### 05B

- **Dispatch seam:** `action_dispatch.rs` (`authorize_prelocked_on`, `local`, `reserve_remote`),
  `action_receipts.rs` (`return_authorized` shares the seam), `message_publish.rs`
  (`publish_on`/`observe`/park), `mcp.rs`/`mcp_runtime.rs`, `http_runtime.rs`, `actions/failure.rs`.
- **Language:** the registry's action `approval` input.
- **Waits:** the uniqueness change and its lookups; `message_receipts.rs` `confirm_suspension`.
- **Attempts:** `task_attempts.stop_reason`, plus the supervisor confirm for the approval park.
- **New modules:** credentials and notices (application and persistence).
- **Email:** the `DecisionNoticeSender` port and adapter, and the app-state constructor (no
  scheduling).
- **Risk:** `scripts/transport-boundary-check.sh` may restrict direct mail sends; check it first in
  05B, and if it objects, bring the sender inside the allowed boundary, never weaken the check.

## 14. Verification plan

**Environment for every check.** `SQLX_OFFLINE=true`, `CARGO_BUILD_JOBS=4`, and
`TEST_DATABASE_URL`/`DATABASE_URL` pointing at the fixture (`127.0.0.1:55450/workflow0410_test`).

- After each init edit, drop and recreate the fixture databases (`workflow0410_test`,
  `workflow0410_fresh_test`), then run `cargo sqlx migrate run`.
- Record commands and results under `logs/05/`. A check that cannot run is reported as such, never
  claimed.

### 05A focused checks, per step

| Change | Command (`cargo test --locked --offline --lib …`) |
| --- | --- |
| Language | `registry::`, `compiler::` |
| Decisions (new DB tests, `workflow_decision_*`) | `workflow_decision_` |
| Wait core and events | `workflow_wait_`, `wait_guard`, `wait_deadline`, `control_wait_order` |
| Candidate SQL | `polling`, `fairness` |
| Shared resume and replay | `workflow_message_` |
| Init pins | `adapters::persistence::workflow::tests` |

Each 05A step also runs:

- `cargo fmt --all -- --check`;
- `git diff --check`, excluding the reviewer-owned file with the known trailing space;
- strict Clippy;
- `cargo sqlx prepare --check` after schema edits.

### 05A batch gate

The DB-backed workflow subsets under `scripts/test-network-isolation.sh`, because the wait core is
shared infrastructure: `--lib adapters::persistence::workflow` and `--lib application::workflow`.

### 05B focused checks

- **New:** `workflow_decision_credential_`, `workflow_decision_notice_`, `workflow_action_approval_`.
- **Seam regressions:** `workflow_action_dispatch`, `workflow_http`, `workflow_mcp`,
  `workflow_message_`, `action_receipt`, `action_reconciliation_runtime`, `lease_`.
- **Email adapter** unit tests over a recording `MailTransport`.
- `scripts/tests/transport-boundary.sh && scripts/transport-boundary-check.sh`.

**05B batch gate:** the same workflow subsets as 05A.

### Final phase 05 gate

After 05B is accepted and corrected, the batch S gate list runs without its squash-equivalence
step, each gate recorded in `logs/05/gate-status.log`.

1. `cargo fmt --check`
2. `git diff --check`
3. The transport scripts
4. The credential and deploy script tests
5. `cargo check --locked --offline --all-targets`
6. Strict Clippy
7. A fresh-schema migrate, with a dump diff against the fixture
8. `cargo sqlx prepare` and `--check`
9. `--no-run`
10. The network-isolated full suite
11. `scripts/stack-budget.sh`
12. `scripts/lease-budget.sh`
13. `graft build`

Not claimed until each passes.

### Acceptance mapping (plan 05:87-92)

| Acceptance | Tests (prefix) |
| --- | --- |
| Comments don't advance; feedback only where mapped | `workflow_decision_comment_*` (run, wait and revision unchanged); compile test: the decision object cannot reach message content |
| Competing reviewers, duplicate submissions, timeout/submit, cancel/submit | `workflow_decision_race_*` (competing claimants, §6) |
| Revoked membership, external credentials, cross-company, stale artifacts | `workflow_decision_auth_*` (revocation under lock, both orders), `workflow_decision_credential_*`, `workflow_decision_cross_company_*`, `workflow_decision_stale_*` (artifact digest, revision, late round on a settled decision) |
| Invalid edit, missing feedback or unknown choice fail without settlement | `workflow_decision_invalid_*` (decision still open, nothing written) |
| No approved action dispatches different arguments | `workflow_action_approval_digest_*`: FK refusal; an approval for invocation A does not let B dispatch; an approved call is not auto-replayed |
| Restart and event-before-park | `workflow_wait_event_before_park_*` (event recorded before the execution exists, then consumed); `workflow_decision_restart_*` (a fresh worker and pool time out and resume) |

## 15. `NO_BACKWARD_COMPATIBILITY_NOTES` and plan files

- **These logs** are not plans and need no entry.
- **A phase 05 brief.** If root wants D1 published as `plan/workflow-foundation/BRIEF-05.md`, the
  brief opens with the no-backward-compatibility note, and 05A adds its path to
  `NO_BACKWARD_COMPATIBILITY_NOTES` (`src/main.rs:153-179`).
- **An existing gap.** `05-…` to `10-…` already carry the note but are not listed. 05A adds `05-…`,
  and `06-…` to `10-…` if root agrees.

## 16. Open questions for B and root

1. **One reviewer or a list?** A recommends an explicit list of 1–16 with first valid wins. That
   keeps "competing reviewers" meaningful without any group model. A list of one is a single
   reviewer.
2. **How a timeout surfaces.** Over the `error` route (A's recommendation) or through a dedicated
   `timeout:` route (§2).
3. **Notice transport.** A uniform direct sender (A's recommendation) or the delivery engine for
   members (§10).
4. **Where approvers are declared.** On the step (A's recommendation) or in operator policy rows.
5. **Reviewer binding roots.** Literal or binding parameters only (A's recommendation), or literal
   only.
6. **External reviewers.** A recommends requiring a channel grant and refusing them on channel-less
   runs.
7. **Batch placement of per-reason wait uniqueness.** 05B (A's recommendation) or 05A (§1).
8. **Production wiring** of the deliverer, sweeper and producer stays in phase 08. Confirm.
