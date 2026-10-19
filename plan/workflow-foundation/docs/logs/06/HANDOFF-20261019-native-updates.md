# Phase06 handoff — user-requested stop, 2026-10-19

No backward compatibility: edit init migration and use fresh disposable test databases;
no backfills, legacy decoders or staged upgrade path.

## Authorization and workflow

User explicitly requested “write handoff and stop”. Stop now; do not autonomously resume.
On a subsequent resume, use implement-astra-reviewed-2.1. Root authors under explicit
no-developer-subagent authorization; independent reviewer /root/reviewer_06 is retained,
idle/completed after architecture06-R013, requested Astra/medium with no inherited history.
No source review of06B2b has occurred. No new author child needed. Current root runtime
model/effort metadata is not authoritative; do not invent it from historical labels.
Context occupancy/control is unavailable; durable records carry continuity.

Starting HEAD remains5b4989ae0038a82f431db60a338b7f59d9bcf25c. Preserve all tracked/untracked
WIP, including assets/app.css. Worktree inventory:06B2b-handoff-worktree.txt. No commits,
deployments, external messages or production database resets authorized or performed.

06B2a remains accepted. Accepted D4 revision2: decision-D4.md,06-R012. Root-pause addendum
revision3: decision-D4-root-pause-r3.md, SHA256
ab2be4fe23c66e31d53296a5d40629cb31a45b5516b3b80e208ac71f774fd3c7,06-R013 PASS, root accepted.
The immutable reviewed addendum still labels itself PROPOSED; TASK records acceptance.
06-F008 remains unused.06B2b is unfinished/unreviewed/unaccepted;06B3/06C and final full
phase verification remain outstanding. No operational native model handler is claimed.

## Completed during this continuation

Accepted root-pause scheduling now has behavioral proof. Pending I/O siblings advance
run_at by five seconds capped by run deadline without attempts/activation/retry debit.
Processing siblings retire one real failure attempt; pending polling cannot repeatedly
spend retries. Last retry terminalizes without an error successor. Refusal is permanent.
Approved never-admitted native requests still receive a fresh fence to repark exact saved
requests when usage itself exceeds the approved allowance. Competing deferred claims work
while an ancestor run lock is held. Deadline and denial retirement consume no fake attempt.

Pure preparation previously conflated deferred work and settled failure through Option.
Added local Pending<T> {Ready, Deferred, Settled}; propagated through pending_recovery,
lease_claim, waits and decisions. Pure batch now returns BatchDisposition::Deferred with
no immediate continuation. Worker polling already honors persisted run_at.

Recovery preserves hard budget/activation failure diagnostics while AwaitingAllowance;
permanent token refusal retains its exact reason. Hard recovery policy and unknown-effect
precedence remain intact. New hard-model-budget test proves terminal code/class while
optional token review is pending.

Added scheduling, rollback, SQL bypass and reviewer-revocation race tests:
- harness_token_scheduling_tests.rs
- harness_token_atomic_tests.rs
Extended harness_token_refusal_tests.rs for approved-request repark after usage itself
exceeds allowance. configured_source helper now supports actual I/O-to-pure fixture.

## Latest implementation: native checkpoint updates

Added application-owned WorkflowCheckpointUpdate and
WorkflowCheckpointStore::update_workflow_checkpoint. Commands: BindTools,
PrepareInvocation, InvocationResult, FinalOutput (boxed), InvalidOutput.
Implementation lives in checkpoint_updates.rs (child of checkpoints.rs). It uses existing
WriteBound, actor/local fence/checkpoint locks and RunCheckpoint::apply. Exact replay
returns AlreadyApplied; ambiguous acknowledgment returns unresolved and grants no next phase.
An active model slot blocks changed mutations; current generation must have execution-time
reservation. Results can retain received truth after sibling refusal. New preparation/final
progression checks the root allowance/refusal under the shared usage lock.

An approved settlement-stage decision can now be consumed by tool/final progression,
without another model reservation. Reservation-stage approval cannot be consumed this way.
CheckpointOwnership::Progression persists these updates. Init SQL token pointer guard
now distinguishes reservation vs settlement consumption; new
 guard_harness_workflow_progression
checks new invocation/final/catalogue progression, execution reservation and active-model
exclusion. SQL still allows received result truth without granting new effect/output.

New harness_checkpoint_update_tests.rs proves:
1. Competing final writes after settlement approval commit once/replay, clear pointer,
   retain one model-call debit and never require another call.
2. BindTools cannot consume a reservation-stage approval; checkpoint and accounting unchanged.
3. Active model blocks InvalidOutput. After saved tool turn and prepared first invocation,
   root rejection permits received result storage but blocks second invocation in Rust/SQL.

Latest source passed native harness40/0 at stock2MiB/four threads:
06B2b-checkpoint-updates-tests.log. These latest SQL/API changes have NOT yet had refreshed
all-target static/Clippy/SQLx checks. Prior scheduling gates do not cover this delta.

## Verification evidence

Scheduling token22/0 PASS:06B2b-token-scheduling-tests-8.log (one unused import removed
afterward; final native40 includes that cleanup). Earlier scheduling logs retain failures:
- initial sibling private helper compile issue
- temporary borrowed settled handle in join
- pure fixture terminal route spelled end instead of $end
- pure fixture used invalid empty context.load output, corrected to items/token_count
- Clippy Copy clone, corrected; failed log named scheduling-clippy-before-copy-fix.log
No acceptance criteria weakened.

Scheduling affected regressions113/0 PASS: batch15, poison12, activation budget9,
completion14, waits19, decision34, action uncertainty10. After hard-diagnostic fix,
budget-runtime9/0 and uncertainty10/0 passed again. Commands:
06B2b-token-scheduling-gates.sh and06B2b-token-scheduling-gates-resume.sh.
Offline locked all-target check54.19s; strict Clippy66s; fresh init migration;
SQLx prepare55.20s/check53.41s, no.sqlx diff. Task metadata DB
workflow06_token_scheduling_20261019_test removed; absence confirmed. Graft build passed
before native-update edits; graft automatically refreshed changed files during later queries.
Run graft build again after subsequent large edits.

All tests use CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true,
RUST_MIN_STACK=2097152, four libtest threads and fresh own_database fixtures.
TEST_DATABASE_URL=postgres://workflow0410@localhost:55450/workflow0410_test.
Retained PostgreSQL cluster/private/tmp/workflow0410-20261017, port55450,
role workflow0410, system ID7697669855432439122. Preserve all retained databases.
Postgres tools:/opt/homebrew/opt/postgresql@18/bin. No builds/tests remain running.

## Next implementation work and trace

1. Audit/update native checkpoint mutation coverage as needed (deferred final-write rollback,
   final refusal without pending tools, SQL parity). Refresh required SQLx/static gates with
   next cohesive source segment. Native model admission currently assumes driver reserves
   execution time first; audit whether that precondition also needs explicit enforcement in
   reserve_on/SQL before provider-entry permission. Do not silently weaken existing raw owner
   tests; trace their actual execution ledger first.
2. Trusted preparation from immutable bundle and saved inputs, current agent/company/grants,
   exact frozen model connection and concrete selected tool targets. Generic prepared Value
   currently supplies bounded immutable storage only, not trusted runtime preparation.
3. Refactor shared Rig Execution into D4 closed legacy/native bindings. Keep one durable
   drive loop; do not use fake legacy leases or a second model runtime. Build canonical
   request fingerprints, consume fresh non-clone admissions only, preserve saved request
   IDs across approval and never reuse unresolved IDs after ownership changes.
4. Bind native saved tools through ActionService, approval and sequential message effect waits;
   retain exact checkpoint/invocation/parked-attempt association. Delivery completion reopens
   the agent job instead of completing agent.run. Preserve receipts/unknown effects.
5. Finish whole06B2b, freeze artifact and request one consolidated independent source review
   from retained reviewer using original requirements/decisions and skill template, not this
   author summary. Then06B3/06C and final full repository gates.

Important preparation trace (not a finalized/new architecture decision or implemented code):
- PublishedBundle::resolve_agent already owns frozen defaults/profile/ceiling semantics.
  ResolvedAgent is Serialize-only deliberately constructed from validated publication; do
  not casually add Deserialize to make untrusted stored JSON a permission object.
- Company model connection identity is (company_id,provider), not a UUID. TASK around3585
  records this accepted implementation refinement. Resolve current secret for this exact
  identity; never reload an edited agent's provider/model/default selection.
- Existing agent_runner::resolve_agent_capabilities loads current defaults, so do not use it
  unchanged for frozen native selection. Its current production endpoint is provider-default
  (test-only scripted endpoint override); use ProviderRegistry scripted factories for tests.
- Native preparation must check current selected tool/skill grants and actual company agent
  ownership, and resolve concrete HTTP/MCP targets. http::registered_on already checks exact
  frozen operation snapshot/current enabled connection and rejects ambiguous unbound routing.
  MCP uses saved binding resource slots/current_on. Message builtins come from
  builtin_message_snapshot, with ActionService authorization of actual messaging arguments.
- Revocation races require current capability authority inside the atomic model admission
  boundary, not only an earlier preparation transaction. Preserve authority→local owner→
  checkpoint→root usage order; never lock an ancestor run.
- Existing shared Execution (rig/execution.rs) is still legacy-owned: scope RunExecution,
  CompiledRun and ToolBridge. response_preamble in rig/final_response.rs takes CompiledRun
  only to render a preamble; direct.rs and execution.rs are its callers. Native preamble
  must be frozen/deterministic, without the legacy dynamic clock/facts renderer.
- HttpActions/HttpRuntime already expose call_tool(fence,HttpToolCall,cancel); MCP/message
  have analogous methods. MessageActions currently collapses pending effect wait observation;
  D4 requires exact invocation wait continuation instead of ordinary step completion.

Update TASK/RESUME when resuming; don't re-run all unchanged prior tests after every small edit.
Full verification, source acceptance and operational native runtime are still outstanding.
