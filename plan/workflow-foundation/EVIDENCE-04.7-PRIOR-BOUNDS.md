# 04.7 genuine prior-entry boundary evidence

This is a bounded implementation/test milestone, awaiting independent Astra actual-code
review. It does not accept the full bounds contract or 04.7; no04.8 work began.
Original Execution6, BRIEF-04.7 and accepted attribution/witness refinements,
CONTRACT-04.7-AUDIT, RESUME and CONTRACT-04.7-BOUNDS remain authoritative.
Accepted affected expansion and production correction were reused without reopening:
`/private/tmp/workflow-04.7-bounds-expansion-recheck.md` and
`/private/tmp/workflow-04.7-bounds-correction-code-review.md`.

Frozen exact implementation/evidence/log identity:
`/private/tmp/workflow-04.7-prior-bounds-01a0f664/HANDOFF.md`.
HEAD remains c9cb4b434258b6d33ca26def4ff6b5728dd5a6bc. Initial status/diff/WIP
hashes and own-before source copies are in that folder. All50 applied migration
files/checksums are unchanged. All unrelated initial WIP remains hash-identical.
Root owns acceptance documents; worker changed only three test/fixture source paths
and this separate evidence record. No production Rust, SQL migration or bound changed.

## Observable discriminator

New `action_reconciliation_prior_bounds_tests.rs` builds ONE invocation with128
remote entries from128 distinct genuinely claimed attempts. Every entry is reserved
and entered through ActionService/PostgresActionDispatch; provider polling durably
registers that exact request. Every lost result is retired by release_io; the genuine
provider operation lock/barrier closes prior requests and rejects delayed application.
Snapshot and trusted ledger verification then create a new final proof covering
exactly all entries, reopening the same job through its existing reconciliation owner.
Subsequent claims genuinely race two claimants and assert one owner.

Fixture setup uses existing supported root-budget fields before admission, including
132 repetitions, and initializes finite schema-valid max_retries=132 BEFORE the first
claim. No allowance changes afterward; no attempt/debit copying, trigger bypass,
history reset, exhausted-budget replenishment or invented workflow retry key.
The original shared setup_action body is extracted into setup_action_on_fixture;
existing callers retain their original fixture and behavior through a boxed seam.
Each phase is boxed to preserve the stock2MiB regression gate.

Each cycle asserts exact entry/consumption totals, snapshot length, coverage count,
provider calls, SQL recovery safety and all-table preservation of attempts/debits/
frozen intent/marker/execution/job identity while scheduling. At the128 boundary,
there are128 entries/127consumptions, exactly one marker, and available unconsumed
final proof. Snapshot128 succeeds without truncation. Old covered/unmatched and
absent exclusions return NULL without writes.

The129th attempt claims normally; reserve+deferred commit+enter then succeed through
actual ActionService and provider polling with128 prior entries plus ONE exact
consuming NEW entry, giving129total/128consumptions. Provider polls exactly once,
returns a genuine lost-response timeout and creates no effect/receipt. The predicate
returns only the consumed proof for that exact consuming entry; NULL exclusion is
unavailable. This positive fails if the former all-entry count is restored: its
129total rows would reject deferred reservation before the required provider poll.
No mutation experiment or production alteration is claimed.

An authentic verifier is concurrently paused AFTER its128-entry snapshot/verification
while this real dispatch creates the129th entry. After retirement/provider barrier,
settlement rechecks overflow and commits only its bounded-refusal command receipt,
with zero evidence/coverage/audit/revision or reopen mutations. A fresh129-entry
snapshot also refuses before verification, rather than truncating. SQL retry safety
is false; projection needs_reconciliation; all129 attempts retain unknown safety.
Actual claim_io and ActionService with the retired fence produce no new claim,
debit, entry, consumption, provider poll/effect or refund. Whole-table equality
establishes conservative retention, including prior proof/consumption history.

## Exact verification

Both URLs in every DB/build command:
`postgres://mac03@127.0.0.1:55439/workflow_admission`.

- Focused `env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_action_reconciliation_bounds_128_prior`: **1PASS/0FAIL/0ignored**, compile40.72s/test29.29s, `prior-r2.log`.
- Affected combined same environment, filter `workflow_action_`: **116PASS/0FAIL/0ignored**,62.41s, `action-combined-r1.log`. This includes all48 reconciliation tests, the new prior discriminator, accepted sibling overflow/expiry/competing commands and dispatches/loss/current authority/all-proof and mixed continuation plus the shared setup helper's history/replay/receipt callers. Broader filter justified by that fixture extraction's actual caller blast radius; this does not claim all remaining runtime acceptance.
- `cargo fmt --all --check` and `git diff --check`: PASS, `fmt-check.log`/`whitespace.log`; own new files additionally checked by frozen preservation script.
- `env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission cargo sqlx prepare -- --all-targets --locked --offline`: PASS14.02s, `sqlx-prepare.log`; no cache delta.
- Same URL environment, `cargo sqlx prepare --check -- --all-targets --locked --offline`: PASS10.39s, `sqlx-check.log`.
- Same URLs plus `SQLX_OFFLINE=true cargo check --locked --offline --all-targets`: PASS0.49s, `offline-check.log`.
- `graft build`: PASS, `graft-build.log`. All50 migration hashes and unrelated WIP preservation: PASS, `preservation.log`.

Initial focused r1 was a testAPI compile error (settle arguments/private token
constructor); corrected to the genuine paused service workflow. No failed runtime
criterion was hidden. Complete failed/successful command logs remain frozen.

## Resource ownership, context and pending checks

Retained cluster verified before startup: systemID7691265791172745923, PG18,
data/private/tmp/workflow-admission-pg-e3aa, clean shutdown. Exact authorized restart
used127.0.0.1:55439, socket/private/tmp, max_connections200. Live database
workflow_admission/mac03/PG18.6 identity and50 SQLx checksums recorded; retained
cluster is cleanly stopped at handoff, data preserved, no disposable database remains.
No stage/commit/deploy/reset/drop retained data was performed.

Worker /root/prior_entry_bounds, own CODEX_THREAD_ID
01a0f664-86ad-7512-9003-00e5c2d88c11, requestedSol6.1/high, no spawning.
Startup26220/25840010.15%@07:36:39.814Z; focused milestone101053/25840039.11%;
combined milestone108766/25840042.09%@07:46:49.285Z, depth0 bundled helper
usage/window sources. Fresh completion sample follows in frozen handoff; these
are context estimates, not spend. Graft estimated savings summed3,048,442tokens.

Required pending: independent review of this frozen test substep; high-entry foreign/
unrelated exclusions and isolated deferred-entry/consumption/current-xid/historical
retrofit negatives; populated stored-proof upgrade/replay and fresh final schema;
remaining eligibility and RESUME groups2–4; strictClippy and broader affected
runtime/final gates; full original-criteria/integration Astra PASS and root acceptance.
Those obligations are not replaced by this one genuine history or prior scoped PASS.
