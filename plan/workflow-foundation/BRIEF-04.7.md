# 04.7 — authorized reconciliation evidence and settlement

Status: concrete affected expansion independently PASS after grouped R1 corrections;
root ACCEPTED 2026-09-30 18:57Z.
Expansion assignment completed without implementation/PG. A partial typed boundary now
exists (handoff below); full04.7 implementation and root acceptance remain pending.
Original execution item 6 in 04-actions-http-and-delivery.md is authoritative.

## Scope and decisions

Add an application-owned command attaching bounded evidence and recording applied,
not-applied, or unknown for an exact frozen remote invocation. Only verified final
not-applied permits another unsafe remote entry. Applied output allows receipt replay;
unknown never schedules. Terminal runs retain truth and never reopen through this
command. Preserve accepted 03/04.1–6 ownership, immutable invocation/digest/key,
receipt-first results, tenant/resource authority and existing deadline/attempt/budget
owners. 04.8 full cancellation matrix, 04.9–11 production transports/messaging, 05
protected actions, 06 Rig, 08 bypass removal and 10 final gates remain mandatory.
No HTTP/UI/model-tool command, production verifier/provider, compatibility/backfill,
new queue/lease/attempt owner, bound increase, or edits to applied migrations through
20260930171000. This foundation is exercised with a registered authoritative fixture.

### 1. Command authority, identity and replay

- Add `WorkflowOperation::Reconcile` and a narrow `ActionReconciliation` persistence
  port plus application `ActionReconciliationService`; keep lifecycle authorization
  and frozen-action/resource authorization distinct. Reuse WorkflowActor, ActionScope,
  ApprovalSubject, RunRevision and IdempotencyKey; add domain newtypes for evidence,
  command, marker, entry, verifier registration/reference and coverage digest.
- `ReconcileActionCommand` carries authenticated actor, company/run/execution,
  invocation/digest, original remote marker, expected run revision, stable command key,
  and `EvidenceInput::{UnknownNote, VerifiedReference}`. A reference names an approved
  verifier registration and bounded opaque evidence reference; the input never carries
  a trusted verdict, receipt, quiescence boolean or deserializable dispatch grant.
  UnknownNote can state a claimed disposition for audit; effective disposition remains
  Unknown. Successful verifier results classify Applied (optional recovered output),
  FinalNotApplied, or Unknown; errors propagate, unsupported registrations reject.
- Preflight current owner/admin and actual run association before any scope/proof read.
  Persistence repeats authorization under company/principal locks before run, then
  association locks, execution and existing job. Do not call the run-first dispatch
  authority loader from this command: a narrow scoped frozen-intent restore validates
  run/bundle/resources/digest/marker while accepting parked/terminal run states.
  Reconciliation requires current command actor access to the frozen resource through
  the application-owned resource policy port, rechecked transactionally at settlement;
  no current I/O approval is needed to record past truth. Resource lookup errors fail
  closed. Original run actor/current resource/ceiling/policy/approval still govern every
  subsequent receipt return or actual dispatch; reconciling actor cannot replace them.
- Reauthorize even exact replay. Command namespace is `(company,command_key)` in a
  dedicated immutable command table. Its request digest hashes canonical versioned
  command bytes, including actor/scope/subject/marker/revision/input/reference/note.
  Existing key with any different bytes returns IdempotencyConflict. An exact replay
  returns its stored result/revision without re-verifying or rescheduling; this remains
  valid after proof expiry but supplies no fresh permission. RevisionConflict records
  a refusal without evidence or transition. Outcomes distinguish UnknownRecorded,
  AppliedRecorded (receipt/no usable result), NotAppliedRecorded, Scheduled (receipt
  continuation or retry), Blocked (typed reason), EvidenceConflict and Replay; errors
  remain distinct from non-disclosing missing/access rejection.

### 2. Trusted proof and verification-to-commit binding

- Trust is an explicit, application-owned `ActionEvidenceVerifier` port paired with an
  approved registration for the exact frozen tool contract/target signature, company,
  provider and verifier version. Registration comes from trusted host composition,
  analogous to approved ProviderReplayContract, never provider discovery/response,
  policy JSON, note, model input or caller-provided verifier. No default success method;
  unsupported production registrations fail closed. The only new implemented adapter
  here is a database-backed test provider/evidence ledger with real applied effects.
- A verifier receives the restored frozen action, stable invocation identity/key,
  original marker and sorted exact prior remote entries with full attempt/generation/
  worker provenance. It verifies authentic authoritative evidence using its trusted
  provider contract. Applied requires a positive operation-specific effect observation;
  recovered output is separately bounded/schema-validated. FinalNotApplied requires
  **final non-application and quiescence of every earlier request that could still apply
  this exact operation**, including marker-before-entry reservation. Mere absence now,
  expiry, timeout, cancelled local future, failed status or HTTP/tool metadata cannot
  satisfy it. A verifier unable to prove old provider work permanently unable to apply
  returns Unknown. It does not infer quiescence from an expired local lease.
- Finality applies to the covered prior requests, not a permanent tombstone prohibiting
  a future newly authorized request. The registered adapter must correlate all old
  requests by exact operation/ledger identity and exclude their later application; if
  its provider cannot distinguish final settlement from momentary absence it cannot
  register this contract. No change to RemoteAction makes an arbitrary remote field
  authoritative. Future production adapters must implement this contract before use.
- Two bounded transactions surround external verification: authorized snapshot under
  owning run lock, release locks, cancellable verifier call, then authorization/locks
  again and settlement. Snapshot contains canonical operation bytes, exact marker and
  entry set, expected revision and coverage digest. Settlement re-reads all of these;
  any changed revision/marker/entry set/operation is a stale-proof refusal with no grant.
  Do not hold database locks during provider I/O. Replays are detected before verification.
- VerifiedEvidence is an internal non-deserializable value issued through the approved
  verifier path, bound to snapshot digest, registration/version, authoritative record
  reference, observed time, verification time and valid-until. It is not a public setter
  for a verdict. Persistence independently checks structural scope/time/digest and
  registration binding; arbitrary JSON cannot enter this trusted method through command
  decoding. This is application trust, not a claim SQL authenticates provider signatures.
- Use DB clock for snapshot/acceptance and conservative time comparison. Verification
  deadline is min(5 seconds, command operation budget); no detached work. Proof validity
  is positive, no more than 24 hours from verification, and cannot exceed the provider's
  attested validity; future observed time or observed time before covered entry creation
  rejects. At commit and actual new-entry commit require unexpired FinalNotApplied.
  A proof may be audit-only after run expiry, never schedule; exact duplicate replay
  still returns old result. Applied truth does not expire into permission to resend.
- Bounds at input, verifier return, stored decode and SQL: note <=4096 UTF-8 bytes;
  references/registration <=128 bytes with explicit grammar; evidence envelope <=16384
  canonical bytes excluding recovered result; recovered output uses existing action
  result limit/schema. Cover at most128 prior entries and128 invocation siblings;
  read129 to detect overflow and return conservative BoundExceeded, never truncate.
  Malformed persisted values fail closed. No secrets/raw transport payloads are stored.

### 3. Append-only facts and applied receipt provenance

- Add additive migration(s) with immutable tenant-scoped evidence and commands.
  Evidence records company/run/execution/invocation/digest/remote marker, actor,
  command identity/request digest, source/version/reference, effective disposition,
  coverage digest/times and bounded versioned diagnostic payload. Coverage join rows
  bind evidence to every exact prior entry via composite tenant/scope/digest/marker FKs;
  the original marker is included even when coverage has zero entries. Evidence and
  command linkage must be constrained both ways at deferred commit, not orphan facts.
  Keep workflow_action_reconciliations as immutable first observations, never proof.
- Applied with validated recovered output inserts into the existing authoritative
  workflow_action_receipts table with an additive `reconciliation_evidence_id` source.
  Preserve local and remote kinds, but require exactly one remote source: actual
  remote_entry_id OR verified applied reconciliation evidence. Composite FK plus
  disposition/guard enforces full same-company/run/execution/invocation/digest/marker
  provenance; local receipts have neither remote source. Never invent a remote entry.
  Receipt-on/output validation/shared completion continue reading one receipt owner.
- Applied without output, or with output rejected by frozen schema/size, records Applied
  truth and a typed MissingResult/InvalidRecoveredResult diagnostic, no receipt/output/
  route/error edge. This vetoes unsafe repeat and continuation. A later distinct valid
  applied command may supply recovered output for the same frozen operation; duplicate
  command cannot reinterpret its old output. Applied truth never downgrades to not-applied.
- Existing receipt has truth precedence. Differing new output/disposition is retained as
  conflicting evidence with EvidenceConflict, not overwritten. Unknown never overwrites
  Applied or FinalNotApplied. An unconsumed verified final proof projects not_applied;
  after consumption without receipt it projects needs_reconciliation again. Receipt
  projects committed, Applied without result projects applied_without_result. Preserve
  the old first-uncertainty audit alongside every projection.
- Contradictory applied evidence/late actual receipt covering a prior FinalNotApplied
  is an immutable conflict fact linked to both evidence and actual entry/receipt.
  Actual bounded receipt is still accepted as truth, even terminal; accepted receipt
  is never deleted. Conflict permanently disables further reconciliation retry grants
  and automatic continuation for this execution in this foundation (future explicit
  remediation contract is separate). A pending reopened job is parked atomically;
  an already processing attempt is not stolen: remote heartbeat/entry gate detects
  conflict and existing exact-fence retirement parks it. Check the same execution-level
  conflict decision under the run lock at **every successful receipt-return and shared
  completion boundary**, including reserve_remote receipt replay, return_authorized,
  and complete_on before batch_commit. A handler that already obtained a valid receipt
  before conflict cannot commit output/route/successor afterward: complete_on retires
  its exact live fence with classified action.evidence_conflict and returns no committed
  result. Stored receipt remains intact; a completed execution replay is historical
  truth, not a new transition. Shared action-safety predicate
  must veto this conflict even when a receipt exists; projection still shows receipt
  truth with conflict separately. A trusted finality breach cannot authorize another
  call or erase a possibly duplicate effect. Terminal runs never advance.

### 4. One-use final not-applied and shared predicates

- `workflow_action_not_applied_available(company,invocation,excluded_entry=NULL)` is
  the sole SQL decision for evidence safety, used by scheduling, Rust marker selection
  and deferred entry guard. It requires exact immutable operation/marker coverage,
  unexpired final proof, no receipt, no applied-without-result/conflict, no existing
  consumption, no processing old attempt, and coverage equal to **all** remote entries
  currently present (apart from an explicitly matched consuming NEW entry). Covering
  a subset or only the first marker attempt rejects. Trusted registration is still
  checked by the application before use; this predicate is not an I/O grant.
- One effective final proof per coverage digest; competing different commands cannot
  mint multiple consumable grants for the same coverage. An immutable consumption
  table links one evidence ID to one newly reserved entry with full composite provenance;
  UNIQUE evidence and entry prevent dual use. Consumption is committed with new entry
  reservation, before any provider polling. A crash/ambiguous acknowledgement consumes
  it conservatively; no call/result refunds it. A new proof must cover that new entry.
- Marker remains immutable; later attempt selects the exact original marker using the
  shared available-proof predicate, alongside the unchanged supported replay branch.
  `reserve_entry_on` inserts consuming entry and consumption in the same transaction.
  Deferred guard checks consumption refers to NEW and proof covers all *other* entries,
  validity/final authority, exact live fence and original subject; it does not reject
  its own matched consumption or ignore any unrelated entry. No prior processing entry
  or same-attempt competing entry can pass. Entry/receipt verification recognizes this
  provenance without rewriting historical marker policy or fabricating provider replay.
  Entering the provider rechecks current fence/policy/access and conflict; an expired
  proof before enter prevents the call, with conservative consumed audit retained.
- Extend workflow_action_retry_safe with this ordered truth table per invocation:
  conflict => false; else usable receipt => true; else applied-without-result => false;
  else supported replay OR available final-not-applied => true; otherwise false. Thus
  applied-without-result (including invalid recovered output) blocks supported replay
  and every other schedule/continuation branch until a later valid receipt resolves it.
  Conflicts remain an absolute veto even after a usable receipt. Keep NULL no-marker and128-sibling overflow behavior.
  All siblings must satisfy the predicate. Rust calls the shared SQL decision rather
  than reimplementing it; any necessary pure Rust classification has a database-backed
  equivalence matrix. Receipt siblings replay; proof siblings consume individually;
  unresolved siblings keep the execution parked without extra attempt/debit.

### 5. Atomic existing-job continuation and terminal boundaries

- Evidence recording attempts optional settlement in the same transaction as evidence,
  receipt, consumption-independent command receipt and actor audit. Only a non-child
  `waiting/reconciliation` run with uncompleted activated execution and its failed,
  lease-free existing workflow job can reopen. Terminal Failed/Cancelled/Succeeded,
  including deadline-expired runs, are audit-only. This command does not replace
  ordinary RetryCommand for failed runs or human/child wait resumes.
- Require no committed output/route/successor, no runnable sibling, deadline > DB now,
  activation <= max_steps, remaining job attempts, no activation/invalid-result/deadline
  poison, and current root budget eligibility. Preserve all ordinary control retry
  constraints except the historical-safe-attempt condition: reconciliation has its own
  evidence gate and does not rewrite attempt safety/class/code/retirement, retry_count,
  max_retries, activation, lineage or budget debits. Exhausted pending paths with no
  suitable retired attempt remain blocked. Prior budget-exhausted retirement remains
  blocked; budget accounting is neither replenished nor reset. Claim reserves any next
  attempt/budget through the existing owner and must recheck exhaustion as usual.
- All markers receipted means receipt-only continuation; receipt and final-proof mix
  means retry only proof siblings. All-final-not-applied means unsafe retry eligibility;
  supported siblings keep their accepted branch. Applied-without-result or any unknown
  means Blocked and no schedule. Command never completes the step itself: it sets the
  existing job pending/run running, then the normal fenced handler returns receipts or
  performs permitted new entries and uses existing shared completion.
- Add a narrow reconciliation reopen predicate and deferred failed-to-pending branch
  in a **new** migration. Retain the existing ordinary retry branch unchanged. New
  branch requires applied reconciliation command receipt at the resulting run revision,
  exact scoped execution/job and evidence IDs, actor audit, previous waiting state and
  all gates above. Do not accept any reconciliation receipt that merely recorded truth,
  or one from a different revision/job. Evidence and conflict insertion each advance run revision through an additive
  AFTER INSERT trigger invoking existing advance_workflow_owned_revision(): their NEW
  company_id/run_id select the existing non-background-task branch, and its nested
  UPDATE participates in advance_workflow_run_revision without top-level revision
  writes. Current workflow_run_events has no revision trigger and remains audit only.
  Attach these triggers only to new evidence/conflict fact tables; command receipts,
  coverage rows, refusal/replay and ordinary events do not bump revision. Reconciliation
  evidence is inserted before optional existing job/run changes and actor event, then
  read the final generated revision after all fact/state mutations and insert the
  immutable command receipt last. Multiple legitimate fact/state triggers can bump
  more than once; no assumption of expected+1. The deferred reopen guard compares
  exactly that final revision and checks linked evidence/audit, while evidence-only
  waiting/terminal commands invalidate stale distinct requests. Revision overflow
  fails the entire transaction through the existing guard; no bound increase.
- Run deadline/poll_work/expire_run remain reconciliation expiry owner. Expiry/cancel/
  evidence/late receipt serialize through owning run; no new workflow_waits/sweeper.
  Once terminal, valid bounded truth can attach, commands cannot reopen, and no success
  output/edge is invented. The full 04.8 cancellation matrix remains separate.

## Actual-tree seams and deliverables

- Current authority: application/workflow/authorization.rs:64–98,147–189;
  persistence/workflow/controls.rs:64–152 (company-before-run control locks/replay),
  action_authority.rs:30–94 (dispatch-only loader must remain dispatch-only).
- Existing retry: control_retry.rs:3–42; immutable controls migration:144–188.
  Add application actions/reconciliation module and separate cohesive persistence
  action_reconciliation module; registration/verifier port stays application-owned.
  Domain IDs/pure classifications stay inside domain/application, SQL adapters outside.
- Marker/reservation/provenance: action_receipts.rs:60–143; action_dispatch.rs:213–329;
  receipts/late truth: action_receipts.rs:145–226. Preserve existing supported replay and
  receipt-before-return checks. Original facts migration150000:9–41,98–150;
  uncertainty migration170000:1–67 and failure identity171000 remain immutable.
- Parking/recovery owners: recovery.rs:21–96,152–168; successful completion
  completion.rs:39–110 (new conflict decision before batch_commit), pending_recovery, maintenance,
  controls cancellation and shared completion/lease/budget callers. Trace exact callers
  with graft before edits. Add isolated persistence action/evidence acceptance tests and
  synchronous domain/application input/classification tests; existing fixture provider
  implements authentic applied/final-absence/quiescence rather than sequential mocks.

## Required discriminatory acceptance checks to retain

- Current owner/admin, wrong company/association/member/outsider, revocation races,
  replay after revocation, directory errors, forged actor/subject/digest/marker, malformed
  or oversized evidence and unsupported proof source all fail closed without scheduling.
- Unknown evidence remains unknown across restart and duplicate commands; ordinary
  retry, safe caller labels, timeout/cancel/error metadata cannot manufacture proof.
- Actual lost remote effect + valid applied output yields durable linked receipt before
  continuation; restart returns saved result with zero I/O. Applied-without-result
  never invents output. Invalid recovered output does not fabricate success.
- Genuine final not-applied proof schedules exactly one eligible existing job, preserves
  old attempt/debit and frozen operation, rechecks current dispatch authority, and
  permits one later actual call. Competing commands/claimants consume one proof once.
  A lost response after that call parks again and cannot reuse the consumed proof.
- At least two unresolved marker siblings: one proven not-applied cannot hide the
  other unknown. All-applied receipt replay and all-proven-not-applied retry cases
  remain distinct. Bound-overflow and expired proof stay conservative.
- Competing applied/not-applied/unknown commands, same-key replay/conflict, late actual
  receipt, cancel and expire all serialize under the owning run; accepted receipt wins,
  terminal runs never advance, and no proof authorizes concurrent old/new provider I/O.
- Inject deferred commit failure: evidence/receipt/command/audit/run/job reopen roll
  back together. Database provenance negatives isolate actual composite FK/check/
  immutability failures, with meaningful competing-claimant tests rather than mocks.
- Stock2048KiB action plus affected control/recovery/receipt/admission suites, required
  format/whitespace/offlinealltargets/strictClippy, fresh migrations/schema, SQLx
  prepare/check and graft build, then independent actual-code/criteria/integration
  PASS. Full phase04 matrix remains04.11; no full completion claim from subset tests.

## Additional discriminatory checks and required gates

- The verifier refuses final absence while an old provider request is still capable
  of applying; release that delayed actual effect and observe Applied/Unknown, zero
  new dispatch. A genuine quiescence barrier then permits only a new entry. Verification
  paused between snapshot/settlement races revocation, changed coverage, revision,
  expiry and late receipt; no stale verified token commits a grant.
- Unknown/applied audit-only commands on waiting and terminal runs advance generated
  revision; distinct stale commands conflict, duplicate/refusal does not bump, scheduled
  receipt stores the exact final revision after fact/job/run mutations. Deferred commit
  rollback includes the generated revision and actor audit.
- Valid result returned to a live worker, then contradictory late truth commits, then
  complete_io before heartbeat: receipt remains, zero committed output/edge/successor,
  exact attempt retires. Conflict before receipt replay refuses output; opposite order
  (completion wins run lock first) retains committed history and never reopens it.
- Supported replay plus Applied without usable result, invalid recovered result, and
  later valid receipt run through real recovery/command paths and SQL/Rust truth-table
  equivalence; no premature scheduling or remote polling.
- Not-applied once, later call loses response, restart and ordinary retry cannot reuse
  proof; fresh proof covers both entries. Two simultaneous proof commands plus two
  dispatch claimants create one consumption/entry. Zero-entry marker crash is covered
  explicitly. At129 entries/siblings all relevant gates return bounded refusal.
- Reconciliation receipts reject source XOR, foreign evidence/digest/marker/entry,
  non-applied disposition, orphan command, mutated/deleted evidence/coverage/consumption;
  each SQL negative isolates the intended FK/check/guard, not an earlier constraint.
- Late applied receipt both before and after consumption retains truth/conflict, parks
  pending work or interrupts live work through existing retirement, never overwrites
  receipt or reopens a terminal run. Include applied-without-result later recovered
  output, conflicting valid outputs, same key different request and exact replay after
  authority revocation. Test all receipt/proof/unknown sibling combinations.
- No PG/checks in this expansion assignment. Implementation gate must run with exact
  retained DATABASE_URL and TEST_DATABASE_URL, SQLX_OFFLINE=true and RUST_MIN_STACK=
  2097152: action tests plus affected admission/control/recovery/budget/lease/completion/
  pending/maintenance suites. Then cargo fmt check, git diff --check, locked offline
  all-target check and strict Clippy, fresh task-owned migrations/schema inspection,
  cargo sqlx prepare and prepare --check --all-targets, graft build. Full phase04 matrix
  still belongs to04.11. No gate can be reported passed from an expansion review.

## Expansion evidence and resources

Fresh worker Sol6.1/high UUID01a0f39e-70eb-75e0-819b-f09c1daf8865.
Startup depth0 codex20,054/258,400=7.76%@18:40:52.904Z; feasibility milestone
82,249/258,400=31.83%@18:44:05.803Z, runtime usage/window sources. No source,
migration, SQLx or database changes. Root owns PROGRESS/RESUME. PG remains stopped;
retained data/private/tmp/workflow-admission-pg-e3aa, log/private/tmp/workflow-admission-
postgres.log, both URLs postgres://mac03@127.0.0.1:55439/workflow_admission.
Independent fresh Astra expansion review PASS; root ACCEPTED 2026-09-30 18:57Z. No unresolved
architecture or user approval decision. Implementation and every code/database gate remain pending.

R1 independent report: /private/tmp/workflow-04.7-expansion-review.md, three grouped
findings corrected before renewed review: explicit evidence/conflict revision triggers
and final command revision order; conflict gates at receipt return and successful
completion (including result-return race); ordered applied-without-result precedence
over supported replay. Original criteria unchanged. Corrected affected expansion independently PASS; see
/private/tmp/workflow-04.7-expansion-review-final.md. No remaining concrete blockers.

Fresh reviewer /root/reconciliation_expansion/review_expansion, Astra/medium, verified
UUID01a0f3a7-152e-7d60-9f83-2cf524920895, codex depth0 startup35,729/258,400=13.83%
@18:50:25.028Z; final review94,787/258,400=36.68%@18:55:17.162Z, runtime usage/window.
Owner-confirmed fresh reviewer96,095/258,400=37.19%@18:55:28.343Z (latest usage/window
info); implementer100,707/258,400=38.97%@18:55:34.264Z runtime usage/window. Reviewer
quiescent, expansion-only assignment complete; parent acceptance and next worker/code
assignment remain root-owned. No active resource operation or code change.

## Accepted expansion implementation handoff

Root accepted this full affected expansion after final independent Astra PASS and
all three R1 corrections, 2026-09-30 18:57Z. Expansion-only subtree retires naturally
before implementation; no source/SQL/migration/PG work authorized to this subtree.
Fresh Sol6.1/high implementer must create its own fresh Astra/medium reviewer and
implement this accepted brief against original04 execution item6, preserving accepted
04.6 and all modified/untracked work. Root owns PROGRESS/RESUME and implementation
queue acceptance. No unresolved architecture blocker or user approval gate.

Start with targeted graft/callers at the exact listed seams; implement typed command/
verifier/persistence ports and scoped evidence/source/consumption/conflict schema in
new migration(s) after171000. Existing proof/output/replay paths and old SQL guards
remain until narrowly extended. Apply the ordered safety predicate, generated revision
triggers and atomic waiting-only existing-job reopen; close receipt-return/completion
conflict race. Do not reset attempts/debits or fabricate transport entries. A material
contract change requires affected expansion re-review before code; local choices do
not. Required meaningful tests and stock2MiB/static/freshschema/SQLx/graft gates above
remain entirely unrun for04.7. Read skill PostgreSQL reference before authorized PG
work; use retained exact both URLs/data/log and preserve immutable applied migrations.
Final overall actual-code/criteria/integration PASS/root acceptance is separate from
this expansion acceptance.

Reviewer is quiescent/retired with reports retained; implementer saving handoff then
quiescent/retired. Parent-confirmed worker103,776/258,400=40.16%@18:56:48Z; reviewer
96,095/258,400=37.19%@18:55:28.343Z. Both remain below50%; no active resource operation.
Graft estimated savings in this worker133,482 tokens; reviewer120,757 separately.


## Partial typed implementation handoff — 2026-09-30 19:41Z

Full04.7 remains **implementing / unverified**. Root has NOT accepted the point.
Only application/domain boundary exists; no04.7SQL/migration/PG/dispatch changes.
Worker `/root/reconciliation_implementation`, verified UUID
`01a0f3af-cc31-7750-b10f-f59327d0bc2e`, Codex Sol6.1/high. Own depth0 samples:
startup20303/258400=7.86%@18:59:59.677Z; milestone88248=34.15%@19:03:53.108Z;
104173=40.31%@19:19:51.439Z;118486=45.85%@19:37:04.736Z;
124245=48.08%@19:40:42.898Z. Runtime usage/window sources, capacity258400.
Retiring conservatively now; no further substantive implementation in this subtree.

Files: domain workflow/evidence.rs (bounded evidence identities/digests), ids.rs/mod.rs
(evidence/command/marker/entry/attempt UUID types/exports), application workflow
 authorization.rs (`Reconcile`), actions/mod.rs and actions/reconciliation/{mod,
contracts,verification,service,tests,service_tests}. Pure command canonical digest
binds every request component. Snapshot exact sorted coverage includes zero-entry
marker, rejects duplicates/invalid provenance/128overflow. Input has no verdict/output/
quiescence grant; unknown claimed notes stayUnknown. Applied missing/invalid output
retains positiveApplied truth and typed diagnostic, with no fabricated success.
Service preflights owner/admin before association read, actual association and current
command-actor resource access; verifier/whole orchestration is bounded/cancellable.

Trusted host installs one optional immutable verifier at service construction;
`reconcile(command,cancellation,budget)` accepts no caller verifier. Registration and
VerifiedEvidence fields are private; issue/unknown are reconciliation-owner-only,
read-only getters available to adapters. Exact requestedID/company/frozen signature
checked before polling; token binds request/coverage/source/version/reference/times.
DB-clock verification anchor plus monotonic elapsed is conservative; concrete adapter
must independently enforce actual DB-clock acceptance/validity and exact snapshot.
The narrow `ActionReconciliation` port has required association/snapshot/settle methods
and explicit two-transaction authority/lock/provenance/atomic-revision obligations.
No implementation of that port exists yet. Production verifier/UI is not claimed.

Independent fresh own Astra/medium reviewer
`/root/reconciliation_implementation/review_typed_boundary`, UUID
`01a0f3c3-eb2b-7772-bafd-db3aad3a1fae`. Initial report
`/private/tmp/workflow-04.7-typed-review.md` requested3P2 fixes: token confinement,
host-owned source selection, discriminatory128/129coverage. All corrected together.
Final **PARTIAL-boundary PASS** `/private/tmp/workflow-04.7-typed-review-final.md`;
no material architecture deviation/no expansion re-review needed. Own depth0 reviewer
startup27200/25840010.53%@19:21:50.565Z; final82830/25840032.05%@19:38:29.529Z;
parent-confirmed84177/25840032.58%@19:39:03.865Z. Reviewer is quiescent/retired;
fresh successor creates own reviewer, do not reuse this child. Fullpoint review remains.

Partial verification only (all Cargo operations sequential):
- `env SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_action_reconciliation`:
  initial6PASS0.05s `/private/tmp/workflow-04.7-typed-tests-r1.log`;
  corrected9PASS/0failed/0ignored0.07s after1m01compile,
  `/private/tmp/workflow-04.7-typed-tests-r2.log`. Six synchronous contract tests plus
  three service preflight/source/error tests; no database-backed reconciliation tests.
- `cargo fmt --all -- --check`:PASS `/private/tmp/workflow-04.7-typed-final-fmt.log`.
- `git diff --check`:PASS `/private/tmp/workflow-04.7-typed-final-whitespace.log`.
- `env SQLX_OFFLINE=true cargo check --locked --offline --all-targets`:finalPASS12.64s,
  `/private/tmp/workflow-04.7-typed-final-check.log`. Earlier48.07sinitial had unread
  token-field warnings, resolved through read-only getters; r2PASS12.77s.
- `env SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings`:
  PASS1m22s `/private/tmp/workflow-04.7-typed-clippy.log`.
- `graft build`:PASS728files/14453nodes/21314edges,
  `/private/tmp/workflow-04.7-typed-graft.log`.

All accepted originalSQL/behavioral/integration obligations above remain OPEN:
additive immutable tenant-scoped evidence/commands/coverage/consumption/conflict tables,
receipt sourceXOR+composite provenance, evidence/conflict generatedrevision triggers;
concrete snapshot/settlement adapter with authorization/DBclock/replay/stalebinding;
shared available-finalproof and ordered retry-safe truth table across scheduling,
marker selection/deferred guard, consume oneproof with newentry beforepolling;
late truth/conflict parking and gates at reserve_remote/return_authorized/complete_on;
waiting-only existingjob reopen preserving attempts/debits/budget/deadline/lineage;
real authoritative DB fixture proving finalquiescence and actualeffects; full exact
meaningful matrix (revocation, coverage/revision/expiry races, lateeffect/receipt,
competitors, siblings/overflow, oneuse/restart, rollback, terminal and isolatedSQL
negatives). Then BOTHretainedURLs stock2MiB action+affectedadmission/control/recovery/
budget/lease/completion/pending/maintenance, finalstaticchecks, freshschema/migrations,
SQLxprepare/check, graftbuild, independent actual-code/integration PASS/root acceptance.
No fullphase04/fullsuite claim from these nine tests.04.8 onward remain untouched.

Next worker starts with targeted graft at accepted SQL/receipt/control seams, preserving
this typed boundary and accepted04.6; adapter integration may add read-only registration
metadata getters. Prefer synchronous helpers if integration touches the orchestration
function to satisfy source-size guidance without growing async depth. Before PG read
skill reference and restart inherited stoppedcluster with exact options. Data/log/URLs
unchanged: `/private/tmp/workflow-admission-pg-e3aa`,
`/private/tmp/workflow-admission-postgres.log`, both
`postgres://mac03@127.0.0.1:55439/workflow_admission`. NoPGoperation was run here;
cluster remained stopped. NoSQLxcache changes or appliedmigration edits. All checks
finished, noactiveoperations at transfer. Baseline hashes/diff preserved at
`/private/tmp/workflow-04.7-preimplementation-hashes.json` and
`/private/tmp/workflow-04.7-preimplementation.diff`; prior49paths unchanged except
expected actions/mod.rs addition (before this brief update). Root owns PROGRESS/RESUME.

Retirement-only final immutable migration hash audit (retained files unchanged):
- `20260930090000_workflow_action_intents.sql` SHA256 `c80798ca450824294e6671694bd85d9da9ee0262daf438395ddd94cb7ae55392`.
- `20260930120000_workflow_action_dispatch.sql` SHA256 `8bd3ceff7491221bdf6c021c6bc1cd7b77bcb7779047328f04aefa1da9fef784`.
- `20260930150000_workflow_action_receipts.sql` SHA256 `e20d987b23a198a088199917b05b8b840117ba9c76e70504e9a0992f1bdea9ae`.
- `20260930160000_workflow_action_replay_validation.sql` SHA256 `5c394b8d5387f7e34b6bd630f5355c346841d7361e79fdc1f26ee4bc04b3e0cb`.
- `20260930170000_workflow_action_uncertainty.sql` SHA256 `a400112030c474ae8ad0ff02fe207d745f45805ddab37be56e8b1247d5d528f3`.
- `20260930171000_workflow_action_failure_identity.sql` SHA256 `3a53fae6968478e761b83fd05af920fb2694b477b8b889c86e9c98496b29d283`.
Final own depth0 sample127476/258400=49.33%@19:42:43.535Z, runtime usage/window; retire before50. All resource/check sessions completed; reviewer quiescent.

## Affected schema correction contract — 2026-09-30 20:04Z

Independent partial schema review found five actual counterexamples in applied
180000; see `/private/tmp/workflow-04.7-schema-review.md`. This migration is now
immutable SHA256 `ad9be5ea22c694eaa91068446c1a24a309cfca33040668ef4da933e7db3e81ab`.
Corrections are additive. Full04.7 remains implementing/unverified, not accepted.

The material affected refinement is **applied request attribution**. A trusted
Applied attestation additionally names `AppliedEvidenceRequest::{RemoteEntry(id),
MarkerReservation,Unattributed}`. VerifiedEvidence privately binds it. RemoteEntry
must be one exact snapshot-covered entry; MarkerReservation is valid only when
there are no prior entries. Unattributed records positive operation truth and can
recover a bounded output, but cannot claim which older request applied. SQL stores
this distinction (optional exact entry FK plus bounded attribution discriminator).
A recovered receipt after an earlier final proof is a demonstrated finality breach
only if attribution names that proof's covered request (or its zero-entry marker).
A new attributed entry outside the old proof is legitimate; do not create conflict.
An unattributed Applied observation in the presence of earlier final proof records
an immutable **unattributed_applied** conflict reason, meaning unresolved provenance,
not demonstrated finality breach; retain receipt truth but suppress automatic
continuation pending future explicit remediation. Proven old-request contradiction
uses reason **finality_breach**. Both veto every shared conflict gate. Caller input
still has no trusted verdict/output/attribution; host verifier owns this statement.
Discriminatory tests cover old-entry breach, legitimate consumed new-entry recovery,
and unattributed positive observation; no historical proof can be silently upgraded.

Other corrections stay within reviewed contracts: exact operation scope equality
in deferred command/evidence linkage; database-generated transition witnesses for
actual OLD waiting/reconciliation to running, linked to exact command/job/final
revision at deferred commit while ordinary retry unchanged; default immutable
full-transaction IDs on entries and consumption with deferred equality guard to
reject historical retrofit; conflict parking limited to uncompleted current
execution, preserving already committed output/successor history. Test direct SQL
terminal/human-wait reopen and retrofit negatives independently. No source/schema
correction depending on the material refinement begins before affected independent
expansion PASS and root acceptance. All original meaningful matrix/gates persist.

Affected refinement review R1 required database-enforced transaction identity (a
DEFAULT alone is caller-overridable). Entry and consumption BEFORE INSERT triggers
must set or reject identity against `pg_current_xact_id()`, and deferred consumption
checks exact entry/consumption/current xid8 equality. Existing entries receive NULL
history identity, never synthetic backfilled reservation identity. Explicit spoofed
historical transaction-ID negative must fail independently of timestamps.

The state witness records the first OLD state/wait reason for each run/current full
transaction at actual state transitions (immutable, no revision bump). A transaction
that goes terminal→waiting→running therefore retains its original terminal state
and cannot masquerade as a reconciliation wait. Deferred reopen checks its genuine
initial waiting/reconciliation witness, exact scoped job/command and final generated
revision. Witness inserts are database-trigger-owned, reject direct caller writes;
ordinary retry uses the existing independent branch and remains unchanged.

Witness capture includes the first change to **either state or waiting_reason**,
including waiting_reason-only changes. Thus a human wait cannot become eligible
through human-wait→waiting/reconciliation→running in one transaction; the witness
retains its original human reason. Add that isolated direct-SQL negative.

## Partial schema implementation handoff — 2026-09-30 20:06Z

Root ACCEPTED the affected applied-attribution/transaction-witness correction
expansion after independent Astra PASS on2026-09-30 20:05Z. Final report
`/private/tmp/workflow-04.7-schema-expansion-review-final.md`; initial five actual
schema findings `/private/tmp/workflow-04.7-schema-review.md`, clarified expansion
round `/private/tmp/workflow-04.7-schema-expansion-review.md`. **No dependent
correction code was implemented in this retiring worker.** Full04.7 remains
implementing/unverified; partial schema actual-code review requests changes.

New applied migration `migrations/20260930180000_workflow_action_evidence.sql` is
IMMUTABLE SHA256 `ad9be5ea22c694eaa91068446c1a24a309cfca33040668ef4da933e7db3e81ab`.
It creates five append-only fact tables (`workflow_action_evidence`, `_coverage`,
`_commands`, `_consumptions`, `_conflicts`), receipt `reconciliation_evidence_id`
source XOR/compositeFK, evidence/conflict owned revision triggers, SQL
`workflow_action_has_evidence_conflict`, `workflow_action_not_applied_available`
(returning optional evidenceUUID), replacement ordered `workflow_action_retry_safe`,
evidence/receipt/consumption guards, finality conflict/parking triggers, replacement
remote-entry guard, narrow `workflow_action_reconciliation_reopen_safe`, and additive
ordinary-or-reconciliation failed→pending guard. There are known five gaps requiring
new correction migration, as reviewed above; never modify180000. Migration application
alone is not behavioral verification. No backfill, queue/lease/attempt owner or bounds
were added; all earlier applied migrations retained.

Typed changes within original exact-provenance contract:
`application/workflow/actions/reconciliation/contracts.rs` ReconciliationEntry
`generation` is now existing WorkflowGeneration(UUID), notu64, coverage serializes
itsUUID and rejectsnil. Tests bind/alteractualUUID. `verification.rs` adds read-only
registration version/provider/company/operation getters plus matches_snapshot for
future adapter checks. No token constructor/caller-controlledverifier was exposed.
Formatting applied after tests. No persistence adapter/dispatch/completion integration
exists yet. No typed applied attribution was added yet (next worker implements the
accepted refinement before adapter use).

Next worker:
1. Add reviewed private AppliedEvidenceRequest attribution to attestation/token, exact
snapshot binding, getters and SQL columns/entryFK/reason discriminator in a **new**
follow-up migration. Correct receipt conflict distinction and completed-execution
parking; add fullscoped command/evidence equality, currentxid8 BEFOREidentity owner +
deferred same-new-entry/currenttx guard, firststateORwaitingreason transition witness
and guarded deferred reopen branch. Preserve existing ordinaryretry branch.
2. Implement narrow cohesive `action_reconciliation` persistenceadapter using the
existing authorize_company/company-before-run/association locks, parked/terminal
frozenintent restore independent of dispatch-only action_authority::load_on. Use
separate SQL command-actor resource-authority hook (no I/O approval required for past
truth) and installed registration metadata. Authorized two-transactionsnapshot/replay,
DBclock/stale exactcoverage/expectedrevision/operation/marker checks, evidence+receipt+
conflict+optional existingjobreopen+actoraudit, final generatedrevision commandlast.
3. Wire marker_on/reserve_entry_on/verify_entry_on/enter_remote to sharedproof and
atomic consumption/currentfence; permit exact supported branch without reinterpretation
of originalmarker providerproof. Add conflict gates at receiptreturns/heartbeat/shared
completion retirement before output/edge. Sole attempt owner remains unchanged.
4. Implement real authoritative providerledger + all original accepted meaningful
matrix, including discriminatory fivefinding cases, then every remaining required
stock2MiB affected/static/freshschema/SQLx/graft gate and fresh independent fullcode/
criteria/integration PASS. Do not treat71old/typedtests as reconciliation DB coverage.
No04.8 work; root/user scope ends after04.7complete.

Actual verification this partial worker (sequential operations):
- required explicit retained BOTHURLs `cargo sqlx migrate run`:PASS43.592ms,
  `/private/tmp/workflow-04.7-evidence-migrate.log`; migration history success and all
  five tables independently inspected with psql. No freshschema/SQLxprepare yet.
- `env SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_action_reconciliation`:
  PASS9/0failed/0ignored0.04s, compile49.06s,
  `/private/tmp/workflow-04.7-schema-typed-tests.log`.
- `env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_action`:
  PASS71/0failed/0ignored21.58s (62existingaction+9typed),
  `/private/tmp/workflow-04.7-schema-action-regression.log`.
- `cargo fmt --all`, thencheckPASS and `git diff --check`PASS,
  `/private/tmp/workflow-04.7-schema-fmt.log`,
  `/private/tmp/workflow-04.7-schema-final-fmt.log`,
  `/private/tmp/workflow-04.7-schema-final-whitespace.log`.
  Initialfmtcheck failedonlyformatting (`schema-fmt-r1.log`), corrected.
- No fullphase04/fullsuite/allaffected/staticClippy/offlinealltarget/freshschema/
  SQLx/graftcomplete claim. Existing nine-testpartial evidence is preserved separately.

Worker `/root/reconciliation_persistence`, Sol6.1/high, verified Codex UUID
`01a0f3d9-b041-7d00-89c5-aaab802bd6ef`, depth0 startup20402/258400=7.90%
@19:45:34.717Z; schema106832=41.34%@19:56:35.996Z;109490=42.37%@19:58:59.635Z;
116293=45.01%@20:01:35.160Z;122262=47.32%@20:04:39.449Z.
Own fresh Astra/medium child `/root/reconciliation_persistence/review_evidence_schema`,
UUID`01a0f3e4-a406-7090-bb83-3928cfcac09b`: actualreview63448/25840024.55%
@19:59:42.643Z; affectedfinalPASS71905=27.83%@20:03:48.400Z;
parentconfirmed73057=28.27%@20:04:10.349Z. Sourcesruntimeusage/window.
Reviewer quiescent/retired; successor must create its own freshreviewer.
PGcleanfaststopped with preserveddata, `/private/tmp/workflow-04.7-schema-pg-stop.log`.
Exactdata `/private/tmp/workflow-admission-pg-e3aa`, log
`/private/tmp/workflow-admission-postgres.log`; BOTHURLs remain
`postgres://mac03@127.0.0.1:55439/workflow_admission`. NoactiveCargo/PG/SQLx/tool
session attransfer. RootownsPROGRESS/RESUME/acceptance. Workerretiresbefore50%.
Graft estimated worker savings~138904tokens (reviewer separate).

## Additive corrections and snapshot milestone — 2026-09-30 20:36Z

Full04.7 remains **implementing/unverified**; no partial milestone replaces its
original acceptance matrix. No04.8 work began. Root owns PROGRESS/RESUME/acceptance.

Applied immutable follow-up migration:
`migrations/20260930181000_workflow_action_evidence_binding.sql` SHA256
`504921262147ab7be0661d875e35ceed97d16431a35219dad5f5a693502fdcb4`.
180000 retained SHA256 `ad9be5ea22c694eaa91068446c1a24a309cfca33040668ef4da933e7db3e81ab`.
The additive correction enforces full command/evidence operation equality both ways;
initial OLD state/reason witnesses owned by DB triggers/current xid8; current-xid8
entry/consumption identity plus deferred equality (historical entries remain NULL);
applied request attribution distinguishing covered-old-request finality breach,
legitimate later request, and unattributed_applied; completed execution conflicts
cannot park a committed successor. Evidence without a recovered receipt still needs
explicit adapter conflict recording; the receipt conflict trigger alone does not do it.

Application `verification.rs` now has `AppliedEvidenceRequest::{RemoteEntry,
MarkerReservation,Unattributed}` in trusted Applied attestations and a private field
with read-only getter in VerifiedEvidence. Issue rejects entries outside the exact
snapshot and marker attribution when entries exist. No token constructor/Deserialize,
caller-provided verdict/source, or public mutation was added.

Partial persistence `action_reconciliation/mod.rs` exports
`PostgresActionReconciliation` and required `SqlReconciliationResources` host hook.
Its inherent association/snapshot methods use bounded transactions, company/principal
before run/actual association/execution/job/resource locks, restored frozen bundle/
resources/subject/remote marker even parked/terminal, current command-actor access
before command replay, durable revision refusal/exact replay/namespace conflict, DB
clock, full sorted entry attempt/generation/worker provenance, and129-read entry and
sibling overflow detection. **No ActionReconciliation trait implementation or settle
exists yet.** No success default or stub settlement was introduced. Dispatch/proof
consumption/receipt-return/completion integration remains unchanged and OPEN.

Actual-code bounded independent reviews (not whole04.7 acceptance):
- `/private/tmp/workflow-04.7-additive-review.md`: bounded static PASS all five
  correction areas/private attribution. Applied-without-receipt conflict obligation
  expressly retained. Before applying181000; no migration edits afterward.
- `/private/tmp/workflow-04.7-snapshot-review.md`: R1 found compile imports/WorkerId
  errors and revocation test used changed/conflicting rather than exact command.
  Grouped corrections applied. `/private/tmp/workflow-04.7-snapshot-review-final.md`:
  bounded static PASS, then PASS attribution fixture correction; gates still OPEN.

Changed/new tests: `action_evidence_binding_tests.rs` (first OLD terminal/human reason
witness, direct caller witness rejection, spoofed entry xid8 rejection) and
`action_reconciliation_snapshot_tests.rs` (parked/terminal exact provenance, durable
revision refusal/exact replay/key conflict and exact-replay resource revocation).
Typed attribution test covers covered/foreign entry, marker with entries, unattributed,
and zero-entry marker. These do **not** replace required isolated full reopen/retrofit/
scope/finality/completion negatives or real authoritative fixture-provider matrix.

Sequential verification with complete logs:
- BOTH retainedURLs `cargo sqlx migrate run`: PASS34.16ms,
  `/private/tmp/workflow-04.7-binding-migrate.log`.
- Initial typed build failed three test field/Box API mistakes; corrected before
  adapter compilation, `/private/tmp/workflow-04.7-binding-typed.log`.
- BOTH retainedURLs + `SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_action_evidence`:
 2PASS/1FAIL wrong expected human-wait literal; corrected,
 `/private/tmp/workflow-04.7-binding-db.log`.
- Initial stock action adapter build failed private re-export/type imports; grouped
  corrections, `/private/tmp/workflow-04.7-binding-action-tests.log`.
- BOTH retainedURLs + `SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_action`:
  executed77, **76PASS/1FAIL**, including all five new DB tests PASS. New typed test
  indexed an initially empty fixture; corrected with a full entry and recomputed
  coverage. `/private/tmp/workflow-04.7-binding-action-tests-r2.log`.
- `env SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_action_reconciliation`:
 10typedPASS/2DBFAIL because filter now includes snapshotDB tests and URLs were
 omitted; DB-required fixture failed explicitly, did not skip.
 `/private/tmp/workflow-04.7-binding-typed-r3.log`.
- Corrected BOTH retainedURLs + `SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_action_reconciliation`:
 **PASS12/0failed/0ignored3.79s** (10typed+2snapshotDB),
 `/private/tmp/workflow-04.7-binding-reconciliation-r4.log`.
- Final `cargo fmt --all -- --check`, `git diff --check`, `graft build`: PASS,
 `/private/tmp/workflow-04.7-binding-final-fmt.log`, `binding-final-whitespace.log`,
 `binding-final-graft.log` (same /private/tmp/workflow-04.7- prefix).
 No offlinealltargets/strictClippy/SQLxprepare+check/freshschema/allaffected/final
 fullaction/full04.7 matrix claim from this milestone. SQLxcache unchanged.

Next worker implements cohesive settlement using snapshot helpers (refactor locally as
needed) and installed registration metadata; repeats current actor/resource authority,
exact immutable operation/marker/ALL entries/revision/DBclock/token binding, records
stale/expired refusal, evidence/coverage/receipt/conflict and actor audit, optional
existing job/run reopen with final generated revision command-last. Ensure Applied
without output also writes required old-request/unattributed conflict and existing
receipt disagreement is immutable. Same-company command-key concurrency across runs
still requires meaningful test/handling, not merely a sequential replay. Then shared
proof selection/atomic new-entry consumption/enter and heartbeat gates, all successful
receipt-return gates, shared completion exact-fence conflict retirement, projection,
real authoritative DB provider ledger, original full matrix and gates in prior handoff.
All five full SQL counterexamples remain required, beyond the narrow guards tested here.

Worker Sol6.1/high UUID `01a0f3ef-5bec-7481-9568-40535e30c46b`: depth0 startup
20342/258400=7.87%@20:09:14Z;93439=36.16%@20:18:19Z;100595=38.93%@20:22:08Z;
108339=41.93%@20:25:46Z;112143=43.40%@20:28:12Z;119009=46.06%@20:31:28Z;
parent confirmed123275=47.71%@20:33:15Z. Runtime usage/window, estimated.
Own fresh Astra/medium reviewer `/root/reconciliation_adapter/review_reconciliation`,
UUID `01a0f3f6-a288-7db1-93ff-2a77f0e1c63e`: bounded additive25.99%, snapshotR1
38.13%, snapshotfinal41.34%, finalfixture43.38%; now quiescent/retired. Fresh successor
must create its own reviewer. No deeper/extraworkers. Preserve all earlier work.

Retained PG clean fast-stopped with preserved data/log; no active Cargo/SQLx/graft
operations at handoff. `/private/tmp/workflow-04.7-binding-pg-stop.log`.
Data `/private/tmp/workflow-admission-pg-e3aa`, log
`/private/tmp/workflow-admission-postgres.log`, BOTHURLs
`postgres://mac03@127.0.0.1:55439/workflow_admission`. Restart exact127.0.0.1/55439/
private tmp socket/max_connections200 per skill reference; never reset retaineddata or
edit applied migrations. Root/user scope still ends only after full04.7 root acceptance.

## Partial settlement/integration handoff — 2026-09-30 20:53Z

Full04.7 remains **implementing/unverified**. User explicitly waived only root60%
stop to finish04.7; worker/reviewer50% rotation remains enforced. No04.8 work.
Root owns PROGRESS/RESUME/acceptance. Preserve every earlier modified/untracked path.
Applied migrations through181000 were not changed or newly applied by this worker.

Current actual changes (all partial, not accepted as full04.7):
- `action_reconciliation/mod.rs` now implements application `ActionReconciliation`;
  prior inherent snapshot/association implementations moved directly into trait.
  `settlement.rs` repeats company/principal→run→actual association→execution/job→
  command-actor resource locks; replay after reauthorization; structural private-token
  registration/snapshot binding; exact operation/coverage/revision/DBclock stale and
  expiry refusals; evidence, coverage, receipt/conflict, optional existing-job reopen,
  actor audit and final generated revision command-last in one bounded transaction.
  Company FOR NO KEY UPDATE already serializes same-company command-key races across
  runs; no separate lock/queue/lease/attempt owner was added.
- `facts.rs` owns evidence/coverage inserts and applied recovered receipt, missing-result
  truth, existing receipt disagreement and old-request/unattributed conflicts when no
  new receipt can trigger detection. Same-coverage distinct final commands retain facts
  but only first grant_eligible fact can mint a consumable proof. Ordinary retry unchanged.
- `action_receipts.rs` marker selection uses shared available-final-proof alongside
  supported replay; reservation atomically inserts consumption with the new entry;
  exact entry provenance accepts immutable matched consumption without rewriting
  marker policy/provider proof. Late receipt storage remains independent of expired
  proof/current live-fence access. return_authorized now checks execution conflict.
- `action_dispatch.rs` gates local and reserve_remote successful receipt return on shared
  conflict; enter_remote checks conflict/applied truth/consumed-proof availability before
  real provider poll. owns_remote heartbeat uses exact live fence and shared conflict
  in a run-locked transaction. `action_uncertainty.rs` exposes shared conflicted_on.
- `completion.rs` checks shared conflict after completed replay/live fence but before
  activation/output/batch_commit; exact attempt retires with action.evidence_conflict
  and returns no committed result. Completed replay retains historical behavior.

Independent **bounded** actual review REQUEST CHANGES, not PASS:
`/private/tmp/workflow-04.7-settlement-review.md`. Sole grouped concrete P1:
`action_receipts.rs:213–219` ON CONFLICT DO NOTHING discards incoming actual receipt
when another receipt already owns invocation, so AFTER INSERT detector never sees
covered-old-entry finality breach (even same result) or differing actual/recovered
result. Under existing owning-run lock, durably record incoming actual entry/result
contradictions even when it cannot own receipt; preserve existing receipt and exact
provenance, deduplicate repeat delivery conflict, retain terminal truth. Test ordering:
old A after final proof then consumed B's successful receipt, same/different result;
old A after recovered receipt; before completion must retire exact fence; already
completed execution/successor unchanged. Any SQL correction must be additive.
No correction was made before rotation. Reviewer static observations on other new
seams do not replace original acceptance tests or full independent review.

Remaining work for fresh worker (original full matrix remains authoritative):
1. Correct above P1 and review other new settlement paths while implementing meaningful
   real trusted DB provider/evidence ledger. Ensure final-not-applied after prior Applied
   truth receives immutable disagreement audit/outcome as required; neither Applied nor
   Unknown can downgrade authority. Reconciliation projection is still OPEN (existing
   effect-state view has not been extended for evidence/consumption/conflict facts).
2. Full application/service and settlement DB tests, not new sequential mock assertions:
   authentic remote effects/lost result/recovered result/no result/invalid schema; trusted
   final absence and quiescence (marker reservation + ALL exact prior entries), Unknown
   on pending/no current effect; DB restart/duplicate command/replay/current revocation,
   verification-to-commit revision/entry/resource races, expiry and cancellation/deadline,
   exact company/association/subject/marker/digest/unsupported source/input bounds.
   Competing same/different commands and cross-run same-company keys, competing claims,
   one-use consumption/crash acknowledgement/expiry-before-enter, multiple unsafe and
   receipt/supported/proof siblings, Applied veto of supported replay, 128/129 detection,
   late old/new/unattributed effect conflict orderings, exact-fence completion race,
   terminal/human/child waits/attempt/activation/deadline/root budget and atomic rollback.
3. Original five SQL counterexamples require full isolated direct-SQL negatives beyond
   existing narrow witness/xid8 tests: operation scope both directions, genuine initial
   terminal/human wait reopen, historical consumption retrofit/current spoofed xid8,
   old/new/unattributed attribution, already completed successor preservation. Include
   expected refusal trigger independent of other guards and valid positive counterpart.
4. Required stock2MiB affected admission/control/recovery/receipt+allaction coverage,
   locked offline alltargets, strict Clippy, fmt/whitespace, fresh migrations/schema,
   SQLxprepare+check, graft, and fresh independent FULLactual-code/originalcriteria/
   combinedintegration review PASS. Reuse unaffected prior evidence only while its
   code/service/fixture assumptions hold; no combinedphase04/fullsuite claim unless run.

Exact sequential check evidence this worker:
- `env SQLX_OFFLINE=true cargo check --locked --offline --lib`: initial compile failed
  tuple Waiting/RunHead reason usage (corrected), second failed missing ActionScope
  import (corrected); **R3 PASS43.85s**, no warnings,
  `/private/tmp/workflow-04.7-settlement-check-r3.log`. Earlier failure logs
  `settlement-first-check.log`, `settlement-check-r2.log` share prefix/directory.
- `env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_action`:
  **PASS77/0failed/0ignored36.34s**, compile1m37s,
  `/private/tmp/workflow-04.7-settlement-action-regression.log`.
  These are existing tests (including prior typed/snapshot guards); no new settlement
  DB test/provider matrix exists in this milestone.
- `cargo fmt --all`, then `cargo fmt --all -- --check`: PASS,
  `/private/tmp/workflow-04.7-settlement-fmt.log`; `git diff --check`: PASS,
  `/private/tmp/workflow-04.7-settlement-whitespace.log`.
- `graft build`: completed at handoff, log
  `/private/tmp/workflow-04.7-settlement-graft.log`. No SQLx/static/freshschema/fullreview
  completion claimed. SQLx cache unchanged. Prior immutable migration hashes preserved.

Worker Sol6.1/high UUID `01a0f40a-a3be-7880-b2ac-3a5cae45c3ee`: depth0 startup
20316/258400=7.86%@20:39:00Z;108592=42.02%@20:45:39Z;
117211=45.36%@20:48:26Z;119746=46.34%@20:49:39Z;
122529=47.42%@20:51:38Z. Sources runtime usage/window, estimates.
Own fresh Astra/medium reviewer `/root/reconciliation_settlement/review_settlement`,
UUID `01a0f414-39ce-7d83-b038-21aa98f38a56`: startup26960=10.43%, final78991=
30.57%@20:51:44Z; reviewer quiescent/retired. Successor creates own reviewer.

Retained PG clean fast-stopped with data/log preserved,
`/private/tmp/workflow-04.7-settlement-pg-stop.log`; data
`/private/tmp/workflow-admission-pg-e3aa`, log `/private/tmp/workflow-admission-postgres.log`,
BOTH URLs `postgres://mac03@127.0.0.1:55439/workflow_admission`. Restart exact
127.0.0.1/55439/private tmp/max_connections200 per skill reference. No retained reset,
new migration application, staging, commit, deployment or bound increase. All check
sessions and reviewer complete at handoff. Scope ends only after full04.7 acceptance.

Retirement final sample126002/258400=48.76%@20:54:00.829Z; parent later sample
may be slightly newer. Reviewer parent-confirmed80302=31.08%@20:51:52Z, quiescent.
Final brief whitespace PASS `/private/tmp/workflow-04.7-settlement-final-whitespace.log`.

## 20:59Z affected correction design — incoming actual receipt audit and projection

Sole P1 correction retains first authoritative receipt and adds an immutable bounded
`workflow_action_actual_receipt_observations` audit table, not a second receipt owner.
Every successful actual entry result is recorded under the existing owning-run lock,
with full composite scope/entry provenance and generated canonical JSONB SHA-256
result digest; UNIQUE(company,entry,result_digest) deduplicates identical delivery.
An AFTER INSERT detector checks all covering FinalNotApplied facts even when the
canonical receipt already exists; it also detects a different result against the
canonical receipt, including actual-versus-actual without an evidence source. Conflict
facts gain a scoped observation FK and allow absent evidence only for an actual
observation disagreement; existing evidence/entry links stay intact. Conflicts retain
incoming result through the observation link, permanently veto automatic advancement,
and never rewrite first receipt or completed successor history. Existing canonical
receipt trigger handles only recovered-evidence finality; actual observation detector
handles actual finality once. No migration through181000 is edited.

Effect-state view preserves existing columns then adds `evidence_conflict` boolean.
Precedence: receipt=>committed, Applied truth without receipt=>applied_without_result,
conflict=>needs_reconciliation, currently available unconsumed final proof=>not_applied,
otherwise old uncertainty/consumed-proof=>needs_reconciliation, marker=>possible_dispatch,
no marker=>prepared. SQL shared available-proof predicate owns final availability.
Tests discriminate old A after consumed B receipt with same/different result, after
recovered receipt, repeated incoming delivery dedup, conflict before live completion
retirement, terminal/completed history preservation, actual-vs-actual disagreement,
and projection across available/consumed/applied/receipt/conflict states. Full original
04.7 matrix and all previously required gates remain pending and unchanged.

Focused independent affected-design PASS (no material contract deviation):
`/private/tmp/workflow-04.7-actual-observation-design-review.md`. Refinements:
canonical receipt AFTER INSERT actual-source branch also inserts/deduplicates matching
observation, preserving direct-SQL finality guarantees. Recovered-source branch retains
attribution and checks differing prior observations. A deferred observation guard
requires the fully scoped canonical receipt at commit, so observation is audit only.
Conflict evidence_id nullable only with a fully scoped actual observation anchor.
Actual result disagreement uses explicit reason actual_receipt_disagreement.
Reviewer UUID01a0f41d-5cb4-7d72-9d73-1ddb88a09e20 final29.47%, quiescent.

## 21:17Z focused implementer handoff — actual observation P1/projection

Full04.7 remains UNVERIFIED. No04.8 work. Preserve all prior modified/untracked work.
This milestone implements the accepted incoming-actual audit correction and projection:
- IMMUTABLE applied182000 SHA256
  `cc5d628e70c4a39e7492e3dcd2b09070ac5b127b60204fac2e32e8d61e045ca1`;
  immutable183000 SHA256
  `0185d80a94a5eda497b9e7004d1fa323dd56631f4e1468d9299f1b5c448b2cbf`.
  Initial182000 migration failed/rolled back because convert_to is stable; unapplied
  corrected expression uses immutable jsonb_send/SHA256.182000R2 and183000 appliedPASS.
  All migrations through183000 now immutable; never edit/backfill retained data.
- action_receipts::insert_receipt_on records bounded immutable actual observation
  before first-receipt-wins INSERT. Direct SQL canonical actual INSERT creates/dedups
  observation too. Observations retain all differing incoming results with exact entry
  composite provenance; deferred guard requires existing fully scoped canonical receipt.
  Covered-old actual truth always records finality conflict, even same result after new
  receipt; differing actual/recovered or actual/actual results record disagreement.
  Actual canonical insertion also compares prior same-TX observations; conflict evidence
  nullable ONLY for scoped actual_receipt_disagreement. Existing receipt never changes.
- Effect projection now includes committed/applied_without_result/not_applied/unknown
  or consumed-proof needs_reconciliation, with separate execution evidence_conflict.
  Shared available-proof predicate remains final-grant owner.
- New action_reconciliation_receipt_tests.rs: real application reconciliation service
  uses LifecycleAuthorizer/current DB resource policy and approved DB ledger verifier.
  Ledger represents exact durable final/effect observations with DB observed_at. This
  is NOT yet the full real RemoteAction/provider/effect/quiescence fixture; full matrix
  remains required. Snapshot Resources/command visibility widened only pub(super) for
  reuse; dispatch_tests adds new sibling test module.

Focused own Astra design PASS
`/private/tmp/workflow-04.7-actual-observation-design-review.md`.
Actual review R1 REQUEST CHANGES (prior observationX followed by actualcanonicalY
missed conflict, nullableevidence accepted wrong reasons); grouped additive183000
fix actual static PASS `/private/tmp/workflow-04.7-actual-observation-code-review-r2.md`.
This scoped static review is NOT fulloriginal04.7 acceptance. Final test-only DB clock
annotation and duplicate wording change occurred after it; no production code changed.

Exact checks (sequential, retained URL both env vars per earlier reference):
- `cargo sqlx migrate run`182000 initialFAIL rolledback,
  `/private/tmp/workflow-04.7-observation-migrate.log`;182000R2 PASS
  `observation-migrate-r2.log`;183000PASS `observation-migrate-r3.log`.
- `env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib action_reconciliation_receipt_tests`:
  **R5 compilePASS45.56s;2PASS/1FAIL/0ignored2.35s**,
  `/private/tmp/workflow-04.7-observation-tests-r5.log`.
  DirectSQL observation-only rollback, observationXbeforeactualcanonicalY conflict,
  exactnullableanchor constraint and append-only guards PASS. Oldactualafterrecovered
  firstreceipt preservation PASS. OldA after finalproof/consumedB successfulreceipt
  same/different outputs, projection available/consumed/committed+conflict, competing
  nextclaims, duplicateauditdedup and precompletion exactfence conflict retirement
  passed before the completed-before-late variant. Completed execution full-row
  historical equality also PASS, then line299 erroneously(?) expects complete_io with
  DIFFERENT invalid output (`"historical replay"`) to return Some; actualNone. Next
  worker must inspect existing completion replay contract, use same accepted stored
  output if appropriate, rerun all variants (second completed/differing not yet reached).
  No passing fullold/new test claim. Earlier R1/R2 failed test privateimport/Clone
  assumptions; R3 onePASS/oneFAIL liveobserved-clock issue below; R4 failed test-only
  SQLclock type inference, all same directory `observation-tests[-rN].log`.
- `cargo fmt --all`, `cargo fmt --all -- --check`: PASS
  `/private/tmp/workflow-04.7-observation-fmt.log`; `git diff --check` PASS
  `observation-whitespace.log`; `graft build` PASS `observation-graft.log`.
  No new allaction/affectedregression/static/SQLx/freshschema/fullreview claim.

### Newly exposed required correction — live observed DB timestamp

service.rs anchors Instant AFTER snapshot returns, but computes verification timestamp
as snapshot.database_now+elapsed. This excludes snapshot transaction/return latency:
valid trusted verifier observation using actual DB current time can exceed estimated
verified_at and VerifiedEvidence::issue rejects BadRequest. R3 final-absence test
exposed it; using durable ledger fact observed_at (recorded at final/effect insertion,
legitimately before snapshot) exercises a valid different case, not a solution.
Correct actual service contract and add liveobserved-at regression plus future-time/
expiry negatives before fullacceptance. Candidate within accepted authority boundary:
use max(trusted attestation.observed_at, estimated verification floor), retaining
snapshot+5s issue bounds and independent settlement DB future-time rejection; own
Astra must assess security/expiry. No production timestamp correction yet implemented.
Also still correct/verify FinalNotApplied after Applied-without-result disagreement
facts (truth_on currently checks only existing receipt before final-disposition conflict).

Remaining fullmatrix/provider/gates are ALL requirements listed in previous20:53Z
handoff and original accepted sections above: no scope weakening. Next worker first
fixes current test expectation/reruns, then liveclock + realprovider/effectledger and
FULLdiscriminatory command/concurrency/authority/coverage/expiry/consumption/sibling/
bounds/budget/childwait/terminal/lateattribution/SQLFK/witness/xid/restart/rollback matrix.
All5oldschemafindings require isolated negatives/positive counterparts;3oldbindingtests
and these3new tests are insufficient. Finish all required stock2MiB affectedregressions,
lockedofflinealltargets, strictClippy, fmtwhitespace, freshschema, SQLxprepare+check,
graft, and ownfresh FULLactualcode/originalcriteria/integration review/rootacceptance.

Worker Sol6.1/high UUID01a0f41a-90a6-7930-8dd3-5904db615e7e depth0 samples:
20350=7.88%@20:56:25Z;84536=32.72%@20:59:41Z;98162=37.99%@21:05:03Z;
104276=40.35%@21:07:50Z;110167=42.63%@21:09:27Z;114266=44.22%@21:11:30Z;
119713=46.33%@21:14:10Z;123504=47.80%@21:15:54Z (258400capacity).
Own Astra/medium reviewer /root/reconciliation_acceptance/review_receipt_correction,
UUID01a0f41d-5cb4-7d72-9d73-1ddb88a09e20 parentfresh105017=40.64%@21:15:41Z;
quiescent, retire with this worker; successor creates own fresh reviewer.
Retained PG clean fast-stopped `observation-pg-stop.log`, original data/log/URL identity
unchanged per earlier handoff. No activechecks/approvals, staging/commit/deploy/reset/
boundsraises. Root owns PROGRESS/RESUME (not edited by this worker).
Final retirement depth0 sample126464/258400=48.94%@21:18:31.332Z;
usage/window runtime sources, estimate, below enforced50%; no further substantive work.

## 21:32Z USER STOP — focused matrix worker handoff

User explicitly stopped execution before FULL04.7 completion. **FULL04.7 remains
implementing/unverified, not accepted. No04.8 work.** No further implementation,
new tests or new checks after stop; parent owns PROGRESS/RESUME. Preserve all prior
modified/untracked files and retained data. No task-created freshschema DB this worker.

Actual files changed in this worker:
- `application/workflow/actions/reconciliation/service.rs`: verified_at now
  max(snapshot.database_now + monotonic elapsed, trusted attestation.observed_at).
  Existing issue/structural5s bounds and independent settlement actualDB future/expiry
  rejection remain. This removes omission of snapshot transaction/return latency.
- `adapters/persistence/workflow/action_reconciliation/facts.rs`: FinalNotApplied
  inserts scoped evidence_disagreement for every prior Applied fact before continuation,
  even absent usable receipt; retained positive truth cannot downgrade into retry.
- `action_reconciliation_receipt_tests.rs`: completed historical replay fixture now
  uses exact previously committed output per completion::replay equality. Later edits
  change LedgerVerifier to live DB observed_at and provider-operation lock; ledger()
  creates operation/effect tables, verifier supports zero-entry marker barrier; nested
  provider_tests module added. These later fixture edits were NOT rerun successfully.
- New `action_reconciliation_provider_tests.rs` (443-ish lines, formatted draft):
  LedgerProvider implements actual RemoteAction with durable request ledger and effect
  row; loss after applied effect leaves real positive truth, Pending leaves a request
  able to apply after local timeout. delayed_apply and barrier serialize on provider
  operation row; closed covered requests cannot apply, future entries remain possible.
  Four authored tests cover lostApplied/recovered receipt/restart zeroI/O; activeUnknown
  until effect or barrier/retry; missing/schema-invalid/oversize recovered output plus
  priorApplied vs FinalNotApplied conflict; liveDB observed_at and future-time settlement
  rejection with zero-entry marker. **Draft compileFAIL, no runtime claims.** ForcedVerifier
  is explicitly trusted fixture composition for contradictory/future attestations;
  caller command carries no verdict/authority. Provider fixture has not had actual-code
  review. More complete FULLoriginalmatrix remains required.

Scoped independent correction static PASS, not fullmatrix/provider acceptance:
`/private/tmp/workflow-04.7-matrix-correction-review.md`. Own fresh Astra reviewed
actual timestamp authority/expiry/settlement, priorApplied conflict scope/order/shared
vetoes and exact historical replay contract, no blocking findings. This review predates
new provider fixture and liveDB verifier changes. No material contract deviation claimed.

Exact sequential commands/evidence (both URLs retained as below):
- `env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib action_reconciliation_receipt_tests`:
  **3PASS/0FAIL/0ignored2.93s**, `/private/tmp/workflow-04.7-matrix-receipt-r1.log`.
  This PASS precedes subsequent liveDB/provider-operation fixture edits; rerun required.
- `cargo fmt --all` ran after fixture creation and after grouped compile corrections;
  no final fmtcheck/whitespace/static/SQLx/freshschema/graft/fullaction gate this worker.
- Same exact env prefix, `cargo test --locked --offline --lib action_reconciliation_provider_tests`:
  R1 compileFAIL3errors (2nonexistent action_retry_safe method calls, nonClone
  VerifiedDisposition), `/private/tmp/workflow-04.7-matrix-provider-r1.log`.
  R2 compileFAIL2errors (both nonexistent method calls remain),
  `/private/tmp/workflow-04.7-matrix-provider-r2.log`.
  ForcedVerifier now uses final_not_applied bool, removing nonClone issue. SQL-backed
  retry_safe helper exists at new file bottom but automatic textual substitution missed
  rustfmt's inline `!f.persistence()` calls at lines287/311 (method lines288/312).
  **Next local correction: replace those two method expressions with
  `!retry_safe(&f, &request).await`, then rerun provider+receipt tests.** No correction
  after explicit stop. First runtime run may expose SQL/behavior fixture errors.

Remaining authoritative originalcriteria: finish provider validity/quiescence and real
I/O authority distinct from command actor; revocation/exact replay/currentoriginalactor,
unknownrestart, applied-withoutresult later valid receipt/supported-replay veto;
coverage/revision/DBclock/expiry races; oneuse lostagain/freshproof coversboth;
competingcommands+claimants/samekeyconflict; all sibling receipt/proof/unknown mixes,
128/129overflow; eligible waiting-only existingjob, budgets/attempts/child/deadline and
unchangedattempt/debit; attribution old/new/unattributed and lateorder terminalcancel;
rollback; isolated compositeFK/check/immutability/statewitnesslaundering/currentxidspoof/
historicalretrofit negatives and counterparts for all5oldschemafindings. Existing3binding
and3receipt tests and new4draft tests do not constitute FULLmatrix. Then required stock
2048KiB allaction+affectedadmission/control/recovery/receipt/budget/lease/completion/
pending/maintenance; fmt/whitespace/lockedofflinealltargets/strictClippy, freshschema
allmigrations/inspection, SQLxprepare+check, graft, independent FULLoriginalcriteria/
actual-code/integration review and rootacceptance. No fullphase04/library claim absent run.

No migration edits or applications this worker. Applied immutable hashes rechecked:
180000 `ad9be5ea22c694eaa91068446c1a24a309cfca33040668ef4da933e7db3e81ab`;
181000 `504921262147ab7be0661d875e35ceed97d16431a35219dad5f5a693502fdcb4`;
182000 `cc5d628e70c4a39e7492e3dcd2b09070ac5b127b60204fac2e32e8d61e045ca1`;
183000 `0185d80a94a5eda497b9e7004d1fa323dd56631f4e1468d9299f1b5c448b2cbf`.
All earlier applied migrations likewise immutable. No reset/backfill/stage/commit/deploy/
boundsraise. New resources are per-isolated fixture test databases; no new retainedDB.

Worker `/root/reconciliation_matrix` Sol6.1/high UUID
`01a0f431-73a1-7bf1-b6d8-f0676d18bb0b`: depth0 runtime input/window258400 samples
20460=7.92%@21:21:23Z;83879=32.46%@21:23:54Z;98402=38.08%@21:28:34Z;
101902=39.44%@21:32:22.585Z (fresh stop sample). Own reviewer
`/root/reconciliation_matrix/review_matrix`, Astra/medium UUID
`01a0f433-ad2f-7191-bb76-519623f31302`: reported review milestone77311=29.92%@
21:25:17Z; parent-helper final79612=30.81%@21:25:56Z, completed/quiescent confirmed
by collaboration list. Reviewer depth0 sources usage/window, estimates; no thresholdwaiver.
All Cargo sessions completed/drained; no active checks/approvals/review.

Retained PostgreSQL clean fast-stopped per userstop, preserved data
`/private/tmp/workflow-admission-pg-e3aa` and log
`/private/tmp/workflow-admission-postgres.log`; stop log
`/private/tmp/workflow-04.7-matrix-pg-stop.log` reports server stopped. Both URLs
`postgres://mac03@127.0.0.1:55439/workflow_admission`. Restart only exact reference
options host127.0.0.1 port55439 socket/private/tmp max_connections200. Resume only
when user authorizes; first repair draft compilecalls, then fullmatrix/gates/review.

## 21:50Z resumed provider checkpoint — bounded scope, not acceptance

User resumed04.7; no04.8. Sol direct worker repaired draft provider tests and mapped
all original criteria/accepted refinements to actual assertions or exact gaps in
[EVIDENCE-04.7-GAPS.md](EVIDENCE-04.7-GAPS.md). Root owns acceptance. Code edits only
`src/adapters/persistence/workflow/action_reconciliation_provider_tests.rs`:
two nonexistent action_retry_safe calls use existingSQL helper; provider reads actual
workflow_action_intents instead of nonexistent workflow_action_invocations; expected
lost-response assertions require Timeout instead of accepting arbitrary database
errors; unknown/unusableApplied outcomes match established UnknownRecorded and
AppliedRecorded{receipt:false}, with explicit failed-job/waiting-run/unsafe checks.
No production code/migration changes, schema bypasses, limits, backfill or commit.

Combined current receipt/provider filter **7tests:5PASS/2FAIL**, complete log
`/private/tmp/workflow-04.7-resume-provider-r6.log`. Three receipt-order tests plus
lostApplied/restartzeroI/O and liveDB/futuretimestamp/zeroentry pass. ActiveUnknown
and AppliedMissing/Invalid scenarios reach a second distinct evidence command and
fail on existing unique index `workflow_run_event_execution_kind` over
(company_id,run_id,execution_id,event_kind). Settlement always inserts event_kind
action_reconciled with execution_id; a second command cannot insert required actor
audit. This is a production schema contract defect, not fixture nondeterminism.
Affected edits paused for root/Astra additive design preserving old event uniqueness
and full immutable command/evidence/actor-audit linkage. Do not ignore conflict,
omit audit, change event names arbitrarily, or alter an applied migration.

Commands use exact prefix
`env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152`
then `cargo test --locked --offline --lib FILTER`. Logs/results:
- provider-r1 FILTERaction_reconciliation_provider_tests: compilePASS **0tests**, no
  runtime evidence (actual nested module name is provider_tests).
- provider-r2 FILTERprovider_tests:14tests including10unrelated,11PASS/3FAIL.
- provider-r3 exact FILTERaction_reconciliation_receipt_tests::provider_tests:
  diagnostic assertion compileFAIL (noDebug on observation), corrected locally.
- provider-r4 same exact filter:1PASS/3FAIL, exposes nonexistent table.
- provider-r5 same exact filter:2PASS/2FAIL, exposes wrong expected outcomes.
- provider-receipt-r1 FILTERaction_reconciliation_receipt_tests::workflow:
  **3PASS/0FAIL/0ignored2.67s**, current live-clock fixture.
- provider-r6 FILTERaction_reconciliation_receipt_tests: **5PASS/2FAIL/0ignored3.53s**,
  exposes repeated actor-audit uniqueness defect after fixture corrections.
All log names above have prefix `/private/tmp/workflow-04.7-resume-` and suffix.log.
`cargo fmt --all`, `cargo fmt --all -- --check`, `git diff --check` pass; logs
provider-fmt.log, provider-fmt-check.log, provider-whitespace.log. SQLxprepare started
sequentially after all tests because fixtureSQL changed, with both exactURLs,
SQLX_OFFLINE=false, RUST_MIN_STACK2097152, `cargo sqlx prepare -- --all-targets`;
complete log provider-sqlx-prepare.log. Final result recorded in frozen handoff.
No fullaction/affected/Clippy/freshschema/preparecheck/graftbuild/full04.7 review
gate claimed. No acceptance follows from this bounded subset.

Retained PG started/verified exactly: workflow_admission/mac03, data
`/private/tmp/workflow-admission-pg-e3aa`, host127.0.0.1 port55439 socket/private/tmp
max_connections200. Startup log provider-pg-start.log. PG stays running for next
explicit owner; never reinit/reset/drop. All initial untracked SHA hashes unchanged
except the provider test file, before this evidence append. Applied migrations
through183000 remain immutable. Frozen handoff is
`/private/tmp/workflow-04.7-resume-provider-frozen/` (full actual artifacts+manifest).
Worker Sol6.1/high UUID01a0f443-62d6-7eb0-ad8f-a03d47ac673b, latest checkpoint
123055/25840047.62%@21:49:56.914Z, usage/window estimate; final fresh sample in handoff.
