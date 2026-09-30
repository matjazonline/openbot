# 04.7 affected expansion — proof coverage bounds at reservation and entry

Status: expansion ready for independent Sol check; no implementation acceptance.
This document is an affected architecture expansion, not implementation acceptance.
Original Execution item 6, BRIEF-04.7 sections 1–5 and acceptance, accepted attribution/
transaction-witness refinements and CONTRACT-04.7-AUDIT remain authoritative. Scope
is only the proof-bound correction within 04.7. Do not start 04.8 or alter earlier
accepted behavior. Preserve HEAD c9cb4b434258b6d33ca26def4ff6b5728dd5a6bc and all WIP;
all 49 applied migrations through 20260930184000 are immutable.

## Required behavior and observed gap

BRIEF-04.7:103–108 bounds evidence coverage to 128 prior entries and 128 invocation
siblings, reads 129 to detect overflow, and forbids truncation. Its sections 4–5
require the sole shared proof predicate at scheduling, reservation, deferred entry
and provider entry; coverage excludes only the exact consuming NEW entry. The bound
is on prior proof coverage, not a new global initial-dispatch ceiling. Preserve the
accepted ordinary live-fence construction of a genuine 129th sibling receipt in
EVIDENCE-04.7-SIBLINGS. Initial dispatch, supported replay and committed receipt truth
retain their existing contracts; reconciliation/recovery cannot use overflow as safe
evidence or gain new proof authority from it.

Sol's final `/private/tmp/workflow-04.7-bounds-r4.log` reports three stock-2-MiB DB tests:
paused 128-to-129 settlement refusal passes; proof reserve and proof enter after a
genuine 129th sibling both fail their refusal assertions. At 128 siblings final
proof legitimately schedules the existing job. The later ordinary sibling makes
shared retry safety false, yet proof reservation/entry still succeeds. This is
reproducer evidence, not an independent code-review pass.

Frozen reproducer identity: `/private/tmp/workflow-04.7-bounds-frozen/HANDOFF.md`,
`bounds.delta.patch`, `sha256.json` (four changed paths), `dependency-sha256.json`
and `applied-migrations-sha256.json` (49 immutable migrations). The final result is
1 passed, 2 failed, 0 ignored, 2324 filtered, 4.54 seconds. Its four-path artifact
contains tests/evidence and fixture wiring/visibility only; no production correction.
The entry-history boundary below is derived from actual SQL and original criteria;
the frozen reproducer does not yet test 128/129 entries for one invocation.

Actual SQL inspection identifies another boundary mismatch: the current predicate
counts all entries before applying `excluded_entry`. Thus a proof covering exactly
128 prior entries can be selected, but its consuming entry (129 total) makes the
deferred predicate reject. Exactly 128 covered prior entries plus their one matched
consuming NEW entry must remain valid; 129 prior entries must not.

## Owners, callers and chosen correction

Paths below are under `src/adapters/persistence/workflow/` unless stated otherwise.

- `action_reconciliation/mod.rs:66–136,273–279`: snapshot reads bounded prior entries
  and dispatch-marker siblings, returning BoundExceeded before verification.
- `action_reconciliation/settlement.rs:115–202`: settlement repeats overflow before
  snapshot/clock checks; optional existing-job continuation uses the shared reopen
  predicate. These gates already reject sibling overflow.
- `action_receipts.rs:60–135`: `marker_on` selects supported replay or available
  final proof; `reserve_entry_on` selects proof before atomically inserting entry and
  consumption. `action_dispatch.rs:213–322` owns authorized reservation and later
  entry; entry checks a consumed proof using the exact reserved entry ID.
- `migrations/20260930180000_workflow_action_evidence.sql:137–210`: sole
  `workflow_action_not_applied_available(company,invocation,excluded_entry)` checks
  entry count but lacks sibling count. `workflow_action_retry_safe` independently
  caps dispatch-marker siblings and preserves conflict/receipt/applied/replay/proof
  precedence. Do not make these functions recursively call each other.
- The same migration:271–345 routes deferred remote entry, consumption and reopen
  through that predicate. Migration 181000:39–52 additionally requires genuine
  entry/consumption/current transaction identity. Keep those protections intact.
- `recovery.rs:21–96` and `pending_recovery.rs:47–96` call shared retry safety;
  retirement callers include lease retirement, completion and budget exhaustion.
  Migration 150000's deferred retirement/control-retry users retain that same owner.
  Migration 182000:108 uses available proof for effect projection, so overflow must
  also stop projecting unconsumed proof as currently usable, without deleting truth.

Implement the smallest additive migration after 184000, replacing only the shared
proof availability function unless the independent check demonstrates another owner
must change. Keep its signature and NULL-on-unavailable convention.

1. After restoring the exact invocation's remote marker, independently count at most
   129 dispatch markers for that marker's company/execution; more than 128 returns
   NULL. Use the same sibling population as snapshot/shared retry safety, including
   receipted or local markers. Do not count only unresolved or remote siblings.
2. Count at most 129 **prior** remote entries for the exact company/invocation. With
   NULL exclusion, count all. With a consuming entry, exclude only that exact row;
   the function may return a proof only when existing matching consumption binds
   that row to the returned proof and existing full composite provenance applies.
   Unrelated, foreign, missing, or merely caller-named entries cannot create an
   exemption. Keep coverage exclusion, no-covered-consuming-row and old-processing-
   attempt checks. A failed match returns NULL even if the provisional count fits.
3. Retain every other condition: remote marker, immutable scope, live validity,
   receipt/applied/conflict vetoes, unique effective proof/consumption, exact complete
   prior coverage, no old live attempt. Retain deferred live-fence and transaction
   identity guards. No historical entry/consumption rewrite, refund, backfill or new
   attempt/queue owner. No production resource bound or coverage cap is raised.
4. Reservation and entry inherit this decision through existing calls; do not add a
   separate Rust count or a global ordinary-dispatch cap. Keep owning-run locking
   and current authorization/lease checks. Overflow before reserve creates no new
   entry/consumption; overflow after reserve refuses provider entry while preserving
   the already committed entry/consumption conservatively.

Do not require whole-execution `workflow_action_retry_safe` at proof entry. It is a
scheduling decision, not an I/O grant: once one sibling consumes its proof without
a receipt, that sibling is deliberately unresolved again. Calling it would block
the other independently proven siblings and could recurse through the proof owner.
The new execution-wide condition here is only bounded sibling population; existing
execution-wide conflict veto remains absolute. Receipt/supported-replay precedence
in shared recovery remains unchanged, including NULL for no markers.

## Delivery and discriminatory acceptance

Independent Sol check must challenge this expansion against originals and actual
owners before root authorizes dependent production edits. Then Sol owns migration,
tests and evidence. Apply the new migration before testing; preserve all applied
checksums and existing fact bytes. Astra reviews the frozen implementation separately.

- Retain the reproduced genuine 128/129 sibling tests, including paused verification
  settlement, post-schedule overflow before reserve, and overflow after reserve before
  enter. Refused reserve has zero additional entry/consumption; refused enter retains
  its one committed consumption. Assert all-table equality on refused boundaries and
  zero provider polling/effect. Original ordinary 129th receipt remains possible.
- Build genuine repeated remote-entry history through existing owners. Exactly 128
  prior entries must permit snapshot, authentic final proof, scheduling, reserve and
  enter with 129 total entries and one new unique consumption. A lost result leaves
  129 prior entries for the next cycle: snapshot/settlement/new proof/recovery remain
  conservative, with no refund or truncated coverage. Distinguish prior count from
  total count explicitly in assertions.
  The isolated fixture may initialize a sufficient finite attempt allowance before
  its first claim, then preserve that allowance throughout all claim/retire/reopen
  cycles. This is test setup, not replenishment of an exhausted job. The existing
  `fixture_source` admits a normal job using the database retry default; no authored
  workflow retry key is established by this expansion. Use a schema-valid fixture
  initialization of `max_retries` before any attempt rather than inventing such a
  key, changing production defaults, or increasing limits during retries. Existing
  claim owners must generate every attempt, entry and debit; never copy provenance.
  Root-budget setup may likewise select sufficient existing supported limits before
  admission. Coverage caps remain 128, the tests must still fail at 129 prior entries,
  and the positive boundary remains in the required regression gate.
- Check 129 prior entries with NULL exclusion and invalid exclusions (unrelated,
  wrong invocation/company, absent or unmatched consumption); none may return proof.
  Exercise deferred entry/consumption rejection as well as the Rust boundary, with
  otherwise valid scope and exact expected diagnostic so an earlier FK does not mask
  the target. Preserve current-xid/historical-retrofit protection.
- Prove SQL/Rust shared recovery agreement at both bounds; all-receipt sibling
  overflow stays unsafe even though receipt truth remains intact. Reuse accepted
  all-proof/mixed sibling continuation tests to prove sequential proof consumption
  still works; re-run expiry, competing commands/claimants/entries, consumed-proof
  loss and current-authority tests. No sequential mock substitutes for contention.
- Fresh schema plus populated upgrade must show the replacement function active,
  no rewritten evidence/coverage/commands/audits/receipts/consumptions/history, exact
  immutable replay still returning its original result, and tightened availability
  on an already stored proof whose siblings subsequently overflowed.
- Run stock-2-MiB focused bounds and combined reconciliation suites, relevant existing
  action/recovery/pending/lease/completion/control/budget regressions, formatting and
  whitespace, locked offline all-target compilation, strict Clippy, fresh migration/
  schema inspection, SQLx prepare/check and graft refresh. Full 04.7 remaining
  eligibility/matrix/schema gates and independent integrated acceptance stay open.

No unresolved product decision is required. This correction defines no production
verifier/transport and does not expand cancellation scope into 04.8.
