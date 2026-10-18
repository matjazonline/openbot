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
