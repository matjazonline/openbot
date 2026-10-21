# Senior review findings

### F-001 — supported entry lost its separate all-relation no-write oracle

- First observed: R-004, D-003 manifest SHA256
  `e2a41cc3b06aabaaefb0939de2916f94f8f657aee91d5ec403d6401643cc93cb`;
  S-004's no-weakened-oracle requirement and DEC-010.
- Impact: acceptance-blocking test regression, not an observed production defect.
  The final net delta remains checked, but reservation and entry are no longer
  independently checked against every public relation.
- Evidence: `action_reconciliation_high_bounds_native.rs:269–284` snapshots only
  `workflow_action_remote_entries` between reserve and entry. The old code checked
  `reservation_delta(before, committed)` immediately after reservation, then full
  `all_tables == committed` after entry. The new check permits, for example, a
  non-entry row changed by reservation and restored by entry: the combined delta
  and intermediate entries-only equality both pass, while the old checks fail.
  Thus IMPLEMENTATION.md's claim that this is equivalent to the old post-entry
  all-table equality is too strong.
- Correction criterion: retain a separate post-reservation observation covering
  every formerly checked relation, and prove both the reservation-only delta and
  entry-only no-write outcome. A faithful compact fingerprint is acceptable where
  row-level assertions are unnecessary. Preserve the frozen reservation window,
  real entry call, original limits and explicit lapse failures. D chooses the
  efficient observation method and validates it under required parallel load.
- Status: **resolved — R-005**.
- History: first recorded open in R-004. D-004 manifest SHA256
  `64ce8860703195366dd8c14b838ac81951d040971fa7183079f457c40bd4935f`
  restores the intermediate observation as an all-public-relation fingerprint.
  Post-entry full rows and fingerprint share one repeatable-read snapshot; equality
  against the intermediate fingerprint proves entry-only no-write, allowing the
  full rows to prove the separate reservation delta. The compensating-write
  counterexample now fails. The extended equivalence test checks real fixture
  write sensitivity. S inspected source and current-binary isolated/full-run
  evidence (three parallel 310/310 passes); limits and lapse failures are retained.

### F-002 — changed prior-bounds helpers lack current negative-control execution

- First observed: R-004, same D-003 manifest; DEC-010/011 and TASK.md group A's
  affected reconciliation verification requirement.
- Impact: acceptance-blocking verification gap. The successful main proof does
  not exercise the exceptional paths that qualify voids, reject unrelated errors
  and ensure bounded exhaustion fails.
- Evidence: D-003 changes `PausedVerifier::run`, the settlement wrapper and
  `park_observed`'s snapshot path in `action_reconciliation_prior_bounds_support.rs`.
  D-001 ran seven `::controls::` tests on binary `f14fb5cd…`; that predates these
  changes. The three D-003 `workflow_action_` runs select the positive prior-bounds
  test but omit the seven controls and the support timeout discriminator, whose
  full test names contain no `workflow_action_`. No current execution evidence
  for those controls is supplied.
- Correction criterion: on the correction artifact, run the seven named
  `prior_bounds_*` controls plus
  `prior_bounds_timeout_discriminator_rejects_prompt_release_and_wrong_errors`.
  A `prior_bounds_` filter also selects the positive test through its module path;
  list and verify the actual count rather than assuming eight selected tests.
  Preserve stock 2 MiB, real task DB execution, original caps, zero ignored/skipped
  cases and drained fixture state. Record binary/source identity and results.
- Status: **resolved — R-005**.
- History: first recorded open in R-004. On the same D-004 artifact,
  [current control log](logs/groupA/runtime-prior-bounds.D004-prior-controls.log)
  and result JSON record all nine selected tests passing on binary `72cca654…`,
  stock 2 MiB/default parallelism, zero ignored and zero residue. This includes
  all seven controls, the timeout discriminator and the positive proof. Logs
  show exact-lapse backstops, both reservation-unknown outcomes, three qualified
  voids followed by the required exhaustion failure, and expected panic rejection
  of foreign-fence/unexpected-error cases. S checked listed names and raw results.

### F-003 — SQLx preparation evidence predates D-003 query edits

- First observed: R-004, same D-003 manifest; `src/AGENTS.md:176` and
  `src/adapters/persistence/AGENTS.md:272–283`, plus TASK.md group A SQLx gate.
- Impact: acceptance-blocking required-check gap, not a demonstrated SQL defect.
- Evidence: D-003 adds/changes runtime SQL in the snapshot/digest and lease-phase
  helpers. IMPLEMENTATION.md:395 says no SQL in `.sqlx` scope changed, but the
  persistence guide explicitly requires running preparation even for runtime
  queries that emit no cache entries. The supplied `sqlx-prepare-check.log`
  predates D-003; offline check and Clippy do not substitute for this command.
- Correction criterion: D runs required SQLx prepare/check against the task-owned
  database on the correction artifact, records logs and cache identity/delta,
  and reviews any generated changes. An unchanged cache is expected for these
  runtime queries and does not itself verify their SQL; retain their DB-backed
  runtime tests as well. No user waiver is needed to perform the prescribed check.
- Status: **resolved — R-005**.
- History: first recorded open in R-004. The D-004
  [SQLx preparation/check log](logs/groupA/sqlx-prepare-check-D004.log) records
  `SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets`, sqlx-cli 0.8.6,
  against the task fixture after query edits, exit 0. S verified all 45 cache
  hashes match before/after, D-001 and the current worktree. No generated delta
  requires review. Runtime-query correctness is supported separately by current
  DB-backed equivalence and full-filter execution, not inferred from the cache.

### F-004 — durable-pending test cannot distinguish ownership interruption from expiry

- First observed: R-006, B1 manifest SHA256
  `822434bc4593d9d78bd32f367374e07e2ee10a6d2a0608395cc04647581719ac`;
  BRIEF-04.8 work-in-flight row and DEC-003's cancellation-driven ownership
  supervision/drop-before-return obligation.
- Impact: acceptance-blocking test-oracle gap, not an observed production defect.
  The test proves eventual drop and preserved durable truth, but could pass if
  cancellation no longer caused ownership supervision to interrupt the provider.
- Evidence: `action_cancellation_boundary_tests.rs:384–431` allows eight seconds
  for dispatch plus cancellation, then requires Interrupted/drop==1. The original
  fixture lease is three seconds (`lease_tests.rs:12–14`); `enter_remote` freezes
  that deadline (`action_dispatch.rs:518–528`). `supervise_remote` independently
  returns Interrupted and drops work at that deadline, even without a false
  `owns_remote` response (`application/workflow/actions/dispatch.rs:164–190`).
  For example, an erroneously always-true ownership probe would still reach that
  deadline inside eight seconds, with all the current durable snapshot, call,
  effect, drop and inertness assertions satisfied. This is a source-derived
  counterexample; S did not mutate production or execute a negative-control run.
  Also, the `dropped==0` assertion at line 398 runs after cancellation commits
  and another snapshot: a correct promptly polled heartbeat is already allowed
  to drop the provider then. That checkpoint encodes a scheduler assumption.
- Correction criterion: establish the live pending premise before cancellation;
  commit the real command with the invocation token still uncancelled; positively
  distinguish the real post-cancel ownership refusal from the independent frozen
  deadline/guard timeout, and require the resulting drop before dispatch returns.
  Use a delegating observation seam and/or a calibrated assertion against the
  actual captured entry deadline; an arbitrary larger timeout or an elapsed-time
  log alone is insufficient. A disabled/always-true ownership-supervision negative
  control must fail this oracle, not pass at deadline. Keep both ledger variants,
  genuine service/adapter calls, current bounds, full durable checks and bounded
  cleanup. Relocate the pending assertion before cancellation, where it is a
  required premise. D chooses the concrete test construction.
- Status: **resolved — R-007**.
- History: first recorded open in R-006; B1's original green runtime evidence did
  not close the missing causal assertion. B1r1 manifest SHA256
  `ef2e0ecd1ef49a5ebea6c0ddb60f42a92cb92d56b1a3300575b4513c5c9aafdb`
  adds a delegating real-ownership probe recorder and captures the actual frozen
  entry deadline. The positive oracle requires a completed false probe started
  after the cancel returned and dispatch return strictly before that deadline;
  errors, absent results, affirmations and deadline termination cannot pass.
  The live pending premise is now checked before cancellation. Both provider
  ledger variants and all previous durable/drop checks remain. A real always-owned
  negative control terminates at the deadline and is rejected by the same oracle;
  synthetic controls cover the other rejected observation forms. Current-binary
  isolated/repeated, 10-test B1 and 320-test full-action runs pass. S inspected
  source and execution evidence, without running tests. No production architecture
  gate is reopened.

### F-005 — receipt-first race does not exclude ambiguous COMMIT resolution

- First observed: R-008, B2 manifest SHA256
  `19abba9a05d1f619339b6a50772ab83fda7de2ee5145a36288523d5b27198a28`;
  DEC-002's explicit release-before-persistence-deadline/ordinary-race requirement,
  clarified in DEC-012.
- Impact: acceptance-blocking test-oracle gap. The run-lock ordering and final
  durable truth are proved, but the test can accept recovery of an ambiguous
  receipt COMMIT instead of the specified ordinary successful attempt. This is
  not evidence of a production defect or a timeout observed in the supplied runs.
- Evidence: `action_cancellation_receipt_tests.rs:268–331` uses six-second
  `gated_policy` (two-second attempt, three-second total persistence deadline).
  It observes COMMIT stall and cancel's run wait, then releases and checks only
  Applied/Interrupted plus durable facts. There is no release deadline assertion
  or completed-versus-resolved outcome discriminator. `Gate::arm` disables server
  statement/lock timeouts in the deliberately stalled receipt transaction.
  `persistence_bound.rs:263–295,316–331` treats an expired COMMIT attempt as
  ambiguous; `action_receipts.rs:243–253` can resolve its durable receipt and then
  return Interrupted after cancellation. A delay after cancel starts waiting but
  before gate release can cross the two-second attempt bound, then release in
  time for cancellation and resolution to succeed. The current oracle accepts
  that path. Logged 11–39 ms to first gate observation does not measure or enforce
  the whole start-to-release/attempt interval.
- Correction criterion: assert timely release against the primary attempt and
  require the ordinary completed settlement outcome, rejecting resolved-applied,
  retry/unknown/not-applied outcomes as this race's positive witness. Existing
  test-only recorder/observation support is sufficient; no production seam is
  needed. Preserve both exact blockers, free/held/free probes, exact business
  outcomes, late truth/replay assertions and bounded draining. Add a calibrated
  late-release negative control proving the ordinary oracle rejects a resolution
  path even when the final business outcome would otherwise match. DEC-012 permits
  this one existing six-second fixture policy with those checks and retained CI
  work-budget coverage; it does not approve another bound increase.
- Status: **resolved — R-009**.
- History: first recorded open in R-008; the original green B2 runs did not
  establish the missing attempt-path discriminator. B2r1 manifest SHA256
  `1b109cc661ccf7dd6b692f06f2882e7d8152325355931bd23d1d2184951eeace`
  asserts release before a conservative deadline anchored before settlement starts,
  and exactly one `action.commit_remote` recorder event with outcome `completed`.
  Its real late-release negative control observes the resolution waiter, obtains
  `resolved_applied` with the same Applied/Interrupted/durable outcome, and is
  rejected by that oracle. Synthetic controls reject missing/retried/resolved/
  unknown events and release at the deadline. Both ordinary and negative cases
  retain calibrated blockers, drained operations and duplicate/restart checks.
  Current repeated isolated controls, 16/16 boundary and 326/326 action execution
  evidence passes. S inspected source and logs without executing tests.

### F-006 — first wait parking returns a noncanonical deadline

- First observed: R-012 / S-012, accepted B3 manifest SHA256
  `c3379ee330dd4795f1f78a4758aa80c4a832b001ee28c845b015e5881f115131`;
  group C full-suite gate and stable wait/lost-ack replay, DEC-013.
- Impact: acceptance-blocking inherited **production precision defect**. Initial
  and repeated successful park calls can return unequal ParkedWorkflow values for
  the same durable wait. No duplicate effect or worker scheduling error is shown;
  the current worker uses only the response's presence.
- Evidence: `src/application/workflow/waits.rs:67–107` preserves an accepted
  RFC3339 nanosecond deadline. `src/adapters/persistence/workflow/waits.rs:183–207`
  binds its capped value into a timestamp column but returns that pre-storage Rust
  value; `:152–156` replays the stored microsecond value. Installed SQLx 0.8.6
  encodes integer microseconds. [Full-library log](logs/groupC/runtime-all-lib.C.log)
  :2748–2751 shows the same wait ID with deadlines ending `.129214477Z` versus
  `.129214Z`; the unchanged wait-filter rerun is again 15/16. Both sides of the
  failed equality are actual production-operation responses. Source equals HEAD,
  so this was not introduced by B3.
- Correction criterion: first/concurrent/replayed park responses must exactly
  match the persisted wait ID/deadline, while retaining run-deadline capping,
  validation, transactional completion/audit and no duplicate scheduling. Return
  canonical storage truth (prefer INSERT RETURNING deadline) rather than altering
  this test's expected precision. Keep an explicitly non-microsecond input and
  exact response/storage equality in the regression, on every platform; execute
  affected wait/control/polling checks and SQLx preparation after the query edit.
- Status: **open — scoped production correction awaits root/user decision**.
- History: first recorded in R-012. DEC-013 flags the narrow inherited wait
  boundary for scope reopening; user authorization for test-only portability
  fixes does not silently authorize this production correction. No code changed
  or tests executed by S; no deferral/waiver of the failed group C gate.
- Status update — **resolved in R-014**, C1 manifest SHA256
  `e59bb3f02615d685854ae03bae674f57017acc0b66bbdb9ee89c12d230be7dfd`,
  HEAD `94c77e0893248a45f338de5701649cea8f9b3c3d`. TASK.md/D-011 record the
  later user authorization for this production correction. `park_on` now obtains
  the deadline from `INSERT ... RETURNING deadline` within the same transaction;
  capping, validation, run-first serialization, completion/audit and commit-before-
  return remain unchanged. The competing-call regression forces 477 ns beyond
  microsecond precision and checks both responses, persisted deadline and replay
  exactly. D's recorded pre-fix negative check fails at the competing equality,
  then restores the fixed bytes; current isolated x3, wait/control/polling and full
  library/stock-stack executions pass. Live SQLx check was rerun; runtime SQL
  generates no cache entry. S inspected source, hashes and logs, without executing
  tests. The original open/scope-pending text above is retained as history.

### F-007 — pending run-deadline oracle can accept a lease-limited entry

- First observed: R-014, C1 manifest SHA256
  `e59bb3f02615d685854ae03bae674f57017acc0b66bbdb9ee89c12d230be7dfd`;
  DEC-014's requirement that the pending run-deadline case prove the actual run
  deadline constrained entry, rather than silently becoming a lease-expiry test.
- Impact: acceptance-blocking test-oracle gap, not an observed production defect
  or assertion that the recorded passing runs took the wrong path. The wider
  fixture window has a new causal assertion, but that assertion does not establish
  which of the two independent expiry sources it is crediting.
- Evidence: `action_dispatch_remote_tests.rs:494–506` renews first, later updates
  the run deadline to DB now + 1.5 s, and records a monotonic timestamp after the
  update returns. No assertion compares the actual run deadline with job expiry
  or the frozen reservation's distinct lease/run windows. `:470–480` accepts any
  entry deadline <= that post-update timestamp + 1.5 s, all recorded probes true
  (including an empty list), and return at/after entry deadline. Actual entry is
  the minimum of four lease/run candidates (`action_dispatch.rs:518–528`).
  Source-derived counterexample: renew a three-second lease, then delay two
  seconds before the run UPDATE. Run expiry is now about 3.5 s after renewal;
  genuine reserve/entry and first poll can still succeed before the lease's
  approximately 3 s expiry. The captured lease-limited deadline passes the upper
  bound, the provider drops at that deadline before the first one-second heartbeat
  (empty probes), and all existing outcome/poll/drop/fact assertions pass. The
  run deadline did not cause termination. No runtime mutation/reproduction was
  performed by S.
- Correction criterion: positively establish from genuine persisted/frozen
  observations that the run deadline, not lease expiry, limits this entry; reject
  a lapsed or reversed fixture premise before crediting the run-deadline proof.
  Preserve first poll while live, actual pending-future drop before return, live
  token, no ownership-refusal substitution, four-second watchdog, real adapters
  and unchanged production bounds. Add a control that the same oracle rejects
  an otherwise successful lease-limited case, including the empty-probe case.
  A delegating test seam can retain existing reservation/window or DB observations;
  no production port or limit change is requested. Rerun affected tests and gates
  on the corrected artifact. The 1.5 s fixture window itself is acceptable once
  this actual-boundary assertion fails reliably; reverting to the guessed 0.4 s
  setup race is not required.
- Status: **open**.
- History: recorded R-014. Existing B1/F-004 proves cancellation-driven ownership
  loss, not this distinct pending run-deadline premise; its green controls do not
  close this gap.
- Status update — **resolved in R-016** on HEAD
  `94c77e0893248a45f338de5701649cea8f9b3c3d` plus the sole source delta
  `action_dispatch_remote_tests.rs`, SHA256
  `36e0f1a287be1c7720809e2d46700861558fe02196bccba1747d49d50d3aafb4`.
  C2 now observes actual persisted run/lease remaining durations immediately
  after genuine entry, brackets that read with monotonic instants, rejects
  lapsed/reversed windows, requires the conservative run upper bound plus 250 ms
  to precede the conservative lease lower bound, and requires the frozen entry
  no later than that run upper bound. No intervening fixture renewal/mutation can
  change this comparison. A genuine lease-first pending control retains the old
  successful Interrupted/poll/drop/facts and upper-bound conditions but is rejected
  by the same oracle; recorded x3 runs show empty probes and the exact reversed-
  window rejection. Synthetic controls cover lapsed/equal/insufficient-margin/
  missing-window and too-late-entry forms. Current positive/control x3, dispatch
  22/22 and full stock-stack 2559/0/22 pass on binary `94a422dc…c8fd5e`.
  S inspected code/logs and hashes, without executing tests. This closes F-007's
  specific defect, not final C2 acceptance; F-008 and R-016's F-009 remain open.

### F-008 — final graph refresh evidence predates C1 source

- First observed: R-015, same C1 manifest; BRIEF-04.8 section C and GUIDANCE.md's
  final artifact/static-schema checklist explicitly include `graft build`.
- Impact: remaining acceptance-blocking verification/maintenance gap, not a
  production correctness defect. The old zero graph delta does not describe the
  final artifact's changed symbols and spans.
- Evidence: `logs/groupC/graft-build-C.log` ran at 2026-10-15 18:29:28Z on the
  unchanged B3 source; `graft-delta-C.txt` belongs to that run. C1 adds the dispatch
  recorder/oracle/mode helpers and changes six source files afterwards. No final
  C1 graph build/delta record was supplied. Query-time discovery of current symbols
  is not evidence that the required complete final build was executed.
- Correction criterion: D runs `graft build` after the final correction source is
  frozen, records success and the graph delta (or a justified zero delta if the
  cache has already refreshed), and ties it to that final source manifest. This
  ignored graph-cache operation alone does not invalidate runtime/schema evidence
  or require new runtime tests; substantive source corrections do require their
  affected rechecks.
- Status: **open**.
- History: recorded R-015; S remained read-only and did not run the graph build.

- Status update — **resolved in R-017**, final C2 manifest SHA256
  `5f8181076864e1fe0e1184380bdac8949987b8685faca8333e7bc51c389debf0`.
  D's `logs/groupC/graft-build-C2.log` records the final frozen-source build at
  2026-10-16T01:07:00Z, exit 0, 16639 nodes / 26557 edges / 862 cards.
  S recomputed the saved before/after manifests: 880 files each, no additions or
  removals, three changes (task tests card and extraction/fingerprint caches),
  exactly as `graft-delta-C2.txt`. The F-007 card had already refreshed through
  queries; the explicit complete build now supplies the missing final gate.
  This ignored-cache refresh requires no runtime rerun.

### F-009 — stop-versus-dispatch test expects task completion without worker closeout

- First observed: R-016, C2 source identity recorded there; direct full-suite a1
  binary SHA256 `94a422dc15bc09486aeb99872f5ecf140982df93183eec696e0db072a0c8fd5e`.
- Impact: acceptance-blocking inherited test-oracle defect in the required full
  suite. This is not evidence of a C2 production regression, duplicate reply, or
  a task lease expiring under load. Passing another scheduling order cannot close
  the invalid expectation. Root must scope the narrow test correction; S-015
  authorizes diagnosis/review, not source edits by S.
- Evidence: `task/tests.rs:7449–7558` races actual StopTask against direct
  `commit_agent_dispatch`, then at `:7532–7540` expects Completed when stop returns
  Conflict and dispatch returns Committed. The C2 a1 failure at `:7532:75` records
  Processing versus Completed, proving it reached that branch. The five-minute
  claim helper is `:4527–4540`; no short fixture deadline is involved in the failed
  assertion. `task/operations.rs:1524–1780` fences the task, writes reply/deliveries,
  completes the outreach and commits, but does not set task status Completed in
  this publication branch. `task/controls.rs:991–1000` refuses the stale outreach
  version before applying stop. Task completion is the distinct fenced operation
  `task/operations.rs:2381–2411`, invoked by worker `close_out_task` at
  `application/services/task_worker.rs:628–654`. This test never invokes that
  operation/worker closeout. The failed Processing state matches the actual
  dispatch-only contract. Failing test body and both persistence implementations
  are byte-identical to `8533a79`; C1 changed another timestamp in this test file,
  and C2 changes only workflow dispatch test helpers.
- Correction criterion: align the test with the operation it actually exercises,
  retaining real contention and exact outcomes for **both** stop-first and
  dispatch-first orders. At the dispatch-only boundary, assert the preserved
  processing fence plus committed reply/delivery/outreach and stale stop refusal;
  if the test intends to assert Completed, invoke and assert the real fenced
  completion after publication, as worker closeout does. Do not merely accept
  either task status or change production dispatch semantics to satisfy this test.
  Make both outcomes observable/reliable (prefer the existing deterministic
  transaction-blocker pattern) so repeated quiet passes cannot omit dispatch-first.
  Retain no-duplicate customer reply and appropriate cleanup. D owns correction,
  both-order/affected-suite verification and the resulting artifact identity.
- Status: **open**.
- History: this exact test passes in group C full/stack, C1 task-filter/full/stack,
  and C2 a1 stack-budget logs; those logs do not record which winner branch ran.
  The a1 direct run is 2558/1/22; the same binary's subsequent stack run is
  2559/0/22. Foreign workload was observed, but load is not a demonstrated cause
  of the bad assertion; it can change which valid race order is selected. No fresh
  reproduction or fixture operation was performed by S.

- Status update — **resolved in R-017**, same final C2 manifest as F-008 above;
  `src/adapters/persistence/task/tests.rs` SHA256
  `9356e0e2280c63745e9a4221fa74c1eb298e8fffc3eb0971f95d3c7a72614254`.
  `stop_race` uses an isolated database, a held outreach-row transaction, and
  database-scoped lock-wait barriers to line up both real competitors in each
  order. Premature completion fails; release must drain both within 10 seconds.
  Stop-first requires successful stop, LeaseLost dispatch, Stopped and no reply
  delivery. Dispatch-first requires Committed, the exact stale-version conflict,
  the original Processing worker/generation fence, completed outreach and one
  reply delivery. It then calls the real fenced `mark_task_completed`, requires
  success/Completed and still one delivery. No production semantics or bound was
  changed, and neither branch accepts alternative task states.
  All five final f2 isolated runs log both winners; the affected task suite is
  113/113 and the final full suite is 2559/0/22. Source and observed outcomes meet
  the correction criterion. Earlier a1's bad assertion remains recorded as a
  defect, not relabeled as host load. S inspected evidence without running tests.


## 04.9-F001 — Preserve the accepted company-before-run lock order
- Date/workflow: 2026-10-17, implement-astra-reviewed-2.1; architecture round 04.9-R001, decision 04.9-D1, base 2729f2caaf58830bea8291bdcc6f9882a3ad03ee. Reviewer B, requested gpt-6-astra/medium (host settings not independently exposed). Records: TASK.md / REVIEW.md.
- Classification: actionable, medium, architecture; originates in decision D1 item 3.
- Evidence: D1 describes the existing transaction as run/attempt-first. Actual `action_dispatch.rs:74-83` calls `action_authority::prelock_on` before `authorize_prelocked_on`; `action_authority.rs:35-52` locks company and membership through `authority::authorize_company` before `lease::lock_fence` at `action_dispatch.rs:92`. Public control and reconciliation share this order (graft callers authorize_company). A run-first implementation would restore the lock inversion corrected and accepted in 04.8.
- Required correction: state the actual company/actor-before-run/attempt/resource/policy/intent order and retain it across new authority/provider-use paths; identify credential locking and management order without introducing inversions. Existing company-before-run race coverage plus new competing revocation tests must remain applicable.
- Instruction lesson: resolve historical plan lock-order descriptions against the latest accepted code before gating a new collaborator; evidence is D1 versus current source, not an implementation defect.
- Resolution: OPEN.

## 04.9-F002 — Resolve company-member versus owner/admin authority mismatch
- Date/workflow: 2026-10-17, implement-astra-reviewed-2.1; architecture round 04.9-R001, decision 04.9-D1, same base/reviewer/records as 04.9-F001.
- Classification: actionable, medium, authorization architecture; originates in D1 item 1.
- Evidence: D1 says dispatch is current company-member authority, but `authority.rs:9-36` additionally requires owner or admin, and both existing action authorization (`action_authority.rs:47`) and ResourceDirectory (`resources.rs:47`) use it. A newly implemented member-only path would diverge from existing action dispatch/return checks, while retaining the helper would contradict the promised access model.
- Required correction: explicitly retain established owner/admin workflow authority or record and justify a coherent scoped change and its affected callers/tests. Do not weaken the shared helper implicitly. Cover member-without-admin, removed membership/principal and foreign actor in the actual admission/dispatch path.
- Instruction lesson: verify existing authorization predicates rather than using a role shorthand in an architecture gate.
- Resolution: OPEN.

## 04.9-F003 — Identify authoritative publication gate replacement
- Date/workflow: 2026-10-17, implement-astra-reviewed-2.1; architecture round 04.9-R001, decision 04.9-D1, same base/reviewer/records as 04.9-F001.
- Classification: actionable, medium, integration/authorization architecture; missing decision detail.
- Evidence: `authority.rs:41-56` rejects every tool-bearing published bundle recursively. D1 supplies trusted ToolSnapshots and readiness but does not identify how this existing production persistence authority gate is replaced. Merely enabling HTTP resource readiness leaves real publication unusable; deleting blanket rejection without current trusted validation permits caller-supplied frozen policy to become authority (the source comment explicitly explains this boundary).
- Required correction: record scoped authoritative validation at publication for supported HTTP registrations, including recursive child behavior and continued denial of unsupported dependency owners. Verify production publish/bind/admit/dispatch composition and forged/unsupported snapshot denial, rather than only compiler mocks.
- Instruction lesson: when opening a formerly fail-closed provider boundary, enumerate the owning publication and readiness checks together; root/author also independently flagged this seam during review.
- Resolution: OPEN.


### 2026-10-17 — 04.9-R002 resolution follow-up (implement-astra-reviewed-2.1)
Reviewed 04.9-D2 (preserved logs/04.9/review-D2.txt), same base as R001.
- **04.9-F001 FIXED at decision level:** D2 item3 states company/actor-before-run/attempt and connection/policy-before-intent order; management and credential access acquire parent connection first, rotation stays ciphertext CAS without inverse connection/run locking. Matches inspected accepted helpers; new implementation/race tests remain to review.
- **04.9-F002 FIXED at decision level:** D2 item1 retains current owner/admin authorization across publication, binding, admission, preparation, dispatch and replay; ordinary membership is explicitly insufficient. No shared helper weakening proposed.
- **04.9-F003 FIXED at decision level:** D2 item7 requires authoritative HTTP registration validation for publication, recursive children, exact bound resource at binding/admission/dispatch, narrow alias substitution and denial of unsupported dependency owners. Real production path and forged/revoked/member negative tests are required. Existing publication replay returns an equivalent saved immutable result after current actor authorization (`definitions.rs:170-183`); D2 correctly distinguishes that from future use authorization.
No implementation acceptance is inferred from these decision-level closures.


### 2026-10-17 — 04.9-R003 implementation follow-up (implement-astra-reviewed-2.1)
At base `e634a82837d50e7ed4aba4bb003f0a98ec294b10`, digest `7b68a7fcdb1c96d8a1e683feae8bc0008b945573c6f846f72cd30b39aa31ab1a`, B verified implementation of the previously closed decision findings: F001 uses existing company-first dispatch/preparation/provider authority and parent-first credential mutation without an inverse rotation lock; F002 retains owner/admin authority and adds ordinary-member/foreign/revocation checks; F003 supplies authoritative HTTP publication validation with exact selected-resource binding/admission checks and real lifecycle tests. No new concrete finding. Whole-batch code review passes; mandatory final verification remains pending as recorded in R003. Earlier decision-only outcomes remain preserved.


### 2026-10-17 — 04.9-R004 final disposition (implement-astra-reviewed-2.1)
Final base `e634a82837d50e7ed4aba4bb003f0a98ec294b10`, digest `1ad9356a5a29195853d8a5be8e53e4e384465e3365db0ed456ea35f35c76f08b`: unchanged implementation retains R003's verification that F001–F003 are fixed. Required final full/stock-stack/lease and other gates now pass; see R004 evidence note. No new finding, no deferred acceptance-blocking defect, no outstanding required verification gap. Final combined review PASS. Prior decision and implementation histories remain unchanged.

## 04.10-F001 — Specify outcomes for pre-marker and non-committed MCP paths
- Context: 2026-10-17, workflow implement-opus-reviewed-2.1, batch 04.10 MCP, architecture gate, round [04.10-R001](agent-review-notes.md#0410-r001--architecture-gate-0410-d1--changes-required). Reviewer B: Opus 5.5 (xhigh requested). Reviewed decision 04.10-D1, SHA256 `3f10553022b5d631876b8174d57d0ea270ff3c7d150b25d5812fd776e285035e`, base `8ce1fd4`. Records: [TASK](TASK.md), [REVIEW](REVIEW.md).
- Classification: actionable defect, **medium**, architecture/requirements (recovery classification). Originates in the gated decision (D1 items 6–7), not in any implementation.
- Evidence — what D1 classifies: the eager transport/drift failures (`mcp.unavailable` Retryable/SafeToRetry, `mcp.contract_changed` Terminal), `isError`, and post-entry failures.
- Evidence — what D1 leaves unspecified (step or direct-tool outcome):
  - (a) Authority or revocation failures before any marker: in the eager `mcp_for_action` (revoked grant or operation, disabled or deleted connection, missing bearer secret, foreign company, actor demotion or membership loss), and authority errors from `reserve_remote` before the marker is created.
  - (b) The `ApprovalRequired`, `PossibleDispatchExists` and `Interrupted` observations.
  - (c) A failed read of the effect-state hint.
  - (d) A worker crash or lease loss during the eager open.
- Evidence — what the existing code does with an unmarked failure, step by step:
  1. With no marker, `recovery.rs:44-55` keeps the handler-supplied safety, because `workflow_action_retry_safe` returns NULL.
  2. `report()` (`recovery.rs:98-109`) defaults an interrupted `mcp.call` to EffectOutcomeUnknown.
  3. `RecoveryPolicy::decide` (`domain/workflow/recovery.rs:89-91`) returns Reconcile for any unknown safety.
  4. `settle_run` (`recovery.rs:157-160`) sets the run to waiting/reconciliation.
  5. `action_uncertainty::record` finds no unresolved marker to record.
  6. Result: a park that no reconciliation command can settle; it lasts until the run deadline.
- Evidence — why the HTTP precedent does not help: the accepted HTTP handler (`application/workflow/http_runtime.rs:106-123`) maps every non-committed observation to Terminal/EffectOutcomeUnknown `http.unresolved`, including pre-marker revocation. Its revocation test (`http_tests.rs:114-190`) asserts only that no network call was made. Copying that pattern would contradict D1's "clean step failure" boundary and turn "unavailable or revoked tools" into an unreconcilable wait. When no marker exists, `tools/call` cannot have been sent, so these cases are not unknown effects.
- Required correction:
  - Add an outcome table to D1 covering every handler and direct-tool result: eager authority failure, reservation authority failure before marker, ApprovalRequired, PossibleDispatchExists, Committed with isError, Interrupted, post-entry error, hint/read error, and crash/lease loss before and after the marker. For each: failure code, class, handler-supplied RetrySafety, the effective RecoveryDecision after `workflow_action_retry_safe`, and the direct-tool observation.
  - Either classify pre-marker revocation and unauthorized access as a clean (SafeToRetry) terminal failure, or explicitly justify the wait. Also state the interim outcome for ApprovalRequired until phase 05.
  - Tests for revocation, approval and crash must assert run and attempt state (`state`, `waiting_reason`, attempt `workflow_retry_safety`, no reconciliation fact), not only zero network.
- Instruction lesson: guidance is missing. A gate that introduces handler classifications should tabulate the effective recovery decisions, including the DB action-predicate override. Evidence is observational (D1 compared against `recovery.rs`). The candidate instruction is recorded in R001.
- Resolution history: 2026-10-17 OPEN.
- 2026-10-17 — [04.10-R002](agent-review-notes.md#0410-r002--correction-recheck-0410-d2--changes-required), D2 SHA256 `84d532eec3476569a878bca7188ed914ecde7f8ff9f09e4b462679e55330ded5`: **RESOLVED at decision level.** Item 10 tabulates rows 1–12 (code, class, handler safety, effective decision, direct-tool result). Pre-marker refusals and ApprovalRequired become clean FinalError. A crash before the marker retries through the shared classifier. Every wait keeps a marker that the 04.7 evidence command can settle. The classifier change was reviewed separately in R002 and found sound. Implementation and tests not yet reviewed.

## 04.10-F002 — `mcp.tool_error` safety label contradicts the shared DB recovery decision
- Context: same date, workflow, batch, stage, round [04.10-R001](agent-review-notes.md#0410-r001--architecture-gate-0410-d1--changes-required), reviewer, decision SHA256 and records as 04.10-F001.
- Classification: actionable defect, **low**, correctness/clarity. Originates in the gated decision (D1 item 7).
- Evidence — what D1 says: a valid `isError=true` result commits a receipt and fails the step with `mcp.tool_error` (Terminal, EffectOutcomeUnknown).
- Evidence — what the code actually does, step by step:
  1. Handler failures reach `recovery::retire_on` through `supervise_io` (`supervise.rs:94`) and `lease.rs:326`.
  2. There, `workflow_action_retry_safe` (`20260930150000_workflow_action_receipts.sql:98-115`) returns true when the execution's marker has a receipt.
  3. `recovery.rs:49-55` then overwrites the handler label with SafeToRetry.
  4. A Terminal failure therefore yields FinalError and `route_failure` (the final_error route or a failed run), not Reconcile.
- Evidence — impact: the effective outcome is the correct one. A committed receipt makes `effect_state = committed`, so reconciliation could never settle a park anyway. The decision states the opposite, though, and an implementation or test that asserts `workflow_retry_safety='unknown'` or a reconciliation wait would either fail or encourage bypassing the shared predicate.
- Required correction:
  - State that tool errors take the declared final_error route (or fail the run), with effective SafeToRetry from the shared predicate and no reconciliation fact.
  - State that an operator retry returns the same receipt with zero MCP requests.
  - Keep "a tool error does not prove not-applied" as the reason no new invocation or call is made.
  - Test the route and the attempt state.
- Instruction lesson: same as 04.10-F001 (check stated handler labels against the `retire_on` override).
- Resolution history: 2026-10-17 OPEN.
- 2026-10-17 — [04.10-R002](agent-review-notes.md#0410-r002--correction-recheck-0410-d2--changes-required), D2 SHA256 `84d532eec3476569a878bca7188ed914ecde7f8ff9f09e4b462679e55330ded5`: **RESOLVED at decision level.** Row 9 and I8: handlers always supply Safe, and the predicate is the only source of Unknown. Tool errors take FinalError with `workflow_retry_safety='safe'` and no reconciliation fact; an operator retry returns the same receipt with zero MCP requests.

## 04.10-F003 — MCP race/recovery matrix and affected-suite gate are incomplete
- Context: same date, workflow, batch, stage, round [04.10-R001](agent-review-notes.md#0410-r001--architecture-gate-0410-d1--changes-required), reviewer, decision SHA256 and records as 04.10-F001.
- Classification: actionable defect, **medium**, verification. Originates in the D1 verification plan.
- Evidence — basis: phase 04 Acceptance; EXPANSION 04.10 "explicit phase04 MCP race/recovery matrix"; the AGENTS.md competing-claimant rule; the persistence AGENTS.md composite-FK rule.
- Evidence — cases missing from the matrix:
  1. **Post-marker lazy session (D1 item 6, third bullet):** this new path has no test. Needed: exactly one new `tools/call` after a 04.7 proven-not-applied, and drift or unavailability inside `invoke` → unknown with no call.
  2. **Stale workers:** lease loss during the eager open and during `tools/call` (Acceptance "stale workers"). Only concurrent dispatchers are listed.
  3. **Session expiry at workflow level:** a 404 on `tools/call` → unknown, no re-initialize, no second call. Only an adapter-level test is listed.
  4. **Pre-marker `mcp.unavailable`:** DNS, connect, initialize or capacity failure → no marker, the retirement guards accept SafeToRetry, and the retry re-runs discovery and dispatches once.
  5. **Direct-tool parity** (Acceptance "identical grants, approval, and idempotency"): revocation, the protected operation and isError are listed only for steps.
  6. **Argument change:** a changed argument set for a direct MCP tool → new invocation (Acceptance "argument changes").
  7. **Competing catalog writers:** the new catalog concurrency protocol (registration CAS without a revision bump, same-name last-writer-wins, grant-FK cascade on refresh drop/update/soft delete) has no competing test. Registration racing grant removal, refresh or soft delete must never leave an operation without its grant or with stale discovery.
  8. **Cross-tenant FK:** no transaction-scoped DB test shows that the composite FKs reject a cross-tenant `workflow_mcp_operations` row.
- Evidence — gate scope, missing affected suites (D1 changes functions they call):
  - Binding, admission and publication: `resources.rs:11-86` (`inspect_on`, `check_readiness`) and `authority.rs:41-78` (`supported_publication`).
  - Receipt read-back and reconciliation verification, which call `validate_action_result`: `action_receipts.rs:31,232,289`, `reconciliation/verification.rs:317`, `action_dispatch.rs:202`.
  - The legacy harness MCP runtime and agent-harness tests: `services/mcp_runtime.rs:81-136,271-309` and the persistence `mcp_*` tests.
- Evidence — stack gate: earlier points ran the accepted workflow_action tests at stock 2 MiB per point (EXPANSION, 04.3 gate paragraph). D1 adds new awaits to the workflow handler chain (the eager connect/discover before `dispatch_remote`). Deferring every stock-stack check to 04.11 leaves 04.10's acceptance without the stack signal that AGENTS.md requires to stay in CI.
- Required correction:
  - Add the eight cases above, or justify each omission, with the expected run, attempt, receipt and marker state and the `tools/call` count.
  - Add the affected suites to the 04.10 batch gate, plus a scoped stock-2-MiB run of the workflow_action and new workflow_mcp tests.
  - The full all-target and lease gates may stay at the 04.11 combined gate.
- Instruction lesson: hypothesis — TASK and AGENTS do not say how to derive the "affected suites" for a shared-function change. Candidate: list the suites of the changed function's graft callers in the batch gate. Evidence: the `validate_action_result` callers.
- Resolution history: 2026-10-17 OPEN.
- 2026-10-17 — [04.10-R002](agent-review-notes.md#0410-r002--correction-recheck-0410-d2--changes-required), D2 SHA256 `84d532eec3476569a878bca7188ed914ecde7f8ff9f09e4b462679e55330ded5`: **RESOLVED at decision level.** Matrix items 1–8 now have expected run, attempt, marker, receipt and fact state plus call counts. The batch gate adds the affected caller suites (binding/admission/publication/resources, action dispatch/receipt/reconciliation, recovery/pending/uncertainty, HTTP, credential rotation, persistence MCP, legacy MCP runtime/harness, routes) and `scripts/stack-budget.sh --offline -- workflow_action workflow_mcp mcp`, which the script supports via cargo test filters. Executed evidence is pending implementation.

## 04.10-F004 — Frozen output contract cannot distinguish declared from absent outputSchema (optional)
- Context: same as 04.10-F001; round [04.10-R001](agent-review-notes.md#0410-r001--architecture-gate-0410-d1--changes-required).
- Classification: optional suggestion, low, requirements/correctness; D1 items 2 and 7.
- Evidence:
  - D1 freezes `{"type":"object"}` when no outputSchema is declared, and validates structuredContent only "when present". The registry contract (`registry/actions.rs:95-98`) already makes structuredContent optional.
  - A declared `{"type":"object"}` and an absent schema therefore become indistinguishable. That makes it impossible to enforce the MCP rule that a tool which declares outputSchema must return conforming structuredContent.
  - Consequence: a declared-schema tool that omits structuredContent commits a receipt. It then fails later as a downstream `binding.missing` instead of as an output-validation failure.
- Suggestion: keep an explicit declared flag in the operation snapshot and treat missing structuredContent under a declared schema as an output-validation failure (unknown, per 04.6). Alternatively, document this as an accepted limitation.
- Resolution history: 2026-10-17 OPEN (optional; non-blocking).
- 2026-10-17 — [04.10-R002](agent-review-notes.md#0410-r002--correction-recheck-0410-d2--changes-required), D2 SHA256 `84d532eec3476569a878bca7188ed914ecde7f8ff9f09e4b462679e55330ded5`: author DECLINED. B accepts this as a documented limitation (D2 "Failure modes": downstream refs must supply defaults). Disposition: dismissed as non-blocking; no change required.

## 04.10-F005 — Invariant I3 overstates "without MCP I/O" (optional)
- Context: same as 04.10-F001.
- Classification: optional suggestion, low, wording/verification; D1 invariants.
- Evidence:
  - I3 says "committed receipts return without MCP I/O".
  - D1 item 6 and its failure-modes paragraph accept that a stale `prepared` hint can open a session (initialize plus `tools/list`) before reservation returns Committed.
  - The requirement itself is narrower: "committed results survive restart without a call".
- Suggestion: restate I3 as "without `tools/call`, and without any MCP request when the hint is current". Keep the sequential restart test asserting zero requests.
- Resolution history: 2026-10-17 OPEN (optional).
- 2026-10-17 — [04.10-R002](agent-review-notes.md#0410-r002--correction-recheck-0410-d2--changes-required), D2 SHA256 `84d532eec3476569a878bca7188ed914ecde7f8ff9f09e4b462679e55330ded5`: **RESOLVED.** I3 now reads "without `tools/call`, and without any MCP request when the hint is current", and the sequential restart test still asserts zero requests.

## 04.10-F006 — Local session-capacity exhaustion consumes step attempts (optional)
- Context: same as 04.10-F001.
- Classification: optional suggestion, low, availability/bounds; D1 items 6 and 9.
- Evidence:
  - `connect` try-acquires from 32 global and 4 per-endpoint permits (`adapters/mcp/mod.rs:70-75,278-293`), and these permits are shared with legacy agent runs.
  - The eager open holds its session across reservation and entry and through a call of up to 30 s.
  - Capacity exhaustion maps to Retryable `mcp.unavailable`, which consumes an attempt and retries with deterministic exponential backoff (`domain/workflow/recovery.rs:98-117`).
  - Consequence: more than four concurrent `mcp.call` steps to one endpoint can exhaust `max_retries` through local contention rather than remote failure.
- Suggestion: either document the bound and how it interacts with step retry budgets, or use a short bounded wait for a permit before failing. Cover the behaviour with a test either way. No limit raise is suggested.
- Resolution history: 2026-10-17 OPEN (optional).
- 2026-10-17 — [04.10-R002](agent-review-notes.md#0410-r002--correction-recheck-0410-d2--changes-required), D2 SHA256 `84d532eec3476569a878bca7188ed914ecde7f8ff9f09e4b462679e55330ded5`: ADOPTED as a bounded ≤ 5 s permit wait with no limit raised. The intent is met, but the unspecified acquisition order introduces cross-endpoint/tenant starvation; see 04.10-F009. The original suggestion is closed; follow-up is under F009.

## 04.10-F007 — Operation register/revoke has no operation-level CAS (optional)
- Context: same as 04.10-F001.
- Classification: optional suggestion, low, concurrency/authorization UX; D1 item 2.
- Evidence:
  - Register and revoke compare-and-swap on the connection revision but do not bump it, and same-name registration is last-writer-wins.
  - Two admins, or an admin and a stale UI, holding the same revision can therefore silently replace or revoke each other's approved snapshot.
  - By contrast, HTTP 04.9 bumps the connection revision on register/revoke (`persistence/workflow/http.rs:200-236` via `revised`).
- Suggestion: CAS on the expected current operation snapshot digest (or its absence) for register and revoke, while still not bumping the connection revision that legacy agent runs use.
- Resolution history: 2026-10-17 OPEN (optional).
- 2026-10-17 — [04.10-R002](agent-review-notes.md#0410-r002--correction-recheck-0410-d2--changes-required), D2 SHA256 `84d532eec3476569a878bca7188ed914ecde7f8ff9f09e4b462679e55330ded5`: ADOPTED as an operation `revision` with CAS. The revision restarts after a revoke or cascade delete, which reintroduces the stale-writer hazard through ABA; see 04.10-F010. The original suggestion is closed; follow-up is under F010.

## 04.10-F008 — HTTP handler parks pre-marker failures with nothing to reconcile; align with D2 in this batch
- Context: 2026-10-17, workflow implement-opus-reviewed-2.1, batch 04.10 MCP, correction recheck (architecture gate), round [04.10-R002](agent-review-notes.md#0410-r002--correction-recheck-0410-d2--changes-required). Reviewer B: Opus 5.5 (xhigh requested). Reviewed decision 04.10-D2, SHA256 `84d532eec3476569a878bca7188ed914ecde7f8ff9f09e4b462679e55330ded5`, base `8ce1fd4`. Records: [TASK](TASK.md), [REVIEW](REVIEW.md).
- Origin: later-discovered miss in accepted 04.9 (its R003/R004 PASS). A follow-up is appended to the 04.9-R004 note.
- Classification: actionable defect, **medium**, correctness/requirements (recovery classification). The defect is in the accepted 04.9 implementation; D2's shared classifier makes the inconsistency sharper.
- Evidence — what the handler does: `application/workflow/http_runtime.rs:106-123` maps every non-committed `step()` result to Terminal/EffectOutcomeUnknown `http.unresolved`. That includes the pre-marker cases: `http_step_request` refusals (actor not authorized, operation revoked, policy mismatch — `persistence/workflow/http_runtime.rs:68-138`), `HttpRequest::parse` errors, reservation authority refusals before the marker, and ApprovalRequired.
- Evidence — what happens next, step by step:
  1. With no marker, `recovery.rs:44-55` keeps Unknown.
  2. `RecoveryPolicy::decide` returns Reconcile.
  3. `settle_run` (`recovery.rs:157-160`) sets the run to waiting/reconciliation.
  4. `action_uncertainty::record` (`action_uncertainty.rs:15`) inserts only unresolved remote markers, so no fact is written and the 04.7 evidence command has nothing to settle.
  5. Operator retry needs the label 'safe' (`workflow_control_retry_safe`), so it is refused.
  6. Result: the run waits until its deadline.
- Evidence — why tests did not catch it: `http_tests.rs:114-190` asserts only that no network call was made.
- Evidence — how D2 sharpens it: D2's `marker_owned_effect_kind` now gives `http.request`/`tool.call` a Safe default when no marker exists. A crash or lease loss during `http_step_request` therefore retries, while the same refusal returned by the handler parks permanently. It also contradicts the rule D2 adopts for MCP (I8: Unknown comes only from the shared predicate) and the MCP row-6 treatment of ApprovalRequired.
- Impact: revoked or protected HTTP actions leave runs in unreconcilable reconciliation waits instead of a classified failure. This directly concerns the phase-04 acceptance bullets for revocation and identical approval behaviour.
- Required correction (recommended in this batch, since D2 already changes HTTP recovery semantics and the `workflow_http` suites are in the 04.10 gate):
  - Apply D2's item-10 rule to `HttpActions::execute`: always supply Safe; map pre-marker refusal to a Terminal rejected code; map ApprovalRequired to Terminal approval_required (interim until 05); map PossibleDispatchExists, post-entry and Interrupted to Retryable and let the predicate decide.
  - Add HTTP DB tests asserting run/attempt/fact state for pre-marker revocation, protection, and crash before the marker.
  - If root keeps this out of 04.10, record it as an open phase-04 defect that blocks the 04.11 combined acceptance, and note in D2 that http.request is temporarily inconsistent.
- Instruction lesson: observation — the 04.9 gate and review did not tabulate effective recovery decisions or assert run outcomes for pre-marker failures. This supports R001's candidate instruction: tabulate effective decisions and assert run/attempt state.
- Resolution history: 2026-10-17 OPEN.
- 2026-10-17 — [04.10-R003](agent-review-notes.md#0410-r003--correction-recheck-0410-d3--pass-architecture-gate-f012-carried-to-implementation), D3 SHA256 `f89b0a89afa11ec9b789ce8e25fc3370a9678bbb43ff1e60c7dcb3e050e7b32b`: **RESOLVED at decision level.** The shared `action_step_failure` mapping is used by MCP and `HttpActions::execute`, with the predicate as the only source of Unknown. Pre-marker refusals go to `http.rejected`/`http.approval_required` FinalError; post-marker failures go to Reconcile with a fact. I9 is added, and HTTP DB tests assert run/attempt/marker/receipt/fact state. Residual readiness gap: 04.10-F012. Implementation not yet reviewed.

## 04.10-F009 — Bounded permit wait must not hold a global permit while waiting for an endpoint permit
- Context: same as 04.10-F008; round [04.10-R002](agent-review-notes.md#0410-r002--correction-recheck-0410-d2--changes-required).
- Origin: D2 item 6 (the adopted F006).
- Classification: actionable defect, **low**, concurrency/bounds (fairness).
- Evidence:
  - `HttpMcpClient::connect` acquires the global semaphore first, then the per-endpoint one (`adapters/mcp/mod.rs:70-75`).
  - With try-acquire, a failed endpoint permit dropped the global permit immediately, so one endpoint held at most 4 global permits.
  - D2 adds a ≤ 5 s wait "each for the global and per-endpoint semaphore" without fixing the order. Under the current order, callers waiting for a hot endpoint each hold a global permit for up to 5 s.
  - A single endpoint, possibly shared across companies because the key is the endpoint URL and the client is shared with legacy runs, can then occupy all 32 global permits. Other tenants' MCP calls time out as `mcp.unavailable`. This conflicts with src/application/AGENTS.md: "one tenant's burst must not starve every other tenant".
- Required correction: specify that the per-endpoint permit is acquired first (bounded wait) and the global permit second (bounded wait), so a waiter never holds a global permit. Add an adapter test: a saturated endpoint with queued waiters while another endpoint still connects within its bound.
- Instruction lesson: observation — an adopted optional change altered a concurrency protocol without restating its lock/permit order.
- Resolution history: 2026-10-17 OPEN.
- 2026-10-17 — [04.10-R003](agent-review-notes.md#0410-r003--correction-recheck-0410-d3--pass-architecture-gate-f012-carried-to-implementation), D3 SHA256 `f89b0a89afa11ec9b789ce8e25fc3370a9678bbb43ff1e60c7dcb3e050e7b32b`: **RESOLVED at decision level.** Permits are acquired endpoint-first, then global, under one shared ≤ 5 s deadline. No global permit is held while waiting, waiters fail immediately on shutdown, and a starvation test is added. Implementation watch item: per-endpoint semaphores in the Weak map must also observe shutdown.

## 04.10-F010 — Operation revision CAS is vulnerable to ABA after revoke or cascade delete
- Context: same as 04.10-F008; round [04.10-R002](agent-review-notes.md#0410-r002--correction-recheck-0410-d2--changes-required).
- Origin: D2 item 2 (the adopted F007).
- Classification: actionable defect, **low**, concurrency/authorization integrity.
- Evidence: register takes `expected: Option<i64>` (None means create-only, so a new row starts at revision 1), and revoke deletes the row. Rows are also deleted by the grant-FK cascade (refresh drop, grant removal) and by soft delete. Concrete sequence:
  1. A stale writer reads operation X at revision 1 (snapshot S1).
  2. X is revoked or cascade-deleted.
  3. Another admin re-registers X as S2, which starts again at revision 1.
  4. The stale writer's `Some(1)` replace silently overwrites S2, or its revoke at revision 1 deletes S2.

  This is exactly the hazard F007 meant to remove.
- Required correction: make the operation version unique across delete and recreate. Options: a random version token (UUID) per write; a monotonic per-(company, connection, name) counter kept in a tombstone/sequence row that survives deletion; or a global sequence. Add a competing test (stale revision after revoke and re-register is refused) to F003 matrix item 7.
- Instruction lesson: observation — a CAS revision over deletable rows needs an ABA argument.
- Resolution history: 2026-10-17 OPEN.
- 2026-10-17 — [04.10-R003](agent-review-notes.md#0410-r003--correction-recheck-0410-d3--pass-architecture-gate-f012-carried-to-implementation), D3 SHA256 `f89b0a89afa11ec9b789ce8e25fc3370a9678bbb43ff1e60c7dcb3e050e7b32b`: **RESOLVED at decision level.** Each write gets a random UUID v4 version that is never reissued after revoke, cascade or soft delete. Register (create-only `None`) and revoke both CAS on it, and stale-writer and one-winner tests are added.

## 04.10-F011 — Record the handler-composition invariant behind `marker_owned_effect_kind` (optional)
- Context: same as 04.10-F008; round [04.10-R002](agent-review-notes.md#0410-r002--correction-recheck-0410-d2--changes-required).
- Origin: D2 item 10.
- Classification: optional suggestion, low, architecture/maintainability.
- Evidence:
  - The classifier is truthful only while every `WorkflowHandler` that `supports` `mcp.call`/`http.request`/`tool.call` performs effects solely through ActionService's marker-first protocol. Today the only such production handler is `HttpRuntime` (`adapters/workflow_http/mod.rs:85-87`), plus the planned `McpRuntime`.
  - The `WorkflowHandler` doc (`application/workflow/worker.rs:16-17`) still says effect and replay safety are "the handler's contract", and phase 06/08 will add tool bridges.
  - A future `tool.call` handler with a direct (non-action) effect would turn a crash before any marker into a silent Safe retry rather than a conservative park.
- Suggestion: state the invariant at the classifier and on `WorkflowHandler`; add it to the 06/08 replacement obligations (BRIEF-04.10). Optionally, the composition seam could assert that only ActionService-backed runtimes register those kinds.
- Resolution history: 2026-10-17 OPEN (optional).
- 2026-10-17 — [04.10-R003](agent-review-notes.md#0410-r003--correction-recheck-0410-d3--pass-architecture-gate-f012-carried-to-implementation), D3 SHA256 `f89b0a89afa11ec9b789ce8e25fc3370a9678bbb43ff1e60c7dcb3e050e7b32b`: **ADOPTED** as documentation on the classifier and `WorkflowHandler`, plus a BRIEF-04.10 06/08 obligation. B accepts declining the runtime assertion, since there is no handler registry at composition time.

## 04.10-F012 — Slotted HTTP preparation does not check connection readiness, so revocations retry until exhausted
- Context: 2026-10-17, workflow implement-opus-reviewed-2.1, batch 04.10, correction recheck (architecture gate), round [04.10-R003](agent-review-notes.md#0410-r003--correction-recheck-0410-d3--pass-architecture-gate-f012-carried-to-implementation). Reviewer B: Opus 5.5 (xhigh requested). Reviewed D3 SHA256 `f89b0a89afa11ec9b789ce8e25fc3370a9678bbb43ff1e60c7dcb3e050e7b32b` (amending D2 `84d532ee…`), base `8ce1fd4`. Records: [TASK](TASK.md), [REVIEW](REVIEW.md).
- Origin: D3 C/D (the mapping and HTTP table row 1), combined with existing 04.9 preparation code.
- Classification: actionable defect, **low**, correctness. Not gate-blocking: it is carried to implementation and must be resolved and tested before batch acceptance.
- Evidence — what D3 claims: table row 1 says `http_step_request` refuses a revoked operation *or connection*, giving `http.rejected` Terminal, and C says a reservation refusal is retried once because "the next attempt's preparation re-checks authority and classifies a revocation as rejected".
- Evidence — what the slotted path actually checks: `persistence/workflow/http_runtime.rs:119-134` resolves the slot to an id and calls only `policy_on`. `policy_on` (`http.rs:267-292`) reads `workflow_http_operations` alone; it never checks `connection.enabled` or bearer-secret presence. The unslotted `registered_on` path does check both.
- Evidence — the resulting loop, step by step:
  1. A disabled slotted connection, or a removed bearer secret, passes preparation.
  2. Reservation refuses it via `validate_resource` (`actions/authorization.rs:97-114`, NotFound).
  3. The mapping turns every `dispatch_remote` error into `http.unresolved` Retryable.
  4. The next attempt passes preparation again and is refused again.
  5. This repeats with backoff until attempts are exhausted, then FinalError. It is bounded and causes no wait, but it contradicts row 1.
- Evidence — MCP: the lazy re-reservation path (hint `not_applied`) has the same shape for readiness checks that its preparation does not perform.
- Required correction:
  - Make preparation check the same resource readiness the reservation enforces. For HTTP, have the slotted `http_step_request` call `connection_on` + `status_on` + `validate_resource` inside its existing company-locked transaction. For MCP, keep that check in step preparation for both the eager and lazy paths.
  - Alternatively, correct D3 row 1 and the C text to state "retries until attempts are exhausted".
  - Add DB tests for a disabled slotted HTTP connection and a removed bearer secret, asserting `http.rejected`, the attempt count and run state.
- Instruction lesson: observation — "the next attempt re-checks" claims should be verified against which checks preparation actually performs versus reservation.
- Resolution history: 2026-10-17 OPEN (carried to implementation; blocks batch acceptance, not the gate).

## 04.10-F013 — Name the changed accepted assertion and refine I9 wording (optional)
- Context: same as 04.10-F012; round [04.10-R003](agent-review-notes.md#0410-r003--correction-recheck-0410-d3--pass-architecture-gate-f012-carried-to-implementation).
- Classification: optional suggestion, low, verification/wording.
- Evidence and suggestion:
  - (a) The accepted test `workflow_http_remote_failure_remains_unknown_without_second_entry` (`http_tests.rs:92-111`, assertion at line 100) checks that the handler returns `EffectOutcomeUnknown`. D3 deliberately changes that to Safe. Name the test in D3 and replace the assertion with the effective retirement state (attempt 'unknown', Reconcile, one reconciliation fact, no second entry). This keeps the guarantee instead of just deleting the line.
  - (b) I9 says every reconciliation wait has an unresolved remote marker. The pre-existing evidence-conflict path breaks that wording: `workflow_action_retry_safe` returns false when `workflow_action_has_evidence_conflict` holds (`20260930180000_workflow_action_evidence.sql:189`) even if every marker has a receipt. That wait is settled by 04.7 conflict handling, not by a marker. Suggested wording: "…without an unresolved remote marker or an evidence conflict".
- Resolution history: 2026-10-17 OPEN (optional).


### 04.10-R004 implementation recheck — F012 and earlier decisions
2026-10-17; workflow `implement-astra-reviewed-2.1`; independent Reviewer B (requested gpt-6-astra/medium); base `8ce1fd49b5a961da705fddf0b92cac3182da427b`, artifact `cb1b1dba9f6a803d7e35308917786c2863e89e73e7abddd4d2f722f2927a6210`. See [R004 note](agent-review-notes.md#0410-r004--whole-batch-codeintegration-review).

- **04.10-F012 FIXED.** `src/adapters/persistence/workflow/http_runtime.rs:104–110,167,190` invokes connection/status/validate_resource readiness in both preparations. `src/adapters/persistence/workflow/mcp.rs:152–165` and `mcp_runtime.rs:73,91` enforce corresponding MCP readiness before eager or lazy dispatch. HTTP disabled-connection/removed-secret cases in `http_tests.rs:128–219` assert `http.rejected`, terminal/safe, failed run, zero markers/facts, exactly one attempt. Inspected real-DB `logs/04.10/gate-focused.log` shows the test passing; MCP readiness cases also pass. Closure is specific to this finding, not a declaration that all batch gates passed.
- F001/F002/F008 implementation verified: shared failure mapping plus predicate-owned unknown classification, effective DB outcomes for pre-marker rejection, protection, post-marker ambiguity and committed tool error. F005/F006/F009 implemented: effect hint skips network for current receipts; endpoint-first permits under one deadline, queued waiters retain no global permit, closed semaphores/cancellation reject shutdown waiters. F007/F010 implemented: fresh UUID operation versions, CAS and competing/ABA tests. F011 implemented in classifier/handler documentation and phase06/08 obligations. F003 matrix code and focused execution inspected; final stock/static gate evidence remains pending because the first stock run failed (see R004 note). F004's previously accepted limitation remains documented, not reopened.
- **04.10-F013 adopted in implementation/documentation.** The former HTTP safety-label assertion is replaced with effective retirement state in `http_tests.rs:93–125`; BRIEF-04.10 states the unresolved-marker-or-evidence-conflict invariant. Historical D2/D3 text is preserved, with this disposition recording its wording refinement.


## 04.10-F014 — Required scoped stock gate fails repeatedly; verification incomplete
- Context: 2026-10-17; workflow `implement-astra-reviewed-2.1`; batch04.10 whole-batch implementation/integration review; [04.10-R004](agent-review-notes.md#0410-r004--whole-batch-codeintegration-review). Independent Reviewer B (requested gpt-6-astra/medium). Base `8ce1fd49b5a961da705fddf0b92cac3182da427b`, digest `cb1b1dba9f6a803d7e35308917786c2863e89e73e7abddd4d2f722f2927a6210`, sole exclusion `plan/workflow-foundation/docs`. Records: [TASK](TASK.md), [REVIEW](REVIEW.md).
- Classification: actionable **medium**, verification; blocks batch acceptance. This is an observed failed required gate, not an established production-code defect or assertion that the MCP changes caused it.
- Evidence: D2/D3 require `scripts/stack-budget.sh --offline -- workflow_action workflow_mcp mcp`. The first unchanged-artifact run in [gate-stock.log](logs/04.10/gate-stock.log) ended **390 passed / 3 failed / 0 ignored**: `workflow_action_reconciliation_completion_race_completion_first` failed at `action_reconciliation_completion_race_lock_support.rs:355` with persistence-bound timeout; `workflow_action_reconciliation_bounds_128_prior_consuming_129_total_then_lost_refuses` failed at `action_reconciliation_prior_bounds_support.rs:128`, and `workflow_action_reconciliation_siblings_later_valid_result_resolves_positive_truth` at `action_reconciliation_receipt_tests.rs:206`, both on lease-release commit-outcome-unknown. Their focused stock recheck passed3/0 in [stock-failure-recheck.log](logs/04.10/stock-failure-recheck.log).
- A second full required scoped run, same source, stack bound and default parallelism, in [resume-gate-stock.log](logs/04.10/resume-gate-stock.log), ended **391 passed / 2 failed / 0 ignored**: `workflow_action_dispatch_resolution_stalled_reservation_adopted` at `action_dispatch_resolution_tests.rs:300` expected effect/receipt counts(1,1), observed(0,0); `workflow_action_cancellation_company_before_run_all_callers` at `action_cancellation_order_tests.rs:261` failed its completed-transactions-release assertion. Both scripts report failures unrelated to stack overflow. Root-cause attribution remains unestablished. Subsequent SQLx/build/static gates in the fail-fast script have not completed.
- Required correction/evidence: investigate the failing timing/transaction premises and fixture/resource conditions; correct any demonstrated implementation or test defect while preserving protocol assertions and resource bounds. Retain the failures and diagnosis. Run affected checks and the entire required scoped stock gate successfully on the correction artifact (a focused rerun alone does not substitute), then finish SQLx prepare/check, locked offline all-target compilation, strict Clippy, formatting/diff, transport/regression, credential CLI and graph gates. Freeze a new digest if project files change; otherwise explicitly retain this one. Do not raise limits or weaken/skip assertions simply to clear the gate.
- Instruction lesson: existing repository/skill guidance already requires actual passing gates and warns against erasing bound signals. No new instruction candidate is supported; cause unknown pending investigation.
- Resolution history: **OPEN**. R004 code inspection found no additional concrete defect; the batch remains unaccepted due to this verification issue.


### 04.10-F014 — Resolution, R005
2026-10-17; **RESOLVED**, workflow `implement-astra-reviewed-2.1`, same independent reviewer; base `8ce1fd49b5a961da705fddf0b92cac3182da427b`, corrected digest `2be42e3e5d8b0651ad1d6bc523cf356dd6bb55ac254050b99914bede669ebc4c` (independently matched before/after), sole exclusion `plan/workflow-foundation/docs`. See [R005 note](agent-review-notes.md#0410-r005--f014-correction-and-evidence-recheck).

The correction uses the existing advisory-transaction drain before inspecting an ambiguous reservation's durable rows/removing its abort rule, and bounded fresh-statistics polling for post-result backend idle. It preserves exact lock probes, durable state/provider counts, primary-attempt success discrimination, late-release/resolved-COMMIT negative controls, void ceilings, production bounds and default test parallelism. Two automatic-dereference Clippy corrections are behavior-neutral. Inspected focused stock correction evidence12/0/0 including shared-helper callers and both negative controls.

The complete corrected scoped stock gate now passes393/0/0 at2048KiB, including all five previously failing tests. `corrected-gate-status.log` and underlying logs also show SQLx prepare/check, locked offline all-target check, strict Clippy, fmt/diff, transport boundary/regression, credential CLI regression and graph build passing. Fresh migration/schema equivalence and focused504/0/0 remain applicable. Prior failed logs are retained; no unsupported universal load/flakiness explanation or raised bound is used to close this finding. F003's per-batch executed-gate obligation is therefore also satisfied. Full combined04.11 gates remain outstanding for the overall task, outside this04.10 closure.


## 04.11 stop-checkpoint observations — R001 INCOMPLETE
2026-10-17; independent Reviewer B, workflow `implement-astra-reviewed-2.1`; D1 SHA256 `ebb523dfdfcf5d7b937ddf48b47161bb79b70ad9bc2d890b511f63ef58b8ad83`. Review stopped at user request. These IDs retain preliminary evidence for resumption; **no completed gate verdict or PASS**. No04.11 code exists.

### 04.11-F001 — Provisional medium: new delivery marker/message recovery contract incomplete
D1 introduces a delivery marker, effect wait and direct-tool Pending owner but does not explicitly integrate their effective outcomes with the shared recovery classifier, retry predicate and effect-state projection. `src/application/workflow/actions/failure.rs:18–20` limits marker-owned kinds to http.request/tool.call/mcp.call (its test asserts message.send false); `src/adapters/persistence/workflow/recovery.rs:98–137` and `pending_recovery.rs:107–145` can classify message kinds unknown absent markers. `action_uncertainty.rs:6–18` records unresolved remote markers only. `migrations/20260930180000_workflow_action_evidence.sql:185–209` retry safety permits receipt or supported remote proof, and `20260930182000_workflow_action_actual_observations.sql:102` projects receiptless markers as possible dispatch.
Risk needing architectural resolution: pre-publication failures/crashes may enter reconciliation without any resolvable marker/fact; direct Pending followed by caller loss needs a defined durable owner before phase06 parking exists. Request explicit effective-outcome/owner coverage for preparation/approval, precommit crash and ambiguous commit, parked step, direct Pending, terminal receipts, uncertainty and mixed siblings, with meaningful recovery tests. This is a design gap observation, not a demonstrated04.11 implementation bug. Final severity/disposition pending completed review.
- 2026-10-17 R002 disposition: **confirmed, expanded, medium, OPEN** — see [04.11-F001 R002](#0411-f001--r002-disposition-confirmed-revised-medium).

### 04.11-F002 — Provisional medium: assumed existing action-count bound is not established
D1 bounds whole-execution receipt processing by “existing action-count bounds.” Author reports no preparation-time admission cap: SQL retry-safe128 is a proof ceiling, not a count limit. Reviewer has not completed the final prepare-path read. Require a concrete enforced bound and persistent discoverability of remaining action/event work before gate acceptance. Author's proposed16-action page and new-delivery-only128 dispatch admission/129 probe require review in a revised decision; they are not accepted here.
- 2026-10-17 R002 disposition: **confirmed, medium, OPEN** — see [04.11-F002 R002](#0411-f002--r002-disposition-confirmed-medium).

### 04.11-F003 — Provisional low: subject bound differs from storage
Existing messages.subject SQL limit is2048 bytes (`migrations` initial schema around line2788); `src/application/workflow/registry/schemas.rs:42–47` uses `text(MAX_TEXT)` and registry MAX_TEXT is16_384. D1 retains that content shape. Author independently confirmed mismatch. Align subject schema/runtime with actual byte bound, including Unicode boundaries, before implementation acceptance. This appears a local mandatory correction rather than standalone architecture blocker; final disposition pending review completion.
- 2026-10-17 R002 disposition: **confirmed, revised (binding bound is `CanonicalContent` 998 bytes; destination count added), low, OPEN** — see [04.11-F003 R002](#0411-f003--r002-disposition-confirmed-revised-low).

### 04.11-F004 — Optional clarification/test: sequence cursor synchronization
Independent sibling callbacks may allocate receipt sequence values in a different order from commit. D1 already says consumers lock action/delivery rows and read authoritative state: locking **all required delivery parents before reading/advancing the cursor** can safely drain/observe a lower-sequence writer. No definite defect asserted. Make the invariant explicit and include a competing-sibling test with delayed lower-sequence commit so a high-water cursor cannot strand accepted truth. Pending author/reviewer disposition.
- 2026-10-17 R002 disposition: **confirmed as optional suggestion; no lost-event defect** — see [04.11-F004 R002](#0411-f004--r002-disposition-confirmed-optional).

# 04.11 — completed architecture gate (04.11-R002)
Common context for every entry below: 2026-10-17; workflow `implement-opus-reviewed-2.1`; batch 04.11 messaging, architecture gate; round [04.11-R002](agent-review-notes.md#0411-r002--architecture-gate-0411-d1-completed--changes-required); Reviewer B Opus 5.5 (`claude-opus-5-5`, xhigh requested; effective effort not visible); reviewed decision 04.11-D1 `logs/04.11/decision-D1.md` SHA256 `ebb523dfdfcf5d7b937ddf48b47161bb79b70ad9bc2d890b511f63ef58b8ad83`, base `8ce1fd49b5a961da705fddf0b92cac3182da427b`. Records: [TASK](TASK.md), [REVIEW](REVIEW.md). All defects originate in the gated decision (no implementation exists). Evidence is code/schema inspection; no checks were run. The provisional R001 entries above are preserved; these are their dispositions plus new findings.

## 04.11-F001 — R002 disposition: confirmed, revised (medium)
- Classification: actionable defect, **medium**, architecture/requirements (recovery and reconciliation contract of the new delivery effect kind). Recurrence of the [04.10-F001](#0410-f001--specify-outcomes-for-pre-marker-and-non-committed-mcp-paths) pattern.
- Observed vs required: D1 §2 adds a delivery effect kind and §4 holds uncertain deliveries "until … owning reconciliation support exists, or the run deadline". It never states how message kinds and delivery markers behave in the shared recovery predicates. Phase 04 items 5–6 require an ambiguous invocation to be marked as needing reconciliation and an authorized applied/not-applied path.
- Evidence (current behavior D1 would inherit):
  1. Pre-marker interruption of a message step: `recovery.rs:107-113` defaults every kind not in `marker_owned_effect_kind` (`actions/failure.rs:18-20`, http/tool/mcp only) to EffectOutcomeUnknown. `workflow_action_retry_safe` returns NULL without markers (`20260930180000:185-206`), so the default stands. The decision is Reconcile, and `action_uncertainty::record` inserts only `effect_kind='remote'` markers. Result: an unsettleable park until the run deadline. `pending_recovery.rs:107-145` (`unknown_effect`) does the same for exhausted pending work. D1's atomic marker+publication makes "no marker ⇒ no effect" true for message kinds, but D1 does not decide to add them (the investigation explicitly leaves it open).
  2. A direct-tool delivery marker inside an agent execution (model_tool origin) with no receipt yet: `workflow_action_retry_safe` treats an unreceipted marker as unsafe unless replay-supported or not-applied evidence exists. So any interruption of the enclosing agent step parks it as unknown, even though the delivery engine owns and will report the outcome, and `record` again inserts nothing for a non-remote kind.
  3. `workflow_action_effect_states` (`20260930182000:102-118`) projects a receiptless marker as `possible_dispatch`. Evidence and reconciliation guards (`20260930180000:107-147,212`, `20260930181000:132`, `20261001090000:8`) and uncertainty rows (`20260930170000:9`) accept only `remote`. Delivery `outcome_unknown` therefore has no reconciliation representation or command.
- Required correction (D2):
  - Tabulate, for message step and direct tool, each outcome: preparation/approval refusal, pre-marker crash, ambiguous commit (absent / committed / unresolved), parked step crash, Pending, accepted, failed, `outcome_unknown`, and mixed siblings. For each, give the code, class, handler safety, the effective decision after `workflow_action_retry_safe`, the effect-state projection, and the direct-tool observation.
  - Decide explicitly whether message kinds join `marker_owned_effect_kind`. Decide how `workflow_action_retry_safe`, `workflow_action_effect_states` and `action_uncertainty::record` treat delivery markers.
  - For an `outcome_unknown` delivery, either mark the invocation as needing reconciliation with an authorized evidence path (applied ⇒ accepted, not-applied ⇒ retry of that delivery only), or justify a documented deferral against phase 04 items 5–6.
  - Tests must assert run/attempt/wait state, not only zero sends.
- Instruction lesson: guidance missing. A new effect kind needs an outcome table across shared predicates. This is the same gap as 04.10-F001; the candidate instruction is in R002.
- Resolution history: 2026-10-17 R001 provisional OPEN; 2026-10-17 R002 **confirmed, scope expanded, OPEN**.
- 2026-10-17 [04.11-R003](agent-review-notes.md#0411-r003--correction-recheck-of-0411-d2--changes-required), D2 `1dcd1a9e…3a44`: **RESOLVED at decision level** (outcome table and predicates); the evidence path is split out as [04.11-F010](#0411-f010--message-deliveries-with-unknown-outcome-have-no-authorized-reconciliation-path). See [R003 dispositions](#r003-dispositions-for-0411-f001-to-f008).

## 04.11-F002 — R002 disposition: confirmed (medium)
- Classification: actionable defect, **medium**, architecture/bounds (`src/AGENTS.md` "Bound work at every external boundary").
- Evidence: D1 §4 bounds whole-execution receipt processing by "existing action-count bounds". None exist:
  - `PostgresPersistence::prepare` (`persistence/workflow/actions.rs:16-48`) inserts one intent per (execution, operation_digest) with no count check.
  - The only numeric ceilings are proof scans: `LIMIT 129`/`scanned>128 ⇒ false` in `workflow_action_retry_safe` (`20260930180000:191-194`, `20260930150000:104-107`), and `workflow_action_not_applied_available` (`20261001090000:12-16`). Exceeding them fails closed (unknown) rather than refusing admission.
  - The comment in `action_uncertainty::record` ("per-execution action bounds limit this set") is likewise unsupported.
  - Direct tools let one agent execution create arbitrarily many distinct message invocations.
- Required correction: specify enforced bounds for:
  - message invocations or delivery markers per execution, refused before any marker or publication with a distinct terminal outcome (not unknown);
  - receipt-consumption work per transaction (page size) with persisted progress, so remaining work stays discoverable and poison sets cannot monopolize scanning;
  - their relation to the 128-dispatch proof ceiling.
  Add boundary tests (N and N+1).
- Note on the unreviewed author proposals (16-action pages; 129-row probe refusing admission at 128): they are acceptable in shape only if they meet the conditions above. They are not accepted until they appear in a revised decision.
- Instruction lesson: guidance exists (`src/AGENTS.md` bounds rule); a decision cited a bound without locating its enforcement. Candidate: "cite the file:line that enforces any bound a decision relies on". Evidence: this finding only, so this is a hypothesis about recurrence.
- Resolution history: 2026-10-17 R001 provisional OPEN; 2026-10-17 R002 **confirmed, OPEN**.
- 2026-10-17 R003, D2 `1dcd1a9e…3a44`: **RESOLVED at decision level**. See [R003 dispositions](#r003-dispositions-for-0411-f001-to-f008).

## 04.11-F003 — R002 disposition: confirmed, revised (low)
- Classification: actionable defect, **low**, correctness/bounds (input contract vs canonical owners).
- Evidence:
  - Subject: `content()` (`registry/schemas.rs:39-44`) allows `text(MAX_TEXT=16_384)` (`registry/mod.rs:36`), and JSON Schema `maxLength` counts code points, which is up to ~64 KiB UTF-8. The binding limit is `CanonicalContent::parse`, `MAX_SUBJECT_BYTES=998` (`transport/ingress.rs:36-39,115`), stricter than the SQL `messages_subject_check` of 2048 bytes (`init_schema.sql:2788`) that R001 cited. Body: 16 384 code points fits `MAX_BODY_BYTES`.
  - Destinations: D1 keeps `MAX_SELECTIONS=128` destinations (`registry/actions.rs:8`) for one canonical message with one association per channel. The established association bound is `MAX_THREAD_ASSOCIATIONS=20` (`ingress.rs:55`, enforced in `thread/reply_publication.rs:28,83`), and 128 channel threads plus 128 deliveries widen one transaction's lock footprint.
  - `sanitize_string` normalizes content after the digest. That is deterministic, but should be stated.
  - D1 says "Keep the existing MAX_SELECTIONS/MAX_TEXT bounds", which perpetuates the mismatch. Valid published inputs then fail deterministically at runtime.
- Required correction: align the registry schema with the canonical owner bounds (byte-accurate subject ≤998, or an explicit runtime refusal with a clean pre-marker outcome). Choose and justify a messaging destination bound against association/delivery limits. Test multibyte boundaries.
- Instruction lesson: cause is reuse of generic registry constants; no instruction change supported.
- Resolution history: 2026-10-17 R001 provisional OPEN; 2026-10-17 R002 **confirmed, revised (998-byte owner bound; destination count added), OPEN**.
- 2026-10-17 R003, D2 `1dcd1a9e…3a44`: **RESOLVED at decision level**. See [R003 dispositions](#r003-dispositions-for-0411-f001-to-f008).

## 04.11-F004 — R002 disposition: confirmed (optional)
- Classification: optional suggestion, low, concurrency clarity. Not acceptance-blocking.
- Evidence: D1 §4 stores a per-action "last processed event sequence" while sibling callbacks commit independently under only their delivery parent (`queue.rs:218-279` takes `lock_held_delivery_on` first). A high-water cursor is safe only if:
  - every event is inserted in the same transaction as its part/parent transition, while that parent's row lock is held;
  - the consumer locks all required parents before it reads states and reads the cursor maximum after those locks.
  No lost-event defect is established.
- Suggested: state that invariant. Enumerate every event-producing transition: delivered, dead-letter, retry, cancelled/superseded (including begin_part refusal), and reaper sending→`outcome_unknown` (`queue.rs:566-600`). Map each to aggregation; refusal-cancel means known not-sent ⇒ failed. Use non-conflicting lock modes on the immutable link row (FK KEY SHARE vs the consumer's locks). Add a delayed lower-sequence sibling-commit test.
- Resolution history: 2026-10-17 R001 provisional; 2026-10-17 R002 **confirmed as optional**.
- 2026-10-17 R003, D2 `1dcd1a9e…3a44`: **adopted; RESOLVED at decision level** (premise to be rechecked under F010). See [R003 dispositions](#r003-dispositions-for-0411-f001-to-f008).

## 04.11-F005 — Canonical message classification and per-recipient provider identity are unspecified
- Classification: actionable defect, **medium**, architecture/integration (phase 04 "preserve provider threading"; existing thread consumers).
- Evidence:
  - D1 §§1–2: "One canonical message can have several channel/thread associations and email intents"; user recipients use the run channel/thread.
  - Each email intent gets a distinct Message-ID: `EmailRenderer::message_id_for` hashes the part key `email:<idempotency key>` (`email/egress.rs:207-223,270-276`), and `delivery_key` includes the destination.
  - A canonical message can carry one RFC identity: `email_message_metadata` PRIMARY KEY (message_id) (`init_schema.sql:4012-4016`). `MessageCorrelation::Email` records one `external_messages` key and upserts one external thread (`thread/message.rs:619-682`).
  - Inbound replies resolve threads only through recorded keys (`thread/external.rs:267-295`), so replies to the other recipients' copies would not reach the run thread or canonical message.
  - The existing multi-recipient producer mints one canonical message per target and records its predicted key (`services/outreach_tool.rs:370-446`).
  - D1 also does not state the new messages' author, direction, role, audience or entry kind, although existing consumers depend on them:
    - the task worker's idempotency guard treats any later outbound `conversation` entry (except outreach) as the agent's answer and skips replying (`services/task_worker.rs:990-1010` → `thread/mod.rs:637-668`); the schedule guard does the same (`use_cases/thread/dispatch.rs:416-420`);
    - the reply-header fallback takes the newest RFC Message-ID in the thread regardless of direction or audience (`use_cases/thread/mod.rs:1025-1041` → `views.rs:587-608`).
  - A workflow message to an internal user, placed in a customer email run thread, could therefore suppress the channel agent's reply or become the In-Reply-To of later customer mail.
  - Email threading mode for `message.send` (Standalone/Anchored/Received) is unspecified.
- Required correction (D2):
  - Choose the cardinality: one canonical message per email destination (outreach precedent), or one message with per-delivery `external_messages` rows (the schema allows several keys per message and has `delivery_part_id`), and say what RFC metadata the canonical message carries and how `message.reply` on such a source picks headers and recipient.
  - Specify author/direction/role/audience/entry kind for in-app channel publication, user email and replies.
  - Specify which thread internal-user email joins.
  - Specify the threading mode.
  - Define the intended interplay with the guards and fallback above.
  - Test that a reply to each recipient's copy resolves correctly, that a workflow message in a customer thread neither suppresses the agent's answer nor changes its In-Reply-To unless intended, and that `message.reply` uses the source's headers.
- Instruction lesson: guidance missing. Candidate (R002): message-writing decisions list classification fields, per-delivery provider-key mapping and affected thread consumers. Evidence: this finding.
- Resolution history: 2026-10-17 R002 OPEN.
- 2026-10-17 R003, D2 `1dcd1a9e…3a44`: **RESOLVED at decision level**. See [R003 dispositions](#r003-dispositions-for-0411-f001-to-f008).

## 04.11-F006 — Verification plan omits CI gates and the tests F001–F005 require
- Classification: actionable defect, **low**, verification (AGENTS.md "CI must exercise…", "Implementation plans must specify per-batch focused checks and a final full-suite gate").
- Evidence: the D1 §6 final gate lists fmt/check/clippy, SQLx, migrations, the full real-DB suite, stock 2048 KiB, lease budget, transport/credential and graft. CI (`.github/workflows/ci.yml:51-117`) also runs `git diff --check` and runs the full suite, stack budget and lease budget under `scripts/test-network-isolation.sh`; D1 names neither (TASK acceptance does). Stock coverage should name `scripts/stack-budget.sh`. Focused per-batch checks are listed by topic without commands or filters to record.
- Required correction: name the exact final commands matching CI (including network isolation and diff check) and record the planned focused filters per area. Add the tests required by F001 (state assertions), F002 (N/N+1), F003 (multibyte), F005 (threading and consumer non-interference), F007 (SQL-only negatives) and F008 (effect-wait expiry, no hot re-polling). Note the ~3.5 GiB disk constraint and state how unavailable gates will be reported.
- Instruction lesson: the existing instruction was followed loosely; no change supported.
- Resolution history: 2026-10-17 R002 OPEN.
- 2026-10-17 R003, D2 `1dcd1a9e…3a44`: **RESOLVED at decision level** (lease-budget detail tracked in F011). See [R003 dispositions](#r003-dispositions-for-0411-f001-to-f008).

## 04.11-F007 — Commit-time and receipt proofs are not database-derived
- Classification: actionable defect, **low**, architecture/security-of-invariants (`src/adapters/persistence/AGENTS.md` "Preserve invariants at the database boundary"; 04.3 practice that SQL guards are exercised independently of Rust).
- Evidence:
  - D1 §2: for a message step, the deferred guard accepts "the exact atomically closed attempt and effect wait" with "the captured original expiry … live at COMMIT". The existing guard derives liveness from `job.status='processing' AND job.lock_expires_at>clock_timestamp()` (`20260930120000:61-77`), and `task_attempts` keeps no expiry. Once the same transaction closes the job, the only expiry left is the value the writer stored on `workflow_message_actions`. D1 does not say it is captured from the live job row by the database.
  - D1 §4: the canonical action receipt (accepted|failed) is written by a background consumer, but D1 does not require the database to verify it against the required deliveries' authoritative states (accepted ⇒ all delivered; failed ⇒ a known terminal failure and no unknown/pending).
- Required correction: capture the expiry through an immediate check or trigger while the job is still processing (equal to `lock_expires_at`, insert ordered before the close), then re-check at COMMIT. Add a receipt guard tying the result to the delivery states. Add SQL-only negative tests: forged expiry, wrong message or delivery link, premature accepted receipt.
- Instruction lesson: cause unknown. The existing house pattern covered it, but D1's wording is ambiguous.
- Resolution history: 2026-10-17 R002 OPEN.
- 2026-10-17 R003, D2 `1dcd1a9e…3a44`: **RESOLVED at decision level** (optional tightening in F009). See [R003 dispositions](#r003-dispositions-for-0411-f001-to-f008).

## 04.11-F008 — Effect-wait deadline and resume semantics unspecified
- Classification: actionable defect, **low**, integration/liveness (`src/application/AGENTS.md` poison-batch rule).
- Evidence:
  - D1 §2 creates `workflow_waits(reason=effect)` without stating its deadline or how generic paths treat it.
  - `resume_on` (`wait_commit.rs:39-54`) expires only `reason='event'` waits by their own deadline. Any other non-timer reason goes to the event lookup by `event_name`/`correlation` (NULL for effect waits) and returns Pending.
  - `CANDIDATES_SQL` (`polling.rs:105-111`) rediscovers a waiting row whenever `wait.deadline<=now`.
  - So an effect wait whose deadline is earlier than the run deadline would be re-polled every scan without progress. D1 §4 only implies the run deadline ("or the run deadline ends it").
- Required correction: fix the effect-wait deadline to the run deadline, or define effect-specific expiry in `resume_on`. Define what control/expiry does to pending and uncertain deliveries (D1 §5 covers cancellation). Add a test that an effect wait is neither hot re-polled nor resumed by the generic path.
- Instruction lesson: no change supported.
- Resolution history: 2026-10-17 R002 OPEN.
- 2026-10-17 R003, D2 `1dcd1a9e…3a44`: **RESOLVED at decision level**. See [R003 dispositions](#r003-dispositions-for-0411-f001-to-f008).

# 04.11 — correction recheck of D2 (04.11-R003)
Common context: 2026-10-17; workflow `implement-opus-reviewed-2.1`; batch 04.11 messaging; architecture-gate correction recheck; round [04.11-R003](agent-review-notes.md#0411-r003--correction-recheck-of-0411-d2--changes-required); Reviewer B Opus 5.5 (`claude-opus-5-5`, xhigh requested). Reviewed decision 04.11-D2 `logs/04.11/decision-D2.md`, SHA256 `1dcd1a9e99e57cd32e15bf095490961722f185153c66945cd82d29bd4d943a44`, base `8ce1fd49b5a961da705fddf0b92cac3182da427b`. Everything below is from inspection only; no checks were run.

## R003 dispositions for 04.11-F001 to F008
- **04.11-F001 — RESOLVED at decision level, except the evidence path, which moves to F010.**
  - D2 §5.1 tabulates 14 outcomes for step and direct tool, and §5.2 decides every shared predicate:
    - message kinds become marker-owned, which is valid because the marker is the publication transaction's first write and provider I/O happens only after commit;
    - `retry_safe` is unchanged and correct for `message` markers (replay and not-applied checks are remote-only: `20260930150000:51`, `20261001090000:8`);
    - `workflow_action_effect_states` gains a `needs_reconciliation` branch;
    - `record()` and the reconciliation CHECK include `message`.
  - Rows 5, 6, 7, 10 and 14 were cross-checked against `recovery.rs:21-137`, `pending_recovery.rs:46-145`, `control_retry.rs:3-42` (which reopens only the same execution's failed job, so it replays the same invocation and never mints a new one) and `CANDIDATES_SQL`.
  - Implementation and tests not yet reviewed.
- **04.11-F002 — RESOLVED at decision level.**
  - D2 §2.2 bounds admission at 128 markers per execution:
    - the probe runs under `authorize_on`'s attempt `FOR UPDATE` lock;
    - refusal is the distinct Terminal `message.action_limit`, written before any marker;
    - there is an SQL backstop, and the limit relates explicitly to the 128 proof ceiling.
  - D2 §4.2 bounds consumption:
    - one action per transaction, at most 20 parents, `LIMIT 41` events;
    - pages of 8;
    - persisted `consumed_through`, backoff, and `stalled` at 5 failures.
  - One optional backstop tightening is in F009.
- **04.11-F003 — RESOLVED at decision level.**
  - Subject: 998 bytes enforced by `CanonicalContent::parse` before the marker, with a coarse code-point schema bound and a byte check on literals at publication.
  - `MAX_MESSAGE_DESTINATIONS = 20`, justified against `MAX_THREAD_ASSOCIATIONS`.
  - The sanitization order is correct: `sanitize_string` only strips characters (`unicode_sanitization.rs:128-156`).
- **04.11-F004 (optional) — adopted; RESOLVED at decision level.**
  - Events come from an AFTER UPDATE OF status trigger while the parent row lock is held. The consumer takes run, then action `FOR NO KEY UPDATE`, then parents `FOR SHARE` in id order, and reads in a new statement, so the high-water invariant holds.
  - The producer list was checked against every `UPDATE message_deliveries` site; none redrives dead_letter.
  - Caveat: the "terminal statuses have no exit" premise changes if F010 adds an evidence path.
- **04.11-F005 — RESOLVED at decision level.**
  - One canonical message, thread and delivery per destination, matching the `email_message_metadata` PK and the outreach precedent.
  - Classification table given. `message_deliveries_message_audience_check` (`init_schema.sql:2706`) blocks external delivery of `internal_only` messages.
  - `message.send` never writes into an existing thread. The external reply counting as the answer is explicit and intended.
  - The tests cover each consumer.
- **04.11-F006 — RESOLVED at decision level.**
  - §8.3 matches the CI order (`ci.yml:47-117`), including `git diff --check`, the network-isolation wrapper, stack budget and lease budget.
  - The focused filters are concrete; `stack-budget.sh:30` passes them through.
  - The disk plan is present, and the unavailable-gate reporting rule is stated.
  - The lease-budget measurement gap is tracked separately in F011.
- **04.11-F007 — RESOLVED at decision level.**
  - A BEFORE INSERT trigger captures `fence_expires_at` from the live job under the `authorize_on` locks, and the ordering is enforced.
  - The deferred guard has a `message` branch.
  - The receipt guard recomputes the exact jsonb from terminal delivery states. This is stable because no exit path exists, and receipt triggers are not fence-bound (`20260930180000:229-256`).
  - The action-state guard is stated, and SQL-only negatives are listed.
  - Optional tightening in F009.
- **04.11-F008 — RESOLVED at decision level.**
  - The effect-wait deadline equals the run deadline, enforced by an insert trigger.
  - Discovery is gated on the receipt, so there is no hot re-poll.
  - `resume_on` gets an explicit effect branch.
  - Cancel and expiry supersede unsent deliveries and keep in-flight ones.

## 04.11-F009 — SQL backstops: admission count race and step-close identity (optional)
- Classification: optional suggestion, **low**, concurrency/defense-in-depth. Not acceptance-blocking.
- Evidence:
  - The D2 §2.2 BEFORE INSERT admission trigger counts `workflow_action_dispatches` for the execution without taking a lock. The Rust path is race-free because `authorize_on` holds the attempt `FOR UPDATE`. Two SQL-only writers under the same fence could still each count 127 and commit 129, because the deferred dispatch guard locks run/execution/job/attempt only at COMMIT (`20260930120000:56-64`), after both counts.
  - In D2 §2.5 the step-origin guard requires the fenced job and attempt to be "no longer processing", while `confirm_suspension` requires the `EffectParked` close. The guard would accept any close reason.
- Suggested: have the admission trigger lock the execution or attempt row before counting, and add a concurrent SQL-only test. Have the deferred guard require the attempt's `EffectParked` close marker.
- Resolution history: 2026-10-17 R003 OPEN (optional).
- 2026-10-17 [04.11-R004](agent-review-notes.md#0411-r004--correction-recheck-of-0411-d3-delta-over-d2--changes-required), D3 `c462e4eb…8a89`: **adopted; RESOLVED at decision level**. See [R004 dispositions](#r004-dispositions-for-0411-f009-f010-f011).

## 04.11-F010 — Message deliveries with unknown outcome have no authorized reconciliation path
- Classification: actionable defect, **medium**, requirements/acceptance (phase 04 "Execution and uncertainty" item 6; Acceptance "Tests cover uncertain writes, reconciliation…"; EXPANSION 04.11 "combined phase04 acceptance matrix including all provider criteria"). It comes from R002 F001's deferral option (see the R002 follow-up).
- Evidence:
  - D2 §5.2/§5.3 has the 04.7 command refuse every message invocation with `reconciliation.unsupported_effect`, and keeps the evidence, coverage and consumption guards remote-only. An `outcome_unknown` delivery is held until the run deadline, and §5.1 row 11 fails the run there.
  - Item 5 (mark and park) is met.
  - Item 6 requires authorized commands that record applied or not-applied, with retry only after proven not-applied. Messaging also requires "retry only unresolved safely retryable work". D2 makes both impossible for the email provider even when proof exists.
  - The accepted precedent does not need a production verifier:
    - 04.7 built the command around a host-installed `ActionEvidenceVerifier` (BRIEF-04.7:70-74, 413-422);
    - 04.10 proved not-applied retry through the real reconciliation service with a test-host verifier (BRIEF-04.10:60-61);
    - MCP likewise has no production verifier (BRIEF-04.10:28).
  - D2's rationale ("no email ledger/verifier exists") therefore does not separate email from MCP.
  - The delivery layer already anticipates this: the email part digest is "what a reconciliation lookup can compare" (`email/egress.rs:393-395`; `transport/delivery.rs:61,373`).
- Required correction (D3 or an amended D2): add a per-delivery, verifier-attested reconciliation path for message invocations through the existing authorized 04.7 service and verifier port.
  - The subjects are the invocation's `outcome_unknown` destination deliveries (at most 20), identified by Message-ID, recipient and part digest.
  - Applied → that delivery is recorded as delivered by evidence.
  - Final not-applied → only that delivery becomes retryable. It needs the 04.7 rule that no old request can still apply. Siblings stay untouched.
  - On a terminal or cancelled run, not-applied supersedes the delivery instead of retrying it, and late truth is recorded without advancing.
  - Unknown → unchanged.
  - Lock order: run, then action, then parent. New events per §4.1, and the action-state guard extended (`uncertain` → `pending`/`accepted`/`failed`). Restate the F004 "no exit" premise, the event bound and §5.1 rows 11–13.
  - Tests: applied, final not-applied with a sibling never resent, unknown, refusal without a verifier, and authorization, all using a test-host verifier. No production verifier is claimed.
  - Alternative: only an explicit user scope reduction (see the R003 note for the exact choice). Root cannot waive it.
- Instruction lesson: see the R003 process feedback. The reviewer's earlier remedy offered a deferral without checking precedent.
- Resolution history: 2026-10-17 R003 OPEN.
- 2026-10-17 R004, D3 `c462e4eb…8a89`: **RESOLVED at decision level**; remaining integration gaps in [F012](#0411-f012--effect-wait-resume-bypasses-the-finality-breach-continuation-veto) and [F013](#0411-f013--message-evidence-shape-and-projection-gaps-against-the-047-schema). See [R004 dispositions](#r004-dispositions-for-0411-f009-f010-f011).

## 04.11-F011 — Lease-budget measurement of new message operations undefined
- Classification: actionable defect, **low**, verification (AGENTS.md raised-limit and CI-signal rule; D2 §7 and §8.3 step 12).
- Evidence:
  - `scripts/lease-budget.sh` gates each op's work phase against the recorded `attempt_bound_ms`. That phase is "the statements after the operation's leading locks".
  - D2 adds `message.publish` and `message.consume` to `REQUIRED_OPS` and `workflow_message_` to the filters, raising no threshold. That direction is sound.
  - But D2 does not say what client bound the background consumer transaction runs under, or which locks count as leading for each op. The publication transaction inherits the `authorize_on`/WriteBound bound; the consumer is not lease-fenced.
  - If the consumer's parent locks fall in the work phase, D2's own deliberate-delay tests (paused producer, F004) would inflate the work time. The gate would be red by construction, which pushes toward raising `LEASE_BODY_BUDGET_PCT`.
- Required correction:
  - State the consumer's bound (for example the shared `LeasePolicy::attempt_bound`-derived bound, or an explicit constant recorded by the phase recorder).
  - Define the leading locks: for consume, run, action and parents; for publish, the `authorize_on` locks.
  - Mark deliberate-delay or contention tests as `injected`, or exclude them from the filter.
- Resolution history: 2026-10-17 R003 OPEN.
- 2026-10-17 R004, D3 `c462e4eb…8a89`: **RESOLVED at decision level**. See [R004 dispositions](#r004-dispositions-for-0411-f009-f010-f011).

# 04.11 — correction recheck of D3 (04.11-R004)
Common context: 2026-10-17; workflow `implement-opus-reviewed-2.1`; batch 04.11 messaging; architecture-gate correction recheck; round [04.11-R004](agent-review-notes.md#0411-r004--correction-recheck-of-0411-d3-delta-over-d2--changes-required); Reviewer B Opus 5.5 (`claude-opus-5-5`, xhigh requested). The effective decision reviewed is D2 `1dcd1a9e99e57cd32e15bf095490961722f185153c66945cd82d29bd4d943a44` plus D3 `logs/04.11/decision-D3.md` SHA256 `c462e4eb56cb76ac816fa107b7b01f7651706796b8723fed8f206c6315858a89`, base `8ce1fd49b5a961da705fddf0b92cac3182da427b`. Inspection only.

## R004 dispositions for 04.11-F009, F010, F011
- **04.11-F009 (optional) — adopted; RESOLVED at decision level.**
  - D3 §A: the admission trigger locks the fenced attempt row before the bounded count. SQL-only writers under one fence are serialized, and only one live attempt exists per execution.
  - D3 §B: the step-origin guard requires the `EffectParked` close cause, matching `confirm_suspension`.
  - A concurrent SQL-only test is planned.
- **04.11-F010 — RESOLVED at decision level.**
  - D3 §H routes message invocations through the existing `ActionReconciliationService`. It reuses the same authority, command idempotency, the snapshot → verify → settle flow and the `ActionEvidenceVerifier` port, with one `outcome_unknown` delivery as the subject.
  - The verifier contract is right:
    - Applied must match the exact Message-ID, recipient and part digest.
    - FinalNotApplied needs quiescence of every request ever made.
  - Effects:
    - Applied → `delivered`.
    - Final not-applied → only that delivery is reopened, through a one-use reopen, and only on a live run with attempts left; otherwise it is superseded or dead-lettered.
    - Unknown → no change.
  - Triggers forbid any other exit from `outcome_unknown` or `delivered`/`dead_letter`. An expired proof stops the call at `begin_part`.
  - Tests use a test-host ledger verifier (the BRIEF-04.10:60 precedent), and no production verifier is claimed.
  - The F004 premise, rows 11–13 and rows 15–19, 21 are consistent with the code.
  - Remaining integration gaps: F012 and F013.
- **04.11-F011 — RESOLVED at decision level.**
  - The consumer runs under `WriteBound::start("message.consume", policy)` with the worker's `LeasePolicy`: `attempt_bound = min(lease/3, 2 s)` (`lease.rs:48-50`).
  - Leading locks are defined for both operations, with `locks_held()` called after the last of them.
  - Deliberate-delay tests use `recorder::Injected::mark()` on current-thread runtimes (`persistence_bound.rs:518-536`) and the `workflow_message_race_` prefix, outside the narrowed filters.
  - `REQUIRED_OPS` makes missing measurements fail. No threshold changes.

## 04.11-F012 — Effect-wait resume bypasses the finality-breach continuation veto
- Classification: actionable defect, **low**, architecture/contract consistency (accepted 04.7 invariant). It originates in the gated decision (D2 §4.4 combined with D3 §G row 20 and §H.4).
- Evidence:
  - The accepted 04.7 schema states that "a finality breach is immutable and permanently vetoes automatic continuation even if a usable receipt now exists" (`20260930180000:240-241`).
  - It is enforced on the shared completion path: `complete_io` checks `workflow_action_has_evidence_conflict` and fails the step with Terminal `action.evidence_conflict` (`completion.rs:106-125`). Dispatch paths check it too (`action_dispatch.rs:198,409,505,549`).
  - D2 §4.4 resumes a message step through `wait_commit::advance` → `batch_commit::complete(…, Parked)`, or through `route_failure`. Neither `wait_commit.rs`, `batch_commit.rs` nor `completion_job.rs` checks the conflict predicate.
  - D3 row 20 records a breach (delivered truth plus a conflict fact) and vetoes only further reopens and `retry_safe`. Once every destination is accepted, the consumer derives an `accepted` receipt and the effect wait advances the step automatically, which contradicts the invariant.
- Required correction:
  - The `resume_on` effect branch (accepted and failed) checks `workflow_action_has_evidence_conflict` for the execution under the run lock.
  - On a conflict it completes the wait and routes the step through the shared failure writer with Terminal `action.evidence_conflict` (EffectOutcomeUnknown, as `completion.rs` does) instead of advancing.
  - Receipt truth is still recorded.
  - Add a test: breach, then all destinations accepted, then no successor and the step fails with `action.evidence_conflict`.
- Instruction lesson: see the R004 candidate (state the accepted invariants of the extended subsystem for the new kind).
- Resolution history: 2026-10-17 R004 OPEN.
- 2026-10-17 [04.11-R005](agent-review-notes.md#0411-r005--correction-recheck-of-0411-d4-delta-over-d2d3--pass), D4 `6bf5efcb…a276`: **RESOLVED at decision level**. See [R005 dispositions](#r005-dispositions-for-0411-f012-f013).

## 04.11-F013 — Message evidence shape and projection gaps against the 04.7 schema
- Classification: actionable defect, **low**, architecture/schema consistency. It originates in D3 §C, §G and §H.
- Evidence and required corrections:
  1. **Applied attribution.** `workflow_action_applied_request_guard` rejects applied evidence without `applied_request`, and `applied_request` must be `remote_entry`, `marker_reservation` or `unattributed` (`20260930181000:1-22`). The `unattributed` value also feeds the `unattributed_applied` conflict logic (`20260930181000:83-108`). D3 §C adds delivery subject columns but does not say which value message applied evidence carries.
     - Fix: define a value, e.g. a new `delivery` value required with the subject columns, and exclude it from the remote `unattributed` handling.
  2. **Reopen DB guard parity.** D3 §H.4 requires "no evidence conflict for the execution" in Rust, but the §C reopen BEFORE INSERT guard does not list it. The remote consumption guard enforces it in SQL, through `workflow_action_not_applied_available` (`20261001090000:9`).
     - Fix: add `NOT workflow_action_has_evidence_conflict` to the reopen guard (F007 spirit), plus an SQL-only negative test.
  3. **Effect-state precedence.** In `workflow_action_effect_states`, a reconciliation row or any evidence row yields `needs_reconciliation` before `possible_dispatch` (`20260930182000:102-118`). After a row-16 reopen, the immutable first-uncertainty reconciliation row and the final-not-applied evidence therefore still project `needs_reconciliation`, while D3 row 16 says `possible_dispatch`.
     - Fix: for `message` markers, derive the projection from action state (pending → `possible_dispatch`; uncertain/stalled → `needs_reconciliation`; receipt → `committed`), ahead of the generic evidence branches, or correct the table. The only non-test reader is `mcp.rs:342`, but the planned per-row tests assert this column.
  4. **Conflict orientation.** The existing finality conflicts store `evidence_id` = the breached final proof and `contradictory_evidence_id` = the applied evidence (`20260930181000:90-93`). D3 row 20 reverses them.
     - Fix: keep the existing orientation, with reason `finality_breach`.
- Instruction lesson: same as F012.
- Resolution history: 2026-10-17 R004 OPEN.
- 2026-10-17 [04.11-R005](agent-review-notes.md#0411-r005--correction-recheck-of-0411-d4-delta-over-d2d3--pass), D4 `6bf5efcb…a276`: **RESOLVED at decision level**. See [R005 dispositions](#r005-dispositions-for-0411-f012-f013).

# 04.11 — correction recheck of D4 (04.11-R005)
Common context: 2026-10-17; workflow `implement-opus-reviewed-2.1`; batch 04.11 messaging; architecture-gate correction recheck; round [04.11-R005](agent-review-notes.md#0411-r005--correction-recheck-of-0411-d4-delta-over-d2d3--pass); Reviewer B Opus 5.5 (`claude-opus-5-5`, xhigh requested). The effective decision reviewed is D2 `1dcd1a9e99e57cd32e15bf095490961722f185153c66945cd82d29bd4d943a44` + D3 `c462e4eb56cb76ac816fa107b7b01f7651706796b8723fed8f206c6315858a89` + D4 `logs/04.11/decision-D4.md` SHA256 `6bf5efcb3de54949df516b45725c8c9d4fe8cc1ff968a38671a367862e1fa276`. Inspection only.

## R005 dispositions for 04.11-F012, F013
- **04.11-F012 — RESOLVED at decision level.**
  - D4 §M:
    - Discovery wakes the effect wait on a receipt or an execution conflict.
    - `resume_on` checks the conflict (`action_uncertainty::conflicted_on`) under the run lock before reading the receipt. Settlement writes conflicts under run `FOR UPDATE`, so the two are serialized.
    - On a conflict, `park_conflicted_effect_on` writes, in one transaction: wait `failed/evidence_conflict` (CHECK and `WaitFailureReason` extended), job `failed`, run `waiting/reconciliation` through the unconditional `settle_run` (`recovery.rs:156-172`), an uncertainty row `action.evidence_conflict`, and an audit event. It also supersedes unsent deliveries of the invocation.
    - There is no route, successor or output, and the receipt is retained.
  - Because `retry_safe` is false under a conflict (`20260930180000:189`), both `reopen_safe` and `workflow_control_retry_safe` are false (`20261001100000:281-330`), so the park cannot be reopened. This matches the remote behavior and the accepted invariant.
  - Tests are in §S.
- **04.11-F013 — RESOLVED at decision level, all four items.**
  - (1) `'delivery'` is added to the `applied_request` CHECK and the shape CHECK, with the subject required. The commit guard enforces message applied ⇔ `'delivery'`, and remote evidence can never use `'delivery'`. The remote finality-conflict trigger cannot see message evidence (receipt FKs are scoped to the invocation, and message receipts carry no remote/evidence links).
  - (2) The reopen guard adds `NOT workflow_action_has_evidence_conflict` after the run lock.
  - (3) The message effect-state branch follows the receipt branch, ahead of the generic branches. Row 16 now correctly projects `possible_dispatch`.
  - (4) Existing conflict orientation (`evidence_id` = breached proof); the CHECKs at `20260930180000:102` and `20260930182000:20-26` are satisfied.
  - Broadened check on D4's renamed outcome: the `DeliveryReopened` outcome with a NULL `scheduled_job_id` is necessary as well as consistent. D3's `Scheduled{receipt_only:false}` would have violated the command CHECK (`20260930180000:71-72`, `scheduled_job_id` only for `scheduled` from `waiting/reconciliation`) and `workflow_action_bind_schedule` (`20261001100000:69-80`).

## 04.11-F014 — Clarify the NULL shape alternative and conflict refusal for later message entries (optional)
- Classification: optional suggestion, **low**, schema precision and parity. Not acceptance-blocking.
- Evidence and suggestions:
  1. D4 §N says "every existing remote alternative additionally requires `delivery_id IS NULL`". The existing `workflow_action_applied_request_shape` first alternative, `(applied_request IS NULL AND applied_remote_entry_id IS NULL)` (`20260930181000:6-9`), also admits **message** final-not-applied and unknown evidence, which has a delivery subject and no `applied_request`.
     - Suggested: state that this alternative stays subject-neutral, and that the effect-kind ⇔ subject rule lives in the commit guard (D3 §C), so the core reopen path is not rejected by a literal reading. The D3 §K final-not-applied tests would catch a misreading.
  2. Remote dispatch refuses new work in a conflicted execution (`action_dispatch.rs:198,409`). D4 §M stops new provider entry only for the conflicted invocation's unsent deliveries. Neither message publication (D2 §2.2) nor workflow-linked `begin_part` (D2 §6, D3 §I) checks for an execution conflict.
     - This matters only for `model_tool` executions with several message invocations, which are unreachable in phase 04.
     - Suggested: add `workflow_action_has_evidence_conflict` refusal to the publication transaction and to the workflow `begin_part` recheck, with a test, for parity with remote dispatch.
- Resolution history: 2026-10-17 R005 OPEN (optional).

# 04.11 — whole-batch and final integration review (04.11-R006)
Common context: 2026-10-18; workflow `implement-opus-reviewed-2.1`; batch 04.11 messaging plus the combined phase-04 gate; whole-batch code/criteria/integration review; round [04.11-R006](agent-review-notes.md#0411-r006--whole-batch-code-criteria-and-final-phase-04-integration-review--changes-required); Reviewer B Opus 5.5 (`claude-opus-5-5`, xhigh requested). Reviewed snapshot: base `5c1de32ecb3c1a7e59e1a60deb74ab5c42e755df`, digest `8891d74001e78bac6b65e0238c3d73ac25b92f3970178edb08f296417c75cabd`; effective decision D2 `1dcd1a9e…3a44` + D3 `c462e4eb…8a89` + D4 `6bf5efcb…a276`. These are implementation findings against an accepted decision. Inspection only.

## 04.11-F015 — Unlisted departures from the decided outcome table and reply relay
- Classification: actionable defect, **low**, correctness/decision conformance. It originates in the implementation, not in the gated decision.
- Evidence:
  1. **Row 2 classification.** D2 §5.1 row 2 decides that a routing or authority refusal in the publication transaction (actor demoted, capability revoked, recipient no longer a member, …) is Terminal `message.rejected` → FinalError, with no marker.
     - In the code, authority is checked by `MessageDispatch::observe` and `publish` → `authorize_on` (`message_publish.rs:288-294, 388-390`), and the route recheck runs inside `publish_on` (`message_publish.rs:70-119,307`). Their errors return as `StepError::Dispatch` (`message_runtime.rs:131-151`).
     - `action_step_failure` maps every `Dispatch(_)` to Retryable `message.unresolved` (`actions/failure.rs:67-70`). So a demoted actor or a revoked capability is retried until the attempt budget runs out, instead of terminating.
     - It is safe: no marker exists, because the publication is one rolled-back transaction, and the retry cannot send. But it departs from the gated table and is missing from the 18 listed deviations.
     - No test drives a step-level revocation before publication. Revocation is tested only at `begin_part` (`message_delivery_tests.rs:81-97`). Phase 04 Acceptance requires revocation coverage.
  2. **Reply relay.** D2 §1.3 decides that an external `message.reply` "continues the source's recorded relay trace, as an agent reply does". `plan_email` gives every email, replies included, a fresh hop-0 trace from the sending channel (`message_runtime.rs:236-242`). Deviation 2 states the precedent but not that replies are included. The impact is limited (loop control for inter-channel sources), but it changes a decided behaviour.
- Required correction: either
  - conform: map pre-marker `observe`/`publish` refusals (NotFound/BadRequest, transaction rolled back, no marker) to Terminal `message.rejected`; continue the source's relay trace for replies; add a step-level revocation test asserting the Terminal outcome with zero markers and messages;
  - or record both as reviewed decision amendments with justification, and test the chosen behaviour.
- Instruction lesson: see the R006 candidate (diff the decision's outcome table and required tests before freezing).
- Resolution history: 2026-10-18 R006 OPEN.
- 2026-10-18 [04.11-R007](agent-review-notes.md#0411-r007--correction-recheck-of-the-0411-batch-final-0411--combined-phase-04-integration--pass), digest `e76ef46c…c2cf`: **RESOLVED**. See [R007 dispositions](#r007-dispositions-for-0411-f015-to-f018).

## 04.11-F016 — Tests required by the accepted decision are missing
- Classification: actionable defect, **medium**, verification. Acceptance-blocking under root's rule that a required test cannot be waived as a known gap. Also AGENTS.md "When adding or changing a concurrency protocol, include a test with competing claimants".
- Evidence: the TASK "Known gaps" list against the test inventory (R006 note). Missing tests the decision named explicitly:
  1. **D4 §S F012.**
     - "a breach (row 20), then every destination accepted, then the consumer writes the accepted receipt and the poll resumes" → no successor, wait `failed/evidence_conflict`, job `failed`, run `waiting/reconciliation`, uncertainty row, receipt retained;
     - the same with a failed receipt.
     - `workflow_message_reconcile_finality_breach_vetoes_resume_and_reopen` covers only the conflict-while-pending path; the failed receipt there is consumed after the park.
  2. **D2 §8.2 F005.** "a reply quoting each recipient's copy resolves to that recipient's message and thread" (the inbound path is unchanged, but the per-recipient `MessageCorrelation::Email` mapping is new), and "… Slack sources refused".
  3. **D2 §8.2 F007.** "a delayed COMMIT past the captured expiry, and past the run deadline, rolls back everything". The guards exist (migration `:385-399`) but are untested.
  4. **D3 §K F010 SQL-only negatives.**
     - "a reopen from expired, non-final or foreign evidence": only double use and conflict are tested (`message_reconcile_tests.rs:406, 794`).
     - "non-admin, cross-company and wrong-association actors are refused": wrong-association is missing.
  5. **D3 §K competing races.** "competing settle, consumer and begin_part, and settle racing the workflow cancellation, all marked Injected". None exists. These are the new concurrency protocol's competing-claimant tests.
  6. **D4 §S §L seam.** "A `MessageDelivery` reconciliation (`DeliveryReopened`) for an unknown delivery of such an execution reopens only the delivery, never the job". `workflow_message_recovery_model_tool_marker_never_reopens_its_job` covers only the receipt path.
- Not required, or acceptable as recorded:
  - step `message.approval_required`: unreachable, because `context().approval_required` is always false (`http_runtime.rs:62-74`) and the messaging current policy never requires approval;
  - the explicit lock-count assertion: structurally bounded by `destination_count BETWEEN 1 AND 20` and ordinal 0–19; record that, or add a schema assertion;
  - remote evidence with a delivery subject: impossible by the composite FK to message destinations.
- Required correction: implement items 1–6, using `Injected`/`workflow_message_race_` for item 5. Rerun the affected focused areas, then the full suite, stack budget and lease budget (`message.*` operations stay required).
- Instruction lesson: R006 candidate (pre-freeze diff of decision-required tests against the inventory).
- Resolution history: 2026-10-18 R006 OPEN.
- 2026-10-18 [04.11-R007](agent-review-notes.md#0411-r007--correction-recheck-of-the-0411-batch-final-0411--combined-phase-04-integration--pass), digest `e76ef46c…c2cf`: **RESOLVED**. See [R007 dispositions](#r007-dispositions-for-0411-f015-to-f018).

## 04.11-F017 — Expired-proof path commits on lease loss, and inspection is not wired (optional)
- Classification: optional suggestion, **low**, robustness/diagnostics. Not blocking.
- Evidence:
  - `expire_reopened_part_on` (`delivery/queue.rs:701-746`) returns `Ok(LeaseLost)` after the part has already moved to `outcome_unknown`, if the fenced parent update affects no row. `begin_part` then **commits** that transaction (`queue.rs:52-55` of the diff). In practice this is unreachable: the parent was just locked `Live` in the same transaction, and `CURRENT_TIMESTAMP` is fixed for the transaction. But it breaks the "roll back rather than partially apply" pattern that `complete_part` follows.
  - D3 §E listed `inspection` among the users of the shared receipt discovery; `inspection.rs` is unchanged.
- Suggested: roll back on that `LeaseLost` branch, and either wire diagnostics or drop the claim from BRIEF-04.11.
- Resolution history: 2026-10-18 R006 OPEN (optional).
- 2026-10-18 [04.11-R007](agent-review-notes.md#0411-r007--correction-recheck-of-the-0411-batch-final-0411--combined-phase-04-integration--pass), digest `e76ef46c…c2cf`: **RESOLVED** (A2 for inspection). See [R007 dispositions](#r007-dispositions-for-0411-f015-to-f018).

## 04.11-F018 — Timing oracle and baseline evidence for the first-run failure (optional)
- Classification: optional suggestion, **low**, verification hygiene. Not blocking.
- Evidence:
  - `workflow_failure_exhausted_expiry_routes_and_deadline_delay_rolls_back` (`recovery_tests.rs:320-345`) sets a 0.15 s run deadline, then runs a whole-state `snapshot` before `retire_expired_io`. Under suite load the deadline can pass first, which takes the deadline route (`Ok`) rather than hitting the delayed-commit guard. It failed once in the first full run and passed in 7 later runs.
  - TASK says 04.11 "does not change … the delay trigger's path". In fact 04.11 changed `recovery::report` and added work to `maintenance::expire_on`.
  - The persistence AGENTS.md baseline step (stash, rerun 3–4 times) was not done.
- Suggested: record the window as a known load-sensitive oracle, and move the deadline update after `snapshot` (or derive it from the measured snapshot time) in a reviewed change; or record a stashed-baseline run.
- Resolution history: 2026-10-18 R006 OPEN (optional).
- 2026-10-18 [04.11-R007](agent-review-notes.md#0411-r007--correction-recheck-of-the-0411-batch-final-0411--combined-phase-04-integration--pass), digest `e76ef46c…c2cf`: **ADDRESSED; residual accepted (optional)**. See [R007 dispositions](#r007-dispositions-for-0411-f015-to-f018).

# 04.11 — correction recheck (04.11-R007)
Common context: 2026-10-18; workflow `implement-opus-reviewed-2.1`; batch 04.11 plus the combined phase-04 gate; correction recheck; round [04.11-R007](agent-review-notes.md#0411-r007--correction-recheck-of-the-0411-batch-final-0411--combined-phase-04-integration--pass); Reviewer B Opus 5.5 (`claude-opus-5-5`, xhigh requested). Snapshot base `5c1de32ecb3c1a7e59e1a60deb74ab5c42e755df`, digest `e76ef46c07965bb7b233d21813ac413efb6c5ad763e207ac3bc242168235c2cf`.

## R007 dispositions for 04.11-F015 to F018
- **04.11-F015 — RESOLVED.**
  - Item 1: pre-marker authority and route refusals in `observe`/`publish_on` return `MessagePublication::Rejected`, mapped to Terminal `message.rejected` (`message_publish.rs:288-364, 415-423`; `message_runtime.rs:505`). With a marker they stay retryable. Tested by `workflow_message_publish_revoked_authority_is_terminal_rejected_before_any_marker` and `…revocation_after_a_marker_stays_retryable`.
  - Item 2: replies continue the source's recorded relay trace (`source_relay_on`, persistence `message.rs:431-457`, keyed by a UNIQUE row; `message_runtime.rs:362-371`), matching `dispatch.rs:1436-1446`. Tested by `…external_reply_continues_the_source_relay_trace` (recorded and unrecorded sources).
- **04.11-F016 — RESOLVED.**
  - Item 1: both receipted F012 variants (`…finality_breach_then_every_destination_final_vetoes_the_receipt`), with the uncertainty assertion under amendment A1.
  - Item 2: per-recipient inbound resolution through `thread_for_message_keys`; Slack source refused.
  - Item 3: delayed COMMIT past the captured fence expiry and past the run deadline rolls back every row, with COMMIT reached proven by a sequence.
  - Item 4: SQL-only reopen from expired, non-final and foreign evidence, each isolated by a passing control; wrong-association actor refused.
  - Item 5: four `Injected` real competing-claimant races (settle vs consumer; settle vs consumer vs `begin_part`; two settlements; settle vs cancellation).
  - Item 6: `DeliveryReopened` on a `model_tool` execution reopens only the delivery.
  - The lock-count assertion is recorded as structurally bounded.
  - Focused areas, full suite (2674/0/22), stack (2669/0/22) and lease budget all pass per the `r006-*` logs.
- **04.11-F017 (optional) — RESOLVED.** `begin_part` rolls back on the expired-proof `LeaseLost` branch (`delivery/queue.rs:206-216`). The inspection claim is withdrawn through the accepted amendment A2 (D3 §E).
- **04.11-F018 (optional) — ADDRESSED; residual accepted.** The TASK diagnosis wording is corrected and the oracle recorded. Test hardening is left to a reviewed change, and the stashed-baseline rerun is explicitly reported as not run.

# Batch S — migration squash (S-R001)
Common context: 2026-10-18; workflow `implement-opus-reviewed-2.1`; Batch S (migration squash, before phase 05); batch review; round [S-R001](agent-review-notes.md#s-r001--batch-review-of-batch-s-migration-squash--changes-required); Reviewer B Opus 5.5 (`claude-opus-5-5`). Snapshot base `4b54485830495dfeb68d45c61c0d6d8dd684e4c5`, digest `0aeeca933e66244420f6b05b37d645cc79facbc2307890591b187d0f9ac43fa5`.

## S-F001 — The removed budget replay carried the only assertion of the sealed-root debit rule
- Classification: actionable defect, **low**, verification (lost non-upgrade coverage). Blocking for this batch. The review asks whether removing the upgrade tests lost any non-upgrade coverage, and TASK records all seven as populated-upgrade tests.
- Evidence:
  - The deleted `workflow_budget_schema_retained_migration_preserves_exact_activations_and_seals` (`budget_schema_tests.rs` at base) asserted two things:
    - (a) the `20260929070000` backfill: provenance `sealed_legacy`, activations 2, 2 receipts. This is upgrade-only and is correctly gone.
    - (b) after that, `receipt(&mut tx, child, "model_call", "no-new-allowance", 1)` is `Some("exhausted")`. This is runtime behaviour that still exists in the init schema.
  - The rule lives in `debit_workflow_budget_receipt` (init `:424`), at `:446-447`: `granted` only `WHEN budget.provenance='frozen_v2' AND NEW.quantity>0 AND …`, otherwise `exhausted`.
  - Sealing is live too. `insert_workflow_budget_link` (init `:925-955`, trigger `workflow_root_budget_link`) seals every root run whose stored bundle is not `compiler_revision` `'2'`, or is malformed (`data_exception`). The shapes are fixed by the CHECKs at `:6911-6912`.
  - In the artifact, `grep -rn sealed_legacy src` finds nothing. No remaining test asserts the sealing or a sealed root's debit (`budget_schema_tests.rs`, `budget_runtime_tests.rs`, `budget_race_tests.rs` and the reconciliation suites only exercise `frozen_v2` roots).
  - The SQL literal `'2'` is coupled to `compiler::SEMANTIC_REVISION` (`application/workflow/publication/storage.rs:105,126`). A revision bump that missed the literal would seal every new root, and only indirect budget failures would show it.
- Required correction: add a fresh-schema test with no migration replay or schema reconstruction:
  - a root run whose stored bundle is not revision 2, and one with a malformed bundle (rows inserted through the existing raw-run pattern, e.g. `run_schema_tests.rs:20`), records `workflow_root_budgets.provenance='sealed_legacy'` with NULL limits;
  - a positive debit for each resource (`activation`, `model_call`, `repetition`) is `exhausted`, and usage is unchanged;
  - a current-revision root is `frozen_v2`, as a control.

  Alternatively, root or the user may decide that the sealing branch is a legacy path under the no-backward-compatibility rule and remove it in a reviewed phase-05 in-place init edit. That is outside Batch S, whose scope is pure equivalence, so record the decision either way. Recheck: the new test, focused `workflow_budget_schema`, fmt, clippy, no-run, and a new digest.
- Instruction lesson: candidate. When deleting a test as upgrade-only, list each of its assertions and mark which ones exercise live schema or runtime behaviour, then replace those against a fresh schema.
- Resolution history: 2026-10-18 S-R001 OPEN.

## S-F002 — `init_function_body` reads past a function without a dollar-quoted body (optional)
- Classification: optional suggestion, **low**, test robustness. Not blocking.
- Evidence:
  - `workflow/tests.rs:35-44` finds `CREATE FUNCTION public.{name}(`, then takes the first ` AS $` after it, without checking that it belongs to the same statement.
  - The init schema has 8 SQL-standard-body functions, which have no ` AS $`: `valid_creation_provenance`, `notification_principal_can_view`, `transport_requires_installation`, `valid_binding_change_reason`, `valid_delivery_failure_class`, `valid_inbound_event_error_class`, `valid_inbound_event_ignore_reason`, `valid_inbound_safe_header_facts`. For any of them the helper would return the next function's body. Their `prosrc` is empty, so a pin would fail with a misleading mismatch, never pass falsely.
  - No pinned name is affected; emulating the helper reproduces `prosrc` for all 140 dollar-quoted functions (`logs/S-R001/checks.log` §3).
  - Related nit: `own_database_with_migrations` (`test_support.rs:296`) now has one caller with a fixed migrator, so its parameter is vestigial.
- Suggested: bound the search to the function's own statement, for example by requiring ` AS $` before the next `\n--\n-- Name:` header, and panic naming the function otherwise. Optionally inline the migrator parameter.
- Resolution history: 2026-10-18 S-R001 OPEN (optional).

## S-R002 dispositions for S-F001 and S-F002
Round [S-R002](agent-review-notes.md#s-r002--correction-recheck-of-batch-s-s-f001-s-f002--pass), digest `33ae9028d28d3047de936b2e9580ffe267bd0187dafe2092601d0cef51514552`.
- **S-F001 — RESOLVED.** `budget_schema_tests::workflow_budget_schema_sealed_roots_exhaust_every_debit` (`budget_schema_tests.rs:209-304`) runs on a fresh schema.
  - Both sealing branches (a revision-1 bundle and an undecodable bundle) are asserted `sealed_legacy` with NULL limits.
  - Every resource's positive debit is `exhausted`, and usage is unchanged.
  - The `frozen_v2` control grants.
  - Root's decision to keep the sealing branch is recorded in TASK.
- **S-F002 (optional) — RESOLVED.** `init_function_body` is bounded to its own statement and panics naming a function without a dollar-quoted body; a `should_panic` test pins this. Emulated against all 148 functions: 140 exact, 8 refused (`logs/S-R002/checks.log`). The vestigial parameter is removed.

# Phase 05 — architecture gate (05-R001)
Common context: 2026-10-18; workflow `implement-opus-reviewed-2.1`; phase 05; architecture gate; round [05-R001](agent-review-notes.md#05-r001--phase-05-architecture-gate-05-d1--changes-required); Reviewer B Opus 5.5 (`claude-opus-5-5`). Artifact: `logs/05/decision-D1.md` SHA-256 `4e3a46ebb03137ba5c86e1b3d64e1d3b0abf756c54eaa59efc8cee8301707931` (supporting `logs/05/investigation.md` `484af70e…975e`). The user's answers Q1, Q2, Q6 and Q8, relayed by root, are applied.

## 05-F001 — The error route carries no failure code, so a timeout is indistinguishable in-graph
- Classification: actionable defect, **medium**, design accuracy. It affects the viability of the user's Q2 answer for phase 07.
- Evidence:
  - `route_failure` commits the `FinalError` route with `output: serde_json::Value::Null` (`recovery.rs:217-219`).
  - The binding roots are only `Input | Params | Step | RunId | ParentRunId` (`domain/workflow/context/mod.rs:60-67`). No root carries a failure code, so an error-route successor reading the decision step sees `null`.
  - On a `decision.human` step, the error route is reached by `workflow.invalid_input` (`pending_recovery.rs:66-75` routes `InvalidInput`), by D1's `decision.reviewer_unavailable` (§5.1 step 4) and by `decision.timed_out` (§5.4). All three look identical downstream.
  - D1 §2 says "Plan 07's child maps the failure code to `choice: "timeout"` with `data.map`". That is not possible: no failure code reaches the graph.
  - Plan 07 (`07-…:52-63`) needs the child output `choice: "timeout"` and a parent "timeout: declared timeout route".
  - D1 itself rejects a dedicated route because downstream type projection has no second output shape. Exposing a code on the error edge has the same cost.
- Required correction: with Q2 fixed, D1 must state accurately what the error route exposes and choose a mechanism:
  - **(a)** `decision.reviewer_unavailable` fails the run instead of routing, so a configuration failure is never presented as a business timeout. This is the closest reading of plan 05:66 and is recommended. Document that the error route of a decision then means timeout or invalid input.
  - **(b)** Or keep both on the error route, and record that in-graph they are indistinguishable.

  In either case:
  - record where the code is durable (the audit `decision_timed_out`, attempt or run state);
  - write the phase 07 obligation explicitly: the child's `timeout` choice, or the parent's "timeout route", must come from the child run's terminal failure code (07.1 durable completion), or from a 07 language change. Do not claim it comes from `data.map`.
  - Add a 05A test that an error-route successor of a timed-out decision receives `null` and runs, and that a run without an error route fails with `terminal_execution_id` set.
- Instruction lesson: none new. Verify every "downstream can map X" claim against the committed output writer.
- Resolution history: 2026-10-18 05-R001 OPEN.

## 05-F002 — Q6 removes external reviewers: D1 must drop the credential design and re-home what depends on it
- Classification: actionable defect, **medium**, scope consistency (user decision Q6, 2026-10-18).
- Evidence: D1 embeds external email-only reviewers and their credentials throughout.
  - §0 rows 2 and 7; §1 05B "Credentials"; §1 point 3 (`DecisionAuthority::Credential`).
  - §2: the `{"kind":"external","email":…}` reviewer item.
  - §3.1: the reviewer `kind` column with `'external'`, and `submission_credential_id`.
  - §3.2: `workflow_decision_credentials`.
  - §6: lock-order steps 1 (externals) and 7 (credential), and the "credential redeemed twice" race and test.
  - §7.2 (external comments), §7.3 (`external` park check), §7.4, §7.5 (credential company).
  - §8 entirely.
  - §10: minting in the claim transaction, the external address, and the credential link.
  - §12: the credentials bound.
  - §13: 05B credential modules.
  - §14: `workflow_decision_credential_*` tests.
  - Investigation §6's proposed plan text ("or existing email-only external participants").

  Outside D1, these depend on the credentials:
  - **REPLACEMENT-MAP:72** (protected-action approvals) assigns the human decision a "scoped single-use link", with removal gate "05, 08 … Move pending/answered links … before removing harness-specific approvals".
  - **The legacy approvals** mail arbitrary `approver_email` addresses (init 4699-4721; `use_cases/approval.rs:259`; tests use non-member addresses such as `manager@acme.com`). Retiring them in phase 08 without a replacement drops that capability.
  - EXPANSION 05.3 ("expiring single-use external credentials"), plan 05:68 and the acceptance line "external decision credentials". Root says these will be amended.
- Required correction:
  - Remove every item above from D1 rather than keeping it as a seam. Under no backward compatibility the schema and language can be extended in place later, so nullable credential columns, a single-value `kind` column or an unused authority variant are speculative.
  - Keep the reviewer item as `{"kind":"user","id":<uuid>}`, the only kind, matching the 04.11 destination shape.
  - Rewrite investigation §6's proposed plan text to "explicitly named company members".
  - Record the deferral and its owner (a later phase, or NOT_PLANNED, as root and the user decide), and flag that REPLACEMENT-MAP:72 and phase 08's legacy-approval removal gate must be amended to match. Root/user decision: either the email-link approval capability is dropped with the legacy code, or the deferred credentials must land before that removal.
  - Then confirm that §5–§7 still hold with members only. They should: authority is `authorize_reviewer_on` plus association, nothing else.
- Instruction lesson: none.
- Resolution history: 2026-10-18 05-R001 OPEN.

## 05-F003 — Notice transport must be re-decided without credentials, and the notice audit collides per execution
- Classification: actionable defect, **medium**, design (reuse versus a new state machine), plus one correctness defect.
- Evidence:
  - D1 §10's case for a bespoke direct sender and a `workflow_decision_notices` ledger (claim, `SKIP LOCKED`, backoff, 8 attempts, withdrawal, a new `DecisionNoticeSender` port and adapter) rests mainly on the credential.
    - The first argument ("the delivery engine stores rendered parts durably, so a credential link would persist in plaintext") and the third ("precedent for one-time secrets") fall away under Q6.
    - The second argument concerns the `notification_events` outbox, not the delivery engine.
    - The fourth already concedes that members could use the delivery engine.
  - The delivery engine already accepts unattributed `notification` deliveries to an address (CHECK at init 5036; `delivery/enqueue.rs:144-175` `insert_standalone_delivery_on`). The notification projection enqueues them inside its own transaction (`notification.rs:995-1004`). So a per-reviewer notice could be written in the park transaction, durable and atomic, with an idempotency key such as decision id plus reviewer. It would get the existing claims, retries, failure classes and dead-lettering, and its rendered content carries no secret. A failed delivery never approves, trivially.
  - **Defect:** D1 §4/§10 audit `decision_notice_failed` "one event per execution". `workflow_run_event_execution_kind` is UNIQUE on `(company_id, run_id, execution_id, event_kind)` (init 10128). With 2–16 reviewers, a second failed notice's result transaction would violate it and could never be marked `failed`.
- Required correction:
  - Re-decide §10 and §16 Q3 for members only. Either use standalone `notification` deliveries enqueued in the park transaction (recommended, with no new port, ledger, deliverer or scheduling), or justify the bespoke ledger with a decision-relevant requirement the engine cannot meet.
  - Fix the audit: record failure per reviewer on the notice or delivery row, or drop the run event.
  - State what happens to a pending notice once the decision settles. A late email linking to a settled decision is harmless and may be accepted.
  - Keep the content rule: no proposal and no comments in the email.
- Instruction lesson: when a scope decision removes a design's primary constraint, re-run its alternatives table.
- Resolution history: 2026-10-18 05-R001 OPEN.

## 05-F004 — Member decision commands take the company row FOR NO KEY UPDATE
- Classification: actionable defect, **low**, liveness/scalability. Correctness is preserved.
- Evidence:
  - D1 §7.1 has `authorize_reviewer_on` take "the same locks as `authorize_company`", that is, `companies … FOR NO KEY UPDATE` (`authority.rs:14-19`), before waiting for the run lock (up to the 1000 ms lock timeout).
  - For admin-only controls this was acceptable. For submissions and comments by any member, one command waiting behind a busy run's worker makes every other decision command and control in the same company queue on the company row, and they can hit their own lock timeouts.
  - §6 claims "shared locks never conflict with shared locks", which does not hold for this row.
- Required correction: take `companies FOR SHARE` in `authorize_reviewer_on`, keeping the member ⋈ principal `FOR SHARE`.
  - That still blocks concurrent owner or role changes (they need `FOR NO KEY UPDATE` or row deletion).
  - It stays compatible with the workers' FK `KEY SHARE`.
  - It cannot form a cycle, since every path takes the company row first.

  Correct the §6 sentence. Alternatively, justify the serialization and record it with a test of a submission blocked behind a held run lock while a submission on another run of the same company proceeds.
- Resolution history: 2026-10-18 05-R001 OPEN.

## 05-F005 — Run-scoped event inbox: inexact 128 bound and unstated late-event semantics
- Classification: actionable defect, **low**, concurrency and specification.
- Evidence:
  - §11 bounds "at most 128 unconsumed events per run; one more is a `Conflict`", while "the run row gets a plain read, not a lock". Under READ COMMITTED, concurrent producers each count 127 and all insert, so the bound and its over-limit test are not exact. Root AGENTS.md requires competing-claimant tests for a new concurrency protocol.
  - Moving from execution scope to run scope (§3.1/§11) also means an unconsumed event can satisfy **any later** matching `wait.event` in the run, such as the next iteration of a repeated wait with a static correlation. Execution scoping prevented that. D1 does not state it.
- Required correction:
  - Make the bound exact without blocking on workers. For example, serialize producers per run with `pg_advisory_xact_lock` on the run key, then count and insert. Or redefine it as a soft bound with a stated overshoot and test that.
  - Add a competing-producers test at the bound.
  - State the late-event semantics: correlation must carry the iteration identity when a step repeats. Add a test that a stale event is consumed by the next matching wait only when its correlation matches.
- Resolution history: 2026-10-18 05-R001 OPEN.

## 05-F006 — 05B is direction-level: require a 05B decision addendum before 05B code
- Classification: actionable defect, **low**, gating. 05A is not blocked.
- Evidence: D1 §9 changes the phase 04 dispatch seam, yet the investigation's own risks are open:
  - I§5.1: reopening a completed job against `workflow_action_capture_schedule` and `workflow_action_confirm_claim_retirement`, plus attempt numbering and `max_retries`;
  - I§5.2: today a protected call rolls back its transaction, so the approval park must commit the wait and decision referencing an intent committed earlier;
  - I§5.3: the full list of one-wait-per-execution lookups for per-reason uniqueness.
  - D1 also leaves open who may approve a protected action. Q4 puts approvers on the step, but the protection itself comes from admin-level operator policy rows (`workflow_http_operations` and `workflow_mcp_operations.approval_required`).
- Required correction:
  - State in D1 that 05B starts with a decision addendum, reviewed by B before any 05B code. The addendum resolves I§5.1–5.3 against the code and enumerates the affected lookups with `graft callers`.
  - The addendum also decides approver eligibility. I recommend that protected-action approvers be owner/admin, rechecked at submit.
  - The rest of D1 §9 is accepted as direction: subject binding by FK to the frozen intent with both digests, Terminal on reject or expiry, no automatic replay, and at most one dispatch per approval.
- Resolution history: 2026-10-18 05-R001 OPEN.

## 05-F007 — Clarifications to carry into the revision
- Classification: actionable, **low**, specification completeness.
- Items:
  1. **Static reviewer roots.** The registry check must walk the binding tree recursively (`Object`, `Array`, `Default` reference and fallback, `Concat`, …) and accept only `Literal` and `Params` roots. Run `params` are admin-configured binding snapshots (verified), so Q5 is sound under that rule.
  2. **Deadline not in the future at park.** State the outcome. Recommended: `workflow.invalid_input` through the existing pending-failure path, never a park.
  3. **The park-time failure writer.** `decision.reviewer_unavailable` before any wait exists is a pending failure. Use `pending_recovery::settle` with a new variant (or F001's choice (a)), not `recovery::route_failure`, which is the `Recovered` completion owner.
  4. **`NO_BACKWARD_COMPATIBILITY_NOTES`.** Root AGENTS.md requires every plan carrying the note to be listed. 05A should add `05-…` through `10-…` (and `BRIEF-05.md` if created), not only `05-…`.
  5. **`validate_completion` visibility.** It is `pub(super)` in `application/workflow/batch.rs:115`; say where the decision command calls it from.
- Resolution history: 2026-10-18 05-R001 OPEN.

# Phase 05 — correction recheck (05-R002)
Common context: 2026-10-18; workflow `implement-opus-reviewed-2.1`; phase 05 architecture gate; correction recheck; round [05-R002](agent-review-notes.md#05-r002--correction-recheck-of-the-phase-05-gate-05-d2-over-d1--changes-required); Reviewer B Opus 5.5. Artifact: `logs/05/decision-D2.md` `831890270a87273b2cbbccf580a942c4582841cc2c4430b15d542d95754815b3`, a delta over D1 `4e3a46eb…7931`.

## 05-R002 dispositions for 05-F001 to 05-F007
- **05-F001 — RESOLVED.**
  - D2 §4.3/§7.1: `decision.reviewer_unavailable` is a non-routed `pending_recovery::settle` variant. Verified: `settle` runs on the pending, unleased park job (`pending_recovery.rs:47-96`), fails the run with `terminal_execution_id`, fails the job, and audits the code as event kind.
  - The error route is stated to carry `null` and to mean timeout or invalid input.
  - The durable code is the `decision.timed_out` audit.
  - The phase 07 obligation is recorded, and four tests are named.
- **05-F002 — RESOLVED.** No external or credential item remains: no `kind` column, credential table, `submission_credential_id` or authority variant. Plan, EXPANSION, NOT_PLANNED and REPLACEMENT-MAP 72/74 amendments are listed (D2 §15), consistent with the user's Q6 and email-link-approval decisions.
- **05-F003 — RESOLVED** (the link integrity point moves to 05-F009).
  - Standalone `notification` deliveries are enqueued in the park transaction through `insert_standalone_delivery_on`, following the `notification.rs:995-1004` precedent. There is no new port, ledger or deliverer.
  - The `decision_notice_failed` run event is dropped.
  - Late notices are accepted as harmless.
- **05-F004 — RESOLVED.** `companies FOR SHARE` in `authorize_reviewer_on`. The §8 statement is corrected, and a two-runs-one-company test is added. Every `authorize_company` caller takes the company row before the run (graft callers), so there is no cycle.
- **05-F005 — RESOLVED.**
  - A producer-only per-run `pg_advisory_xact_lock` (precedent at init 771) makes the 128 bound exact. Consumption only lowers the count.
  - Stale-event semantics are stated, and competing-producers and stale-event tests are added.
- **05-F006 — ADDRESSED.** The §16 addendum is part of this gate. §16.1, §16.3 and §16.4 are accepted; §16.2 carries the new 05-F008.
- **05-F007 — RESOLVED** (all five items).

## 05-F008 — The approval reopen reuses the parked attempt number and is unguarded
- Classification: actionable defect, **medium**, 05B correctness (the approved dispatch could never be claimed).
- Evidence:
  - §16.2 closes the approval-parked attempt and moves the job `processing`→`completed` "as `effect_parked` does". The effect park leaves `retry_count` unchanged (`message_publish.rs:263-266`: `retry_count+1=$6`, no increment).
  - Approve then reopens `completed`→`pending` "with `run_at = now` and `retry_count` unchanged".
  - A claim takes attempt `retry_count + 1` (`lease_claim.rs:64-69`) and inserts it (`:171`), but `task_attempts_task_attempt_key UNIQUE (task_id, attempt_number)` (init 8209) already holds that number for the `approval_parked` attempt. **The re-claim fails on every poll.**
  - The cited precedent `control_retry.rs:37` reopens `failed`→`pending` *after* a retirement, and retirement incremented `retry_count`: `check_workflow_fenced_retirement` requires `NEW.retry_count = OLD.retry_count + 1`, and `workflow_action_claim_episode_guard` requires `job.retry_count = retired_attempt`. A completion does not increment it.
  - So the stated park precondition `retry_count < max_retries` is vacuous: it held when the park attempt was claimed.
  - Nothing guards the new `completed`→`pending` transition. Every other workflow job transition is guarded at the database (`check_workflow_fenced_completion`, `check_workflow_fenced_retirement`, `check_workflow_control_retry`).
- Required correction (in §16.2):
  - Consume the parked attempt on reopen, `retry_count = retry_count + 1`, or at the park close. Then make the park precondition `retry_count + 1 < max_retries`, so that `exhausted` (`lease.rs:409-411`) cannot fire on the reopened job.
  - Add a deferred guard branch admitting `completed`→`pending` for a workflow job only when all of these hold:
    - attempt `OLD.retry_count + 1` is `completed`/`approval_parked`;
    - `NEW.retry_count = OLD.retry_count + 1` (or the chosen accounting), with `max_retries` and ids unchanged;
    - the run is `running`;
    - the execution's approval decision was submitted `approve` and its wait completed in this transaction.
  - Tests:
    - the approved dispatch claims attempt N+1 and dispatches once;
    - a reopen without an approval is refused by the guard;
    - a park with exactly one attempt left fails Terminal `approval_required` with no park.
- Resolution history: 2026-10-18 05-R002 OPEN.

## 05-F009 — `notice_delivery_id` needs uniqueness, insert-only writing and a provenance binding
- Classification: actionable defect, **low**, database-boundary integrity (persistence AGENTS.md "Preserve invariants at the database boundary").
- Judgement on the design point: an id-only FK `notice_delivery_id → message_deliveries(id)` is the only expressible FK, because standalone deliveries have `company_id` NULL by CHECK (init 5036). That is acceptable and matches `notifications.email_delivery_id` (init 12265). However:
  - **The precedent also has `UNIQUE (email_delivery_id)`** (`notifications_email_delivery_key`, init 7930). D2 has no such key, so two reviewer rows, even in different companies, could reference one delivery.
  - **D2 updates the reviewer row after insert** (§7.1 step 6: insert reviewers, then deliveries, then "set `notice_delivery_id`"), and §5.1 exempts the column from immutability generally, so a later update could re-point the link.
  - The rule requires every side of a join to be proven to belong to the same tenant. A tenant-less delivery cannot carry the tenant, but its provenance can be proven. `delivery_key` is readable: `"notification:workflow-decision:{decision_id}:principal:{principal_id}:{destination}"` (`compose.rs:250-264`; well under 512 bytes, so never hashed here). The decision is tenant-scoped.
- Required correction:
  - Add `UNIQUE (notice_delivery_id)`.
  - Insert each delivery first, then the reviewer row with its id, so the column is insert-only. Make it `NOT NULL` with `ON DELETE NO ACTION`: every member has `users.email NOT NULL`, and production never deletes deliveries. If `ON DELETE SET NULL` is kept, the immutability trigger must allow only value→NULL.
  - Add a constraint trigger proving provenance: the referenced delivery is `purpose='notification'` with `idempotency_key` prefixed `'notification:workflow-decision:' || decision_id || ':principal:' || principal_id || ':'`.
  - Add a transaction-scoped DB test that a reviewer row referencing another decision's (or another company's) delivery is rejected (the AGENTS composite-FK test obligation, adapted).
- Resolution history: 2026-10-18 05-R002 OPEN.

# Phase 05 — correction recheck (05-R003)
Common context: 2026-10-18; workflow `implement-opus-reviewed-2.1`; phase 05 architecture gate; correction recheck; round [05-R003](agent-review-notes.md#05-r003--correction-recheck-of-the-phase-05-gate-05-d3-over-d1d2--changes-required); Reviewer B Opus 5.5. Artifact: `logs/05/decision-D3.md` `bd937b8630e48e6c7e2bf0bfeb223cd879004e4c3cfb898bb302b0d432af0b69` over D2 `83189027…15b3` and D1 `4e3a46eb…7931`.

## 05-R003 dispositions for 05-F008 and 05-F009
- **05-F008 — RESOLVED.** D3 §2.
  - The parked attempt is consumed at reopen (`retry_count = k+1`, next claim `k+2`), and the park requires `k+1 < max_retries`.
  - A deferred branch in `check_workflow_fenced_completion` refuses every workflow completed→pending except an approval reopen with `submitted_xact = pg_current_xact_id()`, the `approval_parked` attempt, unchanged identity and bounds, NULL lease fields, an open execution and a running run.
  - At most one reopen per approval.
  - Three tests.
  - Verified: no other production path performs completed→pending (`control_retry.rs:37` and `action_reconciliation/settlement.rs:217` are failed→pending).
- **05-F009 — RESOLVED** for integrity. D3 §3.1/§3.2.
  - `UNIQUE (notice_delivery_id)`, `NOT NULL`, `NO ACTION`, an insert-only row with the delivery inserted first, and a `Created`-only result.
  - The provenance trigger checks purpose, the standalone shape and the prefix `notification:workflow-decision:{decision}:principal:{principal}:`, proving the tenant through the decision's composite FK.
  - The transaction-scoped rejection test covers six cases.
  - The residual liveness edge is 05-F010.

## 05-F010 — The notice key is not proven unhashed, so a long member address loops the park
- Classification: actionable defect, **low**, liveness at park.
- Evidence:
  - D3 §3.1 says the destination is "the member's bounded address, so the key stays under the bound and is never hashed", and that if it were hashed the trigger refuses and "it fails closed".
  - The key is `notification:workflow-decision:{36}:principal:{36}:email:{address}`, 121 bytes plus the address (`compose.rs:250-270`, `delivery.rs:86-93`). `delivery_key` hashes it to `notification:sha256:…` above 512 bytes (`delivery.rs:35`), that is, for an address over 391 bytes.
  - Nothing bounds a member address:
    - `users.email` has only a not-blank CHECK (init 5978-5987);
    - `EmailAddress` is an unbounded newtype (`value_objects.rs:136`);
    - registration only trims (`use_cases/user.rs:449`);
    - I found no length validation in `src`.
  - For such a member, the BEFORE INSERT provenance trigger raises 23514 inside the park transaction. That is a database error, not the D3 §3.2 `BadRequest` class, so it propagates and the park retries on every poll until the run deadline. That is exactly the path D3 §0 says it removed.
  - Integrity is safe (fail closed). Liveness is not.
- Required correction (either):
  - **(a)** In D3 §3.2 step 6, after composing each notice and before any write, check that its idempotency key has the expected unhashed prefix. If not, `settle(ReviewerUnavailable)`, the same as a composition `BadRequest`.
  - **(b)** Bound member addresses: for example `CHECK (octet_length(email) <= 320)` on `users`, edited in place, plus the matching validation at registration and account change.

  (a) is local to phase 05 and recommended. Correct the "bounded address" sentence either way.
- Test: `workflow_decision_park_unprovable_notice_key_fails_run_unrouted`. A member with an address over 391 bytes is listed; the run ends `failed` with `decision.reviewer_unavailable`, and nothing is parked or retried.
- Resolution history: 2026-10-18 05-R003 OPEN.

## 05-R004 disposition for 05-F010
Round [05-R004](agent-review-notes.md#05-r004--recheck-of-the-phase-05-gate-05-d4-over-d1d2d3--pass), D4 `e8ed88c46e1420e3ededa3da33b282a8e6c9cefc9495e3146a420ed77fe1fc86`.
- **05-F010 — RESOLVED** (option (a), root's choice).
  - D4 §1 corrects the hashing statement: hashed above 512 bytes, i.e. addresses over 391 bytes; addresses are unbounded.
  - D4 §2 adds a pre-write prefix check in `DecisionNotices`, returning the composition `BadRequest` class. The park then runs `settle(ReviewerUnavailable)`: unrouted, nothing parked, no retry.
  - The 05B twin fails Terminal `<family>.approval_required` with no park.
  - Named tests cover both batches. The provenance trigger remains as the fail-closed backstop.
  - Bounding addresses (option (b)) is deferred out of phase 05.


## 05-F011 — Decision authorization acceptance lacks actual revocation and visibility races
- Context: 2026-10-18; `implement-astra-reviewed-2.1`; whole 05A batch, round 05-R005; Reviewer B requested `gpt-6-astra` / medium, independent context. [TASK](TASK.md), [REVIEW](REVIEW.md). Base `feefe3e060a851fd8860e4bacb90c11c7b284888`; artifact `e2a59ac010a2b8c819f88a490729a21d97e9dde868ec4a8681ae54277a3e053a`.
- Classification: actionable defect, **medium**, verification/security acceptance.
- Evidence: plan 05 acceptance explicitly requires revoked membership and non-member refusal; accepted D2 §14 requires revocation in both orders. All decision submit callers were enumerated with `graft grep submit_decision`. `decision_tests.rs:451-473` only demotes a still-present admin on a company-only run and supplies a random nonexistent company. It does not remove membership, use an actual non-member, exercise channel/thread visibility, or compete revocation against submission. All `DecisionFixture::configured` reviewers are admins (`decision_tests.rs:22-74`). Existing tests of the shared association helper do not exercise the new command's lock ordering and recheck integration.
- Required correction: add decision-command database tests for a listed reviewer whose membership is actually removed and a non-member, and real existing cross-company decision access. Exercise channel visibility loss and the new command against membership/grant revocation with controlled competing transactions in both serialization orders. Assert refusal with no submission/continuation when revocation wins, and that revocation waits until commit when submission owns authority. Include read/comment refusal after access loss because these commands share the new authorization path. Keep the existing demotion test.
- Instruction lesson: current root concurrency rule and D2 acceptance already require this; test names are not evidence of the mutation they claim. Candidate guidance: acceptance mapping should name the actual revoked row and lock-order witness, not only a test-name prefix.
- Resolution history: 2026-10-18 05-R005 OPEN. This is a missing verification finding, not a demonstrated authorization bypass.

## 05-F012 — Concurrent event-id reuse across runs escapes the promised Conflict classification
- Context: 2026-10-18; `implement-astra-reviewed-2.1`; whole 05A batch, round 05-R005; Reviewer B requested `gpt-6-astra` / medium. [TASK](TASK.md), [REVIEW](REVIEW.md). Base `feefe3e060a851fd8860e4bacb90c11c7b284888`; artifact `e2a59ac010a2b8c819f88a490729a21d97e9dde868ec4a8681ae54277a3e053a`.
- Classification: actionable defect, **low**, correctness/concurrency.
- Evidence: `waits.rs:218-284` serializes producers on `(company,run)` but deduplicates on `(company,event id)`. The init schema's `workflow_wait_events_pkey` is `(company_id,id)` at line 9466. Two producers with the same event id and company, different valid runs and declared event names can both read no saved row under different advisory locks. One inserts, the other gets 23505 from the plain INSERT. `adapters/persistence/mod.rs:123-135` converts that into `AppError::Database`, while the public port (`application/workflow/waits.rs:35-36`) and D1/D2 promise `Conflict` for an id reused with different content/scope. Sequential replay correctly returns Conflict; concurrent replay does not.
- Required correction: handle this specific id uniqueness collision as `Conflict` without converting unrelated database errors, or atomically resolve insertion and compare the saved identity. Preserve the per-run bound and absence of a consumer/advisory lock cycle. Add competing producers using one id across two valid runs; assert exactly one Recorded, one Conflict and one durable row. This conclusion is by code/interleaving inspection; no database reproduction was run during the fixture-owned gate.
- Instruction lesson: align dedup serialization/error handling with the database unique key's scope, not merely the resource-bound scope. The accepted architecture specified per-run locking but did not spell out cross-run id collisions; this is a missed edge in 05-R004's effective design coverage.
- Resolution history: 2026-10-18 05-R005 OPEN.

## 05-F013 — Several explicit 05A boundary and provenance checks are absent
- Context: 2026-10-18; `implement-astra-reviewed-2.1`; whole 05A batch, round 05-R005; Reviewer B requested `gpt-6-astra` / medium. [TASK](TASK.md), [REVIEW](REVIEW.md). Base `feefe3e060a851fd8860e4bacb90c11c7b284888`; artifact `e2a59ac010a2b8c819f88a490729a21d97e9dde868ec4a8681ae54277a3e053a`.
- Classification: actionable defect, **low**, verification/accepted criteria.
- Evidence: D3 §3.1 accepted notice provenance tests cover another company, non-notification delivery, uniqueness reuse and update as well as wrong principal/decision. `decision_sql_tests.rs:70-117` only substitutes two already-linked same-company notification deliveries (wrong principal and wrong decision); the update check exists separately at lines 39-50. No actual foreign-company reviewer/comment/decision relationship rejection test covers the new composite FKs, as persistence AGENTS requires. D2 §12 explicitly promises at-limit and over-limit checks for feedback and decision data; current command tests only reject an integer against a string schema, missing feedback and stale metadata (`decision_tests.rs:180-243`), and digest testing only checks an overdeep value. Comment/reviewer/event-count limits are covered and need not be repeated.
- Required correction: complete the new decision-table cross-tenant/provenance database cases, distinguishing the expected constraint/trigger so one refusal cannot mask another; add accepted/rejected boundary cases for feedback (including character-versus-byte behavior) and decision data at the configured context limit. Assert invalid submissions leave the decision and continuation unchanged. Reuse existing generic validator tests only with explicit evidence of their coverage of the decision command composition/fingerprint path.
- Instruction lesson: the existing accepted matrix suffices; reconcile promised cases against assertions before freezing. No instruction expansion is needed.
- Resolution history: 2026-10-18 05-R005 OPEN.


## 05-F014 — D5 must cover the actual member-removal lifecycle as well as raw deletion
- Context: 2026-10-18; `implement-astra-reviewed-2.1`; correction architecture gate, round 05-R006; Reviewer B requested Astra/medium, independent context. [TASK](TASK.md), [REVIEW](REVIEW.md). Decision D5 SHA-256 `ec061ebe8a01fedf18b18258337c09dcbd1567394d59d13226374eef8c9a1cdb` over D1–D4; code is unfrozen correction WIP, not batch-accepted.
- Classification: actionable defect, **low**, architecture evidence/verification.
- Evidence: D5's opening explains membership revocation exclusively through `company_members` DELETE cascading to principals, based on `correction-first.log`'s 23503. That SQL failure is real, but the actual `CompanyInvitePersistence::remove_member` implementation (`src/adapters/persistence/company_invite.rs:355-384`) first updates the principal to `kind='external', user_id=NULL`, then deletes the member. Existing reviewer reads join that principal and decode `user_id` into non-null UUID (`decisions.rs:551-553`), so actual removal has a distinct broken-read mode even without deleting the principal. The production mutation also takes principal before member, unlike D2's stated member/principal authority sequence; raw DELETE-only race tests do not prove that real path.
- Required correction: amend D5's evidence and verification to explicitly retain raw-delete provenance protection while also exercising the actual remove_member port before/after settlement and in the authority races. Confirm the snapshot read survives both principal disappearance and conversion to external/NULL user. State how the chosen lock sequence handles the actual principal-first mutation (safe transient timeout/retry must not be represented as a guaranteed success/no-cycle proof). No broad principal lifecycle redesign is requested.
- Instruction lesson: follow the production mutation caller before deriving lifecycle semantics solely from FK cascade behavior. Existing graph/caller requirements already support this; no new general instruction needed.
- Resolution history: 2026-10-18 05-R006 OPEN. The local snapshot design and disclosure-stage amendment are otherwise acceptable by inspection; see round note.


## 05-R007 disposition for 05-F014
- 2026-10-18; `implement-astra-reviewed-2.1`; Reviewer B requested Astra/medium. D5 `ec061ebe8a01fedf18b18258337c09dcbd1567394d59d13226374eef8c9a1cdb` + D6 `93aabee933500c4d2f9a02535901be92768f31ff47a2b19493525652d1f54c80`.
- **05-F014 RESOLVED at architecture level.** D6 explicitly covers production principal conversion and raw cascading deletion separately, preserves historical user identity independently of either, upgrades initial provenance locks to FOR SHARE, acknowledges the opposing member/principal lock order without a global no-cycle claim, and requires actual-port lifecycle/race tests. Controlled ordering must reach durable success/refusal; a timeout is admissible only as a separately asserted rollback/contention outcome, not proof of the business branch.
- Implementation and test outcomes remain subject to the corrected whole-batch review. This does not close 05-F011–013 or accept correction WIP.


## 05-R008 dispositions — 05-F011, 05-F012 and 05-F013 RESOLVED
- Context: 2026-10-18; `implement-astra-reviewed-2.1`; retained independent Reviewer B requested Astra/medium. Corrected 05A base `feefe3e060a851fd8860e4bacb90c11c7b284888`, digest `2a428cd707ec4427ddb31ca9ea60d875ba829c1bb0c4c743ab7f38929d99bef3`; sole exclusion `plan/workflow-foundation/docs`. [Round note](agent-review-notes.md#05-r008--corrected-05a-consolidated-recheck--code-pass).
- **05-F011 RESOLVED.** `decision_authority_tests.rs:57-230` tests actual non-member/raw membership removal, read/comment/submit refusal, and controlled membership/grant revocation in both orders with backend lock witnesses and durable outcomes. Lines 244-455 cover production remove_member conversion before/after settlement, actual removal blocked by submit/park, conversion-first and raw-delete races, and history retained after both mutation types. `decision_provenance_tests.rs:53-116` uses an existing foreign company and independently exercises scoped relationships. The D5+D6 identity-snapshot correction is implemented, not just tested: initial tenant/person checks use FOR SHARE, reviewer user_id is populated by the guard and read from history, three ephemeral principal FKs removed, decision/wait composite FKs retained.
- **05-F012 RESOLVED.** `waits.rs:282-292` maps only a unique violation on `workflow_wait_events_pkey` to Conflict. Other DB errors propagate. `workflow_wait_event_id_collision_across_competing_runs_is_conflict` holds a table lock until both producers reach INSERT under their distinct run locks, then asserts one Recorded, one Conflict and one row. This exercises the formerly missing interleaving without changing the producer bound or consumer protocol.
- **05-F013 RESOLVED.** `decision_provenance_tests.rs:53-187` separately verifies tenant/person snapshot guards, decision/wait/comment/reviewer composite relationships, wrong-company notice provenance, unique notice links and non-notification refusal. Transaction-local disabling of the provenance trigger isolates the UNIQUE check; temporary removal of the delivery attribution CHECK isolates the purpose check; both rollback. `decision_boundary_tests.rs:20-69` checks feedback at/over MAX_TEXT for ASCII, two-byte and four-byte characters and exact configured committed-output byte boundary, with unchanged state on rejection. Existing comment/reviewer/event bounds reused.
- Evidence: `logs/05/correction-fourth.log`, 44 passed / 0 failed, inspected alongside assertions. Strict all-target Clippy output inspected in `correction-clippy.log`. Broader correction gates are still pending; these dispositions close the reviewed defects, not root batch acceptance. 05-F014 remains resolved, now with implementation evidence above.


## 05-F015 — 05B omits explicit accepted reopen-guard and notice-failure cases
- Context: 2026-10-18; `implement-astra-reviewed-2.1`; whole 05B batch review 05-R010; retained independent Reviewer B requested Astra/medium, effective telemetry unavailable. [TASK](TASK.md), [REVIEW](REVIEW.md). Base `feefe3e060a851fd8860e4bacb90c11c7b284888`, digest `b42023ba2177a20937e6aa0c14b4abf6dc6e41fcbcf2a89a6fa711370f6fd630`, sole exclusion `plan/workflow-foundation/docs`.
- Classification: actionable defect, **low**, verification/accepted architecture criteria; no implementation bypass demonstrated.
- Evidence: D3 §2 explicitly requires completed→pending rejection with no approval, an approval committed in an earlier transaction, a reject choice, and wrong retry_count. `action_approval_http_tests.rs:268-291` tests only no submission. No source test references submitted_xact (`graft grep` exhaustive). The positive next-attempt test proves the happy path but cannot detect removal of the transaction-identity predicate or accidental admission of stale approvals. The guard is consequential persistent ownership enforcement, not a cosmetic check.
- Separately, D4 §2 explicitly requires the 05B twin `workflow_action_approval_unprovable_notice_key_fails_without_park`. Exhaustive `graft grep unprovable` finds only the 05A business test. 05B has a different error mapping: `decision_approvals.rs:77-81` returns no park on BadRequest so the dispatch family yields Terminal approval_required, whereas business decisions settle reviewer_unavailable. Sharing the composer does not exercise this distinct transactional/failure path.
- Required correction: add the three missing direct-SQL reopen variants, isolating the expected reopen guard rather than accepting an unrelated error. Include a prior-transaction approved decision with otherwise plausible accounting/attempt state, a rejected decision, and an approving transaction with wrong retry_count. Add the approval-specific long-address/unprovable-key case and assert Terminal family.approval_required, no approval wait/decision/reviewer/notice/dispatch/remote-entry rows, and no provider call. Run focused approval/guard checks and retain results; broader rechecks may reuse unaffected evidence for test-only additions.
- Instruction lesson: the prior D3/D4 verification obligations were precise and sufficient; reconcile each promised branch with assertions before freeze. This is another acceptance-matrix coverage gap, following 05-F011/F013, not a request to expand design or add speculative cases.
- Resolution history: 2026-10-18 05-R010 OPEN.


## 05-R011 disposition — 05-F015 RESOLVED
- 2026-10-18; `implement-astra-reviewed-2.1`; retained Reviewer B requested Astra/medium. Base `feefe3e060a851fd8860e4bacb90c11c7b284888`, corrected artifact `ac6e12f99bf53762a9056a6fcea62ac4a90513d4315dc47e5e3b087d21d4a84a`; sole exclusion `plan/workflow-foundation/docs`.
- **05-F015 RESOLVED.** `action_approval_sql_tests.rs:129-202` supplies prior-transaction approval, reject, wrong retry_count and valid positive control. It verifies submitted_xact matches only the current transaction, targets the exact 23514 reopen message, and checks failed reopen leaves completed/retry_count=0. The existing missing-submission test now asserts the same exact guard. Thus unrelated constraint failure cannot masquerade as the target refusal.
- `action_approval_sql_tests.rs:205-241` sets a long address that hashes the notice key, asserts Terminal http.approval_required, zero waits/decisions/reviewers/deliveries/dispatches/remote entries/receipts, and no provider response; the test server is aborted and joined. This exercises the distinct approval no-park mapping required by D4.
- Evidence inspected: `05b-f015-focused.log` 19/0; `05b-f015-gate-status.log` fmt/diff/strict Clippy/SQLx prepare/check/graph PASS. Test-only delta permits reuse of unaffected 05-R010 production and broad batch evidence. No open review findings remain; final phase05 verification remains a separate gate.


### 05-R012 final disposition
No new findings from the final fixture delta or combined integration evidence review. All phase05 findings 05-F001–05-F015 remain resolved. Exact accepted review artifact and verification limitations are recorded in [05-R012](agent-review-notes.md#05-r012--final-phase05-delta-and-combined-integration-evidence--pass).


## 06-F001 — 06B1 lacks promised production usage and revocation-race proof
- Context: 2026-10-19; `implement-astra-reviewed-2.1`; whole06B1 review06-R005; retained Reviewer B requested Astra/medium. [TASK](TASK.md), [REVIEW](REVIEW.md). Base `5b4989ae0038a82f431db60a338b7f59d9bcf25c`, digest `7c438b4fe32fbff52d6d97428577af7551e0c33f74b600452ffae78e4c574a2f`, sole exclusion `plan/workflow-foundation/docs`.
- Classification: actionable defect, **low**, verification/accepted gate obligations; no implementation bypass demonstrated.
- Evidence: D2 Verification expressly requires production Rig reserve/settlement observable rows, partial usage, repair calls and current-membership revocation races. New `task/harness_usage_tests.rs:37-97` manually invokes HarnessRunStore and manually constructs Mixed usage (`24-35`); it does not enter the production Rig runner/provider capture/repair path. `06B1-focused.sh` selects `adapters::harness::rig`, not the production database-backed `application::use_cases::thread` Rig/structured-response scenarios. Exhaustive graft grep model_usage_calls finds no projection assertions in those production scenarios. Thus existing adapter behavior and new SQL projection have separate proofs, but their production composition and repair attribution are not asserted.
- `harness_usage_tests.rs:222-294` checks demotion only after it fully commits. It does not race a report/price write with membership revocation. Existing `workflow/control_acceptance_tests.rs:338-386` proves the shared authority helper locks, but cannot detect this new caller releasing/committing authorization before report/write completion. D2 required that new transaction boundary to be verified.
- Required correction: extend a real durable Rig/ThreadUseCases scripted-provider scenario to assert reservation/settled projection and report attribution for initial+repair calls, include a provider response with missing one usage side to verify actual Mixed capture reaches analytics, and verify replay/resumption does not inflate rows. Add a controlled competing revocation versus report/price-write test with backend lock/synchronization evidence, positive authorized control and revoked refusal, proving the same transaction holds authority through the operation. Reuse existing fixtures/helpers; run the new focused cases plus affected production durable/structured-response checks. No design rewrite or full-suite rerun required for test-only correction unless tests reveal a defect.
- Instruction lesson: explicit D2 verification wording was sufficient; reconcile each promised composition/concurrency case against an actual assertion before freezing. Similar acceptance-matrix coverage gap to05-F015; no new global rule warranted by this round.
- Resolution history: 2026-10-19 06-R005 OPEN.


### 06-R006 disposition — 06-F001 RESOLVED
- 2026-10-19; `implement-astra-reviewed-2.1`; retained Reviewer B requested Astra/medium. Base `5b4989ae0038a82f431db60a338b7f59d9bcf25c`, corrected artifact `4eabf1ca4939d6446eeb92baca22a82696496ecb0cca2beac1ddf3fb2eb44222`, sole exclusion `plan/workflow-foundation/docs`.
- **RESOLVED.** Production ThreadUseCases structured-response scenario now supplies real scripted provider JSON with prompt_tokens=2 and missing completion usage (`model_usage_assertions.rs:10-15`, `structured_response_tests.rs:373-400`), requiring Mixed capture and reserved-output fallback. Shared assertions (`17-101`) check every reservation/request identity, agent/provider/model attribution, settled/unconfirmed shape, token values and authorized report counts. Initial+repair success and exhaustion cases invoke those assertions. Existing production approval-resume and provider-deadline recovery scenarios assert projection at both parked/interrupted and resumed checkpoints, proving saved calls remain single-counted while unresolved reservations survive.
- `harness_usage_authority_tests.rs:39-159` runs both report and price operations in both revocation orders. It observes actual backend blocking after authorization at model_prices, checks concurrent demotion fails specifically with55P03 while the operation holds membership authority, then verifies success and subsequent revoked refusal. Revocation-first observes the operation blocked on the demotion transaction before committing and requires NotFound; final price count proves only authorized mutation committed. Fixture is isolated with own_database.
- Evidence inspected: correction model_usage13/0, production Rig3/0 and structured3/0; final strengthened production rerun9/0. Correction strictClippy/offline/SQLx/fmt/diff/graph pass. No production/schema/UI changes; reuse06-R005 unaffected evidence. No open06 findings remain.


## 06-F002 — Earlier compiler revision bump broke SQL root-budget admission — RESOLVED
- Context: 2026-10-19; `implement-astra-reviewed-2.1`;06-R008 integration recheck, Reviewer B requested Astra/medium. Base `5b4989ae0038a82f431db60a338b7f59d9bcf25c`, artifact `1d7e5545bb22655b22c40e14aac9f49e67f41af5683699a983b7e23deb4ce4d5`, sole exclusion `plan/workflow-foundation/docs`.
- Classification: actionable correctness/integration defect, high in prior06A artifact; author discovered during06B2a, already corrected before this review.
- Evidence: accepted06A bumped compiler SEMANTIC_REVISION2→3 while SQL insert_workflow_budget_link still selected root_budget only for revision2. Current bundles were classified sealed_legacy, causing initial activation to exhaust. Author evidence `logs/06/06B2a-claim-diagnostic.log`; missed by06-R002 pure compiler/publication coverage. This was not caused by D3.
- Resolution: init SQL now accepts strictly revision3, emits/requires frozen_v3 in debit/reconciliation predicates, and retains denial of old/malformed bundles. `budget_schema_tests.rs:309-322` admits the actual current compiler bundle then activates and checks one activation debit/provenance; existing sealed-root matrix adds explicit revision2 refusal. Workflow765/0 evidence includes both. **RESOLVED in06-R008** for this artifact; no compatibility fallback or raised budget.
- Instruction lesson: compiler semantic revision is a cross-language contract. Candidate scoped to compiler/publication changes: exhaustively trace persisted SQL revision/provenance consumers and run actual admission→activation when bumping SEMANTIC_REVISION. Earlier06-R002 follow-up recorded below.

## 06-F003 — Approval audit exemption weakens unrelated event-kind constraints
- Context: 2026-10-19; `implement-astra-reviewed-2.1`;06-R008; Reviewer B requested Astra/medium. Base `5b4989ae0038a82f431db60a338b7f59d9bcf25c`, artifact `1d7e5545bb22655b22c40e14aac9f49e67f41af5683699a983b7e23deb4ce4d5`, sole exclusion `plan/workflow-foundation/docs`.
- Classification: actionable defect, medium, database integrity/accepted D3 contract.
- Evidence: init SQL `workflow_run_event_execution_kind` at10415 now exempts every row with decision_id, while `workflow_approval_audit_guard` at14367-14386 only requires the referenced decision to be action_approval. It never restricts event_kind. A writer can insert the same ordinary event kind for one execution with NULL decision_id and with an actual approval decision_id (or two approval IDs), bypassing the previous uniqueness. D3 explicitly preserves other event-kind uniqueness and exempts only explicitly bound approval events. The shape check at14336 also requires binding only for opened/approved, so `http.approval_rejected`/`http.approval_expired` (and mcp/message equivalents) can still be inserted unbound despite D3's exact provenance requirement.
- Correction: constrain decision-bound event kinds to the approved audit family, require decision_id for that entire family including rejected/expired, and preserve existing unrelated/reconciliation uniqueness semantics. Add direct SQL negative cases for ordinary-kind-with-decision, unbound family rejection/expiry, and duplicate unrelated events, plus valid per-decision approval history controls. No DB mutation was attempted by reviewer; this follows directly from the index/trigger predicates.
- Instruction lesson: the gated rule was precise; a partial-index exception needs a matching discriminator constraint, not merely a non-null foreign key. OPEN.

## 06-F004 — Replayed approval returns a later execution successor
- Context: 2026-10-19; `implement-astra-reviewed-2.1`;06-R008; Reviewer B requested Astra/medium. Base `5b4989ae0038a82f431db60a338b7f59d9bcf25c`, artifact `1d7e5545bb22655b22c40e14aac9f49e67f41af5683699a983b7e23deb4ce4d5`, sole exclusion `plan/workflow-foundation/docs`.
- Classification: actionable defect, low, idempotent response correctness/D3 replay contract.
- Evidence: `decision_approvals.rs:183-190` returns successor=None for an approved action. `decisions.rs:391-403` exact-command replay instead calls successor_of, which reads the execution's current successor (`496-512`). After that approved execution later completes into a next step, replaying the original approval returns Some(next activation), although the original successful submission returned None. Existing new replay test only runs while the later approval is waiting, when successor is still null, and checks disposition rather than the full saved result. D3 requires old commands to return their saved result.
- Correction: preserve the original approval result on replay (distinguishing approving from rejecting/business submissions as needed), and add a regression that replays after the original execution has a successor, comparing result fields and confirming no later execution/job/wait mutation. OPEN.
- Instruction lesson: replay tests should advance state beyond the initial result and compare the whole public result; same-state duplicate tests miss temporal drift.

## 06-F005 — D3 identity/reconciliation proof cases remain uncovered
- Context: 2026-10-19; `implement-astra-reviewed-2.1`;06-R008; Reviewer B requested Astra/medium. Base `5b4989ae0038a82f431db60a338b7f59d9bcf25c`, artifact `1d7e5545bb22655b22c40e14aac9f49e67f41af5683699a983b7e23deb4ce4d5`, sole exclusion `plan/workflow-foundation/docs`.
- Classification: actionable defect, low, verification of accepted architecture.
- Evidence: `action_approval_continuation_tests.rs:86-93,141-214` distinguishes operations by changing argument body; it does not test D3's same arguments with different targets/operations, nor identical frozen operation with different model call IDs. The failure-after-continuation test (`365-414`) covers automatic retry but never enters operator/reconciliation schedule/claim-retirement witnesses after a nonzero continuation. Existing reconciliation fixtures still use continuation_count=0, and test SQL witness helpers continue equating retired attempt with retry_count (e.g. `action_reconciliation_state_witness_validate.sql:5,132,143,148,162-163`). Broad passing tests cannot detect ordinal/failure-count confusion on that new state.
- Correction: supply the explicit operation/target identity matrix promised by D3, and a real continuation→failure→operator/reconciliation resume scenario asserting retired ordinal, witness associations, next claimed ordinal, unchanged failure/continuation accounting on explicit reopen and stale fence refusal. Update affected test witness helpers to distinguish ordinal from failure count where reused. Run focused cases and affected reconciliation checks; these obligations do not require adding runtime/token work. OPEN.
- Instruction lesson: existing broad families with zero-valued new dimensions cannot substitute for a nonzero integration case. D3 already requested these proofs.


## 06-R009 dispositions — 06-F003, 06-F004 and 06-F005 resolved
- Recheck artifact `92cde7ae8804f314f0eb6308a845de4c2d8efdf02be70432949a9ee6e051df46`, base `5b4989ae0038a82f431db60a338b7f59d9bcf25c`, sole exclusion `plan/workflow-foundation/docs`. Independent Reviewer B, requested Astra/medium; effective telemetry unavailable.
- **06-F003 RESOLVED:** the bidirectional `workflow_approval_event_kind` CHECK binds exactly the eight approval event kinds to a non-null decision. Ordinary events cannot enter the exception, all rejection/expiry events require binding, and existing exact decision FK/append-only guards remain. `action_approval_review_tests.rs:44-97` proves ordinary-kind+decision refusal, all eight unbound-kind refusals, ordinary duplicate refusal and genuine multi-decision history.
- **06-F004 RESOLVED:** replay reads immutable decision choice and returns the original None successor for action approval/approve. Other saved decisions retain their existing successor behavior. Regression at `action_approval_review_tests.rs:10-41` advances through a later rejection/successor and verifies the original revision/result and unchanged durable state.
- **06-F005 RESOLVED:** tests at `action_approval_review_tests.rs:100-299` cover real approval→failure→operator reopen with retired ordinal2 versus failure debit1, exact schedule/state associations, next claim3, stale fence refusals and unchanged reopen counters; identical operation/different model call ID reuses one receipt/dispatch; identical arguments with different target or published operation produce distinct digests/fresh decisions. State witness SQL now uses attempt_ordinal-1 for retired identity while retaining retry_count for failure eligibility. Focused final approval32/0. Broader correction gate remains pending, separately recorded in06-R009.

## 06-F006 — Duplicate capability IDs in shared action context — resolved
- Context: author-discovered during F005 proof implementation, independently inspected in06-R009 at the artifact above. Classification: concrete low correctness defect, resolved before review return.
- Previous `http_runtime.rs:62-76` projected one ceiling capability per tool, so distinct published operations sharing a capability produced duplicate capability IDs and freeze rejected an otherwise valid request. The shared context is used by HTTP, MCP and message request preparation.
- Correction collects capability IDs into a BTreeSet before constructing the ceiling. This preserves the authority set, adds no capability, and does not change frozen tool ordering. The F005 distinct-published-operation regression exercises the affected production request path. Final focused approval32/0. No open code finding remains; broader required verification is not waived.


## 06-F007 — D4 approval-time reservation conflicts with native fence and recovery ownership
- Context: 2026-10-19; `implement-astra-reviewed-2.1`;06-R011 architecture review; independent Reviewer B requested Astra/medium, effective telemetry unavailable. Decision06-D4 revision1 SHA-256 `ed282ae6c42f539546d1bd1fca0ce1557852a29ae41629aa70cd6933a2ae360b`; accepted implementation artifact `92cde7ae8804f314f0eb6308a845de4c2d8efdf02be70432949a9ee6e051df46`, base `5b4989ae0038a82f431db60a338b7f59d9bcf25c`, sole exclusion `plan/workflow-foundation/docs`.
- Classification: actionable architecture defect, medium, checkpoint ownership and exactly-once provider-entry authorization. OPEN.
- Evidence: D4 Token facts/pre-I/O identity says “On approval, reserve that exact request atomically with model-call debit before provider I/O”, while Exact new associations requires that “only an exact live WorkflowFence may mutate or use” the checkpoint and native writes require worker/generation/attempt. Actual successful approval (`decision_approvals.rs:122-134,160-198`) has already retired/cleared the parked worker and reopens the job as pending with a continuation debit; it has no live fence. Existing driver (`rig/execution.rs:447-509`) creates a request ID, persists Reserve, then enters provider; existing reservation replay (`runs/state.rs:443-484`) returns an unchanged ledger for the same ID. That replay cannot by itself distinguish a newly authorized blocked request from a request already entered by an earlier crashed worker. D4 requires no duplicate provider entry, so these conflicting boundaries cannot be left to implementation inference.
- Required correction: specify exactly what approval writes versus what the freshly claimed worker writes; which transaction consumes the immutable blocked proposal and charges the model-call/token ledger; and which admission result permits provider entry. Explicitly distinguish new admission, exact committed replay, and unresolved prior reservation. Preserve current shared allowance/refusal and live-fence rechecks at actual admission, including sibling consumption between approval and claim. Add proof obligations for crash after approval/before claim, after reservation/before I/O, after provider entry/before settlement, and competing resume/approval replay, asserting provider counts and retained charges. Do not waive native fence ownership or make an old reservation replay a second-entry permit.
- Author acknowledged the ambiguity during review and stated intended approval-only authorization followed by fresh fenced reservation. This intention is not yet in the immutable reviewed decision and does not resolve this finding until its corrected revision is checked.
- Instruction lesson: architecture gates for human continuation must name both the human command boundary and the later worker admission boundary; “on approval” cannot stand for both when the old lease is retired.


## 06-R012 disposition — 06-F007 resolved
- Decision06-D4 revision2 SHA-256 `ffb176f2446a21cefd438c289aa357f6144f3f3921747df2a3b44248afc848b2`; independent Reviewer B, requested Astra/medium; effective telemetry unavailable. Revision1 retained, no dependent implementation delta.
- **06-F007 RESOLVED at architecture level.** Human approval only changes exact decision/wait, authorized total and pending job/continuation count; no checkpoint mutation, reservation, debit or provider permit. Fresh native claim and current fence own execution reservation and request admission. Atomic admission consumes the blocked pointer, writes checkpoint/usage/root debit and active request owner; current allowance/refusal, hard bounds, fingerprint and lease are rechecked. Sibling usage can require a new proposal without charging the blocked request.
- Private non-clone NewAdmission is issued only for fresh definitely committed admission. PreviouslyAdmitted/replay/ambiguous acknowledgment cannot enter provider. Exact generation/attempt/worker active-request slot excludes parallel same-generation admission; later genuinely claimed owner can release the old slot only after retirement proof, retains unknown charges and uses a different request ID for any subsequent model call. Settlement requires matching live owner; saved turns/effect receipts are reused. The decision explicitly declines exactly-once inference for responses lost before durable settlement.
- Required real-provider crash matrix covers approval-before-claim, reserve-before-I/O, entry-before-settlement, saved-turn recovery, same-fence/claim contenders, sibling consumption/refusal and ambiguous acknowledgments with exact IDs/counts/charges. These proofs remain implementation acceptance obligations; architecture resolution is not evidence they already pass.


### 06-R013 architecture disposition — no new findings
2026-10-19; `implement-astra-reviewed-2.1`; [06-R013](agent-review-notes.md#06-r013--d4-revision3-root-allowance-scheduling-architecture-gate--pass) independently passes06-D4 revision3 addendum SHA-256 `ab2be4fe23c66e31d53296a5d40629cb31a45b5516b3b80e208ac71f774fd3c7`. No concrete issue identified;06-F008 remains unused. F007 remains resolved at architecture level. Pending implementation and its verification are not accepted by this decision-only review.


## 06-F008 — D5 puts publication-only snapshot equality in the shared admission/binding authority check
- Context: 2026-10-19; `implement-opus-reviewed-2.1`; [TASK](TASK.md), [REVIEW](REVIEW.md); batch 06B2b, architecture gate (pre-implementation); round [06-R014](agent-review-notes.md#06-r014--06b2b-d5-agent-owned-capabilities-architecture-gate--changes-required); Reviewer B Opus 5.5 (`claude-opus-5-5`), requested xhigh, effective effort not reported. Decision [06-D5 revision1](logs/06/decision-D5.md) SHA-256 `a5ef1a3f7a3c57495a0b58f495c2ad431c250615ebd43a4445bfd61a784cdb4a`; base `76fe99c93266c73ef4d13b2b10ff40df89580afb`.
- Classification: actionable architecture/requirements defect, **high**; originates in the gated decision. OPEN.
- Evidence: D5 says "`supported_publication` replaces its blanket agent/skill refusal: ... lock each referenced agent row and its skill rows FOR SHARE, re-derive ... require exact equality". `authority.rs:93-133` is not publication-only. It also runs on every binding save (`bindings.rs:42` → `verify_selection` `:117`), on every new admission including child admissions (`admission.rs:40` → `verify_current` `:124`), and on admission replay (`admission_replay.rs:88`). It also walks the whole child closure (`authority.rs:97-131`). Exact equality with *current* rows at those sites means one post-publication agent edit refuses every later binding activation, admission and replay of every published version (and every parent embedding a child) that references the agent. Such edits include a prompt change, model change, added tool or newly linked skill. User decision 3 says the opposite (freeze at publication, republish picks up edits, only revocation applies at execution). So does D5's own "Additions after publication have no effect until republish". Child admission also already holds the parent run and execution locks (`admission_source.rs:95-102`), so agent/skill row locks taken inside `supported_publication` there run local→authority. That breaks D5's stated authority→local lock order. Children are already immutable published versions: `definitions.rs:208-229 verify_children` checks the stored content hash, so they need no capture re-verification.
- Required correction: apply provenance equality only to the root bundle being newly published, inside `publish`. For binding save, admission (root and child) and admission replay, decide explicitly what checks agent-related facts. Either nothing (rely on runtime `current_on`) or a revocation-only subset check. A subset check must be placed in the authority class before any local run/execution lock, or explicitly justified. Keep the existing tool registration checks as they are. Add regressions: publish, then edit the agent (add a tool, change the prompt/model, link a skill). After that, binding save, root admission, child admission and replay of the old version must still succeed, and runtime still uses frozen content. Also prove that a revocation has the specified effect.
- Instruction lesson: the decision changed a function by name without tracing its callers. The existing guidance (`graft callers <sym> --depth all` before changing shared symbols) applies to gate authors as well as implementers. Candidate (architecture gates): when a decision changes a named shared function, list its call sites and the intended behaviour at each. Evidence: this finding.
- Resolution history: 2026-10-19 OPEN (06-R014).


## 06-F009 — D5 capture has no catalogue source for runtime-selected agents
- Context: as 06-F008 (06-R014, D5 revision1 SHA-256 `a5ef1a3f…cdb4a`, base `76fe99c`).
- Classification: actionable requirements/completeness defect, **medium**; gated decision. OPEN.
- Evidence: the accepted 02/06A contract lets `with.agent` be a runtime binding that chooses from a frozen finite catalogue. See `publication/mod.rs:58-96 prepare_step_inputs` ("reject agent selectors outside the published finite catalogue") and `validation.rs:213-232` ("Supply the selected agent snapshot, or a finite catalogue for a runtime selector"). It is tested by `publication/tests.rs:147-173` (`{"ref":"/params/agent"}`). The phase plan's own canonical example uses `agent: { ref: "/params/support_agent" }`. D5 captures snapshots only "for every agent key referenced by the draft's agent.run/decision.agent steps". The SQL owner locks "each referenced agent". Neither defines which agents form the catalogue for a non-literal selector. So such workflows either cannot be published (empty catalogue → `publication.agent`), or catalogue membership and the SQL equality set are left to implementation inference. If only literal references are re-derived, extra AgentSnapshots in a forged bundle are not covered either.
- Required correction: name the authoritative source of the runtime-selector catalogue. For example, an explicit authoring selection of agent keys given to the directory, bounded by `MAX_DEPENDENCIES`, with each entry captured and verified like a literal reference. Otherwise remove runtime agent selection explicitly and get the user's confirmation, since the phase plan example uses it. The SQL owner must verify every AgentSnapshot in the bundle and refuse SkillSnapshots not linked to a captured agent. Add a publication test for a parameter-selected agent and a forged extra-agent test.
- Instruction lesson: cause is a missed existing contract. No new instruction is supported beyond 06-F008's caller/contract trace.
- Resolution history: 2026-10-19 OPEN (06-R014).


## 06-F010 — Directory composition, tool deduplication and the HTTP grant resolution rule are unspecified
- Context: as 06-F008.
- Classification: actionable architecture/correctness defect, **medium**; gated decision. OPEN.
- Evidence: a definition service holds exactly one `PublicationDirectory` (`lifecycle/mod.rs:29-32`, `lifecycle/definitions.rs:196`). No production composition exists today. The HTTP directory emits a snapshot per explicit selection, carrying the author's slot (`http_publication.rs:30-48`). The message directory always emits both builtins (`message.rs:477-483`). Preflight refuses duplicate `(connection, name)` contracts (`preflight.rs:145-161`). Selection requires exactly one snapshot per tool name (`validation.rs:131-140`). D5 says the agent directory "composes with" these but defines neither merge semantics nor whether an HTTP grant is resolved by name across company registrations (connection `None`, `registered_on` style) or must be one of the HTTP selections. Then take a workflow that both uses a tool directly and grants it to an agent (`message.send`, or an HTTP operation selected with slot S). It fails as a duplicate contract, or the name becomes ambiguous and the agent's selection fails. D5 also does not say whether the SQL owner applies the same none/several rule as capture. The SQL owner's `registered_on` disambiguates by exact snapshot (`http.rs:45-51`), so it can accept a bundle that capture would refuse as ambiguous. The phrase "each agent's tools equal its selection universe" does not define this either.
- Required correction: specify composition: a union with exact-snapshot dedup, with an actionable refusal for name-level conflicts. Specify the HTTP and MCP grant resolution inputs, and make capture and the SQL owner apply the same rule. Add a test that a tool used by a direct step and also granted to an agent publishes and resolves to one snapshot.
- Instruction lesson: none supported beyond the "One decision, one place" rule already in `src/AGENTS.md`.
- Resolution history: 2026-10-19 OPEN (06-R014).


## 06-F011 — Existing full-replacement agent and skill writers would wipe the new fields
- Context: as 06-F008.
- Classification: actionable correctness/failure-mode defect, **medium**; gated decision (affected contracts). OPEN.
- Evidence: agent update replaces every column from `AgentWrite` (`persistence/agent.rs:485-514`). Grant vectors default to empty (`use_cases/agent.rs:43-82`). Only `config_json` and `response_contract` are preserved when absent (`persistence/agent.rs:434-448`). Skill update likewise replaces all columns (`persistence/skill.rs:364-367`). D5 adds `agents.workflow_tools` and `skills.required_tools` but defers editing UI to phase 09. It does not say that existing settings, library, onboarding and skill writers keep stored values they do not carry. Without that, any unrelated save clears an agent's workflow grants or a skill's declared requirements. Under D5's runtime rule, a cleared grant terminally rejects in-flight runs, and a cleared requirement would also trip the "unchanged required tools" check.
- Required correction: specify preserve-on-absent (an explicit typed "replace" versus "unchanged") for both writers. Add create→update→re-read round-trip tests proving an unrelated agent or skill save keeps `workflow_tools` and `required_tools`, as `src/adapters/persistence/AGENTS.md` step 3 and its round-trip rule require.
- Instruction lesson: `src/adapters/persistence/AGENTS.md` already covers this (UPDATE SET list, round-trip). Gate authors should apply it when deferring UI to a later phase. No new instruction.
- Resolution history: 2026-10-19 OPEN (06-R014).


## 06-F012 — The skill revocation predicate treats edits as revocations and lacks a stable skill identity
- Context: as 06-F008.
- Classification: actionable requirements-alignment defect, **low**; gated decision. OPEN.
- Evidence: the D5 runtime check requires "every frozen skill is still linked to the agent with unchanged required tools". Required tools are declared requirements, not grants (user decision 2; phase plan: "Skill selection never silently grants tools"), and the frozen skill text is what executes. Changing them is an edit, which user decision 3 makes effective only on republish, yet the clause terminally rejects in-flight runs on benign edits such as removing a requirement. `SkillSnapshot` has no UUID (`publication/types.rs:63-68`), unlike `AgentSnapshot.agent_id`. So "still linked" can only be matched by slug, which is mutable (`persistence/skill.rs:364`). Skill slugs may start with a digit (`skills_slug_format`), which `TypeName` refuses (`domain/workflow/ids.rs:15-24`). D5 does not specify that mapping or its error.
- Required correction: the revocation predicate should cover only capability removal: agent still owned, frozen tools ⊆ current `workflow_tools`, frozen skills still linked by stable identity, frozen model still offered. Either drop the required-tools clause or justify it as an intentional fail-closed rule with user agreement. Add the skill UUID to `SkillSnapshot`. Define the slug→`TypeName` mapping and an actionable refusal when it fails.
- Instruction lesson: none supported.
- Resolution history: 2026-10-19 OPEN (06-R014).


## 06-F013 — Runtime revocation call sites: preparation lock order and replay of an already-dispatched invocation
- Context: as 06-F008; WIP inspected only as integration context, not reviewed.
- Classification: actionable architecture clarification, **low**; gated decision (claims about call sites). OPEN.
- Evidence: D5 says `current_on` is "already inside model admission before local locks, and at every PrepareInvocation". It extends that function's lock set to agent_skills and skill rows. The WIP preparation path also calls `current_on`, but after `runtime::facts` has taken `workflow_runs ... FOR UPDATE` (`http_runtime.rs:16-20`, `agent_preparation.rs:119-135`), which is local→authority. D5 neither names nor orders this site. The WIP checks PrepareInvocation authority on replay too (`checkpoint_updates.rs:10-14`). After a crash between ActionService dispatch and InvocationResult, a revocation therefore refuses before the existing receipt or uncertainty is consumed. D5 says "terminal agent.rejected" but not how that interacts with D4's rule that uncertain effects stay owned by recovery and reconciliation.
- Required correction: list every `current_on` site, including preparation, with its lock position. State that revocation refuses only *new* model admission and dispatch: replay of an invocation that already has an action marker or receipt defers to the existing receipt and unknown-effect finality and is never re-dispatched or dropped. Add a test for revocation after dispatch and before receipt consumption.
- Instruction lesson: none new; D4's "uncertain effects remain blocked" already applies.
- Resolution history: 2026-10-19 OPEN (06-R014).


## 06-F014 — D5 verification plan omits required regression, parity and competing-claimant proofs
- Context: as 06-F008.
- Classification: actionable verification defect, **low**; gated decision. OPEN.
- Evidence: the D5 plan lacks the following.
  - (a) The 06-F008 regression: an old version is still admitted and bound after an agent edit, including child admission.
  - (b) Phase acceptance "An agent with no saved capabilities receives none" (empty tools and skills).
  - (c) An actionable publication error for a skill requirement missing from the agent's tools (the decision states it, but the plan does not test it).
  - (d) The 06-F010 composition and dedup case.
  - (e) The 06-F011 preserve-on-absent round trip.
  - (f) Rust `TypeName` ↔ SQL `workflow_tools`/`required_tools` shape parity. `src/adapters/persistence/AGENTS.md` requires one generated definition or a DB-backed equivalence matrix when a rule lives in both.
  - (g) Competing claimants for the changed runtime protocol (root `AGENTS.md`). One test holds an agent update or skill relink under `FOR UPDATE` while model admission or PrepareInvocation waits, in both orders. Another runs a skill relink concurrently with publication. The plan's "competing agent edit during publication" covers only part of this.
  - (h) The 06-F013 replay-after-dispatch case.
  The publication runtime-selector case is covered by 06-F009.
- Required correction: add these to the focused per-batch checks. Keep the final fmt, offline all-target, strict Clippy, fresh migration, SQLx and full phase06 gate.
- Instruction lesson: the root `AGENTS.md` competing-claimant rule and the persistence Rust/SQL parity rule exist. Gate verification plans should list them explicitly. No new instruction.
- Resolution history: 2026-10-19 OPEN (06-R014).


## 06-F015 — Optional suggestions for D5 (non-blocking)
- Context: as 06-F008.
- Classification: **optional suggestions**, none acceptance-blocking. OPEN (author's discretion).
- (1) Check at publication that the company model connection offers the frozen provider/model, so an actionable error appears before runtime refusal. Model connection replacement validates only current agents (`persistence/company.rs:693-716`), not frozen versions.
- (2) State whether `decision.agent` uses the agent's tools. If it does not, capture can skip resolving its tools, avoiding spurious publication failures (for example an MCP grant without a slot in this workflow).
- (3) State that agent tool snapshots join the bundle's shared tool universe, which direct `tool.call`/`mcp.call` steps also select from. Alternatively, keep a per-agent tool mapping.
- (4) Name the legacy `agent_mcp_selections` table (`init_schema.sql:4433`) in the 06C removal obligations, and state that it does not feed `workflow_tools`.
- (5) Define a canonical bundle-level ordering of SkillSnapshots, so SQL-owner equality is deterministic when agents share skills.
- (6) State that `agent_skills` is mutated only under the agent row `FOR UPDATE` (`persistence/agent.rs:402-421,475`), since D5's `FOR SHARE` serialization relies on that invariant.
- Resolution history: 2026-10-19 recorded (06-R014).


## 06-R015 dispositions — 06-F008 to 06-F014 RESOLVED at architecture level
- Context: 2026-10-19; `implement-opus-reviewed-2.1`; correction recheck [06-R015](agent-review-notes.md#06-r015--06b2b-d5-revision2-correction-recheck--pass); same Reviewer B, Opus 5.5 (`claude-opus-5-5`), requested xhigh, effective effort not reported. Decision [06-D5 revision2](logs/06/decision-D5.md), SHA-256 `eed25e4a5f367ccfd8333039455402d8bea59a4274b361854a398a1a5e084934`, verified before and after. Revision1 is retained unchanged as `decision-D5-revision1.md` (`a5ef1a3f…cdb4a`). Base `76fe99c93266c73ef4d13b2b10ff40df89580afb`.
- **06-F008 RESOLVED.** Equality now runs only in the `definitions.rs` publish transaction, after the company-first locks, and covers only the new root bundle's own snapshots. `supported_publication` keeps its existing tool-registration checks at binding save, admission, child admission and replay, and does no agent/skill comparison or row locks there. This matches the call sites I re-read (`bindings.rs:42,117`, `admission.rs:40,124`, `admission_replay.rs:88`). Child bundles are no longer re-derived (they are hash-verified by `definitions.rs:208-229`). The stated lock order now has no agent/skill locks in admission. The plan includes "old version still binds and admits after an agent edit". Child admission and replay share that function, so one admission regression covers them.
- **06-F009 RESOLVED.** A literal names one key. A non-literal selector must resolve to a finite string enum of agent keys, all captured; otherwise publication refuses, so the rule fails closed. The SQL owner verifies every agent and skill snapshot, not only literal ones. `prepare_step_inputs` (`publication/mod.rs:61-96`) still confines runtime selection to the catalogue, so an imprecise projection cannot widen authority. The plan covers literal, enum-ref and refused non-finite selectors. The projection mechanism is noted in optional 06-F016.
- **06-F010 RESOLVED.**
  - One capture runs the step-direct directories, then the agent capture, and merges tools by `(name, slot)`: byte-identical snapshots dedup, and conflicting snapshots or names that resolve differently refuse.
  - HTTP, MCP and message grant rules are stated, and capture shares them with the SQL owner.
  - The existing `unique_contracts`/`has_tool` checks still apply after the merge. MCP and message builtins dedup byte-identically.
  - An HTTP operation selected for a direct step under a slot and also granted name-only refuses explicitly rather than silently.
  - The plan includes the dedup case.
- **06-F011 RESOLVED.** `workflow_tools` and `required_tools` are Options, with `None` preserved through `COALESCE` in the full-row UPDATEs (checked against `persistence/agent.rs:485-514`, `persistence/skill.rs:364-367`, and `SkillWrite` at `use_cases/skill.rs:25`). Skill links change only under the agent `FOR UPDATE` lock, as already done at `persistence/agent.rs:402-421,475`. The plan includes preservation tests.
- **06-F012 RESOLVED.** The revocation predicate is now: agent still owned, frozen tools ⊆ current `workflow_tools`, frozen `skill_id` still linked, model still offered. Skill text and required-tool edits are explicitly not revocations. `SkillSnapshot` gains a trusted `skill_id`. A slug that is not a valid `TypeName` (including a digit-leading one) is refused at capture with an actionable error. Skill order is defined.
- **06-F013 RESOLVED.** `current_on` runs at model admission and at every PrepareInvocation, including replay, before local locks. In the WIP these are `checkpoint_models.rs:22` and `checkpoint_updates.rs:12-15`. `prepare_agent` no longer calls it, which removes the call after `workflow_runs FOR UPDATE` (`http_runtime.rs:19`). A refused replay of a dispatched invocation leaves its marker, receipt or uncertainty with ActionService recovery and the shared retry predicate, and records no result. This is consistent with D4 revision2/revision3 effect ownership.
- **06-F014 RESOLVED.** The plan now includes: pure Rust/SQL shape parity, merge, finite universe and skill order; capture refusals (missing requirement, model or connection, legacy tool step, digit-leading slug, unregistered or ambiguous HTTP/MCP); an agent with no tools or skills; a forged snapshot; real concurrent agent-edit and skill-relink publishes; old-version bind and admit; writer preservation; runtime revocation including a competing race; replay after dispatch; end to end; and the final full gate. Implementation must still produce these proofs.


## 06-R015 disposition — 06-F015 optional suggestions
- (1) Adopted: model-connection availability checked at capture and in publish equality.
- (2) Partly adopted: `decision.agent` is defined as tool-free. Whether capture skips tool resolution for agents used only by `decision.agent` is not stated, and refusal stays fail-closed (see 06-F016).
- (3) Adopted: granted tools join the shared bundle universe.
- (4) Adopted: `agent_mcp_selections` and `granted_tool_ids` added to the 06C removals.
- (5) Partly adopted: per-agent order is position then `skill_id`. Bundle-level ordering of shared skills is left to the equality implementation (see 06-F016).
- (6) Adopted: the skill-link writer lock invariant is stated.
- Non-blocking; closed as recorded.


## 06-F016 — Optional residual suggestions for D5 revision2 (non-blocking)
- Context: as 06-R015 (D5 revision2 SHA-256 `eed25e4a…4934`).
- Classification: **optional suggestions**, none blocks acceptance. OPEN (author's discretion).
- (1) Finite agent universe mechanism. No existing compiler artifact gives the value set behind a reference. `Schema::guarantees`/`project` in `compiler/schema.rs:84-86,323-364` return only `PathGuarantee`. Step input schemas come from the registry descriptor, where `agent` is `identifier()` (`registry/families.rs:62,88`). So the rule needs a new pure value-set projection. Suggested semantics:
  - project the referenced root schema (input, params or step output) along the reference tail;
  - require a `Default` fallback literal to be a member;
  - take the union for anyOf/oneOf and the intersection for allOf/enum/const, following local `$ref`;
  - refuse non-reference bindings such as operators or templates.
  The function should be shared by capture and freeze validation. Capture must also get the parsed definition: `PublicationDirectory::capture` currently receives only the draft (`lifecycle/definitions.rs:196`). This is safe as written (fail-closed, with runtime guarded by `prepare_step_inputs`); it is about clarity and usability.
- (2) If an agent is referenced only by tool-free `decision.agent` steps, consider not requiring its tools to resolve. Otherwise an unrelated grant can refuse publication.
- (3) Define bundle-level SkillSnapshot ordering (for example by `skill_id`), or compare skills as a set keyed by `skill_id`, when agents share skills.
- (4) The publish SQL owner re-verifies HTTP grants with `registered_on`, which disambiguates by exact snapshot (`http.rs:45-51`). Capture's "single registration of that name" rule is stricter. Either apply the name rule at publish too or accept the snapshot-pinned routing explicitly. No authority widening either way.
- Resolution history: 2026-10-19 recorded (06-R015).


## 06-F017 — A slot-bound HTTP agent grant is prepared and dispatched as an MCP tool
- Context: 2026-10-20; `implement-opus-reviewed-2.1`; whole-batch 06B2b review [06-R016](agent-review-notes.md#06-r016--whole-06b2b-batch-review--changes-required); Reviewer B Opus 5.5 (`claude-opus-5-5`), requested xhigh, effective effort not reported. Artifact: base `76fe99c93266c73ef4d13b2b10ff40df89580afb` plus worktree, digest `6ea98c7ff7ea6f6ff35c3f7dc8005a6cade246da0179b53690aa60f78a02afa8` (excludes `plan/workflow-foundation/docs`), checked before and after the review.
- Classification: actionable correctness defect, **medium**; implementation (D5 and D4 are explicit). OPEN.
- Evidence: `agent_publication.rs:240-256 granted_tool` reuses any single already-captured snapshot with the granted name. That includes a step-direct HTTP selection carrying an author slot, which `HttpPublicationSelection.slot` allows and validation accepts for declared `http` resources (`validation.rs` `validate_tools`). `agent_preparation.rs:127-131 targets` then maps *every* slot-bound tool to `AgentToolTarget::Mcp { slot }`, and `agent.rs:208` accepts that as consistent. So the publish (where `supported_publication` correctly checks it as `http` via `slot_kind`) and the preparation both succeed. After a model turn requests the tool, `AgentActionTools` routes it to the MCP runtime, which builds an `mcp` target for the HTTP connection UUID (`mcp_runtime.rs:10-42`) and fails with not-found. The run ends terminally as `agent.rejected` after a charged model call.
- Required by: D4 ("Selected tool declarations must have a concrete ActionService dispatch binding; unsupported bindings fail preparation explicitly") and D5 revision2 (an HTTP grant resolves to the single ready registration of that name; MCP resolves only through a declared MCP slot).
- Required correction: decide target kind from the slot's declared resource kind (`resources::slot_kind`), not from whether a slot exists. Either support a slot-bound HTTP agent target through the HTTP runtime's bound connection, or refuse reuse of a slot-bound HTTP snapshot at capture with an actionable error. Add a test where the same HTTP operation is used by a direct step with a slot and granted to an agent.
- Instruction lesson: none new. A discriminating attribute was inferred from the presence of an optional field.
- Resolution history: 2026-10-20 OPEN (06-R016).


## 06-F018 — Token-allowance pauses from the agent driver are not confirmed as suspensions
- Context: as 06-F017.
- Classification: actionable correctness/integration defect, **medium**; implementation, in the disclosed untested path "token approval through the driver". OPEN.
- Evidence:
  - When model admission or settlement parks for token review, `WorkflowAgentRuntime` returns `WorkflowHandlerOutcome::Suspended(wait)` (`rig/workflow.rs:116-119,149-150,382-384`). The token decision is `subject_kind='token_allowance'` (`decision_tokens.rs:55-61`).
  - `supervise_io` confirms every suspension through `confirm_suspension` (`supervise.rs:71-81`). That tries the message-effect confirm, then `decision_approvals::confirm`, which accepts only `subject_kind='action_approval'` (`decision_approvals.rs:217`). Both return false.
  - Supervision therefore ends as `Stopped(LeaseLost)` and calls `release_io` with a lease-lost cause (`supervise.rs:103-116`). The committed park survives only because the release then fails to lock the closed fence.
  - Every user-required token pause is consequently misreported as lease loss. Root `AGENTS.md` and `src/AGENTS.md` require telemetry to distinguish lease loss from parked/stuck work. The suspension contract ("Never complete or release a parked attempt") also now depends on a failing release rather than on confirmation.
  - Store-level token tests do not cover this boundary, because they never go through `supervise_io`.
- Required correction: confirm token-allowance parks for the exact fence, wait, job and parked attempt (both reservation- and settlement-stage). Add an end-to-end test of a driver token pause → approval → fresh claim through `supervise_io` (see 06-F019).
- Instruction lesson: when a new suspension subject is added, check every generic suspension consumer. Disclosed "store-level only" coverage hid a cross-layer contract break. Candidate: a gate's proof matrix should name the worker/supervisor boundary for each new parked-state kind.
- Resolution history: 2026-10-20 OPEN (06-R016).


## 06-F019 — D4 driver-level proofs are missing
- Context: as 06-F017.
- Classification: actionable verification defect, **medium**; acceptance condition of accepted D4 ("Required proofs before acceptance"; F007 matrix "real scripted provider counts, request IDs and ledger assertions"). OPEN.
- Covered today: store-level native owner/model/token/scheduling suites (`harness_*_tests.rs`, about 40 tests), plus agent end-to-end tests for:
  - one HTTP tool dispatch and replay;
  - one protected-tool approval;
  - one message delivery wait;
  - revocations;
  - publication.
- Missing at the driver (`WorkflowAgentRuntime`/`Execution`) level:
  - (a) Token pause → approval → fresh claim consuming the blocked request through `blocked_workflow_model` and `WorkflowBinding::reservation` (no test calls `blocked_workflow_model`). Also a settlement-stage pause, sibling re-park and denial. This is the gap that hid 06-F018.
  - (b) A crash or restart after each boundary with provider-call counts: after reservation and before I/O, after provider entry and before settlement, after settlement, and after receipt.
  - (c) Two approval cycles in one run.
  - (d) A model-invented tool name (`unknown_tool` result, nothing dispatched).
  - (e) Structured-output repair and the invalid-output terminal through the workflow binding.
  - (f) A target rebind refused at re-preparation.
  - (g) `Uncertain`/`IndeterminateEffect` → terminal plus the shared reconciliation predicate.
  - (h) An MCP agent tool end to end.
  - (i) Dynamic (parameter-enum) agent capture end to end.
  The author disclosed four of these gaps. Implementing (a) would have exposed 06-F018, and covering MCP or slot-bound tools would have exposed 06-F017.
- Required correction: add these driver-level tests with scripted-provider and effect counts, request IDs and ledger assertions. Then rerun the affected suites and the batch gates.
- Instruction lesson: the skill already says passing tests and author summaries are not proof. A D4-style required proof list should be traced item by item in TASK before freezing. Candidate (author freeze checklist): map each gate-required proof to a test name, or mark it unmet.
- Resolution history: 2026-10-20 OPEN (06-R016).


## 06-F020 — The SQL no longer separates model-tool deliveries from step completion
- Context: as 06-F017.
- Classification: actionable defense-in-depth defect, **low**; implementation versus D4 ("SQL guards distinguish the two transitions using immutable origin and checkpoint association"). OPEN.
- Evidence: the init SQL drops `workflow_message_actions_check (origin='step' OR wait_id IS NULL)`, which previously guaranteed that every effect wait belonged to a step. The only origin-aware guard now is the model-tool *reopen* guard (`init_schema.sql` near 255-282). Completing an execution through an effect wait (`wait_commit.rs` `advance(Wake::Effect)`) is separated from the delivery continuation only by the Rust branch `model_tool_wait_on` (`wait_commit.rs:68-75`). No SQL refuses completing `agent.run` with a model-tool receipt. In addition, `workflow_wait_execution` now excludes `effect` waits for every origin, so step executions also lose their one-effect-wait uniqueness.
- Required correction: add a SQL guard so that completing an execution or step from an effect wait requires the wait's action `origin='step'`. Keep per-execution effect-wait uniqueness for step origin, or document why `workflow_message_actions_wait` plus `workflow_wait_one_active` suffice. Add the negative SQL tests.
- Instruction lesson: `src/adapters/persistence/AGENTS.md` ("Application checks are useful error messages, not substitutes for constraints") already covers this.
- Resolution history: 2026-10-20 OPEN (06-R016).


## 06-F021 — Optional suggestions from the 06B2b batch review (non-blocking)
- Context: as 06-F017. Classification: **optional**, none acceptance-blocking. OPEN (author's discretion).
- (1) `NewModelAdmission::committed` is `pub(crate)` (`application/workflow/checkpoints.rs:89`). D4 asks for private permit construction; narrowing it to the store module, or using a sealed constructor, makes "only fresh committed admission enters the provider" structural.
- (2) The context prompt wraps JSON in `<context>…</context>` (`application/workflow/agent.rs:94-97`). A literal `</context>` inside a JSON string is not escaped. Escaping `<`, for example as `<`, would harden the data boundary. Tool approval remains the security boundary.
- (3) `agent_publication::derive_on(…, lock: bool)` is a behaviour-selecting flag parameter (`src/AGENTS.md` "No flag parameters"). Two named entry points, or a `Locking` enum, would read better.
- (4) The native checkpoint keeps a NO ACTION FK to `agents(company_id,id)` (as legacy rows do). So an agent that has run a workflow cannot be deleted, and `persistence/agent.rs:733-746` reports it as "active on an enabled channel". Consider an accurate message or a retention decision (pre-existing pattern, now reachable from workflows).
- Resolution history: 2026-10-20 recorded (06-R016).


## 06-R017 dispositions — 06-F017 to 06-F020 RESOLVED; 06-F021 dispositioned
- Context: 2026-10-20; `implement-opus-reviewed-2.1`; correction recheck [06-R017](agent-review-notes.md#06-r017--06b2b-r016-correction-recheck--pass); Reviewer B Opus 5.5 (`claude-opus-5-5`), requested xhigh, effective effort not reported. Artifact: base `76fe99c93266c73ef4d13b2b10ff40df89580afb` plus worktree, digest `d9ea9e59570ffd34142fc18c7d6542ad5e2ca93ba7ee4755e85110ab544b2ef3` (excludes `plan/workflow-foundation/docs`), verified before and after.
- **06-F017 RESOLVED.**
  - `agent_preparation.rs:127-136` maps a slot-bound tool to an MCP target only when `resources::slot_kind` is `mcp` and the slot is bound. Anything else fails preparation as unsupported.
  - Capture (`agent_publication.rs:268-296 granted_tool`) reuses a captured snapshot only when it has no slot or an MCP slot. It refuses a slot-bound HTTP step selection granted to the agent.
  - Covered by `workflow_agent_capture_refuses_slot_bound_http_grant` (passed in B's run). `workflow_agent_mcp_tool_dispatches_once_through_the_bound_slot` proves the remaining MCP route end to end.
- **06-F018 RESOLVED.**
  - `decision_approvals.rs confirm` accepts `subject_kind IN ('action_approval','token_allowance')`. The exact job, parked attempt, worker, generation and wait witness is unchanged.
  - Proven through `supervise_io` at both the settlement and reservation stages, with negative confirms for a random wait and for an earlier wait under a later fence (`…settlement_pause_and_second_pause_resume_exact_requests`, `…token_denial_never_enters_the_provider`).
- **06-F019 RESOLVED.** I checked each item of the proof map against test names and spot-read the assertions in the token-pause, settlement/second-pause and crash-during-effect tests:
  - (a) `token_pause_resumes_with_the_saved_request` checks that the admitted request ID equals the approved `token_request_id` and that the provider was called once. The settlement and second-pause test, and the denial test, are also present.
  - (b) restart after reservation, after provider entry, after settlement and after receipt;
  - (c) two approval cycles;
  - (d) model-invented tool;
  - (e) repair once, and invalid output after repairs;
  - (f) rebound target, now terminal by design (`checkpoints.rs` `matching` → BadRequest, which matches D4's immutable preparation);
  - (g) unanswered effect and crash during effect: `effect_unknown`, then reconciliation with one dispatch and no claimable retry;
  - (h) MCP end to end;
  - (i) dynamic selection end to end.
  Sibling consumption between approval and claim remains store-level only (`workflow_harness_token_sibling_usage_reparks_exact_request_without_debit`). This is accepted: the driver path on a fresh claim is the same admission→Parked branch that the second-pause test proves.
- **06-F020 RESOLVED.**
  - The deferred constraint trigger `workflow_model_tool_wait_continuation` (`init_schema.sql:11816`, function at `:4347`) refuses, at commit, completing a model-tool effect wait unless its execution is still open with no committed route and its job is pending again. So a model-tool receipt can only continue the agent job, never complete the step.
  - `workflow_message_step_one_wait` (`:10632`) restores one effect wait per step execution.
  - Negative tests: the continuation-guard refusal inside `…message_tool_parks_on_delivery_and_continues_with_its_receipt`, and `workflow_message_sql_step_execution_keeps_one_effect_wait`. Both passed in B's run.
- **06-F021:**
  - (2) adopted: `agent.rs:94-100` escapes `</` as `<\/`. That is the same JSON string, and `<` occurs only inside strings.
  - (3) adopted: the `Rows::{Read, Shared}` enum replaces the bool.
  - (1) not adopted. The reason is accepted: the permit is non-clone and consumed once, and `pub(in …)` cannot reach the adapter store from `application`.
  - (4) deferred to 06C with agent deletion and retention.
  None blocks acceptance; closed as recorded.


## 06-F022 — Optional: agent driver test file exceeds the file-size guideline (non-blocking)
- Context: as 06-R017 (digest `d9ea9e59…2ef3`). Classification: **optional**, maintainability. OPEN (author's discretion).
- `src/adapters/persistence/workflow/agent_runtime_tests.rs` is 1043 lines. `src/AGENTS.md` says to split files past about 1,000 lines. The recovery proofs already moved to `agent_recovery_tests.rs`; the approval/token driver cases could follow.
- Resolution history: 2026-10-20 recorded (06-R017).


## 06-F023 — The context.history item contract drops each message's disclosure boundary
- Context: 2026-10-20; `implement-opus-reviewed-2.1`; architecture gate 06B3, round [06-R018](agent-review-notes.md#06-r018--06b3-d6-preparation-steps-architecture-gate--changes-required); Reviewer B Opus 5.5 (`claude-opus-5-5`), requested xhigh, effective effort not reported. Decision [06-D6](logs/06/decision-D6.md) SHA-256 `064dbff7c66fcbcb35a2261112630c03fa907802d0fc49488ceec25e8ca73ecd`, verified before and after; base `76fe99c93266c73ef4d13b2b10ff40df89580afb` plus the accepted 06B2b worktree.
- Classification: actionable requirements/security (privacy) defect, **medium**; originates in the gated decision. It is costly to unwind because the output contract is compiled into versioned bundles. OPEN.
- Evidence:
  - D6 A fixes items as `{message_id, role, author, subject, body, truncated}` and excludes only internal notes.
  - Thread messages also carry `messages.audience` ∈ `external_conversation | internal_only | legacy_unclassified` (`init_schema.sql` `messages_audience_check`) and `thread_messages.entry_kind` ∈ `conversation | note | delegation | system_event`.
  - Legacy agent history selects both (`persistence/thread/views.rs:393-427`). The legacy prompt labels every entry "External conversation", "Internal-only" or "Legacy unclassified; do not disclose", plus its entry kind (`agent_runner/prompt.rs:131-141`).
  - Under D6, internal-only delegation and system messages and unclassified legacy messages reach a downstream `agent.run` (for example one drafting a customer reply) indistinguishable from external conversation. That loses a disclosure boundary the current runtime gives every agent.
- Required correction: add typed `audience` and `entry_kind` enums to each item, mirroring the legacy labels, and keep them in the frozen output. Alternatively, exclude non-external messages by an explicit, user-confirmed rule; `omitted` must still report them. Add DB tests for each audience and entry kind.
- Instruction lesson: when replacing a legacy reader, carry over every field the legacy consumer uses for a safety decision, not only the fields of the obvious render. Candidate (gates replacing a legacy path): list the legacy reader's columns and say what happens to each. Evidence: this finding.
- Resolution history: 2026-10-20 OPEN (06-R018).


## 06-F024 — `max_tokens` can exceed the run's per-output byte limit
- Context: as 06-F023.
- Classification: actionable correctness defect, **medium**; gated decision. OPEN.
- Evidence:
  - D6 fixes `max_tokens: 1..131072` and counts one serialized UTF-8 byte as one token. One item can be about 68 KB (a 16384-character body is up to 64 KiB of UTF-8, plus a 2048-byte subject and the author).
  - Every step output is bounded by the run's `limits.max_context_bytes` (1..1 MiB; fixtures use 65536). `graph.rs:56-63` is the per-binding output bound, and `lease_claim.rs:80,133` sets `max_result_bytes = max_context_bytes`.
  - An oversized output stops supervision with `OversizedResult` → `workflow.invalid_result`, which is **terminal** (`supervise.rs:86,148`, `recovery.rs:151-153`).
  - So a valid workflow (for example `max_tokens: 100000`, `max_context_bytes: 65536`) fails terminally whenever its thread history is long, and the size of the downstream `agent.run` context input is also unbounded relative to that limit.
- Required correction: bound the history output by both `max_tokens` and the run's per-output byte limit minus the output envelope. Do it at compile time (refuse `max_tokens` above the available bytes) or by using the minimum at run time, still reporting `omitted`. Add a DB test with a long thread at the boundary.
- Instruction lesson: none new (src/AGENTS.md "Bound work at every external boundary").
- Resolution history: 2026-10-20 OPEN (06-R018).


## 06-F025 — Optional suggestions for D6 (non-blocking)
- Context: as 06-F023. Classification: **optional**. OPEN (author's discretion).
- (1) Memory kinds stay advertised and publishable but unhandled. Once a worker exists, a run reaching `memory.load` or `memory.save` waits as Unsupported until its deadline (`worker.rs:68-73,156-159`). This is the same pattern as the phase-07 child kinds. Refusing them at publication with an actionable error would be more direct, and is reversible. The phase-plan memory acceptance bullets should be recorded as deferred by the user's decision.
- (2) D6 E updates two separate kind lists (`recovery.rs:138-144` and `pending_recovery.rs:166-175`) that already duplicate one decision. Consolidate them into one predicate beside `marker_owned_effect_kind` ("One decision, one place").
- (3) History items carry no timestamp. The legacy prompt gives one for notes; a `created_at` field would let agents reason about recency. Also state whether the admission's own triggering message is included (it is in the membership snapshot).
- (4) State whether newest-first token accumulation *stops* at the first item that does not fit, or *skips* it and keeps going. Both are deterministic and reported through `omitted`, but the contract should say which.
- Resolution history: 2026-10-20 recorded (06-R018).


## 06-R019 dispositions — 06-F023 and 06-F024 RESOLVED; 06-F025 dispositioned
- Context: 2026-10-20; `implement-opus-reviewed-2.1`; recheck [06-R019](agent-review-notes.md#06-r019--06b3-d6-revision2-correction-recheck--pass); Reviewer B Opus 5.5 (`claude-opus-5-5`), requested xhigh, effective effort not reported. Decision [06-D6 revision2](logs/06/decision-D6.md), SHA-256 `e80a5201f60e1b37abf4bf4faef8b3695d19928e05563af3f6e8098775dd497d`, verified before and after. Revision1 is retained unchanged as `decision-D6-revision1.md` (`064dbff7…cecd`, rechecked).
- **06-F023 RESOLVED.** Each item now carries typed `audience` (`external_conversation | internal_only | legacy_unclassified`), `entry_kind` (`conversation | note | delegation | system_event`) and `at`. These are the same facts legacy history labels (`agent_runner/prompt.rs:131-141`). Only internal notes are excluded; every other member stays eligible with its boundary as data. An unknown stored value is a terminal `context.invalid_history`. The verification plan asserts the exact audience, entry kind and `at` for each kind.
- **06-F024 RESOLVED.** An item fits only while the serialized items stay ≤ `max_tokens` *and* the complete serialized output stays ≤ the claim's `max_result_bytes`. That is the run's `max_context_bytes`, the same value `supervise_io` checks before completion (`supervise.rs:85-86`, `lease_claim.rs:80,133`). The minimum is applied at run time, so a long thread gives a larger `omitted`, never a terminal `workflow.invalid_result`. Tests at the cap and one byte over are planned, plus the case where the newest item alone does not fit.
- **06-F025 dispositioned (optional):**
  - (2) adopted: a single `effect_free_kind` predicate with a unit test over every registry kind.
  - (3) adopted: `at`, and the triggering message is included.
  - (4) adopted: stop at the first non-fitting item, giving a contiguous suffix.
  - (1) publication refusal not adopted now. The reason is accepted: no production worker exists before 06C/08. D6 F records the 06C obligation that publication or binding must refuse kinds with no production handler, and phase-plan line 151 marks the memory acceptance item as deferred by the user's decision.
- Note for implementation review: folding `data.map`, `decision.*` and `wait.*` into `recovery.rs`'s SafeToRetry set via `effect_free_kind` is correct because these kinds have no effects. The unit test must pin `memory.save` (and every action kind) outside it.


## 06-F026 — Decide's response contract embeds `data_schema` without its own resource identity
- Context: 2026-10-20; `implement-opus-reviewed-2.1`; whole-batch 06B3 review [06-R020](agent-review-notes.md#06-r020--whole-06b3-batch-review--changes-required); Reviewer B Opus 5.5 (`claude-opus-5-5`), requested xhigh, effective effort not reported. Artifact: base `76fe99c93266c73ef4d13b2b10ff40df89580afb` plus worktree, digest `b64957f986c172239336b34593bb1de360a501a5e9c6158f01e9fa3821b1fb99` (excludes `plan/workflow-foundation/docs`), verified before and after. Gate: 06-D6 revision2 (`e80a5201…497d`).
- Classification: actionable correctness defect, **low**; implementation. OPEN.
- Evidence:
  - `application/workflow/agent.rs:275-283 decision_schema` places the declared `data_schema` verbatim under `properties.data`.
  - The registry builds the same `decision.agent` output contract with `embedded(schema, "decision-data")`, which adds a `$id` (`registry/mod.rs:318-325`). That makes local references (`#/$defs/…`) inside the data schema resolve against the data schema itself.
  - Workflow schemas may use local non-recursive `$ref` (`compiler/schema.rs` `check_resources`/`NoExternalReferences`), and the response validator compiles them the same way (`adapters/response_schema/mod.rs:10-24`).
  - So a `decision.agent` whose `data_schema` uses `$defs` compiles and publishes. At run time the Decide contract's `#/$defs/x` resolves against the new root, which has no `$defs`. Contract validation in `WorkflowAgentRuntime::run` then fails, and the step ends terminally as `agent.rejected`.
  - The same schema works as an `agent.run` `output_schema`, because there it is the root.
  - This also duplicates the step output contract that D6 C says completion already enforces ("One decision, one place").
- Required correction: build the Decide response contract from the compiled step's output contract, or at least with the registry's `embedded` rule. Add a scripted-provider test with a `data_schema` that uses `$defs`.
- Instruction lesson: none new.
- Resolution history: 2026-10-20 OPEN (06-R020).


## 06-F027 — Optional suggestions from the 06B3 batch review (non-blocking)
- Context: as 06-F026. Classification: **optional**. OPEN (author's discretion).
- (1) Bounded database work. `history_reader.rs` materializes every eligible member (up to 10000), including `left(clean_text_body, 16385)` and `left(subject, …)`, because the CTE is referenced twice (count and select). Only then does it apply `LIMIT 256`.
  - Under `begin_http`'s 5 s statement timeout, a very long thread with large bodies can repeatedly fail as `context.unavailable` until its retries run out.
  - Counting eligible members without message bodies, then reading bodies only for the newest 256, bounds the work by the read. `src/adapters/persistence/AGENTS.md` says a bounded result set does not imply bounded database work.
  - If this is deferred pending production evidence, add the duration or working-set metric that AGENTS.md asks for.
- (2) `select` re-serializes the whole output for every candidate item (`history.rs` `size(&output(...))`). That is O(n²) up to 256 × the result cap. An incremental envelope count gives the same result.
- (3) D6 said `context.history` "takes no locks beyond its read". The reader uses `runtime::facts`, which takes the actor prelock and `workflow_runs … FOR UPDATE`. That matches other I/O preparation and adds current actor authority, so either note it in the decision record or use a share lock. Relatedly, an actor-authority refusal surfaces as retryable `context.unavailable`, whereas model steps fail terminally as `agent.rejected`.
- Resolution history: 2026-10-20 recorded (06-R020).


## 06-R021 dispositions — 06-F026 RESOLVED; 06-F027 dispositioned
- Context: 2026-10-20; `implement-opus-reviewed-2.1`; correction recheck [06-R021](agent-review-notes.md#06-r021--06b3-r020-correction-recheck--pass); Reviewer B Opus 5.5 (`claude-opus-5-5`), requested xhigh, effective effort not reported. Artifact: base `76fe99c` plus worktree, digest `afbb5ffb62f5e9cf019323611f2133cc6ae6f24644dda2104acc6ac42f2c3717`, verified before and after.
- **06-F026 RESOLVED.**
  - Decide's response contract is now exactly the compiled step output contract (`CompiledWorkflow::step_output_schema`, `agent_preparation.rs:188-198`, `PreparedStep::Decide { contract }`). The hand-built schema and the choice loop are gone. This is one decision in one place, and the contract is the same object completion enforces.
  - The broadened shared fix, `services::tool_schema::visit`, was checked for safety:
    - Re-rooting at a nested `$id` matches 2020-12 resolution, the validator, and the compiler's `schema_budget`.
    - Every visit, including `$ref` expansion, still consumes the depth (≤32) and node (4096) budgets, and the 65,536-byte size check is unchanged.
    - Fragment-only references can resolve only inside the enclosing resource, so a reference cycle is re-entered under the same root and exhausts the budget. Recursion is still refused.
    - Every node is also visited through ordinary child traversal under its correct enclosing root. So a pointer that reaches into a nested resource from outside cannot hide a cycle in that resource.
    - Non-fragment and `$anchor` references remain refused (stricter than the validator, fail-closed).
  - Tests passed in B's run: `workflow_decision_agent_data_schema_local_references_resolve` (end to end), `schema_local_references_resolve_inside_their_nested_resource` (also refuses the same schema without `$id`), and the verbatim-contract test in `agent_tests.rs`.
- **06-F027:**
  - (1) adopted: the eligible count reads members only; just the newest 256 join messages, principals and identities.
  - (3) adopted: NotFound and InvalidCredentials now map to Terminal `context.rejected`, consistent with model steps, with a unit test; D6's lock wording is corrected in TASK, and the standard claim locks are kept.
  - (2) not adopted; the reason is accepted, since the loop is bounded by 256 items and the result cap.
  Non-blocking; closed as recorded.


## 06-F028 — Operator and README docs still describe behaviour 06C made unreachable
- Context: 2026-10-20; `implement-opus-reviewed-2.1`; whole-batch 06C review [06-R022](agent-review-notes.md#06-r022--whole-06c-batch-review--changes-required); Reviewer B Opus 5.5 (`claude-opus-5-5`), requested xhigh, effective effort not reported. Artifact: base `76fe99c93266c73ef4d13b2b10ff40df89580afb` plus worktree, digest `c9f12d8828d1dbd5dd3b635611d98ea6760c87bab637f7ffd0551502e77a16f5` (manifest `logs/06/06C-batch-snapshot.txt`), verified before and after.
- Classification: actionable documentation defect, **low**; implementation (docs). OPEN.
- Evidence:
  - `docs/deploy.md:181`: the pilot gate "Required evidence before opening access" now requires, for MCP, "a workflow MCP action's successful receipt". 06C removed agent MCP selections, so MCP is reachable only through workflow actions. Workflow worker wiring is a recorded phase-08 obligation (TASK "06C implementation complete"; `infra/setup.rs` wires no workflow worker). So an operator following the checklist cannot produce this evidence on a deployment of this release.
  - `README.md:180-235`: the task-execution and pause diagrams still route agent runs through `HITL{Approval or outreach requested?}` and show agent-originated approval and outreach flows. 06C deleted every producer: the `request_approval`/`outreach_and_await_quorum` native tools are gone, and no production path calls `create_approval` or `create_outreach_and_pause`; only tests do.
  - Root `AGENTS.md` requires production documentation to match what the application does. The rest of the doc set was updated accurately: the `.env.example` keys are all read, and the `docs/rig.md` anchors are valid.
- Required correction: mark the MCP pilot evidence as applying once phase 08 wires workflow execution, or limit it to configuration, refresh and grant review for this release. In the README diagrams, mark agent-requested approval and outreach as retained, inactive machinery until phase 08, or remove those branches. This is a documentation-only change.
- Instruction lesson: when a removal batch removes a feature's only producer, search docs for the consumer-side flows as well, not just for the removed identifiers. Candidate (removal batches): grep docs for the removed capability's user-visible verbs (for example "approval requested", "outreach"), not only its config keys and field names.
- Resolution history: 2026-10-20 OPEN (06-R022).


## 06-F029 — Optional observations from the 06C review (non-blocking)
- Context: as 06-F028. Classification: **optional**. OPEN (author's discretion).
- (1) Dropping `guard_active_agent_harness_change` changes visible behaviour. Previously a `response_contract` edit on an agent with unsettled legacy tasks was refused. Now it succeeds, and those tasks fail closed at the next Rig `authorization()` ("Capabilities changed after dispatch preparation", `rig/execution.rs`). That is safe, no wrong-contract reply, and consistent with skill and sub-agent edits, which were never guarded. Record it as an intended behaviour change in TASK and docs.
- (2) The scheduled-run worker test now uses `StubHarness`, and its token-usage and provider-call assertions are gone. No test drives a *scheduled* run through real Rig. The shared dispatch path is covered by the PostgreSQL thread suites. Accept as a gap until phase 08 replaces legacy ingress, or add one PostgreSQL scheduled-run test with real Rig.
- (3) `McpPersistence::mcp_connections_by_ids` has no production caller after the selections were removed (only `mcp_read_tests` and workflow MCP test support). Its new bound equals the per-company connection cap and the over-cap case is tested. Remove it, or note it as reserved for phase 08.
- (4) The Agent Builder prompt asks for "the minimum workflow tools", but `agents.workflow_tools` has no editing UI until phase 09. It also says the new agent "inherits the company's model … defaults", whereas D5 workflow use needs an explicit provider and model. Consider aligning the wording until phase 08 restores creation.
- Resolution history: 2026-10-20 recorded (06-R022).

## 06-R023 dispositions — 06-F028 RESOLVED; 06-F029 dispositioned
- Context: 2026-10-20; `implement-opus-reviewed-2.1`; correction recheck [06-R023](agent-review-notes.md#06-r023--06c-r022-correction-recheck--pass); Reviewer B Opus 5.5 (`claude-opus-5-5`), requested xhigh, effective effort not reported. Artifact: base `76fe99c` plus worktree, digest `dbeb4953eff762719ffbd56465ae2eaf1b0c263e68d06a06d4fb40e567e4b4d5` (manifest `logs/06/06C-batch-snapshot-r023.txt`, the same 291 paths), verified before and after.
- **06-F028 RESOLVED.**
  - `README.md`: the `HITL{Approval or outreach requested?}` node, its four edges and the agent `outreach question` delivery edge are removed. `AGENTS`/`SAGENT` go straight to `PLAN`. The `Pauses` subgraph title says approval and waiting have no agent producer since 06C and that phase 08 rebuilds them as workflow steps. The remaining approval/wait edges are retained link-action, quorum and manual-control machinery.
  - `docs/deploy.md:181`: the MCP pilot evidence is limited to what this release can show: credentials, refresh/test, grant review, and stale-grant rejection on a schema/revision change. Remote-call receipt and disconnect are deferred to the phase-08 release pilot, which matches the unwired workflow worker.
  - Not machine-verified: Mermaid rendering of the new title, the repo's only Mermaid label containing `;`. Optional: quote the label. Non-blocking.
- **06-F029** (TASK.md:4690-4696):
  - (1) adopted as recorded intent: an edit is allowed and legacy runs fail closed at Rig `authorization()`.
  - (2) accepted gap: no scheduled-run test through real Rig; phase-08 obligation.
  - (3) `mcp_connections_by_ids` kept for workflow MCP test support; phase-08 cleanup.
  - (4) Agent Builder prompt wording revised in phase 08.
  All four are accepted. Carry-forward: add (2)-(4) to the phase-08 obligation list in `RESUME.md` when 06C is accepted, since today they are only in the TASK dispositions paragraph. Non-blocking; closed as recorded.

## 06-F030 — D2's phase06 acceptance condition on model usage is unmet for the Rig text classifier
- Context: 2026-10-20; `implement-opus-reviewed-2.1`; phase06 combined integration review [06-R024](agent-review-notes.md#06-r024--phase06-combined-integration-review--changes-required); Reviewer B Opus 5.5 (`claude-opus-5-5`), requested xhigh, effective effort not reported. Artifact: base `76fe99c93266c73ef4d13b2b10ff40df89580afb` plus worktree, digest `33cfebc799ec8a3f6a0814dec2496987a2042541234502354b363a55878c4bf0` (manifest `logs/06/06-final-snapshot.txt`).
- Classification: actionable requirement gap, **medium**; implementation or decision. OPEN.
- Requirement: accepted D2 (`logs/06/decision-D2.md:63-65`, restated in TASK.md:3247-3249) says "Direct in-memory Rig execution and alternate-runtime classifier calls remain explicit 06C migration obligations: before phase06 acceptance every production model entry must persist usage or be removed." The 06 plan's usage section (user revision 2026-10-19) also says "missing usage must not silently become zero".
- Evidence:
  - 06C replaced the ai-agents classifier with `RigTextClassifier` (`adapters/harness/rig/classifier.rs:1-4,42-78`, wired at `infra/setup.rs:185-203,264`). It makes a provider call with the company's credential and records no `model_usage_calls` row. It returns text only, and the response usage is dropped.
  - Production callers:
    - The Stage-3 spam guardrail runs on every non-trusted legacy agent run, before the harness (`agent_runner/mod.rs:230-246`), and repeats on every task attempt.
    - The system-prompt generator (`use_cases/agent.rs:938-983`; routes `agent_library.rs:276,298`, `ui_agents.rs:422`).
  - The guardrail emits only a monitoring estimate (`llm_guardrail.rs:166-174`, characters/4), which is not analytics. Its own comment (`llm_guardrail.rs:119-121`) records that before this change the call never reached a provider. So 06C turned it into a working, unmetered production model entry.
  - The only usage writer is `adapters/persistence/model_usage/mod.rs`, reached from the durable Rig checkpoint paths. `model_usage_calls.task_id` and `agent_id` are NOT NULL (`init_schema.sql:14224-14225`), so the prompt generator, which runs with no task, has no attribution path at all.
  - Nothing in TASK, RESUME or the review records discharges or dispositions the D2 condition. The direct Rig path is not a production entry (see 06-R024), so only the classifier remains.
- Effect: company provider usage on these paths is silently missing from analytics and cost estimates. Nothing marks it as unknown.
- Required correction (either):
  - (a) Persist usage. The guardrail can be attributed to the claimed task and agent inside `AgentRunner::execute`; record the reservation and settlement from the Rig response usage. For the prompt generator, either add a schema-supported attribution (the schema is edited in place; reset the DB) or remove the entry.
  - (b) Record an explicit decision that narrows D2 for these two system calls. If it narrows the user's 2026-10-19 measurement requirement, the user decides. Disclose the gap where analytics are documented, and carry "guardrail relocation out of AgentRunner must persist usage" into the RESUME phase-08 obligations.
  Either way, add a test that a classifier call's usage appears in, or is explicitly excluded from, the analytics read model.
- Instruction lesson: an accepted decision's "before phase acceptance" condition that is deferred to a later batch is invisible to batch reviews. Candidate (skill, final integration stage): list every such condition from the accepted decision records in the final gate record, and mark each as met or dispositioned.
- Resolution history: 2026-10-20 OPEN (06-R024).


## 06-F031 — Stale references left by phase06 removals and renames
- Context: as 06-F030. Classification: actionable documentation/tooling defect, **low**; implementation (docs, test script, build comment). OPEN.
- Evidence:
  - (a) `docs/workflow-language-v1.md`, the language reference for workflow authors, was not updated for D5 or D6.
    - It still documents `context.load` with `sources` and the old result shape (line 92), `ai.classify` without `agent` and with a `profile` enum property (line 95), and `agent.run` "Optional `capability_profile`" (line 96).
    - It still describes profile, classifier-omission and ceiling semantics as "phase06 responsibilities" (lines 131-138, 231-233, 241-248).
    - The registry now defines `context.history` (`max_tokens` only; items plus `omitted`) and requires `agent` for `ai.classify` and `agent.run` (`application/workflow/registry/families.rs:5-12,44-60`). D5 removed profiles and ceilings.
    - An author following the reference gets publication errors.
  - (b) `scripts/tests/harness-query-plans.py:13-21` extracts `lock_task_agent_harnesses` and `guard_active_agent_harness_change` from the current init migration. 06C dropped both (0 occurrences), so `re.search(...).group()` raises, and the command documented in `docs/harness-database-performance.md:19-22` fails. The rest of that doc (admission-lock and configuration-guard measurements, MCP selection reads) and `scripts/tests/fixtures/harness-fencing-before.sql` describe removed code.
  - (c) `Dockerfile:4-6` still installs `git` in the builder stage "for the ai-agents git dependency". `Cargo.toml` and `Cargo.lock` have no git sources after 06C; ai-agents was the only one (base `Cargo.toml:30`).
  - (d) Comments at `domain/entities/value_objects.rs:383-384,392-393` still describe tool ids as ai-agents built-ins or `outreach_and_await_quorum`, and skill slugs as compiled ai-agents YAML ids.
- Required correction:
  - (a) update the language reference to the D5/D6 contracts;
  - (b) delete the script, fixture and performance doc, or rewrite them for queries that still exist;
  - (c) drop the git install and its comment;
  - (d) correct the comments.
  Docs, script and comment changes need no Rust suite. After (c), rebuild the image or run the deploy script test if available. Keep Tailwind class literals out of docs (TASK pitfall), and re-run `npm run check:css` equivalence after editing `.md` files.
- Instruction lesson: removals and renames need a repo-wide search for the old identifiers in docs and scripts, not only in `src/` and the files the batch touched. Candidate (removal/rename batches): a `git grep` over the whole tree for each retired identifier, with the remaining hits listed in the gate record.
- Resolution history: 2026-10-20 OPEN (06-R024).


## 06-F032 — Deferred memory steps have no owning phase
- Context: as 06-F030. Classification: planning gap, **low**; user or root decision. OPEN.
- Evidence:
  - The 06 plan marks its memory acceptance item "Deferred by user decision 2026-10-20 (06-D6)" (plan lines 150-151). `decision-D6.md:117-123` keeps `memory.load`/`memory.save` publishable but unhandled (`registry/families.rs:13-28`). The user's words were "don't implement memory now".
  - No phase is named to implement them, and the RESUME phase-08 obligation list does not mention memory.
  - Phase 08 replaces the legacy "memory toggles" with workflow bindings (`08-…md:19-20`). The legacy runner's automatic recall and persistence goes with the legacy binding, so memory disappears entirely unless steps exist by then.
  - Phase 10 cutover still expects "Classification/context/memory visible" (`10-…md:20`).
- Required correction: record which phase implements the memory steps (and add it to that phase's plan or the RESUME obligations), or record a user decision that this release ships without memory and adjust the phase 08/10 criteria. Planning only.
- Instruction lesson: a deferral without a destination is a silent drop. Candidate: every "deferred" acceptance item records its owning phase, or an explicit "dropped" decision.
- Resolution history: 2026-10-20 OPEN (06-R024).

## 06-R025 dispositions — 06-F030, 06-F031, 06-F032 RESOLVED
- Context: 2026-10-21; `implement-opus-reviewed-2.1`; correction recheck [06-R025](agent-review-notes.md#06-r025--phase06-r024-correction-recheck--pass); Reviewer B Opus 5.5 (`claude-opus-5-5`), requested xhigh, effective effort not reported. Artifact: base `76fe99c` plus worktree, digest `0d8bba5d139ad7dfc69380b45ed32ed48cc3ec481c4be45abdeb2d8f5f329b00` (manifest `logs/06/06-R024fix-snapshot.txt`), verified before and after. The user decisions of 2026-10-21 are recorded verbatim in TASK.
- **06-F030 RESOLVED** by user decision plus removal.
  - The Stage-3 LLM spam guardrail is gone end to end: service, runner call and builders, dispatch skip logic, company field/column/SQL/forms/settings, `AppConfig` and `ENABLE_LLM_SPAM_GUARDRAIL`, `fly.toml`, docs and marketing. No reference remains, and the fresh schema has no column.
  - The prompt generator stays unmetered by user decision. D2 is narrowed in plan 06:148-152 and disclosed in `docs/rig.md` and `docs/deploy.md`.
  - Stage 1/2 scanning is untouched.
  - B's full lib suite: 2692/0.
- **06-F031 RESOLVED.** (a) The language reference matches the registry contracts. (b) The script, fixture and doc are deleted. (c) The `Dockerfile` git install and cache mount are removed (not built: no docker). (d) Comments fixed. Optional nit: the `ToolId` comment at `value_objects.rs:383-385` understates its use for workflow tool names in diagnostics (`rig/workflow.rs:76-81`).
- **06-F032 RESOLVED** by user decision. Memory is out of this release, recorded in plans 06, 08, 10 and the plan index files. Phase 08 decides delete-or-dormant for the provider infrastructure; root adds that to the RESUME phase-08 obligations.
