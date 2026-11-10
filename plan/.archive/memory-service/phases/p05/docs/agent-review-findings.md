# p05 agent review findings

Common context for the MEM-P05-B2-F* entries below unless stated otherwise:
2026-11-08; workflow `implement-opus-reviewed-2_2`; task
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p05/docs/TASK.md; review record
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p05/docs/REVIEW.md; batch p05-B2; stage
batch review; round note MEM-P05-B2-R1 (agent-review-notes.md); reviewer claude-opus-5-5, effort xhigh; phase
`p05`; worktree /Users/mac03/.codex/worktrees/60a0/openbot (detached HEAD a6bed48a); review base 113801e4;
artifact digest d20858a7e356ccd1cce9dbad6bff214a2bdf37dd60dd3ad00c7e8fab10c2f314 (non-record tree = acbda41e).
Severity scale: Major (acceptance-blocking), Minor (actionable, should be fixed before acceptance unless an
explicit disposition is agreed), Optional (suggestion; does not block).

## MEM-P05-B2-F1 — the terminal-intent wake path (disable/expiry/refusal → step failure) has no test

- Classification: actionable defect; Minor; verification (behavior confirmed working by a reviewer probe).
- Evidence: E2 requires disable to fail pending intents "(their waits wake with a terminal failure)" and E9 says
  its race test "asserts that nothing is delivered and that every wait resolves". The production branch is
  `wait_commit::resume_waiting` → `memory_steps::effect_resume_on` → `MemoryResume::Failed` →
  `wait_commit::memory_effect_failed` (wait_commit.rs:237-254), discovered by the new memory arm in
  `polling.rs` CANDIDATES_SQL (:147-150). No committed test reaches `MemoryResume::Failed` or the poll arm (grep:
  only `memory_tests.rs` maps outcome strings to codes in pure code); the round trip covers only `Delivered`
  via a direct `resume_wait`; `memory_integration_disable_races_save_and_capture_then_reenable` freezes intents
  with no wait at all; TASK's "Unproved by B2b … the wait wake on a terminal intent lands with the effect wait in
  B2c" was never closed.
- Reviewer probe (isolated copy, own CARGO_TARGET_DIR, deleted afterwards; source and output retained at
  logs/MEM-P05-B2-R1-B-probe-terminal-wake.rs.txt and logs/MEM-P05-B2-R1-B-probe-terminal-wake.txt): for
  disabled, expired and service-refused (403 → `failed/authority`) intents the poller lists the run as `Wait`
  only after the intent is terminal, `resume_wait` completes with `FinalError`, the run fails, the wait completes
  with audit `memory_delivery_failed`, and a control retry is `Unsafe`. So the behavior holds on this artifact;
  the defect is the missing regression test.
- Observation: the specific step code (`memory.not_enabled`, `memory.save_expired`, `memory.save_cancelled`,
  `memory.save_failed`) only selects the final-error route; the run records the generic `memory_delivery_failed`
  audit (same as the message precedent); the intent row keeps the outcome.
- Required: a committed test parking a real memory.save and, for at least a disable and an expiry, asserting the
  poll candidate, the step failure/route and the settled wait (the probe is a usable model).
- Instruction lesson: Violated: verification-contracts (an obligation recorded as "Unproved" was carried into the next sub-batch and dropped). Candidate: TASK's "Unproved by" lines get an owning batch and are re-checked before freeze.
- Resolution history: 2026-11-08 open.
  - 2026-11-09 recheck MEM-P05-B2-R2 (digest a369b44e…fc75, tree 8e4baa51): FIXED. `memory_integration_terminal_intent_wakes_and_fails_the_parked_save` (memory_steps_tests.rs) parks a real save and, for disabled, expired and refused intents, asserts no poll candidate while open, exactly `[Wait]` once terminal, a `FinalError` completion, run failed, wait completed and the `memory_delivery_failed` audit — the probe's assertions, committed. Passed in B's full lib run (logs/MEM-P05-B2-R2-B-lib-full.txt).

## MEM-P05-B2-F2 — memory.save not made marker-owned (E4); an interrupted save attempt becomes Reconcile

- Classification: actionable defect; Minor; architecture/correctness (undeclared deviation from gated E4).
- Evidence: E4 (implementation-decisions.md, "E4 `memory.save` joins `marker_owned_effect_kind`
  (`actions/failure.rs:26-38`): its only effect path is the committed intent (E8)"). The batch leaves
  `src/application/workflow/actions/failure.rs:26-37` unchanged and its test
  `workflow_action_marker_owned_kinds_are_exactly_the_action_service_owners` (failure.rs:292-309) still asserts
  `!marker_owned_effect_kind("memory.save")`; `workflow_effect_free_kinds_are_exactly_the_reads_pure_waits_and_model_steps`
  keeps memory.save in its `unknown` set. So `retry_safe_without_marker("memory.save")` is false and
  `recovery.rs:184` labels any lease expiry / interrupt / crash of a claimed memory.save attempt
  `RetrySafety::EffectOutcomeUnknown`, which `RecoveryPolicy` always turns into `RecoveryDecision::Reconcile`
  (domain test `workflow_recovery_unknown_effect_never_becomes_retry_or_final_error`). Nothing reconciles a
  memory.save: the run waits in `reconciliation` until its deadline. Yet the save's only effect path is the intent
  committed atomically with the park (memory_steps.rs `park_on`), so an attempt that died before that commit sent
  nothing and is provably safe to retry — exactly the property E4 relied on. Neither TASK.md nor the P7c notes
  declare this deviation.
- Impact: a process restart or lease loss while a memory.save job is claimed (claim → park window) strands the
  run instead of retrying it.
- Required: implement E4 (add memory.save to the marker-owned set or an equivalent "intent-owned" predicate,
  update the invariant comment and both tests), or record an explicit agreed deviation with its recovery story;
  add a test that an expired memory.save lease without an intent retries (and one with an intent is already parked).
- Instruction lesson: gated decisions that name an existing predicate to extend should be listed in the
  author's contract-impact table; cause otherwise unknown.
- Resolution history: 2026-11-08 open.
  - 2026-11-09 recheck MEM-P05-B2-R2 (digest a369b44e…fc75, tree 8e4baa51): FIXED. `memory.save` is in `marker_owned_effect_kind` (failure.rs; R1→R2 diff holds only these lines and the two updated classification tests). `memory_integration_interrupted_save_without_an_intent_retries` expires a claimed save's lease before any intent and asserts retirement `safe`, run running, job pending, then one parked intent on retry; mutation D (patch retained, applies to the current tree) turns it into `unknown/waiting/failed`. After a committed park the job is completed, so no retirement can reach it; the SQL predicate returns NULL (no markers) and does not veto. Doc wording of the invariant: optional F15.

## MEM-P05-B2-F3 — `context.history` exactness test cannot distinguish exact from inexact agent messages

- Classification: actionable defect; Minor; verification.
- Evidence: `memory_access_level_history_read_lowers_for_conversational_or_inexact_messages`
  (history_reader_tests.rs:291-338) admits every case through `claimed(&f, &c, …)` with the Conversation fixture,
  whose trigger message is role `human` (admission_history_tests.rs:49). The existing test
  `workflow_context_history_returns_the_admission_snapshot_with_boundaries` (history_reader_tests.rs:114-160,
  assertion at :154-158, "triggering message included") shows the snapshot includes the trigger, so
  `read.contains(&c.message)` is true and `expected = case == "agent_exact" && !read.contains(&c.message)` is
  false in all three cases. The test therefore only proves "a human row lowers"; it never asserts that an exact
  agent message keeps the run exact, and a mutation that ignores the stored agent bit
  (`stored_message_exactness`'s `("agent"|"system", Some(exact))` arm) would still pass. TASK.md maps it to
  "human or inexact agent message lowers" and E11 lists "a run reading an inexact run's message through
  `context.history` … saves at the fallback".
- Required: a discriminating case whose snapshot holds no human row (e.g. an agent/system trigger, or a run
  whose snapshot excludes the trigger) asserting exact stays exact and an inexact agent row lowers.
- Instruction lesson: verification-contracts.md already requires discriminating cases; the author's mapping
  treated a hedged `expected` expression as proof. Candidate: "an assertion whose expected value is computed from
  the fixture must be shown to take both values across the cases".
- Resolution history: 2026-11-08 open.
  - 2026-11-09 recheck MEM-P05-B2-R2 (digest a369b44e…fc75, tree 8e4baa51): FIXED. The test now admits on an exact agent message in its own thread (snapshot = exactly the earlier row + trigger, asserted), expects agent_exact → exact and agent/system-inexact and human → inexact, and asserts the expectation took both values; mutation C (stored bit ignored) fails it.

## MEM-P05-B2-F4 — E6 race and mutation evidence use undeclared stand-ins

- Classification: actionable defect; Minor; verification.
- Evidence: `memory_access_level_competing_recall_and_save_freeze_preserve_the_watermark`
  (memory_steps_tests.rs:361-419) races a bare transaction calling `knowledge::raise_content_level_on` (not
  `memory_steps::observe_on`/`observe_memory_load`, nor `knowledge_checkpoint::observe_on`) against
  `deliveries::enqueue_save_on` in a bare transaction (not `park_on`, which takes `runtime::facts`
  `FOR UPDATE OF run` and `lease::lock_fence` first). It proves the shared freeze function's row lock, not the
  production observation/park pair. The mutation record (logs/MEM-P05-B2-R1-a8-mutation-e6-ab.txt) keeps only the
  failing output, not the applied patches; mutation (A) as described in 05's record is "the freeze reads the run's
  levels without the run lock", whereas E6 defines (A) as "freezing from the claim-time `RunKnowledge`" — the
  latter would be the `freeze_save_on(db, intent, run)` entry with caller-supplied `LockedRun`, which the race
  test never calls. TASK.md does not name these stand-ins.
- Required: state the stand-ins and what they leave unproved in TASK's mapping, retain the mutation patches as
  evidence, and either race the production entry points (observe_memory_load vs park_memory_save on two
  connections) or justify why the shared-function race is sufficient; B3's tool-level race must exercise E6's
  mutation (A) on the path that passes a `LockedRun`.
- Instruction lesson: Candidate (verification-contracts): retain each mutation's patch beside its failing output, and name stand-ins in the mapping row they support. Otherwise cause unknown.
- Resolution history: 2026-11-08 open.
  - 2026-11-09 recheck MEM-P05-B2-R2 (digest a369b44e…fc75, tree 8e4baa51): FIXED. New `memory_access_level_competing_rise_and_parked_save_freeze_the_committed_level` (memory_park_race_tests.rs) drives the production park (`MemoryStepRuntime::execute` → `park_on`) against an in-flight rise in both orders, exact and inexact, on an agent scope so the level is discriminated; mutation A as E6 defines it (claim-time `RunKnowledge` passed as `LockedRun`) fails it, B still fails the shared-function race; stand-ins (bare-transaction rise; tenant-row holder pausing the park) are named in TASK with the argument that a run has one claimed execution, which I accept. Patches A–F retained and apply cleanly to the current tree.

## MEM-P05-B2-F5 — missing step-level 05A/E3 cases and competing tests for new settlement protocols

- Classification: actionable defect; Minor; verification (requirements coverage).
- Evidence: 05A focused checks require `memory_access_level_` cases for participants 6/8/9, read levels 0–6, an
  audience drop to 3 and a missing trusted ceiling, and AGENTS.md requires competing-claimant tests for new
  concurrency protocols. In this batch `memory_steps::read_ceiling_on` (memory_steps.rs:132-151) implements
  `min(run ceiling, destination, 9)` and the "destination below content" refusal for `memory.load`, but no test
  runs memory.load in a thread run (readers 6/8/9 → ceiling 6, items above 6 hidden) or lowers the destination
  below the content level (refusal before any service call); the only load test is a manual run at actor level 9.
  Owner removal's protocol (B2b-5: the namespace UPDATE "waits out a concurrent freeze's FOR SHARE") is tested
  only sequentially (`memory_integration_disable_fails_and_owner_removal_cancels_open_intents`), and no test
  shows disable or run cancellation against an in-flight *claimed* delivery whose completion must then be `Lost`.
  The author-declared gap "workflow-tool child path has no dedicated test" is also an E3 obligation ("a result
  through each `InvocationResult` path"): `workflow_tool_calls::call_result`'s absorb (workflow_tool_calls.rs:
  263-290) is exercised by no exactness assertion.
- Required: add these cases (or record an explicit deferral to p05-B3/B4 in TASK with the owning batch); the
  competing owner-removal-vs-freeze case in both orders through the production lock functions; an inexact
  workflow-tool child lowering its calling agent run.
- Instruction lesson: Violated: AGENTS.md competing-claimant rule; 05A focused-check list not mapped item by item. Candidate: map each listed focused-check case to a test or an explicit deferral.
- Resolution history: 2026-11-08 open.
  - 2026-11-09 recheck MEM-P05-B2-R2 (digest a369b44e…fc75, tree 8e4baa51): FIXED. Added and read: thread-run load at readers 9/8/6 (ceiling 6, reads exactly 0–6 of 0–9), a reader at 3 joining (reads 0–3), content 4 + drop to 3 refusing with `KNOWLEDGE_AUDIENCE_BELOW_CONTENT` and no `observations/*` call; missing trusted ceiling (frozen body without `producing_ceiling` → `failed/invalid_intent`, nothing sent; production cannot lack it, `knowledge_ceiling` is NOT NULL); owner removal vs freeze in both orders with a real lock wait; disable and run cancel during an in-flight claimed delivery (completion `Lost`, no receipt); workflow-tool child exactness through `call_result` (exact keeps, inexact lowers, replay keeps). All passed in B's full run.

## MEM-P05-B2-F6 — an unopenable credential envelope makes one intent poison the whole delivery queue

- Classification: actionable defect; Minor; correctness/operations.
- Evidence: `claim_delivery` (deliveries.rs:431, `let Some((credential, request)) = self.open_request(&row)? else`)
  propagates an `open_envelope`/`credential_cipher` error out of the claim transaction, which rolls back. The row
  stays `pending` and due, and the claim orders by `(available_at, id)`, so every later pass (after the worker's
  5 s error backoff, delivery.rs `run`) claims the same row first and fails again. The worker is single-flight, so
  every company's deliveries stop. A malformed body is settled (`invalid_intent`); a cipher failure is not.
  src/application/AGENTS.md: "a fixed set of poison records cannot monopolize a batch and hot-spin the poll loop".
- Required: settle (e.g. `failed(credential_unavailable)`) or postpone with backoff inside the claim transaction,
  with a poison test.
- Instruction lesson: Violated: src/application/AGENTS.md poison-record rule (only the decode failure was considered). Cause otherwise unknown.
- Resolution history: 2026-11-08 open.
  - 2026-11-09 recheck MEM-P05-B2-R2 (digest a369b44e…fc75, tree 8e4baa51): FIXED. An envelope that cannot be opened settles `failed/credential_unavailable` inside the claim transaction (logged); `memory_integration_delivery_settles_an_unopenable_credential_poison_batch` shows the healthy intent behind it delivered and an empty queue after.

## MEM-P05-B2-F7 — delivery worker is single-flight FIFO; the Delivery permits and tenant fairness are inert

- Classification: actionable defect; Minor; architecture/operations.
- Evidence: `MemoryDeliveryWorker::pass` claims one intent and awaits its HTTP call before the next claim
  (delivery.rs); one worker per process. `MemoryPool::Delivery` (limits.rs) adds a global semaphore sized like
  background and a per-company share of 1, but with one request in flight per process they can never be
  contended. Claim order is global `available_at`, so one company's burst of saves delays every other company's
  parked `memory.save` steps by burst × request latency, and each outage pass waits up to the request timeout.
  src/application/AGENTS.md "Bound execution and distribute capacity": "explicit global and per-tenant concurrency
  or fairness … one tenant's burst must not starve every other tenant". E8 said "on the background pool" with the
  E1 per-company share, which presumes concurrent work. Not declared in TASK.
- Required: bounded concurrent dispatch governed by the Delivery permits (or per-company round-robin claims),
  with a mixed-tenant fairness test; or an explicit agreed disposition with the throughput bound documented.
- Instruction lesson: Violated: src/application/AGENTS.md per-tenant concurrency/fairness. Candidate: worker designs name their concurrency and fairness bound in TASK. Cause otherwise unknown.
- Resolution history: 2026-11-08 open.
  - 2026-11-09 recheck MEM-P05-B2-R2 (digest a369b44e…fc75, tree 8e4baa51): FIXED, with a new consequence (F14). Lanes = `MEMORY_SERVICE_BACKGROUND_IN_FLIGHT`; the claim skips a company with a live claimed intent (index `memory_deliveries_in_flight`); concurrent lanes can still both claim one company (documented; the per-company permit then serializes sends and the loser retries without an attempt). `…claims_are_fair_across_companies` (mutation E fails it) and `…lanes_send_companies_concurrently` passed under full-suite load. The lanes' extra pool connections are not reserved → MEM-P05-B2-F14.

## MEM-P05-B2-F8 — settled intents keep the frozen memory text forever; unbounded table scans

- Classification: actionable defect; Minor; requirements/operations.
- Evidence: `memory_deliveries.request` holds the full frozen body (facts, up to 3.5 MiB) and is never cleared
  when the intent settles; there is no retention, and with no run FK (B2b-1) the 90-day run retention
  (`storage_retention.rs:48-51`) does not remove intents either. 05A.6: "Only service connection/configuration
  and delivery intents belong in OpenBot's database; the memory corpus stays in the service database" — a
  delivered intent is no longer a delivery intent but a copy of the memory text, and service-side correction or
  deletion (06A) cannot reach it. The claim predicate `(pending ∧ due) ∨ (claimed ∧ lease expired)` is not served
  by the partial `memory_deliveries_due` index, and the run-cancel trigger filters by `run_id` without an index,
  so both scan a table that only grows. None of this is recorded among the carried 06A obligations (TASK.md
  "Carried obligations" lists lifecycle-row retention only).
- Required: clear the request body (keep key/receipt/outcome) at terminal settlement, or record an explicit 06A
  obligation covering body retention, row retention, the claim index and the run-cancel lookup.
- Instruction lesson: Missing guidance: no rule says a settled outbound intent must drop its payload. Candidate (persistence AGENTS, remote resources): terminal intents keep identity/receipt only, or the retention owner is recorded.
- Resolution history: 2026-11-08 open.
  - 2026-11-09 recheck MEM-P05-B2-R2 (digest a369b44e…fc75, tree 8e4baa51): FIXED. `request` nullable with CHECK open ⇔ body present; every terminal writer (settle_open_on, settle_claimed_on, complete_delivery CASE, run-cancel trigger) clears it — grep finds no other production writer. Indexes `memory_deliveries_leased`, `_in_flight`, `_open_by_run` serve the claim arms, the busy check and the trigger. Row retention recorded as a 06A obligation in the decision record (R2 block).

## MEM-P05-B2-F9 — the editor catalogue never lists memory steps even when they are handled

- Classification: actionable defect; Minor; requirements/consistency (relates to the author-declared E10 gap).
- Evidence: `WorkflowServices::step_catalogue` (src/infra/workflow/services.rs:91-105) is a binary-static
  `LazyLock` built with `UNHANDLED_STEP_KINDS`, documented as "every kind this deployment runs", while
  `refused_kinds()` (services.rs:127-135) now returns `&[]` when memory is configured. With the service configured
  and memory enabled, memory.load/save publish and run but the editor's step catalogue omits them; the UI test
  (`authoring/tests.rs:442-446`, `!html.contains("memory.load")`) runs only unconfigured. E10 required these
  authoring refusal tests to split by gating; TASK declares the split as a gap but not the catalogue behavior.
- Required: derive the catalogue from the deployment's handled kinds (with the memory.not_enabled note), and add
  the configured/unconfigured split of `the_catalogue_covers_every_runnable_type_with_a_compiling_example` and the
  UI catalogue test.
- Instruction lesson: Cause unknown (static catalogue predates configurable kinds); E10's split obligation would have exposed it.
- Resolution history: 2026-11-08 open.
  - 2026-11-09 recheck MEM-P05-B2-R2 (digest a369b44e…fc75, tree 8e4baa51): FIXED. `step_catalogue` returns one of two static catalogues by `refused_kinds()`; `the_catalogue_covers_every_runnable_type_with_a_compiling_example` checks both configurations and `the_catalogue_lists_memory_steps_only_when_the_memory_service_is_configured` the configured UI page (with the memory.not_enabled note); the unconfigured UI test still asserts absence.

## MEM-P05-B2-F10 — the channel-reply exactness assertion does not isolate the B2a-5 lowering

- Classification: optional suggestion; verification.
- Evidence: `channel_reply_authoring_native_real_review_closes_reject_and_publishes_only_edited_approval`
  submits through the reviewed template's `review` gate, a `decision.human` step
  (templates/agent_workflow.rs:400-413), with feedback "Reviewed privately" and data — so hook 1 in
  `submit_business_on` already lowers the run in both choices (exactness.rs `decision_carries_free_text`). The
  `reviewed_output_exact` lowering in `channel_reply_resume::complete_saved_on` is never the only lowering, so the
  test (cited by TASK for "channel_reply_resume (reviewer edit lowers)") does not prove it.
- Suggestion: a case where only complete_saved_on can lower (or record it as defense in depth).
- Instruction lesson: Hypothesis: the cited test was chosen by name, not by checking which hook satisfies its assertion.
- Resolution history: 2026-11-08 open (optional).
  - 2026-11-09 recheck MEM-P05-B2-R2 (digest a369b44e…fc75, tree 8e4baa51): DEFERRED by the author (recorded as defence in depth; isolation left to 06C). Accepted: optional.

## MEM-P05-B2-F11 — raise to max(exposed_level, item levels) as defense in depth

- Classification: optional suggestion; security hardening.
- Evidence: `memory_steps::observe_on` raises the run to the service's `exposed_level` and checks items only
  against the ceiling used; the adapter (memory_service/mod.rs fetch) checks `exposed_level ≤ ceiling` but not
  `exposed_level ≥ max(item.access_level)`. A service answer under-reporting `exposed_level` would under-account
  content that the output then carries with its own `access_level`. E6 specifies raising to `exposed_level`, so
  this is per design.
- Suggestion: raise to the max of both (one line) or refuse such an answer as `Protocol`.
- Instruction lesson: No instruction change supported.
- Resolution history: 2026-11-08 open (optional).
  - 2026-11-09 recheck MEM-P05-B2-R2 (digest a369b44e…fc75, tree 8e4baa51): FIXED (taken). The run is raised to max(`exposed_level`, item levels) and that value is recorded in the consumption row. Residual nit: the step output's `exposed_level` field still echoes the service value — harmless for accounting.
  - 2026-11-09 recheck MEM-P05-B2-R3 (digest 9c7929bf…1abc, tree dddff33c): Nit FIXED (R3). `carried_level(exposed, items)` (application/workflow/memory.rs) feeds the raise, the consumption row and the step output's `exposed_level`; an item without a valid level refuses `memory.refused` before any observation. Unit test `memory_access_level_load_carries_the_highest_of_the_label_and_its_items` and the round trip with a client reporting `exposed_level` 0 (output, run and consumption carry 9).

## MEM-P05-B2-F12 — consumption item refs store `content_sha256`, not E6's `sources_sha256`

- Classification: optional suggestion; design note for p05-B3/B4.
- Evidence: E6 specifies `workflow_memory_consumptions` item refs `{id, revision, sources_sha256}` "for E7";
  `memory_steps::observe_on` stores `{"id","revision","sha256": content_sha256}`. The service's
  `ContextValidateRequest.items[]` (`PackedRevision`) requires `sources_sha256`, which `ObservationDetail` (fetch)
  does not return. Harmless for steps (not revalidated), but the tool consumption rows B3 adds feed E7's
  validation.
- Suggestion: settle the ref shape and where `sources_sha256` comes from before B3.
- Instruction lesson: No instruction change supported; design note for B3/B4.
- Resolution history: 2026-11-08 open (optional).
  - 2026-11-09 recheck MEM-P05-B2-R2 (digest a369b44e…fc75, tree 8e4baa51): FIXED (taken as recorded). Ref key renamed `content_sha256`; the decision record states B3/E7 takes `sources_sha256` from the `context.pack` PackItem.

## MEM-P05-B2-F13 — the competing-claimant delivery test assumes which claimant wins (flaky under load)

- Classification: actionable defect; Minor; verification (flaky test; reproduced).
- Evidence: `memory_integration_delivery_fences_competing_claimants_and_bounds_attempts`
  (delivery_tests.rs:307-378) runs two claims with `tokio::join!` and correctly asserts distinct rows, but then
  reads `row(&f, "ob.s.lease-a")` (delivery_tests.rs:356 and the exhausted check after it) as if claimant `a`
  had claimed `ob.s.lease-a`, while it mutates the row by `a.id`. Which claimant gets which row is not
  deterministic. In the reviewer's full lib run (logs/MEM-P05-B2-R1-B-lib-full.txt) it failed at :356 (`left: 0,
  right: 1`); rerun alone it passed (logs/MEM-P05-B2-R1-B-failures-isolated.txt). Production fencing was not at
  fault: the preceding distinct-claim, reclaim-fence and `Lost` assertions held.
- Impact: CI flake in the database-backed suite AGENTS.md requires.
- Required: look the rows up by `a.id`/`b.id` (or by the key each claim returned), then rerun under load.
- Instruction lesson: Candidate (verification-contracts): competing-claimant tests must not assume which claimant wins; assert on the claims' own identities. Evidence: failure under full-suite load only.
- Resolution history: 2026-11-08 open.
  - 2026-11-09 recheck MEM-P05-B2-R2 (digest a369b44e…fc75, tree 8e4baa51): FIXED. Rows are looked up by `a.id`, and the competing intent is in another company (the F7 exclusion would otherwise leave one intent). Passed under B's full-suite load (it failed there in R1).

## MEM-P05-B2-F14 — delivery lanes take more pool connections than the reservation and docs state

- Context: found in round MEM-P05-B2-R2 (correction recheck), artifact digest
  a369b44e6526a7e542e81d78108877c0874d3308185c17d9e3ffb62cff2cfc75 (tree 8e4baa51); introduced by the F7
  correction.
- Classification: actionable defect; Minor; operations/documentation (AGENTS.md raised-limit and docs rules).
- Evidence: `MemoryDeliveryWorker::run` now runs `lanes` concurrent loops (`join_all`,
  src/application/memory/delivery.rs), with `lanes = config.background_in_flight` (src/infra/setup/mail.rs;
  `MEMORY_SERVICE_BACKGROUND_IN_FLIGHT`, default 4, max 16). Each lane's `claim_delivery` opens a pool
  transaction and `complete_delivery` runs a pool query, so up to `lanes` connections can be held at once (idle
  lanes poll every second independently). `RESERVED_DATABASE_CONNECTIONS` (src/infra/workflow.rs, 14) still
  reserves one connection for the delivery worker, `worker_slots` checks only `14 + 2 × slots`, and
  docs/deploy.md:765 (unchanged in R2) still says "The delivery worker holds one database connection". At the
  defaults (pool 20, 2 slots, 4 lanes) peak demand is 21 > 20; at 16 lanes the shortfall is 15, which a
  workflow slot's lease renewal can wait behind.
- Required: either bound the lanes' database use to the reserved connection (e.g. one claim/complete at a time
  behind a semaphore, sends still concurrent), or reserve `background_in_flight` connections for the worker in
  `worker_slots` (refusing at startup, with its test) and correct both docs.
- Instruction lesson: AGENTS.md "a raised limit must leave something that fails when it is approached" — the
  concurrency raise was not traced to the connection budget. Candidate: when a worker's concurrency becomes
  configurable, its connection reservation must be derived from the same setting.
- Resolution history: 2026-11-09 open.
  - 2026-11-09 recheck MEM-P05-B2-R3 (digest 9c7929bf…1abc, tree dddff33c): FIXED. `MemoryDeliveryWorker` serializes every store call (claim and completion) behind one `tokio::sync::Mutex`; each call takes and returns its pool connection inside the guard, and the sends overlap outside it, so the worker holds at most one connection at any lane count — matching `RESERVED_DATABASE_CONNECTIONS` (unchanged 14) and the corrected docs/deploy.md:765 and infra/workflow.rs comment. The lanes test now wraps the store in a counter and asserts peak concurrent store calls = 1 with 2 lanes and both sends in flight; mutation G (guard removed; patch retained) fails it with peak 2. Checked: one lock, never nested and never held across a send, so no deadlock; tokio's Mutex is FIFO, so a lane's queued completion precedes any later reclaim by the same process; there is no heartbeat to starve, and a completion delayed past the 60 s lease is either applied (fence unchanged) or `Lost` to another instance's reclaim, whose resend is an idempotent replay; the claim's lock waits are bounded by its `lock_timeout 2s`. Residual liveness note: optional F16.

## MEM-P05-B2-F15 — marker-owned invariant docs no longer describe memory.save

- Context: round MEM-P05-B2-R2, digest a369b44e…fc75.
- Classification: optional suggestion; documentation/maintainability.
- Evidence: F2 added `memory.save` to `marker_owned_effect_kind` with an inline comment, but the invariant text
  still says every handler of these kinds acts "solely through `ActionService` remote/local dispatch"
  (src/application/workflow/actions/failure.rs header), `WorkflowHandler::execute` says the same
  (src/application/workflow/worker.rs:23-25), and `ProductionHandler` says each action-owned kind "performs
  effects only through ActionService's marker-first protocol" (src/infra/workflow.rs:132-134). messaging and
  agent.run each got a "joins" paragraph explaining their equivalent marker.
- Suggestion: add a "`memory.save` joins (phase 05 E4)" paragraph (the committed intent + park is its marker)
  and generalize the two other comments.
- Instruction lesson: none supported.
- Resolution history: 2026-11-09 open (optional).
  - 2026-11-09 recheck MEM-P05-B2-R3 (digest 9c7929bf…1abc, tree dddff33c): FIXED. failure.rs (module header, `marker_owned_effect_kind` doc with a "`memory.save` joins (phase 05 E4)" paragraph), worker.rs:22-26 and infra/workflow.rs (`ProductionHandler`, reservation comment) now name the committed intent + park as memory.save's marker equivalent.

## MEM-P05-B2-F16 — one row-lock wait in a completion now pauses every delivery lane

- Context: found in round MEM-P05-B2-R3, digest 9c7929bf1e3b9fce84664264e0530133669cf078bc77ff45a8ad587dafd51abc;
  a consequence of the F14 correction.
- Classification: optional suggestion; operations/liveness (not acceptance-blocking).
- Evidence: `complete_delivery` (src/adapters/persistence/memory/deliveries.rs) runs its fenced `UPDATE` directly on
  the pool with no `lock_timeout` (the claim sets `lock_timeout 2s`), and since R3 it runs under the worker-wide
  store Mutex. If a disable, owner removal or run-cancel transaction holds that intent's row (they lock open intents
  `FOR UPDATE`), the completion waits for that transaction and every other lane waits on the Mutex behind it. These
  transactions are short in normal operation, so the stall is bounded by their duration; before R3 only the one lane
  waited. Separately (pre-existing): a lane waiting on the Mutex at shutdown still claims an intent and then releases
  it as `interrupted`, spending a transient retry.
- Suggestion: give the completion the claim's `lock_timeout` (a timeout leaves the intent to lease-expiry reclaim and
  an idempotent resend), and check `stop` before claiming.
- Instruction lesson: none supported.
- Resolution history: 2026-11-09 open (optional).

## MEM-P05-B3 findings — common context

2026-11-09; workflow `implement-opus-reviewed-2_2`; task
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p05/docs/TASK.md (`## p05-B3`); review record
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p05/docs/REVIEW.md; batch p05-B3 (the four agent
memory tools); stage batch review; round note MEM-P05-B3-R1 (agent-review-notes.md); reviewer claude-opus-5-5, effort
xhigh (B abdd866fb428eea05); phase `p05`; worktree /Users/mac03/.codex/worktrees/60a0/openbot (detached HEAD a6bed48a);
review base dddff33c (p05-B2 accepted, MEM-P05-B2-R3); artifact digest
0d0b4622ed5d78726877bfccc1604450e4a67eff42ddff189957a58090336c3d (freeze tree dfeeca67). Same severity scale as above.

## MEM-P05-B3-F1 — the user scope is reachable from a manual run of a workflow another person published

- Context: round MEM-P05-B3-R1, digest 0d0b4622…6c3d (tree dfeeca67).
- Classification: actionable defect; Major (acceptance-blocking); security/privacy (E2 user-scope eligibility) and
  verification.
- Evidence: `private_session_user_on` (src/adapters/persistence/workflow/memory_tool_checkpoint.rs:208-250) makes
  the user scope eligible whenever the root run is bound to an unattached session created by the root's actor, the
  current run has the same actor, and the root's admission source is manual. D4 (TASK.md p05-B3) states why that is
  every manual start without a channel/thread association: `agent_history_sessions::capture` binds an unattached
  session to every such admission, and `manual::start` (src/application/workflow/manual.rs:48-101) starts any
  published workflow's active binding with the caller's authority. Nothing checks who published the workflow version
  (`workflow_versions.actor_id`), configured the binding (`workflow_binding_revisions.actor_id`) or authored the
  selected agent. So when U manually runs a workflow V published, V's agent step with `search_memory`/
  `memory_details` reads U's private memories and V's later steps (or the agent's own `message_send`) can deliver them
  wherever V chose. This is exactly scenario 3 of the architecture review's M2
  (tmp/memory-service-run/reviews/p7a-arch-r1.md:95-124: "A manual run of someone else's workflow … can forward the
  actor's memories wherever that workflow sends output. Levels cannot protect any of this."), whose resolution E2
  encodes as "the reader set is that user … a manual run outside a session … never eligible", and E11
  (implementation-decisions.md:1597) requires `memory_scope_` refusal cases for "a thread run's child, a schedule run
  as another user, a foreign-authored manual workflow and an attached session". M2 was closed for steps by removing
  their user scope; the tools reintroduce the vector, and D4 redefines E2's "manual run outside a session" as "a manual
  run associated with a channel/thread" in TASK.md only, without a decision-record change or a ruling on M2's case.
  Tests: `memory_scope_user_memory_requires_the_actors_own_manual_session` covers the actor's own manual root
  (eligible) and a same-actor schedule stand-in (refused). Not exercised: the foreign-authored manual workflow, an
  attached session (manual start with a channel/thread association), a thread run's child, and inheritance by a child
  of an eligible root (TASK's stand-in note says "no child fixture", but child fixtures exist, e.g.
  src/adapters/persistence/workflow/child_runtime_tests.rs).
- Required: get the decision owner's ruling on which manual runs may reach the user scope (for example: only when
  the actor published the root's workflow version and bound the selected agent, or only the agent's own private-chat
  binding), record it in implementation-decisions.md E2 (replacing D4's reinterpretation), implement it in
  `private_session_user_on`, and add the E11/M2 `memory_scope_` cases: foreign-authored manual workflow refused (no
  user namespace provisioned, no service call), attached session refused, child of a thread/message run refused,
  child of an eligible root inherits.
- Instruction lesson: Cause: E2's rule is written in a product term ("a manual run outside a session") that has no
  counterpart in the code, and the batch resolved it locally without re-checking the threat scenarios the term came
  from. Candidate instruction: when an implementation reinterprets a decision's eligibility or authorization term,
  trace it back to the review finding that introduced it and record the reinterpretation in the decision record,
  with the original threat scenarios as tests.
- Resolution history: 2026-11-09 open.
  - 2026-11-09 recheck MEM-P05-B3-R2 (digest 50d052b7…3457, tree d9d63035): RESOLVED by decision-owner ruling U4 (E12, COORDINATION I003 r2): eligibility rests on run facts only; the foreign-authored manual workflow is an allowed, documented case (docs/workflow-operations.md "Agent memory tools", E11 list amended). Records and docs match the ruling as worded (owner/admin-only manual starts confirmed by `manual::start`'s test). Code refactored into the pure `user_scope_owner` over facts read by `user_scope_owner_on`, equivalent to R1's SQL (ambiguous or missing root → none). Tests: unit matrix (own root, same-actor child, other-actor child, schedule, message/thread, attached, foreign creator, no root); real child inheritance on the production child path; the publisher ≠ actor assertion. Residual: the attached-session and thread-run refusals are unit-level only (optional F7).

## MEM-P05-B3-F2 — the memory tool checkpoint's authorization and fencing have no adversarial tests

- Context: round MEM-P05-B3-R1, digest 0d0b4622…6c3d.
- Classification: actionable defect; Minor; verification (E11 `memory_integration_` obligations, 05A focused checks).
- Evidence: `require_memory_invocation` (src/application/workflow/memory_tools/ports.rs:238-309) is the only
  binding between a model call and the saved invocation and prepared tool: argument equality, single matching
  prepared tool, target/name/input schema, origin binding, built-in source, trusted snapshot, pending-call state.
  No test references it, and none of the nine DB-backed `memory_tools::` tests forges a call: none changes the
  arguments, dispatches an unbound or mismatched memory tool, uses a stale fence or expired lease, replays a save
  invocation, or answers 401/403. E11 lists for `memory_integration_` "ungranted tools, tampered arguments, revoked
  credentials, lost ownership, … duplicate saves", and 05A's focused checks repeat them. The knowledge checkpoint
  that E6 says the tools follow has these families (knowledge_search_atomic_tests.rs:145
  `…forged_fences_and_changed_arguments_fail_before_embedding`, :51 `…failure_after_result_and_rise_rolls_back…`,
  :290 `…current_grant_revocation_blocks…`; knowledge_search_lease_tests.rs:51, :139 `…replacement_claim_replays_
  original_and_fences_stale_competitor`, :240 `…unknown_commit_ack…`); memory_tool_checkpoint.rs is a separate copy
  of that logic, so those tests do not cover it. Reviewer mutation C (frozen tree, isolated copy; patch
  logs/MEM-P05-B3-R1-B-mutation-C-invocation-binding.patch.txt) drops both the argument binding and the
  trusted-snapshot check from `require_memory_invocation`: every `memory_` test still passes (116 passed, 0 failed;
  logs/MEM-P05-B3-R1-B-mutation-C.txt).
- Required: add tool-level cases on the production entry points: changed arguments and a forged fence/task refuse
  before any service call; a call naming a memory tool the agent did not bind (or a tampered prepared tool) is
  refused; a stale fence or expired lease on observe, save and refuse commits nothing (no result, rise, consumption or
  intent); a replayed save invocation returns the same receipt with one intent and no second freeze; a 401/403 from
  the service is a controlled refusal that raises nothing.
- Instruction lesson: Cause: the new checkpoint module copies the knowledge one, but its adversarial test families
  were not ported. Candidate instruction: when a module is copied from an existing pattern, list the original's
  adversarial tests and port or explicitly map each one.
- Resolution history: 2026-11-09 open.
  - 2026-11-09 recheck MEM-P05-B3-R2 (digest 50d052b7…3457, tree d9d63035): FIXED. `memory_integration_forged_calls_and_fences_refuse_before_the_service` (7 forged fences × 4 entry points, changed arguments, other/unbound tool, save through a search invocation, immutable prepared tools; nothing sent or committed; genuine call then works), `…_a_replacement_claim_fences_the_stale_saver_and_replays_one_intent` (stale owner refused; same receipt and one intent on replay and on a second save), `…_service_auth_refusals_are_controlled_and_raise_nothing` (401/403 → `memory.refused`, recorded, replayed, nothing raised). The author's re-run of my mutation C fails at the changed-arguments assertion; the trusted-snapshot half is not reachable through stored state (prepared tools and agent manifests are immutable) — accepted as defense in depth.

## MEM-P05-B3-F3 — one multi-scope search takes several interactive slots at once (E1: a tool call is one operation)

- Context: round MEM-P05-B3-R1, digest 0d0b4622…6c3d.
- Classification: actionable defect; Minor; capacity/fairness (E1, src/AGENTS.md "Bound work at every external
  boundary").
- Evidence: `MemoryToolRuntime::search` (src/application/workflow/memory_tools/runtime.rs:142-161) runs one
  `client.search` per chosen scope under `join_all`, and each call acquires its own interactive permit
  (`MemoryServiceHttp::client`, src/adapters/memory_service/mod.rs:253-270; `MemoryPools::acquire`, limits.rs), with a
  per-company interactive share of 2 (`PER_COMPANY_INTERACTIVE`, src/infra/memory_config.rs:19). A search over
  company + agent scopes therefore holds the company's whole share, and with the user scope the third request waits
  for one of the first two inside its own 12 s deadline. Meanwhile every other interactive memory operation of that
  company (another run's `memory.load` or tool call) waits or fails `Unavailable`. E1
  (implementation-decisions.md:1181-1184): "at most 2 interactive … operations in flight per company. One recall,
  with its up to three concurrent `context.pack` calls, is one interactive operation; a tool call or `memory.load` is
  one." `memory_details` issues its per-scope fetches sequentially, so it holds one slot at a time. The e2e test
  passes because its client has `interactive_in_flight: 2` and only two scopes are ready.
- Required: make a multi-scope tool call one interactive operation (for example an adapter call that holds one
  company/global permit for its up to three per-namespace requests — the same mechanism E7's recall will need), and
  add a test showing that a three-scope search holds at most one company slot while a concurrent operation of the same
  company proceeds. Alternatively, get E1 amended with the decision owner.
- Instruction lesson: Cause: E1's accounting unit (operation, not request) was not traced when a tool started making
  several requests. Candidate: when a change adds concurrent remote requests, state which E1 operation they belong to
  and how its permit is held.
- Resolution history: 2026-11-09 open.
  - 2026-11-09 recheck MEM-P05-B3-R2 (digest 50d052b7…3457, tree d9d63035): CODE FIXED, VERIFICATION NOT: `MemoryClient::search_scopes` holds one interactive permit for all of a search's per-namespace requests (sent concurrently, shared deadline, answers in order, mixed tenants a protocol fault) and the tool uses it. But the decisive test does not distinguish it from per-request permits: reviewer mutation D (search_scopes back to one permit per request) passes it 5/5 → new MEM-P05-B3-F6.

## MEM-P05-B3-F4 — memory tools are offered and publishable while the memory service is unconfigured

- Context: round MEM-P05-B3-R1, digest 0d0b4622…6c3d.
- Classification: actionable defect; Minor; configuration gating (E1, E5).
- Evidence: the catalog (`builtins(memory_enabled)`, src/adapters/persistence/agent_authoring/catalog.rs:245; reads.rs
  passes `memory_enabled_on`) and agent publication (`memory_publication_on`,
  src/adapters/persistence/agent_publication/tools.rs:115-128) check only `memory_tenants.state = 'enabled'`. E1
  (implementation-decisions.md:1185-1186): with `MEMORY_SERVICE_URL` unset, "memory kinds and builtins refused"; E5
  (:1433-1434): offered "only while memory is configured and enabled". The steps do this
  (`WorkflowServices::refused_kinds`, src/infra/workflow/services.rs:141-147;
  `memory_integration_configured_publication_gates_memory_steps_per_company`). D10 records only that already-published
  tools then fail with `ServiceUnavailable` at dispatch (src/adapters/workflow_agent/mod.rs:201-206). It does not say
  that, in the same state (tenant enabled, URL later removed), the editor still offers the four tools and a new agent
  version with them publishes, and every run calling one fails retryably.
- Required: refuse the memory built-ins in the catalog and at publication while the service is unconfigured, as the
  steps do, with a configured/unconfigured test. Alternatively, extend D10 to the catalog and new publications and get
  the decision owner's acceptance of the deviation from E1/E5 recorded.
- Instruction lesson: Cause: the known edge was recorded for one surface (dispatch) instead of every surface the
  decision names. Candidate: a recorded deviation lists each surface the decision names and its behavior there.
- Resolution history: 2026-11-09 open.
  - 2026-11-09 recheck MEM-P05-B3-R2 (digest 50d052b7…3457, tree d9d63035): RESOLVED by decision-owner ruling U5 (E12, COORDINATION r2): a documented operator edge. docs/deploy.md "Memory service" states the editor list, publication and failing calls, and the remedy; D10 names every surface. Checked: disable is allowed while unconfigured (`MemorySettings::disable`; the company memory page still shows the switch for an enabled tenant), so the documented remedy works.

## MEM-P05-B3-F5 — the workflow-bundle publication gate and the catalog's enabled read are untested

- Context: round MEM-P05-B3-R1, digest 0d0b4622…6c3d.
- Classification: optional suggestion; verification.
- Evidence: D6's second gate, `authority::builtin_memory_tool` on `ToolCheck::Publish`
  (src/adapters/persistence/workflow/authority.rs:167-186), refuses a workflow bundle with memory snapshots while
  memory is disabled. The tests exercise it only on the enabled path (the fixture enables memory first). The
  catalog's DB wiring (`reads.rs` → `memory_enabled_on`) is untested; only the pure `builtins(bool)` is.
  `memory_scope_memory_tools_publish_only_while_memory_is_enabled` covers agent publication only.
- Suggestion: publish a workflow using a memory-tool agent after a disable (refused, NotFound) and read the built-in
  catalog through `AgentAuthoringReads` in both states.
- Instruction lesson: none supported.
- Resolution history: 2026-11-09 open (optional).
  - 2026-11-09 recheck MEM-P05-B3-R2 (digest 50d052b7…3457, tree d9d63035): RESOLVED. Catalog taken: `memory_scope_authoring_catalog_lists_memory_tools_only_while_enabled` reads through the production `tool_options` (four while enabled, none after disable). Bundle gate not taken — reason verified: a bundle's own tool snapshots are the message built-ins, HTTP/MCP selections and workflow pins; registrations refuse reserved names; a memory-named snapshot would be untrusted and refused anyway.

## MEM-P05-B3-F6 — the one-operation search test passes with one permit per request

- Context: found in round MEM-P05-B3-R2 (correction recheck), digest 50d052b7858ba6aab6cdad37ff2e0022d73d4a7f465bbe3d235146a802b23457
  (tree d9d63035); the test added for F3.
- Classification: actionable defect; Minor; verification (the F3 correction's decisive test does not discriminate).
- Evidence: `memory_integration_a_multi_scope_search_is_one_interactive_operation`
  (src/adapters/memory_service/tests.rs, added in R2) holds every `observations/search` for 400 ms and then waits up
  to 5 s for `arrivals` to reach 3. It does not require the three requests to be in flight together. With
  per-request permits (company share 2), two requests arrive, finish their hold and release their slots, and the
  third then arrives. The loop accepts that, `interactive_in_use` is then 1 (only the third holds a slot), the
  concurrent fetch gets the free slot, and the search is still held, so every assertion passes. Reviewer mutation D
  (logs/MEM-P05-B3-R2-B-mutation-D-per-request-permits.patch.txt: `search_scopes` as `join_all` of `self.search`, the
  R1 behaviour) passed it 5 of 5 runs, and the whole `adapters::memory_service::` module plus the tool e2e stayed green
  (logs/MEM-P05-B3-R2-B-mutation-D.txt; 0.86 s per run ≈ two hold periods, i.e. sequential). TASK's claim "Under the
  old per-request permits only two could arrive" holds only within one hold period, which the test does not check.
  The production code is correct by reading: one `acquire`, then `join_all` of `send` under it.
- Required: make the test fail under per-request permits. For example, require all three arrivals before the first
  hold ends (a long hold with a wait bounded well below it, or the fake recording the peak number of concurrently held
  calls and the test asserting 3 while `interactive_in_use` is 1). Record mutation D (or an equivalent) failing it.
- Instruction lesson: Cause: the decisive assertion was argued ("only two could arrive") but not checked against the
  old behaviour. Candidate: a correction's decisive test is run once against the uncorrected code (or an equivalent
  mutation) and the failure recorded, as the E6 mutations are.
- Resolution history: 2026-11-09 open.
  - 2026-11-09 recheck MEM-P05-B3-R3 (digest 8857f538…a777, tree a69f88c5): FIXED. The contract fake counts held calls now/peak (`held_now`, `held_peak`). The test holds each search 1.5 s and waits ≤ 5 s until three are held at the same time — unreachable with per-request permits under the company share of 2 — then asserts one slot in use, a concurrent fetch completing meanwhile, peak 3, and 0 slots after. Reviewer mutation D re-run on the freeze tree fails it 3/3 with `Elapsed` at the wait (logs/MEM-P05-B3-R3-B-mutation-D.txt); the unmutated build passes it (mutation E run). `search_scopes` itself is unchanged since R2.

## MEM-P05-B3-F7 — the attached-session and thread-run refusals are proven only in the pure decision

- Context: found in round MEM-P05-B3-R2, digest 50d052b7…3457.
- Classification: optional suggestion; verification (E11 `memory_scope_` as amended by U4).
- Evidence: E11 (amended) lists user-scope refusals for "a thread run's child, a schedule run as another user and an
  attached session". `memory_scope_user_scope_eligibility_matrix` covers them on `user_scope_owner` with hand-built
  `UserScopeRoot` facts. The DB cases (the actor's manual root, a same-actor schedule stand-in, a real child) never
  produce `attached = true` or a message source, so the SQL fact `(session.channel_id IS NOT NULL OR
  session.thread_id IS NOT NULL)` in `user_scope_owner_on` could read the wrong columns or be constant `false`
  without any test failing. TASK cites "no channel/thread fixture in this harness", but other harnesses admit with an
  association (e.g. message_test_support.rs `admitted_with`, the history thread fixtures).
- Suggestion: one DB case of `user_scope_owner_on` for a manual start associated with a channel/thread (attached →
  none) and for a message-triggered root's child.
- Instruction lesson: none supported.
- Resolution history: 2026-11-09 open (optional).
  - 2026-11-09 recheck MEM-P05-B3-R3 (digest 8857f538…a777, tree a69f88c5): TAKEN (optional). `memory_scope_a_manual_run_in_a_channel_has_no_user_scope` (message_tests.rs) admits real manual runs of one binding by one actor: the channel run's stored session is attached and `user_scope_owner_on` gives none; the company-wide run gives the actor. Reviewer mutation E (the attachment fact read as constant `false`, logs/MEM-P05-B3-R3-B-mutation-E-attachment-ignored.patch.txt) fails it at message_tests.rs:640 while the other eligibility tests stay green (-B-mutation-E.txt). The message-source refusal stays proven through the schedule stand-in's non-manual source plus the unit matrix — sufficient.

## MEM-P05-B4 findings — common context

2026-11-09; workflow `implement-opus-reviewed-2_2`; task
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p05/docs/TASK.md (`## p05-B4`); review record
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p05/docs/REVIEW.md; batch p05-B4 (automatic
recall, 05B.1–4, E7); stage batch review; round note MEM-P05-B4-R1 (agent-review-notes.md); reviewer claude-opus-5-5,
effort xhigh (B abdd866fb428eea05); phase `p05`; worktree /Users/mac03/.codex/worktrees/60a0/openbot (detached HEAD
a6bed48a); review base a69f88c5 (p05-B3 accepted); artifact digest
a998bca3b5a3688a535b43b4cbd49d62e1cbe2d3601921a78679b049d21eece1 (freeze tree 3749a60c). Same severity scale as above.

## MEM-P05-B4-F1 — a resumed attempt replays its frozen pack unvalidated when no recall is composed

- Context: round MEM-P05-B4-R1, digest a998bca3…eece1.
- Classification: actionable defect; Minor; fail-open path (E7 resume check; the U5 operator edge).
- Evidence: `WorkflowAgentRuntime::run` (src/adapters/harness/rig/workflow.rs:306-357) derives `resumed` only from
  `recall.step()`. With no `MemoryRecall` composed (`with_memory` runs only while `MEMORY_SERVICE_URL` is set), `step` is
  `RecallStep::None`, `resumed` is false and the check at :340 never runs. The preparation still restores the frozen
  pack (`frozen::restore_on`, agent_history/frozen.rs:24-30) and `initial_prompt` sends it to the provider. So a run
  that froze a pack under a configured service and resumes after the URL is removed (U5's documented edge) replays
  recalled memory with no revalidation at all. Deleted, corrected or revoked memory therefore reaches the model, even
  though E7 requires validation "once per resumed attempt, before its first provider entry", with service-unavailable
  answered as retryable. Under the same edge the tools fail closed (U5: "every call fails closed"). Not tested: every
  recall test composes recall.
- Required: when the restored preparation carries frozen memory items and no recall is composed, answer the
  retryable `memory.context_unavailable` (never a provider entry), and add a test that resumes a frozen pack on a
  runtime without `set_recall`. Optionally take `resumed` from the preparation result (restored, not frozen now)
  rather than from the earlier plan read, so the two cannot disagree.
- Instruction lesson: Cause: the resume check was attached to the recall component's presence instead of to the
  frozen state it protects. Candidate: a safety check on restored state is keyed on that state, not on whether the
  optional component that produced it is configured.
- Resolution history: 2026-11-09 open.
  - 2026-11-09 recheck MEM-P05-B4-R2 (digest 507e8938…8f1b, tree ff1607fd): FIXED (structural). `WorkflowAgentRuntime.recall` is no longer optional; `new` (bound `S: MemoryRecallPersistence + Clone`, satisfied only by `PostgresPersistence`) composes `MemoryRecall::unconfigured`, and `with_memory` replaces it. `run` calls `validate` unconditionally before the only call of `run_ready`; with no client, a restored pack or recorded tool items answer retryable `memory.context_unavailable`. Construction sites checked: production `infra/workflow.rs` (then `with_memory`), the autonomous e2e support and the test fixtures all go through `new`. Test `memory_preparation_a_resumed_pack_without_a_configured_service_is_unavailable`; the author's mutation K (no client → Valid) fails it. Not taking `resumed` from the preparation result is accepted: under the owner lease the plan read and the freeze cannot disagree, and the plan checks Frozen before the enabled check.

## MEM-P05-B4-F2 — recorded memory tool items are never revalidated on resume, though a fetch-based check is possible

- Context: round MEM-P05-B4-R1, digest a998bca3…eece1.
- Classification: actionable defect; Minor; deviation from E7 without a decision-owner record.
- Evidence: E7 (implementation-decisions.md:1510-1513): once per resumed attempt `context.validate` "checks the pack
  and the recorded memory tool items: changed/expired/unavailable/401/403 → terminal `memory.context_revoked`". TASK
  p05-B4 (TASK.md:472-474) skips the tool items because `PackedRevision` requires `sources_sha256`, which tool results
  lack. The deviation is recorded only in TASK; E7 is unchanged. As a result a resumed attempt replays the checkpoint's
  earlier `search_memory`/`memory_details`/`memory_timeline` results even if those memories were deleted, corrected or
  moved above the ceiling since. Only the "recorded exposure ≤ current ceiling" replay rule applies, which catches a
  ceiling drop but not deletion or correction. The data for a substitute check already exists: every tool read
  records `{scope, id, revision, access_level, content_sha256?}` in `workflow_memory_consumptions`, and
  `MemoryClient::fetch` (src/application/memory/ports.rs:376-383) returns whole bodies with revision under the
  ceiling, leaving deleted, foreign or above-ceiling ids absent. An absent id or a different revision (or
  `content_sha256` for details) would be the revocation. Only E7's provenance-change and expiry statuses would remain
  undetected.
- Required: either revalidate recorded tool items on resume (for example by `fetch`, mapping absent or changed
  revision to `memory.context_revoked` and an outage to `memory.context_unavailable`, with a test), or have the
  decision owner accept the deviation and record it in E7 (with the residual risk) rather than only in TASK.
- Instruction lesson: Cause: a contract gap was resolved by dropping the requirement rather than by looking for
  another route to the same guarantee. Candidate: when a decision's mechanism is unavailable, record the alternative
  considered and get the deviation accepted in the decision record.
- Resolution history: 2026-11-09 open.
  - 2026-11-09 recheck MEM-P05-B4-R2 (digest 507e8938…8f1b, tree ff1607fd): FIXED, with a test gap → MEM-P05-B4-F6. Recorded tool items (this execution's non-nil consumption rows) are revalidated on every claim through `fetch` under the current ceiling (per scope, chunks of 64, dedup by id and revision): an absent id or a different revision → `memory.context_revoked`, an outage → `memory.context_unavailable`, the highest recorded exposure above the ceiling → revoked before any call. The residual (tool-item provenance change and expiry) is recorded in E7's "As implemented" paragraph. That is the route this finding offered, so no further decision-owner ruling is needed. The test covers only a corrected revision; reviewer mutation L (an absent id passes) survives every recall and tool test.

## MEM-P05-B4-F3 — E11's "recall deadline" and "two concurrent recalls in one company" cases are missing

- Context: round MEM-P05-B4-R1, digest a998bca3…eece1.
- Classification: actionable defect; Minor; verification (E11 `memory_preparation_`, E1).
- Evidence: E11 (implementation-decisions.md:1600) lists for `memory_preparation_`: "… optional unavailable, recall
  deadline, two concurrent recalls in one company both succeeding". None of the nine B4 tests exercises either.
  `memory_preparation_optional_failures_are_distinct_from_refusals` only maps `Ambiguous` to `"deadline"`. Nothing
  makes a pack slower than `MEMORY_SERVICE_RECALL_DEADLINE_MS` (`context_pack`'s deadline is
  `recall_deadline.min(request_timeout)` with the permit wait included, src/adapters/memory_service/mod.rs:501).
  Nothing runs two recalls of one company together. The second case is E1's accounting: a three-scope recall must
  hold one interactive slot, so two concurrent recalls fit the company share of 2. Code with one permit per pack
  request would pass every current test, which is the gap F6 closed for `search_scopes` in B3. The contract fake
  already supports `holds`/`held_peak`.
- Required: add (1) a recall whose packs are held past the recall deadline, with the run continuing and the
  preparation frozen as `unavailable{deadline}` (or the permit-wait `unavailable`) with nothing raised or recorded;
  and (2) two concurrent three-scope recalls of one company both completing with their packs while the company's
  interactive slots in use never exceed 2 (held_peak showing the requests in flight together). Each should fail under
  its obvious mutation (no recall deadline; one permit per request).
- Instruction lesson: Cause: the E11 case list was not checked line by line against the mapping. Candidate: the
  verification mapping cites each E11 case of the batch's prefix or marks it as a declared gap.
- Resolution history: 2026-11-09 open.
  - 2026-11-09 recheck MEM-P05-B4-R2 (digest 507e8938…8f1b, tree ff1607fd): FIXED. `memory_preparation_a_recall_past_its_deadline_continues_without_memory` (pack held 3 s, deadline 1 s → completes < 2.5 s, frozen `unavailable{deadline}`, no recalled block; the author's mutation H fails it) and `memory_preparation_two_concurrent_recalls_in_one_company_both_succeed` (six pack requests held at once while 2 company slots are in use, both return 3 packs, peak 6; the author's mutation I, a permit per request, fails it). Margin note: the second test needs all six requests to arrive within a 700 ms hold (wait 650 ms); it passed in my full focused run and three repeats.

## MEM-P05-B4-F4 — the recall cap and byte term deviate from E7 only in TASK

- Context: round MEM-P05-B4-R1, digest a998bca3…eece1.
- Classification: actionable defect; Minor; decision-record drift (bounds changed without the decision owner).
- Evidence: E7 (implementation-decisions.md:1502-1503) fixes `max_tokens = min(2,048, …/4)` and bytes ≤ min(64 KiB,
  `max_context_bytes`/4). The code uses `MAX_RECALL_TOKENS = 8192` and a fixed `MAX_RECALL_BYTES = 65_536`
  (src/application/workflow/agent_memory.rs:16-18, D1 in TASK). On substance D1 is justified. The service charges a
  pack 64 + 2 tokens per item byte (services/memory/src/domain/packing.rs:7-9, "OpenBot's provider-independent upper
  bound"), and a compact item is roughly 400–700 JSON bytes (snippet ≤ 320 bytes plus ids, hashes and timestamps).
  So E7's 2,048 cannot fit even one item in a 512-token agent or user quarter. 8,192 still yields to the request's
  quarter rule (history keeps ≥ 3/4), and roughly 12 items at most keep the consumption row under its 64-item CHECK.
  But it quadruples the decided per-run recall size, and E7 still states 2,048. The byte term is also dropped
  ("the harness has no byte policy").
- Required: record D1 and the byte term in E7 (or as an E12 user decision) with the decision owner's acceptance,
  including the item-count/CHECK reasoning above. No code change is required if accepted.
- Instruction lesson: Cause: a calibration found by a test was fixed in code and TASK only. Candidate: a deviation
  from a decided number is recorded in the decision record in the same batch.
- Resolution history: 2026-11-09 open.
  - 2026-11-09 recheck MEM-P05-B4-R2 (digest 507e8938…8f1b, tree ff1607fd): RESOLVED by decision-owner ruling U6 (E12; E7 budget text updated; COORDINATION r3): 8,192-token cap and fixed 64 KiB byte bound.

## MEM-P05-B4-F5 — revalidation runs per scope sequentially, outside the recall deadline

- Context: round MEM-P05-B4-R1, digest a998bca3…eece1.
- Classification: optional suggestion; latency/capacity (E1 spirit).
- Evidence: `validate_scopes` (src/application/workflow/memory_recall.rs:225-276) calls `context_validate` scope by
  scope, and each call takes its own interactive permit with the full `request_timeout` (12 s default;
  memory_service/mod.rs:524-). A resumed attempt can therefore wait up to about three timeouts before its first
  provider entry, outside `MEMORY_SERVICE_RECALL_DEADLINE_MS`. The fresh-attempt pack is one operation under that
  deadline.
- Suggestion: validate the scopes concurrently as one interactive operation under the recall deadline (the
  `context_pack` shape), mapping a deadline to the retryable `memory.context_unavailable`.
- Instruction lesson: none supported.
- Resolution history: 2026-11-09 open (optional).
  - 2026-11-09 recheck MEM-P05-B4-R2 (digest 507e8938…8f1b, tree ff1607fd): NOT TAKEN (optional), reason accepted: revalidation runs only on a claim restoring earlier memory, never per tool round; a failure is already the retryable `memory.context_unavailable`; each call is bounded by the request timeout.

## MEM-P05-B4-F6 — a deleted tool-read memory is never shown to be revoked: the absent-id path is untested

- Context: found in round MEM-P05-B4-R2 (correction recheck), digest
  507e893896000e551a847db69b4179ba7b399d7683e4feb2e82fd9a8401f8f1b (tree ff1607fd); the test added for F2.
- Classification: actionable defect; Minor; verification (E7: "revocation/deletion produces the existing controlled
  refusal").
- Evidence: `validate_tool_items` (src/application/workflow/memory_recall.rs, `async fn validate_tool_items`) revokes
  when a recorded id is absent from the `fetch` answer (deleted, purged, foreign or above the ceiling) or comes back
  at another revision. `memory_preparation_resumed_attempts_revalidate_recorded_tool_items`
  (memory_recall_tests.rs:437) exercises only the second case (revision 1 → 2) and the unchanged case. Reviewer
  mutation L (logs/MEM-P05-B4-R2-B-mutation-L-deleted-tool-item-passes.patch.txt: an id the fetch no longer returns
  passes; a changed revision still revokes) passes every `memory_preparation` and `memory_tools::` test, 32 of 32
  (logs/MEM-P05-B4-R2-B-mutation-L.txt). Deletion is E7's primary revocation case, and its absent-id branch is
  separate code from the revision compare. The production code is correct by reading.
- Required: add the deleted case to the tool-item test (remove the observation from the fake, or raise it above the
  ceiling, between the attempts → `memory.context_revoked`, no provider entry), and record mutation L failing it.
- Instruction lesson: Cause: the decisive test covered one branch of a two-branch predicate. Candidate: each
  revocation cause a decision names (deleted, corrected, above the ceiling) gets its own case or a stated mapping to
  a shared branch.
- Resolution history: 2026-11-09 open.
  - 2026-11-09 recheck MEM-P05-B4-R3 (digest 8490e2e6…78ee, tree 2a0d2550): FIXED. `memory_preparation_resumed_attempts_revalidate_recorded_tool_items` now runs corrected / deleted (the fake no longer returns the id) / unchanged; each revoked case asserts (Terminal, `memory.context_revoked`), 0 provider entries and exactly one revalidation fetch. My mutation L, re-run on the freeze tree (isolated copy, deleted), fails it at memory_recall_tests.rs:481 (31/32; logs/MEM-P05-B4-R3-B-mutation-L.txt); the unmutated selection passes 32/32.

## MEM-P05-B4-F7 — the docs do not say that disabling memory revokes in-flight runs that were shown memory

- Context: found in round MEM-P05-B4-R2, digest 507e8938…8f1b.
- Classification: optional suggestion; operator documentation.
- Evidence: after a disable, `revalidation` finds no ready scope (`ready_scopes_on` → `NotEnabled` → no scopes), so
  a resumed claim whose earlier attempt was shown recalled or tool-read memory ends terminally with
  `memory.context_revoked`. That fits E7, where a disabled tenant answers 403 and 403 means revoked. But docs/deploy.md
  ("after a disable a published agent keeps running and each memory call answers `memory.not_enabled`") and
  docs/workflow-operations.md ("memory deleted, corrected or above the ceiling since fails the step") do not mention
  this operator-visible effect of a disable.
- Suggestion: add one sentence to the disable text: runs that were shown memory and resume after the disable fail
  with `memory.context_revoked`.
- Instruction lesson: none supported.
- Resolution history: 2026-11-09 open (optional).
  - 2026-11-09 recheck MEM-P05-B4-R3 (digest 8490e2e6…78ee, tree 2a0d2550): TAKEN (optional). docs/deploy.md's disable paragraph and docs/workflow-operations.md's recall paragraph now state that a run already shown memory fails with `memory.context_revoked` when it resumes after a disable. That matches `scopes_on` → `ready_scopes_on` (no scopes while disabled) → revoked.

## MEM-P05-B5 findings — common context

2026-11-09; workflow `implement-opus-reviewed-2_2`; task
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p05/docs/TASK.md (`## p05-B5`); review record
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p05/docs/REVIEW.md; batch p05-B5 (capture policy,
generation intents at acceptance, generation delivery, E9 races); stage batch review; round note MEM-P05-B5-R1
(agent-review-notes.md); reviewer claude-opus-5-5, effort xhigh (B abdd866fb428eea05); phase `p05`; worktree
/Users/mac03/.codex/worktrees/60a0/openbot (detached HEAD a6bed48a); review base 2a0d2550 (p05-B4 accepted);
artifact digest be48350f22f124546d81d9d9abc175039d0c971b8580c817ac32fe103629a5c3 (freeze tree 8f5f0f97). Same
severity scale as above.

## MEM-P05-B5-F1 — the operator docs say message-triggered and scheduled runs are never captured

- Context: round MEM-P05-B5-R1, digest be48350f…29a5c3.
- Classification: actionable defect; Minor; documentation of a privacy-relevant data flow (AGENTS.md: production
  docs must describe what the application does).
- Evidence: docs/workflow-operations.md:265-266 (new "Agent memory tools" capture paragraph): "Scheduled,
  message-triggered and child runs, failed or cancelled runs and samples are never captured." Thread capture does
  capture message-triggered runs. `capture_on` (src/adapters/persistence/workflow/agent_history/capture.rs:49)
  maps every `thread` conversation to the agent scope whatever started the run. The batch's own test
  `memory_capture_a_thread_exchange_is_captured_into_the_agent_memory_at_the_thread_level` captures a run triggered
  by a customer email, and that is the common thread source. A scheduled run whose conversation is a thread is
  captured the same way. Only private-session capture excludes scheduled and message-triggered roots, through U4's
  manual-source check. An owner deciding whether to turn thread capture on is told the opposite of what happens to
  customer-thread content.
- Required: scope the sentence correctly, for example: scheduled and message-triggered runs never reach
  private-session capture; child-branch, attached private-session, failed or cancelled exchanges and samples are
  never captured; thread capture covers every completed exchange of the agent's channel threads.
- Instruction lesson: Cause: an exclusion list for one capture kind was written as if it applied to both. Candidate:
  check each capture-exclusion statement in the docs against `capture_on`'s match arms.
- Resolution history: 2026-11-09 open.
  - 2026-11-10 recheck MEM-P05-B5-R2 (digest 6041e394…4f1e, tree 8e18b451): FIXED. docs/workflow-operations.md and the settings page now say that private-session capture never covers scheduled, message-triggered or child runs, and that thread capture covers every completed exchange of the agent in a channel thread, whatever started the run (customer messages included). Child branches, failed or cancelled runs and samples are never captured. The settings test asserts the corrected sentences. This matches `capture_on`.

## MEM-P05-B5-F2 — with the URL removed, capture keeps freezing new intents that nothing sends or settles

- Context: round MEM-P05-B5-R1, digest be48350f…29a5c3.
- Classification: actionable defect; Minor; retention of conversation text / documentation (U5 edge).
- Evidence: the acceptance hook (agent_history/checkpoint.rs:283-298) → `capture_on` → `enqueue_capture_on`
  (memory/deliveries.rs:305-) checks only database state: tenant enabled, cached cap > 0, ready namespace, policy on.
  None of it changes when `MEMORY_SERVICE_URL` is removed. Without the URL there is no memory composition and no
  delivery worker (E1). Every completed exchange of a capture-enabled agent therefore keeps inserting a `pending`
  generation intent whose frozen request holds the exchange's input and output (up to 512 KiB). Nothing claims it, so
  nothing marks it `expired` or drops the body; that happens only at a claim once a worker runs again. docs/deploy.md:778
  says "Without the URL, captures already queued are never sent and expire", which implies nothing new is queued and
  that expiry cleans up. Both are inaccurate. Under the same edge every other memory path fails closed (U5: tool
  calls; recall stops; resumed memory is unavailable).
- Required: either skip capture while the process has no memory service (for example a `not_configured` skip
  decided where the composition is known, with a test), or record with the decision owner (U5 extension) that capture
  keeps queuing in this state and correct docs/deploy.md: new captures queue with their text until the URL returns,
  then the expired ones settle and drop it.
- Instruction lesson: Cause: the U5 edge was applied to the paths that call the service, not to the path that only
  writes for later delivery. Candidate: for an operator edge, list every writer of work for the missing component,
  not only its callers.
- Resolution history: 2026-11-09 open.
  - 2026-11-10 recheck MEM-P05-B5-R2 (digest 6041e394…4f1e, tree 8e18b451): RESOLVED by decision-owner ruling U7 (E12; COORDINATION r4) and implemented: capture is armed only in a preparation frozen by a runtime with the service configured (`MemoryPreparation.capture = recall.configured()`, `with_capture` at the first freeze, re-bound by `restore_on` before its whole-document comparison), and `accepted_on` captures only an armed exchange. The test `memory_capture_nothing_is_captured_without_a_configured_service` covers it, and the author's M6 (acceptance ignores the flag) and M7 (every runtime arms) each fail it. The U7 residual (prepared while configured, accepted after removal) is recorded, and the deploy.md text now matches the code: disable settlement drops the text (`settle_open_on` sets `request = NULL`). The thread test now runs the production `with_memory` composition and its own delivery worker.

## MEM-P05-B5-F3 — `validate`'s doc comment now documents `provision`

- Context: round MEM-P05-B5-R1, digest be48350f…29a5c3.
- Classification: optional suggestion; code quality (nit).
- Evidence: src/application/workflow/memory_recall.rs:234-250. The new `async fn provision` was inserted between
  `validate`'s doc comment ("Before a claimed attempt's first provider entry (E7): a resumed attempt revalidates its
  frozen pack …") and `pub async fn validate`, so rustdoc attaches that text to `provision` and `validate` has none.
- Suggestion: move `provision` above the comment and give it its own one-line doc.
- Instruction lesson: none supported.
- Resolution history: 2026-11-09 open (optional).
  - 2026-11-10 recheck MEM-P05-B5-R2 (digest 6041e394…4f1e, tree 8e18b451): FIXED: `validate`'s comment is back above `validate`, and `provision` has its own. A new placement nit of the same kind was introduced next to it → MEM-P05-B5-F5 (optional).

## MEM-P05-B5-F4 — the mapping does not cite E11's "delete vs delivery" for capture intents

- Context: round MEM-P05-B5-R1, digest be48350f…29a5c3.
- Classification: optional suggestion; verification mapping.
- Evidence: E11 lists `memory_capture_` "… competing claimants, delete vs delivery". The B5 tests race delete
  against the freeze, not against an in-flight generation delivery. Settlement, claim fencing and completion are
  kind-agnostic and were proven with saves in B2 (`memory_integration_disable_during_an_in_flight_delivery_loses_its_completion`,
  `memory_integration_run_cancel_during_an_in_flight_delivery_loses_its_completion`, owner-removal cancel). The B5
  mapping cites none of them.
- Suggestion: cite those tests in the B5 mapping with the kind-agnostic reasoning, or add one in-flight generation
  delivery vs owner removal case.
- Instruction lesson: none supported.
- Resolution history: 2026-11-09 open (optional).
  - 2026-11-10 recheck MEM-P05-B5-R2 (digest 6041e394…4f1e, tree 8e18b451): TAKEN. `memory_capture_agent_delete_during_an_in_flight_delivery_loses_its_completion` holds a generation at the fake and deletes the agent: the intent becomes `cancelled(owner_removed)` at once with the body dropped, and the worker's completion is fenced out (stays cancelled, no result).

## MEM-P05-B5-F5 — `unconfigured`'s doc comment now prefixes `configured()`

- Context: found in round MEM-P05-B5-R2, digest 6041e394d913523eb06c8aaac8348c675134643e8947fe1281a6a20eca9e4f1e
  (tree 8e18b451); introduced with U7's `configured()`.
- Classification: optional suggestion; code quality (nit, same kind as F3).
- Evidence: src/application/workflow/memory_recall.rs:158-167. `pub fn configured` was inserted between the
  comment "The guard an unconfigured deployment composes: no recall, and no replay of earlier memory it cannot
  revalidate." and `pub fn unconfigured`. Rustdoc now gives `configured` both texts and `unconfigured` none.
- Suggestion: move `configured` (with its own line) below `unconfigured`.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).
  - 2026-11-10 recheck MEM-P05-B6-R1 (digest 88808f5a…b0a1, tree 7388d1c2): FIXED. The comment is back above
    `unconfigured`, and `configured` keeps its own one-line comment (memory_recall.rs:158-165).

## MEM-P05-B6 findings — common context

2026-11-10; workflow `implement-opus-reviewed-2_2`; task
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p05/docs/TASK.md (`## p05-B6`); review record
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p05/docs/REVIEW.md; batch p05-B6 (the phase-05
gate, E11: live integration script, CI job, live and browser tests); stage gate review, reviewed as code; round note
MEM-P05-B6-R1 (agent-review-notes.md); reviewer claude-opus-5-5, effort xhigh (B abdd866fb428eea05); phase `p05`;
worktree /Users/mac03/.codex/worktrees/60a0/openbot (detached HEAD a6bed48a); review base 8e18b451 (p05-B5
accepted); artifact digest 88808f5ad06857e86e417fe9593fd6b72185bdc01b42ad52871054c77e81b0a1 (freeze tree 7388d1c2).
Same severity scale as above. All three findings are optional; none blocks the gate.

## MEM-P05-B6-F1 — the CI job builds the memory service with an implicitly installed toolchain

- Context: found in round MEM-P05-B6-R1, digest 88808f5ad06857e86e417fe9593fd6b72185bdc01b42ad52871054c77e81b0a1
  (tree 7388d1c2); the new `memory-integration` job in .github/workflows/ci.yml.
- Classification: optional suggestion; CI reproducibility and clarity (low).
- Evidence:
  - .github/workflows/ci.yml:34 installs `dtolnay/rust-toolchain@stable`. The action runs `rustup toolchain install`
    and `rustup default`, and sets no `RUSTUP_TOOLCHAIN`.
  - scripts/memory-integration.sh:53 builds inside services/memory, where rust-toolchain.toml:2 pins 1.95.0 (with
    rustfmt and clippy). So the service build depends on rustup installing 1.95.0 implicitly at that moment, and
    the stable toolchain the job installed builds only OpenBot.
  - memory-service.yml:31 installs `dtolnay/rust-toolchain@1.95.0` explicitly.
  - Implicit installation is the default in current rustup, but rustup 1.28.0 turned it off and
    `RUSTUP_AUTO_INSTALL=0` disables it. rust-cache does not cache that download.
  - CI execution was not exercised, so neither outcome has been shown.
- Suggestion: install the pinned toolchain explicitly in this job, so it states what memory-service.yml states.
  Either add a `dtolnay/rust-toolchain@1.95.0` step before the stable one (the last step sets the default), or run
  `rustup toolchain install` in services/memory, which installs the toolchain the toml names.
- Instruction lesson: none supported. The toml pin already satisfies AGENTS.md's reproducibility rule; this
  finding is about making the pin visible in the job.
- Resolution history: 2026-11-10 open (optional).

## MEM-P05-B6-F2 — the service URL keeps only a TCP host and port from the admin URL

- Context: found in round MEM-P05-B6-R1, digest 88808f5ad06857e86e417fe9593fd6b72185bdc01b42ad52871054c77e81b0a1
  (tree 7388d1c2); scripts/memory-integration.sh.
- Classification: optional suggestion; script robustness and documentation (low).
- Evidence:
  - scripts/memory-integration.sh:61-63 strips the query and keeps only the authority after the last `@`.
  - A socket-style admin URL (`postgres:///postgres?host=/path`) leaves the host empty. The service is then
    pointed at its driver's default server, not the one psql provisioned through provision-db.sh. Locally that
    default can be localhost:5432.
  - Query parameters such as `sslmode` are dropped from the service URL.
  - The run fails closed: the role exists only on the provisioned server, so startup fails and cleanup drops the
    database.
  - The header (line 5) does not say the URL must name a TCP host.
  - Analysis only: not run, because it would reach the default server.
- Suggestion: before provisioning, refuse an empty host with a message saying the URL must name `host[:port]`.
  State in the header that query parameters are not carried to the service.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).

## MEM-P05-B6-F3 — the operator token is passed on curl's command line

- Context: found in round MEM-P05-B6-R1, digest 88808f5ad06857e86e417fe9593fd6b72185bdc01b42ad52871054c77e81b0a1
  (tree 7388d1c2); scripts/memory-integration.sh.
- Classification: optional suggestion; secret hygiene (low).
- Evidence:
  - scripts/memory-integration.sh:82-83 passes `-H "authorization: Bearer $operator"`. While the call runs, the
    operator token is in curl's argv, which any local user can read with `ps`.
  - The token is never printed. It belongs to a throwaway loopback service whose database is dropped on exit, so
    the exposure is brief and the token is of little value.
  - The script's other secrets stay in variables and the environment.
- Suggestion: pass the header on stdin, which the request leaves free because `-d` is inline:
  `printf 'authorization: Bearer %s\n' "$operator" | curl … -H @- …` (curl ≥ 7.55).
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).
