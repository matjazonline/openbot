# Phase 05 investigation — human decisions, comments, feedback and waits

Author A (implement-opus-reviewed-2.1), 2026-10-18. Read-only investigation: no source,
migration or test edits and no cargo commands were made for it.

- **Base.** HEAD `feefe3e` (user commit "plan"), which reproduces the frozen batch S digest
  `33ae9028…4552`. The root-applied, user-approved edits to `src/adapters/persistence/AGENTS.md` and
  the `serve()` warning in `src/main.rs` are present. Line numbers in
  `migrations/20260817000000_init_schema.sql` refer to that squashed file, and every other
  `file:line` reference refers to this tree.
- **Sources.**
  - [Phase 05 plan](../../../05-human-decisions-and-waits.md) and EXPANSION rows 05.1–05.4
    (`EXPANSION.md:321-324`).
  - REPLACEMENT-MAP rows 63-65, 72-74 and 83, and NOT_PLANNED rows 17-19.
  - Plans 07 and 08 (`07-…:44-78`, `08-…:38-60`), BRIEF-03.6, BRIEF-04.11 and BRIEF-02.2 row 84.
  - The code paths cited below.
  - Three read-only Explore inventories (waits; approvals and authorization; decision language and
    lifecycle), which were re-checked against the source where they are cited here.

## 1. Requirements

| ID | Requirement (plan line) | EXPANSION |
| --- | --- | --- |
| R1 | A general human task returns `{choice, feedback, data}`; approve/reject is one configuration of it (5-6, 22-30) | 05.1 |
| R2 | It declares the proposal, the reviewers, named choices mapped to routes, an editable data schema, feedback rules per choice, and a deadline. Reviewers cannot submit step IDs (18-20) | 05.1 |
| R3 | Authoritative metadata: reviewer identity, submission time, decision revision, and reviewed artifact identity/version. Original outputs stay immutable, and edited data must be schema-valid (32-34) | 05.1 |
| R4 | Separate `AddComment` and `SubmitDecision` commands. Comments are append-only, never settle, and never reach agent input implicitly (45-50) | 05.2 |
| R5 | Expected revisions plus command idempotency. Exactly one of submit, timeout or cancel wins. Settlement and continuation are atomic. Invalid edits leave the decision open. Late rounds cannot affect a new one (52-54) | 05.2 |
| R6 | Comments and feedback are internal, governed by run/thread visibility. Prompts map feedback explicitly, and message steps map the accepted artifact, never the decision object (56-59) | 05.2 |
| R7 | Explicit reviewers (user decision: **explicit only, no groups**); the first valid authorized response settles. Current access is rechecked on submission. Assignment is deterministic, separate from model choice. A missing reviewer is a failure, never approval (63-66) | 05.3 |
| R8 | Email-only reviewers use scoped, expiring, single-use credentials. Links and authenticated routes share one command (68-69) | 05.3 |
| R9 | Notification intents are durable and name the exact decision. Email replies never settle a decision, and a delivery failure never approves (69-71) | 05.3 |
| R10 | Business decisions and action approvals share infrastructure but keep different subjects. An approval covers one frozen invocation/digest and grants no other permission. Protected actions reject on expiry. A business timeout takes a declared route, or else fails the run (73-76) | 05.3 |
| R11 | Timer/event waits: scoped correlation, payload schemas, deadlines, and atomic consumption. Events are persisted before signalling (event-before-park). A duplicate never resumes twice. Unmatched messages stay ordinary ingress (80-83) | 05.4 |
| R12 | Acceptance (87-92): comments don't advance; competing reviewers; duplicate submit; timeout/submit and cancel/submit races; revoked membership; external credentials; cross-company access; stale artifacts; invalid edit, missing feedback or unknown choice fail without settling; no approved action dispatches different arguments; restart and event-before-park | 05.1-05.4 |

Schema rule (plan 10-14): edit `20260817000000_init_schema.sql` in place, reset dev/test databases,
and add no follow-up migrations, backfills, shims or populated-upgrade tests. The user approved the
matching `src/adapters/persistence/AGENTS.md` amendment, which root applied (§1, the composite-FK
bullet and the credential paragraph, all marked pre-release).

## 2. Inventory: what exists today

### 2.1 Language and registry (`decision.human` is declared but not executable)

- **Registry contract.** `src/application/workflow/registry/families.rs:89-116`.
  - Inputs: `proposal` (`true` schema), `reviewer: selector()`, `deadline` (text), `data_schema`
    (literal), optional `feedback_required` (a literal list of declared choices).
  - Output: `{choice: enum of the declared choices, data: data_schema, feedback: string ≤ MAX_TEXT}`,
    all required.
  - Feedback rule: an `allOf` if/then demands non-empty feedback for the listed choices.
- **Choice names** come from `routes.choices` (`registry/mod.rs:148-157`); `literal()` enforces
  literal bindings (`registry/mod.rs:133-143`).
- **Selector.** `registry/schemas.rs:24-29` is `{kind: user|group, id: identifier()}`, and
  `identifier()` is a slug pattern, not a UUID (`schemas.rs:6-8`).
  - Compare the 04.11 `message.send` destinations, `{kind: user|channel, id: uuid()}`
    (`registry/actions.rs:119`), which reject groups (BRIEF-04.11:10).
  - The example `registry/examples.rs:87-89` uses `{"kind":"user","id":"reviewer"}`.
- **Help text is stale.** `registry/help.rs:71-73` says "Execution unavailable … Reviewer is a
  user/group selector", and `help.rs:95-97` covers waits.
- **Route grammar.** `compiler/wire.rs:72-121` allows exactly one of `success` or `choices`, plus an
  optional `error`; there is no timeout route.
  - Committed routes are `success`, `choice:<name>` (`batch_commit.rs:25`) or `final_error`.
  - `completion.rs:240` parses `choice:`.
- **Bindings** are `Literal | Reference(root, path) | …` (`domain/workflow/context/mod.rs:17-58`), so
  a registry check can restrict which reference roots an input accepts.
- **Metadata.** `registry/metadata.rs:82` classifies `decision.human` as `DurableSuspension` /
  `HumanReview`.

### 2.2 Runtime: waits, the worker and recovery

- **Worker dispatch.** `src/application/workflow/worker.rs:129-145`.
  - `wait.event`/`wait.timer` run `park_wait` then `resume_wait`.
  - `decision.human` falls to `WorkDisposition::Unsupported` (line 143).
  - `WorkflowWorker` has no production caller; only tests construct it. `sweep_waits` and
    `record_signal` have no production caller either.
- **Application wait port.** `src/application/workflow/waits.rs`:
  - `WorkflowSignal` (22-28) addresses an exact `ActivationRequest` (company/run/execution/job);
  - the `WorkflowWaits` trait (54-68);
  - `specification` (76-116), which reads `deadline`, `event` and `correlation` from frozen inputs;
  - `prepare` (117-124), which always validates with `RouteSelection::Success`.
- **Persistence waits.** `src/adapters/persistence/workflow/waits.rs`:
  - `lock` (97-130) sets `lock_timeout`/`statement_timeout` to 1000 ms and locks run → execution →
    job (pending/completed, unleased) `FOR UPDATE`;
  - `row` (131-137) is the wait `FOR UPDATE` and assumes **one wait per execution**;
  - `audit` (143-151) writes `workflow_run_events`;
  - `park_on` (152-217) uses the pure-park pattern: insert a wait with
    `notification_intent = {version 1, kind wait_registered, wait_id, run_id, execution_id}`
    (197-198; the comment says phase 04/05 consume this identity "never a separate queue"), then
    `completion_job::finish(Pure)`, then run `waiting/<reason>`, then audit `wait_parked`;
  - `signal_on` (218-281) only stores an event for an execution that already exists (`lock` must
    succeed) and validates its payload by activating that execution. An event for a not-yet-created
    execution returns `false` and is lost. At most 128 events per execution;
  - `is_wait` (308-333) accepts only `wait.event | wait.timer`.
- **Resume and commit.** `src/adapters/persistence/workflow/wait_commit.rs`:
  - `resume_on` (5-81): run expiry first, then replay a settled wait; an event wait past its deadline
    `expire`s; the effect branch serves 04.11; a timer wakes at its deadline; an event wait takes the
    oldest matching event `FOR UPDATE`;
  - `advance` (103-169): prepare the output, check the activation bound, complete the wait (with
    `consumed_event_id`), set the run to running, then `batch_commit::complete(Parked)`, all
    atomically;
  - `expire` (175-188) fails the run and leaves `terminal_execution_id` NULL. There is no timeout
    route;
  - `saved` (198-250) replays only `success` (plus `final_error` for effect waits), so a `choice:`
    route cannot be replayed;
  - `fail` (252-276).
- **Candidate discovery.** `src/adapters/persistence/workflow/polling.rs:100-150`.
  - A wait is a candidate at its deadline, at the run deadline, when the run is terminal, when a
    matching event exists for the same **execution** (124-130), or, for effect waits, on a
    receipt/conflict.
  - No in-memory listener or NOTIFY is involved.
- **Recovery.**
  - `pending_recovery.rs:141-144` lists the kinds whose pending exhaustion cannot have unknown
    effects. `decision.human` is missing from it.
  - `recovery.rs:292-334` `route_effect_failure` takes the frozen `error` route or fails the run and
    sets `terminal_execution_id`, for a parked execution whose job is `completed`. This is the
    reusable "declared route or fail" writer.
- **Controls and maintenance.**
  - `controls.rs:64-129`: the lock order is `authorize_company` → run `FOR UPDATE` →
    `authorize_association` → `cancel_on`.
  - `cancel_on` (163-192) locks every execution, then every job, then sets all `waiting` waits to
    `cancelled` and cancels message deliveries.
  - `maintenance.rs:5-…` `expire_on` takes the run lock, checks the clock, and expires all waits.
  - `control_retry.rs:3-42` reopens a failed job.
  - `workflow_control_retry_safe` (init 3814-3843) requires a failed attempt and
    `retry_count > 0`, so a parked-then-routed decision is not retryable.

### 2.3 Schema (init file)

- **`workflow_waits`** (7076-7099).
  - Reasons: `decision|event|timer|child_run|effect|reconciliation`. States:
    `waiting|completed|expired|failed|cancelled`. `failure_code` is in
    `activation_limit|invalid_output|evidence_conflict`.
  - `notification_intent` must be version 1 with `wait_id = id` and ≤ 2048 bytes.
  - `UNIQUE (company_id, run_id, execution_id)` (9288-9289) means one wait per execution.
  - The `consumed_event_id` FK is execution-scoped (13840-13841).
- **`workflow_wait_events`** (7057-7069).
  - Execution-scoped, `PK (company_id, id)` (9280-9281, producer dedup), payload ≤ 1 MiB.
  - Immutable (`immutable_workflow_wait_event`, trigger 11395), with an FK to executions (13848).
- **Deadline guard.** `check_workflow_wait_commit` (365-378, deferred constraint trigger 11388) checks
  the deadline at commit, but only when `notification_intent IS NOT NULL`, and the per-wait deadline
  only for `reason='event'`.
- **Effect-wait guard.** `workflow_message_effect_wait_guard` (4057-4068, trigger 11262).
- **Waiting shape and attempts.**
  - `workflow_runs.waiting_reason` admits `decision` (7009).
  - `task_attempts.stop_reason` includes `effect_parked` and the legacy `approval_rejected` (5577).
- **Action intents.** `workflow_action_intents` (6141-6158): `policy_decision` is in
  `unevaluated|approval_required`, and there are UNIQUE keys on
  `(company, run, execution, id, argument_digest)` and on
  `(company, run, execution, operation_digest)` (8745-8769).
- **Audit and commands.**
  - `workflow_run_events` (6933-6947) carries no payload; it is not usable for comments.
  - `workflow_control_commands` (6567-6585) is the command-receipt pattern: `command_key`, expected
    revision, result.
- **Legacy approvals.** `human_approvals` (4699-4721) uses a plaintext `token uuid` and an
  `approver_email`. Its use case builds `http://{domain}/approvals/{token}?action=…` links that settle
  on GET (`application/use_cases/approval.rs:154-279`, link at 163). **It must not be reused.**
- **Notification outbox.** `notification_events`/`notifications` (5133-5190) have closed
  `source_kind`/`action_kind` CHECKs with no workflow decision kind, and
  `notifications.channel_id NOT NULL` while `workflow_runs.channel_id` is nullable (6987).
  `enqueue_actionable_notification_event` (734-790) refuses workflow jobs.
- **Deliveries.** `message_deliveries.purpose` includes `notification` (5045). Standalone
  deliveries have no company or channel and carry an `external_destination` (5036). Delivery parts
  store the **rendered** payload durably (`message_delivery_parts.payload`, 5055-5080; rendering
  happens before storage, `application/transport/delivery.rs:367-375`).
- **Principals.**
  - `principals.kind` is `person|agent|external|system` (1332-1344).
  - `participant_identities` carries `transport email|slack` and `status observed|verified|disabled`
    (5197-5217).
  - `channel_principal_grants` has `participate|view` (1261-1270).
  - `notification_principal_can_view` (1351-1353) is the member-visibility predicate.

### 2.4 The phase 04 action-approval seam

- **Contracts.** `actions/contracts.rs`:
  - `ActionPolicyContext.approval_required` (31-36);
  - `ApprovalSubject {invocation, argument_digest}` (60-65);
  - `ActionIntent::approval_subject` (98-104);
  - `ActionPolicyDecision {Unevaluated, ApprovalRequired}` (107-112). There is no approved state.
- **Freeze.** `actions/freeze.rs:12-47`: `argument_digest` is the SHA-256 of the canonical arguments;
  `operation_digest` covers scope, key, target, contract and arguments (28-31).
- **Policy and protection.**
  - `actions/authorization.rs:117-129`: `validate_current_authority` returns `ApprovalRequired` when
    the frozen context or the current operator policy demands it.
  - The trusted contexts hard-code `approval_required: false` (`persistence/workflow/http_runtime.rs:72`,
    `http.rs:41`, `http_publication.rs:43`, `message.rs:48`, `application/workflow/mcp.rs:211`).
  - Only the operator rows (`workflow_http_operations`, `workflow_mcp_operations.approval_required`)
    can make a call protected today.
- **The single dispatch seam.** `persistence/workflow/action_dispatch.rs:85-146`
  `authorize_prelocked_on` locks fence → attempt → authority → current resource/policy → intent
  `FOR UPDATE`, and returns `approval_required`.
  - Its callers: `authorize_on` (74-83), used by `local` (197), `reserve_remote` (404-408) and
    message publication (`message_publish.rs:301`, `observe` 433), and 04.7 `return_authorized`
    (`action_receipts.rs:313-359`).
  - Protected calls roll back before any dispatch write, then fail Terminal
    `<family>.approval_required` (`actions/failure.rs:66-90`, line 77).
- **Replay.** `workflow_action_replay_supported` (init 3694-3740) requires
  `current_policy.approval_required = false`, so a protected action is never auto-replayed; its
  ambiguous outcomes go to 04.7 reconciliation.
- **The 04.11 I/O park pattern.** `message_publish.rs:252-276` `park_on`: in the fenced publication
  transaction it inserts the wait, closes the attempt with `effect_parked`, marks the job
  `completed`, and sets the run to `waiting/effect`.

### 2.5 Authorization helpers

- **`authorize_company`** (`persistence/workflow/authority.rs:9-36`): `companies FOR NO KEY UPDATE`,
  then `company_members ⋈ principals FOR SHARE`; owner/admin only.
- **`authorize_association`** (`association.rs:6-61`): channel `FOR SHARE`; `team`/`public` pass; an
  `allowlist` channel needs the owner or a `view` grant `FOR SHARE`; a thread `FOR SHARE`. Thread
  visibility equals channel visibility.
- **Lifecycle contract.** `application/workflow/authorization.rs:54-99` returns a non-disclosing
  `NotFound`. Its doc requires that later commits recheck current authority transactionally.
- **`reviewer_is_eligible_on`** (`persistence/response_review.rs:375-413`) is the member-reviewer
  predicate (person principal; owner, or member with channel view). It takes **no** `FOR SHARE`
  locks.

### 2.6 Credential and command patterns

- **Hashed single-use codes.** `application/use_cases/user.rs:338-380` mints a code and stores only
  `confirmation_code_hash` (keyed by `jwt_secret`, scope and code; 991-999). It sends through a
  **direct, non-durable** sender (`ConfirmationCodeSender`; `adapters/protocols/email/mail.rs:150-187`
  over `MailTransport`).
  - Consumption is an atomic `DELETE … WHERE hash AND expires_at > now RETURNING`
    (`persistence/user.rs:571-640`).
  - Six-digit codes are too weak to serve as link credentials.
- **`response_review_commands`** (init 5339-5352): command id, fingerprint replay, expected version,
  assigned reviewer.
- **Config keys the application reads.** `APP_DOMAIN_NAME` → `app_domain_name` (`infra/config.rs:106,569`);
  `secure_cookies` is derived from it (126-131); `SMTP_FROM_ADDRESS` (112, 601); `jwt_secret` (104).

### 2.7 Phase 04.11 messaging

`message.send` reaches company users and channels only, with no groups and no external addresses
(BRIEF-04.11:10-12). External email-only reviewers can therefore be reached only through a
standalone notification or a direct sender. Effect waits use run-deadline waits
(BRIEF-04.11:24-27).

## 3. Gaps

| Gap | Requirement | Missing today |
| --- | --- | --- |
| G1 | R1-R3 | `decision.human` has no runtime: worker `Unsupported`, `is_wait` excludes it, `prepare` forces `Success`, `saved` cannot replay `choice:`. There is no decision row, artifact digest or reviewer record |
| G2 | R7 | The selector allows `group` and slug ids. Reviewer bindings may be any expression, so model-produced output could pick the reviewer |
| G3 | R4-R5 | No `AddComment`/`SubmitDecision`, comment storage, decision revision or command idempotency for decisions |
| G4 | R5 | Run expiry, cancel and wait expiry exist, but no decision timeout route; `expire` always fails the run |
| G5 | R7 | There is no reviewer-authority recheck taking `FOR SHARE` locks for a non-admin member (`authorize_company` is admin-only; `reviewer_is_eligible_on` takes no locks) |
| G6 | R8 | No decision credential. The legacy token is plaintext and settles on GET over `http://` |
| G7 | R9 | No decision notice intent or deliverer. The existing outbox cannot express workflow decisions or channel-less runs. Durable delivery parts would persist a link token in plaintext, against the persistence secrets rule |
| G8 | R10 | Protected actions fail Terminal; there is no approval park, no approval subject row, and no approved dispatch path |
| G9 | R10 | One wait per execution blocks an approved `message.send` from parking again for its effect receipt |
| G10 | R11 | Events are execution-scoped, so a producer must know the execution id, and an event before the execution exists is dropped. There is no unmatched/duplicate result for producers |
| G11 | R11 | No production wiring of the worker, sweeper or event producer (owned by phase 08 public interfaces, `08-…:51-60`) |
| G12 | R6 | No check that a message step cannot map the whole decision object. This relies on content schema typing (a string field) and needs a test |
| G13 | - | Help text is stale, the example still shows the old selector, and `pending_recovery` omits `decision.human` |

## 4. Obligations from other plans and constraints

- **BRIEF-03.6:15.** No generic event API may settle `decision.human` or protected actions. The event
  inbox must therefore never match a decision wait.
- **REPLACEMENT-MAP 63-65, 72-74 and 83.** A human decision owns assignment, deadline,
  comments/feedback, artifact identity, submission and audit. An action approval is for one
  invocation digest and cannot broaden grants. No effect dispatches before approval; current access
  is rechecked at dispatch. Pages and links project the decision and are never authorization owners.
  Legacy tables are removed in 08 only after replacement tests pass, so phase 05 leaves
  `human_approvals` and `response_review*` untouched.
- **NOT_PLANNED 17-19.** No voting or quorum; one matching response per wait.
- **07 (44-78).** A round is a child run with its own decision. "timeout: declared timeout route",
  and acceptance on the last round never auto-approves. Phase 05 must leave a timeout outcome a
  child can map to `choice: "timeout"`.
- **08 (44-47, 51-60).** Matched outreach replies resume their intended wait, and unrelated messages
  start independent runs. HTTP routes for comments, decision submission and notification links,
  identical across entry points, are phase 08.
- **Root `AGENTS.md`.** Competing-claimant tests for every new concurrency protocol; focused
  per-batch checks and a final full gate; pinned config keys that the application reads; a
  raised-limit signal.

## 5. Risks to verify during implementation

1. **Reopening a parked I/O job after approval** (completed → pending, the next attempt). The
   precedent is `control_retry.rs:37`. It needs checking against the job triggers
   `workflow_action_capture_schedule` (10835) and `workflow_action_confirm_claim_retirement` (10912),
   and against attempt numbering and `max_retries`.
2. **The intent row must commit before the protected rollback.** The approval decision references
   `workflow_action_intents`, so this must be confirmed in `ActionService::prepare_tool` and the
   per-family prepare paths.
3. **The one-wait-per-execution lookups** in `waits::row`, the message receipt and effect-wait code,
   `cancel_on`, the polling join and `workflow_message_effect_wait_guard` all change if the
   uniqueness becomes per reason.
4. **`batch_commit::complete(Parked)` from a command path.** It must stay within the 1000 ms
   lock/statement timeouts used by `waits::lock` and leave the stack depth unchanged (box it).
5. **jsonschema 0.33 does not assert `format`.** Edited `data` with `format` keywords is accepted
   loosely, as it is for every other schema in the engine.

## 6. Plan-text amendments needed for explicit-only reviewers

Not edited yet; they are docs edits for the 05A batch once D1 is accepted.

- **`05-human-decisions-and-waits.md:18`.** "an authorized reviewer or reviewer group" becomes
  "one or more explicitly named reviewers (company users, or existing email-only external
  participants)".
- **`05-…:63-64`.** "Support an explicit reviewer or eligible company reviewer group, with the first
  valid authorized response settling the task." becomes "Support explicitly named reviewers only —
  no reviewer groups or membership-derived pools; the first valid authorized response settles the
  task."
- **`EXPANSION.md:323` (05.3).** "Deterministic eligible reviewers" becomes "Explicit, deterministic
  reviewers (no groups)".
- **`NOT_PLANNED.md:17`.** "One assigned reviewer or eligible group with first valid response."
  becomes "Explicitly named reviewers with first valid response; reviewer groups, voting and quorum
  are not planned."
- **`BRIEF-02.2.md:84`** is an accepted historical brief ("closed user/group selector"). Leave it
  as history and record the supersession in the phase 05 brief.
- **Code text** (05A source edits, not plan text): `registry/help.rs:71-73`,
  `registry/schemas.rs:24-29` and `registry/examples.rs:87-89`.

## 7. `NO_BACKWARD_COMPATIBILITY_NOTES`

- **These deliverables.** They are logs under `docs/logs/05/`, not plans, so they carry no note and
  create no obligation.
- **A phase 05 brief.** If D1 is accepted as a brief (for example
  `plan/workflow-foundation/BRIEF-05.md`, like BRIEF-04.11), it must open with the
  no-backward-compatibility note, and its path must be added to `NO_BACKWARD_COMPATIBILITY_NOTES` in
  `src/main.rs:153-179`. That is a source edit in 05A.
- **An existing gap.** The phase plans `05-…` to `10-…` already carry the note but are not listed
  (the list has README, BRIEF-04.10 and BRIEF-04.11). Recommendation: 05A adds `05-…` (and, if root
  agrees, `06-…` to `10-…`) in the same edit.
