# 04.7 historical-audit refusal fixture decision

Status: proposal frozen for independent Sol check; no implementation or acceptance.
Scope: same-execution/same-kind historical audit substitution with a genuine current
pending retirement. Original Execution 6, BRIEF-04.7 section 5/required checks,
CLAIM-BUDGET:89–118,140–145,208–218 and AUDIT:48–55,132–135 remain authoritative.
All 53 applied migrations and inherited WIP remain immutable/preserved.

## Required behavior and feasibility

A refusal must link the event emitted by its exact current retirement. A previously
committed same-execution exhaustion event cannot supply that linkage, even after a
real current pending-to-failed job transition. Genuine production refusal remains
the positive control. This does not require two exhaustion events to coexist:
AUDIT deliberately preserves UNIQUE(company,run,execution,kind) for non-reconciliation
events (`20260930184000_workflow_action_audit_commands.sql:52–54`). Requiring an
already linked new exhaustion event alongside the old same-kind event is impossible
under that accepted contract, and is not a reason to change cardinality or history.

The earlier manually executed pre-audit UPDATE-prefix proposal remains unaccepted.
Instead, a **test-owned BEFORE INSERT probe** on `workflow_run_events` can invoke
the attack at the exact point reached by the unmodified production retirement owner.
It adds an adversarial write attempt; it neither skips nor changes a production write.
The intended guard rejects the probe before the attempted second audit reaches the
unique index. No successful retirement is claimed for this deliberately aborted call.

Evidence: `pending_recovery.rs:47–96` performs actual run/job retirement, then calls
`waits::audit` at line 89; `waits.rs:136–144` allocates the actual prospective NEW
event. Migration `20261001100000`:147–194 captures OLD pending episode/current xid
and confirms the job transition; :196–206 links only an inserted NEW audit, AFTER
INSERT; :219–254 checks refusal provenance immediately and at commit. Its current
witness therefore already exists and is confirmed when the BEFORE INSERT probe
runs, while its audit_sequence is still NULL. The unchanged refusal guard acquires
run→execution→job→usage and rejects an otherwise scoped historical event because
it is not linked to that current retirement. :256–266 retains atomic linkage rules.
Lifecycle migrations 110000/120000 do not replace these capture/link/refusal rules.

## Bounded test delivery

1. Use an isolated database with all migrations and the existing real scheduled
   reconciliation fixture (`action_reconciliation_claim_sql_tests.rs:18–53`): actual
   provider/park/final proof/command and actual child reservation exhaust shared
   model allowance. Retain all episode/command/attempt/limits/proof facts.
2. Commit an event through existing `waits::audit`, with the same execution and
   literal `workflow.root_budget_exhausted` kind. Save sequence and transaction xid;
   explicitly establish pending lease-free job, active unexpired run, ineligibility,
   and no retirement witness/refusal. This is a historical independently labelled
   event, not a historical genuine budget retirement. The latter cannot legitimately
   be reopened under BRIEF section 5 and remains a separate current-xid test concern.
3. Snapshot all public-table rows after that history commit. Begin the attack
   transaction; install a uniquely named test function and BEFORE INSERT ROW trigger
   transactionally, scoped to the exact prospective company/run/execution/kind and
   known command/history sequence (typed trigger arguments suffice; no helper table
   is needed). Other events return NEW unchanged. The probe never writes witnesses,
   updates/deletes history, substitutes NEW, returns NULL, or disables any trigger.
4. Call `lease::lock_scope`, then the **actual complete**
   `pending_recovery::settle(...RootBudgetExhausted)`. Do not copy its UPDATE SQL,
   extract a production phase API, or call `current_retirement` whose success unwrap
   is intentionally inappropriate for this expected failing owner call. Capture and
   confirm triggers acquire the genuine usage serialization as today. Using the
   full claim owner is also possible if its error observation remains exact.
5. Before its attack, the probe asserts with distinct failure diagnostics: exactly
   one current-xid witness for the genuine episode and retired ordinal; confirmed
   retirement; NULL current audit link; exact failed lease-free job with unchanged
   retry count/no greater attempt; activated uncompleted execution; correct retired
   run state/reason/terminal execution selected by actual action truth; future owner
   and witness deadlines; false shared budget predicate. Assert historical event
   has exact nonnull execution/kind/scoped identity, differs from prospective NEW
   sequence, and belongs to the previously recorded transaction. No historical
   event/witness fields are modified to satisfy these assertions.
6. The probe INSERTs refusal using the actual episode's full composite references
   and the historical sequence. It must observe SQLSTATE 23514 and exact diagnostic
   `invalid current workflow reconciliation budget refusal`. All unrelated FKs and
   uniqueness checks on that refusal are valid. No refusal may already exist.
   Because AppError::Database stores a String (`application/app_error.rs:6–36`),
   capture SQLSTATE and SQLERRM in a narrowly enclosing PL/pgSQL exception handler
   around **only this INSERT** and rethrow a distinct test diagnostic containing
   their actual values. Rust asserts that exact diagnostic. A successful INSERT
   raises a different explicit unexpected-acceptance diagnostic; every other error
   yields different captured values. Do not synthesize a success marker without
   observing the exact database error, swallow the error, or continue to uniqueness.
7. Explicitly roll back the outer transaction. Compare the exact all-public-table
   snapshot, including history, episode/refusal/witness/run/job/revision/attempt/
   budget/receipt/action/proof/provider state. Transactional probe DDL also disappears;
   assert its function/trigger absence. The original historical event remains exact.
8. Retain/run the genuine committed owner-path refusal positive control and existing
   current-retirement/wrong-kind test. Retain the non-action event uniqueness test.
   No same-fixture successful second exhaustion event is possible or required.

## Discrimination, boundaries and dependencies

Unlike the accepted absent-current-retirement historical test, this proves a real
current retirement already occurred before the attack. If refusal validation were
weakened to current confirmed retirement plus presence of any same-execution/kind
event, the probe would observe unexpected acceptance and fail. If the real owner
did not produce its current witness/transition, prerequisite assertions fail. If an
earlier FK/uniqueness check fires, the exact captured diagnostic differs and fails.
It exercises missing **current audit linkage**, not a standalone timestamp comparison;
that linkage is precisely the original historical-event prohibition. It does not
claim two simultaneous same-kind events, a completed successful second retirement,
historical genuine witness/xid coverage, or deferred revalidation coverage.

This is a local test feasibility decision, not a changed production contract.
Independent Sol must verify it against originals and current SQL before implementation;
root alone records acceptance. A test cannot be credited from this source analysis.
No product decision presently blocks this fixture. If the independent check finds
the probe cannot preserve the prerequisites/diagnostic, return the concrete blocker;
do not weaken uniqueness, mutate history, or change applied SQL to make it pass.

Sol owns subsequent test code, optional narrow test-helper visibility, and required
stock-2MiB targeted/affected suites, formatting, offline locked all-target compilation,
strict Clippy, migration inspection, SQLx prepare/check and graph refresh. No migration
or production API change is proposed. Root must assign DB/build ownership separately;
retained PG is stopped and was untouched here. Astra actual-code review follows.
Wrong-execution work is independent; remaining V1 item 2/foundation/full 04.7 stay open.
