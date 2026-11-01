# FIXAPP-P02 — agent review notes

Workflow `implement-opus-reviewed-2_2`. Task record: [TASK.md](TASK.md). Review index:
[REVIEW.md](REVIEW.md). Findings: [agent-review-findings.md](agent-review-findings.md).

## FIXAPP-P02-G1-R1 — Architecture gate G1, decision revision r1

- Date: 2026-10-31. Stage: architecture gate G1 (batch B1). Reviewer: Opus 5.5 / xhigh (Reviewer B).
- Phase p02, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p02`, branch `fix-app/p02`.
- Phase start `e6b0a51`, review base `e6b0a51`, HEAD `e6b0a510ec27a636bf76c9aa9312605ca3e5c8cb`,
  digest `9d5bde9263086a6007787848a407759f6e8a3ca7b391addfaef755df2fee4c66`
  (exclusion: `plan/fix-app/phases/p02/docs`). Confirmed at start of review (snapshot.sh: no files).
- Gated decisions: D1, D2, D3 (TASK.md "Architecture gate G1 — decision revision r1"); D4 for record.
- Coordination: `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md` revision 1; fix-app-I001 r1,
  I002 r1, I003 r1 applicable; no cross-phase notices target p02.

- Reviewed decision text: TASK.md sections "Architecture gate G1 — decision revision r1" (D1–D4), sha256 of TASK.md at review `f1760b98f7d68e2e29f2feb1712751ea0bbcd04c8e8b5586c0dcf3e1c6e29dc9`.

### Working notes (saved during review)

- D1 inspected: `message_publish.rs:295-377` (publish_on order: authorize_on → require_progression →
  approval park → conflicted → existing/stored → recheck_routes → admission probe → marker →
  action/destinations → live_window → receipt or park_on); `park_on:268-292`;
  `wait_commit.rs:74-96,121-136` (step-origin effect wake completes the step via commit_wake, no
  reopen; model_tool wake goes to continue_delivery → reopen_job); `decision_approvals.rs:56,126,261`;
  `decision_tokens.rs:35`; `workflow_tool_calls.rs:118-123`; init_schema `:166-266`
  (check_workflow_fenced_completion; literal 63 at :189,:199,:211), CHECK `:3338`,
  `workflow_continuation_counter_guard :10931` (no literal). Literal inventory matches the plan's
  eight places. Step jobs: only approval reopen can raise their count (reopen guard admits
  model_tool delivery / decision / child_tool_call), one approval per invocation → ≤1; so the
  step-origin mapping is defensive as claimed.
- D1 observation: `ActionService::prepare_tool/prepare_step` persist the immutable
  `workflow_action_intents` row before `publish_on` runs, so the plan's test wording "creates no
  intent" cannot hold literally with the plan's own placement ("top of publish_on"). Existing
  `ActionLimit` precedent also leaves the intent. TASK does not record this interpretation.
- D2 inspected: `schedules.rs:95-128` (ClaimedRow strict decode after commit), `:412-460`
  (capture_now snapshot write), `:621-713` (claim_due capture loop + claim + commit-then-decode),
  `:715-742` (claim_occurrence); `schedule_admission.rs:188-196,343-360` (only company_id,
  channel_id, id read); `schedule.rs:196-204`; init_schema `:1533-1594`
  (require_schedule_occurrence_scope), `:4063-4090` (column + CHECK). Repo-wide `git grep`:
  remaining snapshot references are tests only (admission_source_tests.rs:123,
  schedule_tests.rs:899,942,974-976,1045, and `row.schedule.id` at :1405). Single
  `ScheduleOccurrences` impl. Defer path already backs off (schedules.rs:815-830), so the poison
  test needs no new code. Capture loop still decodes the whole ChannelSchedule inside the claim tx.
- D3 inspected: queue.rs whole file (claim :63-145, renew :147-168, begin :170-247, complete
  :249-313, fail/release own fence, reaper :632-715, fences :808-829); cancellation.rs :83-215,
  :262-268 (lock_held_delivery_on = own fence FOR UPDATE), :294-400; delivery_worker.rs whole
  file; transport/queue.rs :40-56, :340-420 (port, no defaults); init_schema :3785-3797
  (`message_deliveries_lease_check` forces execution_id NULL whenever status <> 'sending'),
  :3822-3831 (parts: part_index < 50); message_receipts.rs:122 (receipt consumer locks
  deliveries `ORDER BY delivery.id FOR SHARE`); is_claimable = pending|retryable only.
  Conclusion: expiry-free own fence on completion is safe — no path re-mints or re-claims a
  `sending` row without leaving `sending` and clearing execution_id under the parent lock.
  Remainder renewal is the only reading under which the plan's "last delivery completes" test can
  pass while renewal keeps the live-lease predicate (persistence AGENTS.md). Gaps: no
  competing-claimant test planned; multi-row renewal lock order vs id-ordered lockers.
- D4 inspected: notification.rs :160-300, :510-545; constraints :3892-3911. Matches fail path.

### Coverage and evidence

- Requirements derived from `plan/fix-app/02-silent-stalls.md` (all four sections, Focused checks,
  Final gate) and `plan/fix-app/README.md` (concurrency rules, Known overlaps, Verification);
  instructions `AGENTS.md`, `src/AGENTS.md`, `src/application/AGENTS.md`,
  `src/adapters/persistence/AGENTS.md`. Plan spans (as of 4e92edd) re-checked at e6b0a51; all cited
  spans still match within a few lines.
- Method: inspection only (gate; no code changed). No tests run — none are meaningful before
  implementation; no DB used. Files read are listed in the working notes above.
- Assumptions challenged and outcome:
  - D1 "after the replay check, only for delivery plans": accepted. `MessageActions::publish`
    calls `observe` first, so `publish_on`'s `existing` branch is a race path; an all-in-app plan
    commits its receipt with no wait and never needs a continuation; approval parks already
    refuse at the limit. Literal inventory of 63 matches the plan's eight places;
    `workflow_continuation_counter_guard` has no literal. Function placement precedes
    `background_tasks` (CHECK dependency) — init_schema `:2931` vs `:3310`.
  - D1 step-origin mapping (`message.action_limit`, Terminal): accepted as defensive. A step-origin
    effect wake completes the step via `commit_wake` (no reopen, `wait_commit.rs:95,236`); a step
    job's count can only rise through one approval reopen (reopen guard admits model_tool
    delivery / decision / child_tool_call only), so a step job never reaches 63.
  - D2 "no other reader/writer": accepted. Only tests reference the column/check/trigger branch
    (list in working notes); one `ScheduleOccurrences` impl; the trigger function must change with
    the column (TASK records the overlap). Optional F6 on the capture loop.
  - D3 own fence without expiry vs reaper and re-claims: accepted as safe by inspection
    (`message_deliveries_lease_check`, claim takes only pending/retryable, reaper clears execution
    under the parent lock with SKIP LOCKED, completion locks the parent first). Not tested →
    F1.
  - D3 batch renewal as the reading of the plan: accepted; renewing only the next delivery cannot
    satisfy the plan's own "last delivery in the batch completes" test while renewal keeps the
    live-lease predicate the persistence guide requires. Failure handling: lock ordering defect
    F2; optional F4 (per-part granularity), F5 (renewal error handling).
  - D4: matches fail-path rule and backoff; optional F8 (observability).
- Merge risk (README Known overlaps): p05 edits `message_deliveries` / `message_delivery_parts`
  constraints and member-removal cancellation; D3 edits `delivery/queue.rs` (complete_part,
  renewal), the `DeliveryQueue` port in `application/transport/queue.rs` and the worker. Disjoint
  statements; textual conflict possible only if p05 also edits the `DeliveryQueue` trait or
  `cancellation.rs` locking — whichever merges second reruns both `delivery` suites. D2's
  `require_schedule_occurrence_scope` edit is not named by any sibling plan.
- Lightly reviewed: D4 (not gated) beyond the stated SQL; worker shutdown interplay with renewal
  (pre-existing `resubscribe` behaviour, not changed by D3).

### Outcome

- **Changes required** for G1 r1: actionable [FIXAPP-P02-G1-F1](agent-review-findings.md) (Medium,
  D3 competing-claimant test + guide-rule reconciliation), [FIXAPP-P02-G1-F2](agent-review-findings.md)
  (Medium, D3 renewal lock order), [FIXAPP-P02-G1-F3](agent-review-findings.md) (Low, D1 test
  interpretation of "no intent"). Optional: F4, F5, F6, F7, F8.
- D1 and D2 are sound as decisions; only F3 (record-keeping for D1's test) is required there. A
  recheck can be limited to the revised D1 test text and D3 (r2), reusing this round's D1/D2/D4
  analysis while the code is unchanged.
- Freshness at return: HEAD and digest unchanged (snapshot.sh re-run: base e6b0a51…, digest
  9d5bde92…, no files); COORDINATION.md still revision 1, no cross-phase notices (sha256
  758532a6…); TASK.md G1 still r1.

### Process and instruction feedback

- Batch/gate size: appropriate; the three gated decisions are the right ones (schema, guard,
  concurrency protocol). D4 correctly left ungated.
- Author records: D1 and D2 have no per-decision "verification plan" bullet (only the acceptance
  criteria list); D3's verification plan copied the plan's sequential tests. The gate template's
  verification item did not prompt for the root AGENTS.md competing-claimant rule (F1).
- Useful: TASK's explicit reaper confirmation and the "why the remainder" reasoning made the D3
  review fast to verify against code.
- Candidate instructions (scope, evidence): (1) skill architecture-gate step — a decision that
  removes a predicate listed in a scoped guide's "preserve" rules cites the rule and plans the
  adversarial/competing test it requires (F1); (2) `src/adapters/persistence/AGENTS.md` — blocking
  multi-parent locks are taken in primary-key order (F2; convention currently only in a code
  comment at `queue.rs:641`).

## FIXAPP-P02-G1-R2 — Architecture gate G1 recheck, decision revision r2

- Date: 2026-10-31. Stage: architecture gate G1 recheck (batch B1). Reviewer: Opus 5.5 / xhigh
  (Reviewer B, same agent as R1). Phase p02, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p02`,
  branch `fix-app/p02`, HEAD `e6b0a510ec27a636bf76c9aa9312605ca3e5c8cb`.
- Decision under review: TASK.md "G1 decision revision r2 (2026-10-31) — responses to
  FIXAPP-P02-G1-R1" amending r1 D1–D3; TASK.md sha256
  `91e6e9b6d6bb160bdf385a285d1087407b40a852854f997972ce85527c4a591a` (matched the author's
  message at start and at return). r1 decision text confirmed preserved verbatim above the r2
  section.
- Code artifact: digest `9d5bde9263086a6007787848a407759f6e8a3ca7b391addfaef755df2fee4c66` matched
  at the start of the recheck. **During the recheck the digest drifted** (`a8723079…`, then
  `79780f33…`) because the author began the ungated D4 change (`src/adapters/persistence/notification.rs`,
  `notification_tests.rs`), as TASK.md "Next action" states. No D1–D3 file changed. This gate
  verdict is pinned to the decision revision (TASK.md sha above), not to a code digest; the D4
  code is not reviewed here and belongs to the batch review.
- Coordination: `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md` still revision 1
  (sha256 `758532a6…`), fix-app-I001/I002/I003 r1, no cross-phase notices — unchanged from R1.

### Coverage and evidence

- Rechecked by finding ID against the r2 text and the code already traced in R1 (unchanged for
  D1–D3): F1 (exception recorded with the lease_check / claimable-statuses / reaper-lock
  justification; two barrier-synchronised own-database competing tests specified), F2 (CTE locks
  `ORDER BY delivery.id FOR UPDATE OF delivery`, live-lease predicate kept, no SKIP LOCKED —
  consistent with id-ordered lockers at `cancellation.rs:93-103`, `message_receipts.rs:119-123`,
  `queue.rs:641`; EPQ re-check after a lock wait excludes rows the reaper took), F3 (interpretation
  recorded), F4/F5 (per-part renewal through `BatchLeases`; error → live-fenced `begin_part`;
  unreturned/processed fences leave the set), F6 (capture loop column list), F7 (declined,
  accepted), F8 (structured warning; census counts dead letters at `notification.rs:312`).
- New-risk check of r2: per-part renewal cost is bounded (≤50 parts × ≤20 rows per statement);
  renewal still requires a live lease at statement start, so a lease that lapses in real time
  during a lock wait behind a cancellation can be extended — safe, since no other party can own
  the row while it is `sending` under this execution (F1's invariant), and identical to the
  existing single-row renewal. Dropping a future delivery from the set is safe because every
  reason a fence is not returned would also fail `begin_part`'s live fence.
- D1, D2 analysis reused from R1 (code and decision text unchanged apart from F3/F6 additions).
- Inspection only; no tests run (none exist yet for the gated decisions).

### Outcome

- **PASS for G1 decision revision r2** (D1, D2, D3; D4 recorded, not gated). F1, F2, F3 fixed in
  decision; F4, F5, F6 adopted; F7 deferred (accepted); F8 partly adopted (accepted). New optional
  [FIXAPP-P02-G1-F9](agent-review-findings.md) (deterministic lock-order regression test for
  renewal) — not blocking.
- To verify in the batch review: the two competing tests and their repeat counts, the renewal SQL
  (fence join, live predicate, id order), `BatchLeases` set maintenance, the F3 table-by-table
  assertions, and the D4 code begun during this recheck.

### Process and instruction feedback

- The digest drift during a gate recheck came from permitted independent (ungated) work. The skill
  says to freeze the worktree during review but also lets the author continue independent work
  during a gate; for gates the pinned identity is the decision text. Candidate (skill, gate step):
  "An architecture-gate request pins the decision revision by TASK.md digest; independent ungated
  edits may continue and are named in the request so the reviewer can tell expected drift from an
  unexpected change." Evidence: this round.
- Corrections arrived in one round covering every actionable finding, as the skill asks.

## FIXAPP-P02-B1-R1 — Batch review B1 (whole phase, plan sections 1-4)

- Date: 2026-10-31. Stage: batch review. Reviewer: Opus 5.5 / xhigh (Reviewer B). Phase p02,
  worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p02`, branch `fix-app/p02`.
- Phase start `e6b0a51`, review base `e6b0a51`, HEAD `e6b0a510ec27a636bf76c9aa9312605ca3e5c8cb`
  (nothing committed); batch = working-tree delta (23 modified files + untracked
  `delivery/late_completion_tests.rs`, `workflow/message_park_continuation_tests.rs`). Digest
  `382bd557ff5dbf75c15e46d18f8cb438de0d52162df1af08f569c8ec23bf2614` (exclusion
  `plan/fix-app/phases/p02/docs`) confirmed at start.
- Gated decisions: G1 r2 (PASS FIXAPP-P02-G1-R2); F9 adopted per author.
- Coordination: COORDINATION.md **revision 2** — fix-app-I001 r1, I002 r1 (adds CARGO_INCREMENTAL=0),
  I003 r1, **I004 r1** (user instruction: no full suite / check.sh; targeted tests with nonzero
  counts after reset-db when schema changed; fmt + compile of affected targets; supersedes the
  plan/README final gate). No cross-phase notices.

(working notes follow)
- Inspected so far: init_schema diff (function, 4 guard sites incl. effect_parked, CHECK, schedule_runs
  column/CHECK, trigger snapshot removal) — matches G1 r2. D1 code (`continuations_exhausted_on`,
  placement after route recheck / before admission probe, only `has_delivery`, new variants, agent
  TOOL_LIMIT mapping + unit test, step mapping), constant in workflow/mod.rs, 4 SQL strings. D1
  tests file read in full (continued() drives real park/continue cycles; F3 assertions via
  `rows()` = markers/actions/destinations/deliveries/receipts/waits/messages + state; SQL-only
  park clone with 62-commit control and 63-refused 23514). D2 code + tests (own_database Fixture,
  so page totals are legal). D3 queue renewal SQL (id-ordered CTE FOR UPDATE, live predicate),
  `own_fence_sql`/`live_fence_sql` split, complete_part on own fence; worker `BatchLeases`
  (renew before each part, drop unreturned, error → proceed, finish after deliver). No defect yet.
- Tests read in full: late_completion_tests.rs (7 incl. worker), delivery_worker_tests diff
  (lost-renewal + per-part renewal count), notification_tests diff (3 stale tests, own_database),
  schedule_tests diff. Lock-order test would fail on wrong ordering via the renewal's deadlock
  error at the final unwrap (renewal waits first, so its deadlock check fires first).
- Evidence audit: author's focused-message_park_continuation_.log records a FAILED run (bind
  error in the SQL-only park test); the pass is in focused-guard.log (single test) and
  neighbours-b1.log (21:17). Ten files (incl. delivery_worker.rs and four test files) were
  modified at 21:25:49-50, after every recorded test run (latest test build 21:22) but before
  clippy (21:27). So the recorded test evidence predates the artifact; reviewer reran focused
  filters on the current artifact (logs/review-b1-focused.log) and fmt --check (pass,
  logs/review-b1-fmt.log).
- Triggers on message_deliveries fire only on `UPDATE OF status`; renewal does not touch status.

### Coverage and evidence

- Requirements: plan 02 §1-§4 tests and changes, Focused checks; README concurrency rules and
  Known overlaps; fix-app-I004 r1 as the verification scope (supersedes plan Final gate);
  AGENTS.md (competing-claimant rule), `src/AGENTS.md`, `src/application/AGENTS.md` (poison batch,
  terminal states), `src/adapters/persistence/AGENTS.md` (fences, guards, own_database, §2 payload
  rule). G1 r2 decisions D1-D3 and D4 checked against the implementation — all implemented as
  decided (function + 4 guard sites + CHECK + 4 SQL strings + Rust constant with DB equality test;
  refusal placement/scope; TOOL_LIMIT mapping; step mapping; snapshot column/CHECK/trigger branch
  removed, claim from columns, capture loop columns only; expiry-free completion fence, live
  fence kept for begin_part, id-ordered renewal CTE, BatchLeases per part with error → proceed;
  stale release dead-letter + backoff + warning).
- Whole diff read (23 files) plus both new test files in full. Callers checked: every
  `MessagePublication`/`MessageObservation` match updated; single `DeliveryQueue` production impl +
  `ScriptedQueue`; single `ScheduleOccurrences` impl; no remaining `schedule_snapshot` /
  `renew_delivery_lease` references.
- Checks actually run by the reviewer on digest 382bd557 (DATABASE_URL 127.0.0.1:55402,
  SQLX_OFFLINE=true, CARGO_INCREMENTAL=0; schema as reset by the author, init schema unchanged
  since):
  - `cargo test --locked --offline --lib -- message_park_continuation_ delivery_late_completion_
    notification_stale_ schedule_claim_` → 17 passed, 0 failed (logs/review-b1-focused.log);
    each plan filter nonzero (5/7/3/2).
  - neighbours: author's filter set plus `message_tests` (all message publication paths) →
    443 passed, 0 failed (logs/review-b1-neighbours.log).
  - `cargo fmt --all -- --check` → pass (logs/review-b1-fmt.log).
  - Relied on author's `clippy --lib --tests -D warnings` (21:27, after the last source edit).
- Not run (per I004): full suite, scripts/check.sh, stack budget. `.sqlx/`: no macro query
  touches the changed tables (author's grep; consistent with the runtime-query files changed).
- Merge risk for the manager: p05 also edits delivery code; p02 changes the `DeliveryQueue` port
  (`renew_delivery_leases`), `delivery/queue.rs` (complete_part fence, renewal) and the worker. Any
  p05 `DeliveryQueue` impl/mock or `queue.rs` edit conflicts textually; rerun both plans'
  `delivery` filters after the second merge. init_schema statements touched are listed in TASK.md.

### Outcome

- **PASS for batch B1 at digest `382bd557ff5dbf75c15e46d18f8cb438de0d52162df1af08f569c8ec23bf2614`.**
  No acceptance-blocking defect. Optional: [FIXAPP-P02-B1-F1](agent-review-findings.md) (records:
  cite the reviewer's reruns; stale failing log), [FIXAPP-P02-B1-F2](agent-review-findings.md)
  (renewal write amplification; time-threshold throttle), [FIXAPP-P02-B1-F3](agent-review-findings.md)
  (no test for the renewal-error path).
- Lightly reviewed: the SQL-only park clone's column-copy statements (validated by its passing
  62-commit control rather than line by line); D4 beyond the changed function.
- Freshness at return: HEAD/digest unchanged; COORDINATION.md revision 2 (I001-I004 r1), no
  notices.

### Process and instruction feedback

- Batch size was reviewable in one round (23 files, ~650 lines); the G1 gate front-loaded the
  risky decisions, so the batch review found no design defects.
- Evidence gap: test logs predated late edits, and a filter-named log kept a superseded failure
  (B1-F1). Candidate in B1-F1.
- Useful: the 62-commit control in the hand-written park test isolates the new guard predicate;
  the id-order test is deterministic.

## FIXAPP-P02-B1-R2 — Batch B1 correction recheck

- Date: 2026-10-31. Stage: correction recheck of B1 (by finding ID). Reviewer: Opus 5.5 / xhigh
  (Reviewer B). Phase p02, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p02`, HEAD
  `e6b0a510ec27a636bf76c9aa9312605ca3e5c8cb` + working tree, digest
  `38bf8facc727de388b98561499c803b9324db75f92c54332294699465c2b5bbb` (exclusion
  `plan/fix-app/phases/p02/docs`), confirmed at start and at return. Coordination revision 2
  (I001–I004 r1), no notices.
- Delta since 382bd557: only `src/application/services/delivery_worker_tests.rs` (mtime 21:46:55;
  every other artifact file unchanged since 21:25:50, as at B1-R1). Change: `ScriptedQueue`
  `renewal_errors` + test `delivery_late_completion_a_renewal_error_still_sends`. Test-only, so
  B1-R1's review and reruns of all production code and other tests are reused unchanged.
- Checks run by reviewer: `cargo test --locked --offline --lib -- services::delivery_worker
  delivery_late_completion_` → 16 passed, 0 failed (no recompile needed; binary from the author's
  21:49 build of the current source); `cargo fmt --all -- --check` → pass
  (logs/review-b1-r2.log). Relied on author's post-change clippy (logs/fix-b1-clippy.log, 21:50).
- Dispositions: B1-F1 fixed (records), B1-F2 declined by author and accepted (deferred
  candidate), B1-F3 fixed. See [agent-review-findings.md](agent-review-findings.md).
- **Outcome: PASS for batch B1 at digest `38bf8facc727de388b98561499c803b9324db75f92c54332294699465c2b5bbb`.**
- Process note: narrow, single-file correction with fresh logs; no instruction change suggested.
