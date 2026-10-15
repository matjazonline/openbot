# 04.8 group B — senior guidance, revision 1

S: `thr_xku2jgixtv`; 2026-10-15. Source baseline:
`8533a792a195bf4e2b0d48f1c2f1bc706fc3d006`. Group A source disposition: [R-001](agent-review-notes.md).
Group B implementation starts only after root accepts group A verification.

## Original requirements (not new S decisions)

Authority is [phase 04 execution item 7 and acceptance](../04-actions-http-and-delivery.md),
[EXPANSION 04.8](../EXPANSION.md), accepted [BRIEF-04.8 §B and boundary table](../BRIEF-04.8.md),
and [04.7 calibrated cancellation probes](../CONTRACT-04.7-CANCEL-SERIALIZATION.md).
Cancellation suppresses undispatched work, cannot retract accepted remote effects,
and preserves late truth without cancelled advancement. Competing dispatchers,
stale workers and duplicate callbacks must not produce new logical effects.
Tests use real services/CancelCommand, isolated migrated fixtures, actual blockers,
bounded cleanup and exact durable outcomes. No raised bounds, provider integration,
schema work or new public test ports are authorized by this guidance.

The decisions below resolve consequential test coverage and oracle choices. They
are S guidance, not additional product requirements. Existing named tests below
were inspected as source; their presence is not fresh execution evidence.
Persistence paths are relative to `src/adapters/persistence/workflow/`.

## DEC-001 — reuse exact coverage; fill the uncovered combinations

Affects every BRIEF boundary row. D's expansion must map final exact test names
to this table and distinguish reused, extended and new coverage.

| Boundary | Exact existing source coverage | Group B disposition |
|---|---|---|
| Prepared/undispatched | `workflow_action_schema_fk_immutable_and_cancelled_new_intent_fail_closed` (`action_tests.rs:101–131`) uses raw SQL cancellation. `workflow_action_dispatch_stale_fences_scopes_protection_and_errors` (`action_dispatch_tests.rs:238–275`) covers local/remote approval refusal. Group A's `workflow_action_cancellation_company_before_run_all_callers` covers actual cancel before adapter local/reserve. | Add real CancelCommand followed by public local/remote service dispatch, including protected intent. Assert prepared identity retained, zero new marker/entry/receipt/effect/provider call and no completion. Reuse the schema/authority negatives; do not call their SQL updates a real command test. |
| Lock competition | `workflow_action_cancellation_company_before_run_all_callers` and `workflow_action_cancellation_return_lost_owner_precedes_denied_authority` (`action_cancellation_order_tests.rs:315–353`). | Covered by group A for the authority inversion and all affected callers. Re-run; do not duplicate this ten-variant matrix in B. Use DEC-002 for the new receipt/completion pairs. |
| Marker before entry | `workflow_action_dispatch_service_post_marker_revoke_cancel_zero_provider_polls` (`action_dispatch_remote_tests.rs:69–98`) and `workflow_action_dispatch_post_marker_revocation_cancel_and_expiry_refuse_entry` (`action_dispatch_tests.rs:395–418`) use SQL state changes. | Add a service gate after real reservation returns and before real entry authorization; apply actual cancellation, then release. Retain original marker and reserved remote-entry identity, zero provider polls, exactly one retirement and inert fresh-service replay. Reservation already creates an entry: “no new entry” means no additional entry beyond that baseline. |
| Work in flight | `workflow_action_dispatch_pending_actual_provider_dropped_on_loss_cancel_deadline` (`action_dispatch_remote_tests.rs:225–282`) has a real pending future and token-only mode, but its durable-cancel mode uses SQL. `workflow_control_cancel_actual_handler_future_is_dropped_by_heartbeat` tests the generic handler. | Add actual CancelCommand while the action provider is pending; independently retain/extend token-only coverage to assert no durable cancellation. DEC-003 supplies the distinction. |
| Result/receipt/cancel | `workflow_action_receipt_late_cancel_and_stale_truth_without_advancement` (`action_receipt_tests.rs:277–310`) uses SQL cancel; `workflow_action_receipt_resolution_stalled_commit_against_completion` covers receipt/completion resolution, not CancelCommand. | Add real receipt-commit/cancel contention in both observed orders, and token cancellation after the service observes a valid result but before settlement returns. DEC-004. |
| Duplicate/restart | `workflow_action_receipt_committed_ambiguous_ack_restart_zero_io` (`action_receipt_acceptance_tests.rs:50–72`) proves live restart. `workflow_action_reconciliation_old_actual_after_new_receipt_keeps_same_and_different_truth` (`action_reconciliation_receipt_tests.rs:320–458`) proves canonical-result preservation and duplicate observation idempotency. `workflow_action_reconciliation_late_truth_cancelled_retains_conflicting_actual_receipt` (`action_reconciliation_late_truth_tests.rs:333–393`) covers actual cancel, late conflict, inert claim/dispatch/completion. | Reuse the conflict/immutability cases. Extend the new ordinary cancelled late-receipt case with same-observation redelivery and fresh service retry; assert exact unchanged truth and zero additional provider polls. No duplicate full conflict matrix. |
| Completion boundary | `workflow_control_cancel_completion_competitors_preserve_winning_result` (`control_tests.rs:303–351`) is a barrier race, not deterministic both orders. `workflow_action_receipt_actual_result_precedes_shared_completion_writer` (`action_receipt_acceptance_tests.rs:134–167`) proves receipt-before-completion without cancellation. | Add their missing composition: actual receipt, then actual competing cancel/shared completion, deterministically both orders. DEC-005. |
| Parked/reconciled | `workflow_action_uncertainty_parked_deadline_cancel_and_late_receipt_truth` (`action_uncertainty_tests.rs:281–351`) already uses real cancel. `workflow_action_reconciliation_control_serialization_command_before_cancel` and `workflow_action_reconciliation_control_serialization_cancel_before_command` each cover Applied/FinalNotApplied/Unknown, terminal no-dispatch, exact evidence/revision/audit and no claim (`action_reconciliation_control_serialization_tests.rs:134–445`). `workflow_action_reconciliation_claim_cancellation_cancel_first` and `workflow_action_reconciliation_claim_cancellation_claim_first` cover scheduled claim retirement/refusal (`action_reconciliation_claim_cancellation_tests.rs:67–310`). | Reuse these accepted contracts and their expiry companions. Proven-not-applied does not require another full matrix. Add a compact cancelled SafeRepeat/ProviderIdempotency history check: the live replay contract remains valid but cannot override durable terminal state; zero additional calls/entries/claims. Existing `workflow_action_receipt_lost_actual_effect_supported_reclaim_and_competing_replay` supplies the live positive counterpart, not cancelled coverage. |
| Rollback | `workflow_control_cancel_deferred_faults_roll_back_complete_state` and `workflow_control_wait_and_retry_deferred_faults_roll_back_complete_state` (`control_acceptance_tests.rs:257–317`), plus `workflow_action_receipt_deferred_rollback_returns_no_success` (`action_receipt_tests.rs:313–335`). | Reuse control fault locations and receipt-failure semantics. Add action-aware cancellation rollback evidence because `control_state` omits action tables and the old test retries cancel instead of demonstrating continued action work under the old fence. DEC-006. |

## DEC-002 — prove the real blocking site and both winning orders

Affects lock, receipt and completion races. Preserve the calibrated contract;
company contention alone never proves run ownership. Use the actual same-company,
same-run operations with valid revision/fence, not direct calls to `cancel_on`.

- Where both owners first authorize company, reuse group A/04.7: hold only the
  exact execution row; positively probe the exact company/run as free; stop the
  first real owner at that execution gate; require run NOWAIT `55P03`; then observe
  the second owner's company query blocked by the first PID. Roll back every
  probe, including failed probes. Prove the run is free again after drain.
- Receipt persistence (`action_receipts.rs:199–254`) and completion
  (`completion.rs:72–173`) lock run before execution and do not first acquire
  company authority. For these pairs observe the second real owner's run query,
  Lock wait and `pg_blocking_pids` pointing at the first owner directly. Do not
  transplant a company-wait expectation onto these paths.
- Cancel-first: stop actual cancellation at its execution gate while it owns run,
  then observe receipt settlement/completion waiting on that exact run. The free
  probe before contenders and held probe before the second starts are calibrated
  negative controls for accidental run locking by the fixture and missing owner
  locking. Completion-first: stop completion at its execution acquisition while
  it owns run, then observe actual cancellation's head/run query behind it.
- Receipt-first: use a bounded existing transaction/commit gate after the actual
  observation/receipt writes while the receipt transaction owns run. Observe
  cancellation (which already holds company) waiting at its run query before
  releasing. For this ordinary success race, release before persistence deadlines;
  deliberately ambiguous COMMIT resolution remains the existing separate tests.
- Keep contenders polled and reject premature completion. Verify distinct backend
  PIDs and the expected statement, release gates, await both real results, and
  prove clean backend transactions. A timeout is failure (or an explicitly void
  attempt requiring a witnessed successful replacement), never a passing race.
  Do not raise production/stack/lease limits. Include an ungated live positive
  action/completion case so a refusal-only implementation cannot satisfy the oracle.

If exclusive blocker attribution is unavailable, use the contract's bounded
run-probe waiter correlated to the first PID and drain it before gate release.
D chooses helper layout and established gate reuse; no production seam is needed.

## DEC-003 — token interruption and durable cancellation are different events

Affects prepared, marker and pending rows. `WorkflowControl::cancel` owns durable
retirement. A CancellationToken interrupts one invocation and does not mutate run,
job, attempt, waits, revision or command audit into cancelled state.

For the durable pending case, keep the invocation token uncancelled: await the
provider's genuine first-poll signal, commit a real CancelCommand, and let ownership
supervision drop the actual provider future. Require one first poll, one drop before
service return, `Interrupted`, retained marker/entry, no fabricated receipt, exactly
one durable retirement, no live job/attempt, and inert subsequent dispatch/claim.
An “accepted then pending forever” provider ledger may record one real effect;
interruption must neither delete it nor infer not-applied. A never-applied pending
fixture instead has zero effects. Assert the fixture's actual ledger truth.

For token-only pending interruption, require the same drop-before-return but retain
the run/ownership/control facts from the pre-interruption snapshot while within its
live lease. The unresolved marker persists. A live retry must obey the original
recovery contract; token cancellation itself grants no new attempt/replay permission.
An already-cancelled token prevents initial remote reservation/polling. A token
cancelled after final entry authorization but before first poll also suppresses the
poll through the existing monotonic/token check (`application/workflow/actions/dispatch.rs:132–200`).
Do not impose a promise that durable cancellation after final locked authorization
must prevent all transport: the accepted linearization contract explicitly allows
that already-authorized effect.

## DEC-004 — observed truth settles before output suppression

Affects result/receipt, restart and duplicates. Use a real provider ledger and
real reservation/entry. Place the service seam at `finish_remote` entry to prove
the supervising service has selected and validated the provider result before
cancelling the token. A provider merely becoming Ready is insufficient: the biased
select can choose cancellation first. The seam delegates to real persistence.

Once observed, result settlement must finish within its existing bounds even if
the token is now cancelled; then the service returns `Interrupted`. Require one
effect, one canonical receipt and actual observation tied to the original exact
entry/digest, no completion/output/successor, and no durable cancellation for the
token-only variant. Do not cancel/drop the settlement future to make this test pass.
Operational DB/policy/timeout failures remain errors, not false success or blanket
Interrupted; reuse `workflow_action_receipt_return_authority_errors_propagate_after_commit`.

For the ordinary no-conflict receipt/cancel race, require `CancelResult::Applied`
in both DEC-002 orders: initial receipt/observation insertion does not itself
advance the control revision when it creates no evidence conflict. No unrelated
writer may stale the command. Cancel-first allows late receipt truth but no caller
success. Receipt-first preserves that truth; the deliberately queued cancel holds
company authority, so receipt return authorization waits until cancellation retires
the fence and returns `Interrupted`. Do not broaden either oracle to arbitrary errors.
If testing a conflict/reconciliation history instead, its revision writes change
the contract; reuse the exact 04.7 expectations rather than importing this outcome.

After durable cancellation and late settlement, repeat identical accepted observation
delivery and reconstruct a fresh service. No additional effect, entry, canonical
receipt, identical observation or conflict may appear; no cancelled execution
advances. Reuse the named 04.7 conflicting-truth tests for the append-only conflict
case and canonical-first-result rule. A dropped unobserved result remains uncertain;
neither cancellation nor a timeout fabricates receipt or final-not-applied evidence.

## DEC-005 — completion winner determines result priority

Affects the completion row. Obtain a real action receipt first, with result valid
for the frozen step's output schema; receipt commitment alone has not completed the
step. Build the competing cancellation command from this current revision.

1. **Cancel commits first:** actual cancellation is Applied; shared completion
   returns `None` under the original fence. Receipt/effect/observation remain;
   completed_at, output, route and successor are absent; no successor job appears.
2. **Completion commits first:** completion is `Committed`; the command using the
   earlier expected revision is exactly `RevisionConflict`, even when completion
   made the run terminal. `controls.rs:98–110` checks revision before terminal.
   Output/route/successor and completed attempt remain byte-stable. Repeating the
   same exact completion may replay its committed result and must not duplicate it.
   On an end-route fixture, a new command at the current revision returns
   `AlreadyTerminalOrApplied`, without rewriting that result.

`completion.rs:94–106,209–269` checks completed execution replay before live-window
refusal; preserve this priority. Do not assert “any later cancel erases success.”
If D also uses a nonterminal successor route, a fresh correctly revisioned cancel
can retire remaining work, but cannot erase the predecessor's committed result.
Include a route with a possible successor to make cancel-first's no-successor
assertion meaningful; an end-route case supplies the terminal result check.
Expected control-command receipts may be added even for conflict/terminal results;
compare execution truth and explicitly allowed audit deltas, not indiscriminately
byte-identical whole databases.

## DEC-006 — rollback must restore action usability, not just a run label

Affects cancellation atomicity. Existing control tests exercise deferred failures
on run-event INSERT, command INSERT and attempt UPDATE, but their snapshots omit
action facts and never use the old fence after failure. Retain those tests.

Add focused action-bearing cases using a deferred command-commit fault (or an
equivalent established deferred gate after all cancellation writes): prepared
undispatched work and entered/accepted work awaiting receipt. Before the fault,
capture the run/revision, execution, job, exact attempt, waits/events, control
receipt/audit, intent/marker/entry/receipt/actual observations, reconciliation/evidence/
conflicts and affected budget/claim witnesses. Require the injected error and a full
rollback to that baseline, including any cancellation uncertainty audit. Remove the
test fault and prove the **same** original fence still performs valid work without
reclaim/new attempt: prepared work can dispatch once; the already accepted effect
can settle its receipt and complete without another provider call. Do not invoke a
fresh cancellation before this proof.

Reuse `workflow_action_receipt_deferred_rollback_returns_no_success` for the converse:
a receipt commit failure leaves the already accepted external effect intact, no
receipt/success, and unresolved marker without blind replay. Cancellation rollback
does not roll back an independently committed provider effect.

## DEC-007 — three cohesive test sub-batches and evidence ownership

Suggested split, matching BRIEF §B:

1. **B1:** prepared/protected, post-reservation, pending provider and token-only
   semantics; action-bearing cancellation rollback (DEC-003/006).
2. **B2:** both receipt/cancel orders, observed-result token case, ordinary cancelled
   duplicate/restart, and reuse of conflict cases (DEC-002/004).
3. **B3:** both shared-completion orders, SafeRepeat/ProviderIdempotency terminal
   refusal, and exact accepted uncertainty/reconciliation rechecks (DEC-001/005).

D owns implementation expansion, small typed fixture helpers, chosen test names,
all source edits, builds and the DB. Use `own_database`, keep the five-connection
fixture budget in mind when gates/observers/contenders coexist, retain bounds and
stock 2 MiB checks. Record exact commands, fixture identity, source artifact and
non-skipped results per sub-batch. Freeze each cohesive review handoff and let root
accept it before dependent work. Final group C gates from BRIEF remain required;
this guidance does not substitute isolated tests for the combined acceptance gate.

## DEC-008 — production gap disposition and reopen rule

No production behavior gap found by this inspection. Group A's authority fix
supports a test-only group B: durable cancel, run-locked truth persistence,
post-observation settlement and completion priority already implement the accepted
contract. This conclusion is source-based and subject to D's runtime evidence.

If a constructed real-operation test contradicts these invariants, stop only the
affected dependent edits and return the concrete trace, artifact, requirement and
decision ID to root/S. A needed production behavior change **reopens the affected
architecture gate**; do not quietly weaken an assertion or extend production scope.
An unreachable blocker constructor is a test-design question, not itself proof of
a production bug. There are no open product questions requiring root/user now.

## DEC-009 — Linux verification failures: separate the three mechanisms

S-002, 2026-10-15; additive guidance to revision 1. Evidence is D's
[implementation record](IMPLEMENTATION.md), the actual logs below, and current
source at `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006`. No runtime check was executed
by S. These are verification failures, not grounds to mark the suite accepted.

| Failure | Evidence and disposition |
|---|---|
| `workflow_action_reconciliation_high_bounds_native_owners_and_supported_entry` | **Established fixture lease-lifetime sensitivity; calibration/test orchestration correction appropriate.** Group A [run 1](logs/groupA/runtime-workflow-action.run1.log):346–347 fails entry; [run 2](logs/groupA/runtime-workflow-action.run2.log):346–347 fails reservation. The import-only [baseline](logs/groupA/runtime-workflow-action.ab-baseline.log):349–352 directly observes `live=false` in `assert_subject`, after three native rejection checks. The baseline patch changes imports only. `high_source` returns with attempt 130 freshly claimed (`high_bounds_source.rs:398–411`), but the outer test then performs catalog reads, native attacks and repeated `all_tables`/catalog assertions without renewing that lease (`high_bounds_tests.rs:27–62`). `supported_positive` adds whole-table snapshots and provenance checks before entry (`high_bounds_native.rs:228–268`). `all_tables` reads every public table in a separate query inside a repeatable-read transaction (`action_reconciliation_proof_tests.rs:31–52`). This is fixture work between bounded operations, not one production transaction taking the test's entire 54–93 seconds. Two isolated passes ([1](logs/groupA/high-bounds-isolated-1.log), [2](logs/groupA/high-bounds-isolated-2.log)) support the load-dependent diagnosis. The exact cause of each generic group A Conflict was not logged; the baseline proves the fixture premise fails, not that every Conflict is expiry. |
| `workflow_action_reconciliation_bounds_128_prior_consuming_129_total_then_lost_refuses` | **Likely fixture timing, but attribution remains unverified; do not label it a proven three-second lease lapse.** Run 2 :352–353 reports the distinct `Timeout("Workflow reconciliation verification")` at `prior_bounds_tests.rs:403`. That line is reached only when `PausedVerifier.returned` is true. The five-second outer timeout in `application/workflow/actions/reconciliation/service.rs:74–93` covers authorization, snapshot, verifier, evidence validation **and settlement** (`:95–184`), not just the provider. The test intentionally pauses the verifier while the competitor claims, consumes entry129, parks, runs a barrier and takes several whole-database snapshots (`prior_bounds_tests.rs:128–189,345–415`). This plausibly exhausts the command's budget, but no release/return/settlement timings or actual blocker were logged. A/B's other failure does not reproduce this particular timeout. DEC-011 specifies the required discriminator; production slowness or a lock wait has not been excluded. |
| `workflow_action_reconciliation_runtime_mixed_invalid_then_same_genuine_result` | **Probable fixture lease lapse, not yet directly measured.** Baseline :335–336 unwraps `None` from shared completion (`runtime_mixed_truth_tests.rs:429–449`). `continue_genuine` claims with the same three-second policy, then runs full snapshots, a rollback recovery probe, three service dispatches and additional assertions before completion (`:359–449`). Its `heartbeat` helper exists (`:106–125`) but is not called in this final segment. Completion's live-window refusal fits this failure. `None` also has non-expiry causes, so record exact fence/state/window and phase timings before calling the individual event confirmed calibration. DEC-010's correction is appropriate if that observation confirms expiry; a live-fence refusal requires further investigation. |

No production correctness defect is established by these logs. In particular, no
unsafe effect, deadlock cycle, fairness failure, residual owning transaction or
unbounded production loop is demonstrated. Zero post-run residue does not rule out
a transient lock wait or prove performance. The A/B comparison disproves “group A
is necessary for the high-bounds failure”; it does not prove equal latency/failure
rates or explain every timeout. R-001's source PASS remains; runtime acceptance is
still blocked until the corrected artifact and required checks pass.

## DEC-010 — correct lease premises without weakening the resource guard

For high-bounds, and mixed-truth if exact-state diagnostics confirm expiry, first
correct the test's ownership lifetime. Keep the common three-second `policy()`,
production bounds, default-parallel suite and real ownership checks unchanged.
Move expensive diagnostic/catalog/full-table work outside the critical interval
where possible. Use genuine, checked `renew_io` at quiescent phase boundaries
before the old fence expires; never update expiry by SQL to resurrect it. Preserve
the same attempt, worker, generation, native history and real replay proof.

Reservation freezes its own monotonic window. Renewing the DB lease **after** a
reservation does not extend that saved window: remove/reorder heavy observations
between reserve and entry instead, or start the phase with sufficient measured
headroom. No test may bypass `enter_remote`, fabricate a reservation or count a
refusal as the required supported-entry success. Keep all native rejection
oracles. When renewal changes lease timestamps, compare snapshots at correctly
chosen boundaries or account for precisely those renewal fields; never normalize
away arbitrary job/run differences. Drain any owned supervision helper.

Add phase-labelled diagnostics for elapsed monotonic time, exact owner identity,
DB clock, lease expiry, run deadline and operation result at relevant boundaries.
Measure fixture diagnostics separately from acquire/lock/work/commit time. A later
DB observation of expiry alone must not retroactively excuse an earlier generic
Conflict; preserve unrelated errors and any live-owner refusal as hard failures.

**What still fails early:** the full test must positively prove supported entry
130 (or genuine mixed completion), and a lost live premise produces a distinct
phase-labelled lease-lapse failure rather than an unwrap panic or a passing skip.
Keep the existing three-fresh-fixture ceiling for explicitly qualified void
attempts; exhaustion remains failure and must be tested. Do not add unbounded
retry, silently omit native checks, globally serialize the suite, or exclude the
heavy cases from required parallel verification.

Retain `scripts/lease-budget.sh` and its CI invocation, required operation coverage,
50% work-phase threshold, stock-stack gate and real expired-fence rejection tests.
With the original three-second policy, the attempt bound is 1 s and the existing
work gate is 500 ms. The script separates acquire/lock/commit from code-owned work;
its default filters cover dispatch/receipt/completion, **not** the whole
reconciliation command or these high-history constructions. Thus that gate alone
cannot clear DEC-011 or establish high-history performance.

**No raised limit is approved by this decision.** If measurements show a longer
fixture lease is still necessary, D returns the phase measurements and proposed
fixture-only value for S review. Raising 3 s also raises derived persistence bounds
up to their caps (`application/workflow/lease.rs:31–67`), so it is not merely a
test sleep adjustment. Such a revision must preserve a CI failure tied to the
old code-work budget for the affected high-history operations, measured without
diagnostic scans, plus explicit lapse classification and exhaustion failures.
Reporting longer durations without a failing assertion is not an early guard.

## DEC-011 — distinguish deliberate verifier delay from a slow settlement

Before relaxing the prior-bounds classifier, D should retain one instrumented
default-parallel failure or a successful measured reproduction of the same boundary.
Record command start, verifier entered/ready, release sent, verifier returned,
settlement entered/returned and command outcome using one monotonic clock. Record
DB owner/window and backend wait/blocker information when an actual DB phase stalls.
Use an existing test adapter/delegating wrapper where needed; no production public
port or timeout change. Also measure the actual settlement on the same 128-to-129
history with a promptly released verifier, independently of fixture preparation.

`returned=true` means only that `PausedVerifier::verify` reached its return. It does
not mean the service completed settlement within its remaining budget. The existing
`withheld_timeout` permits only the exact outer timeout, elapsed >=5 s and no release
or release >=5 s; its negative control explicitly rejects a prompt release.
Preserve that discriminator. A late-release timeout can be qualified independently
of the returned flag only with those causal timestamps and the durable backstop.
If the verifier was promptly released, the timeout remains a hard failure until
the remaining phase is identified; do not broaden the classifier to “any timeout
under load.” A near-deadline release needing a new margin rule requires measured
settlement headroom and corresponding negative controls, not a guessed allowance.

Prefer reducing test-only work while verification is deliberately paused: prepare
immutable diagnostic inputs before the command; perform only the genuine competing
128-to-129 transition and indispensable pre-settlement snapshot inside the pause;
move post-transition analysis outside it. Preserve the stale 128-entry snapshot,
actual competing claim/consumption/retirement, and the required `BoundExceeded`
refusal. Do not bypass the application service or replace the contender with mocks.

A qualified timeout is a **void attempt, never a boundary proof**. Before a fresh
fixture, drain operations and inspect exact command/evidence/audit/job/attempt/
entry/consumption/budget state. Do not assume a timeout after verifier return wrote
nothing: it can overlap settlement/COMMIT. Use established exact-command observation
or replay where applicable; no new key that conceals an unresolved outcome. The
existing no-reconciliation-delta backstop must actually hold for a no-write void
classification. Preserve `prior_bounds_timeout_discriminator_rejects_prompt_release_and_wrong_errors`,
`prior_bounds_three_void_fixtures_are_not_a_proof`, and the expiry/foreign-fence
controls. Add the returned-but-late discriminator case if that is the observed race.
At least one complete fresh proof is mandatory within the existing three attempts.

Production investigation is required if a promptly released, minimally instrumented
command still exhausts its budget, an unexpected lock cycle/owner appears, or the
same live fence is refused without a documented domain cause. Separate host
scheduling/IO, expected lock wait and code-owned work; do not infer starvation from
the total suite duration. Retain the unchanged five-second command cap and the
native 128/129 limits. No production architecture change is authorized here.

If that investigation proves an inherited reconciliation defect, report it as a
04.7 foundation issue requiring a separate scoped correction decision; baseline
presence does not make it acceptable. If the new company-before-run path introduces
or materially causes the defect, reopen the affected BRIEF-04.8 architecture gate.
Neither conclusion is established yet. No product/user decision is needed to do
these bounded fixture corrections and measurements; root must keep verification
open and assign them to D. Strict Clippy remains outside S-002.

## DEC-012 — B2 receipt gate policy and primary-attempt evidence

S-008, 2026-10-15; consequential fixture guidance, not a new original requirement.
This narrowly qualifies DEC-002's no-lease-increase instruction for the receipt-first
COMMIT-gated fixture. Reusing the existing six-second `gated_policy()` through genuine
renewal is acceptable **provided the test explicitly proves the ordinary first
attempt completed without ambiguous-COMMIT resolution**. It changes this fixture's
attempt bound from one to two seconds and its total persistence bound from 1.5 to
three seconds; “no production limit changed” alone is not sufficient justification.
The common three-second policy, ungated live receipt case, production bounds,
stock-stack checks and CI lease-work budget remain unchanged.

Record and assert release before the actual primary attempt deadline, or a
conservative deadline anchored before calling settlement, and positively establish
the primary completed outcome. The existing test-only persistence recorder can
distinguish `completed` from `resolved_applied` without adding a production port.
A late release/resolution must fail this ordinary-race oracle, even if the receipt
eventually commits and cancellation returns Applied. A targeted negative control
must show that distinction, with operations drained. See F-005/R-008.

The earlier failure signals are this ordinary-attempt assertion and the retained
CI work-phase gate in `scripts/lease-budget.sh` (50% of the reported attempt bound,
including required action.commit_remote and action.return_authorized coverage).
The latter measures code-owned work, not deliberate COMMIT stalls, and cannot
substitute for the former. No further lease/timeout increase is authorized.

For B3, retain DEC-005's both-order completion outcomes and DEC-002's actual
blockers/calibrated probes. Do not count a resolved ambiguous write as the ordinary
primary-attempt winner. Reuse existing ambiguity tests for that separate contract;
do not automatically apply this six-second fixture choice to every new race.

## B3 guidance readiness — S-010 / R-010

S2 `thr_4pqv29gbbd`, 2026-10-15. DEC-001/002/005/007/012 and the
R-006 Q3 / R-008 carries remain sufficient against current code. No new S
decision or DEC-013 is needed; this confirms existing scope, not B3 acceptance.

- Completion locks run before execution, checks completed replay before the live
  window, and separately resolves ambiguous writes (`completion.rs:23–69,72–173,
  209–269`). DEC-005's exact results and DEC-012's ordinary-attempt discriminator
  both remain necessary; a returned successful result alone cannot identify the
  primary path. Cancellation checks revision before terminal state
  (`controls.rs:98–110`), so the stale command must report RevisionConflict.
- R-006 Q3 permits composed existing coverage: B1's actual applied-then-hung
  provider/drop/uncertainty proof plus B3's exact terminal recovery rechecks.
  In `workflow_action_reconciliation_control_serialization_cancel_before_command`,
  the Applied variant rejects the pre-cancel snapshot, then a fresh authorized
  command records applied evidence and a recovered receipt without scheduling.
  Its Final/Unknown siblings, opposite order, claim-cancellation and late-truth
  cases remain the DEC-001 rechecks. Map these explicitly; ordinary late receipt
  settlement by itself does not establish verifier-based recovery. No duplicate
  full reconciliation matrix or mandatory combined hung-provider fixture is added.
- SafeRepeat and ProviderIdempotency remain subject to the current fence before
  receipt/replay handling (`action_dispatch.rs:85–146,381–446`). The new cancelled
  histories still need the genuine live contract premise and terminal no-call/
  no-entry/no-claim outcomes already required by DEC-001. No replay permission
  follows from cancellation. D retains routine fixture and test-design choices.

## Group C integration-review checklist — BRIEF section C

Use after B3 acceptance; D owns execution and the fixture, S reviews the frozen
combined artifact. This checklist collects existing gates, not new product scope.

- **Artifact and coverage:** freeze the combined source, tests, relevant untracked
  files, SQLx metadata and product docs with baseline/digests and preserved changes;
  record the guidance revision. Map every BRIEF boundary to exact new/reused tests,
  including R-006 Q3 and R-008/DEC-012. Reconcile all finding dispositions and
  review substantive deltas since each accepted artifact.
- **Combined behavior:** review authority company-before-run ordering alongside
  run-only receipt/completion, actual competing owners and calibrated blockers;
  verify cancellation versus token interruption, accepted/late/conflicting truth,
  terminal replay refusal, completion winner priority, and rollback with continued
  same-fence work. Ordinary completion and ambiguity recovery need distinct proof.
  Retain exact effects/entries/receipts, revision/audit deltas, budgets and no
  cancelled advancement or duplicate successor/claim.
- **Runtime evidence:** focused new cases plus accepted action/control/completion/
  lease/recovery/budget and affected reconciliation cases; full stock-2-MiB library
  suite at default parallelism. Match selections to `--list`, include negative
  controls outside `workflow_action_` (notably `prior_bounds_`), verify real DB
  execution with missing-DB allowance unset, and inspect bounded drain/no residue.
- **Resource guards:** unchanged `scripts/lease-budget.sh` and
  `scripts/stack-budget.sh`, their CI coverage, and retained F-004/F-005 causal
  controls. Diagnose any first Linux failure with measurements; no automatic
  bound increase, broad serialization or timeout-as-success. DEC-010/011/012
  remain the limits on lease renewal, void attempts and the receipt-gate exception.
- **Static/schema evidence:** fmt, diff check, locked offline all-targets check,
  strict Clippy, live SQLx prepare/check after query edits (runtime SQL included),
  fresh migration/schema/checksum verification on task-owned resources, and
  `graft build`. Inspect cache/graph deltas and bind logs, toolchain, binary and
  DB identity to the final source; skipped or unavailable gates stay unverified.
- **Disposition:** S records the combined actual-code/criteria/integration review
  and exact artifact PASS only when supported; root records acceptance after all
  required checks and finding closure. Later substantive edits reopen affected
  evidence. 04.9, migration squash, commits and deployment remain outside scope.

## DEC-013 — group C timestamp precision: four fixture fixes and one production gap

**S decision**, S-012 / R-012, 2026-10-15. User authorizes the precision fixes
within 04.8, test-only unless production behavior is implicated; a production
gap returns to root for the user's scope decision. Baseline remains B3 manifest
`c3379ee330dd4795f1f78a4758aa80c4a832b001ee28c845b015e5881f115131`.

The full-library failure log has three exact sub-microsecond equality differences,
one schedule remainder of 3599 rather than zero, and two unequal ParkedWorkflow
responses for the same wait. The installed SQLx 0.8.6 codec encodes DateTime through
NaiveDateTime as integer microseconds since PostgreSQL's epoch (`types/chrono/
datetime.rs:35–43,76–79`); it cannot round-trip the observed extra nanoseconds.
This is established from local code and failure values, without depending on an
unverified claim that every macOS clock has a particular resolution.

| Failing test (short name) | Classification and correction criterion |
|---|---|
| `agent_work_stays_out_until_a_human_decision_is_required` | **Test-only.** `attention_tests.rs:717–801` binds an expiry with nanoseconds, then compares the projected stored value to that original. `attention.rs:87–388` selects the earlier stored business due/outreach expiry; `:49–84` returns the stored timestamp unchanged. Use a fixture timestamp exactly representable at microsecond precision before binding, or assert against the independently read stored expiry. Retain hidden-agent-work, one delegation-decision item, exact priority/responsibility and earlier-expiry selection; no epsilon equality or weakened due-date check. |
| `the_queue_ages_the_generation_rather_than_the_row` | **Test-only.** `attention_tests.rs:1745–1798` binds nanosecond generation-open times; the SQL correctly projects/sorts `generation_opened_at`, not the 30-day-old row creation time. Canonicalize both fixture instants before writes, or compare with exact stored generation-open values. Retain the reversed-row-age premise, ordered IDs and exact current-generation age. |
| `extending_an_expired_outreach_serializes_with_the_timeout_sweep` | **Test-only.** `task/tests.rs:6759–6813` compares stored expiry to the nanosecond `new_expiry`. The extension binds one candidate into both outreach/task deadlines (`task/controls.rs:655–706`); the sweep compares stored expiry to the DB clock (`task/operations.rs:835–871`). The Rust ingress future/max-duration check does not compare a reread timestamp for equality and does not cause this failure. Supply an exactly representable future expiry and retain genuine competing extension/sweep, exact saved expiry and waiting states. A timestamp tolerance must not replace the serialization oracle. |
| `claiming_anchors_the_next_run_on_the_slot_not_the_clock` | **Test-only for this interval-claim path.** `schedule.rs:351–430` loads the persisted due slot, records it, and advances from that same stored slot entirely in SQL. The test (`:775–817`) subtracts its pre-storage nanosecond `missed_slot`; four hours minus a fractional microsecond becomes 14399 whole seconds and remainder 3599. Compare against the canonical persisted slot (or a microsecond-exact fixture), retain a positive whole-interval advance and exact rhythm, and keep the future/roughly-half-hour checks. Prefer an exact duration/microsecond remainder plus the claimed `scheduled_for` linkage over second truncation that could conceal a subsecond drift. No production slot-anchoring defect is demonstrated. `cadence_changed`'s one-off comparison is a separate path; even a representational mismatch there binds the same stored one-off instant and does not explain this interval failure. |
| `workflow_wait_competing_park_signal_resume_and_lost_ack` | **Production defect, F-006; do not hide it by truncating the fixture or weakening equality.** `application/workflow/waits.rs:67–107` accepts an RFC3339 nanosecond deadline. `persistence/workflow/waits.rs:145–208` binds the capped deadline but returns the original Rust value on first park; replay returns the stored microsecond value. Both responses come from the real public persistence operation, not from a test-side expected timestamp. First park, concurrent park and replay must return exactly the same persisted deadline and wait ID. Prefer obtaining the canonical deadline from the INSERT's RETURNING row within the same transaction; retain run-deadline capping, state/expiry guards and atomic completion/audit. Keep a deliberately non-microsecond input in the regression on every platform, compare both responses to the stored row, and retain the signal/resume/lost-ack/duplicate assertions. |

F-006 is an inherited wait-foundation precision inconsistency, not an observed
04.8 regression or demonstrated duplicate effect. The current worker uses only
`park_wait(...).is_some()` (`application/workflow/worker.rs:125–130`), so this
evidence does not show worker scheduling from the noncanonical returned deadline.
It still violates stable durable-operation replay. **Reopen only this production
boundary through root/user before D changes it.** The four fixture corrections
can proceed independently under existing authorization. Preserve the current
wait equality failure until the narrow production correction is authorized.

Use one explicit, locally appropriate precision convention for test inputs; do
not sweep production timestamps or change SQL column precision/schema. Truncating
to whole seconds would obscure more than necessary. SQLx's encode rule, not a
guessed rounding epsilon, defines the stored representation. Query edits retain
live SQLx prepare/check and actual database execution obligations. No migration
or resource-limit change is implied by these corrections.

## DEC-014 — reduce timing races without claiming independence from real deadlines

**S decision**, S-012 / R-012, 2026-10-15. Applies the user's choice of quieter-host
rerun or stronger verification. No production lease/timeout, stack budget or
global test parallelism change is authorized. Arbitrary host descheduling can
expire any real lease: event ordering can remove guessed sleeps, but cannot make
a successful bounded PostgreSQL operation independent of real elapsed time.

### The two dispatch failures

`action_dispatch_remote_tests.rs:101–130` shortens the DB lease to 300 ms **before**
reservation/entry, then sleeps 400 ms **after** actual entry. A Conflict at the
outer unwrap does not identify which authorization phase lost its premise.
The construction is sensitive to setup/DB scheduling, but the log alone is not a
phase-labelled proof of lease expiry. The unchanged 20/20 dispatch rerun supports
load sensitivity; it does not turn the failed full run into a pass.

**Feasible test-only stabilization:** use the common three-second policy with
genuine renewal at a quiescent setup boundary; remove the artificial 300 ms DB
lease compression. In a delegating entry seam, first obtain and record the genuine
authorized RemoteEntry, then wait until its **captured actual monotonic deadline**
has elapsed before returning that same entry to the service. Assert the seam was
reached and expired, the invocation token stayed live, zero provider polls and the
same one marker/no-effect/no-receipt facts. A refusal before entry is a failed
premise, never accepted as Interrupted. Do not synthesize entry identity/deadline,
renew after reservation to extend its frozen window, or rely on a fixed sleep.
This proves the final pre-poll guard with an actual expired authorization and
removes the fast-DB requirement of the compressed fixture. The retained early
signal is zero first polls after captured expiry (with a negative/oracle control
that rejects a polled provider), plus the unchanged real persistence/CI work
bounds. It is not a promise that preparation succeeds under arbitrary load.

`workflow_action_dispatch_pending_actual_provider_dropped_on_loss_cancel_deadline`
(`:225–282`) mixes four modes behind a four-second outer join. If dispatch refuses
before first poll, the sibling waits forever for `started`; the outer Elapsed can
therefore hide a failed live premise rather than a slowly dropped provider. The
log does not identify the failed mode or phase. Record named modes and distinguish
entry, first poll, interrupt commit/token, ownership result, deadline and return.
Race the real first-poll signal against premature dispatch completion so a refused
entry fails immediately; always drain the actual invocation and interruption work.

| Mode | Feasible oracle and remaining timing dependency |
|---|---|
| Token-only | Signal from the real pending provider, cancel the token after that signal, require one poll/drop-before-return and no durable cancellation. Ordering can replace sleeps. To attribute termination specifically to the token rather than a competing deadline, keep a live captured-window witness and fail if the deadline wins. B1's token test and its same-fence live retry are reusable evidence. |
| Ownership loss / durable cancelled state | Commit the real existing ownership mutation after first poll; record a delegated real ownership refusal and require return/drop before the captured frozen deadline, as in the accepted B1/F-004 causal oracle. B1 actual CancelCommand coverage can be reused rather than copied. A DB error, timed-out probe, or mere eventual Interrupted is not an ownership-refusal witness. No ordering-only oracle removes the need to beat that deadline on the real service. |
| Run deadline | Retain proof that the real DB run deadline constrained the captured entry and that the actual provider first polled while live. Wait/observe that exact deadline instead of guessing 400 ms from setup. A distinct test may demonstrate only final pre-poll refusal, but cannot replace the pending-future deadline/drop obligation. The deadline branch and its competition with heartbeat are intrinsically timed; retain bounded real-time coverage and rerun on a quiet host if the live premise cannot be established. Do not silently turn this into a lease-expiry or token test. |

The original 300/400 ms values are test compression, not product latency promises.
Removing that compression is acceptable with the exact captured-deadline and
first-poll assertions above. A larger outer watchdog alone is not a correction:
any changed watchdog must retain an earlier failing causal/boundary assertion and
bounded cleanup, per AGENTS.md. Do not merely wrap every failure in a retry loop.
Virtual-time or pure-supervision checks may supplement these tests, but pausing
Tokio does not pause PostgreSQL's clock and cannot replace genuine DB/fence proof.

### Existing DEC-009–012 coverage

- **High-history/mixed-truth lease premises (DEC-009/010):** move costly snapshots/
  diagnostics out of reserve-to-entry and claim-to-completion intervals; genuine
  checked renewals at quiescent boundaries and exact phase/fence evidence remain
  feasible. Keep the separate all-relation boundary oracles, native bounds and
  successful real entry/completion. Renewal cannot revive an expired fence or
  extend a previously frozen authorization. These are less sensitive to load,
  not unconditionally independent of it; a remaining live-window lapse needs a
  fresh successful run, not an accepted refusal.
- **Prior 128-to-129 competing history (DEC-011):** prepare immutable work before
  the verifier pause and defer diagnostics until after settlement, but the genuine
  competing transition must still occur after the stale snapshot. Causal gates
  and minimal work can improve reliability, not suspend the five-second whole
  command cap. Retain exact release/return/settlement timestamps, prompt-release
  failures, durable no-write backstop, three-void ceiling and mandatory completed
  positive proof. Do not classify arbitrary overload as a passing void.
- **B1 ownership supervision / B2 ordinary receipt / B3 ordinary completion:**
  use actual first-poll/lock/COMMIT signals and recorded persistence outcomes as
  already implemented. B1's pre-deadline causal return and B2's primary-attempt
  release/outcome checks remain; B3's one completed primary event remains. A
  delayed real transaction can correctly time out or resolve, but then it is not
  the ordinary-race witness. Recovery negative controls test that separate path.
  No blanket six-second policy, relaxed primary oracle or added broad retries.

**Recommendation:** adopt the bounded test-only stabilization for the compressed
first-poll test if D implements a change; add causal/phase diagnostics to pending
modes only as needed to resolve an actual failure. Existing B1/B2/B3 causal tests
already cover much of the stronger option. Finish required real-time, stock-stack,
default-parallel verification on a quieter host with a stable artifact. An isolated
rerun helps diagnosis but does not replace the final combined gate. Do not stop
unrelated workloads without authorization.

### Budget scripts and early failures

- `scripts/lease-budget.sh` is **load-sensitive**. It runs selected tests serially
  but does not reserve the host. `persistence_bound.rs:370–449` measures elapsed
  monotonic time between phase marks, not CPU time; scheduler stalls, DB execution,
  IO and any waits after the leading-lock mark can inflate work_ms. Excluding
  acquire/leading-lock/COMMIT and injected events does not remove those costs.
  Keep the 50% work gate, required-operation coverage and CI invocation. Inspect
  per-phase JSONL, exact outcome and whether a lock mark exists (without one,
  work starts at acquisition), then repeat at the same bound under controlled
  low interference. A first Linux failure is a calibration investigation, not
  automatic regression attribution, automatic threshold increase or a pass.
  Persistent quiet-host work-budget failures require diagnosis/reduction before
  a separately justified calibration. The gate does not measure all high-history
  reconciliation-command work and cannot replace DEC-011's command evidence.
- `scripts/stack-budget.sh` enforces a **memory** threshold, not a duration
  threshold. At the same code/toolchain/path, more scheduler load does not grant
  a larger stack need. Nevertheless it runs the full real-time library suite;
  those tests may fail under load, and the script explicitly reports non-overflow
  failures as unrelated to the stack budget. Such a run is still not a passing
  full gate. Diagnose a real overflow by call/frame/path and platform before
  changing the 2048 KiB threshold; do not dismiss it as host load. Keep stock-stack
  execution, runtime/libtest stack settings and existing CI gates intact.

The resource signal remains a failing assertion at the actual semantic boundary
and the unchanged CI budgets. Performance obligations cannot be replaced solely
with causal/event-order tests, nor can a slow total suite establish a code-owned
work regression without phase evidence.
