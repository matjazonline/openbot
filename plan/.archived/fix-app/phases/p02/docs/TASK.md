# FIXAPP-P02 — Silent stalls (phase record)

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. No backfills, dual reads/writes, legacy decoders or compatibility shims.

## Identity

- Phase ID: `p02` (IDs `FIXAPP-P02-…`). Workflow: `implement-opus-reviewed-2_2`
  (skill dir `/Users/mac03/dev/openbot/.claude/skills/implement-opus-reviewed-2_2`).
- Record root: `/Users/mac03/dev/openbot/.worktrees/fix-app-p02/plan/fix-app/phases/p02/docs/`.
- Worktree / branch: `/Users/mac03/dev/openbot/.worktrees/fix-app-p02`, `fix-app/p02`.
  Starting commit `e6b0a51` (plan spans cite `4e92edd`; re-checked before edits). No preserved
  pre-existing changes (clean worktree at start).
- Root = Author (A): phase root subagent of the manager session, Opus 5.5 / `high` (set by the
  manager at spawn; manager-confirmed). Reviewer (B): Agent `general-purpose`, model `opus`,
  effort `xhigh`; agent ID recorded below once spawned.
- `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75` observed via `printenv` in this session.
- Resources (fix-app-I002): private Postgres 18 at `127.0.0.1:55402` (trust),
  `.env` `DATABASE_URL=postgres://mac03@127.0.0.1:55402/mail_agents`, tests on `mail_agents_test`
  and `own_database` siblings; `CARGO_TARGET_DIR` default `target/` inside this worktree.

## Requirements and sources

- Plan: `plan/fix-app/02-silent-stalls.md` (four independent batches), `plan/fix-app/README.md`
  (concurrency rules, verification).
- Instructions: `AGENTS.md`, `src/AGENTS.md`, `src/application/AGENTS.md`,
  `src/adapters/persistence/AGENTS.md`.
- Coordination: `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md` (writer: manager), last
  checked revision 1. Applicable: fix-app-I001 r1 (repo/plan rules; edit only named init-schema
  statements; final gate reset-db + check.sh) — acknowledged; fix-app-I002 r1 (isolation/resources)
  — acknowledged; fix-app-I003 r1 (commit accepted phase on `fix-app/p02`, no push, no
  COORDINATION.md commit) — acknowledged. No cross-phase notices target p02.
- Coordination revision 2 (checked 2026-10-31): fix-app-I004 r1 (user instruction) — do NOT run
  the full suite or `scripts/check.sh`; run targeted tests for affected code (nonzero counts) after
  `./scripts/reset-db.sh --all` when the schema changed; `cargo fmt --check` and compiling affected
  targets are fine; regenerate `.sqlx/` if macro queries changed — acknowledged; it supersedes the
  plan's "Final gate" (reset-db + check.sh). Manager notice (not a coordination ID): prefix cargo
  with `CARGO_INCREMENTAL=0` (shared disk); own `target/debug/incremental` deleted.

## Acceptance criteria (from the plan)

1. 64th email send: one SQL `IMMUTABLE` function `workflow_job_continuation_limit()` = 63 used by
   the CHECK, the three guard sites (+ effect_parked) and the four SQL strings; Rust uses a
   constant asserted equal to the function by a DB test; `publish_on` refuses an email send with no
   continuation left with the TOOL_LIMIT observation, writing nothing; the effect_parked guard
   branch requires `OLD.continuation_count < limit`. Tests `message_park_continuation_*`.
2. Schedule snapshot: `schedule_runs.schedule_snapshot` and its CHECK dropped; claim carries
   `schedule_id`, `company_id`, `channel_id` from row columns; no other reader. Tests
   `schedule_claim_*` (page of N returns N; poison batch two iterations).
3. Delivery late completion: `complete_part` fenced on execution + `sending` without expiry; worker
   renews leases so the last delivery of a slow batch completes. Tests `delivery_late_completion_*`.
4. Stale notification: `release_stale_projection` dead-letters at `attempt_count >= max_attempts`
   and backs off like the fail path. Tests `notification_stale_*` incl. poison batch.

## Batching

All four plan batches form one review batch **B1** (small, independent, one coherent phase); one
architecture gate **G1** for the decisions below (schema + changed lease/completion protocol).
Batch 4 (notification) uses the established fail-path pattern and is not gated.

## Architecture gate G1 — decision revision r1 (2026-10-31)

### D1 — One continuation limit; email refused before send (schema + guard)

- Decision: add `public.workflow_job_continuation_limit() RETURNS smallint LANGUAGE sql IMMUTABLE
  RETURN 63` (placed before `workflow_message_action_capture`, before `background_tasks`).
  `background_tasks_continuation_count_check` becomes `BETWEEN 0 AND
  workflow_job_continuation_limit()`; in `check_workflow_fenced_completion` the approval and child
  branches and the reopen bound (`NEW.continuation_count > …`) use the function, and the
  `effect_parked` branch gains `AND OLD.continuation_count < workflow_job_continuation_limit()`.
  SQL strings `decision_approvals.rs` (park head, close_attempt, reopen_job) and
  `decision_tokens.rs` (head) use the function. Rust constant
  `WORKFLOW_JOB_CONTINUATION_LIMIT: i16 = 63` in `adapters/persistence/workflow/mod.rs`, used by
  `workflow_tool_calls.rs` and the new message check; a DB test asserts equality.
- Message refusal: in `publish_on`, after authorization/locks, the replay (`existing`) check and
  route recheck, and before the admission probe and the marker (first write), when
  `plan.has_delivery()` read the fenced job's `continuation_count`; if `>= limit` return
  `End::Rollback(MessagePublication::ContinuationLimit)`. New variant flows to
  `MessageObservation::ContinuationLimit`; the agent adapter maps it to
  `AgentToolObservation::Completed(tool_error(TOOL_LIMIT, …))` — the same observation shape as
  `workflow_tool_calls.rs:118-123`. A step-origin send maps it to Terminal `message.action_limit`
  (a step job reaches at most one continuation — one approval per invocation — so this is a
  defensive mapping, never a stall).
- Why after the replay check, not literally first: a replay of an already-published call must
  still read its stored receipt at the limit (observe normally answers first; this keeps
  `publish_on` correct if it is reached anyway). Why only delivery plans: an all-in-app send
  never parks and needs no continuation; approval parks already check the limit themselves.
- Invariants: a job at the limit can never be left in an effect park it cannot resume; nothing is
  written on refusal; Rust and SQL agree on 63 (DB test).
- Failure modes: SQL guard catches any writer bypassing Rust (`23514`, transaction rolls back).
- Contracts: `MessagePublication`/`MessageObservation` gain one variant (exhaustive matches in
  `message_runtime.rs`, `workflow_agent/mod.rs`). Plan 03 later classifies this guard's errors.

### D2 — Drop the schedule snapshot (schema)

- Drop `schedule_runs.schedule_snapshot` and `schedule_runs_snapshot_object_check`; in
  `require_schedule_occurrence_scope()` drop the snapshot from the immutable identity tuple and
  delete the snapshot-scope check (the column no longer exists; the remaining relational scope
  checks are unchanged). This trigger function is not named in the plan but must change with the
  column (recorded as an init-schema overlap note).
- `ClaimedScheduleRun { id, materialization_generation, scheduled_for, binding_id, schedule_id,
  company_id, channel_id }` read from row columns (the claim's `RETURNING`); remove the two
  snapshot writes (`claim_due`, `capture_now`); admission reads the ids directly. No decode after
  commit remains, so no row can fail a page.
- Failure mode removed: one undecodable row failing a whole committed claim page.

### D3 — Delivery completion fence and batch lease renewal (concurrency protocol)

- `complete_part` part UPDATE uses a new `own_fence_sql()` (delivery alias): `delivery.id=$2 AND
  delivery.status='sending' AND delivery.execution_id=$3 AND delivery.owner_worker_id=$4` — no
  expiry. `begin_part` keeps `live_fence_sql()` (= own fence + `lock_expires_at > now`): no new
  provider request may start on a lapsed lease. `lock_held_delivery_on` and the parent
  settle/retry statements already use the expiry-free `own_row_fence_sql()`.
- Reaper confirmed (`reap_expired_leases_on`, `queue.rs` ~626-700): it locks the parent
  `FOR UPDATE SKIP LOCKED` and sets `status` to `outcome_unknown`/`retryable`/settled and clears
  `execution_id`/`owner_worker_id` in the same transaction, so after a reap the own fence matches
  nothing; a re-claim mints a new `execution_id`. `complete_part` locks the parent first via
  `lock_held_delivery_on`, so a concurrent reaper skips the row (SKIP LOCKED) and cannot interleave.
- Renewal: replace the unused single-row `renew_delivery_lease(fence, until)` port method with
  `renew_delivery_leases(fences: &[ExecutionLease<DeliveryId>], lease_for: Duration) ->
  AppResult<Vec<DeliveryId>>` (one UPDATE over `unnest` of (id, execution, owner); still
  requires a live lease `lock_expires_at > now`; DB clock, as the claim). In `drain_once`, before
  each delivery the worker renews every not-yet-processed delivery of the batch (the current one
  included); a delivery whose renewal did not come back is skipped with a lease-lost warning and no
  provider call; renewal errors skip the current delivery the same way (its lease then lapses and
  the reaper charges an attempt, as today).
- Why the whole remainder, not only the next delivery: with one lease over the batch, renewing
  only the delivery about to send cannot help a delivery whose turn arrives after the original
  expiry (renewal requires a live lease) — exactly the plan's "last delivery in the batch" case.
  Renewing the remainder keeps every waiting delivery live while the worker makes progress.
- Lease duration becomes a worker field (`DELIVERY_LEASE_SECONDS` default) with a builder so a
  test can shorten it; `drain_once` becomes `pub(crate)` (as `NotificationWorker::drain_once`).
- Invariants: completion only by the execution on the row and only while `sending`; a stale
  execution (reaped or replaced) gets `LeaseLost` and changes nothing; no provider request starts
  without a live lease.
- Verification: DB tests late-completion-after-expiry (delivered + provider key), reaped-first
  (`LeaseLost`, stays `outcome_unknown`), short-lease worker batch where the last delivery
  completes; existing competing-claimant/fence tests in `delivery/tests.rs` rerun.

### G1 decision revision r2 (2026-10-31) — responses to FIXAPP-P02-G1-R1

r1 text above is preserved; r2 amends it as follows.

- F3 (D1 test interpretation, accepted): `prepare_tool`/`prepare_step` persist the
  `workflow_action_intents` row before `publish_on`. "Creates no intent, delivery or wait" is read
  as: no dispatch marker, message action, receipt, destination, thread/message, delivery or wait is
  written, the job/attempt stay `processing` — the same contract as the existing `ActionLimit`
  refusal. The test asserts those tables exactly (`rows()` helper) plus job state.
- F7 (declined, out of scope): approval-required sends at the limit keep today's
  `ApprovalRequired` error path; the plan names only the email park. Recorded for a later plan.
- D3 r2 replaces the renewal design and adds tests:
  - Lock order (F2, accepted): `renew_delivery_leases` is one statement
    `WITH held AS (SELECT delivery.id FROM message_deliveries AS delivery JOIN unnest($1::uuid[],
    $2::uuid[], $3::uuid[]) AS fence(id, execution_id, owner_worker_id) ON … WHERE
    delivery.status='sending' AND delivery.lock_expires_at > CURRENT_TIMESTAMP ORDER BY
    delivery.id FOR UPDATE OF delivery) UPDATE message_deliveries AS delivery SET lock_expires_at
    = CURRENT_TIMESTAMP + lease, updated_at = … FROM held WHERE delivery.id = held.id RETURNING
    delivery.id` — rows locked in id order (the convention of the reaper, cancellation and receipt
    consumer), live-lease condition kept, no SKIP LOCKED.
  - Granularity (F4, accepted): the worker renews before **each part** (which includes before each
    delivery's first part), renewing the current delivery plus every not-yet-processed delivery of
    the batch, through a small `BatchLeases` holder owned by `drain_once` and passed to `deliver`.
    A delivery whose fence did not come back is dropped from the set; if it is the current one the
    worker stops it (lease-lost warning, no provider call). Processed deliveries leave the set.
  - Errors (F5, accepted): a renewal *error* is logged and the worker proceeds to `begin_part`,
    whose live fence is the authority; the set is left unchanged.
  - Competing claimants (F1, accepted): the expiry-free completion fence is a deliberate exception
    to the persistence AGENTS.md "completion stays conditional on a live lease" guard. Justification:
    `message_deliveries_lease_check` forces `execution_id IS NULL` whenever `status <> 'sending'`;
    the claim takes only `pending`/`retryable`; the reaper changes status and clears the execution
    under the parent row lock (`FOR UPDATE SKIP LOCKED`), and `complete_part` takes that lock first
    (`lock_held_delivery_on`). So "status = sending and execution = mine" is ownership; expiry only
    marks reaper eligibility. Tests added (own database, `tokio::sync::Barrier`, repeated rounds):
    `delivery_late_completion_races_the_reaper_and_exactly_one_wins` (expired-lease row with a
    started part: either `complete_part` → Applied(Delivered) and the row/part are delivered with
    the provider key, or `LeaseLost` and the row/part are `outcome_unknown`; never both) and
    `delivery_late_completion_renewal_races_the_reaper_and_exactly_one_wins` (lease at the expiry
    boundary: either renewed and still `sending` under this execution, or reaped and not renewed;
    never both).
- F6 (accepted, in D2's spirit): the `claim_due` capture loop selects only `id, company_id,
  channel_id, binding_id, next_run_at` of due schedules, so no schedule row decode can fail the
  capture/claim transaction for every tenant.
- F8 (partly accepted): the adapter logs a structured warning (event id, attempts) when a stale
  release dead-letters; the census already counts `dead_letter` events. No schema change (no stale
  error class in the CHECK; plan 08 owns error-class rules).

### D4 (not gated, for record) — Stale notification projection

- `release_stale_projection`: `status = CASE WHEN attempt_count >= max_attempts THEN 'dead_letter'
  ELSE 'pending' END`, `available_at = CURRENT_TIMESTAMP + (2 * POWER(2, LEAST(attempt_count, 8)))
  seconds` (fail-path backoff); fence unchanged. No schema change; `last_error_class` stays NULL
  (the error-class CHECK has no stale class; plan 08 owns error-class rules).

## Batch B1 status

- G1 r2: PASS by B (FIXAPP-P02-G1-R2). F9 (optional, deterministic lock-order test) adopted.
- Implementation: complete for D1-D4 (+F4/F5/F6/F8/F9). Awaiting B batch review.
- Files: `migrations/20260817000000_init_schema.sql` (new function; `check_workflow_fenced_completion`
  4 sites; `background_tasks_continuation_count_check`; `require_schedule_occurrence_scope`;
  `schedule_runs` column + CHECK); `adapters/persistence/workflow/{mod.rs, decision_approvals.rs,
  decision_tokens.rs, workflow_tool_calls.rs, message_publish.rs, schedules.rs}`;
  `application/workflow/{message.rs, message_runtime.rs, schedule_admission.rs}`;
  `adapters/workflow_agent/mod.rs`; `domain/entities/schedule.rs`;
  `adapters/persistence/delivery/queue.rs`; `application/transport/queue.rs` (port: renewal);
  `application/services/delivery_worker.rs`; `adapters/persistence/notification.rs`. Tests:
  new `workflow/message_park_continuation_tests.rs`, `delivery/late_completion_tests.rs`; edits
  in `notification_tests.rs`, `schedule_tests.rs` (new page test; poison test renamed to
  `schedule_claim_poison_batch_…`; snapshot SQL removed), `admission_source_tests.rs`,
  `delivery/tests.rs`, `delivery/cancellation_tests.rs` (renewal call sites),
  `delivery_worker_tests.rs`, `workflow_agent/mod.rs` (mapping unit test).
- Hand-written park test note: a first attempt planted the count with a fixture trigger; it
  cannot work because `attempt_ordinal` is generated from `continuation_count`, so the fence no
  longer matches. Replaced by a SQL-only clone of a full model-tool publication + park under the
  current fence: it commits at 62 (control) and is refused with 23514 at 63.
- Decision detail: renewal error keeps the set and proceeds to `begin_part` (F5);
  `BatchLeases` renews before every part (F4).

## Checks (commands and results)

Environment: worktree `fix-app/p02` at base e6b0a51 + working tree; Postgres 127.0.0.1:55402;
`DATABASE_URL=postgres://mac03@127.0.0.1:55402/mail_agents` (tests on `mail_agents_test` and
`own_database` siblings); `SQLX_OFFLINE=true`; `CARGO_INCREMENTAL=0` from the second build on.

- `./scripts/reset-db.sh --all --yes` after the schema edits: exit 0
  (`logs/reset-db-b1.log`); `workflow_job_continuation_limit()` = 63; `schedule_runs` has no
  `schedule_snapshot`.
- Focused (each nonzero): `cargo test --locked --offline --lib notification_stale_` 3 passed;
  `schedule_claim_` 2 passed; `delivery_late_completion_` 7 passed (6 DB + 1 worker unit);
  `message_park_continuation_` 5 passed (4 DB + agent mapping unit, the latter also run alone).
  Logs `logs/focused-*.log`.
- Neighbours: `cargo test --locked --offline --lib -- workflow_action_approval workflow_tool
  token decision workflow_message message_park schedule delivery notification continuation
  admission_source workflow_agent` → 397 passed, 0 failed (`logs/neighbours-b1.log`).
- `cargo fmt --all -- --check`: pass. `cargo clippy --locked --offline --lib --tests -- -D
  warnings`: pass (`logs/clippy-b1.log`).
- `.sqlx/`: no macro-checked query touches the changed tables (grep of `.sqlx/*.json` for
  `schedule_runs|background_tasks|message_deliveries|notification_events`: none); not regenerated.
- Not run per fix-app-I004 r1: full suite, `scripts/check.sh`, stack budget.
- Superseded (FIXAPP-P02-B1-F1): `logs/focused-message_park_continuation_.log` records a FAILED
  first run (08P01 bind error, fixed by binding the digest); the author's logs above also predate
  edits made at 21:25. The checks on B1 digest `382bd557…` are the reviewer's reruns:
  `logs/review-b1-focused.log` (17 passed, nonzero for all four plan filters + agent mapping),
  `logs/review-b1-neighbours.log` (443 passed), `logs/review-b1-fmt.log` (pass); clippy
  `logs/clippy-b1.log` (21:27, after the last source edit).

## B1 review round 1 resolution (FIXAPP-P02-B1-R1: PASS, optional F1-F3)

- F1 (records): fixed — "Superseded" entry above cites the review logs.
- F2 (renewal cost): declined for this phase. The persistence guide asks for evidence before
  optimizing; the batch-wide renewal is the gated G1 r2 F4 behaviour and correct, and a time
  threshold adds a second clock to a fence that was just made deadlock-free. Recorded as a
  candidate for when delivery write load is measured.
- F3 (renewal-error test): fixed — `ScriptedQueue` can script a renewal `Err`; new
  `delivery_late_completion_a_renewal_error_still_sends` asserts one renewal, one `begin_part`,
  one completion, and the lease still in `BatchLeases::held`. Only
  `src/application/services/delivery_worker_tests.rs` changed.
- Checks after the fix (`CARGO_INCREMENTAL=0`, no schema change so no reset):
  `cargo test --locked --offline --lib delivery_late_completion_` → 8 passed
  (`logs/fix-b1-focused.log`); `services::delivery_worker` → 10 passed (`logs/fix-b1-worker.log`);
  `cargo fmt --all -- --check` pass; `cargo clippy --locked --offline --lib --tests -- -D warnings`
  pass (`logs/fix-b1-clippy.log`).

## Cross-phase notes

- init_schema statements edited by p02 (merge-risk): new function before
  `workflow_message_action_capture`; `check_workflow_fenced_completion`; `background_tasks`
  CHECK `background_tasks_continuation_count_check`; `schedule_runs` table (column + CHECK);
  `require_schedule_occurrence_scope()`.
- For p03: the single limit is `workflow_job_continuation_limit()`; the effect_parked branch now
  also bounds the count. For p08: notification stale path now dead-letters (no error class; the
  `notification_events_error_check` has no stale class).
- For p05 (Known overlap, delivery code): `DeliveryQueue::renew_delivery_lease(fence, until)` was
  replaced by `renew_delivery_leases(fences, lease_for) -> Vec<DeliveryId>` (id-ordered lock);
  `complete_part` now fences with `own_fence_sql()` (no expiry); `live_fence_sql()` is built on it.
  Any p05 change to `delivery/queue.rs`, `cancellation.rs` or `DeliveryQueue` impls must rebase onto
  this.
- `graft/` graph not rebuilt in this phase (untracked local index; left to the
  manager after merges).

## Agents

- B: agent `a8d7d7abb4d07fbc5` (general-purpose, opus, xhigh). G1-R1: changes required (F1-F3 actionable, F4-F8 optional).

## Next action

Phase accepted: FIXAPP-P02-B1-R2 PASS at digest 38bf8facc727de388b98561499c803b9324db75f92c54332294699465c2b5bbb. Committed on `fix-app/p02` per I003; manager merges.
