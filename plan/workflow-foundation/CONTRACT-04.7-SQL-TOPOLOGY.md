# 04.7 SQL topology and explicit fixture-boundary decision

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

**Architecture proposal, not independently checked, implemented or accepted.** A remains blocked. C has a concrete adversarial-topology proposal below, which requires an explicit amendment to the intermediate fixture contract before implementation. Full04.7 remains incomplete; B/2d/2e/2f are not reopened. Base HEAD `9cff5164fa0694a4e7664f6ff8c69e473d4c2a97`; root verified the inherited862 source hashes.

## Authority and the distinction that matters

Read directly: Execution6 (`04-actions-http-and-delivery.md:24–38`), BRIEF04.7:194–240/261–326, CLAIM-BUDGET:52–118/178–220, original V1 review:47–65, REMAINING-SQL-TESTS, REMAINING-SQL-RESOLUTION, HANDOFF20261002, PROGRESS and the linked independent blocker/topology/status reports.

CLAIM-BUDGET:69–73 requires integrity failure for multiple applicable episodes; :190–192 requires duplicate-candidate negatives; :206–207 requires isolated raw-SQL guards. REMAINING-SQL-TESTS:148–190 subsequently requires two authentic candidates before the attack and excludes raw topology/status setup from genuine history. These are distinct conditions. The exact production duplicate-capture diagnostic can potentially be exercised by adversarial topology with genuinely owner-created protected schedules, even though that topology cannot arise from ordinary owners. That does not satisfy the later, stricter pre-attack owner-history condition. No condition is silently waived here.

## Inventory and bounded owner-history proof

Saved inventories in `/private/tmp/`: `sql-topology-normalized.json` (original line offsets;286 DML occurrences including tests), `sql-topology-triggers.json` (multiline CREATE/CONSTRAINT TRIGGER statements), `sql-topology-dynamic.txt`, `sql-topology-query-construction.txt`, and `sql-topology-{executions,jobs,attempts,claim-on}-graft.txt`. DML scan covered `.rs/.sql/.sh/.py` under src/migrations/scripts, case-insensitive whitespace, optional public qualification/quoted names, INSERT/UPDATE/DELETE/MERGE/COPY/TRUNCATE. Graph-first broad literal discovery preceded source inspection. This is a checked static inventory, not a SQL parser, arbitrary client enumeration or live-catalog experiment.

| Writer/transition class | Actual evidence and effect |
|---|---|
| Execution/job creation | `admission_write.rs:4–27,29–84`: run INSERT first, one first execution/job. `batch_commit.rs:5–90`: single successor and predecessor completion in same transaction. No other production execution INSERT found. Raw test constructors are not owners. |
| Successor callers | `completion.rs:24–50,39–140`, `batch.rs:106–168`, `wait_commit.rs:58–112`, `recovery.rs:189–238`: all share batch completion, including wait/final-error paths that need not install another attempt. |
| Identity/activation/completion | `activation.rs:44–111`; migrations `20260928104000:18–31`, `20260928130000:25–45`, `20260928133000:39–61`: no reparenting; activated facts and completed output cannot be undone; committed progression immutable. |
| Workflow status/attempt writers | `completion_job.rs:30–65`, `maintenance.rs:50–71`, `pending_recovery.rs:47–96`, `recovery.rs:135–187`, `control_retry.rs:3–42`, `lease_claim.rs:5–75,102–123`, `lease.rs:43–62`, `action_reconciliation/settlement.rs:154–202`. None creates an execution; claim inserts a greater attempt, permanently consuming the old binding under `claim_budget.sql:110–120`. |
| Legacy writers | Every production status/attempt DML found in approval(+transitions), response_review(+commands), attention, task/instructions, ownership, queue, controls, operations explicitly filters legacy, or joins a legacy-filtered source. Apparent unfiltered company_invite/agent_channel/task-counts hits are tests. `task/harness_runs.rs:654` changes only legacy outreach linkage. Thus legacy stop/resume cannot supply a workflow owner suspension route. |
| SQL indirect writers | `20260928080000:5–16,374–445` replaces baseline deletion/principal-release routines with legacy-filtered selection. `20260930181000:111–127` is the final conflict-park replacement: status/run state only, no new execution. |
| Trigger topology | Baseline legacy triggers replaced by legacy WHEN gates in `20260928073000:46–64`; workflow attachments cover association/identity, immutable facts/progression, deferred deadline/completion/retirement/retry, revision, parent/wakeup/budget links, state/schedule/capture/confirmation witnesses. No attempt INSERT trigger or execution-producing trigger found. Only migration dynamic EXECUTEs are lifecycle ALTER TABLE DDL (`20261001110000:24`, `20261001120000:27`). No DML rule/inheritance/partition constructor found. Dynamic table-name DML search found test fixtures, not a production alternative owner. |

For histories originating in current admitted runs and composed solely of these committed production owners, induction gives **at most one uncompleted execution per run**: admission establishes one; successor completes the previous frontier atomically; all remaining operations preserve or reduce that count. Same-run concurrency serializes through the run; uncommitted successor is not visible to a separate scheduling transaction. No intermediate successor has a protected schedule while its predecessor remains uncompleted. A rollback removes the successor as well. One job per execution and one binding per job/retired ordinal then bound applicable candidates by one.

This proves the ordinary-owner construction obstruction within the enumerated current source closure. It does **not** prove arbitrary schema-valid raw SQL preserves that invariant: the schema permits a fresh distinct uncompleted execution/job with no predecessor. Nor does it certify arbitrary historical imports, external SQL clients, malicious DDL or universal SQL reachability. Those are exactly why a raw-SQL defensive negative remains meaningful. The prior status-only detour is not rerun or presented as an owner constructor.

## C: concrete amendment and candidate sequence for Sol checking

Proposed amendment: permit explicit, committed **adversarial fixture preparation** in a disposable own_database before the final rollback-only public capture attack. Preserve real production owners for every dispatch, entry, receipt/proof, retirement, command, schedule witness and episode. Permit only (a) a new unprotected execution/job identity, (b) run state reset with generated revision, (c) hiding an unconsumed pending job as stopped. Do not describe these as genuine owner history. No copied protected rows, counter/attempt reset, removed guard, function shadow, API seam or applied migration change.

This relinquishes the intermediate requirement that both candidates originate before *any* raw attack, and that *every preparatory raw mutation* is rolled back in the final transaction. It preserves complete rollback of the targeted attack against its full committed baseline; disposable-database removal owns preparatory cleanup. Root must decide authorization after independent Sol original-criteria/feasibility check. No authorization is assumed here.

1. In an isolated real root admission with adequate existing policy limits (two activations and all real dispatch debits; do not raise production bounds), use an action with inputs independent of predecessor output. Claim/dispatch/retire E1 through actual owners into waiting/reconciliation, with failed finished attempt and unknown remote result. Do not yet schedule it.
2. **Adversarial preparation transaction P1:** insert a fresh execution E2 in the same company/run, valid published step and unused ordinal2; insert its correctly scoped workflow job with canonical payload and default counter/lease fields. Set run running/NULL waiting reason through raw state UPDATE, not direct revision writes. E1 remains failed/uncompleted. Commit normally, forcing all deferred constraints through COMMIT. Assert exactly one added execution/job, no protected history fabricated, no changed E1 counter/attempt/output, no predecessor/successor link invented. Source basis: association/shape/ordinal/progression guards; activation:44–111 has no predecessor-presence gate. Runtime feasibility still needs confirmation.
3. Claim/activate/dispatch/retire E2 with real owners and real uncertain provider behavior. Assert independent exact dispatch/entry/attempt identities, both executions activated/uncompleted, both jobs failed/lease-free, current run waiting/reconciliation, no greater attempts beyond each retired ordinal. Real budgets remain eligible.
4. Execute E1's genuine proof/reconciliation owner and require Scheduled. Full owner (`settlement.rs:5–79`) commits its own transaction, including deferred retry/episode guards. Assert command final revision, actor audit, state/schedule witnesses and episode scoped to E1. E2 failed means sibling gate passes.
5. **Adversarial preparation transaction P2:** E1 pending→stopped with leases NULL; set running run to waiting/reconciliation. Commit normally. Assert E1 protected tuple byte equality, no new attempt, only expected status/run/revision/witness changes. Capture sees zero applicable candidates after stop. This uses the already inspected status frontier, not a claim of owner legitimacy.
6. Execute E2's genuine proof owner; require Scheduled and actual successful deferred commit. E1 stopped means sibling gate passes; its earlier deferred guards already committed. Record both genuine protected schedules and all-public-table baseline. E1 is intentionally not yet applicable; E2 is applicable. Require no pending transaction/DDL fixture.
7. **Target attack transaction:** raw-restore E1 stopped→pending, preserving counter/lease/history. Assert pending helper returns exact respective command keys for *both* distinct executions/jobs, with all status-independent prerequisites valid. Then public UPDATE run running→waiting/reconciliation (deadline still live). Require immediate SQLSTATE23514, exact `multiple current workflow reconciliation claim episodes` from actual `workflow_action_capture_claim_retirement`, not a uniqueness/FK/retry error. Fail distinctly if either setup fails or capture unexpectedly succeeds. Roll back; require every public table equals step6 baseline, including revisions, all witnesses/history/accounting.

Masking guards: stopped→pending is not failed→pending or processing retirement/completion; revision trigger advances normally. Prior episode/retry deferred triggers do not re-run in a later transaction. Target run capture counts before its single-candidate budget/retirement branch (`claim_budget.sql:147–178`). State-witness/parent/wakeup/revision triggers remain enabled; enumerate their lexical firing order and assert no incompatible terminal metadata or overflow in Sol's check. Do not SET ALL or invoke the guard directly as a substitute.

Positive controls: retain a separate entirely genuine admitted/scheduled single-candidate owner claim/refusal control; additionally require both scheduling owners above to commit with their real deferred guards. Capture's no-error single-candidate transition can be checked in its own rollback transaction with budget-eligible state. Compare whole public tables for every rollback, assert no test DDL and remove only the disposable database after all connections drain. Any unexpected preparation rejection is a design failure, not the intended negative. Do not weaken assertions to get a red result.

## A and next gate

No new unused genuine historical command/schedule tuple was exposed; immediate binding/key obstruction remains exactly as RESOLUTION. A remains blocked. Do not redo excluded historical upgrade or early-trigger substitutions. The concrete previously documented A policy alternative is real production-function current/historical discrimination plus public attachment/success/malformed-boundary controls, explicitly relinquishing the exact historical-xid public INSERT diagnostic; it is not approved here.

Sol independently checks this proposal, exact setup guards, input/claim/budget provenance, full-original criterion mapping, and the explicit fixture amendment before implementation. If the amendment is declined, C remains blocked under the owner-history proof above; repeating ordinary-owner searches is not a productive next action. A/C/04.7 acceptance remains with root, never this report.

Only this architecture file and temporary read-only discovery artifacts were written. No DB/build/test/source/migration/SQLx/service/stage/commit operations. Retained PG untouched; no children or pending commands. Astra UUID `01a0fb9b-5ba7-7bf3-83b3-20202842adac`; startup26,168/258,400=10.13%, latest pre-write112,870=43.68% at2026-10-02T07:59:22.290Z (token_usage_record.usage / task_started.model_context_window). Final measurement/hash supplied to root. Visible graft savings minimum3,859,810 tokens, discovery estimate not spend; one earlier ask result was truncated.
