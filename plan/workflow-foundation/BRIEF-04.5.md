# 04.5 — authoritative action receipts and supported replay

Status: ROOT ACCEPTED on 2026-09-30 at 17:29Z after independent overall implementation and gate review PASS.

Original phase 04 Execution item 4 and the deferred 04.4 supported replay integration
remain authoritative. All production and acceptance findings are resolved. Earlier
phase 03 and 04.1–04.4 acceptance is preserved. Historical partial checkpoints below
are superseded by the final implementation PASS and quiescent handoff section.

## Concrete contracts

- Successful local and remote dispatch returns `Committed(ActionReceipt)` only after its
  bounded, output-schema-validated result has committed. Remove remote `ResponseObserved`
  as a successful public return; timeout, cancellation, invalid output, storage rollback or
  ambiguous commit acknowledgement cannot return accepted output. First-marker/proof facts
  remain immutable. A restart/model-call/step lookup returns the same receipt without any
  transport call, after current exact scope/fence/access checks. Receipt replay does not need
  a currently registered replay guarantee: completed effect truth is independent of retry
  permission. Protected actions remain undispatched pending05.
- Add only migration20260930150000: permit remote receipts, append immutable remote dispatch
  entries referencing the FIRST marker plus existing task_attempts exact fence, and link
  remote receipts to their entry. No job/attempt/lease/queue/recovery owner is added. A unique
  entry per invocation+existing job attempt fence allows at most one reservation per attempt;
  same-attempt restarts/competing step/tool claimants cannot obtain a second permit. Entries
  have composite tenant/scope/digest/marker and task-attempt foreign keys; deferred commit
  guards repeat run-first live-fence/deadline checks. Local receipt atomicity remains intact.
- Reservation first looks for a committed receipt. Otherwise first dispatch commits marker,
  approved descriptor and per-attempt entry before I/O. Existing unresolved marker can reserve
  only in a NEW, currently owned attempt after the old attempt is retired; all previous
  attempt ownership has ceased. Exact first persisted descriptor must be non-null and match
  the current trusted registered descriptor and frozen company/target/tool/schema/policy.
  Reconcile/missing/changed/unsupported proof never permits replay. Idempotency uses the
  original stable logical key. Retention runs from FIRST marker creation: authoritative DB
  time proves its remaining guarantee covers the remaining run deadline, rather than using
  the deliberately conservative monotonic lease window. Entry repeats these checks; actual
  provider future remains supervised and is dropped before another attempt may own work.
- Receipt writer consumes private entry evidence, repeats exact immutable provenance and
  bounds/schema checks, inserts append-only receipt and awaits COMMIT before exposing it.
  It preserves bounded late effect truth even when current lease/run access was lost;
  returning a result still repeats current authority/fence/deadline and returns interruption
  on loss. Receipt persistence never advances a cancelled/stale execution. Shared existing
  `completion::complete_on` remains the ONLY workflow completion writer. Test actual receipt
  result into that writer; receipt commit precedes step output/advancement.06 owns Rig and
  conversation checkpoint adoption;08 owns bypass removal,04.9/10 production transport.
- Necessary phase03 recovery dependency: `recovery::retire_on` inspects ALL dispatch facts
  for its exact execution before selecting retry safety. A committed receipt establishes
  resolved effect truth. An unresolved first remote marker permits bounded queue retry only
  when its persisted approved first descriptor exactly validates against its frozen intent
  and retention still covers the run horizon. Missing/malformed/expired/unsupported proof
  forces unknown even if a caller claims safe; one unsafe sibling keeps the execution unknown.
  This is scheduling eligibility, never proof of result or a reusable transport grant; each
  actual redispatch repeats CURRENT registered descriptor and CURRENT access. Runs without
  action markers keep accepted03 behavior. Action scanning is bounded (overflow stays
  unknown); no resource bound is raised. Existing recovery policy/attempt caps/backoff/budget
  and deadline retirement remain the sole queue owner. No frozen method/MCP annotation or
  client header establishes safety. Unknown Reconcile is never relabeled safe.
- Additive database retirement/reopen invariants must enforce that same action-aware safety
  decision. Existing `check_workflow_fenced_retirement` and `workflow_control_retry_safe`
  currently trust the supplied/historical safe label; preserve their fence, cancellation,
  deadline, attempt, budget and no-marker checks while rejecting safe retirement/retry if
  ANY unresolved action has missing/malformed/unsupported/mismatched historical approval
  facts, insufficient retention from first marker, or scan overflow. Reopen rechecks CURRENT
  durable facts/retention, rather than only the attempt's old label. Centralize the scheduling
  predicate or provide the required DB-backed Rust/SQL equivalence matrix. Persisted JSON
  validation MUST NOT construct a trusted `ProviderReplayContract` or reusable I/O permission;
  actual provider entry still requires the current registered descriptor. Resolve any extra
  historical-proof evidence columns/functions additively as specified below.
- Concrete historical binding evidence: an additive nullable `replay_subject bytea` column
  on the immutable FIRST dispatch marker retains bounded
  canonical UTF-8 subject bytes for exactly `{"tool": frozen ToolSnapshot, "target": frozen
  ActionTarget}` using existing Rust canonical JSON, alongside its immutable first-marker
  linkage. New first-marker INSERT commits these bytes with the approved descriptor.
  Bound these bytes at524288 (existing policy-record bound), preserve underlying canonical
  operation limits, and retain the existing marker append-only trigger. Entry composite FKs
  inherit exact scope/digest/first-marker provenance. SQL historical
  proof validation converts those bytes to JSONB, compares the resulting object exactly to
  the immutable intent's frozen contract/target, and verifies
  `encode(sha256(subject_bytes),'hex') = first_descriptor.operation`. Validate descriptor
  version/registration/mode/nonzero bounded retention and recovery against that frozen
  snapshot; malformed UTF-8/JSON/shape returns conservative unknown, never approval. The
  first marker itself is the evidence source; later entries inherit these same bytes and
  cannot manufacture missing first evidence or restart proof/retention. SQL uses PostgreSQL's built-in bytea
  SHA256, not JSONB text hashing or the differently scoped intent operation digest.
  Pre04.5 immutable markers have NULL binding evidence and remain conservative unknown
  and cannot redispatch. No backfill/compatibility shim manufactures historical approval.
  Existing local receipts remain resolved. This restriction preserves prior conservative
  marker behavior; production registration/integration and evidence settlement remain later.

## Cross-point boundaries

04.6 still owns complete action-aware uncertainty parking/audit, including handler integration,
output-error/budget/cancellation/crash classifications and parked deadlines. This point adds
only the supported replay dependency and verifies its negative boundaries.04.7 owns authorized
applied/not-applied evidence settlement and permitting a retry after proven-not-applied; no
evidence command or fabricated not-applied state is introduced.04.8 still owns the full late
receipt/cancellation audit matrix and integration;04.5's receipt write already preserves late
truth without advancement. MCP `isError` envelope/classification/diagnostics remain04.10;
generic output validation failure stays unresolved regardless of possible provider effect.

## Files and meaningful acceptance

Application actions/{dispatch,replay,mod}.rs: receipt return, consuming entry/receipt port,
proof validation and monotonic supervision. Persistence workflow/{action_dispatch,
action_receipts,action_recovery,recovery,mod}.rs plus action receipt/replay test siblings;
additive migration only. Existing authorization/freeze/lease/completion contracts reused.

- Actual provider counts and durable DB result prove first-marker-before-call and
  receipt-before-success. Deferred receipt fault/rollback/ambiguous acknowledgement returns
  no success. Restart and durable model-call linkage return exact committed output with zero
  additional provider calls, including missing replay registration; current revocation,
  protected policy/wrong digest/stale fence cannot expose output or advance.
- Actual crash/lost-response after logical provider effect, real retirement/expiry/reclaim
  through existing owners, current-approved SafeRepeat and provider-key deduplication replay
  commit one receipt and no duplicate logical effect. Competing new claimants/step/model
  callers issue at most one call/entry per attempt. Same-attempt no-receipt repeat refuses.
- Unsupported/missing/changed descriptor and expired retention cannot repeat. Persisted
  first marker/proof cannot be replaced or its retention restarted; lock-wait retention
  regression retains authoritative DB-clock calculation. Unsafe sibling/malformed proof,
  timeout/invalid result/budget/deadline/cancel/stale-owner failures retain unknown facts,
  and Reconcile never calls twice. Late receipt is retained with no cancelled advancement.
- Migration FK/check/append-only/deferred fence tests reject foreign provenance and remote
  receipt without entry. Same-DB receipt/effect tests still prove atomic rollback.
- Direct SQL forged-safe retirement/reopen fails transactionally. Rust/SQL equivalence
  covers receipt-resolved actions, supported unresolved replay, Reconcile/missing/malformed/
  mismatched proof, expired original retention, unsafe siblings and bounded scan overflow.
  Include numeric schema values (canonical Rust bytes vs JSONB equality), wrong frozen
  snapshot, malformed subject bytes and pre04.5 markers lacking binding evidence.
- Required gates: scoped stock2048KiB stack-budget action tests plus affected recovery/budget
  and completion integration tests; fmt, both whitespace checks, locked offlinealltargets,
  strictClippy, fresh41migration schema, SQLx prepare/check, graft build. No limit raised.
  Independent fresh actual-code/criterion/final-evidence review before root acceptance.

## Identity and resources

Implementer01a0f254-ecc6-7100-a410-8f7a72edfdc5 actual turn_context Sol6.1/high;
startup22894/2584008.86%12:40:56Z; reconciliation91275/25840035.32%12:45:45Z.
Nested reviewer01a0f255-481c-7e43-b4ad-d41af047c8c0 actual Astra/medium;
startup29174/25840011.29%12:41:18Z, review-only/no deeper agents.
TaskPG retained STOPPED; both task URLs postgres://mac03@127.0.0.1:55439/workflow_admission.
Root owns PROGRESS/RESUME. No stage/unstage/commit/reset/deploy or applied migration edits.

## Independent expansion review evidence

Astra found and resolved two material contract omissions before implementation:
additive action-aware retirement/reopen SQL guards, then canonical historical-proof
binding independent of JSONB serialization. Corrected expansion PASS12:54:13Z,
79772/25840030.87%; focused nullable immutable FIRSTmarker-column substitution
PASS12:55:33Z,80705/25840031.23%, same reviewer UUID/model above. Sources runtime
usage/capacity. No outstanding expansion blockers. Root ACCEPTED12:57Z.
Code/schema unchanged; taskPG remains stopped/retained; no builds/tests/approvals active.

## Focused successor packet — expansion accepted, implementation next

Read this brief/original04 Execution1–7/BRIEF04.4 deferred04.5 contracts and relevant
AGENTS; reuse accepted light04–10 and concrete04.3/04.4 expansion without redoing discovery.
Root alone owns PROGRESS/RESUME and acceptance. Fresh implementer creates one nested
Astra/medium review-only worker; no old worker/reviewer reuse. Actual-code and final-gate
review are still mandatory: expansion PASS is not implementation evidence.

Exact graph-indexed seams (query literal/skeleton/callers before source):
- actions/dispatch.rs:32–78 private reservation/entry/port;95–100 public observations;
  120–174 supervised actual future;176–192 existing bounded/schema result validation.
  Add committed receipt reservation branch and consuming finish/receipt port. Remove
  response-observed success path; keep external future boxed and cancelled before cleanup.
- actions/replay.rs:24–68 trusted descriptor/private invocation;71–99 preparation;
  101–107 SHA256 canonical `tool`+`target` signature. freeze.rs:111–126,153–193 canonical
  numeric JSON; general operation digest differs. Share canonical subject bytes helper
  beneath existing bounds, never Deserialize a trusted descriptor from stored proof.
- persistence/workflow/action_dispatch.rs:71–129 current run-first fence/authority/intent;
  131–156 first-marker lookup/insert;161–188 atomic local effect+receipt;202–251 remote
  reservation;253–280 consuming final entry;282–286 lease-only heartbeat. Extract receipt
  and replay helpers as needed rather than extending >80line functions/async depth.
- action_authority.rs:30–94 authoritative frozen action loader; completion.rs:39–110 sole
  shared completion writer; lease.rs:158–171 monotonic live_window. DB retention duration
  in current reserve_remote is authoritative and separate from that conservative window.
- recovery.rs:21–72 sole retirement decision;74–109 existing default failure/safety;
  additive corrections to check_workflow_fenced_retirement (immutable controls migration
  20260928200000:85–137) and workflow_control_retry_safe (immutable budget retry migration
  20260929080000:2–29). Implement one centralized bounded action-safety predicate used for
  retirement/reopen, or meaningful Rust/SQL equivalence; no safe-label shortcut.
- Existing isolated reusable fixtures/provider counts: action_dispatch_tests.rs:70–142
  setup_action/facts, action_replay_tests.rs:1–86 registered descriptor/scripted provider;
  action_dispatch_remote_tests.rs intercepts actual marker/entry boundaries;
  action_dispatch_sql_tests.rs provenance/deferred transaction guards. Tests need updates
  for committed successful remote result, then meaningful new receipt/recovery groups.

Additive migration20260930150000 only: nullable bounded replay_subject on immutable first
marker, append-only remote per-existing-attempt entry facts with composite provenance/live
commit guards, remote receipt entry FK/check, scheduling/retirement/reopen predicates.
First marker bytes+descriptor inserted atomically; existing NULL cannot be filled later.
Receipt writes deliberately do NOT require a currently live fence, enabling late truth;
returning output/completion does require current fence/access. Receipt conflict returns
existing authoritative result rather than replacing it. Scheduling supported firstproof
does not replace current registered descriptor at redispatch or imply an effect result.

No stage/unstage/commit/reset/deploy; preserve all modified/untracked prior04 work.
TaskPG sole resource owner transfers to successor: retained STOPPED cleanly, data
/private/tmp/workflow-admission-pg-e3aa, log/private/tmp/workflow-admission-postgres.log;
both URLs postgres://mac03@127.0.0.1:55439/workflow_admission. Read skill postgres reference,
restart exact -h127.0.0.1 -p55439 -k/private/tmp -c max_connections=200 under host rules;
routine localPG preauthorized, never reset retained data. Builds/SQLx sequential, logs disk.
Unchanged prior36action gates available BRIEF04.4; schema change requires fresh41migration
and SQLx gates. No gate was run for04.5 yet, no new source/migration exists.

Retired implementer UUID01a0f254-ecc6-7100-a410-8f7a72edfdc5 actualSol6.1/high;
boundary107657/25840041.66%12:56:45Z; root independently saw108985/25840042.18%
12:57:07Z. Reviewer UUID01a0f255-481c-7e43-b4ad-d41af047c8c0 actualAstra/medium,
81098/25840031.38%12:55:40Z; interrupt confirmed completed/quiescent. Root chose natural
rotation before substantial code to preserve full implementation/review capacity. No
active test/build/SQLx/approval or database process at handoff.


## Partial implementation checkpoint — successor resumes corrections and acceptance

Current implementer UUID01a0f267-9a44-7ac1-bca2-0ba4adc13e14, verified actual
turn_context Sol6.1/high; startup22808/2584008.83%13:01:23Z. Milestones80388/258400
31.11%15:26:33Z,94211/25840036.46%15:31:35Z,105212/25840040.72%15:37:49Z,
113276/25840043.84%15:39:37Z,121222/25840046.91%15:42:40Z,
123788/25840047.91%15:44:57Z. Usage/runtime capacity; worker rotates before further
substantive corrections. Root alone owns PROGRESS/RESUME and point acceptance.

Partial code added in application actions/dispatch.rs +replay.rs +mod.rs and persistence
workflow/action_receipts.rs +action_dispatch.rs +recovery.rs +mod.rs. Reservation looks
for a receipt after exact current authority, independently of registration; otherwise
first marker and per-existing-attempt entry commit before actual provider polling.
Receipt writer consumes entry provenance, validates bounded result and immutable frozen
operation/proof/subject, commits effect truth, then checks current authority before output.
Late cancelled/stale truth is retained without advancement. Public successful remote output
is now Committed(ActionReceipt), replacing ResponseObserved. Existing local atomicity intact.
Shared SQL historical action predicate is called by Rust retirement and additive deferred
retirement/operator-reopen guards; no new queue/job/lease/recovery owner or raised bounds.

Migration20260930150000_workflow_action_receipts.sql APPLIED to retained taskPG; now
IMMUTABLE. SHA256 e20d987b23a198a088199917b05b8b840117ba9c76e70504e9a0992f1bdea9ae.
Adds immutable nullable replay_subject first-marker bytes, append-only remote entries,
receipt entry linkage, historical scheduling predicates and additive guards. All earlier
applied migrations unchanged. DO NOT repair this file even before fresh-schema testing.
Prior source snapshots at /private/tmp/workflow-04.5-baseline/{dispatch.rs,replay.rs,
action_dispatch.rs,recovery.rs}; no baseline snapshot needed for new action_receipts.rs.
All prior modified/untracked work preserved; no stage/unstage/commit/reset/deploy.

Development evidence only, no final gate/acceptance:
- env SQLX_OFFLINE=true cargo check --locked --offline --all-targets PASS19.44s;
  executed before latest tests/receipt replay monotonic correction. Tool output retained;
  no disk log for this initial check. cargo fmt ran, final fmt check still required.
- Both exact task URLs, SQLX_OFFLINE=true RUST_MIN_STACK=2097152
  scripts/stack-budget.sh --offline workflow_action_:
  /private/tmp/workflow-04.5-actions.log =39/40PASS,0ignored,15.39s. Original36 passed;
  new receipt restart/model/no-registration/current-access, latecancel/stale retention,
  and deferred receipt rollback passed. Actual supported lost-effect/reclaim group fails.
  Sandbox-first /private/tmp/workflow-04.5-initial-actions.log29DBpermission failures
  superseded by authorized escalated run above. These were sandbox failures, not regressions.
- /private/tmp/workflow-04.5-retry-diagnostic.log focused actualeffect retry diagnostic:
  SafeRepeat iteration completes realretirement/reclaim/competing-entry/deduplicatedeffect;
  ProviderIdempotency fails historical safety predicate BEFORE retirement. Diagnostic query
  still in reclaim helper action_receipt_tests.rs; refine into meaningful assertions once fixed.
- Migration apply PASS35.97ms. Fresh migration, SQLx, Clippy, remaining broader checks pending.

Initial actual-code reviewer /root/receipts_implementation/receipt_review, UUID
01a0f268-43da-75f0-ad6c-b5e6cc367193, verified actual Astra/medium. Startup29174/258400
11.29%13:02:08Z, review88163/25840034.12%15:44:40Z usage/runtime. Review-only/no deeper
agents; code held unchanged while inspecting. CHANGES REQUIRED, no implementation PASS:
1. P1 historical validator migration:80 JSONB subtraction parses before -> extraction;
   ProviderIdempotency always falls into conservative exception false. Actual readonlyPG
   expression reproduces invalid input syntax for typejson / Tokenretentioninvalid.
   /private/tmp/workflow-04.5-readonly-retention-repro.log. Parenthesized extraction is required.
2. P2 migration:66 registrationregex accepts invalid1bad/a..b/a. and rejects valid_provider.
   Match exact TypeName grammar src/domain/workflow/ids.rs:15–24,26–67,72, including
   underscore-leading dotted segments and TOTAL128byte bound.
3. P2 migration:71 policy_revision bigint rejects valid u64 above i64MAX. Check integral
   positive numeric through u64MAX; publication/types.rs:85–90 is authoritative.
4. P2 action_receipts.rs:217 return_authorized hides ALL DB/policy/timeouterrors using
   matches! fallback. Preserve committed receipt but propagate operational errors, or log
   explicitly intentional failclosed fallback per src/AGENTS; distinguish known loss.

## Focused correction expansion — independently PASS, root owns acceptance

Necessary corrective migration20260930160000 replaces ONLY historical validator function;
keep applied150000 byte-for-byte. Correct retention JSONB parentheses, exact TypeName grammar
and original positive u64 policy revision domain. Preserve all other historical binding,
retention, conservative failure and scheduling-only contracts. Required fresh gate now
42 migrations. This is the sole amendment to previous “only150000/41migration” packet.
Astra independently reviewed reproduction and affected contracts, focused expansion PASS
15:44Z, reviewer identity/sample above. Root acknowledgement required before dependent
corrective code; root records acceptance in PROGRESS. No160000 source/migration created yet.
Finding4 is local correction within original contracts. Corrected code still needs a NEW
successor's nested reviewer actual-code review and final criterion/gate signoff.

Next successor must implement these findings first, then complete still-unwritten meaningful
acceptance tests: malformed/missing/changed/expired approval +legacy NULL, canonical numeric
schema bytes vs JSONB equality, wrong frozen snapshot/hash and invalidUTF8/shape; unsafe
siblings +129fact scan overflow; directSQL forgedsafe retirement/reopen and shared Rust/SQL
predicate equivalence; current descriptor/access failures after actual reclaim; remoteentry
FK/append-only/deferred-fence/receiptwithoutentry provenance; committed receipt ambiguousACK
then restart zeroIO; actual committed receipt result passed into sole shared completion writer
with receipt timestamp preceding advancement. Existing tests include actual firstmarker/entry
before actualproviderinvoke, logical provider-key DBdedup after lostresponse, competingclaimants,
sameattempt refusal, original36tool/step identity/fence/revocation/deadline/localatomicity.
Generic MCP isError accepted-as-schema-valid result remains04.10 classification boundary;
updated original tests now assert authoritative committed receipt, without inferring retry
from transport metadata. Keep all original criteria and later04.6/7/8/9/10/11/06 obligations.

Required final checks remain scoped stock2048KiB action tests AND affectedrecovery/budget/
completion tests; fmt/bothwhitespace; lockedofflinealltargets; strictClippy; fresh42migrations;
SQLx prepare/check; graft build. Builds/SQLx sequential and full logs disk. No bound raised.
No active build/test/SQLx/approval at checkpoint. TaskPG lifecycle quiescence recorded below.


Root ACCEPTED focused160000 correction expansion after Astra PASS, before successor code;
all four code findings remain UNVERIFIED. User steering: no backwardcompatibility work,
shims or backfill; preserve immutable applied migrations and uncertain effect facts. NULL
historical evidence stays unknown solely as fact preservation, without extra legacy adapters.
At rotation taskPG STOPPED CLEANLY/retained with pg_ctl fast stop; exact data/log/endpoint
unchanged, both URLs postgres://mac03@127.0.0.1:55439/workflow_admission. Reviewer interrupt
confirmed completed/quiescent. No active test/build/SQLx/approval, no code edits after review.

## Corrective implementer checkpoint — 2026-09-30

Implementer UUID01a0f301-ff17-7f01-b4cd-22375c619b6b verified actual Sol6.1/high;
startup22958/2584008.88%15:50:03Z; latest124813/25840048.30%16:42:22Z.
Usage/runtime capacity sources. Root alone owns PROGRESS/RESUME and acceptance.
New sole nested reviewer /root/receipts_corrections/receipt_review UUID
01a0f305-7bab-7891-bbe9-d4c9d55d55db verified actual Astra/medium; startup29643/
25840011.47%15:53:54Z. Identity-only startup so far; no actual-code review yet.

Four accepted corrections implemented. Additive migration160000 APPLIED12.798ms and
now immutable. It replaces ONLY workflow_action_replay_supported: retention extraction
parentheses, exact dotted underscore-leading TypeName grammar plus TOTAL128byte bound,
integral positive numeric policy revision through18446744073709551615. Migration150000
unchanged SHA256 e20d987b23a198a088199917b05b8b840117ba9c76e70504e9a0992f1bdea9ae.
return_authorized now explicitly tests known current fence/window loss, then propagates
DB/current-policy and timeout errors while leaving the already committed receipt intact.
Timeout uses the existing lease::timed_out Conflict classification, without swallowing it.
Removed large diagnostic query from reclaim in favor of exact shared-predicate assertion.

New acceptance files action_history_tests.rs (451lines before final formatting) and
action_receipt_acceptance_tests.rs are nested through action_receipt_tests.rs. Added:
23case historical malformed/missing/proof/shape/UTF8/snapshot/hash/retention matrix with
actual Rust retirement/shared SQL equivalence; canonical numeric subject vsJSONB plus
positive u64 revision endpoints/invalid values; unsafe sibling and128/129scan boundary;
directSQL forged-safe retirement and current-retention reopen; exact remoteentry composite
FKs/append-only/deferred fence and receipt without valid entry; committedreceipt lostACK
then restart zeroIO; actual reclaimed current registration/access failures; actual receipt
result into shared complete_io with receipt timestamp before execution advancement; postcommit
DB/policy/timeout propagation preserving receipt truth. Existing dedup provider now carries
an explicit result Value so the completion test uses the ACTUAL provider/receipt output.

Development evidence (not final acceptance):
- /private/tmp/workflow-04.5-corrected-actions.log PASS40/40,0ignored,14.76s stock2048KiB,
  BOTH exact task URLs, SQLX_OFFLINE=true scripts/stack-budget.sh --offline workflow_action_.
  Actual SafeRepeat/ProviderIdempotency lost-effect retirement/reclaim now passes.
- /private/tmp/workflow-04.5-history.log PASS3/3,0ignored,1.39s stock2048KiB same URLs;
  initial history matrix/numeric/sibling+overflow groups. Additional SQLgroups added later.
- /private/tmp/workflow-04.5-receipt-acceptance.log initial new receipt groups7/8PASS;
  failure was assertion expecting Timeout while existing lease::timed_out returns Conflict.
  Corrected assertion now matches EXACT DB/policy/timeout error text/class. Completion,
  ambiguous receiptACK and actual currentproof/access reclaim tests passed.
- /private/tmp/workflow-04.5-complete-actions.log latest full run49/51PASS,0ignored16.26s:
  all receipt groups including corrected postcommit error propagation passed. Two history
  test assertions failed: append-only guard text was “append-only” (not “immutable”), and
  reopen guard is deferred (UPDATE succeeds before SET CONSTRAINTS). Both assertions fixed;
  current-control-predicate false asserted explicitly before deferred reopen guard.
- /private/tmp/workflow-04.5-history-corrected.log running at this checkpoint; command had
  accidental shortened TEST_DATABASE_URL host127.0.1; MUST supersede using BOTH exact URLs
  postgres://mac03@127.0.0.1:55439/workflow_admission before citing final evidence.

No other production contracts changed; no compatibility shim/backfill/new owner/bounds.
All applied migrations immutable and all prior modified/untracked work preserved.
No stage/unstage/commit/reset/deploy. Allfinalgates remain pending: full51action rerun,
affectedrecovery/budget/completion stockstack, fmt/bothwhitespace/lockedofflinealltargets/
strictClippy/fresh42migration/SQLxprepare+check/graftbuild, independent fresh actual-code
and finalcriterion review. Reopen test currently proves sharedpredicate currentretention
false and directSQL refusal; reviewer must assess isolation of other reopen guards.
TaskPG still running retained at checkpoint until explicit final lifecycle note below.

FINAL RETIREMENT NOTE supersedes pending state above: implementer129721/25840050.20%
16:45:35Z, mandatory rotation; no further substantive edits/checks. Last history rerun
6/7PASS2.00s (shortened-host command, not final evidence): deferredreopen passes. The
append-only assertion replacement did NOT apply through rustfmt's split method chain;
action_history_tests.rs:439 still contains("immutable") and MUST change to
contains("append-only"). All production corrections remain implemented; finalreview
and gates are UNVERIFIED. Exact next check after this one assertion fix is BOTH exact
URLs +SQLX_OFFLINE=true +RUST_MIN_STACK=2097152 scripts/stack-budget.sh --offline
workflow_action_ (expected51tests), then affectedrecovery/budget/completion filters and
all previously listed final gates. All code is frozen pending successor; no bounds raised.
Migration160000 immutable SHA256
5c394b8d5387f7e34b6bd630f5355c346841d7361e79fdc1f26ee4bc04b3e0cb.
TaskPG STOPPED CLEANLY/retained16:45Z with pg_ctl fast stop; endpoint/data/log unchanged.
No active build/test/SQLx/approval. Reviewer interrupted at mandatory implementer boundary;
last known54728/25840021.18%16:45:33Z; retirement-save-only return pending, no further
inspection authorized. Successor must create its OWN fresh independent actual-code/full
criterion/final-gate reviewer; never treat this bounded partial audit as implementationPASS.

Reviewer retirement return: STOPPED/quiescent, incomplete audit, NO PASS; no new concrete
defect established. Independently checked160000 solefunction replacement/retention/grammar/
u64bound, both migration hashes, receipt commit before return_authorized and explicit loss vs
error behavior. Remaining audit: authorize_on/lease affectedcallers, tests/logs, full revision
contract and remaininginstructions. No reviewer tests run. Verified lastusage54728/258400
21.18%16:45:33Z. This evidence does not close any full implementation acceptance gate.

## Final verification checkpoint — 2026-09-30

Fresh implementer UUID 01a0f338-0231-71b2-90de-6658dd7c7611, actual Sol6.1/high;
startup 22925/258400 (8.87%) 16:49:07Z; intermediate 77564/258400 (30.02%) 17:00:13Z.
Fresh nested reviewer final_receipt_review UUID 01a0f339-3e34-7961-bff5-36c077d1d5d2,
actual Astra/medium; startup 26162/258400 (10.12%) 16:50:20Z. No previous partial review
is treated as PASS. Root owns PROGRESS/RESUME and acceptance; 04.6 remains untouched.

Only code change in this successor: action_history_tests.rs append-only assertion now
checks the actual SQL error text. Both applied migration hashes remain exactly as above.
Both DATABASE_URL and TEST_DATABASE_URL explicitly use
postgres://mac03@127.0.0.1:55439/workflow_admission for every retained-database gate.
The following stock-stack commands all use SQLX_OFFLINE=true RUST_MIN_STACK=2097152
and scripts/stack-budget.sh --offline FILTER, with no stack bound changed:

| Filter | Passed / ignored | Duration | Full log in /private/tmp/ |
| --- | --- | --- | --- |
| workflow_action_ | 51 / 0 | 16.34s | workflow-04.5-final-actions-host.log |
| workflow_failure_ | 8 / 0 | 3.28s | workflow-04.5-final-recovery.log |
| workflow_budget_ | 24 / 0 | 8.12s | workflow-04.5-final-budget.log |
| workflow_completion_ | 14 / 0 | 6.75s | workflow-04.5-final-completion.log |
| workflow_io_ | 13 / 0 | 5.68s | workflow-04.5-final-lease.log |
| workflow_control_ | 26 / 0 | 9.54s | workflow-04.5-final-control.log |
| workflow_maintenance_ | 12 / 0 | 5.97s | workflow-04.5-final-maintenance.log |

Final gates PASS: cargo fmt --all -- --check; git diff --check;
git diff --cached --check; SQLX_OFFLINE=true cargo check --locked --offline --all-targets
(48.22s); SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings
(59.23s); cargo sqlx prepare -- --all-targets (19.93s); cargo sqlx prepare --check --
--all-targets (22.93s); graft build (14333 nodes / 21380 edges / 718 cards).
Full logs: workflow-04.5-final-{fmt,whitespace,staged-whitespace,check,clippy,
sqlx-prepare,sqlx-check,graft}.log under /private/tmp/. SQLx cache has no changed files.
Fresh task-created database workflow_045_final_schema_01a0f338 applied all42 migrations;
ledger count/success/max version =42/42/20260930160000. Fresh DATABASE_URL and
TEST_DATABASE_URL both explicitly use that database on the same retained cluster.
Full logs workflow-04.5-final-fresh-migrations-host.log and final-fresh-schema.log.
Initial sandbox action and fresh-migration runs failed on connection permissions;
successful authorized host runs above supersede them, without treating failures as PASS.

All code and plan files now frozen for fresh actual-code/full-criteria/final-gate review.
Implementation acceptance remains UNVERIFIED until that independent review and root acceptance.
Task PG running retained at exact endpoint, max_connections=200; no build/test/SQLx running.
The fresh schema database is task-created and will be dropped after review; no retained reset.

## Fresh independent audit corrections — 2026-09-30

Full actual-code/criteria/gate audit returned CHANGES REQUESTED, no production defect:
[/private/tmp/workflow-04.5-final-independent-review.md](/private/tmp/workflow-04.5-final-independent-review.md).
Two P2 acceptance gaps: reopen refusal was masked by unrelated run/command guards;
receipt-resolved recovery had no meaningful actual-retirement test. Both are local test
corrections within accepted contracts. Reviewer retired/quiescent at 48.26% (final tool
return49.36%); no correction reassignment. Fresh nested correction reviewer
receipt_correction_review UUID01a0f349-7754-73c3-a7d9-5c73523cf1f4 verified Astra/medium,
startup29219/25840011.31%17:08:02Z; no deeper delegation.

Corrected action_history_tests.rs uses the same otherwise-valid direct SQL reopen helper
in positive/negative rollback transactions: running owner, NULL terminal execution,
applied retry receipt with matching current revision and audit event. Only negative
retention horizon changes; exact invalid explicit workflow retry error and full rollback.
Added actual Reconcile/no-registration provider probe in action_receipt_acceptance_tests.rs:
unresolved predicate false before provider effect; real receipt COMMIT then shared predicate
true; real expired retirement/reclaim records safe; new current fence returns identical
receipt without another entry/call/effect. Prior tests/production/migrations preserved.

Superseding action gate PASS52/52,0ignored16.22s, stock2048KiB BOTH exact task URLs using
the same command above; /private/tmp/workflow-04.5-final-corrected-actions.log.
Affected final reruns PASS: fmt/both whitespace; locked offline all-target check53.26s;
strict Clippy1m07s; SQLx prepare37.43s/check24.19s, no .sqlx diff; graft build14338nodes/
21394edges/718cards. Logs /private/tmp/workflow-04.5-correction-{fmt,whitespace,
staged-whitespace,check,clippy,sqlx-prepare,sqlx-check,graft}.log. Earlier unaffected
recovery/budget/completion/lease/control/maintenance and fresh42migration gates retained.
No production contract/schema/bound changes, no compatibility shim/backfill.
Own fresh context113004/25840043.73%17:21:01Z (usage/runtime capacity sources).
Files frozen again for fresh independent correction/integration/final-gate review.
No active build/test/SQLx; task PG retained running. Root acceptance still pending.

## Final implementation PASS and quiescent handoff — 2026-09-30 17:27Z

Fresh correction reviewer returned explicit overall04.5 implementation PASS, no remaining
acceptance gap, combining prior completed full independent actual-code audit with actual
correction/integration review and final gates. Report:
[/private/tmp/workflow-04.5-correction-independent-review.md](/private/tmp/workflow-04.5-correction-independent-review.md).
Both P2 findings resolved; no production defect or contract change. All required gates
PASS as recorded above, superseding action suite52/52. Root alone records acceptance.
Reviewer final self sample87294/25840033.78%17:25:01.735Z; owner verified latest
89663/25840034.70%17:25:08.377Z; actual Astra/medium UUID above, quiescent.
Implementer final119295/25840046.17%17:27:15.034Z; actual Sol6.1/high UUID above.
Both samples use runtime usage/capacity sources, depth0; retired prior reviewer quiescent.

Fresh schema database workflow_045_final_schema_01a0f338 DROPPED after verified42migrations.
Retained task PG FAST STOPPED CLEANLY; pg_ctl status confirms no server running.
Retained data/private/tmp/workflow-admission-pg-e3aa, log/private/tmp/workflow-admission-postgres.log;
both next-task URLs postgres://mac03@127.0.0.1:55439/workflow_admission. Restart with
-h127.0.0.1 -p55439 -k/private/tmp -c max_connections=200 per skill/reference. No reset.
No active build/test/SQLx/approval; files preserved, no stage/unstage/commit/deploy.

Next scope only AFTER root acceptance:04.6 full action-aware uncertainty parking/audit,
handler output-error/budget/cancellation/crash classifications and parked deadlines.
04.5 supplies supported receipt/replay dependency and its negative boundaries;04.7
authorized evidence settlement,04.8 full late-effect audit,04.9/10 production transports,
protection05, Rig06 and bypass08 remain separate. No04.6 code or expansion started here.
