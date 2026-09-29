# 03.9 — global capacity and sustained tenant fairness

Status: contract reconciliation for root coverage acceptance; no source edits yet.
Authority: 03 original acceptance, BRIEF-03.9 contracts4–5 and matrix5–7.
Budgets through20260929080000 verified and immutable; no reopening without defect.

## Cohesive contract

- Existing background_tasks/task_attempts remain the sole ownership/attempt ledger.
  At lease_claim::claim_on (after lock_scope locks run/execution/job), serialize
  admission to I/O against one PostgreSQL singleton scheduler-policy row. Count
  live workflow processing leases belonging to active, unexpired runs, then install
  the existing fence/attempt in that transaction. No in-memory semaphore or second
  slot ledger. Every direct claim goes through this same check; renewal cannot
  revive an expired lease. Completion/wait/retirement need no capacity release write.
- One database-wide immutable bounded policy is shared by all workers; initial
  explicit typed configuration is insert-once and mismatches refuse. The database
  stores defaults when first used without explicit configuration. No environment
  variables or phase08 startup wiring are invented. Schema rejects malformed limits
  and mutation of installed policy. Defaults and policy API finalized at acceptance.
- Fair worker discovery is a separate transaction that acquires scheduler metadata
  only, never a run/execution/job row lock. Each call picks the next eligible company
  in stable cyclic UUID order and returns at most one bounded page for that company.
  A durable per-company job UUID cursor traverses unchanged unsupported/poison
  pages and wraps. Company cursor advances even if candidates cannot be dispatched.
  The worker consumes this fair discovery port; existing raw keyset discovery can
  remain explicitly diagnostic/recovery-test API, never the production worker loop.
  Metadata stores turns/cursors only; loss/restart acknowledges no work.
- With N continuously eligible companies and bounded page P, every company is
  visited within N successful discovery calls, irrespective of replenishment by
  any existing tenant. Its jobs advance by at most P per visit. Unsupported/wait/
  poison tenants cannot monopolize company visits. Per-company capacity strictly
  below global leaves other tenants room. Existing local worker is one operation;
  operation deadline/cancellation bounds hung handling. Fairness is measured in
  discovery/dispatch turns, with wall time also bounded by existing I/O/poll limits.
- Lock order: own run -> execution/job -> budget (if activation) -> capacity row.
  Discovery transaction releases coordination before any run transition. No claim
  takes scheduler coordination then another run, and metadata must not introduce
  ancestor FK rechecks into a run-owning transition. Short statement/lock timeouts
  retain existing bounded error backoff. Active-history predicates precede counts.

## Files and acceptance

Known seams: persistence/workflow/lease_claim.rs5–69; lease.rs26–42,99–133;
polling.rs15–63; application/workflow/worker.rs44–85,132–157; polling.rs55–63.
Add cohesive scheduler module/port and additive migration, isolated DB test siblings.
Trace callers and preserve existing raw polling evidence unless API must change.

1. Real competing distinct-run claims across worker handles hit exact global and
   per-company ceilings; observe durable active maxima and actual handler maxima.
   Direct claim, fresh handles/restart and mismatched policy cannot bypass limits.
2. Sustained replenishment with multiple workers and quieter eligible tenants:
   record company visits/dispatches and bounded quiet progress, not only final totals.
   Include full unsupported pages, poison, parked work and hung handler; no timing-only
   correctness assertions. New noise must persist throughout the quiet progress test.
3. Completion/terminal/wait and lease expiry free capacity; restart retirement
   charges exactly one attempt and old fence cannot renew/commit. Contested claims
   retain attempt/frozen activation semantics when denied. No scheduler/run lock
   inversion: hold a run and prove discovery remains independent; competing claim
   with held coordination cannot hold another run on behalf of discovery.
4. Migration/schema malformed and immutable policy checks; metadata rollback and
   company deletion integrity; representative EXPLAIN for bounded eligible discovery
   and lease count with retained terminal history. No bounds raised.
5. Focused realDB tests and all workflow tests at stock2MiB, migration/schema,
   sequential SQLx prepare/check, locked offline all-target check/Clippy, fmt,
   both whitespace diffs, graft build; independent actual-code/correction/evidence
   review with edits paused. Root acceptance before combined full03 library gate.

## Ownership and preservation

Implementer /root/fairness_capacity session01a0ec10-6214-77b0-9a56-e6bb66210f26;
sole reviewer /root/fairness_capacity/reviewer session01a0ec10-a7d1-7ef2-ad0b-ac98a3291a8f,
Astra/medium. Startup9.65%/9.39% of258400, usage/runtime sources. Root owns
PROGRESS/RESUME. All existing staged/unstaged/untracked work preserved, including
external website files. No stage/commit/reset/deploy. Baseline status saved at
/private/tmp/workflow-fairness-status-before.log. TaskPG retained running at
127.0.0.1:55439, workflow_admission, /private/tmp/workflow-admission-pg-e3aa;
both task database URLs explicit, no reset. All migrations through080000 immutable.

## Arbitration amendment — pending root/design review

Discovery rotation alone cannot prove dispatch progress when at least eight noisy
companies refill global16 before a quiet company claims. Add company demand turns
at the universal claim boundary, not job reservations:

- After successful existing activation (run/execution/budget owned), acquire the
  singleton scheduler lock. A capacity-denied claim creates no attempt; activation
  debit remains the existing durable debit-once receipt. Register/refresh one demand
  per company, with a monotonic ticket, a job UUID eligibility hint and a bounded
  demand expiry. Existing unserved ticket survives refresh/expiry; only successful
  admission clears it and subsequent demand obtains a later ticket.
- Count live owners after obtaining the singleton lock in a separate READ COMMITTED
  statement. Among demand companies below their per-company limit, select the oldest
  ticket whose hinted existing job is pending/due and whose execution/run is live.
  Only that company may install a new I/O lease. A newer noisy caller cannot steal
  capacity ahead of an already registered eligible quiet company. Several available
  slots may admit other companies only after older demand is served or expires.
- Demand is not ownership: it grants no job/attempt, never acknowledges work and does
  not authorize I/O. A job UUID is only a revalidated eligibility hint. A different
  eligible job of that company may consume its turn. Cancellation/parking/terminal
  transition immediately makes its hint ineligible. An abandoned or newly unsupported
  demand expires after a fixed bounded interval; retry reactivates its original
  unserved ticket so long poll intervals cannot repeatedly push quiet work to the
  back. No new ticket on each denial. Poison activation registers no demand.
- Demand/cursor metadata uses company/job UUID hints without ancestor FKs, just like
  the singleton discovery cursor. Scope authority is proved exclusively by the real
  scoped run/execution/job and existing FKs, before demand registration. Therefore
  metadata insertion/refresh inside a run-owning transaction cannot lock companies
  or another run via FK rechecks. Deleted-company hints are inert and can be removed
  by bounded metadata cleanup; they never own capacity or block live demand.
- Deterministic fairness bound: after quiet demand registers, at most the finite
  earlier eligible company tickets can be admitted before it; each admission clears
  that ticket, and continuously replenished noise receives later tickets. Expired
  unsupported demand is skipped; retained old ticket matters only on fresh claim.
  Fair discovery ensures quiet requests are retried. Test actual dispatch and exact
  admission order with full capacity, queued quiet demand, continuously replenished
  noise, multiple worker handles and controlled handler gates.

Defaults accepted by root: global16/company2; validator global>=2, company>=1 and
company<global (reserve room for another tenant), explicit upper bound; defaults
are not deployed phase08 configuration. Raw polling remains diagnostic/recovery;
worker must use fair discovery. Late uncancelled external effects are not live
lease ownership and remain guarded by actual-future cancellation/effect contracts.

## Capacity-only fragment — implementation underway

Root authorized splitting verified capacity from unresolved fairness2026-09-29.
Do NOT implement or claim the demand-expiry amendment above: reviewer found it
incorrect. Counterexample: quiet demand registers while full, expires before each
release, then retries after noise refills; retained ticket never dispatches. Fairness
needs priority discovery and a correct unsupported/capability disposition or another
enforced service opportunity. Next fragment must resolve this before fairness code.

Second design finding fixed in capacity implementation: uncommitted renewal can
extend before old expiry, stay invisible across that expiry, and otherwise permit
overbooking. Renewals now acquire the same singleton after own run/fence locks,
then freshly validate expiry/update, retaining coordination through commit.

Capacity-only files: application/workflow/capacity.rs and mod; persistence/workflow/
capacity.rs,lease_claim.rs,lease.rs,mod; additive20260929090000_workflow_capacity.sql
APPLIED and IMMUTABLE; capacity_tests.rs/capacity_renew_tests.rs linked under lease
and small shared fixture support in tests.rs/binding_tests.rs. No fairness code or
production startup wiring. Default16/2, maximum1024, strict company<global; insert-
once explicit policy with identical replay and mismatched configuration refusal.
Denied claims retain only existing debit-once activation; create no attempt.

Focused tests pending: competing9distinct-run claims across3companies against4/2,
policy race/schema immutable guards, expiry/restart/fence/attempt preservation,
and actual public renewal gated after UPDATE before commit: pg_blocking_pids proves
renewal waits on test gate and competing claim waits on renewal after OLD expiry.
No timing-only inference. Logs /private/tmp/workflow-capacity-focused.log.
Source edits pause for independent actual-code review after initial test corrections.

### Capacity correction review and gates — 2026-09-29 07:46Z

Independent actual-code review found no production defect. Two evidence findings
resolved: denied activation now checks exact root usage/receipt debit1 and unchanged
accounting after repeated denial AND eventual claim; additional schema test rejects
malformed singleton/global/company limits and proves omitted-config default16/2
installation plus mismatched explicit configuration refusal. Reviewer inspected
corrections and returned PASS07:45:51Z,83030/25840032.13%, usage/runtime sources.
Actual-code initial review07:42:12Z77329/25840029.93%. Edits paused during both.

Initial focused compile1m30s succeeded; runtime0/4 because sandbox denied taskPG
connections. Proper escalated rerun3PASS1fixtureFAIL1.37s: new tenant fixture target
workflow_id differed from copied source. Source now names the new workflow ID.
Corrected focused5/5PASS1.40s at stock2MiB; budget and schema corrections included.
Logs /private/tmp/workflow-capacity-focused{,-authorized,-corrected}.log preserve
all outcomes. No failed run counted as acceptance. Schema inspection PASS via
required sandbox escalation, /private/tmp/workflow-capacity-schema.log.

Sixth test shares production ACTIVE_COUNTS_SQL, creates256terminal histories and
one live owner, asserts exact1/1 and prints EXPLAIN(ANALYZE,BUFFERS). Correction
review confirms SQL extraction unchanged; runtime evidence pending. No index/bound
changes. Sequential gate script/private/tmp/workflow-capacity-checks.sh currently
running. SQLx prepare PASS42.72s; remaining gates pending. Logs all share prefix
/private/tmp/workflow-capacity-: migrations,sqlx-prepare,focused-final,integration,
sqlx-check,check,clippy,fmt,diff,diff-staged,graft.log. Both database URLs explicit;
RUST_MIN_STACK2097152 for all tests; no skipped DB tests.

Implementer latest107824/25840041.73%07:46:11Z. Complete this capacity checkpoint
and rotate before unresolved fairness; do not infer full03 or fairness acceptance.

### Capacity gate results — 2026-09-29 07:53Z

- Final focused6/6PASS6.18s, allworkflow445/445PASS148.63s, no skips, stock2MiB.
  Includes real9-claimant/3-company competing admission and actual uncommitted renewal
  across old expiry, denied budget debit-once, expiry retirement/fence/restart and
  first-use/explicit immutable policy checks. Full03 library gate remains later.
- Actual count query EXPLAIN uses existing background_tasks_processing_lease_idx,
  one live job among256terminal histories,17shared buffers,0.034ms execution. The
  small execution table chooses a seqscan filtering completed rows; no index added
  without production evidence. Full plan in focused-final.log.
- Fresh isolated test databases apply all migrations; retained task schema/info
  PASS, migration090000 immutable. SQLxprepare42.72s and SQLxcheck16.09s PASS.
- Initial offlinecheck0.90s PASS; Clippy found one useless test String.into identity
  conversion. Removed only that conversion in tests.rs78; no semantic/runtime/SQL
  change. Corrected check/Clippy46.09s PASS, then fmt,bothdiff,graft PASS; exact
  static-corrected.sh EXIT0. Unaffected test/SQLx evidence reused appropriately.
- No prior status path removed or staging column changed. Both external website
  files retain original staged status. Comparison logs retained. No stage/commit/
  reset/deploy. TaskPG remains RUNNING/data retained; no active build/test job.

Independent final correction/evidence review pending at this record. Reviewer already
returned actual-code and correction PASS; final packet sent07:53Z. Root owns acceptance
and progress. Capacity-only complete when that gate returns; no fairness implementation,
no actual-handler sustained mixed-tenant maximum/progress proof, no full03 acceptance.
Next worker must resolve expired-demand fairness counterexample BEFORE adding demand
metadata. Capacity owners serialize renewal as well as claims; do not lose this fix.

### Capacity final handoff — 2026-09-29 07:54Z

Independent final evidence PASS, no unresolved capacity correctness findings.
Reviewer explicitly qualifies EXPLAIN: existing processing lease index prunes jobs,
but the small execution relation is scanned/filter-checked; this is NOT proof of
history-independent database work. No performance/index guarantee is claimed.
Reviewer session01a0ec10-a7d1-7ef2-ad0b-ac98a3291a8f, latest reported87634/258400
33.91%07:52:51.408Z, usage/runtime sources, now quiescent. Owner refreshed samples
may be slightly later/higher. Implementer116965/25840045.27%07:53:07.700Z same
sources, retiring at this cohesive root-authorized boundary. No further assignment.
All commands complete and reviewer stopped; taskPG remains running for transfer.

Root can accept CAPACITY ONLY. Remaining03.9: robust sustained actual dispatch
fairness under many replenished tenants, heterogeneous unsupported handlers/poison/
hung work, bounded prioritized discovery and stale-demand handling; then root
acceptance and combined full03 library gate. Counterexample and lock constraints
above are mandatory inputs, not an accepted algorithm. No04 work begun.

## Replacement fairness design — independent design review 2026-09-29

The original shared discovery cursor and expiring-demand amendments above are
REJECTED. Shared discovery can phase-lock complementary heterogeneous workers so
neither sees supported work; ticket expiry while full permits indefinite refill
starvation. Capacity-only code remains accepted and unchanged pending this design.

Replacement: per-worker discovery plus persistent company demand and a protected
free-capacity service opportunity. Background jobs/attempts remain sole ownership.

- Discovery keeps independent worker company and worker/company job cursors. Each
  worker cyclically visits every eligible company, one bounded page per call. Job
  scans traverse a finite created-at horizon before beginning the next epoch, so
  replenishment cannot insert endlessly ahead of existing supported work. No run
  lock is taken while discovery holds scheduler metadata. Cursor metadata has no
  ownership authority or ancestor FK lock acquisition; stale cleanup is bounded.
- A supported I/O claim, after existing activation and own run locks, registers
  one company demand under the existing capacity singleton: monotonic ticket,
  requester worker, scoped job hint, optional opportunity deadline. An existing
  demand retains its requester and ticket; later callers cannot overwrite it.
- While global capacity or that company's capacity is full, demand never expires.
  Once BOTH ceilings have room, the oldest currently eligible company receives a
  protected opportunity: all newer claims are denied until it succeeds, becomes
  ineligible, or the opportunity deadline passes. Only then does timeout begin.
  Successful admission removes demand. Invalid/expired removal compares ticket;
  expired abandoned requesters rejoin at the tail on retry. No slot ledger exists.
- A capacity-denied requester immediately stops its current page. Its next poll
  returns only its outstanding candidate, preventing other handlers from delaying
  retry. Local unsupported disposition may withdraw only that worker's own demand;
  another heterogeneous worker cannot invalidate it. Fresh workers have independent
  discovery and recover abandoned demand after a bounded free-capacity opportunity.
- The opportunity duration must exceed the enforced maximum poll interval (30s)
  plus discovery/kind/claim operation bounds (2s each), with explicit margin. The
  progress claim applies to a responsive worker/DB completing that bounded retry;
  indefinite suspension or repeated DB failure cannot guarantee liveness. Waiting
  full consumes none of this opportunity. With N earlier eligible tickets, quiet
  supported demand gets its protected service opportunity after at most N prior
  admissions/opportunity expirations; replenished noise obtains later tickets.
  Capacity release cannot be stolen while quiet is between bounded polls.
- All claims and renewals retain own-run-first singleton serialization and fresh
  lease counts; discovery never locks another run. Unsupported, invalid and parked
  hints do not own attempts/capacity; dead requester blocking lasts one opportunity.

Required deterministic evidence: full capacity retained longer than opportunity
then release immediately after quiet retry; deny replenished noise until quiet
actual dispatch; complementary-support workers alternate independent discovery;
stop remainder of page on denial; unsupported peer cannot erase demand; company
full/global free starts no timer; abandoned opportunity recovery and stale removal
race; fresh worker/expired claim, live maxima, poison/hung/parked coverage. No
conveniently aligned sleeps may substitute for controlled state/handler gates.

Design reviewer /root/tenant_fairness/reviewer, session
01a0ec28-b8ea-7bf2-8ed6-007698ef764a, Astra/medium, design conditions returned;
actual-code review remains mandatory after implementation. Implementer session
01a0ec28-65ae-78b1-a2d1-10d03ef91307. Root acceptance of replacement design pending;
no fairness source or migrations changed at this checkpoint.

## Fairness core handoff — 2026-09-29 08:16Z

Status: IMPLEMENTED, FOCUSED-TESTED, NOT ACCEPTED. Root accepted the replacement
bounded-responsive service contract before source edits. Full original03.9 fairness
matrix and broad gates remain below; do not mark03.9 or full03 verified.

Current source:
- application/workflow/polling.rs adds WorkflowFairPolling, FairPollPage and typed
  DemandTicket; raw poll_work explicitly diagnostic/recovery. FAIR_OPPORTUNITY45s
  is maxpoll30 + discovery2 + kindlookup2 + claim1 + responsiveness margin10.
- application/workflow/worker.rs uses fair discovery, immediately breaks current
  page on denied claim (WorkDisposition::Deferred), retries requester sticky demand,
  withdraws capability-loss demand only by its worker+ticket receipt CAS.
  Heterogeneous capability support is preserved. Handler.supports must be pure and
  nonblocking. Actual I/O retains existing60s bound/supervision/cancellation.
- persistence/workflow/fair_polling.rs: independent worker company cursor and
  worker/company job cursor; strict created_at<horizon UUID traversal; no run locks.
  Durable sticky hinted candidate returns alone. Metadata cleanup locks stale worker
  parents FOR UPDATE SKIP LOCKED then deletes at most32children, then empty parents;
  one-day age greatly exceeds responsive60s handler+30s poll. No ancestor company FKs.
- persistence/workflow/fairness.rs: universal claim admission under SAME capacity
  singleton AFTER own run/execution/budget. Demand company PK, unique requester,
  monotonic ticket, scoped hint, optional opportunity. No expiry while either cap
  full. Existing offered ticket precedes newly eligible older tickets; successful
  company admission consumes ticket. Expired offer removed, next retry joins tail.
  Invalid hints excluded and cleaned32at a time, ALWAYS offered invalid row first.
- capacity.rs factors bounded LIVE_COMPANIES_SQL; renew still locks singleton after
  own run and freshly revalidates expiry, unchanged. lease_claim.rs calls fairness
  admission universally. Existing capacity count EXPLAIN test now uses actual new
  production LIVE_COMPANIES_SQL; runtime evidence must be rerun, old plan is stale.
- polling.rs shares unchanged eligibility SQL between raw diagnostic and fair query.
- migration20260929100000_workflow_fairness.sql APPLIED57.8ms and IMMUTABLE. Adds
  demand and worker/cursor metadata; partial UNIQUE index enforces ONE offered row.
  Applied predecessors through090000 also immutable. No ownership/attempt queue added.
- New tests fairness_tests.rs under capacity_tests and fairness_worker_tests.rs below
  it. No production startup/provider/model wiring. No bounds raised.

Independent actual-code review with ALL source edits paused:
1. Found offered quiet ticket could be preempted when older capped tenant uncapped;
   corrected offered-first ordering + unique offered index + deterministic regression.
2. Found stale worker cleanup could deadlock mutually reviving workers; corrected
   SKIP LOCKED parent-before-child. Competing revival test passes; deterministic held-
   parent gate strengthening still recommended below.
3. Found >32older invalid rows before invalid offered row could cause one-offer
   unique violation and rollback cleanup forever. Corrected invalid offered-first
   bounded cleanup; regression seeds33schema-valid historical orphan hints ahead of
   invalid offered row, opens/ages abandoned next offer and proves newer claim proceeds.
4. Horizon regression initially could pass due to other company/UUID cursor. Fixed
   explicit company selection and after_job reset, asserts target nonempty epoch,
   exact timestamp exclusion and next-epoch inclusion.
Final LIMITED correction-code PASS, no unresolved reviewed production findings;
reviewer session01a0ec28-b8ea-7bf2-8ed6-007698ef764a, Astra/medium,101704/258400
39.36%08:15:14Z usage/runtime sources, now QUIESCENT. Full acceptance NOT reviewed.

Evidence provenance (all logs/private/tmp):
- workflow-fairness-check-initial.log offlinealltargets PASS50.69s before ticket
  receipt change. check-receipt.log PASS47.63s before later corrections/tests.
  These are NOT current final static gates.
- focused.log initial4compiled80s but0/4runtime: sandbox DB PermissionDenied. Proper
  escalated rerun, no bypass: focused-authorized.log5/5PASS2.45s, compile42.67s.
- focused-expanded.log7/7PASS2.54s, compile40.63s, BEFORE invalid-offer correction.
- focused-corrected.log CURRENT8/8PASS2.68s, compile39.93s, stock2MiB, no skips.
  Exact command: env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true
  RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_fairness -- --nocapture
  Runtime requires proper sandbox escalation. All tool sessions finished.
- Eight tests cover full waiting age without premature expiry; protected quiet retry;
  older capped tenant uncap cannot preempt; abandoned opportunity/ticket CAS; independent
  worker traversal + exact timestamp horizon + competing stale revival; sustained
  actual gated dispatch of3quiet tenants while3noise companies repeatedly replenish,
  exact observed actual/durable maxima2/1; complementary capability actual dispatch;
  real worker.run stops128page at first denial and sticky retry activates only1of8;
  plus invalid offered cleanup regression (some properties share tests).
  The long full wait is represented by aging registered_at beyond45s under controlled
  capacity, not a45s sleep. Noise replenishment continues during each quiet invocation.
- workflow-fairness-schema.log inspected all3new tables/indexes via authorized psql;
  no malformed-schema/rollback/deletion matrix yet. Both whitespace diffs passed before
  last correction (rerun final). fmt has run on latest source; final --check pending.
- workflow-fairness-status-current.log captured status. No stage/commit/reset/deploy;
  all prior staged/unstaged/untracked preserved, including both external website files.

NEXT WORKER — remaining exact acceptance groups, then broad gates:
A. Strengthen integrated fair *discovery* + sustained actual dispatch (current gated
   sustained test drives actual process calls; separate run-loop test proves sticky
   page break). Cover heterogeneous unsupported full pages, poison/backoff, parked
   work and hung actual future mixed with quiet/noisy load. Reuse existing established
   tests where unchanged, but establish combined fairness evidence rather than assume.
B. Deterministic run-lock independence and singleton/run order under competing claim/
   renewal. Existing capacity renewal regression must rerun unchanged. Strengthen
   stale-worker revival with both parent locks held via explicit gates, not barrier only.
C. Schema constraints, scheduling metadata transaction rollback, actual company deletion/
   orphan handling (current invalid-hint regression uses schema-valid orphan metadata),
   fresh worker restart and expired claims preserving attempts/activation budgets.
   Audit cursor metadata retention for active workers across deleted-company churn.
D. Representative EXPLAIN(ANALYZE,BUFFERS) of fair eligible discovery and new production
   LIVE_COMPANIES_SQL with terminal history; status/time pruning before joins/aggregation.
   No index/performance guarantee from old capacity plan; add indexes only with evidence.
E. All focused tests and workflow suite at stock2MiB. Old capacity competing test selects
   first denied array element for eventual claim; fair ticket ordering may require
   updating that expectation to oldest queued demand, preserving maxima/budget checks.
   Do not add compatibility bypass merely to keep old direct-claim ordering assertions.
F. Migration info/schema, sequential SQLxprepare/check, locked offlinealltargets,
   strictClippy, fmtcheck, bothdiff, graftbuild and independent final actual-code/evidence
   review. Source pauses during reviews; root accepts03.9 before combined full03 library
   gate (later separate assignment). No04+ work.

TaskPG remains RUNNING/data retained/private/tmp/workflow-admission-pg-e3aa, host
127.0.0.1 port55439, databaseworkflow_admission, both explicit URLs above, maxconn200.
Current sandbox workspace-write/networkrestricted; oldFullAccessnotes stale. No active
build/test/approval at checkpoint. No database reset. Root owns PROGRESS/RESUME.
Implementer01a0ec28-65ae-78b1-a2d1-10d03ef91307 latest118995/25840046.05%08:14:56Z,
usage/runtime sources, taking cohesive rotation before50%; final refreshed sample follows.

## Acceptance expansion and current evidence — 2026-09-29 08:30Z

Implementer /root/fairness_acceptance session01a0ec3e-15e9-7701-9acb-809cd8c20d03;
sole reviewer /root/fairness_acceptance/reviewer session01a0ec3e-5420-7690-a1c8-4353e47c16cd.
Both Astra/medium; startup9.69%/9.43% of258400 verified usage/runtime sources.
Root owns PROGRESS/RESUME. Applied migrations remain immutable; taskPG retained.

A–F expansion executed as follows (acceptance still pending final gates):
- A: new fairness_mixed_tests real worker.run with page2 traverses4unsupported
  memory jobs, terminal poison and parked future timer; two actual gated owners
  saturate2/1, quiet is discovered and registers demand,8noise arrivals before
  release and8during quiet handler cannot steal room. One actual owner remains
  hung throughout; quiet actual future is cancelled on shutdown. Existing3quiet
  sustained-dispatch and complementary capability tests retained.
- B: new held run+capacity discovery independence; pg_blocking_pids establishes
  claimant waits on run while another company claims; two explicitly held stale
  parents survive thirdworker cleanup. Original renewal regression unchanged.
- C: cursor/demand trigger faults roll back parent metadata/activation/accounting;
  cursor FK and single-offer unique schema guards; actual company deletion leaves
  inert demand and own active worker removes orphan cursor. Retention audit found
  active workers previously never cleaned deleted-company cursors: fixed bounded32
  own-worker cleanup under already-held worker parent, preserving lock order.
  Existing expiry/restart/old-fence/attempt and budget debit-once tests retained.
- D: existing256terminal history test now EXPLAINs actual LIVE_COMPANIES_SQL AND
  factored production fair company query. No indexes or bounds changed. Current
  plans show processing index for live counts; eligible status filters precede
  aggregation. Small relation execution scan remains; no history-independent
  database-work promise is made.
- E: capacity competing test chooses actual oldest denied ticket for eventual
  claim, preserving exact maxima, attempt totals, budget/receipt assertions.
- F: sequential script/private/tmp/workflow-fairness-gates.sh prepared for schema,
  migration, SQLxprepare, workflow suite stock2MiB, SQLxcheck, offlinealltargets,
  strictClippy, fmt, bothdiff and graft. Final review remains mandatory.

Evidence to date: acceptance-initial.log12/12PASS3.22s (first lock/deletion tests).
acceptance-expanded.log20PASS1FAIL17.10s; reviewer found mixed fixture startup race
and missing observed unsupported traversal. Fixed semaphore startup after owner
receipts + supports counter>=4 before quiet admission; correction-codePASS.
acceptance-corrected.log20PASS1FAIL17.03s identified poison fixture input={} was
valid for literal-input registry example. Corrected poison to original fixture's
/input/value binding. workflow-fairness-focused-final.log now running; no failed
run counted as acceptance. All logs under/private/tmp. Source paused for actual
independent reviews; no production defect found in review so far.

### Final acceptance checks — 2026-09-29 08:38Z

- Corrected focused21/21PASS8.74s stock2MiB; logworkflow-fairness-focused-final.log.
  Reviewer combined actual-codePASS08:31:15Z,98421/25840038.09%; no unresolved findings.
- Allworkflow460/460PASS141.01s stock2MiB, no skipped DB tests; integration-final.log.
  Both database URLs target retained taskPG explicitly. No full03 library claim yet.
- Migration info and actual3table schemaPASS; sqlxprepare18.03s and sqlxcheck16.91s
  PASS. Logsworkflow-fairness-{migrations,schema,sqlx-prepare,sqlx-check}-final.log.
- Initial strictClippy found2collapsible-if warnings in worker demand withdrawal.
  Replaced nested conditions with equivalent short-circuit let-chain; no policy,
  SQL, scope, timeout or cancellation semantics changed. Original failed log retained.
  Corrected offlinealltargets and strictClippyPASS (Clippy53.74s). Affected fairness
  rerun15/15PASS3.60s stock2MiB; focused-corrected-final.log. Unaffected460workflow/
  SQLx evidence reused after this syntax-only edit. Final correction review pending.
- /private/tmp/workflow-fairness-static-corrected.sh EXIT0: fmtcheck, both whitespace
  diffs and graftbuildPASS. Logs use workflow-fairness-*-final.log except corrected
  check/Clippy logs use*-corrected.log. All commands finished, no pending approvals.
- No stage/commit/reset/deploy. Existing staged/unstaged/untracked preserved, including
  both external website files; status capturedworkflow-fairness-status-final.log.
  TaskPG remains RUNNING and retained. Final independent evidence audit requested.

Implementer latest107160/25840041.47%08:37:52Z usage/runtime sources. Reviewer last
combined sample38.09%; fresh final sample follows. Root owns acceptance/nextphase.

### Independent final acceptance PASS — 2026-09-29 08:39Z

Reviewer verified actual let-chain correction and all evidence above; full A–F
fairness acceptance PASS, no unresolved findings. Reviewer session
01a0ec3e-5420-7690-a1c8-4353e47c16cd,102022/25840039.48%08:38:53.928Z,
usage/runtime sources, now quiescent. Implementer completes this fairness boundary
and retires; no active command/approval/reviewer work. Root accepts the point.

Limits remain explicit: liveness assumes responsive supported worker/DB within the
enforced retry bound, not arbitrary suspension/repeated database failures. EXPLAIN
fixtures show current status/time pruning and existing indexes, not proof of
history-independent database cost. No production startup wiring or phase04 changes
were added. Combined full phase03 library gate is the next separate assignment.
