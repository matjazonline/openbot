# 04.3 — durable dispatch foundation

Status: EXPANSION VERIFIED and root ACCEPTED2026-09-30. Implementation and
combined independent actual-code/criteria/point-gates review PASS2026-09-30.
ROOT ACCEPTED2026-09-30 12:00Z; all29 stock2MiB workflow_action tests PASS.
Original04 Execution item2 and all nested criteria remain authoritative. Root owns
PROGRESS/RESUME/queue acceptance; worker owns code/this brief/affected expansion/taskPG.

## Accepted expansion and independent review

Read original remaining04–10, accepted04.1/04.2 briefs, RESUME, REPLACEMENT-MAP and
complete light EXPANSION. Added concrete section
`2026-09-30 concrete04.3 dispatch transaction contract` to EXPANSION.md. No accepted
historical implementation was repeated or changed. Applied migrations through
20260930090000 remain IMMUTABLE; new migrations must be additive.

Independent reviewer inspected originals, full remaining light expansion and actual
authorization/lease seams. Initial F1 blocking finding: a first remote reservation
could escape to a caller and be used after lease expiry/ownership loss; unique marker
alone only prevents the second reservation. Deferring all actual provider-neutral
orchestration until04.9/10 did not establish original04 item2's remote protocol.

Correction:04.3 owns application RemoteAction orchestration using scripted-provider
tests. First reservation remains internal, single-use, non-cloneable/non-serializable,
carrying exact saved operation/fence and conservative monotonic lease/run window.
Immediately before first polling, repeat transactional current authority+marker/fence
checks, then monotonic expiry check with no intervening await. Supervise actual future
for deadline/cancellation/ownership loss; never detach it. No second permit derives
from an existing marker. Delayed use, post-marker revocation/cancel, live-work ownership
loss, ambiguous commit acknowledgment and restart/no-second-call tests are required.
Remote outcomes remain observations before04.5 authoritative receipts.

Correction review PASS: F1 resolved; all unblocked selected04.3–04.11 and05–10
expansion points passed.10.4–10.8 operational cutover remains blocked pending target
and explicit authorization. Reviewer actual-code implementation review remains
future work, not implied by expansion PASS. Root accepted this result before rotation.

## Focused successor packet

Use implement-sol-astra skill (including references/postgres.md), current repository
AGENTS.md plus src/application and persistence guides. Root spawns a fresh Sol6.1/high
implementer; it must create its own Astra/medium nested reviewer. Do not reuse this
retired subtree or repeat the accepted original-whole-scope expansion review.
Read original04 plan, accepted concrete EXPANSION section and04.1/04.2 briefs; root
acceptance/progress and RESUME carry original remaining queue. Material contract
changes still require independent affected expansion review before implementation.

Implement current04.3 only and checkpoint to root before04.4. Deliver cohesive
application actions/dispatch port/service, SQL action_dispatch transaction adapter,
additive normalized dispatch/receipt schema and isolated realSQL race/crash tests.
No new job/lease/attempt/recovery owner. Current task_attempts is sole attempt ledger.
Run-first locking, immutable lineage and shared completion writer stay intact.

Concrete existing seams (graft spans checked, use graph before source discovery):
- application actions/authorization.rs:14–48 saved authority/current policy ports;
  58–112 observational authorize;115–194 pure frozen/current policy/target rules.
- persistence workflow/action_authority.rs:17–86 saved authority restoration and
  current company/principal access locks. Extract transaction helper without changing
  accepted04.2 observational behavior; observations never become dispatch permission.
- persistence workflow/lease.rs:103–151 run/execution/job lock_scope/lock_fence;
  158–171 live_window validates current exact task_attempt plus monotonic lease/run
  deadlines. lock_fence alone does not check lease expiry/attempt-row liveness.
- persistence workflow/authority.rs:9–36 authorize_company locks current grants;
  workflow/completion.rs:39–110 shared completion transaction retained for04.5.
- application actions/contracts.rs:62–65 ApprovalSubject;125–128 ActionReceipt;
  service.rs:22–28 FrozenAction,71–73 ActionService; mod exports govern port exposure.
- applied migration20260930090000_workflow_action_intents.sql immutable subject/
  tenant/execution/digest and model-call identity owner. SQL/migrations are unindexed.

04.3 current authorization must use transaction-aware trusted adapter composition
hooks that lock authoritative resource/policy rows through final write/commit. Unlocked
ResourceDirectory/ActionPolicyDirectory observations cannot fence revocation. No
provider/network I/O inside SQL transaction. Missing production resource/policy owners
remain fail closed pending04.9/10; never invent another grant store. SameDB registered
effect and bounded schema-valid receipt commit together; faults/expiry must rollback
all writes. Commit-time deferred SQL fence/deadline guards are required independently
of Rust checks. Exact tenant/intent/digest/current attempt provenance must be constrained.

Protected actions remain undispatched pending05. Remote marker conservatively persists
unknown across commit ambiguity, crash-before-send, lost response and rejected delayed
first use. Existing marker never authorizes a new permit until04.4 replay proof/04.7
authorized proven-not-applied evidence.04.5 owns remote receipt-before-success/replay;
04.6 parking,04.8 late receipts/cancel,04.9/10 actual HTTP/MCP adapters,04.11 messaging,
06 Rig/checkpoints and08 exhaustive legacy/bypass removal all remain mandatory.
No production adapter completion or combinedphase04/full-suite pass is claimed here.

Required checks: competing dispatchers, stale/wrong scopes/fences, real membership/
resource/policy revoker contenders, strengthened approval/errors, local rollback faults,
commit-time expiry and SQL FK/immutability negatives, scripted remote boundary races,
restart/ambiguous-ack/no-second-call. Retain accepted14 workflow_action tests. Run at
stock2MiB; formatting/whitespace, locked offlinealltargets, strictClippy, fresh migrations,
SQLxprepare/check, graftbuild and separate actual-code review. No resource bounds raised.
SQLx/builds sequential; logs/evidence on disk, compact criterion checkpoint to root.

## Identities, measurements and resources

Implementer collaboration handle/root/action_dispatch; CODEX_THREAD_ID
01a0f1f4-9b89-71f2-a2af-dcaa1cf4aace, runtime turn_context VERIFIED
gpt-6.1-sol/high. Startup36531/25840014.14%10:56:19Z;
expansion91187/25840035.29%11:00:32Z; correction96309/25840037.27%11:03:25Z;
PASS checkpoint98890/25840038.27%11:04:15Z. Sources token_usage_record.usage /
task_started.model_context_window. Natural early rotation after expansion; no code started.

Reviewer collaboration handle/root/action_dispatch/review_dispatch; sessionUUID
01a0f1f5-18a9-7b71-a756-b7bfe57817bc, actual turn_context VERIFIED
gpt-6-astra/medium. Startup30183/25840011.68%10:56:19Z; initial expansion finding
73064/25840028.28%11:02:02Z; correctionPASS77680/25840030.06%11:03:56Z.
Parent fresh observed78075/25840030.21%11:04:06Z token_count.info sources.
No reviewer edits/delegation/DB activity. Final quiescence/sample appended below.

TaskPG remains STOPPED/retained and untouched in this assignment:
/private/tmp/workflow-admission-pg-e3aa, log/private/tmp/workflow-admission-postgres.log.
Both DATABASE_URL and TEST_DATABASE_URL:
postgres://mac03@127.0.0.1:55439/workflow_admission.
Startup explicit options `-h 127.0.0.1 -p 55439 -k /private/tmp -c max_connections=200`,
per skill references/postgres.md. Routine task-owned localPG work already authorized;
host sandbox escalation rules remain mandatory. Never reset retained data.

Only EXPANSION.md and this new brief changed by this worker. All staged/unstaged/
untracked prior work preserved; PROGRESS/RESUME untouched. No stage/unstage/commit/
reset/deploy, migration, build, test or SQLx work. git diff --check PASS at expansion
checkpoint. Graft savings estimate2,810,795tokens (map/whole-file baseline, not measured
runtime token usage); orientation dominates the estimate.

Reviewer quiescence confirmed by interrupt_agent: prior status completed with correction
PASS; no active subtree work. All checks/SQLx/approvals inactive; taskPG remains stopped.
Final worker sample102881/25840039.81%11:07:10Z usage/runtime capacity sources;
root independently verifies fresh sample. Rotation is proactive at the expansion boundary, not threshold
failure. Root may assign a fresh implementer after confirming this stopped handoff.

## Partial implementation checkpoint2026-09-30; fresh successor required

Preserved accepted04.1/04.2 and accepted expansion. Implemented application
actions/dispatch.rs: ActionDispatchRequest, RemoteAction, first-marker reservation,
locked final entry, conservative monotonic lease/run window, supervised actual provider
future/drop and bounded schema-valid response observations. No protected dispatch or
remote accepted success. Extracted shared current resource/policy/run validators in
actions/authorization.rs; added request getter in service.rs, bounded canonical visibility
in freeze.rs, dispatch exports. Extracted tx-aware action_authority::load_on preserving
04.2 observational behavior. Added persistence workflow/action_dispatch.rs: trusted SQL-only
SqlActionAuthority/SqlLocalAction hooks, unavailable production authority fails closed;
run-first/exact task_attempt locks, saved/current authority checks, atomic local effect+
receipt, durable first remote marker and exact final-entry checks. Added isolated
action_dispatch_tests.rs registered under lease_tests.rs. No new ownership ledger/queue.

Migration20260930120000_workflow_action_dispatch.sql APPLIED on retained taskPG and is now
IMMUTABLE, alongside all earlier migrations. Any correction must be additive. Composite
job/attempt provenance keys, dispatch/receipt FKs, invocation uniqueness, append-only guards,
deferred commit live-fence/lease/run-deadline check and mandatory local receipt are installed.
Remote receipts deliberately unsupported pending04.5. No later phase implemented.

Exact verification state:
- `cargo fmt --check`, `git diff --check`, `git diff --cached --check` PASS.
- `env SQLX_OFFLINE=true cargo check --locked --offline --all-targets` PASS before final
  test additions. Initial wrong SourceSpan import corrected; passing build warned unused
  kind, corrected before subsequent test compilations.
- `env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx migrate run` PASS; new migration28.63ms. Fresh isolated test DB migrations PASS.
- Initial `env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true
  RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_action_dispatch_`:
  4PASS/0FAIL/0IGNORE, compile65s/test3.14s. Competing local/restart receipt, effect/schema/
  insert/deferred-commit faults, stale fences/scopes/approval/errors, committed remote
  marker/crash/lost-response/no-replay/competing reservations. Compact terminal evidence
  `/private/tmp/workflow-04.3-tests-initial.log` (build banner compacted).
- Expanded same command with filter `workflow_action_`:21tests=14accepted+7new,
  19PASS/2FAIL/0IGNORE, compile48s/test7.24s. Complete terminal output saved at
  `/private/tmp/workflow-04.3-tests-expanded-failed.log`. Both failures are NEW fixture-only
  expiry SQL violating existing background_tasks_lease_check: post-marker expiry test
  and deferred SQL expiry test set expires into past but locked_at stays current. Correct
  by backdating locked_at together with expiry, using lease_tests::expire shape:
  `locked_at=clock_timestamp()-interval '10 seconds',
  lock_expires_at=clock_timestamp()-interval '1 second'`. Corrections NOT made yet.
- StrictClippy/final offlinealltargets/SQLxprepare+check/stock stack-budget/explicit graftbuild
  NOT RUN for final04.3. Graph auto-refresh on queries is not the final gate.

Independent initial actual-code audit NOT PASS. Source frozen during reviewer inspection.
F1: actions/dispatch.rs public RemoteReservation/RemoteReservationResult and public
ActionDispatch::reserve_remote/borrowed enter_remote, wildcard reexports, allow caller
reservation retention/repeated entry; fields additionally pub(crate). This contradicts
accepted internal/single-use boundary EXPANSION184–190. Current service itself uses its
local reservation once; reviewer found no duplicate-send exploit through dispatch_remote.
Successor must narrow protocol/type visibility and enforce consuming one-use final entry,
preserving public provider-neutral orchestration. Correction toward accepted contract,
not material expansion. No other concrete runtime defect found in initial audit.

Required remaining acceptance from reviewer (do not waive):
1. Service actual pending provider drop on ownership loss/cancel/deadline; delayed first
   polling refuses expired conservative monotonic window.
2. Inject ambiguous reservation acknowledgement AFTER genuine committed marker; zero
   initial calls and no retry/restart/reclaim call.
3. Post-marker authority revoke/cancel through dispatch_remote with provider poll counts;
   existing tests only call adapter enter_remote.
4. Membership/resource/policy authority locks held through effect+commit with competing
   revokers; current waiting-revoker test proves only inverse order.
5. SQL provenance negatives tenant/run/execution/digest/job/attempt/generation/worker and
   receipt linkage; independent deferred run deadline rejection. Lease deferred and
   following immutability assertions need expiry fixture correction above.
6. Model-tool saved invocation dispatch alongside step, wrong-digest rejection; complete
   required static/fresh migration/SQLx/stock2MiB+stack-budget/graft gates; independent
   corrected actual-code+acceptance review. Retain all14accepted workflow_action tests.

Implementer collaboration /root/dispatch_implementation, UUID
01a0f200-980e-73f2-bc6a-39463caca281; turn_context VERIFIED Sol6.1/high.
Own depth0 usage/runtime capacity samples:startup22638/2584008.76%11:08:56Z;
implementation91820/25840035.53%11:13:39Z; compile99399/25840038.47%11:18:23Z;
tests112484/25840043.53%11:22:57Z; review request115823/25840044.82%11:25:32Z;
checkpoint121890/25840047.17%11:28:18Z. Retire early before50%, no substantive corrections.
Reviewer /root/dispatch_implementation/dispatch_review, UUID
01a0f201-0674-7642-9c4a-1bf038a59fa8; turn_context VERIFIED Astra/medium;
startup26142/25840010.12%11:09:16Z; initial audit77282/25840029.91%11:27:06Z;
parent-observed77758/25840030.09%11:27:26Z. No edits/DB mutations/deeperagents.
This is initial audit, not full acceptance/PASS. Successor must create its own reviewer.

Retained taskPG started exact authorized options, no reset; final stop/quiescence appended
below. No stage/unstage/commit/reset/deploy, PROGRESS/RESUME edits or04.4 work. Root alone
accepts queue and rotates implementer. All historical unrelated changes preserved.

Final resource checkpoint: `pg_ctl -D /private/tmp/workflow-admission-pg-e3aa -m fast
-w stop` PASS; taskPG STOPPED/retained, exact endpoint/data/log unchanged. No active
build/test/SQLx/approval remains. Reviewer interrupt confirmed completed NOTPASS audit
and quiescence; retired subtree must not be reused. Post-brief git diff --check PASS.
Final worker depth0 sample128716/25840049.81%11:32:40Z usage/runtimecapacity;
retirement save+stop only. Graft saved estimate2,790,264tokens this worker turn (mostly
map orientation baseline, not measured runtime consumption). Root owns next assignment.

## Correction checkpoint2026-09-30; fresh review pending

F1 corrected: public ActionDispatch port exposes local/remote orchestration only;
RemoteDispatch, reservation/result/entry and supervisor are crate-private, explicitly
exported internally. Final entry takes ownership of non-cloneable RemoteReservation.
Service never returns a reservation/grant; provider-neutral RemoteAction remains public.
Same final locked boundary and immediate monotonic first polling supervision retained.
No applied migration edited, no bounds raised, no04.4 work or root-owned file edits.

Expiry fixtures backdate locked_at with expired lease; run expiry fixtures also backdate
created_at to preserve deadline>created_at. First corrected full acceptance run27/29
passed, two run-expiry fixture failures saved /private/tmp/workflow-04.3-corrections-first.log.
Second full run PASS29/29/0ignored, including all14accepted tests +15dispatch tests;
compile25.61s/tests9.12s, complete output /private/tmp/workflow-04.3-corrections-passed.log.
Exact command: env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true
RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_action_.

Six missing acceptance groups now present (actual code to be independently reviewed):
- action_dispatch_remote_tests: pending actual provider Drop/poll counts on ownership-loss,
  DB cancellation, run deadline and token cancellation; real final-entry delay beyond SQL
  lease window yields zero provider polls (monotonic check).
- Genuine committed-marker lost-ACK injection yields zero sends; retry/restart and real
  lease expiry/reclaim to attempt2 retain one marker and zero provider polls.
- Service post-marker resource/membership revocation, approval strengthening and run
  cancellation produce zero provider polls with retained possible-dispatch record.
- action_dispatch_sql_tests: resource/policy/membership revokers block while local effect
  transaction is held; only acquire lock after effect+receipt commit. Existing inverse
  contender order retained and passing. Uncommitted writes remain invisible.
- Direct SQL negatives company/run/execution/invocation/digest/job/attempt/generation/worker
  and receipt linkage reject; independent run deadline COMMIT rejection (live lease) rolls
  back marker/effect/receipt. Existing lease COMMIT/append-only checks now pass.
- Saved model tool-call invocation competes with step through same dispatch, one local
  effect+receipt; restart mapping preserved; wrong digest before and after commit rejects.

Worker /root/dispatch_corrections UUID01a0f218-63d0-7a70-96b4-7457f4d23862 runtime
turn_context verified gpt-6.1-sol/high. Startup22783/2584008.82%11:34:54Z;
correction65745/25840025.44%11:39:04Z; additions70514/25840027.29%11:43:06Z;
compile74430/25840028.80%11:45:30Z; passing83375/25840032.27%11:48:08Z.
Measurement token_usage_record.usage / task_started.model_context_window, depth0.
Reviewer /root/dispatch_corrections/dispatch_review UUID01a0f219-1721-7fc3-a2cc-0715bfd4b3b1,
Astra/medium; startup26228/25840010.15%11:35:32Z, idle readiness53697/25840020.78%11:36:21Z.
Review freeze and remaining gates to follow; root alone accepts04.3.

## Fresh actual-code review and final gates2026-09-30

Independent review /root/dispatch_corrections/dispatch_review inspected actual source,
callers/exports, immutable migration, all six added acceptance groups and passing test
log. F1 internal consuming reservation resolved; no additional runtime/security/concurrency
finding. Two local findings corrected: unnecessary &current in shared policy validator,
and unnamed (FrozenAction,Value,bool) adapter return. Named AuthorizedAction now carries
operation/current_policy/approval_required; all three production/two SQL-test consumers
preserve checks. Focused correction review PASS; reviewer actual runtime verified from
turn_context as Astra/medium. Reviewer correction sample105857/25840040.97%11:53:30Z.
Full prior actual-code/criterion audit retained; final evidence signoff remains pending.

Final corrected static gates PASS:
- cargo fmt --check; git diff --check; git diff --cached --check (no output/errors).
- env SQLX_OFFLINE=true cargo check --locked --offline --all-targets18.11s;
  log/private/tmp/workflow-04.3-offline-alltargets.log.
- env SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings41.68s;
  log/private/tmp/workflow-04.3-strict-clippy.log.
- Fresh EMPTY task DB workflow_dispatch_gate_20260930 created on taskPG endpoint;
  env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_dispatch_gate_20260930
  TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_dispatch_gate_20260930
  cargo sqlx migrate run PASS all40migrations through20260930120000;
  full log/private/tmp/workflow-04.3-fresh-migrations.log.
- env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx prepare -- --all-targets PASS16.70s and
  cargo sqlx prepare --check -- --all-targets PASS30.03s;
  logs/private/tmp/workflow-04.3-sqlx-{prepare,check}.log. Final .sqlx diff EMPTY.
- Explicit graft build PASS14242nodes/21136edges/712cards (all712cache replayed);
  summary/private/tmp/workflow-04.3-graft-build.log. Local graph gitignored.

SQLx/builds strictly sequential. Fresh DB creation/migration initially sandbox socket denied;
reran concrete commands with required escalation, PASS; no approval bypass. Retained taskPG
never reset. Final scoped scripts/stack-budget.sh --offline workflow_action_ running at
2048KiB after the two pure Rust refactors; combinedphase04/full-suite gate remains later.
Own boundary97723/25840037.82%11:53:16Z, final static100837/25840039.02%11:54:23Z.

Final post-refactor scoped stack-budget PASS29/29/0ignored: env
DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true
RUST_MIN_STACK=2097152 scripts/stack-budget.sh --offline workflow_action_. Script confirms
2048KiB; compile44.37s/tests12.06s. Complete log/private/tmp/workflow-04.3-stack-budget.log.
All14accepted tests retained, all15dispatch tests pass after final review corrections.
No widened stack/resource bound. Final code/gate sample103368/25840040.00%11:56:35Z.
Final reviewer evidence signoff/resource stop to follow.

## Verified04.3 handoff; root acceptance pending

Fresh combined independent actual-code +04.3 criteria +required verification PASS,
no unresolved findings. Reviewer confirmed final logs, empty SQLx diff, all six groups,
F1 and both code-quality corrections;14accepted+15dispatch tests PASS2048KiB.
No full-suite/combinedphase04 pass implied; later04.4–11/05–10 obligations retained.
Reviewer final109385/25840042.33%11:57:31Z, UUID01a0f219-1721-7fc3-a2cc-0715bfd4b3b1,
actual Astra/medium runtime verified. Interrupt confirmed completed PASS and quiescence.

Task-owned fresh migration DB workflow_dispatch_gate_20260930 removed successfully;
retained workflow_admission data never reset. TaskPG STOPPED cleanly via pg_ctl -D
/private/tmp/workflow-admission-pg-e3aa -m fast -w stop. Restart exact accepted options
and same endpoint if needed. No active build/test/SQLx/approval or reviewer work remains.
No stage/unstage/commit/reset/deploy; all unrelated modifications preserved. Root owns
PROGRESS/RESUME and accepts current point before04.4. Implementer waits for root.
Known graft savings from printed lines at least263912tokens (first query output truncated,
not counted); estimate of avoided whole-file reads, not runtime consumption.

Final implementer depth0 sample107484/25840041.60%11:59:04Z; latest parent-observed
reviewer109766/25840042.48%11:57:40Z. Sources usage/runtimecapacity as above.
Both below50%; natural point handoff, no further source work until root acceptance.
