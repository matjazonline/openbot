# 03.9 durable root budgets — reconciled expansion

2026-09-29. Scope: original03 Failure/recovery, BRIEF-03.9 contract3 and
matrix4; preserve accepted03.1–03.8 and recovery/controls. Root accepted coverage
2026-09-29 before source edits. No fairness, phase04, production model handler,
child-call creation or phase03 combined final acceptance in this fragment.

## Concrete contracts

1. **Frozen limits.** Extend existing ExecutionLimits with a cohesive RootBudgetLimits
   (activation, model-call and repetition ceilings), represented by optional YAML
   `limits.root_budget` with explicit positive, platform-capped values. Omission
   selects bounded defaults:100,000 activations,1,000 model calls,1,000 repetitions;
   authors may lower each. Existing per-run max_steps/max_context_bytes remain
   unchanged and independently enforced. Model count is not token/spend accounting;
   phase06 still owns actual provider bounds/integration. Persist exact limits at
   admission, never infer changed limits during retry or bundle reread.
2. **Root linkage and sole accounting owner.** A root owns one immutable scoped
   budget identity/limits row; a separate mutable usage row references that identity.
   This separation avoids the already-proven PostgreSQL repeated-UPDATE FK path
   from mutable counters into the root workflow_runs row. Each run has immutable
   tenant-scoped root-budget linkage, selected from itself for a root and from its
   existing parent's linkage for a descendant. Descendant limits cannot create a
   fresh allowance. Native scoped FKs and immutable guards enforce relationships,
   including direct SQL. Initial admission and all linkage are atomic. Applied
   migrations through20260928210000 remain untouched; new additive migration must
   account for retained admitted/activated fixtures rather than resetting data.
3. **One append-only reservation receipt owner.** Receipts are accounting facts,
   not jobs/attempts/results. Exact identity includes company, owning run, execution,
   resource kind and bounded logical reservation key; exact requested quantity is
   immutable. Equivalent replay returns saved disposition without another debit;
   altered quantity or mismatched scope conflicts without writes. Activation keys
   are internally derived from existing execution identity; model/repetition keys
   identify logical operations independently of worker attempts. Positive bounded
   quantities, checked arithmetic and SQL constraints prevent overflow/bypass.
   There is no refund, decrement, delete-while-owner-live, or limit-increase API.
   Both granted and budget-refused requests save immutable disposition receipts.
   Equivalent replay requires correct scope/payload and returns only the saved
   accounting fact, never authority; fresh reservations require live state/fence.
4. **Atomic reservation and lock discipline.** Application owns a narrow required
   reservation port with typed kind/key/quantity/result; SQL adapter locks the
   requesting run first, then its shared usage row, never any other run. Scope,
   active state, deadline and live existing lease fence are checked for scripted
   I/O callers. Debit and receipt commit together before work. A replayed receipt
   is not provider dispatch permission; phase06 must reuse saved results or apply
   its explicit recovery policy. Cancellation/lease loss cannot grant fresh work.
   Ambiguous consumed work remains charged across restart/retry.
5. **Runtime integration and exhaustion.** Existing first activation charges one
   root activation atomically with frozen inputs; replay/completed work never
   charges twice. Preserve the single shared completion/successor writer. Real
   queued/pure/I/O/wait activation paths must encounter the same root check.
   Model/repetition reservations are exercised through fenced scripted callers
   now; production handlers remain06/07. Exhaustion returns a typed durable refusal
   and settles the requesting execution/job/run through existing classified
   retirement, with a bounded audit code, so subsequent unchanged-time polls do
   not reclaim it. Do not cancel sibling/root runs by taking another run lock.
   Explicit operator retry cannot reopen budget exhaustion or replenish allowance;
   ordinary safe retry preserves all receipts and usage.

## Implementation seams

- domain/workflow/definition.rs38–43 and graph.rs66–84: limits/value validation;
  application/workflow/compiler/wire.rs292–370: source-located decoding, frozen
  bundle roundtrip/tests. Extend cohesive types, no parallel configuration owner.
- persistence/workflow/admission_write.rs29–67: atomic frozen limits/linkage;
  additive migrations: root identity/usage/receipts and scoped run association.
- activation.rs39–100, pending_recovery.rs25–78, batch_commit.rs5–107 and existing
  fenced retirement/control retry seams: prework charging, observable exhaustion.
  Trace callers before changes; update only affected shared behavior.
- New application/workflow budget port and cohesive persistence budget helpers/
  tests. background_tasks/task_attempts remain sole execution ownership.

## Acceptance and evidence gates

- Pure validation and compiler/restore tests: zero, platform boundary and one-over
  for each ceiling/quantity/key; explicit defaults and lower authored limits.
- Isolated real DB competing descendant reservations: exact shared maxima, one
  debit for duplicate contenders, same-key changed quantity conflict, foreign
  company/root/run/execution/fence refusal, no partial counter/receipt writes.
- Crash rollback before commit and saved replay after commit using fresh handles;
  ordinary retry keeps usage, ambiguous reservation never refunds, budget failure
  cannot operator-retry. Test cancellation/expiry and reservation competitors.
- Held parent/root run+execution row locks must not block child budget consumption
  or completion; retain unchanged existing2s regression. Root linkage immutable,
  native cross-tenant/wrong-parent rejection, owner cascade/direct mutation guards.
- Exact and one-over activation/model/repetition limits. Root activation charged
  across descendants and first activation only; pure/I/O/wait paths preserve
  max_steps, deadline, context/output bounds. Exhaustion is durable and second
  unchanged-time poll cannot reclaim poison. Reuse unaffected accepted boundary
  evidence, rerun affected cases plus all workflow:: tests at stock2MiB.
- Retained-row migration/schema integrity, SQLx prepare and --check (sequential
  with builds), locked offline all-target check, Clippy -D warnings, fmt,
  staged+unstaged whitespace, graft refresh. Separate nested Astra/medium actual
  code/callers/evidence review while implementer pauses edits; correct all findings
  and rerun affected gates before root acceptance. Full phase03/library acceptance
  remains a later root-owned gate.

## Ownership and startup evidence

Implementer /root/root_budgets, session01a0ebc4-c634-7583-833e-5295f694343e,
Astra/medium. Startup24,796/258,4009.6%2026-09-29T06:05:46.788Z;
expansion milestone71,118/258,40027.52%06:07:36.090Z, usage/runtime-window
sources via skill helper. No reviewer yet. Source baseline clean except preserved
website/index.html. No source edits or DB work performed during expansion.
TaskPG retained/stopped; use skill PostgreSQL reference and current sandbox rules.
Root owns PROGRESS/RESUME/acceptance; worker owns this brief/code/evidence/taskPG.

## Intermediate frozen-contract milestone — 2026-09-29 06:15Z

Implemented domain validated RootBudgetLimits/BudgetCharge and checked arithmetic,
optional strict YAML limits.root_budget, explicit frozen compiled budget values,
and compiler/publication boundary/roundtrip/tamper tests. Existing step/context
limits remain independent. No application reservation port, SQL, migration, live
accounting, activation/recovery integration or production handler enforcement yet.

Compiler SEMANTIC_REVISION advances1->2 as its existing contract requires for
new implicit defaults. Retained revision1 bundles fail closed explicitly; no
silent recompile or business-data upgrade. New fixtures publish revision2 and
must pass existing behavior suites. The durable tranche should backfill exact
activation consumption from persisted activated_at/execution identities and
initialize historical model/repetition at zero only where absence of consumers
proves none occurred; unknown consumption fails closed. Do not invent upgrade
machinery or alter applied migrations. TaskPG remains stopped, untouched.

Actual-code review06:14Z found no production issue; one test-only compile issue
(`unwrap_err` needs Debug on CompiledWorkflow) corrected to `.err().unwrap()`.
Initial focused and all-target builds failed on that same issue, logs retained
at/private/tmp/workflow-budgets-contract-{tests,check-initial}.log, not acceptance.
Final sequential checks running from/private/tmp/workflow-budgets-contract-checks.sh:
stock2MiB focused/domain/application tests, locked offline all-target check/Clippy,
fmt/bothdiff/graft. No queries changed, so SQLx/migration gates attach to next SQL
tranche; full workflow DB suite/combined03 gate still pending.

Reviewer /root/root_budgets/budget_reviewer session01a0ebc8-1198-70e3-a319-cc5e905902c1
Astra/medium, review55,896/258,40021.63%06:14:30.878Z usage/runtime-window;
parent verified56,866/258,40022.01%06:14:42.591Z token_count.info sources.
Implementer101,559/258,40039.3%06:15:05.033Z usage/runtime-window. Source edits
paused during independent review; corrected evidence review remains required.

### Verified intermediate handoff — 2026-09-29 06:19Z

Final sequential script/private/tmp/workflow-budgets-contract-checks.sh EXIT0.
Stock RUST_MIN_STACK=2097152, SQLX_OFFLINE=true, no skips:
focused4PASS0.08s, domain35PASS0.07s, application145PASS0.51s. Locked offline
all-target check40.76s, Clippy -D warnings53.21s, fmt,bothdiff,graft PASS. Logs
prefix/private/tmp/workflow-budgets-contract-: focused,domain,application,check,
clippy,fmt,diff,diff-staged,graft.log. SQLx/migration evidence not applicable yet:
no SQL/query/schema change. No test/build command remains running.

Independent actual-code review plus correction/final evidence acceptance PASS;
no unresolved findings. Reviewer59,675/258,40023.09%06:18:58.957Z usage/runtime
sources; quiescent. Implementer105,450/258,40040.81%06:18:36.045Z same sources,
final helper sent separately to root. Graft estimate3,078,603 tokens saved by
whole-file comparison (map dominates), not measured model-token savings.

NEXT worker implements durable tranche from contracts1–5/acceptance above:
application reservation port, additive scoped identity/usage/receipt schema,
admission freeze/linkage, activation debit, fenced scripted reservations, durable
exhaustion/retirement/retry refusal, then all DB/concurrency/SQLx/migration gates.
No reservation port was added in this intermediate fragment. Trace callers at
listed seams first; preserve single completion writer and root-lock separation.
Compiled defaults are now part of persisted identity; do not silently load old
revision1 bundles. Decide retained migrated runs' safe terminal/fail-closed
treatment in the additive migration without compatibility machinery or reset.
Full durable budget/03.9/fairness/phase03 acceptance remains PENDING.

TaskPG stopped and untouched throughout; migrations through20260928210000 remain
immutable. No stage/commit/reset/deploy. Preserve root PROGRESS.md and external
website/index.html changes. Only this brief and domain/compiler/publication files
listed above belong to this implementer; no application source port/runtime changed.

### Schema/accounting foundation checkpoint — 2026-09-29

Worker `/root/durable_budgets`, session01a0ebd2-5bb3-78c2-9f8b-90995bf1621a,
Astra/medium; nested reviewer `/root/durable_budgets/reviewer`,
session01a0ebd2-a4c8-78b2-a839-7e3195d4b2c2,Astra/medium. Latest worker
116,251/258,40044.99%06:36:05Z; reviewer64,712/258,40025.04%06:36:33Z,
token_usage_record.usage/task_started.model_context_window sources.

New additive20260929070000_workflow_root_budgets.sql establishes immutable scoped
root identity/frozen ceilings, immutable descendant linkage, separate mutable
usage referring only to immutable identity, and append-only exact receipts with
SQL-trigger debit. Admission derives explicit v2 compiled root ceilings from the
stored bundle, inherits descendant allowance, and commits all facts together.
Retained roots are sealed_legacy, limitsNULL: exact activated execution identities
backfill activation receipts/counts; previously absent model/repetition consumers
have provable0usage. Unknown lineage fails. No old bundle restoration/recompile,
fresh allowance, job/attempt owner or successor writer was introduced.

Independent review found and corrected BEFORE INSERT + ON CONFLICT double debit:
trigger locks requesting run, checks exact existing payload, then touches usage.
Actual SQL reproduction validates DO NOTHING and no-op DO UPDATE replay. Reviewer
also corrected a masked regression test (replay after exhaustion); final tests
exercise first duplicate contenders and replay while allowance remains. Native
parent FK trigger must run before budget lookup; final trigger name
workflow_root_budget_link preserves that ordering. Reviewer accepted final SQL,
tests and ordering; final execution/static evidence still pending below.

Five isolated actual-schema tests live in budget_schema_tests.rs, included under
wakeup_tests.rs. They cover3competing root/child/grandchild claimants across all
resources; exact limits/refusal; first duplicate+payload conflict with spare
allowance; scope/mutation/rollback/owner cascade;2child debits in one transaction
while parent run+execution locks remain held; retained migration reapply inside
rollback-only isolated DB transaction with exact historical accounting+sealed
refusal. Existing native FK and2s no-parent-completion regression retained.

Initial focused compile lifetime corrected; sandbox DB-connect denial rerun with
required escalation. Initial actual focused4PASS1fixture failure corrected by
using grandchild rather than admission-deduplicated same parent cause. Previous
broader run419PASS1native-FK assertion failure154.66s (ordering fixed afterwards),
log/private/tmp/workflow-budget-schema-workflow.log. This is NOT final acceptance.
Final sequential script/private/tmp/workflow-budget-schema-checks.sh session25005
runs corrected workflow:: stock2MiB, migrate/info, SQLxprepare/check, locked offline
alltargets/check/Clippy-Dwarnings,fmt,bothdiff,graft. Logs prefix
/private/tmp/workflow-budget-schema-, corrected suite suffix workflow-final.log.

NEXT after schema gate: application required reservation port and typed key/result,
fenced model/repetition adapter, first activation debit+durable refusal integration,
existing retirement work-budget exhaustion and explicit retry refusal. Schema
receipts currently remain accounting facts, NOT liveness/fence/dispatch permission.
Activation is NOT yet charged automatically. Complete original runtime acceptance
matrix (fence/deadline/cancel/crash/restart/retry and pure/I/O/wait integration),
then review/fullworkflow/SQLx/static gates. Fairness/fullphase03 still separate.
Preserve root PROGRESS and external website/index.html; no stage/commit/reset/deploy.
TaskPG running retained/private/tmp/workflow-admission-pg-e3aa port55439; ownership
may transfer running. Database had0runs/0activations at startup. Prior migration
max20260928210000 verified before final script; new migration becomes immutable
once final script applies it. Public schema not used for experimental reapplication.

Final schema checks: script25005 EXIT0; corrected420workflow tests PASS0skips
144.85s stock2MiB. Migration20260929070000 applied37ms (now immutable), SQLxprepare
17.67s/prepare-check14.72s PASS; locked offline alltargets check0.91s and strict
Clippy24.01s PASS;fmt,bothdiff,graft PASS. Exact commands in script and full logs
at the prefix above;schema-shape.log records actual four table definitions. No
.sqlx diff (runtime SQL only). Worker fresh123,864/258,40047.93%06:42:46Z
usage/runtime sources. Independent final evidence confirmation follows in root
record. New external website/assets/BB-logo1-transparent.png appeared during work;
preserved along with website/index.html. No task command remains running; PG
transfers running to root/replacement. Schema foundation complete; runtime and
original full durable-budget acceptance remain PENDING as listed above.

### Fenced reservation runtime milestone — 2026-09-29

Worker `/root/budget_runtime` session01a0ebe8-e3d1-7cf2-9675-1061c0ca513e,
Astra/medium; reviewer `/root/budget_runtime/runtime_review`
session01a0ebe9-2508-7da3-bc2a-ed2fc311b101,Astra/medium. Root approved bounded
sequencing: complete fenced model/repetition reservation and refusal first, then
first-activation integration with a fresh context after acceptance as needed.

Application required WorkflowBudgets port has validated stable key, typed charge,
request and Recorded/Replayed/NotOwned result. Activation charges cannot enter
this handler-facing port. Saved scoped receipts are accounting facts only, even
after cancellation/completion/lease loss; callers must revalidate ownership and
reuse results/apply effect recovery policy before dispatch. SQL adapter locks only
the requesting run, checks exact company/run/execution/job scope before replay,
checks live fence for fresh work and rechecks after the shared usage lock wait.
Debit/receipt/refusal retirement commit atomically. No refund or replacement job/
attempt owner. Exhaustion uses existing recovery with exhausted work budget;
safe read handlers fail durably, effect-capable handlers preserve reconciliation
for unknown prior work. No final_error continuation is scheduled on refusal.
Additive20260929080000_workflow_budget_retry.sql prevents explicit/direct-SQL retry
eligibility when that run has any immutable exhausted receipt. Ordinary safe
retry retains receipts and usage.

Reviewer initial P1 found forced-safe refusal could erase unresolved prior effects;
corrected to existing handler classification with an effect-capable regression.
P2 requested lock-wait/commit-failure/competing-outcome/descendant evidence; added
five tests alongside four initial tests. Correction actual-code review PASS,
no remaining findings; reviewer73,388/258,40028.40%06:55:27Z usage/runtime-window.
Initial4actualDB tests PASS1.52s stock2MiB. Subsequent9test sandbox run failed all
before DB connection (EPERM), log/private/tmp/workflow-budget-runtime-focused.log;
escalated rerun pending at focused-final.log. No skipped tests count as evidence.
Final checks pending; script/private/tmp/workflow-budget-runtime-checks.sh and logs
same prefix. Migration080000 is not immutable until retained DB application.

NEXT after this bounded gate: first activation charges across queued/pure/I/O/wait
paths (including standalone activation API), durable activation refusal, exact/
one-over activation across descendants, preserve max_steps/deadline/context and
shared progression writer, full remaining runtime matrix. Activation is STILL NOT
charged automatically here. Production handlers06/07, fairness and full03 combined
acceptance remain outside this milestone. Root owns queue acceptance/progress.

Final reservation gate: script35261EXIT0. Corrected9focused tests PASS0skips4.69s
(log focused-corrected.log);430workflow:: tests PASS0skips131.83s stock2MiB
(log workflow-final.log). New migration20260929080000 applied16ms and is NOW
IMMUTABLE. SQLxprepare44.06s/prepare-check14.53s, locked offline alltargets0.94s,
Clippy-Dwarnings60s,fmt,bothdiff,graft PASS. Exact sequential commands in script,
complete logs/private/tmp/workflow-budget-runtime-*.log. No .sqlx diff. Direct
pg_get_functiondef inspection confirms installed exhausted-receipt retry guard.
Initial expanded realDB run6PASS3fixture failures4.58s (focused-final.log) corrected:
effect fixture now existing memory.save (HTTP fixture required resource binding),
actual completed run state is succeeded, child admission keys distinguish parent
causes. Corrected focused and broader runs above include all cases and pass.

Independent initial+correction+fixture+final evidence review PASS, no unresolved
findings. Final reviewer78,779/258,40030.49%07:03:57Z usage/runtime-window sources;
reviewer quiescent. Parent latest root-verified44.32% before evidence append;
final self-measure sent to root. New runtime files: application/workflow/budget.rs,
persistence/workflow/budget.rs,budget_runtime_tests.rs,budget_race_tests.rs;
existing module exports, lease/recovery and control_tests wiring; additive retry
migration. Schema070000, frozen limits/compiler and all prior changes preserved.
External actor staged all budget/prior/website changes during checks; this subtree
issued NO stage/commit/reset/deploy. Preserve index as found. This final brief
append may be unstaged beside externally staged earlier brief content.

TaskPG transfers RUNNING retained/private/tmp/workflow-admission-pg-e3aa,
postgres://mac03@127.0.0.1:55439/workflow_admission (both URLs),max_connections200.
No task command/build remains running. Root accepted early rotation after this
bounded gate; do not assign this subtree activation work.

Fresh worker starting activation should trace activate_on callers (activation API,
pending_recovery,completion,recovery route_failure,wait_commit,wait signal).
Existing pending_recovery::activate owns pending invalid-input/per-run-limit
settlement; completion/recovery/wait paths reuse already frozen executions and
must not charge twice. Current activation API returns AppResult<ActivatedExecution>,
so choose an explicit typed internal refusal boundary that allows the standalone
API to COMMIT durable refusal before returning it, and lets pending paths settle
without matching error strings or inventing a second writer. Existing activation
limit/deadline failures must retain behavior. Handler-facing WorkflowBudgets API
intentionally rejects Activation; runtime owns the fixed activation key/quantity.
Its SQL helpers currently only implement model/repetition; reuse schema accounting
semantics and keep no-root-lock discipline. No other work remains in the accepted
reservation milestone; all activation/fairness/full03 gates remain as stated above.

### Activation integration in progress — 2026-09-29 07:10Z

Worker `/root/budget_activation`, session01a0ebfb-b949-7a60-8593-7a66728ee417,
Astra/medium; sole reviewer `/root/budget_activation/reviewer`,
session01a0ebfb-fbcf-7c42-942a-ed59eb6dba85,Astra/medium. Latest worker31.36%
81,041/258,40007:10:11Z usage/runtime-window. No approval or scope changes.

Actual caller reconciliation: pending pure batches, I/O claims and wait parking
share `pending_recovery::activate`; standalone `WorkflowActivation::activate`
now consumes an internal optional result and commits before exposing exhaustion
as Conflict. `None` represents durable budget refusal; no error-string dispatch.
Completion/recovery/wait resumption expect saved activation. Fresh activation
requires active requesting run and pending unowned job; fixed activation receipt
commits with frozen inputs, and a post-usage-lock deadline check rolls back both
receipt and debit if time expires. Shared pending retirement retires the job/run
without creating attempts, spending a final_error activation or touching ancestors.
No schema migration added; existing retry receipt guard applies unchanged.

Source edits paused for independent initial actual-code review. Four new tests
cover competing root/child/grandchild exact allowance, replay, rollback/fresh
handle, standalone and pure/I/O/wait refusals/no-hot-poll, usage-wait deadline.
Initial alltargets and focused compile failed, not evidence: async retirement
cycle requires Box::pin, and test poll_work takes cursor rather than company.
Both known mechanical fixes await review completion. Logs
/private/tmp/workflow-budget-activation-{check-initial,focused}.log.
Remaining matrix/fullworkflow/SQLx/static/combined-review gates still pending.

Activation code/correction review PASS, no unresolved findings. Actual source
review covers combined frozen limits/schema/reservation/activation interaction;
reviewer89,724/258,40034.72%07:21:28.974Z usage/runtime-window. Root activation
refusal uses boxed existing pending settlement to bound its saved-activation
async cycle, with no raised stack limit or second progression writer.

Corrected9activation tests PASS0skips3.41s stock2MiB, log
/private/tmp/workflow-budget-activation-focused-verified.log. Added successful
I/O/timer completion and replay accounting, prior-result reuse after mid-batch
root exhaustion (error continuation forbidden), two held ancestor run/execution
locks during fresh grandchild activation+completion, cancellation-winning
activation, and deferred commit failure for BOTH grant and refusal. Existing
activation tests now assert usage+receipts around duplicate claimants, rollback,
statement failure, scope/input/output/step/deadline boundaries. Existing fenced
reservation tests account for the new activation receipt and select model facts
by exact resource/key instead of row position.

Initial compile errors and sandbox EPERM were not runtime evidence. First real
DB run7PASS1test-only unwrap expectation failure3.22s (focused-db.log), corrected
to reject replay of refused uncompleted work while reusing prior completed work;
timer assertions now require Committed then Replayed. Production review found
no additional defect. Broad sequential verification is RUNNING session72398,
script/private/tmp/workflow-budget-activation-checks.sh, logs same prefix.
No new migration; applied070000/080000 remain immutable. Full durable budget
acceptance awaits that evidence and reviewer final confirmation; fairness and
fullphase03 still outside this worker's scope.

### Verified durable-budget integration handoff — 2026-09-29 07:26Z

Final sequential script72398EXIT0; exact commands in
/private/tmp/workflow-budget-activation-checks.sh.9focused tests PASS0skips3.41s;
439workflow:: tests PASS0skips138.11s at stock RUST_MIN_STACK=2097152. Logs prefix
/private/tmp/workflow-budget-activation-: focused-verified,workflow-final,migrate,
migrate-info,prepare,prepare-check,check,clippy,fmt,diff,diff-staged,graft.log.
Migration/info PASS(no new migrations); SQLxprepare19.45s/prepare-check18.15s,
locked offline alltargets0.54s, strictClippy29.31s,fmt,bothdiff,graft PASS. No.sqlx
diff. All previous schema/reservation tests and unchanged2s parent-lock regression
included. Earlier accepted frozen/schema/reservation evidence remains applicable.

Independent actual-code initial/correction/final combined durable-budget review
PASS; no unresolved findings. Final reviewer92,420/258,40035.77%07:26:30.836Z
usage/runtime-window; reviewer now quiescent. Worker latest helper112,742/258,400
43.63%07:26:10.762Z; root refreshed44.65% before final append, final self-measure
sent separately. Source integration, all budget acceptance matrix and scoped
verification complete; root owns queue acceptance. Production handlers06/07,
fairness/globalcapacity, and fullphase03/library acceptance remain separate.

No task commands/builds running. PG transfers RUNNING retained
/private/tmp/workflow-admission-pg-e3aa,port55439,max_connections200; both URLs
postgres://mac03@127.0.0.1:55439/workflow_admission. Applied070000/080000 unchanged.
No stage/unstage/commit/reset/deploy; externally staged source and website files
preserved. New budget_activation_tests.rs remains untracked by design. This
worker is quiescent after handoff; next root assignment should use fresh workers.
Graft reported~2.85M whole-file-baseline tokens saved, not measured model savings.
