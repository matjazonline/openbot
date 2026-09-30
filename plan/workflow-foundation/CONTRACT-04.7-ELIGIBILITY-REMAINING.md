# 04.7 remaining eligibility constructors and coverage

Proposed affected expansion, awaiting independent Sol check. No implementation or
acceptance claim. Base `9cff5164fa0694a4e7664f6ff8c69e473d4c2a97`; preserve all WIP.
Only remaining-matrix row 1 / BRIEF-04.7 section 5; no 04.8. Current RESUME and
EVIDENCE-04.7-SQL-AC supersede historical A/C blockers. Existing accepted budget,
child-policy, seven basic eligibility and sibling evidence remain reusable on
unchanged code/fixture assumptions.

## Authoritative requirement and observation boundary

Execution 6 requires authorized truth attachment and proven non-application before
retry. BRIEF:194–240 requires an activated, uncompleted execution, failed lease-free
existing job, no committed progression/runnable sibling, remaining limits and a
suitable retired attempt. Archive RESUME:355–377 preserves these behavior cases.
It does **not** require every redundant SQL conjunct to have a service test in
which removing only that conjunct permits scheduling. The inventory's independent
per-case isolation is a useful tactic, not a new product requirement.

BRIEF:282–284,311–313 separately requires exact isolated SQL diagnostics for the
listed provenance/FK/check/immutability attacks. Preserve those requirements in
their later queue group; this expansion does not amend them or A/C coverage.

The actual service boundary is `action_reconciliation/settlement.rs:154–202`:
non-continuation truth returns unchanged; non-reconciliation waiting/terminal states
return audit-only; absent failed job returns `Blocked(IneligibleJob)` **before** the
predicate; a false predicate returns the same outcome. Thus that outcome alone
does not establish which gate ran. The current predicate is migration
`20261001100000_workflow_action_claim_budget.sql:281–305`; budget subpredicate:14–25.
Evidence must label service behavior, predicate result and invariant rejection
separately. Never describe an earlier rejection as exercising a later conjunct.

## Constructor and acceptance contract

Source paths below are relative to `src/adapters/persistence/workflow/`.

| Remaining behavior | Concrete construction and required evidence | Coverage limit |
| --- | --- | --- |
| Runnable sibling | Begin with the normal parked unsafe-action fixture and genuine provider barrier. Insert a fresh, schema-valid **unactivated** same-run execution at an unused ordinal and its pending workflow job through public SQL, with scoped payload/association; preserve the subject's complete owner history. This is an explicitly adversarial extra queue candidate, not a purported normal successor or fabricated action/attempt history. Compare against a separate unchanged fixture with no extra candidate. Reconciliation must record genuine final truth and return `IneligibleJob`, with both jobs and all prior attempts/accounting unchanged. Assert the subject passes every other reopen predicate and exactly one other pending job exists. | This is a runnable-topology robustness test, not evidence that sequential normal progression creates two runnable executions. Do not call a lower-level progression helper out of sequence and label it normal history. No marker/entry/receipt/proof is copied. One pending sibling tests the original no-runnable-sibling behavior; processing topology is not automatically claimed. Sol must independently verify this ordinary constrained INSERT is admissible before implementation. |
| Activation versus max_steps | Build a real two-step workflow and genuinely activate/dispatch/park ordinal 2. Positive control uses max_steps >=2. For the negative, explicitly inject `workflow_runs.max_steps=1` through its normal public UPDATE, without changing the frozen bundle, activation, action or root budget. This is a malformed mutable run projection, not a changed frozen allowance. Read the resulting revision before issuing the new command. Assert only the ordinal comparison fails among reopen conditions; exact `IneligibleJob`, retained truth and no scheduling. | Normal `batch_commit.rs:15–19` prevents producing an over-limit successor and `activation.rs:69–75` refuses first activation over the bound. The negative therefore must be labelled adversarial projection, never owner-produced limit exhaustion. Do not change an immutable budget or claim this replaces activation-limit poison coverage. If actual guards reject this projection UPDATE, record that exact invariant instead and return the affected constructor for review; do not disable it. |
| Completed/output/route/successor | Use actual receipt return and `complete_io` on a two-step workflow, retaining the first action's real invocation/marker/receipt. Park the second execution through its genuine unsafe action so the run is waiting/reconciliation. A distinct nonconflicting Applied reconciliation command for the completed first execution must retain truth and refuse scheduling that first job (`IneligibleJob` from no failed target job). Preserve its exact completion timestamp/output/route/successor and the second execution/job/attempt. Also use an end-route completion control, whose successor is NULL, to distinguish completion protection from successor presence. | This is reachable completed-history behavior, not an isolated test of `completed_at IS NULL`. Schema couples completion/output and requires completion for routes; ordinary completion writes the group atomically and completes the job (`batch_commit.rs:5–67`). No independent otherwise-valid route-only or successor-only case exists under those checks. |
| Coupled completion/activation schema invariants | On fresh appropriate rows, attempt output without completion, completion without output/activation, and route/successor without completed output. Assert the exact `workflow_execution_output_shape` or `workflow_route_shape` check intended by each shape (with all earlier shape prerequisites valid), then full rollback. On a genuinely activated action fixture, attempted clearing of activation/frozen inputs must raise `23514`, constraint `workflow_activation_immutable`. No target service call follows a rejected setup. | These checks explain why independent persisted-field negatives cannot exist; they are not substitutes described as successful reconciliation predicate execution. Existing exact tests may be reused if they actually assert these diagnostics. |
| Unactivated execution | A genuinely admitted unactivated execution has no claimed attempt or genuine remote marker. Show it remains unactivated with no attempt/action entry after the normal over-limit/invalid-activation refusal; show reconciliation cannot obtain a real scoped subject/marker for it and cannot append evidence or schedule. Pair with the activation immutability attack above and a normal activated/parked positive reconciliation control. | Do not fabricate a remote marker/retired attempt or clear immutable activation merely to reach `continue_on`. This proves public construction/eligibility behavior jointly, not the independent redundant SQL activation conjunct. Retain explicit no-facts/no-new-job/no-debit assertions. |
| Failed job carrying lease fields | On a genuine parked failed job, separately try assigning worker_id, execution_generation, locked_at and lock_expires_at; choose type-valid values and assert exact `23514/background_tasks_lease_check`, rollback and byte-equal history. Include a real processing lease as the shape-positive control and ordinary genuine failed lease-free scheduling as the continuation-positive control. | `20260817000000_init_schema.sql:1990` requires **all** lease fields NULL for every non-processing status. A committed failed lease-bearing job is logically impossible with the installed CHECK. Do not attach copied guard functions or disable constraints to create one. No isolated service lease-field conjunct coverage is claimed or required by BRIEF5. |
| No suitable retired attempt / exhausted pending | Retain the real pending-recovery owner path (`lease.rs:193–217`, `pending_recovery.rs:47–96`): exhausted pending work is retired without inventing an attempt. Existing `pending_recovery_tests.rs:105–141` and `pending_io_tests.rs:37–93` explicitly inject the exhausted counter into a fresh pending fixture; reuse that declared adversarial setup, assert no task attempt exists and normal competing retirement creates none. The failed result must remain ineligible to ordinary retry/reconciliation and never receive a newly fabricated action history. Pair with genuine parked action + exact retired attempt positive scheduling. | This path has both exhausted allowance and no suitable retired attempt, and may be unactivated/no-marker. Those are inseparable in this constructor. It covers the original coupled exhausted-pending refusal; do not claim independent missing-attempt conjunct sensitivity or send a forged marker merely to obtain `IneligibleJob`. If existing boundary APIs cannot express reconciliation for absent action history, test the exact public rejection and zero deltas, plus predicate false, and state that limitation. |
| Activation/invalid-result/deadline/root-budget poison | Continue Sol's independently reachable actual `release_io` retirement cases, using actual unsafe invocation and genuine proof barrier. Record the exact immutable retirement code/class/safety and prove non-poison positive control schedules while poison commands return `IneligibleJob`. | Do not backdate the deadline to test deadline **code**: that tests another gate. No mutation of retired attempts. Poison-code tests are not actual resource-exhaustion tests. |
| Root resource eligibility | Reuse accepted genuine model equality/headroom, activation/repetition equality, root/company isolation and next-claim competitors. Add genuine owning-run exhausted reservation cases for model-call and repetition through `reserve_budget`, starting with remaining headroom but requesting more than remains. Verify actual exhausted receipt, unchanged usage, owner retirement and truthful reconciliation refusal; prove all prior accounting facts immutable. Include no-exhausted-receipt/equality positive control. | `budget.rs:24–68` couples exhausted receipt to BudgetExhausted retirement; report that coupling instead of claiming standalone receipt predicate isolation. `20260929070000_workflow_root_budgets.sql:166–198` grants only if consumed+quantity<=limit; owner histories cannot reach activation/repetition usage > frozen limit. Do not fake usage or mutate ceilings to manufacture that branch. Equality plus genuine oversize-reservation refusal exercises the meaningful boundary. |

The schema proof for coupled execution fields is migration
`20260928130000_workflow_activation.sql:9–18,25–44` and
`20260928133000_workflow_pure_batches.sql:20–28,41–65`. Budget identity/receipt
immutability and receipt-owned usage are in root-budgets migration:133–163,201–224.
Test direct usage mutation separately only as its actual guard diagnostic; an
earlier usage guard must never be reported as testing reconciliation's `>` branch.

## Delivery and independent check

Sol owns all source, fixture, DB and build changes; this document changes no schema
or eligibility policy. Implement in small test modules using the existing isolated
database/provider/whole-public-table snapshot helpers. Preserve stock 2 MiB and
box deep fixture seams. Do not introduce a production test escape hatch.

Before affected implementation, Sol independently checks each proposed constructor
against current constraints and originals. In particular: scoped extra pending
candidate insertion, mutable max_steps UPDATE, historical first-execution Applied
verification, and exact rejection available for no-action unactivated/exhausted
states. Return all findings together. A constructor failure is not a passing test;
revise it before reporting coverage. No row is accepted by this proposal alone.

For service negatives, generate fresh expected revision **after** setup, retain
genuine proof/receipt authority, and assert exact outcome, stored command/evidence,
and complete history delta. No new job, attempt, lease, entry, consumption, lineage,
output or budget debit; original counters/frozen bytes unchanged. Evidence may
advance generated revision through its existing owner. For SQL attacks compare
the entire public snapshot after rollback and name the actual failing constraint.
Positive controls must reach scheduling or the tested schema shape, as applicable.

No original eligibility behavior is declared impossible here. Certain *independent
conjunct diagnostics* are impossible by schema implication; they are explicitly
unclaimed. If independent review finds an original requirement specifically demands
one, stop only that affected discriminator, record its exact original wording and
constraint proof, and ask for a coverage decision. Do not generalize the separately
authorized A/C reference-trigger exception to this work.

Required checks remain the original targeted affected suites, formatting/whitespace,
offline all-target compilation, strict Clippy, migrations/schema/checksums, SQLx and
graft under Sol's resource ownership; final full 04.7 integration remains separate.
