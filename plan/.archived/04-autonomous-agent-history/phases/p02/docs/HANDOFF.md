# AH-P02 continuation

## Scope and authority

Implement all seven phases of plan 04, in order, after plan 03. Plan 03 is fully
accepted and pushed to main as `4ab45555f8316ec7a8b1b64e8c05b77ff7d9a29d`.
User explicitly requests targeted tests only; no full runtime, stack, or lease
suite and no offer to run them. Status questions do not cancel this task.
No plan 04 commit, push, or deploy is inferred.

Root/A `/root` is user-confirmed Sol 6.1/xhigh. B is
`/root/channel_reply_review_recovery`, requested Astra/high with independent
original context; reuse this reviewer only. Actual runtime telemetry unavailable.
The original selected skill is preserved at
`/private/tmp/openbot-chs-p06-3f43t2k7/workflow-skill-implement-astra-reviewed-2-2`.
The primary skill deletions are external inputs; do not restore them.

Always set cwd to `/private/tmp/openbot-agent-history-h_82_5pw/worktree`, branch
`codex/autonomous-agent-history-h_82_5pw`. Starting commit, review base and HEAD
are `4ab45555`. Primary `/Users/mac02/dev/openbot` is only coordination authority.
Root owns TASK/HANDOFF/coordination; B owns REVIEW/notes/findings.

Authoritative coordination revision 5:
`/Users/mac02/dev/openbot/plan/04-autonomous-agent-history/COORDINATION.md`.
Local copy and immutable `logs/coordination-r5.md` match. Notices acknowledged:
AH-I001–I006/r1, AH-C001/r1, AH-C002/r4, AH-C003/r1. C002/r4 narrowly permits
the existing safe native control-retry owner to reopen the same failed exchange;
ordinary terminal rules do not change. C003 is architecture acceptance only.

## Accepted producer and architecture

P01 full code accepted after AH-P01-B1-R1 PASS, source
`e62ebc6c7dac3ec2e34ca3ddce727df87b3d70f780154eb5b581b710a394aa91`.
It owns 59 source paths; immutable tar/hash/scope and acceptance are in P01 logs.
165 targeted tests passed; no full-suite claim. Do not write P01 records here.

P02 cohesive architecture D1/r2 accepted after AH-P02-AG1-R2 PASS, decision SHA
`4070ba26945e3e675d84f103a9fe3d3b3cc77c6a90514afe5b5254e2bdef819b`.
Read the full decision and original 02 requirements. R1 F1 safe operator retry was
fixed in that decision. No P02 full-code review or acceptance yet. B is idle.

## Current implementation

Unreviewed P02 implementation now includes reset-only exchange schema, original
immutable execution input/digest, scoped source/provenance/checkpoint FKs, native
order allocation and replay, bounded values, indexes, deferred checkpoint/output
agreement, genuine failure/cancel/routed-error and safe control-reopen hooks,
restrictive source pins and retention eligibility skip.

AgentPreparation now takes explicit LeasePolicy, uses native WriteBound and
current global authority before local fences, and commits input/order before
credentials/checkpoint/provider entry. PreparedAgent v4 carries a typed exchange
reference outside business context. Preparation ambiguity resolves read-only.
New persistence agent_history preparation/storage/checkpoint modules integrate
queued-to-active linking, matching-checkpoint reactivation COMMIT, active provider
and execution floors, exact structured final JSON and monotonic stamp in the same
final checkpoint transaction. Model settlement also calls the same terminalizer:
commit_turn can mark exhausted repair InvalidOutput directly. Native model
fixtures now use the exact production spec/contract/prompt; their accepted
structured fixture output preserves original accounting assertions.

New tests cover capture before checkpoint, competing preparation/replay/stale and
changed inputs, native identity/order/source deletion guards, exact structured
JSON and accepted null, repairs and exhaustion, competing finalizers/lost-ack
replay, deferred commit failure rollback, level stamp and actual competing row
lock, cancellation before/after, workflow failure after acceptance, and native
safe control retry preserving checkpoint/input/order/counters and refusing the
old owner. Latest repeated-activation test uses a new genuine root admission in
the same private session; the language rejects graph cycles.

P03 turn serialization and P04 history selection remain later phases. Do not claim
them here. Failed reopened old positions must not displace newer active turns or
be lost behind a greatest-completed watermark. P06 must pin resumable failed
sources/preparations until existing immutable deadline and enforce hard bounds.

## Verification state and active operation

Fresh P02 migration pair passed with the new DDL. Initial capture test passed.
Checkpoint locked-offline --tests compile passed. Completion05: 8 passed/1 failed.
History06: 42 passed/1 failed. Those failures were the scripted exhaustion fixture
providing only two responses; MAX_REPAIR_CALLS=2 requires initial+two repairs.
History08: 46 passed/1 failed; exhaustion, null, levels and lifecycle passed, with
only an invalid cyclic fixture failing publication. That fixture was replaced
with the actual supported repeated-admission path. Retain all failed receipts.
Earlier compile failures were corrected fixture API imports/field/ownership errors.

Current operation `history-and-affected-list-09` is compiling the latest source,
exec session 41801. Read its receipt/process before starting another Cargo or DB
operation. Fmt style05 passed. Latest external audit style05 checked all 36 paths
and found zero mismatches. No code edits after this build began; record writes only.

After list success, run the same exact selected runtime filters (list receipt)
through the loopback isolation guard; require nonzero selection and zero ignored.
Fix affected failures without weakening assertions. Then actual SQLx prepare/check
all-targets, locked offline all-targets, Clippy -D warnings, fmt/diff/Graft and final
external hash audit. No full suite. One whole P02 B code review after checks;
freeze source, provide exact snapshot/producer archive/delta/coordination/checks.
Fix all findings together, one recheck, root exact acceptance and coordination
publication before implementing P03. All P02–P07 remain required.

## Resource and artifact ownership

Root-owned PostgreSQL 18.6 PID 85060, loopback 65037, data under
`/private/tmp/openbot-chs-xb34mvy9/pgdata`. Keep it alive while task is active.
Dedicated pair `openbot_ah_p02` / `openbot_ah_p02_test`; never reset other pairs.
Explicit URLs and stock RUST_MIN_STACK=2097152 in logs/run-check.py. Warm target:
`/private/tmp/openbot-chs-p06-3f43t2k7/worktree/target`; serialize Cargo and DB
lifecycle. OwnDatabase for every unscoped/sweeping test; fixtures here already own
isolated databases. Do not reset while any test or fixture is alive.

36 external source/config/skill inputs and accepted P01 producer hashes are in
logs/setup.json. Preserve them byte-for-byte and never claim/stage as ours.
Snapshot exact nine exclusions remain unchanged across history: coordination,
p01 through p07 docs, and integration/final/docs. Exclude no source. Use immutable
skill snapshot.sh with logs/tools GNU realpath adapter via PATH. Read current
coordination before review/acceptance. Source is currently unfrozen.


## Superseding current state — code review freeze

All final gates completed successfully; session64245 and prior39651 are closed.
Runtime10 PASS227/0ignored, latest history11 PASS49/0ignored, unique union248 with
19 unaffected retention passes retained from09. SQLxprepare/check, offlinealltargets,
Clippy -Dwarnings, fmt/diff/Graft and fresh schema pair PASS. No check active.
External36 preserved; coordination authority r5 matches immutable pin SHA9db6424e.
Whole-phase input logs/AH-P02-B1-R1-input.json identifies source
0c6df19310118c599e38cebf51b47cd4cfcb4e3a37b65c1be901da2a14308a8c,
owned79/phase-delta32, tarSHA77a024f060139459a07e59087eba539bb96c773dee76031c90e12add776c167b.
Source is now FROZEN for B whole-code review. Do not mutate any implementation,
SQLx metadata, fixtures, dependencies or source while B is reviewing. Root only
TASK/HANDOFF/coordination records; B only its own review records/logs. After B
returns read all notes/findings, fix all actionable items together then one recheck;
verify exact source/coordination/evidence before root acceptance and publish the
accepted producer notice before P03. P02 code acceptance is still pending.

Independent B /root/channel_reply_review_recovery is now RUNNING AH-P02-B1-R1 code review against the frozen input; followup assignment sent. Do not resend or edit source while it runs.

Root reverified frozen snapshot after assignment: same0c6df193, code unchanged. Graft savings in this continuation approximately630k reported tokens; prior history continuations conservative500k+, no dollar metadata. Original03 turn-order/recovery and07 criteria read for readonly orientation; no phase03 design or code started before02acceptance.


### B review in progress: two candidate gaps, source still frozen

B has not returned a final review. It is independently reviewing AH-P02-B1-R1.
B raised two concrete paths and root confirmed implementation facts:
1. Native DELETE protects queued/active only. No additional reverse checkpoint FK
   or native guard protects a failed-but-safe-retry exchange itself before deadline.
   Current source cleanup skips retained exchanges, and no production history
   pruning writer exists yet; deleting the failed exchange would remove its pin.
2. open_workflow_checkpoint Ambiguous resolver proves only an existing matching
   checkpoint. It does not prove that a matching reopen committed Queued→Active.
   The downstream Active floor prevents fresh provider entry, but its AlreadyApplied
   outcome can still lack reactivation proof. Existing lost-ack-named coverage is
   duplicate durable replay, not injected commit/transport ambiguity.

Root confirmed these facts to B and will wait for the COMPLETE aggregate review,
then fix all actionable findings together and request one recheck. Do not edit
source or start Cargo/DB checks before B finishes. No new architecture decision
or phase03 work has started. B is not relying on a nonexistent plan README: the
plan has seven numbered docs, with shared originals in01 and07.


### Complete B1-R1 returned — correction batch authorized

B completed CHANGES REQUIRED with exactly AH-P02-B1-F1/F2, both medium; no other
finding or optional suggestion. Root read complete new REVIEW, findings and coverage
notes (older architecture records were already read). B verified all32 paths and
248 receipts, did not rerun runtime checks. Reviewer exit source/coord match.
Source freeze is now lifted only for one cohesive correction batch. Both findings
are implementation gaps against approved D1/r2, not a new architecture decision.
Fix both with genuine failed-row DELETE protection and a read-only entire-reopen
proof; add real deferred-COMMIT ambiguity injection (existing Gate infrastructure)
at enqueue/reopen/final boundaries, deletion/retry contention and permitted expiry.
Do not request recheck until all actionable findings are fixed with focused evidence.
B idle; reuse same reviewer for one correction recheck. Keep originalR1 artifact
and logs immutable. No phase03 implementation before actual code acceptance.

Author found existing test infrastructure through Graft: lease_tests::resolution_gate
in lease_resolution_gate.rs exports Gate/GatePool/Stall/Abort/connect/resolution_behind
within workflow, GatePool already implemented for AdmissionFixture. Gate can stall a
real deferred COMMIT beyond WriteBound attempt, then apply or abort it; use current
read-only source spans from Graft. channel_question_commit_tests.rs shows actual use.
No new proxy framework needed. Gate::drain releases/retakes/releases the advisory gate.
Its gated_policy is 6s; native model fixture actual claim60s can use a shorter test
write policy. Runtime agent fixtures use their actual lease() policy. Recorder::Injected
marks intentionally stalled tests; no raised limits. Graft additional≈45k after review
freeze (current continuation≈675k), no dollar metadata.


### Correction implementation checkpoint

Both F1/F2 code fixes and all planned focused tests are written, unreviewed and
not yet runtime verified. Full details logs/AH-P02-B1-corrections.md. NewSQL uses
source FOR SHARE NOWAIT from DELETE to avoid exchange→run wait cycle, retains
failed/resumable/unclassified until immutable deadline under actual retry owner,
allows expired/nonresumable/tenant teardown. New linked_on read-only reactivation
proof integrated in open's Ambiguous branch. Added retry/deletion and real commit
ambiguity test child files (reopen, enqueue, final), normal replay test renamed.
Main/test newDDL migrations-findings02 PASS after resetting onlyownedp02pair.
Active Cargo: findings-history-list-02 via execsession35214, after style01. Wait
for completion and fix compile errors before runtime. Earlier storage list01
PASS9 proves old correction source compilation before adding ambiguity modules;
not a runtime pass. No other active checks, B idle after whole review. Do not
request R2 until all findings have focused evidence and source/SQLx/lint gates.


### Superseding correction check state

findings-history-list03+runtime03 PASS58/0ignored. findings-affected-list04+runtime04
PASS238/0ignored. Actual injected commits all pass, including both reactivation outcomes
and actual final provider replay count1; native failed deletion/expiry/teardown and
real control contention pass. Aggregate257 with19 explicitly reused unchanged retention
cases in AH-P02-targeted-runtime-evidence-r2.json. Both fresh migrations-findings02,
SQLxprepare/check-findings02 and offlinealltargets-findings02 PASS. Active pipeline
execsession78114 is on Clippy then fmt/diff/Graft; no other checks. Source currently
unfrozen pending final gates, no further edits planned. R2 freezer is prepared as
logs/freeze-review-r2.py (do NOT rerun original R1 freezer or overwrite R1 archives).
Run it only after every named gate passes; it builds full35-path phase delta plus
explicit ten-path R1→R2 correction diff and pins original source/archives/findings.
Then update records and freeze source for one B correction recheck. B idle, not yet
assigned R2. OriginalP02code still unaccepted; no phase03 implementation allowed yet.


### R2 correction freeze ready

All correction gates PASS, session78114 closed. Source now FROZEN
c24664b28aed5506158d3a2e709ae3c5ec1228567e03fdff06bd15c146a08147, owned82/phase-delta35/corrections10.
ArchiveSHAbfea3a87c7f347549e52ace9857fd1b4084a8780765430c0adab8de42e931c53; logs/AH-P02-B1-R2-input.json pins all.
238current targeted PASS/0ignored including58history, aggregate257 with19 explicit
unchanged retention passes. Schema/SQLx/offline/lint/fmt/diff/Graft PASS, external36
preserved, coord5 exact. No checks active. One B correction recheck is being assigned;
no source edits while it runs. R1 archives/records retained. P02 not yet accepted.

B /root/channel_reply_review_recovery is now RUNNING AH-P02-B1-R2 correction recheck against c24664b2. Same source freeze/coord5/external36; no root checks active. Await full return, read all new reviewer records, then exact verification/root acceptance before publishing producer coordination6 and starting03.

B reports R2 source review complete, both fixes address findings; real Gate semantics and238+19/58 evidence verified. It is saving dispositions and final artifact/coordination checks. Await FINAL PASS before root acceptance; B ran no runtime tests. Source remains frozen c24664b2; no checks active.


## Phase02 accepted; producer published

AH-P02-B1-R2 PASS, F1/F2 FIXED, root read complete new review/notes/findings and
independently verified exact sourcec24664b2, source/prior/producer archives, all82
owned and36 external hashes, currentcoord5 and every pinned check receipt/log.
Receipt logs/AH-P02-code-acceptance.json.257 targetedPASS including58history; no
full suite/live provider. Source freeze lifted AFTER acceptance. Root published
primary/local coordination revision6, AH-C002/r5 and AH-C003/r2 accepted producer;
immutable pin logs/coordination-r6.md. Originalr5 review pins stay unchanged.
Next phase03 may start from this exact producer, with its own records/resources
and same9exclusions. Do not write phase02 records from later phases. B idle after
PASS; reuse for03 architecture/code gates. No history commit/push authorization.
