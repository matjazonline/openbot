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
