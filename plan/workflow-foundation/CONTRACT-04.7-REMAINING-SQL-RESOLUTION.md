# Remaining SQL A/C — bounded feasibility reassessment

## Authorized A/C amendment — 2026-10-02

The implementation request supersedes the historical A/C acceptance and fixture
restrictions below only as follows. A accepts layered coverage: the unchanged
production guard accepts genuine current witnesses and rejects the genuine historical
pair on a reference relation; catalog attachment, genuine public scheduling and public
wrong-scope/ordinal controls remain required. This does **not** exercise the historical
witness diagnostic through an otherwise-valid public episode INSERT, nor independently
isolate the two transaction-ID predicates.

C accepts the real public run-update duplicate-candidate diagnostic after committed
adversarial P1/P2 preparation in a disposable database, with both protected scheduling
histories created by full application owners. P1, P2 and the two schedules commit
separately. The final attack rolls back every public table; database disposal removes
preparation. No fabricated protected history, disabled guards or production changes.

The original criteria and obstruction analysis below remain historical context.
The mandatory [Phase 10 integration reassessment](10-verification-and-cutover.md#reconciliation-stale-evidence-and-duplicate-candidate-integration)
retains the exact-public-boundary obligation. A/C do not complete all 04.7.
Implementation proceeds directly at the user's request; no Sol/Astra workflow or
independent-review PASS is implied. Evidence: [A/C record](EVIDENCE-04.7-SQL-AC.md).

## Historical preparation record

Status: **A and C remain blocked; no implementation-ready design or acceptance change proposed.** This supplement preserves the original blocked contract and independent Sol recheck. B and group 2f are already accepted and are not reopened. Root owns acceptance. No full 04.7/phase 04 or 04.8 claim. Independent Sol must check this reassessment before it changes any queue decision.

Authority remains Execution 6; BRIEF-04.7:194–240/261–326; CONTRACT-04.7-CLAIM-BUDGET:52–118/178–220; foundation review V1 item 2 at `/private/tmp/workflow-04.7-claim-budget-foundation-code-review.md:47–65`. Read the originals and `/private/tmp/workflow-04.7-remaining-sql-expansion-check-final.md` directly. This is a bounded investigation, not an exhaustive impossibility proof.

## A: exact public episode INSERT with historical witness pair

The existing requirement is unchanged: reach the actual public deferred episode guard with unrelated keys/FKs/predicates valid and historical rather than current transaction witnesses; assert its exact diagnostic, rollback all state and retain genuine scheduling positive control. Optional reference-relation guard-function coverage cannot discharge it.

New avenues assessed against the accepted 2f history fixture and current schema:

1. **Current committed scheduled command.** `20261001100000_workflow_action_claim_budget.sql:40–108` installs an immediate command AFTER INSERT binder. It selects the current-xid schedule witness and inserts the episode synchronously; missing current witness raises the earlier binder diagnostic. The resulting command-key PK and job/retired-ordinal UNIQUE block a historical duplicate before its deferred guard. Reusing a different command key is not an otherwise-valid identity: command/evidence/actor-audit scope is coupled by `20260930180000_workflow_action_evidence.sql:47–76` and `20260930184000_workflow_action_audit_commands.sql:55–80`. Fabricating a new linked command/evidence/audit is not a genuine-history fixture.
2. **Accepted genuine pre-episode upgrade fixture.** `action_reconciliation_upgrade_tests.rs:15–156` creates real historical schedules under the 50-migration prefix. That prefix has state witnesses but no schedule-witness or episode tables; those are introduced by migration 20261001100000. Its pending historical schedule is intentionally rejected by the preflight before installation. Its completed unambiguous history upgrades successfully but cannot supply an old matching schedule witness, even if one could restore pending state. Thus this new fixture does not isolate the two xid conjuncts: a required non-xid schedule tuple is absent. Do not backfill it or loosen the migration.
3. **Delete only the binding, or change a command/witness.** Live-owner protected history remains append-only. The accepted lifecycle/cascade migrations 20261001110000:1–38 and 20261001120000:1–35 only permit coordinated deletion once the owning run is absent; they do not leave an authentic surviving command/schedule tuple available for reinsertion. Deleting/recreating owner history is outside the authorized fixture boundary.
4. **Run the guard early on the real table.** Adding a BEFORE checkpoint that invokes the guard before uniqueness would change the tested execution boundary. The production deferred AFTER INSERT guard still never receives the duplicate row. It is not a solution to the required deferred public-boundary test, just another partial function-order test. No such trigger is authorized here.

These facts close those concrete construction attempts. They are not a universal theorem over every possible raw SQL history or migration sequence. No experiment or mutation was run. The already documented reference-relation option remains optional, partial, and unimplemented.

## C: two genuine, simultaneously applicable candidates in one run

The criterion still requires two distinct valid candidate identities before the attack. `20261001100000_workflow_action_claim_budget.sql:110–120,147–178` counts applicable episodes across company/run, not just one job. A per-job UNIQUE violation, foreign-run pair, or function mocked to return a second candidate cannot replace the required exact duplicate-capture diagnostic.

Focused graph/source follow-up found these concrete owner construction paths:

- Indexed production execution INSERTs are admission (`admission_write.rs:69–84`) and successor (`batch_commit.rs:69–90`); graph search also finds raw test-only constructors. The literal search covers indexed files, not a complete normalized-SQL/topology inventory. It is explicitly insufficient for an exhaustive unreachability claim.
- Admission creates the first execution for its newly admitted run. A second root or child admission is a distinct run and therefore not the required pair.
- `batch_commit.rs:5–67` creates a successor and completes the predecessor in the same owner transaction. Progressing a genuinely scheduled predecessor first installs the next attempt, which already disqualifies its old episode; committed predecessor completion independently disqualifies it. `20260928130000_workflow_activation.sql:25–45` prevents clearing completed history. The fact that successor INSERT precedes completion inside the transaction does not by itself produce two applicable episodes: the old claim is already consumed, and the new uncommitted execution has no genuine scheduled history.
- The raw test constructor in `activation_tests.rs:128–146` creates another execution/job. It is not an owner-created second protected episode and cannot be treated as genuine scheduled history. The existing reopening no-runnable-sibling gate still blocks ordinary scheduling while another pending candidate is applicable.
- An older per-job episode cannot be made applicable by deleting later attempts, resetting retry_count or uncompleting its execution under these fixture rules. None of those operations is proposed.

No allowed sequence was found that first commits two genuine schedules and subsequently makes both applicable without rewriting protected history or bypassing owner/guard contracts. Conversely, this investigation does **not** establish that all valid SQL insertion/update/deletion paths preserve at most one candidate. The existing schema permits distinct execution/job identities; a global reachability proof still needs all sources and transitions, including raw inserts and transient states before deferred validation. C remains a missing constructive discriminator, not proven dead code.

## Outstanding decision and next gate

No new product behavior decision is justified by this evidence. Do not ask to widen budget policy, public APIs, production triggers or migration compatibility merely to make a test convenient.

For **A**, a verification-policy decision exists only if the original exact public-deferred-boundary criterion is to change. The concrete alternatives are (1) retain it and leave A open until a permitted public construction is independently established, or (2) explicitly amend the acceptance contract to a conjunction of the already described real-function current/historical pair, real public attachment/success controls and existing public malformed-binding negatives. Option 2 would relinquish the exact historical-xid public INSERT diagnostic coverage; it requires an explicit authorized criterion change and an independent Sol check, and is **not** approved or recommended as equivalent by this supplement. Function-only coverage cannot silently close A.

For **C**, there is no independently justified replacement criterion to choose yet. Continue only with a bounded constructive or exhaustive-topology investigation. Its deliverable must identify every pre-attack candidate and exact authentic provenance, list each raw mutation separately as the attack, prove earlier guards do not mask capture, and preserve protected rows. If an exhaustive theorem is proposed instead, inventory every owner and SQL topology/transition path and seek a separate coverage decision; one unique key or ordinary successor path is insufficient.

Any future feasible A/C proposal must include: named target constraint/diagnostic, all unrelated predicates asserted first, genuine owner positive control, deliberate unexpected-acceptance failure, explicit transaction rollback, equality of every public table (including revision/attempt/command/evidence/audit/episode/witness/refusal/accounting) and absence of test DDL. No implementation, fixture, test run or altered criterion follows automatically from this document.

## Preservation and measurement

Only this supplement is written. Existing blocked contract preserved, SHA256 2cf26ea5a406b5388c7bbfb4e6b5fbaec0af5bff044e0af7d896cc3f1a0d1c36. HEAD c9cb4b434258b6d33ca26def4ff6b5728dd5a6bc; accepted 2f source, inherited WIP, all migrations, SQLx and PROGRESS/RESUME untouched. No DB/build/test/source edits, no graph rebuild, agents, stage, commit, deployment or service operation. Retained PG remains inherited RUNNING and untouched.

Astra UUID 01a0fb39-e3c1-7720-8be9-024d3734ab74. Assignment startup 84,373/258,400 (32.65%) at 2026-10-02T06:13:22.081Z; pre-write 114,681 (44.38%) at 06:16:18.309Z. Runtime token_usage_record.usage / task_started.model_context_window; final sample reported to root. This assignment graft savings ~71,887 estimated tokens, not spend. Independent Sol gate remains required; no self-acceptance.

Final context sample: 117,228/258,400 = 45.37%, 2026-10-02T06:17:45.563Z. Reviewer/architect quiescent, no commands remain active.
