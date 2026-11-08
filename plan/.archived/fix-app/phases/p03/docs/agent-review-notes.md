# FIXAPP-P03 agent review notes

## FIXAPP-P03-G1-R1 — architecture gate G1, decision revision r1

### Identity

- Date 2026-11-01. Workflow `implement-opus-reviewed-2_2`.
- Records:
  - task: `TASK.md` ("Architecture gate G1 — decision revision r1");
  - review record: `REVIEW.md`;
  - findings: `agent-review-findings.md`.
- Batch: phase p03 (plan 03, batches 1–4). Stage: architecture gate G1. Reviewer B: Opus 5.5 /
  xhigh (independent reviewer subagent).
- Phase p03, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p03`, branch `fix-app/p03`.
- Phase start and review base: `48626a1`. HEAD: `48626a1240f032ca3b99372bd3f5a05eb08c0c66`.
- Artifact digest `c325eea2f3e71d55bbb4f3c5acdfb0df01515171e063db16d2366218d4aa3462`
  (exclusion `plan/fix-app/phases/p03/docs`). Confirmed with `snapshot.sh` at start and again
  before returning; the working tree holds only the phase records.
- Gated decisions: D1–D4, revision r1. TASK.md sha256
  `61469d8e708e39f1d467aac611f99570f3ce867f94ef56a2cdd8119ddfc6e0a7`, confirmed at start and
  end. D5–D7 were read for context but are not gated.
- Coordination: `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md`, revision 4.
  - Byte-identical to the immutable copy `logs/coordination-r4.md` at start and at end (`diff -q`).
  - Applicable: fix-app-I001 r1, I002 r1, I003 r1, I004 r1, C001 r1, C002 r1, C005 r1.
  - Freshness: no revision change during the review, so no changed applicable requirement.
- Original requirements: `plan/fix-app/03-workflow-database-guards.md`, `plan/fix-app/README.md`.
  Instructions: `AGENTS.md`, `src/AGENTS.md`, `src/application/AGENTS.md`,
  `src/adapters/persistence/AGENTS.md`, `src/adapters/http/AGENTS.md`.

### Coverage and evidence

All evidence is code inspection at HEAD, plus one Postgres probe. No cargo command was run; none
is needed for a decision gate, and fix-app-I004 forbids the full suite.

**D1 — identification.** Re-counted the RAISEs in init-schema functions whose names contain
`workflow`:

- 130 in total: 115 × 23514, 11 × P0001, 1 × 54000, and 3 × 23503 in
  `validate_workflow_job_association`, which is excluded. That leaves 127, matching TASK.md's ~127.
- The four pre-existing constraint names match TASK.md.
- Probe (`logs/g1-r1-probe-raise-constraint.log`, throwaway database, created and dropped): a
  deferred trigger's `RAISE ... USING CONSTRAINT='workflow_guard:x'` reaches the client at COMMIT
  for both 23514 and default P0001.
- No production code string-matches error text.
- `decisions.rs::closed_at_commit` inspects the raw `sqlx::Error`, so the remapping does not
  affect it.

**D1 — mapping and consumers.**

- `From<sqlx::Error>` (`persistence/mod.rs:105-117`).
- Exhaustive `AppError` matches: `app_error_impl.rs`, `inbound_event_worker.rs:658`.
- Wildcard consumers: `actions/failure.rs:99-112`, `message_runtime.rs:540-580`,
  `harness/rig/workflow.rs:402-416`, `message_admission.rs:444-461`.
- `persistence_bound.rs` classifies a COMMIT refusal as `Failed`, not `Ambiguous`, so guard errors
  do propagate (`:217-226`).

**D1 — worker and failure transaction.**

- Read `worker.rs` in full, plus `CANDIDATES_SQL` and the scope for each `PollWork` kind
  (`polling.rs:62-160`).
- Lock order and effects: `maintenance.rs`, `controls.rs:173-230`, `recovery.rs:60-270`,
  `lease.rs:66-420`, `completion.rs:23-130`.
- `check_workflow_fenced_retirement`, `check_workflow_fenced_completion`,
  `check_workflow_control_retry`, `workflow_control_retry_safe`, `..._reopen_safe`.
- Message receipt consumption swallows per-action errors with backoff
  (`message_receipts.rs:27-62`), so receipts never reach the wrapper. That part is fine.

**D1 — challenged assumptions.**

- Deferral rules. They miss the retirement race and do not check that the scope execution is still
  current (F1). Wait deadlines are not deferred (F5).
- Coverage of handler-internal guards and the C001 claim (F2).
- Children (F3).
- The "never retry-safe" claim (F4).
- The metric owner: `metrics.rs` gauges versus counters. In-process counters have precedent in
  `schedule_admission.rs` and `message_admission.rs`, so no finding.

**D2 — transition table.**

- Checked every real writer of `workflow_runs.state`:
  - 26 Rust UPDATEs in 17 files;
  - SQL `workflow_action_conflict_park_pending` (`init_schema.sql:2206`);
  - the test fixture SQL.
- Every non-test writer fits the r1 table, including queued→waiting, queued→succeeded/failed,
  waiting→waiting re-park, reconciliation reopen from waiting only (`settlement.rs:181-221`), and
  cancel from live states only.
- `waiting → succeeded` never happens directly: every resume path sets `running` before
  `batch_commit`.

**D2 — trigger order and retry edge.**

- Listed every trigger on `workflow_runs`, including multi-line ones. All BEFORE triggers start
  with `workflow_`, so `guard_…` fires first.
- `workflow_control_retry_safe` does not test the job's status, so evaluating it in a BEFORE
  trigger after `retry_on` has reopened the job is consistent with the deferred
  `check_workflow_control_retry`.

**D2 — `terminal_execution_id`.**

- Inventory checked against the code: every writer of `failed` or `succeeded` is listed. The
  `wait_commit` `expire`/`fail` gap and the `child_waits.rs:264` follow-up are correctly
  identified.
- The follow-up must go: with an immediate CHECK, the `fail` UPDATE would otherwise violate it.
- The retry-eligibility claim holds for the `wait_commit` paths (their job is not `failed`;
  TASK.md says "completed", but for timer/event steps it is `pending`, with the same conclusion).
- `workflow_run_terminal_execution_fk` is already composite, scoped by run.

**D3 — root budgets.**

- FK and trigger order verified:
  - root budget → run FK is deferrable;
  - run_budgets.root → root_budgets FK is immediate;
  - usage → root_budgets FK is immediate;
  - `preserve_workflow_budget_usage`'s INSERT branch has no dependence on trigger depth.
- Compiled `root_budget` equals `definition.limits.root_budget` with `limit()` and token policy
  (`compile.rs:709-725`). The SQL reads the root version's compiled `root_budget`, which
  `PublishedBundle::compiled()` also returns.
- Every provenance reference is covered by D3: `init_schema.sql:477`, `:2584`, `:5514-5521`,
  `token_budget.rs:94`, the validate SQL and the schema tests.
- `admission_write::insert_run` is the only non-test insert site for runs. Reruns and schedules go
  through `create`.
- No defect found.

**D4 — lease renewal.** Renewal statement at `lease.rs:85` checked. The decision is sound;
verification is vacuous (F6.5); optional completeness item in F8.

**Instructions checked.** Persistence `AGENTS.md`:

- "Encode queue state machines as constraints" plus the equivalence matrix: D2 complies.
- "Fence leases": D4.
- "Audited transitions carry typed reason": the guard name comes from a structured field.

Root `AGENTS.md` competing-claimant rule: F1. Application `AGENTS.md` poison-batch rule: F6.6.

**Reviewed lightly.**

- D5–D7: not gated, read only.
- The ~44 test fixtures that update run state: sampled, not individually checked (F6.3 asks for an
  inventory).
- Guard functions outside the `workflow` naming rule: listed, not analysed one by one (F7).

### Outcome

**Changes required** for decision revision r1. No PASS.

- Actionable:
  - FIXAPP-P03-G1-F1 (High): the failure transaction misclassifies a lost-lease race that a
    competing retirement settles first, and does not check the scope execution is current.
  - FIXAPP-P03-G1-F2 (High): guard errors inside handler-owned transactions never reach the
    wrapper; the C001 response is incorrect; the handler-contract decision is missing.
  - FIXAPP-P03-G1-F3 (Medium): live children are not cascaded; the deadline-equivalence rationale
    is false.
  - FIXAPP-P03-G1-F4 (Low): "a guard failure is never retry-safe" is false for pending-job guard
    failures.
  - FIXAPP-P03-G1-F5 (Low): wait-deadline time races are not deferred.
  - FIXAPP-P03-G1-F6 (Low): verification plan gaps.
- Optional: FIXAPP-P03-G1-F7, FIXAPP-P03-G1-F8.
- D2, D3 and D4 have no acceptance-blocking defect of their own. F4 touches D2's retry edge only
  through D1's claim, and F6 items 2–5 are verification-plan corrections. The recheck must cover
  D1 in full. For D2–D4 it can be limited to the F4 and F6 deltas, unless the corrections change
  them.
- Unresolved questions:
  - F2's choice between options (a) and (b) is the author's to make and record.
  - F3: cascade or justify.

### Process and instruction feedback

- Batch and gate size were appropriate. One gate for D1–D4 was reviewable in one pass. D1 alone
  carries the concurrency and contract risk, and gating it was justified: F1 and F2 would be
  costly to unwind after the port, wrapper and handler mappings exist.
- TASK.md was demonstrably useful:
  - accurate writer inventories (D2), RAISE counts (D1) and provenance references (D3);
  - a clear statement of invariants, which made F1, F3 and F4 checkable against code.
- What TASK.md was missing:
  - the actors competing for the run lock after a failed commit (F1);
  - the error's consumers after `From<sqlx::Error>` (F2).
- Candidate instructions are recorded in F1, F2 and F6 under "Instruction lesson":
  - competing owners for compensating transactions;
  - trace consumers of a reclassified error;
  - grep tests for assertions on the changed error fields.
  Each is supported by one case in this phase only, so treat them as candidates, not established
  patterns.
- All findings are defects in gated decisions. There is no implementation yet.

---

## Working notes saved mid-review (2026-11-01, kept for continuity; superseded by the entry above)

Candidate findings identified by inspection before final write-up:

1. Lease-retirement race in D1's deferral (became F1).
2. Wrong C001 response for handler-internal guards (became F2).
3. Children not cascaded (became F3).
4. "Never retry-safe" claim (became F4).
5. Wait-deadline races (became F5).
6. Verification gaps (became F6).

## Follow-up to FIXAPP-P03-G1-R1 (2026-11-01, during recheck R2)

R1 missed one gap in r1 step 2. A guard trip at the `RunExpiry` site was deferred whenever the scope
job was `processing` with an expired lease. After the deadline nothing else settles such a job, so
the deferral is unbounded. The gap persists in r2's rule 5 and is now logged as FIXAPP-P03-G1-F9
(together with a related rule-4 issue r2 introduced). R1's outcome (changes required) is unchanged;
the miss is recorded for later evaluation. Cause: R1 checked the deferral rules against the
competing owners of the `Step` and `LeaseExpiry` sites, but not against the `RunExpiry` site's own
job states.

---

## FIXAPP-P03-G1-R2 — correction recheck, decision revision r2

### Identity

- Date 2026-11-01. Workflow `implement-opus-reviewed-2_2`.
- Records: `TASK.md` (section "G1 decision revision r2 — responses to FIXAPP-P03-G1-R1"; the r1
  text above it is preserved and was spot-checked unchanged), `REVIEW.md`,
  `agent-review-findings.md`.
- Batch: phase p03. Stage: architecture gate G1 correction recheck. Reviewer B: Opus 5.5 / xhigh,
  the same reviewer instance as R1, resumed.
- Phase p03, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p03`, branch `fix-app/p03`.
  Review base `48626a1`, HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`.
- Decision under review: revision r2, TASK.md sha256
  `bb318b3df60479a1d38f44abd9dd05d69a49918c2f585c1859c790ada448f9a7`. Confirmed at start and end.
- Working-tree digest at recheck: `4c4d8ed41059c06725a679f1c02cdff8d971865c3b251b23494e801c93adc5ac`
  (exclusion `plan/fix-app/phases/p03/docs`).
  - It differs from R1's `c325eea2…` only by A's announced, ungated batch-4 edit to
    `src/domain/entities/transport/delivery.rs` (D5).
  - The gate reviews decision text, so that file is outside this recheck. A will freeze a new
    digest for batch review B1.
  - At the end of the recheck the digest was `a99f7816d3b9dd12ad26fbd68113258ce5b59ea9aaf0a09304ad8166b8778afd`.
    A's further ungated D6 edits produced it; `git diff --stat` shows:
    - `init_schema.sql`: `token_refusal` CHECK and `refuse_expired_workflow_tokens` only;
    - `harness_token_refusal_tests.rs`;
    - `domain/workflow/budget.rs`;
    - `delivery.rs`.
    None touches a D1–D4 statement. Incidentally, D6 no longer refuses the root on a cancelled
    token wait, which removes the D6 interaction noted under F3's cascade. Batch review B1 will
    verify it.
- Gated decisions: D1–D4. Coordination is revision 4, byte-identical to `logs/coordination-r4.md`
  at start and end, with the same applicable notices as R1 and no changed requirement.

### Coverage and evidence

Recheck by finding ID against r2's text and the code it relies on. Inspection only; no checks run.

- **F1, retirement race.** Verified that r2's confirmation re-run handles the retirement-first
  interleaving:
  - `complete_on` locks the run, then `lock_job` refuses a settled fence (`completion.rs:78-95`).
  - Rule 5's exact-fence deferral, the rule-2 currency check and the READ COMMITTED reasoning are
    correct.
- **F2, handler-internal guards.** Verified the handler mappings r2 changes and the guard lists.
  - `StepFailure::new` accepts a bounded diagnostic (`outcome.rs:99-117`).
  - Failure codes are length-checked only (`init_schema.sql:4196`).
  - Receipt consumption absorbs per-action errors (`message_receipts.rs:27-62`, `:213-233`), so
    one entry in the guard list is inaccurate (F11).
- **F3, cascade.** The cascade's lock order matches `cancel_tree_on` (`controls.rs:173-197`).
- **F4, retry safety.** The exclusion is inherited by `check_workflow_control_retry` and by D2's
  trigger.
- **F5, wait deadlines.** Confirmation covers the race; the residual is acceptable.
- **F6–F8.** Adopted as recorded.
- **New checks against the r2 deferral rules:**
  - Claim and renewal leases are capped at the run deadline (`lease_claim.rs:168`, `lease.rs:85`).
  - The `CANDIDATES_SQL` CASE order puts `deadline` before `expired`.
  - `finish_job` requires a live lease unless the cause is expiry (`recovery.rs:213`).
  - The resume and timeout paths are in `wait_commit.rs:5-191`.
  These lead to F9.
- **New check on the confirmation wording:** read `worker.rs:177-202` against r2's confirmation
  rule. This leads to F10.
- Not rechecked: D2, D3 and D4 beyond the r2 deltas, which are unchanged and so reuse R1's
  evidence.

### Outcome

**Changes required** for revision r2. No PASS yet.

- Resolved at decision level, with implementation and tests to be verified in B1:
  - F1 fixed;
  - F2 fixed, option (b);
  - F3 fixed;
  - F4 fixed;
  - F5 fixed, with one documented residual;
  - F6 fixed (plan);
  - F7 declined with reason, closed;
  - F8 fixed, closed.
- New actionable findings:
  - FIXAPP-P03-G1-F9 (Medium): rules 4 and 5 defer to the owner that just failed. At `RunExpiry`
    the deferral is unbounded past the deadline (also present in r1; missed in R1). At a `Wait`
    site's timeout path it spins until the deadline.
  - FIXAPP-P03-G1-F10 (Low): the confirmation contract at the `complete_io` site needs stating,
    plus a site test.
- Optional: FIXAPP-P03-G1-F11, an accuracy fix for the receipt-consumption guard list.
- The next recheck can be limited to the F9 and F10 deltas, plus the F11 wording if adopted.

### Process and instruction feedback

- The correction round was efficient. Each response is traceable to its finding ID. Confirmation
  by re-execution is a clean, uniform answer to time races.
- The new defect (F9) came from deferral rules that name an owner without checking that the owner
  differs from the failing operation. The candidate instruction is recorded in F9.
- This reviewer missed the rule-5 instance in R1 (follow-up above). That is evidence that
  per-site enumeration of job states should be part of reviewing a compensating transaction, not
  only competing actors.

---

## FIXAPP-P03-G1-R3 — correction recheck, decision revision r3: **PASS**

### Identity

- Date 2026-11-01. Workflow `implement-opus-reviewed-2_2`.
- Records: `TASK.md` (section "G1 decision revision r3 — responses to FIXAPP-P03-G1-R2"; r1 and
  r2 text preserved, spot-checked), `REVIEW.md`, `agent-review-findings.md`.
- Stage: architecture gate G1 correction recheck. Reviewer B: Opus 5.5 / xhigh, the same instance
  as R1 and R2, resumed.
- Phase p03, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p03`, branch `fix-app/p03`.
  Review base `48626a1`, HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`.
- Decision under review: revision r3, TASK.md sha256
  `bb5e48e479581a889125faac1b3ae2677a07874db4affe31b669b412f4cfd864`. Confirmed at start and end.
- Working-tree digest `a99f7816d3b9dd12ad26fbd68113258ce5b59ea9aaf0a09304ad8166b8778afd`
  (exclusion `plan/fix-app/phases/p03/docs`), confirmed at start and end. It is the same as at the
  end of R2: only A's ungated D5/D6 edits, none touching D1–D4.
- Coordination: revision 4, byte-identical to `logs/coordination-r4.md` at start and end. The
  applicable notices are I001–I004 r1, C001 r1, C002 r1 and C005 r1; there is no changed
  requirement.

### Coverage and evidence

The recheck is limited to the F9, F10 and F11 deltas, as agreed. All other evidence is reused from
R1 (D2–D4 full review, D1 r1) and R2 (D1 r2, D2/D4 r2 deltas); the decision text it rests on is
unchanged. Inspection only; no checks run.

- **F9.**
  - Rule 4 is gone.
  - Rule 5 at `RunExpiry` admits any job state. This matches `retire_jobs`, which handles
    `processing` (live or expired) and `pending` jobs (`maintenance.rs:58-79`), and `expire_on`'s
    own currency check (`:22-27`).
  - Rule 3 is not circular. `resume_on` runs `expire_on` itself (`wait_commit.rs:8`, `:48`), but
    the next poll classifies the work as `deadline` (`polling.rs` `CANDIDATES_SQL`) and reaches
    `expire_run` at site `RunExpiry`.
  - Every remaining deferral in the r3 final list has a distinct settling owner, or leaves no
    candidate to loop on. Details are in F9's resolution entry.
- **F10.**
  - `execute_io` (`worker.rs:177-202`) now confirms and acts internally, with the claim fence, and
    never re-raises. So the wrapper's candidate re-run applies only to errors raised before any
    handler ran (`claim_io` or earlier).
  - Test specified: one claim, one handler execution, retirement `guard`, two empty polls.
- **F11.** The list correction was adopted. In the F11 resolution I noted that
  `workflow_message_reopen_guard` also fires at worker level through `continue_delivery`; r2's
  "site decides" already covers it.

### Outcome

**PASS for architecture gate G1, decision revision r3 (decisions D1–D4).**

- Every G1 finding has a disposition: F1–F6, F9 and F10 fixed at decision level; F7 declined with
  reason (closed); F8 fixed (closed); F11 adopted (closed). No acceptance-blocking defect remains
  in the decision text.
- Coverage relied on:
  - R1's full read of D1–D4 against the code: writers of run state, trigger order,
    `terminal_execution_id` writers, budget FK and trigger ordering, the error-consumer trace, the
    worker and polling paths, the retirement and expiry lock order;
  - the Postgres probe in `logs/g1-r1-probe-raise-constraint.log`;
  - R2's and R3's delta checks.
- This is a decision pass, not an implementation pass. Batch review B1 must verify the following
  (D1 r2's "F6 verification additions", D1 r2 confirmation and deferral, and r3):
  - **Tests:**
    - competing-claimant tests (i) retirement-first, (ii) guard failure vs cancel, and (iii)
      pending-job guard failure not retry-safe;
    - forced-guard tests at the expiry, decision-timeout and IO-completion sites, plus the pure
      step, each with two empty polls;
    - the D2 matrix asserting the guard constraint name and including an accepted
      `failed → running`;
    - D4's predicate tested on the UPDATE itself;
    - the F6 test and fixture updates (empty-constraint pins, the state-witness attack disposition,
      the raw fixture inventory);
    - the handler mapping units.
  - **Behaviour:**
    - the cascade cancels only the children's subtrees;
    - the counter and log on the `execute_io` path;
    - the `workflow_control_retry_safe` exclusion;
    - p02's C001 filters still pass.

### Process and instruction feedback

- Three rounds were needed. R1 found the main issues (F1–F6). R2 found one issue the r2 correction
  introduced (rule 4) and one I had missed in R1 (the `RunExpiry` instance), plus a contract
  precision gap (F10). R3 closed them. The rounds converged with new evidence each time, so they
  were not a stalled loop.
- Candidate instructions, already recorded in F1, F2, F6 and F9:
  - for compensating transactions, enumerate competing actors per site, and for every deferral
    name a distinct settling owner and the candidate that reaches it;
  - trace the consumers of a reclassified error;
  - grep tests for assertions on changed error fields.
  Supported by this phase only.


## FIXAPP-P03-G1-R4 — fresh architecture review of consolidated r4: PASS

### Identity

- Date: 2026-11-04. Workflow: `implement-astra-reviewed-2-2`; phase p03, whole B1 architecture gate G1, D1–D4.
- Reviewer: `/root/p03_reviewer`, requested `gpt-6-astra` / `high`, independent context. Actual runtime model/effort metadata is not exposed; no substitution inferred. New reviewer, not the historical Opus reviewer.
- Records: [TASK](TASK.md), [REVIEW](REVIEW.md), [findings](agent-review-findings.md).
- Worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p03`, branch `fix-app/p03`.
  Phase start/review base/HEAD: `48626a1240f032ca3b99372bd3f5a05eb08c0c66`.
- Decision: [G1 r4](logs/G1-r4-decision.md), SHA256
  `5753375da4d526406487a78080c9deecdbd090b8dc16afa947ab7e398bbf2e25`.
- Preserved implementation snapshot: `7b5299ca08b6dec63a7c84ef37d266e2a1c98103a838681ee0a1e6c721c9c979`, sole exclusion `plan/fix-app/phases/p03/docs`. Confirmed before inspection and after inspection using the installed snapshot script via the retained macOS realpath adapter. [Final identity evidence](logs/G1-r4-review-verification.log).
- Coordination authority `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md`, revision 6; pinned [copy](logs/coordination-r6.md), SHA256 `d3c5b83ecf08b42314e1194bb371ebee030216d8c875cc7f76cf5b0f20322371`. Authority and pinned copy byte-identical at entry and exit. Applicable I001–I004/r1 and C001/C002/C005/C007/r1; C006/r1 is a proposed output, not an accepted producer contract. r4 acknowledges those requirements. C007 remains an integration obligation.

### Coverage and evidence

Original phase 03 plan and README supplied requirements; root/src/application/persistence/http/infra guides supplied constraints. Historical r1/r2/r3 decision text in [paused TASK](logs/TASK-paused-20261101.md), prior REVIEW entries, review notes, and F1–F11 dispositions were inspected independently. r4 preserves their effective decisions and the explicitly bounded handler/receipt exceptions; historical passes were not treated as fresh acceptance.

Graft orientation, ranked discovery, skeletons and caller traces preceded source reasoning. The worktree has no local map; graft reported fallback to the primary checkout graph. Therefore graph spans/edges were discovery hints, not proof of the preserved source: the pertinent local definitions were read directly. New `guard_failure.rs` is unindexed and was inspected directly. Ambiguous `renew_io` call edges were not assumed exhaustive.

- **D1:** Inspected SQL error conversion (`persistence/mod.rs:105`), action mappings (`actions/failure.rs:95`), worker candidate confirmation and IO completion (`worker.rs:114–315`), polling candidate priority (`polling.rs:98`), wait timeout/resumption (`wait_commit.rs:5`), deadline maintenance (`maintenance.rs:5`), compensation (`guard_failure.rs`), and cancellation traversal (`controls.rs:163`). Traced action failure consumers, completion, retry, and cancellation callers. The decision distinguishes guard identification from policy at each consumer. Its repeated-error confirmation, exact live-fence check, execution currency, deadline owner, and RunExpiry exception address historical F1/F5/F9/F10. Child subtree cancellation and durable guard-event exclusion address F3/F4. No new acceptance-blocking decision defect found. Compensation-transaction errors, including guard errors, must retain r4's no-handler-reexecution and observability requirements in B1; the preserved partial code is not accepted by this gate.
- **D2:** Inspected `control_retry.rs:3–43`, deferred SQL `check_workflow_control_retry` (schema:121), `workflow_control_retry_safe` (schema:2833), run BEFORE-trigger declarations, and relevant wait/maintenance writers. Reopening the job before the run is consistent with the immediate eligibility guard plus deferred receipt check. The new `guard_` trigger sorts before existing `workflow_` triggers. The explicit eligible-retry exception is necessary in the domain/SQL matrix; table CHECKs are deliberately outside typed guard mapping. Every row fixture and every rewritten terminal-state writer still requires final batch review, rather than inference from this gate.
- **D3:** Inspected `admission_write::insert_run` (`:94–125`) and its callers, existing SQL budget-link trigger (`schema:675`), budget usage INSERT guard (`schema:1186`), root-budget deferred owner FK (`schema:10827`) and immediate link FK (`schema:10843`). The proposed budget/zero-usage insertion before the root run is consistent with those constraints; child linking continues to use its parent's root. Removing compiler-revision/provenance branching and sharing token encoding meet the original no-compatibility requirement. Raw fixtures, rollback behavior and compiled-limit equality remain B1 obligations.
- **D4:** Inspected `lease.rs:68–118` and lease API surface. Complete identity predicates on the renewal UPDATE preserve existing ownership and deadline semantics. The proposed direct UPDATE test avoids a vacuous test that only exercises preceding `lock_fence`.
- **Coordination/verification:** C001 continuation limit and handler backstop policy are carried forward with its required focused filters. C002 fixture ownership atomicity and C005 FK preservation are explicit. C007 needs combined integration review. C006 must await accepted/integrated p03. D5/D6 are ungated and not freshly accepted here. Competing-claimant, forced-site, two-empty-poll, transition-matrix, root-budget, and direct renewal tests are specified; none was executed by this decision review.

Checks actually run: branch/HEAD checks; read-only graph/source/SQL inspection; installed snapshot and SHA256 checks before/after. No cargo, database migration, DB fixture, or full-suite command was run. No test pass is claimed. Historical test evidence is contextual only. I004 constrains later checks to affected scope; the source remains incomplete and frozen only for this gate.

### Outcome

**PASS for G1 decision revision r4, SHA256 `5753375da4d526406487a78080c9deecdbd090b8dc16afa947ab7e398bbf2e25`, D1–D4.** No new concrete decision findings. F1–F11 keep their historical dispositions; their implementation obligations remain open until B1 evidence is reviewed. This is neither implementation acceptance nor integration acceptance.

### Process and instruction feedback

The consolidated decision and immutable historical records made one cohesive fresh gate practical. Its scope is appropriate because D1 ownership/compensation and D2/D3 schema contracts are expensive to unwind. The fallback graph is stale relative to preserved work, so local source verification was necessary and is explicitly recorded. No additional instruction change is supported beyond the historical candidates. Keep the existing B1 checklist; do not convert this decision pass into acceptance of the partially implemented tree.


## FIXAPP-P03-G1-R5 — mac02 architecture continuity validation: PASS

### Identity

- Date: 2026-11-04; workflow `implement-astra-reviewed-2-2`; p03 whole B1, G1 decision-continuity stage. Reviewer `/root/p03_reviewer`, requested Astra/high with independent context; actual runtime settings/compaction threshold not independently exposed. Root/A Sol 6.1/xhigh confirmed by user per current coordination.
- Worktree `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p03`, branch `fix-app/p03`; phase start/review base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`.
- Frozen source digest `05defcdaaf223bb332f5123dcddda09b740a1d64b96501f9108892e25a1220e9`, sole exclusion `plan/fix-app/phases/p03/docs`. Entry and exit checks match; retained restored source remains unaccepted.
- Immutable [decision r4](logs/G1-r4-decision.md), SHA256 `5753375da4d526406487a78080c9deecdbd090b8dc16afa947ab7e398bbf2e25`. The decision's old adoption digest and machine/role identities remain historical, not current source identities.
- Authority `/Users/mac02/dev/openbot-recovery/plan/fix-app/COORDINATION.md`, r13, byte-identical to [pinned r13](logs/COORDINATION-r13-resumed.md), SHA256 `d685871baf915c1f3b5460686226b50b540ce0eb616c4d2fb7d989fc981ec4e5`. Freshness rechecked on exit. Applicable I001/I003/I004 r1, I002 r2, C001/C002/C005/C007/C009 r1; I005 r1 not applicable to p03; C006 r1 proposed output only. [TASK](TASK.md) and [HANDOFF](HANDOFF.md) current resume sections acknowledge these revisions.
- [Identity evidence](logs/G1-r5-review-verification.log), [REVIEW](REVIEW.md), [findings](agent-review-findings.md).

### Coverage and evidence

Read original phase 03 requirements and plan README, root/src/application/persistence and relevant HTTP/infra guides, immutable r4, historical G1-R4 review and F9–F11 resolution detail, and current coordination and phase resume records. This assignment validates continuity of the unchanged decision, not an intermediate implementation batch. Graft map supplied repository orientation; no current source implementation was reviewed or accepted in this round.

- D1 retains identifiable named guards, typed conversion, terminal worker handling with metrics, and connection-error retry. Challenged the distinction between identification and terminal policy: handler transaction guards remain bounded by attempt budgets and receipt consumption by backoff/stall; those are explicit historical exceptions, not newly broadened ones. The second confirmation and exact IO fence must not repeat a handler after execution. A failed compensating transaction remains observable/retryable; it must not be mistaken for a successfully terminal outcome. Historical F9's own-timeout deferral trap remains removed, and F10's completion-only confirmation remains explicit. No new requirement or restored-environment change contradicts these decisions.
- D2 retains the complete transition table, eligible failed-to-running exception with immediate eligibility plus deferred retry receipt, trigger ordering, terminal-id shape and non-vacuous SQL/Rust equivalence testing. C001's running-to-waiting effect park is admitted. C005's run/binding and deferred history keys remain protected; the gate does not authorize fixture bypasses or weaker adversarial assertions.
- D3 retains transactional Rust-owned root budget/usage insertion and child linking, with deferred root ownership FK and immediate link presence checks. Removal of bundle/compiler-revision/provenance compatibility remains required. Local fixture restoration changes URLs and identity only; it supplies no new proof of budget equality or rollback behavior.
- D4 retains the complete direct renewal UPDATE fence and direct-predicate test, independent of preceding lock_fence. D5/D6 remain ungated B1 obligations under the original smaller-mismatch requirements.
- Coordination r13 changes host/authority, resources, roles and resume authorization. It does not change the technical gate. C007 continues to require preserving both WorkflowGuard and Forbidden at integration; C006 remains unpublished producer semantics until acceptance/integration. I005's retention selection targets p06/p08/integration and introduces no p03 storage redesign. Later main changes are still integration work, not permission to rebase this frozen artifact.
- The plan's full-suite gate is superseded by explicit I004/r1 and r13 targeted-only authorization. Competing-claimant tests, forced-guard sites/two empty polls, one IO handler invocation, transition matrix and retry eligibility, budget equality/child link, direct renewal predicates, affected UI/persistence and C001 filters remain necessary B1 evidence. The 31 historical affected failures remain unresolved; neither their cause nor their resolution is inferred from the architecture gate.

Checks run: branch/HEAD, installed snapshot through unchanged macOS wrapper, decision SHA256, coordination pin equality and freshness. No cargo, DB test, migration, or full-suite command run; no test pass claimed. Historical code review and tests are retained evidence, not fresh implementation verification.

### Outcome

**PASS for unchanged G1 decision revision r4 D1–D4 under current original requirements and coordination r13**, SHA256 `5753375da4d526406487a78080c9deecdbd090b8dc16afa947ab7e398bbf2e25`. No new concrete findings. Historical F1–F11 dispositions remain unchanged and their implementation obligations stay open until whole-B1 review. This does not accept source digest `05defcda…20e9`, resolve historical failures, or accept integration.

### Process and instruction feedback

The bounded continuity check is appropriate after restoring hosts: source and decision identities plus explicit coordination deltas prevent treating setup or an old pass as implementation acceptance. No redesign or intermediate code review was needed. No new instruction change is supported by this round.


## FIXAPP-P03-B1-R1-MAC02 — whole B1 review: CHANGES REQUIRED

### Identity

- Date 2026-11-04, workflow `implement-astra-reviewed-2-2`; p03 whole B1 implementation review. Reviewer `/root/p03_reviewer`, Astra/high requested with independent context; actual runtime model/effort metadata unavailable. Root/A `/root`, Sol 6.1/xhigh confirmed by user in current coordination. [TASK](TASK.md), [REVIEW](REVIEW.md), [findings](agent-review-findings.md).
- Worktree `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p03`, branch `fix-app/p03`; phase start/review base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`. Reviewed the full preserved and resumed dirty tree, including untracked guard port/compensation/tests; there are no phase commits beyond the review base.
- Frozen digest `16ab2bf541275753b6b0c2dfdef77dee8359af0e65ba6723e471f646beddf31b`, sole exclusion `plan/fix-app/phases/p03/docs`; installed snapshot through unchanged `logs/snapshot-mac02.sh` matches entry and exit. [Author manifest](logs/mac02-b1-review-snapshot.txt), [review identity](logs/B1-R1-MAC02-review-identity.txt).
- G1 decision r4 SHA256 `5753375da4d526406487a78080c9deecdbd090b8dc16afa947ab7e398bbf2e25`; G1-R5 continuity pass remains valid. Coordination authority `/Users/mac02/dev/openbot-recovery/plan/fix-app/COORDINATION.md` r13 matches [immutable pin](logs/COORDINATION-r13-resumed.md), SHA256 `d685871baf915c1f3b5460686226b50b540ce0eb616c4d2fb7d989fc981ec4e5`, rechecked on return. I001/I003/I004 r1, I002 r2, C001/C002/C005/C007/C009 r1 apply; I005 not applicable, C006 proposed output only.

### Coverage and evidence

Original p03 plan and README, applicable guides and normative r4 supplied requirements. Whole-batch review covered production changes, schema, callers and changed tests; it was not limited to resumed corrections. Graft discovery/skeleton/callers/grep preceded source inspection. Ambiguous caller edges for `insert_run`, `fail_run_on_guard` and `is_terminal` were not treated as complete; indexed grep and actual use sites supplied the necessary trace. The graph refreshed its index during read discovery; source identity remained unchanged.

- **Typed guards and compensation:** inspected error name validation/conversion and timeout precedence; HTTP and inbound classification; action/agent handler mappings and callers; worker confirmation/IO handling/metrics; guard port and full compensation transaction; run-first locking, execution/job currency, live claim fence, deadline and lease deferrals; retirement guard shape, durable event and retry exclusion; shared child subtree cancellation and action uncertainty. Inspected forced pure/IO/expiry/decision-timeout tests, transient completion, ordinary 08006 handling, pending-job retry exclusion, actual cancellation race and retirement-first stale owner coverage. Found F1: compensation itself can return a guard into the generic candidate retry wrapper, causing post-handler candidate re-entry and swallowing the error as Deferred. No architecture change is needed; r4 already forbids that path.
- **State transitions and terminal execution:** read SQL transition function and trigger order, Rust table, complete 36-pair DB matrix, eligible/ineligible retry tests, shape tests and exact guard pins. Inventoried production run-state writers through [writer inventory](logs/B1-R1-run-writer-inventory.txt); wait failure/expiry now write terminal execution atomically and redundant child follow-up is removed. Retry fixtures use actual safe failed IO/control retry; wakeup tests retain immutable terminal facts and exact terminal execution. State-witness attacks preserve rollback/native catalog checks: forbidden terminal-to-waiting attacks now identify the earlier guard, while reachable human waiting/reconciliation paths still exercise the original witness guard. Retention reopen tests preserve both immediate refusal and the deferred recheck when the horizon changes after reopen.
- **Budgets/admission:** inspected root budget/zero-usage insertion before run inside shared admission transaction, child `insert_run` caller, immediate root link and deferred owner dependency, SQL bundle/provenance removal, budget debit/reconciliation/token queries, canonical token policy shared with compiler, root/child equality and raw-root refusal tests. Raw root fixtures seed budgets within the same transaction/savepoint; native guard assertion adaptations name the intended guard instead of accepting generic errors. No backfill or legacy decoding was added.
- **Lease and smaller mismatches:** read direct renewal UPDATE full execution fence and stale-component tests plus existing run-first lock/live-window path. Delivery terminal predicate removal has no remaining delivery callers; other `is_terminal` APIs remain separate. Child token cancellation no longer refuses the common root, while expiry/refusal logic remains; inspected cancellation-versus-approval and parent progression regressions. The run-list query-plan fixture supplies required terminal IDs to temporary table clones.
- **All fixture/adaptor changes:** audited diff-wide structural/terminal-id updates, worker monitoring additions, raw budget inserts, native SQLSTATE/message/constraint assertions and rollback controls. The prior-bound fixture now selects the intended attempt-1 entry rather than assuming UUID snapshot order. No assertions were weakened to an arbitrary error. Repeated mechanical fixture additions received diff-level review; security/concurrency/compensation, admission, state-witness and retry paths received deeper source inspection. C001 continuation/delivery/notification/schedule behavior and C002/C005 fixture/FK requirements remain in scope. C007 and later main differences still need combined integration review; no current phase pass can accept them.

### Checks actually run or inspected

- Reviewer ran read-only graph/source/diff inspection, `git diff --check` (PASS), source snapshots and coordination/decision hash checks. No reviewer cargo command or DB mutation; author owned the active fixture checks.
- Inspected [affected union log](logs/mac02-b1-affected-union.log): **1102 passed, 0 failed, 1 ignored**, 1500 filtered, 283.88 seconds. [Command](logs/mac02-b1-affected-union-command.json) and [filter counts](logs/mac02-b1-affected-union-filter-counts.json) show all nine filters nonzero, including 13 guard tests, 1051 workflow-persistence matches, 26 UI-run matches and required C001 interfaces. Counts overlap; do not sum them as distinct tests. This resolves the historical affected failures on the reviewed artifact but does not test F1.
- Initial current-host offline lib/tests compilation passed; [log](logs/mac02-check-lib-tests-initial.log). The earlier focused correction run retained **106 pass/1 failure** in [log](logs/mac02-correction-focused.log); its prior-bound failure was corrected in current source and passes the complete affected union.
- SQLx prepare all-target log reaches successful completion (56.24 seconds), [log](logs/mac02-sqlx-prepare-all-targets.log); final author process result/digest confirmation remains pending at this review return. Clippy all-target `-D warnings` is still running, [log](logs/mac02-clippy-all-targets.log); no pass claimed. Author reports fmt PASS; final verification reconciliation remains A's task. [Live-schema static audit](logs/mac02-static-contract-audit-corrected.json) confirms no compiler revision/legacy budget values/provenance and no unnamed intended workflow RAISEs; initial overbroad-regex false positives remain documented rather than erased.
- No full suite or scripts/check.sh run under I004/r1. Source remains unaccepted; no production/commit/integration check claimed.

### Outcome

**CHANGES REQUIRED** for digest `16ab2bf541275753b6b0c2dfdef77dee8359af0e65ba6723e471f646beddf31b`: [FIXAPP-P03-B1-F1-MAC02](agent-review-findings.md#fixapp-p03-b1-f1-mac02--io-compensation-guard-error-re-enters-the-candidate-and-is-swallowed), Medium, is the complete actionable set from this round. No optional suggestions. Preserve the unchanged G1 r4 pass; correct F1 and run its focused regression/affected guard paths before one recheck. Outstanding final verification evidence must also be reconciled before acceptance.

### Process and instruction feedback

The cohesive batch was reviewable with focused source inspection and mechanical fixture diff audit. Passing core and broad tests did not cover the compensation-error origin crossing the retry wrapper, the same boundary highlighted by historical G1-F10/G1-R4. Candidate review guidance: follow errors raised by recovery/compensation back through any generic retry wrapper, and test that origin separately from the initial failing operation. This is a review-process lesson, not permission to edit skills. No new architecture gate is justified because the correction restores existing r4 semantics.


## FIXAPP-P03-B1-R2-MAC02 — correction recheck and whole-B1 artifact: PASS

### Identity

- Date 2026-11-04; workflow `implement-astra-reviewed-2-2`; phase p03, B1 correction recheck for F1 and final affected verification. Reviewer `/root/p03_reviewer`, independent Astra/high requested; actual runtime model/effort metadata unavailable. Root/A `/root` confirmed Sol 6.1/xhigh. [TASK](TASK.md), [REVIEW](REVIEW.md), [findings](agent-review-findings.md).
- Worktree `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p03`, branch `fix-app/p03`; phase start/review base/current HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`. New frozen source digest `ba772625d27ac6dfa497e3798067e0665d09ebfff4081d9b0dd02ef724cf1440`, sole exclusion `plan/fix-app/phases/p03/docs`. Entry/exit snapshot checks match; [review identity](logs/B1-R2-MAC02-review-identity.txt), [author manifest](logs/mac02-f1-final-candidate-snapshot.txt).
- Original p03 plan/README and normative G1 r4 remain unchanged (decision SHA256 `5753375da4d526406487a78080c9deecdbd090b8dc16afa947ab7e398bbf2e25`, continuity G1-R5). Coordination authority `/Users/mac02/dev/openbot-recovery/plan/fix-app/COORDINATION.md` r13 matches [pin](logs/COORDINATION-r13-resumed.md) before/after, SHA256 `d685871baf915c1f3b5460686226b50b540ce0eb616c4d2fb7d989fc981ec4e5`. I001/I003/I004 r1, I002 r2; C001/C002/C005/C007/C009 r1 apply. I005 not applicable; C006 proposed output only.

### Coverage and finding disposition

Reused unaffected whole-B1 source/schema/fixture coverage from [R1](#fixapp-p03-b1-r1-mac02--whole-b1-review-changes-required). Independently inspected the correction in `worker.rs`, its complete affected process/IO/error paths, new compensation tests, test module registration and generic polling helper; graph skeleton/callers preceded source reads. This was a correction recheck, not a redundant full review of unchanged code.

**FIXAPP-P03-B1-F1-MAC02 is fixed.** `ProcessFailure` distinguishes Candidate errors from AfterHandler errors. The worker marks every complete-confirmed/compensation error AfterHandler; both confirmation matches handle only Candidate guards. Thus failed compensation propagates its original cause without another claim/candidate visit, while claim errors still receive their original confirmation policy. Completion-only confirmation, claim fence, metrics/logs and handler safety remain intact. Test helper genericization changes only the concrete handler type accepted by the fixture and preserves the WorkflowHandler contract for existing callers.

Inspected the real [red reproduction](logs/mac02-compensation-regression-red.log): old code returns Deferred where an error is required. The corrected guard and ordinary 08006 compensation cases assert original cause, one capability visit before the claim path, one handler execution and attempt, rollback to processing, no committed guard event, and one error metric. Existing transient/permanent completion and competing-owner cases run alongside them. The [guard union](logs/mac02-compensation-and-guard-green-r2.log) passes 15/15. No new defects or optional suggestions. The correction implements r4's existing error-origin boundary; no architecture gate reopening is needed.

### Final evidence reconciled

- [R2 affected union](logs/mac02-b1-affected-union-r2.log): **1104 passed, 0 failed, 1 ignored**, 1500 filtered, 280.04 seconds. [Filter counts](logs/mac02-b1-affected-union-r2-filter-counts.json): all nine nonzero, including workflow persistence 1053, UI runs 26, guard errors 15, and C001 continuation 5/delivery 8/notification 3/schedule 2. Counts overlap; they are not separate totals. The ignore remains the existing ignored test; no all-suite claim is made.
- [SQLx prepare/all-target compilation](logs/mac02-sqlx-prepare-all-targets-r2.log): PASS (1m00s), no `.sqlx` delta; [offline all-target clippy with -D warnings](logs/mac02-clippy-all-targets-r2.log): PASS (1m08s). Author final driver exited 0; [summary](logs/mac02-f1-final-check-summary.txt) records fmt and diff-check PASS and matching snapshots after every check. Reviewer independently reran read-only `git diff --check`, branch/HEAD/digest/coordination checks and confirmed no `.sqlx` diff.
- R1 broad affected evidence is now superseded by the R2 union for the shared worker; unchanged schema/admission/transition evidence is retained, with the same phase-owned fixtures and original baseline. No schema changes in this correction. Historical red failures and the helper compile mismatch remain retained evidence rather than being erased.
- Reviewer ran no cargo/DB mutation while author checks used the isolated phase fixture. No full suite or scripts/check.sh was run, per I004. No check required for the authorized affected scope remains unavailable or pending.

### Outcome

**PASS for the whole p03 B1 artifact at digest `ba772625d27ac6dfa497e3798067e0665d09ebfff4081d9b0dd02ef724cf1440`**, using R1 whole-batch coverage plus this correction and final verification. F1 is closed; no unresolved acceptance-blocking findings remain. Root may perform its acceptance step for this exact artifact. This pass does not accept an eventual merge, later main changes, p04 overlap, or C007 combined integration; C006 remains a proposed downstream contract until the required acceptance/integration publication.

### Process and instruction feedback

One focused correction closed the sole batch finding and the red/green regression directly reproduced the missing path. Re-running the affected union was justified by the shared worker change and retained all earlier guard/transition/budget/UI/interface coverage. Keep R1's candidate lesson about following recovery errors through generic retry wrappers; no additional instruction change is supported by R2.
