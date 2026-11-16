# Attribution p04 independent review findings

## AS-P04-B1-F1 — 2026-11-16 — Exercise the gated store-disable/use race

- Actionable verification defect, P2/medium; initial status **open**.
- Workflow implement-astra-reviewed-2-3, p04 implementation review AS-P04-B1,
  [note AS-P04-B1-N1](agent-review-notes.md), [TASK](TASK.md), [REVIEW](REVIEW.md).
  B `/root/attribution_reviewer`, Astra/high requested; actual telemetry unavailable.
  Worktree `/Users/mac02/orca/workspaces/openbot/attribution-service`;
  base/HEAD `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`, reviewed digest
  `6837fba3f8f66fb0160bf308ecc0668d25f33be5a70ad3b65a2895fbedea0c0f`.
- Required: gated AS-P04-D1/r1 AC7 explicitly includes competing store disable/use,
  with a waiting operation rechecking state after the lock. Original04A calls for
  concurrent domain/configuration updates; verification-contract guidance requires
  preserving a planned behavioral gap until exercised or explicitly disposed of.
- Actual: `services/attribution/src/adapters/setup/http_tests.rs:828–847` awaits
  `Mutation::UpdateStore(enabled=false)` to completion and only then calls
  `begin_merchant`. The competitor test at886–959 covers key revocation, not store
  disablement. Scoped exhaustive Trail references find no other StoreUpdate(false)
  competitor. The final TASK AC7 mapping substitutes existing lifecycle/key tests
  without recording the missing store case or an agreed disposition.
- Impact: the sequential assertion cannot establish that an admitted merchant
  waiter observes a disable that wins the tenant lock, or that disable itself waits
  behind admitted use. Inspection finds the intended shared lock in the current
  implementation; this is a missing promised regression, not a demonstrated unsafe
  production commit. Existing key/lifecycle competitors remain valid evidence for
  their own cases.
- Required correction: add a real PostgreSQL competing store-disable/use test
  through the production transaction/mutation boundaries, asserting the documented
  serial outcome and the waiting use's state recheck. Retain the actual criterion
  mapping and result. No architecture revision is required if the protocol stays
  unchanged.
- Evidence: [source excerpts](logs/AS-P04-B1-reviewer-coverage.txt), independent
  [setup run](logs/AS-P04-B1-reviewer-setup.txt):16 PASS, browser separately run.
- Instruction lesson: existing verification-contract guidance already covers
  replacing promised cases with adjacent green tests; no new instruction proposed.
  This gap originated in implementation verification, not the gated decision.

### Resolution — 2026-11-16 — AS-P04-B1-R2 / AS-P04-B1-N2

**Fixed** in `a5e729a610b120f50fb43acddf1c3bd7cda8c7b8a3cc5ae8d394e971607c7275`.
New `attribution_credentials_competing_store_disable_and_use_recheck_both_orders`
at `http_tests.rs:1430–1545` exercises actual `Mutation::UpdateStore` and
`begin_merchant` in both lock orders. `pg_blocking_pids` proves a production
contender is waiting; the disable-first arrangement pauses the production disable
on the session row only after it owns the tenant lock. Releasing that guard lets
disable commit, then the waiting use returns Forbidden without changing last use.
The use-first arrangement commits accepted use before disable can proceed and
rejects later use. B inspected the full fixture/observer and independently reran
the18-test setup selection, all PASS; see
[R2 reviewer output](logs/AS-P04-B1-R2-reviewer-setup.txt). TASK records the formerly
missing AC7 case explicitly. Production protocol and architecture are unchanged.

## AS-P04-B1-F2 — 2026-11-16 — Make expiry replay cross the original payload deadline

- Actionable verification defect, P2/medium; initial status **open**.
- Same workflow, phase, reviewer, worktree and reviewed base/digest as F1;
  review-round note AS-P04-B1-N1, [TASK](TASK.md), [REVIEW](REVIEW.md).
- Required: the credential creation retry contract returns original metadata under
  current authority after the credential has expired; creation's future-expiry
  validation applies only to a new mutation after receipt lookup. This boundary was
  explicitly clarified at G1 and recorded in AS-P04-G1-N1; AC4/AC6 and the final
  TASK cite `expiry_overlap_and_expired_metadata_replay` as its evidence.
- Actual: `services/attribution/src/adapters/setup/http_tests.rs:1255` sets the
  original submitted expiry to now+five minutes. At1337 the test changes only the
  database row to the past, then replays the unchanged still-future payload. The
  full independent setup run finishes in5.28s. Thus the original payload's expiry
  validation could incorrectly move ahead of receipt lookup and this test would
  continue to pass. It establishes replay after editing persisted key state, not
  replay when the original creation payload becomes invalid as a new request.
- Impact: the claimed regression does not protect lost-response recovery across
  the original expiry boundary. Production currently looks correct:
  `postgres.rs:229–235` serves receipts before `apply`, and creation expiry is
  checked at351–357. No current runtime rejection is alleged.
- Required correction: exercise an exact creation replay after the original
  submitted expires_at is at/before the database's current time while key/time/body
  and receipt remain unchanged. Assert metadata-only replay, original credential ID
  and no second key/secret; the same expired payload with a new operation identity
  must be rejected. Keep the fixture bounded and distinguish this case from rotation
  overlap and manual persisted-state changes.
- Evidence: [source excerpts](logs/AS-P04-B1-reviewer-coverage.txt), independent
  [setup run](logs/AS-P04-B1-reviewer-setup.txt).
- Instruction lesson: existing discriminating-case guidance is sufficient; no new
  instruction proposed. The architecture stated the correct rule; its execution
  evidence misses the time-dependent branch.

### Resolution — 2026-11-16 — AS-P04-B1-R2 / AS-P04-B1-N2

**Fixed** in `a5e729a610b120f50fb43acddf1c3bd7cda8c7b8a3cc5ae8d394e971607c7275`.
New `attribution_credentials_replay_after_original_payload_expiry` at
`http_tests.rs:1548–1623` creates a credential with an expiry derived from database
time two seconds ahead, then waits (bounded) until database time crosses that exact
deadline. It preserves original body/key/time/receipt and does not edit stored
expiry. Exact replay returns metadata only and the original credential ID; the same
expired payload with a new operation identity returns Invalid. One credential and
one credential-creation receipt remain. B inspected each decisive assertion and
independently reran the18-test setup selection, all PASS;
[R2 reviewer output](logs/AS-P04-B1-R2-reviewer-setup.txt). The earlier persisted-row
test is correctly retained as separate evidence, not proof of the original-payload
deadline. Production receipt/expiry ordering and architecture are unchanged.
