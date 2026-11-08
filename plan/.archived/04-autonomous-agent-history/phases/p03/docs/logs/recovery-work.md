# Unfinished P03 recovery work

Architecture D1/r2 accepted/root activated coord9; no code review yet. Continue all03
before P04, then04–07; targeted-only. B idle; original roles/settings/external36 remain.

## Current evidence (2026-11-07)

- Native turn/caps/waits/preparation/runtime/readiness implementation in working tree.
- Fresh own migrations-main/test03 PASS. compile02 all-targets PASS before newer tests.
- ordering-recovery-runtime03 PASS6: actual competing preparations, parked proof/fast
  wake/once-only continuation, queued cancel, actual scripted final checkpoint release
  before workflow completion and lost-notification durable polling/sweep,32/256 exact
  caps/atomic refusal/no provider, genuine safe retry full/conflict and old position
  preserving newer pointer. No ignored cases. Earlier errors preserved.
- history-runtime01 PASS63/FAIL1 of64. Sole failure old deletion contender gated retry
  after it now owns the exchange tuple; expected55P03 got23514. Updated actual contender
  now holds job row BEFORE starting real retry, observes pg_blocking_pids to prove
  source owner blocked before exchange preflight, DELETE hits source SHARE NOWAIT,
  then blocker rollback lets retry apply with identity intact. retry-deletion-runtime01
  PASS1. This is focused correction evidence; aggregate current history must be run
  after all substantive changes. Prior P02 records untouched.
- New2 real deferred COMMIT tests in agent_history_turn_ack_tests.rs. park-ack-runtime01
  FAILED compile3 fixture Box errors; corrected. Current park-ack-runtime02 RUNNING
  execsession16078 (fmt07 first). Wait actual completion before further Cargo/DB.
  No production fix made yet for the hypothesis below. All failed receipts retained.

## Ambiguous Parked hypothesis to reproduce

Current preparation::proof_on first looks for immutable parked proof, then calls
candidate_on (requires old live fence). During a real COMMIT stall, first SELECT
sees no uncommitted wait; candidate then blocks on original run. If park commits,
its original fence is intentionally retired, so candidate errors OwnershipLost
instead of re-reading the now durable exact park receipt. Aborted park should still
resolve NotApplied and preserve old live source. New two tests release the real
COMMIT after observing the resolver behind it, then assert applied Parked vs
aborted NotApplied, no queue/order allocation on abort, original activation replay
and exact confirmation after fast resume. Do not claim the hypothesis confirmed
until actual runtime02 failure evidence.

A bounded read-only fix if confirmed: in ambiguous resolution use a separate first
transaction to SELECT own workflow_runs id FOR SHARE (serialize behind original
COMMIT without demanding old live fence), then read only exact immutable park proof.
Rollback that transaction before any current global authority lock. If Parked proof
exists, return only WaitId acknowledgement (no history/secret/provider authority).
If absent, use a fresh read-only transaction with existing global-before-local
candidate/fence/matching/owned Ready proof. No writer or new queue/owner, same total
WriteBound resolution budget. A source-first park acknowledgement transaction must
never keep its source lock while taking later global authority locks. This avoids
both false NotApplied while original commit is in flight and false stale lease
requirement after genuine park. Existing Ready applied/aborted tests must stay green.
No schema decision change expected; full code review still challenges it.

## Required remaining focused proof / implementation gaps

1. Complete new real Parked COMMIT applied/aborted proof; extend Ready/reopen/release
   effect assertions. Existing accepted02 Ready applied/aborted/final ack gates pass
   current history63 but new turn effects need explicit assertions. Add final output
   unknown-ack queued successor boundary: provider once, stalled final COMMIT keeps
   old visible pointer; after real commit accepted output and pointer release agree,
   queued successor is ready. Invalid-output model settlement must also release.
2. Actual current revocation at queued head: resume readiness reserves pointer without
   granting provider/history rights, next real lease prepare refuses, normal native
   failure owner releases; no hot wait-head auth retry. Deadline vs resume, true
   stale/lease replacement, restart after enqueue/acquire before CP, invalid final,
   cancel/final and checkpoint/turn actual contention. Preserve all unknown model/effect
   accounting/counters, immutable inputs/checkpoints/deadlines and63 continuations.
3. Native bypass negatives: steal/clear live pointer, activate nonowner, cross tenant/
   conversation pointer, forged/retagged turn-wait/attempt proof, wrong generation,
   no provider on queued/cap refusal. Real capacity contenders at boundary (current
   32/256 tests exact sequential population plus separate actual admission contest).
4. Actual parent/private-child/other-root three-way contender required by AG1-F2.
   Candidate strong setup: actual workflow-tool parent Active waiting for child;
   another genuine manual root reuses parent's private session, actual child has
   private branch. Hold tenant admission leaf in an independent fixture tx; child
   prepare locks own source/private conversation then waits leaf. Start real parent
   cancel: it settles parent, holds parent conversation, then blocks on child source.
   Start other-root prepare, observe it blocked on parent conversation (without leaf).
   Release fixture leaf; child finishes/preparation releases own source, parent
   cascade finishes, other root progresses. Observe actual pg blocking graph under
   bounded hang guard; join all spawned operations. Reuse existing workflow-tool
   fixtures, no invented parent/child scopes/false copied preparations. Existing
   depth/resource/supervision bounds and human/approval/delivery holds must pass.
5. Parked slot release/independent Ready progress through actual existing supervision
   or worker boundary, no held DB resources during provider. Current runtime returns
   Suspended and has no CP; generic baseline capacity/fairness84 is only prerequisite.
   Check bounded readiness query plans on representative pending rows where useful;
   no unrelated production perf changes. Existing positive poll/pages/SQL timeouts stay.
6. Focused affected old wait/poll/control/fairness/capacity84, lease/deadline/cancel/
   approval/tool-child/delivery/checkpoint/model-accounting/repair/channel cases; not
   a full lease or whole suite. Actual SQLx prepare/check, final offline alltargets,
   Clippy-Dwarnings, fmt/diff and graft build. Correct last capacity test currently
   ~94 lines by extracting fixture setup/cancellation helper; no tests/struct tuple
   style weakening. No limit/stack raises.
7. Freeze all source with exact9 exclusions, collect cumulative source scope/archive
   (currently ~96 owned, earlier checkpoint94/delta35 stale), verify external36,
   current coord9 decision/codechecks, and request one whole cohesive P03 B review.
   Correct all findings/recheck/root exact acceptance, publish producer notice before
   P04. No history commit/push authority inferred. No full suite or offer.

Graft reported savings in this continuation conservative ~500k tokens (no dollars
provided); earlier history continuations >1M, original03 >2M. No tool upgrade.
