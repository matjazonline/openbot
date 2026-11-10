# FIXAPP-P02 — agent review findings

Workflow `implement-opus-reviewed-2_2`. Task record: [TASK.md](TASK.md). Review index:
[REVIEW.md](REVIEW.md). Review notes: [agent-review-notes.md](agent-review-notes.md).

Common context for the G1 r1 findings below: date 2026-10-31; batch B1; stage architecture gate
G1, decision revision r1 (TASK.md "Architecture gate G1 — decision revision r1"); review round
[FIXAPP-P02-G1-R1](agent-review-notes.md#fixapp-p02-g1-r1--architecture-gate-g1-decision-revision-r1);
reviewer Opus 5.5 / xhigh (Reviewer B); phase p02; worktree
`/Users/mac03/dev/openbot/.worktrees/fix-app-p02`; HEAD `e6b0a510ec27a636bf76c9aa9312605ca3e5c8cb`,
digest `9d5bde9263086a6007787848a407759f6e8a3ca7b391addfaef755df2fee4c66` (exclusion
`plan/fix-app/phases/p02/docs`). No code changed yet; every finding is against the recorded
decision text, checked against the code at HEAD.

---

## FIXAPP-P02-G1-F1 — D3 changes a concurrency protocol without a competing-claimant test, and does not reconcile the "live lease on completion" guard rule

- **Classification:** actionable defect · severity **Medium** · category verification / architecture
  (originates in gated decision D3 r1).
- **Evidence:**
  - D3 removes `lock_expires_at > now` from the `complete_part` part update
    (`src/adapters/persistence/delivery/queue.rs:265-277`, `live_fence_sql` `:808-815`) and adds a
    new multi-row renewal protocol used by the worker. Its verification plan is the three plan
    tests (expire→complete, reap→complete, short-lease worker batch — all sequential) plus
    "existing competing-claimant/fence tests rerun". None of the existing tests
    (`delivery/tests.rs:376` `two_claimants_never_own_the_same_delivery`, `:440`
    `a_stale_execution_cannot_write_over_the_one_that_replaced_it`) exercise an expiry-free
    completion or a renewal racing the reaper.
  - Root `AGENTS.md`: "When adding or changing a concurrency protocol, include a test with
    competing claimants rather than relying only on sequential mocks."
  - `src/adapters/persistence/AGENTS.md` "Preserve the existing correctness guards": "completion,
    failure, payload replacement, and lease renewal stay conditional on the current worker owning
    a live lease … When a proposed simplification removes one of these predicates …, assume it
    changes correctness until a concurrency or adversarial database test proves otherwise." D3
    removes exactly that predicate from completion (as the plan directs) but neither acknowledges
    the rule nor plans the adversarial test that the rule makes the condition for doing so.
  - Inspection supports the decision itself (not the gap): `message_deliveries_lease_check`
    (`init_schema.sql:3792`) forces `execution_id`/`owner_worker_id` NULL whenever status is not
    `sending`; the claim only takes `pending|retryable` (`is_claimable`,
    `domain/entities/transport/delivery.rs:101-103`); every reaper branch clears the execution under
    the parent lock (`queue.rs:632-715`, `cancellation.rs:367-400`); `lock_held_delivery_on`
    (`cancellation.rs:262-268`) takes the parent `FOR UPDATE` before the part write. So the
    expiry-free own fence is an exact ownership witness — but that is an inspected claim, not a
    tested one.
- **Required correction (decision text):** record in D3 that the delivery queue deliberately
  departs from the persistence guide's live-lease rule for completion (as it already does for
  fail/settle via `own_row_fence_sql`), with the invariant above as the justification, and add to
  D3's verification plan competing-claimant DB tests (own database, barrier-synchronised), at
  minimum: (a) an expired-but-unreaped delivery with a started part where `complete_part` races
  `reap_expired_deliveries` (optionally followed by a re-claim): exactly one side wins — either
  `delivered` + provider key and the reaper changes nothing, or `outcome_unknown` and
  `LeaseLost` with nothing of the late completion applied; (b) `renew_delivery_leases` racing the
  reaper on a lease at its expiry: renewed XOR reaped, never both.
- **Instruction lesson:** observation — the gate template's "verification plan" item was filled
  for D3 from the plan's sequential test list only. Candidate (skill, architecture-gate step): "A
  gated decision that removes or weakens a predicate listed under a scoped guide's 'preserve'
  rules must cite the rule and include the adversarial/competing test that rule requires."
- **Resolution history:** 2026-10-31 — open (G1 r1).
  - 2026-10-31 (G1-R2, decision revision r2, TASK.md sha256 `91e6e9b6…`, round [FIXAPP-P02-G1-R2](agent-review-notes.md)) — **fixed in decision.** r2 records the deliberate exception to the persistence guide with the lease_check / claim-statuses / reaper-lock justification and adds two own-database, barrier-synchronised, repeated-round tests: late `complete_part` vs reaper (Applied(Delivered)+key XOR LeaseLost+outcome_unknown) and renewal vs reaper (renewed-and-sending XOR reaped-and-not-renewed). Tests themselves to be verified in the batch review.
  - 2026-10-31 (B1-R1, digest `382bd557…`, round [FIXAPP-P02-B1-R1](agent-review-notes.md)) — **implementation verified.** `delivery_late_completion_races_the_reaper_and_exactly_one_wins` and `delivery_late_completion_renewal_races_the_reaper_and_exactly_one_wins` (own database, Barrier, 12 rounds) read and rerun: pass (logs/review-b1-focused.log). The renewal race also admits "neither" (renewal after expiry, sweep before) and asserts never both — consistent with the invariant. Closed.

---

## FIXAPP-P02-G1-F2 — D3 batch renewal locks parents in claim order, not id order; deadlocks with id-ordered lockers

- **Classification:** actionable defect · severity **Medium** · category concurrency / architecture
  (originates in gated decision D3 r1).
- **Evidence:**
  - D3 renews "every not-yet-processed delivery of the batch" in "one UPDATE over `unnest` of
    (id, execution, owner)". A multi-row `UPDATE … FROM unnest(…)` acquires row locks in plan /
    input order (the worker's `fair_order` remainder, `delivery_worker.rs:454-490`), not id order,
    and it is the first blocking multi-parent lock on the worker side (the claim uses
    `SKIP LOCKED`; begin/complete/fail/release lock one parent).
  - Other producers lock several parents with blocking locks **in id order**:
    `cancel_workflow_deliveries_on` (`cancellation.rs:93-103`, `ORDER BY delivery.id FOR UPDATE OF
    delivery`, includes `sending` rows) and the workflow receipt consumer
    (`workflow/message_receipts.rs:119-123`, `ORDER BY delivery.id FOR SHARE OF delivery`, which
    conflicts with the renewal's row-update lock). The reaper documents the convention
    (`queue.rs:641-642`: "Parents first, in id order, the lock every other producer holds before a
    transition, so … never deadlocks").
  - Failure scenario: a workflow email to three recipients yields three deliveries that commonly
    land in one claim batch (fair order keeps up to 4 per company per pass). While the worker
    renews them (locking, say, D3 then D1), the run is cancelled or its receipt consumer runs
    (locking D1 then D3) → `40P01`. Either the renewal errors — and D3's error path skips the
    current delivery, letting its lease lapse and charging it an attempt — or the cancellation /
    receipt transaction is aborted.
- **Required correction (decision text):** state the lock-order invariant for renewal and take the
  parent locks in id order before updating (e.g. a CTE `SELECT id FROM message_deliveries WHERE id
  = ANY($1) ORDER BY id FOR UPDATE` joined to the fenced `UPDATE`, keeping the live-lease
  predicate). Do not switch to `SKIP LOCKED` for renewal: a row briefly locked by a cancellation
  that keeps the lease would then be reported "lost" and abandoned. Cover with F1(b)-style test or
  a renewal-vs-cancellation competing test.
- **Instruction lesson:** cause — the id-order convention is stated only in a code comment in
  `reap_expired_leases_on`; it is not in `src/adapters/persistence/AGENTS.md`. Candidate (persistence
  guide, "Fence leases" section): "Any statement that takes blocking locks on more than one queue
  parent locks them in primary-key order, as every other multi-row producer does."
- **Resolution history:** 2026-10-31 — open (G1 r1).
  - 2026-10-31 (G1-R2, decision revision r2, TASK.md sha256 `91e6e9b6…`, round [FIXAPP-P02-G1-R2](agent-review-notes.md)) — **fixed in decision.** Renewal is one statement whose CTE joins the fences, keeps `status='sending' AND lock_expires_at > CURRENT_TIMESTAMP`, and locks `ORDER BY delivery.id FOR UPDATE OF delivery` (no SKIP LOCKED) before the UPDATE; LockRows above the sort acquires locks in id order, matching the reaper, cancellation and receipt consumer. The r2 competing tests race the reaper (SKIP LOCKED) and so do not exercise the ordering itself; a deterministic ordering regression test is logged as optional [FIXAPP-P02-G1-F9](#fixapp-p02-g1-f9--optional-deterministic-lock-order-regression-test-for-renewal).
  - 2026-10-31 (B1-R1, digest `382bd557…`, round [FIXAPP-P02-B1-R1](agent-review-notes.md)) — **implementation verified.** Renewal CTE locks `ORDER BY delivery.id FOR UPDATE OF delivery` with the live predicate; `delivery_late_completion_renewal_locks_in_id_order` passes. Closed.

---

## FIXAPP-P02-G1-F3 — D1 does not record how the plan's "creates no intent" test is read; the action intent is written before `publish_on`

- **Classification:** actionable defect · severity **Low** · category requirements / verification
  (gated decision D1 r1).
- **Evidence:**
  - Plan batch 1 test: "At the limit, the send returns the tool-limit error and creates no intent,
    delivery or wait." The plan also places the refusal "at the top of `publish_on`".
  - `MessageActions::call_tool` (`application/workflow/message_runtime.rs:62-68`) and `step`
    (`:118-122`) call `ActionService::prepare_tool` / `prepare_step` before `publish`; those persist
    the immutable `workflow_action_intents` row (and the model-call mapping) in their own commit
    (`actions/service.rs:64-72`; see `action_tests.rs:37-68`). So with both the plan's and D1's
    placement an intent row exists after the refusal; the literal assertion cannot pass.
  - The existing `ActionLimit` refusal (`message_publish.rs:347-349`, documented "nothing was
    written" at `message.rs:375-376`) has the same shape: it means no marker/action/destinations.
- **Required correction (decision text):** record the interpretation: "no intent" = no dispatch
  marker (`workflow_action_dispatches`), no `workflow_message_actions`/receipt, no destination,
  thread, message, delivery or wait; the prepared `workflow_action_intents` row is expected and
  replays to the same refusal. Name these exact tables in the `message_park_continuation_*` test
  assertions. (Moving the refusal ahead of `prepare` would contradict the plan's placement and
  is not required.)
- **Instruction lesson:** observation — plan test wording used "intent" loosely for a table that a
  separate earlier transaction writes. No instruction change supported beyond the existing
  "record requirement interpretations in TASK.md".
- **Resolution history:** 2026-10-31 — open (G1 r1).
  - 2026-10-31 (G1-R2, decision revision r2, TASK.md sha256 `91e6e9b6…`, round [FIXAPP-P02-G1-R2](agent-review-notes.md)) — **fixed in decision.** Interpretation recorded (no marker, message action, receipt, destination, thread/message, delivery or wait; job/attempt stay `processing`; prepared intent expected), asserted table by table.
  - 2026-10-31 (B1-R1, digest `382bd557…`, round [FIXAPP-P02-B1-R1](agent-review-notes.md)) — **implementation verified.** `message_park_continuation_at_the_limit_is_refused_before_any_write` asserts `rows()` (markers, actions, destinations, deliveries, receipts, waits, messages) unchanged plus run/job/attempt state. Closed.

---

## FIXAPP-P02-G1-F4 — (optional) Renew before each provider call, not only before each delivery

- **Classification:** optional suggestion · severity Low · category robustness (D3 r1).
- **Evidence:** a delivery may hold up to 50 parts (`message_delivery_parts_index_check`,
  `init_schema.sql:3826`) sent sequentially (`delivery_worker.rs:161-193`); the code itself notes
  "a multi-part send can outlive its lease between parts". With renewal only per delivery, a
  multi-part delivery whose parts take longer than the lease loses its own lease mid-send (next
  `begin_part` → `LeaseLost`, reaper charges an attempt) and lets every remaining batch lease lapse,
  so the remainder is skipped and charged too. D3's own argument for renewing the remainder applies
  equally per part. Renewing the remainder before each `begin_part` subsumes "before each
  delivery" at no extra design cost.
- **Resolution history:** 2026-10-31 — open (optional; does not block G1).
  - 2026-10-31 (G1-R2, decision revision r2, TASK.md sha256 `91e6e9b6…`, round [FIXAPP-P02-G1-R2](agent-review-notes.md)) — **adopted.** Renewal before each part via a `BatchLeases` holder owned by `drain_once`, covering the current delivery plus the unprocessed remainder; unreturned fences leave the set; a missing current fence stops that delivery before any provider call.

---

## FIXAPP-P02-G1-F5 — (optional) A renewal *error* need not abandon the current delivery

- **Classification:** optional suggestion · severity Low · category failure handling (D3 r1).
- **Evidence:** D3 skips the current delivery on a renewal error. The plan only requires stopping
  "when renewal reports it lost". `begin_part` is itself fenced on a live lease
  (`queue.rs:220-246`), so proceeding to it after a renewal error is safe; skipping strands a
  possibly still-live claim until expiry and charges an attempt, which dead-letters a delivery
  that was on its last attempt because of a transient DB error. Also state that skipped deliveries
  leave the renewal set (otherwise later renewals keep re-extending a claim the worker will never
  send).
- **Resolution history:** 2026-10-31 — open (optional; does not block G1).
  - 2026-10-31 (G1-R2, decision revision r2, TASK.md sha256 `91e6e9b6…`, round [FIXAPP-P02-G1-R2](agent-review-notes.md)) — **adopted.** A renewal error is logged and the worker proceeds to the live-fenced `begin_part`; set unchanged; processed deliveries leave the set.

---

## FIXAPP-P02-G1-F6 — (optional) Capture loop still decodes the whole schedule entity inside the claim transaction

- **Classification:** optional suggestion · severity Low · category robustness (D2 r1, adjacent).
- **Evidence:** after D2 the capture loop in `claim_due` (`schedules.rs:633-650`) still selects
  `COLUMNS` and calls `row.schedule()?` (`ScheduleType`/`ScheduleTimezone::from_str`,
  `schedules.rs:43-62`) for every due schedule, although with the snapshot write gone it needs only
  `id`, `next_run_at` and the three ids (which the INSERT trigger derives anyway,
  `init_schema.sql:1541-1553`). One `channel_schedules` row that no longer parses rolls back the
  capture *and* the claim for every tenant on every poll — the same one-row-fails-the-page shape
  the plan's Outcome targets. Selecting only the needed columns removes it.
- **Resolution history:** 2026-10-31 — open (optional; does not block G1).
  - 2026-10-31 (G1-R2, decision revision r2, TASK.md sha256 `91e6e9b6…`, round [FIXAPP-P02-G1-R2](agent-review-notes.md)) — **adopted.** Capture loop selects only `id, company_id, channel_id, binding_id, next_run_at`.

---

## FIXAPP-P02-G1-F7 — (optional) Approval-required send at the limit still surfaces a misleading error

- **Classification:** optional suggestion · severity Low · category consistency (D1 r1, pre-existing).
- **Evidence:** the approval branch runs before D1's check (`message_publish.rs:317-331`);
  `approvals::park` returns `None` when `!head.remaining` (`decision_approvals.rs:56,65`), giving
  `ApprovalRequired` → agent adapter `Err(approval_required())` "Protected tool requires an
  agent.run approval policy" (`workflow_agent/mod.rs:182,193-195`). Not a stall and outside the
  plan's scope; noted so the TOOL_LIMIT refusal can be made uniform later (e.g. map the
  out-of-continuations park refusal to the same observation).
- **Resolution history:** 2026-10-31 — open (optional; does not block G1).
  - 2026-10-31 (G1-R2, decision revision r2, TASK.md sha256 `91e6e9b6…`, round [FIXAPP-P02-G1-R2](agent-review-notes.md)) — **declined by author (out of plan scope), accepted by reviewer.** Pre-existing, not a stall; recorded for a later plan. Closed as deferred.

---

## FIXAPP-P02-G1-F8 — (optional, D4) Stale → dead_letter is not distinguishable from a requeue

- **Classification:** optional suggestion · severity Low · category observability (D4, not gated).
- **Evidence:** `release_stale_projection` returns `()` (`notification.rs:530-548`) and the caller
  reports `Stale` either way (`:226-233`); with D4 a stale event at its last attempt goes
  terminal with NULL `last_error_class`/`last_error_detail` (the claim clears them, `:187`). The
  worker therefore cannot log or count the terminal transition (`src/AGENTS.md`: metrics must
  distinguish retries from terminal failures). Returning the resulting status lets the worker
  record it.
- **Resolution history:** 2026-10-31 — open (optional; not gated).
  - 2026-10-31 (G1-R2, decision revision r2, TASK.md sha256 `91e6e9b6…`, round [FIXAPP-P02-G1-R2](agent-review-notes.md)) — **partly adopted, accepted.** Structured warning (event id, attempts) when a stale release dead-letters; census already counts `dead_letter` events (`notification.rs:312`); no schema change (error-class CHECK owned by plan 08). Closed.

---

## FIXAPP-P02-G1-F9 — (optional) Deterministic lock-order regression test for renewal

- **ID and context:** 2026-10-31; G1 recheck, decision revision r2; round
  [FIXAPP-P02-G1-R2](agent-review-notes.md); reviewer Opus 5.5 / xhigh; TASK.md sha256 `91e6e9b6…`.
- **Classification:** optional suggestion · severity Low · category verification (D3 r2).
- **Evidence:** r2's competing tests race renewal against the reaper, which uses `SKIP LOCKED`, so
  neither would fail if the renewal locked parents out of id order (the F2 defect). A deterministic
  test can pin the ordering without relying on deadlock timing: transaction A locks the lower-id
  delivery `FOR UPDATE` and holds it; start `renew_delivery_leases` over {higher, lower} in the
  background; after a short pause A locks the higher-id delivery with a `lock_timeout`; with
  id-ordered renewal A succeeds (renewal is still waiting on the lower row), commits, and the
  renewal then returns both; with input-order locking A times out or deadlocks.
- **Instruction lesson:** none beyond F2's candidate.
- **Resolution history:** 2026-10-31 — open (optional; does not block G1).
  - 2026-10-31 (B1-R1, digest `382bd557…`, round [FIXAPP-P02-B1-R1](agent-review-notes.md)) — **adopted and verified** as `delivery_late_completion_renewal_locks_in_id_order` (wrong ordering would surface as the renewal's deadlock error at the final unwrap). Closed.

---

# Batch review B1 — round FIXAPP-P02-B1-R1

Common context for the B1 findings below: date 2026-10-31; batch B1 (whole phase, plan sections
1-4); stage batch review; review round
[FIXAPP-P02-B1-R1](agent-review-notes.md); reviewer Opus 5.5 / xhigh (Reviewer B); phase p02;
worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p02`; HEAD
`e6b0a510ec27a636bf76c9aa9312605ca3e5c8cb` + working tree, digest
`382bd557ff5dbf75c15e46d18f8cb438de0d52162df1af08f569c8ec23bf2614` (exclusion
`plan/fix-app/phases/p02/docs`); gated decisions G1 r2; coordination revision 2 (I001-I004).

---

## FIXAPP-P02-B1-F1 — (optional) Recorded test evidence predates the frozen artifact and one cited log shows a failure

- **Classification:** optional suggestion (records accuracy) · severity Low · category verification.
- **Evidence:** TASK.md "Checks" states `message_park_continuation_` "5 passed" and cites
  `logs/focused-*.log`, but `logs/focused-message_park_continuation_.log` records a FAILED run
  (08P01 bind error in the SQL-only park test, plus an unused-variable warning); the passing
  evidence is `focused-guard.log` (that test alone) and `neighbours-b1.log`. Ten artifact files
  (including `delivery_worker.rs` and four test files) were modified at 21:25:49-50, after the last
  recorded test build (21:22) and the neighbours run (21:17); only `clippy-b1.log` (21:27) postdates
  them. The behaviour is now verified on the frozen digest by the reviewer's reruns —
  `logs/review-b1-focused.log` (17 passed, 0 failed, nonzero for all four plan filters plus the
  agent mapping) and `logs/review-b1-neighbours.log` (443 passed, 0 failed) and
  `logs/review-b1-fmt.log` (pass) — so nothing is unverified; the record should say so.
- **Suggested correction (records only, no artifact change):** cite the review logs above as the
  checks on digest `382bd557…`, and mark `focused-message_park_continuation_.log` as a superseded
  failing run.
- **Instruction lesson:** observation — logs were named per filter and overwritten only when
  rerun with the same filter, so a later single-test rerun left a stale failure under the filter's
  name. Candidate (skill, step 1): "Record the artifact digest in each check log header, and rerun
  or re-cite checks after any edit made after them."
- **Resolution history:** 2026-10-31 — open (optional; does not block B1).
  - 2026-10-31 (B1-R2, digest `38bf8fac…`, round [FIXAPP-P02-B1-R2](agent-review-notes.md)) — **fixed (records).** TASK.md "Checks" marks `focused-message_park_continuation_.log` as a superseded failing run and cites the review-b1 logs for digest 382bd557; post-fix logs (`fix-b1-*.log`, 21:49–21:50) postdate the only later edit (21:46:55). Closed.

---

## FIXAPP-P02-B1-F2 — (optional) Per-part renewal of the whole remainder writes O(batch²) rows per batch

- **Classification:** optional suggestion · severity Low · category efficiency (implementation of
  G1 r2 F4; the cost was not stated in the gated decision).
- **Evidence:** `BatchLeases::renew` (`src/application/services/delivery_worker.rs`, called before
  every part in `deliver`) renews every held lease each time. For a full batch of 20 single-part
  deliveries that is 20+19+…+1 = 210 row updates per batch (plus a round trip per send) where the
  claim itself wrote 20; each renewal rewrites `lock_expires_at` and `updated_at`, both indexed
  (`message_deliveries_sending_lease_idx`, `message_deliveries_company_updated_idx`,
  `init_schema.sql:7758,7786`), so the updates are not HOT. Correct, and no status trigger fires
  (all delivery triggers are `UPDATE OF status`), but with a fast provider the renewals become most
  of the worker's writes. A time threshold (renew only when the last successful renewal is older
  than, say, a quarter of `lease_for`) keeps every waiting lease at ≥ ¾ of its length and makes the
  fast path ~1 renewal per batch, while the slow-provider case behaves as now.
  The persistence guide asks for evidence before optimizing; this is recorded as a candidate, not
  a defect.
- **Instruction lesson:** none; reviewer's own G1 suggestion (G1-F4) did not state its cost.
- **Resolution history:** 2026-10-31 — open (optional; does not block B1).
  - 2026-10-31 (B1-R2, digest `38bf8fac…`, round [FIXAPP-P02-B1-R2](agent-review-notes.md)) — **declined by author, accepted.** Reasoning in TASK.md "B1 review round 1 resolution" (no production evidence yet; gated F4 behaviour). Kept as a candidate for when delivery write load is measured. Closed as deferred.

---

## FIXAPP-P02-B1-F3 — (optional) The renewal-error path (G1 r2 F5) has no test

- **Classification:** optional suggestion · severity Low · category test coverage.
- **Evidence:** `BatchLeases::renew` returns `true` on `Err` and leaves the set unchanged, so the
  worker proceeds to the live-fenced `begin_part`. `ScriptedQueue` (`delivery_worker_tests.rs`)
  can script a lost renewal and counts renewals, but cannot script a renewal error, so neither
  "an error still sends" nor "an error does not drop the lease from the set" is exercised. A
  scripted `Err` case asserting one `begin_part`, one send and an unchanged `held` would pin it.
- **Resolution history:** 2026-10-31 — open (optional; does not block B1).
  - 2026-10-31 (B1-R2, digest `38bf8fac…`, round [FIXAPP-P02-B1-R2](agent-review-notes.md)) — **fixed.** `ScriptedQueue.renewal_errors` makes `renew_delivery_leases` return `Err`; `delivery_late_completion_a_renewal_error_still_sends` asserts one renewal, one `begin_part`, one completion and the lease still in `BatchLeases::held`. Reviewer rerun: 16 passed (logs/review-b1-r2.log). Closed.
