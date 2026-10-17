# Senior review notes

### R-001 — group A source confirmation

- Reviewer: S, thread `thr_xku2jgixtv`, assigned Codex / `gpt-6-astra` / high;
  settings confirmation is recorded in [TASK.md](TASK.md). No workers spawned.
- Artifact: base `5b3d7dc08340d7b516740b9fcd71b753183ced16`, group A commit
  `67d319b`, current HEAD `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006`.
  Independently SHA256-checked all seven paths in
  [groupA-source-reconcile.txt](logs/groupA-source-reconcile.txt) and byte-compared
  each with both commits: all match. Prior manifest SHA256
  `9148f4c18b17105ff43fbe22334ea7a35101e3079a896dfc7d835de00bd05b57`.
  Scope is these committed source files, not the concurrently growing verification
  record. Existing untracked shell/IDE/agent files and coordination docs are preserved.
- Design: [BRIEF-04.8](../BRIEF-04.8.md), SHA256
  `3f76f3181278bfff7744a47e6701add420fe22d0090f6145a6a0eac89885cd04`;
  [cancellation serialization contract](../CONTRACT-04.7-CANCEL-SERIALIZATION.md).
- Assignment: adopt or challenge the prior source PASS, with fresh diff/caller
  inspection; group B coverage guidance is separate in [GUIDANCE.md](GUIDANCE.md).
- Coverage: read `git diff 5b3d7dc 67d319b -- src` in file groups, applicable
  AGENTS.md, original execution item 7/acceptance and EXPANSION 04.8, and prior
  [architecture review](/tmp/workflow-foundation-20261013-04.8/reviewer/ARCHITECTURE.md),
  [CODE-A](/tmp/workflow-foundation-20261013-04.8/reviewer/CODE-A.md), and
  [CODE-A-RECHECK](/tmp/workflow-foundation-20261013-04.8/reviewer/CODE-A-RECHECK.md).
  Traced `prelock_on`, `authorize_on`, and `return_authorized` transitively with graft.
  Inspected local dispatch, remote reservation/entry, standalone authority and
  receipt return; receipt persistence and shared control/completion ordering were
  also inspected for integration effects.
- Assessment: company/principal authorization now precedes the run fence on all
  affected callers. The unlocked actor is only a hint, rechecked under the run
  lock (`action_authority.rs:35–120`); live state/deadline, frozen scope/digest,
  current policy/resource, exact attempt and post-wait liveness checks remain.
  `return_authorized` (`action_receipts.rs:313–359`) defers only authority
  NotFound, preserves known lost ownership as false/Interrupted, propagates
  operational errors, and rolls back before returning. Receipt truth remains in
  the independent run-locked transaction (`:168–254`). Explicit `invalid` imports
  only disambiguate existing helpers.
- Regression coverage: the ten variants in
  `workflow_action_cancellation_company_before_run_all_callers` establish actual
  backend blockers and completed outcomes for all five callers in both orders.
  Cancel-first has the corrected free/held/free exact-run probes, including
  SQLSTATE `55P03`, rollback and post-drain transaction cleanliness
  (`action_cancellation_order_tests.rs:131–262`). The prior R1 is closed in this
  artifact. `workflow_action_cancellation_return_lost_owner_precedes_denied_authority`
  covers cancelled ownership plus revoked membership. Group B's broader service
  matrix is still required; this test does not claim it.
- Checks executed by S: read-only Git diff/status/show, SHA256/byte comparisons,
  graph queries and targeted source reads. No builds, tests, SQLx, DB access or
  fixture mutation. A graft query reported an automatic one-file graph refresh;
  no tracked source diff resulted. Prior review records were inspected, not
  treated as newly executed runtime evidence.
- Findings: **none**. No new finding IDs; prior reviewer R1 closure adopted.
- Limits: D's concurrent stock-stack/runtime/SQLx/lint verification is outside
  this review and remains a separate acceptance requirement. Group B/C unverified.
- Result: **adopt group A CODE PASS** for the seven-file manifest above at HEAD
  `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006`. No concrete production defect found.
  This is source confirmation, not group A runtime acceptance or point completion.

### R-002 — Linux parallel-load failure diagnosis and correction criteria

- Reviewer: S, `thr_xku2jgixtv`, same assigned Codex / `gpt-6-astra` / high role.
- Artifact: unchanged group A source at HEAD
  `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006`; D's runtime evidence used binary
  SHA256 `f14fb5cda77c66735354f34141597cdc9ef718038eed2901c121d9340168517e`.
  Baseline is `5b3d7dc` plus the inspected five-import-only patch, binary SHA256
  `aa9bfa2f96246d5753354c664240ebed320307a10879f896c0890e0b2544c351`.
  D records that the original binary was replaced by its shared-target A/B build;
  subsequent HEAD rebuild is `c7491b3a…`, not the binary used in these runs. This
  note adopts no execution evidence for that replacement binary.
- Assignment: S-002; classify reconciliation failures under Linux parallel load
  and specify corrections preserving early resource-limit failures. Updated
  [GUIDANCE.md DEC-009–011](GUIDANCE.md#dec-009--linux-verification-failures-separate-the-three-mechanisms).
- Evidence inspected: [IMPLEMENTATION.md](IMPLEMENTATION.md), actual failure
  sections of [run1](logs/groupA/runtime-workflow-action.run1.log),
  [run2](logs/groupA/runtime-workflow-action.run2.log),
  [baseline](logs/groupA/runtime-workflow-action.ab-baseline.log), the
  [import patch](logs/groupA/ab-baseline-imports.patch), both isolated high-bounds
  logs, runner, fixture identity recorded by D, and CI budget-gate configuration.
  Run1 is **308/309 (one failure)**, run2 **307/309**, baseline **305/307**;
  there was not a second varying failure in run1. Isolated passes are 92.94 s and
  53.93 s. Source inspected includes high-bounds source/native/assertions/test,
  prior-bounds competitor/verifier/classifier/controls, mixed-truth continuation,
  all-table snapshot helper, renewal/live-window policy, reconciliation service
  and settlement, and `scripts/lease-budget.sh`.
- Analysis: high-bounds has direct baseline evidence of a lost three-second lease
  premise and an unsupervised final native/diagnostic phase. Correct fixture
  ownership timing without changing production policy. Mixed-truth's final
  claim-to-completion segment has the same structural exposure, but its `None`
  lacks a timestamped cause. Prior-bounds has a distinct five-second whole-command
  timeout **after verifier returned**; settlement is inside that timer. Existing
  logs cannot distinguish deliberate fixture delay from slow/blocked settlement.
  It is not valid to declare all failures the same measured lease lapse or to
  accept every timeout as a void attempt.
- Checks performed by S: graph queries/call tracing, targeted source/log reads,
  Git status; no source edits, test/build execution, DB queries or mutations.
  Only these guidance/review records were edited. No threads or agents spawned.
- Findings: no confirmed production defect; no F-ID allocated. High-bounds test
  calibration is established; mixed/prior attribution needs the targeted evidence
  specified in DEC-010/011. Those unknowns are not a code PASS for their behavior.
- Limits: no per-phase timings, lease timestamps at the generic Conflicts/None,
  or backend blockers are present in the failing logs. A/B sample is too small
  for performance equivalence, and the prior-bounds timeout did not recur in that
  baseline run. No fresh reproduction by S is claimed.
- Result: **guidance delivered; runtime acceptance remains open**. R-001 source
  PASS is unchanged. No limit increase, production fix, architecture reopening or
  waiver is approved. D can implement bounded fixture corrections and collect
  diagnostics; root/user input is not needed for that technical work. Any proven
  production defect follows DEC-011's 04.7-versus-04.8 scope decision.

### R-003 — D-002 strict Clippy remedy, scoped delta review

- Reviewer: S, `thr_xku2jgixtv`, assigned Codex / `gpt-6-astra` / high.
- Artifact: [source-manifest-D002.json](logs/groupA/source-manifest-D002.json),
  SHA256 `9a617cd6ea6424d44c61b70df16e155f6b4e03991f51413c6f3288bde9e301aa`,
  base `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006`.
  [source-D002.diff](logs/groupA/source-D002.diff) SHA256
  `826c44be352cbc316b5d478db6921ec2814aa1036f9561423fff67f0dfb8c620` also verified.
  All six listed worktree file hashes matched at review start and again immediately
  before this disposition. Inspected the actual base-to-worktree diff restricted to
  those six paths; it contains exactly the recorded changes. D-003's concurrent
  reconciliation test edits are outside this artifact and were not reviewed.
- Authority/coverage: TASK.md's explicit 2026-10-15 user decision authorizes one
  crate-level `allow(clippy::double_must_use)` with an async-trait reason comment,
  plus five `fetch_update` → `try_update` renames. Reviewed `src/lib.rs`, Rig budget,
  dashboard SSE drop, MCP bounded response stream, runtime-metrics test double and
  task-worker test closure. Applicable source/application/HTTP instructions read.
  Graft traced MCP bounded stream into `post_message`; exhaustive scoped charge
  lookup resolved ambiguous Rig caller names (tools, compile and skills test).
- Lint assessment: `src/lib.rs:1–4` allows **only** `clippy::double_must_use`, with
  the Clippy 1.99/async-trait reason and an explicit removal condition. It does not
  allow warnings, deprecations, unused results or other Clippy lints. Its scope is
  the whole library, including authored code and lib tests, as the user explicitly
  chose; it is not falsely characterized here as expansion-only suppression. No
  duplicate allowance was added to the binary or dependencies.
- Semantic assessment: inspected the installed Rust standard-library source at
  `/home/mh/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/core/src/sync/atomic.rs:3408–3419`.
  Integer-atomic `fetch_update` delegates directly to
  `self.try_update(set_order, fetch_order, f)`. All five sites retain both memory
  orderings, closure logic, checked/saturating arithmetic, previous-value use and
  success/error handling verbatim; none uses explicit generic arguments. No
  concurrency protocol or resource bound changed. The new method is stable since
  Rust 1.95 according to that local source; Cargo declares no `rust-version`, and
  current CI selects stable. No dependency, SQL or schema delta.
- D evidence inspected, not rerun by S: [strict Clippy](logs/groupA/clippy-D002.log),
  [fmt](logs/groupA/fmt-D002.log), [diff check](logs/groupA/diff-check-D002.log),
  [offline all-targets check](logs/groupA/check-offline-D002.log) and
  [test build](logs/groupA/test-build-D002.log) each record exit 0; Clippy/check
  have no diagnostics. The build records binary SHA256
  `d769bca7fdcde79d492e83ab775ee5f5e6170b0c64887b3613c80421db887972`.
  [Runtime results](logs/groupA/runtime-results.D002.json), runner output and
  corresponding raw test logs agree: Rig 52/52, dashboard 5/5, MCP 7/7,
  runtime metrics 6/6, bounded task-loop case 1/1 — **71 passed**, no failed or
  ignored tests, stock 2 MiB, recorded binary identity, and zero DB/client residue.
- Limits: MCP's over-byte-limit error branch lacks direct runtime coverage, as D
  explicitly records; the inspected standard-library alias and unchanged closure
  establish semantic equivalence for this rename. No claim of a full-suite pass
  or verification of concurrent D-003 content. S ran only read-only inspection
  and identity checks; no source/test edits, builds, tests or DB activity.
- Findings: **none**.
- Result: **PASS — D-002 delta**, for manifest SHA256
  `9a617cd6ea6424d44c61b70df16e155f6b4e03991f51413c6f3288bde9e301aa`.
  This closes source review of the authorized Clippy remedy; the reconciliation
  runtime failures and group A/04.8 combined acceptance remain separate.

### R-004 — D-003 reconciliation corrections and group A readiness

- Reviewer: S, `thr_xku2jgixtv`, assigned Codex / `gpt-6-astra` / high;
  same role/settings confirmation as R-001. No workers spawned.
- Artifact: HEAD `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006` plus
  [source-manifest-D003.json](logs/groupA/source-manifest-D003.json), SHA256
  `e2a41cc3b06aabaaefb0939de2916f94f8f657aee91d5ec403d6401643cc93cb`.
  Verified every one of its 15 file hashes, exact tracked-change set, empty index,
  and no additional untracked project files; excluded workflow coordination/log
  artifacts. The six D-002 hashes are unchanged from R-003. Recorded diff SHA256
  `dd44b9004a43d1c78d3b9a9fe09aec0efe49daf789ab60f571aedbbac9979530`
  matches. Identity rechecked before disposition.
- Assignment/coverage: S-004, all nine D-003 test-file diffs, DEC-009–011, the
  raised-limit rule, BRIEF-04.8 section A and TASK.md's group A gates. Used graft
  caller tracing and targeted source reads for snapshots, genuine renewal,
  prior-bounds competitors/controls, service execution and settlement delegation.
  R-001 remains applicable to unchanged group A production source; R-003 remains
  applicable to its unchanged six-file delta and 71-test semantic-alias evidence.
- Calibration assessment: D's measurement log directly observes high-bounds lease
  expiry (-183 ms at 3152 ms), mixed completion beginning with only 87 ms remaining,
  and prior-bounds fixture work extending beyond the five-second command budget.
  The corrected prior proof releases at 1591/1595/1600 ms and settles in 36/44/44 ms
  in the three full runs. This supports correcting diagnostic/fixture scheduling;
  it does not establish the exact cause of every historical generic error or prove
  production performance under all workloads. No production defect is established.
- Bounds/renewal: three-second lease, five-second command cap, native 128/129
  limits and three-fixture ceiling remain unchanged. `renew_phase` calls genuine
  production `renew_io`, rejects a lost live premise distinctly, checks unchanged
  run revision, and rebases only the exact job's committed `lock_expires_at`.
  Refused supported entry/completion remains failure, never a successful skip.
  Lease/stack budget scripts and CI invocations are unchanged. These retained
  assertions are the early failure signal; no raised bound needs a new waiver.
- Snapshot assessment: batched `all_tables` retains the catalog-owned names,
  repeatable-read/read-only transaction and identical row JSON/order expression.
  There is one data statement plus the catalog query, not literally one total
  database command. The populated-fixture equivalence test compares batched and
  retained per-relation reads, meaningfully testing the changed assembly path;
  the shared row expression is unchanged by source inspection. Subset reads are
  checked against the full result. Coverage digests contain row count and SHA256
  of every canonical row, in bytewise order; JSON escapes embedded newlines and
  count retains multiplicity. This is an adequate compact equality oracle for
  the large history relation. Structural command/run assertions retain full rows.
- Prior-bounds assessment: scheduled-128 preparation moves before command start,
  while the genuine competing claim, renewal, entry129 consumption, retirement,
  barrier and required pre-settlement state remain inside the verifier pause.
  Scoped snapshots retain the previously compared relations; coverage alone is
  digested. The stale proof still requires `BoundExceeded`. The wrapper delegates
  real settlement with the original service/budget. `withheld_timeout`, durable
  no-delta backstop and bounded exhaustion remain; no broad timeout-success or
  returned-late classifier was introduced. Current negative-control execution is
  nevertheless missing (F-002).
- High-bounds assessment: all native rejection/rollback checks and final
  130/0/128 accounting, provenance, calls and effects assertions remain. However,
  the separate supported-entry all-relation no-write oracle was weakened to one
  relation plus a combined reserve/entry net delta (F-001). This is not equivalent
  to the old two-boundary assertion and must be corrected.
- Fourth test disposition: `live_return_conflict_vetoes_genuine_new_result` is
  outside DEC-009's original three failures. Its delta adds read-only phase probes
  only; it retains the genuine conflicting return, Interrupted result, persisted
  observations/history and required release. The cheaper shared snapshots explain
  its improved measured headroom (at least 1834 ms in the final runs). No separate
  production behavior change or architecture reopening is warranted.
- D evidence inspected, not executed by S: D-003 measurement and temporary cost
  logs, final full-run raw logs and result JSON, fmt/diff/offline check/strict
  Clippy, binary freshness and residue records. Final binary SHA256
  `c973c76d0cd5858ec03b3471b11b3b0ace0ca366db4d14c0dea156f8145b84a8`.
  Three stock-2MiB/default-parallel `workflow_action_` runs pass **310/310** each,
  zero failed/ignored, in 467.30/507.39/538.24 seconds, zero qualified voids and
  zero residual DBs/clients. Static checks report exit 0. Fixture system identifier
  is `7696100747480141867`. These passes establish execution of the submitted
  assertions; they cannot compensate for F-001 or select omitted F-002 controls.
- Group A readiness: **not ready**. R-001 and R-003 remain PASS; current action
  filter and static evidence clear the earlier observed runtime/Clippy failures.
  D-001 control-suite evidence can be reused where source and dependencies are
  unchanged, but its prior-bounds control results cannot cover the changed D-003
  helpers. Current SQLx preparation/check is also absent after query edits (F-003).
  Full library/lease-budget/stack-budget, fresh combined schema checks and graph
  refresh remain group C gates, not additional blockers invented for this group.
- Checks by S: read-only Git/source/log inspection, hashes and caller analysis.
  No source/test edits, builds, test execution, DB access or mutating checks.
  Only S review records were written. Runtime claims above are D's evidence.
- Findings: [F-001, F-002, F-003](agent-review-findings.md), all open. F-001 is a
  test-oracle regression; F-002/F-003 are required verification gaps. No production
  gap, architecture reopening or user decision is identified.
- Result: **CHANGES REQUIRED** for manifest SHA256
  `e2a41cc3b06aabaaefb0939de2916f94f8f657aee91d5ec403d6401643cc93cb`.
  D must restore the boundary oracle and supply affected checks on the resulting
  artifact; root retains group A acceptance authority after S recheck.

### R-005 — D-004 finding recheck and group A acceptance recommendation

- Reviewer: S, `thr_xku2jgixtv`, assigned Codex / `gpt-6-astra` / high;
  same settings confirmation as R-001. Assignment S-005; no workers spawned.
- Artifact: HEAD `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006` plus
  [source-manifest-D004.json](logs/groupA/source-manifest-D004.json), SHA256
  `64ce8860703195366dd8c14b838ac81951d040971fa7183079f457c40bd4935f`.
  Verified all 15 file hashes, exact tracked-change set, empty index, no additional
  untracked project files, and saved combined diff SHA256
  `9bf348297edca3bee77afe5fad634a325d2dba336469db3d6504609fcf6068ef`.
  Workflow coordination/log artifacts are excluded. Only high-bounds native tests
  and proof tests differ from R-004; other 13 hashes are identical. Reconstructed
  the two D-003 files from its saved patch and HEAD, verified their old manifest
  hashes, and reviewed the precise D-003-to-D-004 delta. Final identity rechecked.
- Coverage: F-001–003, DEC-009–011, BRIEF-04.8 section A and TASK.md batch-order
  gates. R-001's unchanged production-source assessment, R-003's unchanged six-file
  remedy, and unaffected R-004 assessments remain applicable. New shared snapshot
  plumbing and its affected action/prior-bounds tests were reviewed and rerun by D.
- **F-001 resolved:** `supported_positive` now fingerprints every public relation
  immediately after real reservation, then performs real entry and compares that
  fingerprint against a post-entry fingerprint. Post-entry full rows come from
  the same repeatable-read/read-only transaction as the fingerprint, so they also
  establish the separate reservation-only delta. A non-entry row changed and then
  restored no longer escapes detection. The canonical full-row/count/SHA256
  encoding and catalog name set cover the original state. Shared `relations_on`
  preserves row assembly/order; `read_snapshot` sets isolation before any read.
  No lease renewal occurs inside the frozen reservation-to-entry interval.
- Equivalence evidence: the extended populated-fixture test compares full rows
  against the retained per-relation implementation, whole fingerprints against
  all-name digests, and counts against full arrays. After `barrier` genuinely
  updates provider operations/evidence, it requires fingerprint differences iff
  full rows differ, with at least one changed relation. Source inspection confirms
  the barrier writes; ledger setup fixes the earlier missing fixture premise.
  This is meaningful sensitivity coverage as well as agreement of read paths.
- **F-002 resolved:** [prior-bounds log](logs/groupA/runtime-prior-bounds.D004-prior-controls.log)
  and listed names confirm **9/9**, including all seven controls, the timeout
  discriminator and positive proof. Exact-fence expiry, unknown reservation both
  committed/uncommitted, foreign-fence/error rejection, pause draining, withheld
  timeout and three-void exhaustion all retain their expected outcomes. Stock
  2 MiB/default parallelism, zero ignored and zero DB/client residue are recorded.
- **F-003 resolved:** current [SQLx check](logs/groupA/sqlx-prepare-check-D004.log)
  runs live `cargo sqlx prepare --check -- --all-targets` successfully after the
  query changes. Independently compared all 45 current cache hashes with before,
  after and D-001 manifests: identical. Migration info records 53 installed and
  exit 0. Runtime strings are verified by DB-backed tests, not by offline metadata.
- Current runtime evidence inspected: binary SHA256
  `72cca654c30c76bea9c63216da8672c7e914145a8949ca943727c2d1ff852984`
  matches the actual file, manifests, log headers and post-Clippy rebuild.
  Three `workflow_action_` runs pass **310/310** each, no failed/ignored tests,
  stock 2 MiB/default parallelism and zero residue. Raw libtest durations are
  584.13/561.04/624.34 s; runner elapsed times include small overhead. No qualified
  voids occur in these full runs. Prior-bound settlement is 115/74/101 ms after
  releases at 2258/2838/1797 ms, within the unchanged five-second budget.
  Intermediate high-bounds fingerprints leave 2891/2862/2765 ms of lease headroom.
  The slower total suite duration is not by itself evidence of a production stall;
  no assertion or resource limit was relaxed to obtain these passes.
- Group A gate disposition:

  | Gate | Evidence and disposition |
  |---|---|
  | Authority order, actor reload, error semantics, actual caller regression | R-001 source PASS retained; D-004 full action runs re-exercise the authority/cancellation cases. |
  | Dispatch, receipt operational errors/refused return, action/reconciliation | Current full filter passes three times; it includes the affected named cases. |
  | Control integration | D-001 `workflow_control_` 29/29 and `control_tests` 37/37 reused: these tests and their control implementation are unchanged; D-004 snapshot corrections are in the sibling action-test module. Changed prior-bounds dependencies have current 9/9 evidence. |
  | Authorized Clippy remedy | R-003 PASS and D-002 71 passing affected tests reused; all six file hashes unchanged. Current strict Clippy also passes. |
  | SQLx prepare/check | Current live check passes; cache identity independently verified. |
  | fmt, diff, offline all-targets check, strict Clippy | D-004 logs each record exit 0; check/Clippy have no diagnostics. |
  | Fixture execution and cleanup | Task PostgreSQL fixture used, stock stack/default parallelism recorded, no ignored cases, final own DBs/clients 0/0. |

- Findings: **F-001, F-002 and F-003 resolved** in
  [agent-review-findings.md](agent-review-findings.md); no new findings.
  No production gap, architecture reopening or outstanding user question.
- Checks by S: read-only Git/hash/patch reconstruction, graft and exact source/log
  inspection. No builds, tests, SQLx execution or DB activity by S. Only review
  records were edited. D's executions are evidence inspected, not S executions.
- Limits: this is group A acceptance readiness, not final 04.8 completion.
  Group B matrix and group C combined full-library/budget/schema/graph gates retain
  their scheduled obligations. No remaining group A gate is deferred or waived.
- Result: **PASS; group A ready for root acceptance** on manifest SHA256
  `64ce8860703195366dd8c14b838ac81951d040971fa7183079f457c40bd4935f`.
  Root records formal acceptance before group B begins.

### R-006 — whole-batch group B1 review

- Reviewer: S, `thr_xku2jgixtv`, assigned Codex / `gpt-6-astra` / high;
  same confirmed role as R-001. Assignment S-006; D idle, no workers spawned.
- Artifact: HEAD `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006` plus
  [source-manifest-B1.json](logs/groupB/source-manifest-B1.json), SHA256
  `822434bc4593d9d78bd32f367374e07e2ee10a6d2a0608395cc04647581719ac`.
  Independently verified all 19 file hashes, the exact 17 tracked changes and two
  untracked source additions, empty index, recorded tracked diff SHA256
  `5d9146846ecb57d4e8e5f2ace053aa7406ed6b41dcf3dad4078ac75353317d50`,
  and current test binary. Workflow coordination/log artifacts are excluded.
  Exactly five paths differ from accepted group A manifest `64ce8860…`:
  two new test/support files, two module registrations and the proof-module move.
  All other accepted source hashes are unchanged. Identity rechecked at disposition.
- Requirements/coverage: phase 04 execution item 7 and relevant acceptance,
  BRIEF-04.8 section B/table and DEC-001/002/003/006/008. Read every new B1 test
  and helper, source registrations, actual service supervision, reserve/entry/
  ownership paths, CancelCommand transaction/commit path, fixture construction,
  provider ledger and original snapshot helper block. Graft discovery/call tracing
  and exact spans were used; D's summary did not substitute for source review.
- Prepared/protected: four local/remote/plain/protected variants use genuine
  CancelCommand before public service dispatch. Exact lifecycle Conflict, retained
  intent/action facts, zero calls/effects/marker/receipt, repeated fresh service
  refusal, old-fence completion refusal and no new claim establish the requested
  cancelled-state boundary. Existing live/protection checks are reused and rerun.
- Post-reservation: the delegating writer first awaits real committed reservation,
  then real cancellation, then returns that same reservation to the actual service.
  Real entry is delegated and refused. Exact marker/entry identities remain, no
  receipt/poll/effect appears, reconciliation is recorded, whole post-cancel state
  remains unchanged through refusal and fresh retry/claim/completion.
- Pending durable: both never-applied and applied-then-hung providers signal after
  their genuine first poll (and after the ledger write in the applied variant).
  RAII drop is sampled at service return; the token stays uncancelled; cancellation
  preserves the effect and uncertain history without fabricated receipt or replay.
  These state/drop checks are sound, but the eight-second completion allowance
  cannot identify ownership supervision as the cause instead of the three-second
  frozen deadline. The post-cancel still-pending assertion is also misplaced.
  **F-004** requires a causal witness and a correctly ordered premise checkpoint.
- Token-only: pre-cancelled token leaves the whole DB unchanged and polls nothing;
  the entry seam cancels only after actual authorization, with zero provider polls.
  Pending-token interruption checks whole state against the pre-interruption
  snapshot and drop-before-return. Ownership/control facts stay unchanged, and the
  successful same-fence PossibleDispatchExists retry establishes live ownership
  and no replay permission; deadline-only termination cannot satisfy that retry.
- Rollback: deferred constraint trigger on command insertion fires at COMMIT,
  after cancellation writes and audit. The injected error and full-table baseline
  equality include action uncertainty/witness/receipt/evidence and ownership facts.
  Fault removal is followed by valid work under the same fence: prepared work
  dispatches once and replays its receipt; accepted work settles without another
  provider invocation. Both complete with valid output, one attempt and one effect.
  Existing control fault-location and converse receipt rollback cases are rerun.
- Shared cancellation assertions: non-control/action tables remain byte-stable;
  run cancellation, current-attempt retirement cause, job ownership clearing,
  appended state witness, control event/receipt counts and absent completion are
  checked explicitly. This is not a claim that every column of each allowed
  control table is compared; the existing exact control tests remain part of the
  composed coverage. B2 must retain its specified receipt/revision/audit oracles.
- **Question 1 — yes:** the sequential post-reservation seam is the DEC-001 gate.
  Returning the unchanged reservation releases the service to final authorization
  only after the real cancellation has committed. No competing transaction pair
  is being claimed, so exact backend blocker evidence is unnecessary for this
  boundary. B1's pending case does run cancellation concurrently with provider
  supervision; that is distinct from a receipt/cancel lock-order race.
- **Question 2 — yes:** the snapshot move is acceptable and independently checked.
  Reconstructed the accepted D-004 proof file from HEAD plus its saved patch and
  verified its original SHA256. The extracted helper block is byte-identical
  after allowing the new module header/import, `per_relation_tables` becoming
  `pub(super)`, and removal of one trailing separator blank line. Remaining proof
  code differs only by the documented `all_tables` re-export/comment. No helper
  logic or original equivalence-test assertion changed. Current action and
  prior-bounds executions cover the moved helpers and old imports.
- **Question 3 — yes:** applied-then-hung recovery belongs to B3's accepted
  uncertainty/reconciliation integration (DEC-007). B1 proves the actual effect
  remains with no fabricated receipt, an unresolved marker and terminal refusal.
  B3 must map/rerun the exact existing terminal recovery/late-truth contracts;
  cancellation never supplies not-applied evidence or replay permission. B2 still
  owns observed-result settlement and ordinary cancelled late receipt/restart.
- B2 guidance clarification: no changed requirements or new DEC-ID is needed.
  DEC-002/004 continue to require actual run-blocker witnesses and both receipt/
  cancel orders, plus a `finish_remote` seam after result selection/validation
  for token interruption. B1's sequential seam is not a substitute for that
  contention proof. Carry forward the shared snapshots, not the pending test's
  deadline-ambiguous completion oracle.
- Evidence inspected, not run by S: actual B1 isolated logs (8/8), three repeated
  B1 filters (8/8 each), full action filter **318/318**, prior-bounds **9/9** and
  workflow-control **29/29**; raw logs agree with result JSON and recorded binary
  SHA256 `596408e77e589d68078de72a0e061d77465032bbd5fb0787f73f0d10db638a72`.
  Runner unsets missing-DB allowance, uses task DB URLs, stock 2 MiB and default
  parallelism, checks listed/pass counts, and records zero DB/client residue.
  fmt/diff/offline all-targets/strict Clippy and live SQLx prepare/check exit 0.
  Independently compared all 45 cache hashes: before/after/current/D-004 identical.
  Post-Clippy rebuild and actual binary hash match. These executions demonstrate
  the submitted tests pass, not that F-004's missing discrimination is present.
- Checks by S: read-only source/log/Git/hash inspection and in-memory patch
  reconstruction. No source/test edits, builds, tests, SQLx or DB activity.
  Only review records were written. Existing F-001–003 stay resolved.
- Findings/limits: **F-004 open**; no other finding or confirmed production gap.
  No architecture reopening or root/user product decision required. B2/B3 and
  group C obligations remain outside this B1 disposition.
- Result: **CHANGES REQUIRED — B1**, manifest SHA256
  `822434bc4593d9d78bd32f367374e07e2ee10a6d2a0608395cc04647581719ac`.
  Correct and recheck the durable-pending causal oracle before B1 acceptance.

### R-007 — B1r1 correction recheck, F-004

- Reviewer: S, `thr_xku2jgixtv`, assigned Codex / `gpt-6-astra` / high;
  same confirmed role/settings as R-001. Assignment S-007; D idle, no workers.
- Artifact: HEAD `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006` plus
  [source-manifest-B1r1.json](logs/groupB/source-manifest-B1r1.json), SHA256
  `ef2e0ecd1ef49a5ebea6c0ddb60f42a92cb92d56b1a3300575b4513c5c9aafdb`.
  Verified all 19 hashes, exact 17 tracked changes plus two declared untracked
  source files, empty index, HEAD, saved tracked-diff hash and actual test binary.
  Workflow coordination/log artifacts excluded. Only the boundary-test file
  differs from R-006; all other 18 hashes match. Reconstructed the old boundary
  file by replacing the changed block with R-006's inspected text and matched
  its B1 SHA256, proving the entire delta is confined to pending-test supervision,
  the positive case and two new controls. Final identity rechecked.
- Scope: F-004, DEC-003, BRIEF's work-in-flight obligation and retained original
  bounds. Read the actual wrapper, observation classifier, extracted scenario,
  positive test and both controls. R-006's remaining B1 assessments and answers
  remain applicable; production service, adapter, cancellation, snapshot helpers,
  registrations and all other test cases are unchanged.
- **F-004 resolved:** `Supervised` delegates real reservation, entry, settlement
  and ownership checks; only its explicitly selected negative-control mode
  substitutes always-owned. It captures the actual `RemoteEntry.deadline` and
  records probe start plus completed result. An error records None; cancellation
  of the probe before it finishes records nothing. The positive classifier requires
  at least one probe begun after real cancellation returned, every such result
  false, its start no later than dispatch return, and return strictly before the
  frozen deadline. In the real wrapper, a stored result necessarily completed
  before dispatch returned, since recording happens within the awaited probe.
  The never-ready provider, uncancelled invocation token and these observations
  distinguish ownership refusal from deadline, probe error or guard timeout.
- Pending premise: first poll, zero drops, time before actual entry deadline and
  a genuine successful ownership query are checked before cancellation. The old
  invalid post-cancel zero-drop assertion is gone. Both effect variants still
  require Interrupted, one first poll, RAII drop sampled at return, exact retained
  post-cancel state, marker/entry/no receipt/reconciliation, actual ledger count,
  fresh refusal and inert claim/completion. No durable oracle was removed.
- Negative evidence: the new real always-owned scenario still reaches Interrupted
  and drop at/after the actual deadline; the same classifier rejects its true
  probe results. The isolated log records that rejection. Synthetic observations
  accept false-after-cancel/before-deadline and reject missing-result timeout,
  error, affirmation, deadline-only, probe-start after return and return at the
  deadline. This directly covers R-006's counterexample rather than merely adding
  elapsed-time reporting.
- Bound clarification: contrary to a literal reading of D's “no bound changed”
  summary, the outer scenario watchdog changed **8 s to 10 s**. The fixture lease,
  captured entry window, heartbeat and production supervision guard did not change.
  The new positive assertion fails any dispatch return at/after the original
  frozen deadline (at most the original three-second lease), and the deadline-only
  negative control is a normally registered full-action/library test. That is the
  retained earlier failure signal required by AGENTS.md; the larger outer cleanup
  watchdog cannot turn a delayed positive invocation into success. This limited
  test watchdog change is acceptable without a production-limit waiver.
- D evidence inspected, not rerun by S: final binary SHA256
  `de27a83b2cbfffc16c3928c75dab6620828599a2f30f59385b56d17e71dd123a`
  matches manifest, actual file, log headers and freshness check. Corrected positive
  test passes three exact isolated runs; live negative and synthetic classifier
  controls each pass. Current B1 filter is **10/10** and full `workflow_action_`
  **320/320**, no failed/ignored cases, stock 2 MiB/default parallelism, task DB,
  missing-DB allowance removed, zero DB/client residue. Raw logs agree with JSON;
  full-action libtest duration is 404.32 s. Current fmt/diff/offline all-targets
  check/strict Clippy each exit 0 with no source diagnostics; the offline check's
  build-directory lock wait is not a failure.
- Reused evidence: B1's prior-bounds 9/9, workflow-control 29/29 and live SQLx
  prepare/check remain applicable. This correction adds no SQL, changes no
  shared snapshot/control helper and leaves all those files/cache unchanged.
  Group A's accepted source and verification conclusions remain intact.
- Limits: the classifier deliberately demands a probe that starts after the
  cancel acknowledgement. A run with only a probe overlapping that acknowledgement
  cannot be credited as this positive witness; it fails rather than being counted
  as success. This is a conservative test-witness requirement, not a claim that
  every valid production interruption must have that ordering. No timeout is
  converted into a passing positive case.
- Checks by S: read-only hashes/Git, graph/source and log inspection, in-memory
  old-file reconstruction. No source/test edits, builds, test/SQLx execution or DB
  activity. Only findings and review notes were changed.
- Findings: **F-004 resolved**, F-001–003 remain resolved, no new findings.
  No production gap, architecture reopening or user decision.
- Result: **PASS — B1 ready for root acceptance** on manifest SHA256
  `ef2e0ecd1ef49a5ebea6c0ddb60f42a92cb92d56b1a3300575b4513c5c9aafdb`.
  Root retains formal acceptance authority; B2/B3 and final group C remain pending.

### R-008 — whole-batch group B2 review

- Reviewer: S, `thr_xku2jgixtv`, assigned Codex / `gpt-6-astra` / high;
  same confirmed role/settings as R-001. Assignment S-008; D idle, no workers.
- Artifact: HEAD `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006` plus
  [source-manifest-B2.json](logs/groupB/source-manifest-B2.json), SHA256
  `19abba9a05d1f619339b6a50772ab83fda7de2ee5145a36288523d5b27198a28`.
  Verified all 21 hashes, exact 18 tracked changes and three untracked source
  additions, empty index, HEAD, saved tracked diff SHA256
  `aa7fdbadefbaef6bcc9bf6ab3366528482e4699ca01bdad784ed78fe9683e869`,
  and actual binary. Workflow coordination/log artifacts excluded. Exactly the
  declared three paths differ from accepted B1r1. Removing the child-module
  registration from the B1 file reproduces its accepted hash. Group A helper
  changes are four visibility changes plus rustfmt line wrapping/trailing comma;
  bodies are unchanged. Final artifact identity rechecked.
- Requirements/coverage: phase 04 item 7/relevant acceptance, BRIEF-04.8 result/
  receipt/cancel and duplicate/restart rows, DEC-001/002/004/008. Read the complete
  new test file, calibrated blocker/probe/drain helpers, COMMIT gate implementation,
  real receipt validation/settlement/resolution/return authorization, provider
  preparation, lease bounds, recorder and retained CI work-budget behavior.
  Graft call traces confirm return_authorized is called after commit_remote's
  settlement; reconstructed duplicates enter the same real writer.
- Cancel-first: actual cancellation is blocked on the exact execution gate;
  free/held run probes establish run ownership without fixture contamination.
  Actual receipt settlement then waits on its run query behind the cancellation
  PID, while the cancel future remains polled. Gate release requires Applied and
  Interrupted, then free run and drained backends. Durable assertions bind the
  one receipt/observation to exact entry, invocation, argument digest and result,
  retain one effect/marker/entry, retire the attempt, prevent execution output/
  route/successor, and require the unresolved-at-cancel reconciliation witness.
- Receipt-first: the deferred receipt trigger gates COMMIT after observation and
  receipt writes; held run probe and cancellation's actual head/run Lock wait
  behind the receipt PID establish the opposite serialization order. The final
  no-reconciliation-row witness distinguishes this order from cancel-first.
  Applied and Interrupted are required, not arbitrary errors. However, no assertion
  excludes late COMMIT release and successful ambiguity resolution: **F-005**.
- Live and token cases: the ungated genuine accepted effect returns Committed
  under the common three-second policy, defeating a refusal-only implementation.
  The token seam is at real finish_remote entry after service result selection
  and validation; it cancels only then and delegates settlement to completion.
  Real finish_remote must return Committed before the service suppresses success.
  One call/effect, one exact receipt/observation, unchanged ownership/control facts
  and fresh live receipt replay without another write/call establish the intended
  token-only contract. B1 drop-before-observation coverage remains unchanged.
- Duplicate/restart: after each cancelled race, redelivery goes through genuine
  finish_remote and exact durable entry verification. Whole-database equality
  forbids additional observation/receipt/entry/conflict/effect; a fresh service
  refuses cancelled dispatch and old-fence completion/new claim remain inert.
  Named 04.7 conflicting-truth/canonical-first cases are reused in the full action
  run rather than copied. No new production behavior was introduced.
- **Question 1 — yes, narrowly:** rebuilding the consumed in-crate entry is a
  valid fixture for repeating the identical accepted observation. It retains the
  original marker, entry, fence/request, live-read immutable action and result.
  Reconcile plus no registration makes prepare_provider produce the same empty
  contract/key; its supplied retention value is unused on this branch. The real
  verify_entry_on checks durable identity/proof. The synthetic monotonic deadline
  is not used by finish_remote, which intentionally persists late truth independently
  of live transport permission. This construction must not be reused as proof of
  a new authorized dispatch or changed to bypass the real entry verifier.
- **Question 2 — conditionally yes:** DEC-012 explicitly permits the existing
  six-second policy only for this renewed receipt-first fixture, with F-005's
  primary-attempt discriminator. It doubles the fixture attempt bound from one
  to two seconds; it is not neutral merely because production configuration is
  unchanged. Retained three-second live coverage and CI work-phase checks provide
  resource signals, while the race itself must reject an expired primary attempt.
  No common-policy change or blanket six-second policy for B3 is approved.
- **Question 3 — no third blocker required:** the required competing pair's
  run wait is directly observed before gate release. Source inspection establishes
  company-before-run return authorization, and group A independently exercises
  that caller's contention. After receipt commit, cancellation may finish before
  return authorization is scheduled, so an actual company wait need not occur.
  Applied/Interrupted with the durable assertions is sufficient for this output
  suppression obligation, but Interrupted alone does not prove that literal wait.
  Treat D's statement that it waited as an inference, not observed third-blocker
  evidence; an additional gate would test a stronger ordering than required.
- B3 guidance: DEC-005's exact completion winner/result priority and DEC-002's
  real run blockers/calibrated probes remain. Added DEC-012 requires keeping the
  ordinary successful attempt distinct from ambiguous recovery when reusing a
  commit gate. B3 still owns accepted uncertainty/reconciliation rechecks; no
  production gap or architecture reopening follows from this test-only finding.
- D evidence inspected, not rerun by S: all four new tests pass three exact
  isolated runs (12/12), with expected blocker statements in each race log.
  Current B2 filter **4/4**, B1+B2 **14/14**, full action **324/324**, workflow
  control **29/29**, stock 2 MiB/default parallelism, task DB, no ignored tests
  and zero DB/client residue. Raw logs match result JSON. Binary SHA256
  `78998780a0d13a8a965214747bce7db459116d22a4f13bff4c7cee0c206a2901`
  matches actual file, headers, manifest and post-Clippy rebuild. Current fmt/diff/
  offline all-targets/strict Clippy and live SQLx prepare/check exit 0. Independently
  verified all 45 cache hashes equal before/after/current/B1. These green results
  do not establish the unasserted ordinary-attempt property.
- Checks by S: read-only Git/hash/source/log/graph inspection. No source/test edits,
  builds, tests, SQLx execution or DB activity. Only review records and DEC-012
  guidance were written. Group A/B1 accepted implementation is otherwise retained.
- Findings: **F-005 open**; F-001–004 remain resolved. No confirmed production
  defect, architecture reopening or question requiring a user decision.
- Result: **CHANGES REQUIRED — B2**, manifest SHA256
  `19abba9a05d1f619339b6a50772ab83fda7de2ee5145a36288523d5b27198a28`.
  Recheck F-005 on the correction artifact before B2 acceptance.

### R-009 — B2r1 correction recheck, F-005

- Reviewer: S, `thr_xku2jgixtv`, assigned Codex / `gpt-6-astra` / high;
  same confirmed role/settings as R-001. Assignment S-009; D idle, no workers.
- Artifact: HEAD `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006` plus
  [source-manifest-B2r1.json](logs/groupB/source-manifest-B2r1.json), SHA256
  `1b109cc661ccf7dd6b692f06f2882e7d8152325355931bd23d1d2184951eeace`.
  Verified all 21 file hashes, exact 18 tracked changes and three declared
  untracked source additions, empty index, HEAD, saved tracked-diff hash and actual
  test binary. Workflow coordination/log artifacts excluded. Only the receipt-test
  file differs from R-008. Reconstructed its old B2 bytes from the previously
  inspected block/imports and matched the B2 SHA256: all changes are confined to
  recorder/resolution imports and receipt-first construction/oracle/controls.
  No SQL or other test/source changed. Final identity rechecked.
- Coverage: F-005 and DEC-012, retaining R-008's actual blocker, durable truth,
  identical-redelivery, token-only settlement and fresh-service assessments.
  Read the complete correction, existing recorder semantics and resolution-wait
  helper. The unchanged production receipt path and bounds were reviewed in R-008.
- **F-005 resolved:** `ordinary_receipt` requires exactly one recorded
  `action.commit_remote` operation with outcome `completed`, and release strictly
  before the conservative primary-attempt deadline. The deadline starts before
  constructing/polling settlement; release time is sampled after unlock returns.
  These choices cannot falsely credit a late release as early. Recorder slicing
  excludes preparation and includes the completed settlement before redelivery.
  Default current-thread Tokio tests and directly joined futures keep the
  thread-local recorder scoped correctly; no unrelated runtime task is spawned.
  Multiple/retried, resolved, unknown, missing or late outcomes cannot satisfy
  the ordinary oracle.
- Positive race retains the actual receipt COMMIT gate, free/held/free exact-run
  probes, cancellation's run query blocked by the receipt PID, early-completion
  rejection, Applied/Interrupted results, clean contender transactions and exact
  settled truth. Identical redelivery, fresh refused dispatch and inert completion/
  claim remain after the new oracle. The corrected assertion no longer claims an
  observed company-lock wait that the test does not measure.
- Negative control is substantive: it keeps the gate until a real resolution
  transaction is observed behind the original receipt, then observes cancellation
  behind that resolution PID (the first row-lock waiter). The ordinary branch
  still requires the original receipt PID. After release it awaits both real
  operations, verifies the same business/durable outcomes and redelivery invariants,
  requires a recorded `resolved_applied`, and requires oracle rejection. Thus the
  original F-005 counterexample is exercised, not simulated solely by a timestamp.
  Synthetic controls separately cover resolved/adopted/unknown, not-applied plus
  retry, missing event and release exactly at the deadline.
- Bounds: the conditional six-second fixture policy now satisfies DEC-012.
  Primary attempt remains two seconds; no further bound, production/common policy,
  script or CI invocation changes. The new assertion fails late release and
  non-primary completion; registered controls keep that signal in normal action/
  library execution. The existing CI code-work gate remains unchanged and separate
  from this deliberately stalled COMMIT test.
- D execution evidence inspected, not rerun by S: final binary SHA256
  `b36b7b85efce7d17c9a8223fcf45e17828a7071de36d97c7a1b388eae9255a1b`
  matches actual file, manifest, logs and freshness check. Three exact ordinary
  runs record one completed receipt operation and release margins of
  1968/1976/1968 ms. Three exact live negative runs record resolved_applied and
  explicit ordinary-oracle rejection; the synthetic control passes. Combined
  boundary tests **16/16**, full action filter **326/326**, no failed/ignored
  cases, stock 2 MiB/default parallelism, task fixture and zero DB/client residue.
  Raw results agree with JSON. Current fmt/diff/offline all-targets check/strict
  Clippy each exit 0; build-lock waiting is not a source diagnostic.
- Reused evidence: B2's SQLx prepare/check and workflow-control 29/29, earlier
  unchanged prior-bounds coverage, and accepted group A/B1 assessments remain
  applicable. No SQL, shared control/snapshot helper or metadata changed; all
  45 current cache hashes independently match the B2 cache manifest. All affected
  action tests were rerun on the correction binary.
- Checks by S: read-only Git/hash/graph/source/log inspection and in-memory
  old-file reconstruction. No source/test edits, builds, tests, SQLx execution or
  DB activity. Only findings and review notes were edited.
- Findings: **F-005 resolved**, F-001–004 remain resolved, no new findings.
  No production gap, architecture reopening or outstanding user question.
- Result: **PASS — B2 ready for root acceptance**, manifest SHA256
  `1b109cc661ccf7dd6b692f06f2882e7d8152325355931bd23d1d2184951eeace`.
  R-008's three answers remain, with the lease condition now fulfilled. Root
  records acceptance; B3 and final group C obligations remain pending.

### R-010 — S2 onboarding and B3 guidance check

- Reviewer: S2 `thr_4pqv29gbbd`, assigned Codex / `gpt-6-astra` / high by
  S-010 and TASK.md; replacement for retired S `thr_xku2jgixtv`. Role/settings
  attribution is the assignment/task record, not an independent runtime-setting
  probe. No agents or threads spawned; report to root through final response.
- Artifact: HEAD `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006`; accepted B2r1
  [manifest](logs/groupB/source-manifest-B2r1.json) SHA256
  `1b109cc661ccf7dd6b692f06f2882e7d8152325355931bd23d1d2184951eeace`.
  S independently rehashed all 21 listed files: zero mismatches; index empty.
  Initial Git status agrees with the 18 tracked changes and three untracked
  source additions. Preserved IDE/agent content and coordination artifacts were
  not altered. D2 is concurrently assigned B3; this is a guidance check against
  the accepted baseline, not a frozen review or acceptance of D2's pending work.
- Guidance: DEC-001–012 as read had SHA256
  `d31b87d3f7f99978e59fbb9605e563e785351c846e213c87828c938685ecaba7`.
  Appended B3 readiness confirmation and the group C integration-review checklist;
  existing decisions and IDs unchanged. No DEC-013 or new F-ID allocated.
- Assignment/coverage: read the assigned skill and review-record contract,
  repository/applicable source instructions, TASK/REVIEW/GUIDANCE, R-001–009 and
  F-001–005, B1/B2 implementation coverage and correction summaries, D-009,
  phase 04 item 7/acceptance, BRIEF-04.8 and cancellation-serialization contract.
  Used graft and exact source spans for current completion, cancellation,
  dispatch authority/replay ordering, terminal reconciliation and late truth.
- Completion assessment: `completion.rs:72–173` locks run then execution and
  job/attempt, replays completion before live-window refusal and otherwise commits
  the validated result through the shared writer. `:209–269` preserves exact
  fence/output replay and successor identity. `controls.rs:64–129` checks replay,
  then expected revision before terminal state, and records permitted command
  receipts. DEC-005's cancel-first None, completion-first Committed/stale-cancel
  RevisionConflict, terminal fresh-command result and durable result priority
  match the current implementation.
- R-008 carry: `completion.rs:23–69` has a distinct ambiguous-write resolution
  branch that can return recovered completion truth. A successful business result
  does not alone prove the ordinary attempt. DEC-002/012 already require the
  discriminator and actual blocker evidence; the narrow B2 six-second exception
  does not authorize changing B3 fixture limits. No additional S decision needed.
- R-006 Q3 carry: B1's applied-pending case retains one real effect and uncertainty
  without receipt (`action_cancellation_boundary_tests.rs:548–582`). Existing
  `action_reconciliation_control_serialization_tests.rs:69–93,134–247,342–445`
  exercises applied/lost-result history, real cancellation, stale-snapshot refusal,
  then fresh terminal AppliedRecorded with receipt/evidence, stable execution/job/
  attempt/budget facts and no new dispatch/claim. The actual settlement only
  continues a reconciliation-waiting run (`action_reconciliation/settlement.rs:
  154–202`), preserving terminal audit-only recovery. The late-conflicting-actual
  receipt case (`action_reconciliation_late_truth_tests.rs:333–393`) supplies its
  separate late-truth obligation. R-006 permits this composed coverage: D must
  explicitly map and rerun it, not treat B2's ordinary receipt as verifier recovery.
- Replay assessment: `action_dispatch.rs:85–146,381–446` validates the current
  fence and live ownership before receipt/provider replay branches. DEC-001's
  missing cancelled SafeRepeat/ProviderIdempotency combinations remain required;
  the live safe-replay foundation cannot grant terminal dispatch permission.
- Checks by S: read-only Git/status/hash, graph and targeted source/record reads.
  No source/test/fixture edits, builds, runtime tests, SQLx or DB activity. Earlier
  runtime PASS entries were read as historical accepted evidence, not rerun or
  independently revalidated here. Only GUIDANCE.md and this review note changed.
- Findings/limits: **none new**; F-001–005 remain resolved as recorded. No observed
  production gap or architecture reopening; B3 runtime/code review and all final
  group C gates remain pending. No consequential open question for D/root/user.
- Result: **B3 GUIDANCE CONFIRMED** against the accepted B2r1 baseline above.
  Root should point D2 to the appended readiness/checklist sections; they retain
  existing B3 obligations and add no implementation prerequisite. This is not
  a B3 artifact PASS or final 04.8 acceptance.

### R-011 — whole-batch group B3 review

- Reviewer: S2 `thr_4pqv29gbbd`, assigned Codex / `gpt-6-astra` / high;
  same role/settings attribution as R-010. Assignment S-011; root reports D2
  idle. No workers spawned. S remained read-only on source/tests and fixtures.
- Artifact: HEAD `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006` plus
  [source-manifest-B3.json](logs/groupB/source-manifest-B3.json), SHA256
  `c3379ee330dd4795f1f78a4758aa80c4a832b001ee28c845b015e5881f115131`.
  Independently verified all 24 hashes, exact 19 tracked changes and five
  untracked source files, empty index, HEAD and saved/current tracked-diff SHA256
  `2423cf9841ad85c6f8e6c4062f7cba029ef21016f46693231bb884afe0e1b7e7`.
  Workflow coordination/log artifacts are excluded; preserved prior work remains.
  Relative to accepted B2r1, exactly four paths differ: two new test files and
  two registrations. Removing the three-line completion registration reproduces
  the accepted B1 boundary-file hash; removing the hung-reconciliation registration
  reproduces the control-serialization file's HEAD bytes. No helper body changed.
  Guidance SHA256 `189f37d79cc9bfca3e7885ca744cfff92624980577888e8b31e0ebe4848e6e88`.
- Coverage: phase 04 execution item 7/relevant acceptance; BRIEF-04.8 section B
  completion and parked/reconciled rows; DEC-001/002/005/007/008/012; R-006 Q3 and
  R-008. Read both new files completely, inherited provider/reclaim/cancel/inert
  helpers, exact blocker/probe/drain and COMMIT-gate helpers, recorder semantics,
  actual completion attempt/resolution branches and reconciliation evidence/audit
  assertions. R-010's unchanged production ordering/replay/terminal-continuation
  assessment remains applicable. Graft supplied source spans; ambiguous
  complete_io call edges were not treated as an exhaustive caller inventory.
- Ordinary completion races: a genuine service call supplies one receipt/effect
  and a valid step result before either contender; a genuine renewal preserves
  the common lease. Both end and successor routes use an execution-only gate,
  free/held/free exact-run probes and real operations. Cancel-first observes
  cancellation behind the gate, then completion's run query behind that cancel
  PID. Completion-first observes completion behind the gate, then cancellation's
  head/run query behind completion. Early finishes and timeouts fail. Release,
  awaited results and clean backend transactions are checked. The second owner
  need not acquire a company lock when it is the run-only completion path.
- Winner outcomes: cancel-first requires Applied/None, retained receipt/effect,
  cancel retirement, absent output/route/successor and inert old-fence completion
  and fresh claim. The successor-capable fixture makes no-successor meaningful.
  Completion-first requires Committed and exact prior-revision RevisionConflict,
  persisted output/route/target, one completion event and no cancel event; the
  end route succeeds and the successor route schedules one distinct execution.
  Identical completion replays with whole-database equality. A fresh terminal
  cancel adds only its receipt; a fresh successor-route cancel retires remaining
  work while predecessor execution/job/completed-attempt bytes remain unchanged.
- Primary-versus-recovery discriminator: the race-scoped recorder slice requires
  exactly one completion event with outcome completed; winning completion must
  additionally be Committed. Default current-thread Tokio runtimes and directly
  polled/joined futures preserve the thread-local recorder. The live negative
  control holds the completion COMMIT until resolution is observed, then observes
  cancellation behind the resolution PID. It drains all three backends and proves
  resolved_applied plus Replayed despite the same successful business outcome;
  the ordinary oracle rejects it. Synthetic cases also reject missing/unknown/
  retried/resolved evidence, including the None/refusal form. This satisfies the
  R-008 carry without relying on elapsed-time logging as the oracle.
- Cancelled replay: SafeRepeat and ProviderIdempotency each exercise both original
  attempt and genuine competing reclaim to attempt 2. The real effect/lost-response
  history and retry_safe=true establish the live replay premise, alongside the
  reused live competing-replay positive test. Actual cancellation retires the
  exact current fence while keeping the contract safe; old and fresh services
  then return the exact lifecycle Conflict with no new provider call, effect,
  entry or durable write. Old-fence completion/fresh claim are inert. Cancellation
  does not become evidence of not-applied or a source of replay permission.
- Hung recovery: the provider commits its genuine ledger effect/recoverable result,
  signals readiness, then remains pending. Actual cancellation with an uncancelled
  invocation token leaves a cancel-retired, never-parked history with one effect,
  retained marker/entry and no receipt. Drop is sampled before dispatch returns.
  The real verifier/service then records AppliedRecorded with linked scoped
  evidence/coverage/audit and a recovered receipt; no scheduled job, execution/
  attempt/budget mutation, new effect, dispatch or claim follows. This adds useful
  direct coverage of the cancellation-created uncertainty history.
- **Question 1 — retain the combined test.** R-010 allowed composition and did not
  require this addition, but the never-parked recovery boundary is useful and in
  scope. Its ten-second watchdog/drop/Interrupted assertions do not independently
  prove that ownership supervision, rather than the frozen deadline, caused the
  drop. Read D's causal wording with that qualification: B1's unchanged causal
  positive and always-owned negative control supply that separate proof and were
  rerun. This test is credited for recovery of the exact cancelled history, not
  as a replacement for F-004's corrected oracle. No new finding or test expansion
  is necessary for the composed coverage.
- **Question 2 — yes, retain the common three-second lease.** Both ordinary races
  have a one-second primary attempt and actual cancellation's one-second server
  timeout. There is no bound increase to approve. The six-second policy is confined
  to deliberate ambiguity recovery in the negative control, consistent with the
  existing completion-resolution fixture; it is not an ordinary-race exception.
  The completed-event oracle fails if the primary path is lost; no extra release
  timestamp requirement is needed for this execution-gated ordinary race.
- **Question 3 — yes.** None plus exactly one completion/completed event proves
  ordinary primary refusal. Ambiguous retirement can also return None, but records
  resolved_applied and is rejected. This is the intended R-008 distinction on the
  refusal side as well as for the winner.
- **Question 4 — yes.** The small local successor fixture matches the private
  claim-SQL fixture's six-line construction. It configures this test's independent
  example, carries no duplicated production/domain decision and creates no shared
  behavioral contract requiring visibility changes across those module trees.
- D evidence inspected, not executed by S: all six new tests pass three exact
  isolated runs (**18/18**). Ordinary race logs contain actual statement/blocker
  pairs and free/held/free probes; the three live controls record resolved_applied
  and explicit oracle rejection. Raw result counts agree with result JSON:
  cancellation **24/24**, boundary **21/21**, control serialization **5/5**, claim
  cancellation **4/4**, late truth **4/4**, revision **9/9**, uncertainty **10/10**,
  receipts **12/12**, completion resolution **2/2**, prior-bounds **9/9**, workflow
  controls **29/29**, full action **332/332**. Runner uses stock 2 MiB/default
  parallelism, explicit task DB URLs and no missing-DB allowance; selected/pass
  counts match and no test is ignored. All filter records report zero DB/client
  residue. Task fixture is the recorded PostgreSQL 18.6 system
  `7696100747480141867`; no fresh DB identity probe by S is claimed.
- Binary SHA256 `18d7c86ab7d56031d38450f7a7748d1198ff5f7a8a49ed978b780dc440584bb4`
  matches the actual binary, build/freshness records and isolated/filter logs.
  Current fmt/diff/offline locked all-targets check/strict Clippy and live SQLx
  prepare/check each record exit 0. Build-directory lock waiting is not a source
  diagnostic. All 45 SQLx cache hashes match B2, B3 before/after and current files;
  runtime-query correctness is supported by actual DB tests, not unchanged cache.
- Reused evidence: accepted A/B1/B2 assessments remain applicable to unchanged
  implementation/helpers. Current full-action/boundary/negative-control and
  reconciliation runs re-exercise their integration with B3. Full library, resource
  budgets, fresh schema/checksum and final combined review remain group C gates.
- Checks by S: read-only graph/source/log/Git/hash inspection and in-memory
  registration stripping; no builds, tests, SQLx or DB activity. Only this review
  note was appended. Source identity rechecked before disposition.
- Findings: **none new**; F-001–005 remain resolved. No production gap,
  architecture reopening, changed guidance decision or outstanding user question.
- Result: **PASS — B3 ready for root acceptance**, manifest SHA256
  `c3379ee330dd4795f1f78a4758aa80c4a832b001ee28c845b015e5881f115131`.
  Root records acceptance; group C and final 04.8 acceptance remain pending.

### R-012 — group C precision classification and timing guidance

- Reviewer: S2 `thr_4pqv29gbbd`, assigned Codex / `gpt-6-astra` / high, same
  role/settings attribution as R-010. S-012; D2 owns active background budget
  verification. S ran no tests/builds/SQLx/DB operations and spawned no workers.
- Artifact: HEAD `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006`, B3 manifest SHA256
  `c3379ee330dd4795f1f78a4758aa80c4a832b001ee28c845b015e5881f115131`.
  Rehashed all 24 listed files with zero mismatches. The affected attention,
  schedule, outreach controls/operations/tests, workflow wait production/tests,
  remote-dispatch tests and budget scripts separately equal HEAD bytes. This is
  diagnostic/guidance coverage, not a new frozen full-artifact acceptance review.
- Assignment/authority: user permits the five precision fixes within 04.8 if
  test-only; production findings reopen scope through root. User asks to assess
  causal verification versus quieter-host reruns for timing failures. Read the
  actual failure sections of the full-library log, current rerun result/log
  records, accepted guidance and original boundary/resource requirements. At
  inspection the Group C implementation section was not yet appended; D's
  diagnosis in S-012 was treated as a hypothesis, not a substitute for source.
- D evidence inspected: [full library](logs/groupC/runtime-all-lib.C.log),
  **2550 passed / 7 failed / 22 ignored**, 1607.76 seconds. Actual failure values
  identify 393/481/340 ns lost in the two attention/outreach equalities, schedule
  remainder 3599, and a 477 ns difference between two actual park responses.
  Dispatch failures are a generic lifecycle Conflict and four-second outer
  Elapsed; neither log records an exact failed mode/phase. The unchanged
  [rerun results](logs/groupC/runtime-results.C-rerun.json) show dispatch **20/20**
  and wait **15/16**, binary `18d7c86a…`, zero DB/client residue. No claim of a
  fresh S reproduction or completion of still-running budget scripts.
- Precision coverage: inspected test inputs/assertions, attention SQL projection
  and conversion, schedule DB-slot load/advance and cadence update, outreach
  extension and timeout sweep, workflow deadline parsing/parking/replay and worker
  caller. Local SQLx 0.8.6 codec confirms integer-microsecond serialization.
  The first four are exact test expectation errors; schedule computes from the
  stored slot, so its interval anchoring is not drifting. DEC-013 gives separate
  correction criteria retaining the original domain/concurrency oracles.
- **F-006 opened:** wait first-park response returns a nanosecond input while
  replay returns DB precision. This is actual production response inconsistency,
  not a fixture expectation error. Keep the exact failing equality, return the
  canonical persisted deadline after a narrowly authorized correction, and force
  non-microsecond test input on all platforms. The worker's current is_some use
  limits demonstrated impact; no incorrect timer firing or duplicate effect was
  established. Root must take the scoped production change to the user.
- Timing coverage: read the two dispatch tests and Intercept/PendingProvider,
  actual service pre-poll/heartbeat/deadline select, DB-to-monotonic live-window
  construction, existing B1 causal observation and both budget scripts/recorder/
  CI invocations. DEC-014 recommends the actual captured-deadline seam over the
  compressed 300/400 ms setup race, identifies the pending-join hidden-premise
  failure, and separates token/ownership/deadline evidence. Generic Conflict or
  Elapsed cannot be retroactively classified as a particular lease lapse solely
  from reported host load. Host load ~19 is D/root context, not S measurement.
- DEC-009–012 assessment: quiescent renewals, shorter diagnostic intervals and
  actual event/lock/attempt outcomes improve robustness; none suspends real DB
  leases, the five-second reconciliation command cap or the primary persistence
  deadline. Retain the causal and negative-control oracles, void-attempt ceiling
  and a completed positive witness. Quiet-host default-parallel combined checks
  remain necessary when real-time premises cannot be established under load.
- Budget assessment: lease work is elapsed phase time, still affected by scheduler
  and DB interference even with serial selected tests; inspect phase evidence and
  preserve the 50%/required-operation CI guards. Stack threshold is memory-based,
  but its full test run can fail for unrelated timing assertions; no overflow
  signal is established by the seven failures. Neither non-overflow failures nor
  first-platform calibration justify declaring a failed gate passed or raising
  a limit automatically.
- Changes/checks by S: appended DEC-013/014, F-006 and R-012 only; read-only graph,
  source, local dependency codec, log, hash and Git inspection. No project code,
  fixture or running verification changes. Existing decisions/findings retained.
- Result: **GUIDANCE DELIVERED; group C acceptance remains open.** Four test-only
  corrections may proceed; F-006 requires a narrow production scope decision.
  B1/B2/B3 accepted coverage remains intact but does not waive the combined gate.

### R-013 — S3 onboarding and C1 review preparation (no verdict)

- Reviewer: S3 `thr_4e8he92jaz`, assigned Codex / `gpt-6-astra` / high by
  S-013 and TASK.md; replacement for retired S/S2. Settings attribution is the
  assignment/task record, not an independent runtime probe. No agents/threads
  spawned or messages sent; report to root through final response.
- Assignment: onboard and pre-read the F-006 boundary/callers and DEC-014 tests
  while D2 implements C1. **No review of C1's in-progress implementation and no
  artifact verdict.** D2 retains all source/test/build/fixture ownership.
- Baseline/identity: source bodies were read with `git show 8533a79:<path>` at
  graph/record-directed ranges, avoiding D2's mutable implementation. Accepted B3
  manifest SHA256 remains
  `c3379ee330dd4795f1f78a4758aa80c4a832b001ee28c845b015e5881f115131`;
  original group C manifest is
  `6e4f93aac25f2263de6938b1d70b1b9e1a21e5a247ebd1c80fdc5928f49ddadd`.
  S checked these manifest-file hashes, not their live source contents.
  Guidance as read has SHA256
  `2a6f4b5c92da1da4da93f05bc897ed72faa2c3784ad89d6dce1d37295c1a2f55`.
  During onboarding, actual HEAD was observed as
  `cfef3d8cfb81db7d1136b4da3dec1da1235f494a` (`workflow-foundation 4.8`),
  ahead of TASK.md's recorded `8533a79`. Commit metadata/path inventory includes
  accepted work and precision/wait paths; contents were not assessed. Root/D2
  must reconcile the actual HEAD at freeze and retain D-011's full comparison
  against `8533a79`, including committed changes, for C1's identity and review.
- Records read: the assigned skill and review-record reference; repository,
  source, application and persistence AGENTS.md; TASK.md including user decisions,
  REVIEW.md, GUIDANCE.md (especially DEC-008–014 and group C checklist), R-010–012
  in full and earlier R notes skimmed, F-001–006, IMPLEMENTATION.md's group C
  section, D-011, original phase 04 item 7/acceptance and BRIEF-04.8.
- Scope reconciliation: A/B1/B2/B3 acceptance is historical and remains recorded;
  group C is open. TASK.md's later user decision and D-011 authorize the narrow
  F-006 production correction. The earlier “awaits root/user decision” wording in
  F-006/DEC-013/R-012 is historical, not a fresh permission prerequisite. F-006
  remains open pending correction review; F-001–005 remain resolved. The original
  group C implementation diagnosis calling all five precision failures test-only
  is superseded by R-012/DEC-013's four-test/one-production classification.
- F-006 pre-read: baseline `application/workflow/waits.rs:30–33,52–59,67–107`
  defines exact ParkedWorkflow equality and accepts RFC3339 nanoseconds.
  `persistence/workflow/waits.rs:26–35` commits the transaction before returning;
  `:90–130` serializes on run/execution/job then reads the saved wait. `:145–208`
  replays persisted identity/deadline, otherwise validates/activates, caps against
  the run deadline, inserts, finishes the job and records waiting state/audit.
  Its initial response uses the pre-storage deadline. Review C1 for canonical
  returned storage truth within that same transaction, unchanged capping/error/
  replay ordering, and preserved atomicity; no timestamp tolerance or schema change.
- Caller preparation: `graft callers park_wait --depth 2` reported ambiguous
  duplicate definitions and no indexed incoming edges; this was not treated as
  absence of callers. `graft grep park_wait` supplied the exhaustive indexed
  inventory (44 hits, 16 files). Baseline worker `worker.rs:95–138` uses only
  `is_some()` before resume, not the returned deadline. Read the competing
  park/signal/resume/lost-ack test, ordered cancel/resume caller, deferred control
  rollback, stored-event/downtime and due-timer polling, poison wait deadlines,
  competing timer sweeps/capping and commit-crossing rollback. C1 must keep the
  competing park oracle, deliberately non-microsecond input, exact response =
  stored row = replay, and one wait/successor/event with the existing late/replay
  checks. Affected wait/control/polling execution and live SQLx preparation remain
  required; this runtime SQL need not generate a cache delta.
- DEC-014 pre-read: baseline `action_dispatch_remote_tests.rs:7–65,101–130,
  201–282` has the 300 ms lease/400 ms post-entry delay and four unnamed pending
  modes. A refused dispatch can leave its joined sibling awaiting `started` until
  the outer watchdog. Baseline application `actions/dispatch.rs:132–200` performs
  final token/monotonic checks before polling, then independently selects token,
  deadline and heartbeat; it drops the actual provider future before return.
  Persistence `action_dispatch.rs:481–530` limits entry to both current and
  reservation-frozen lease/run windows. Review C1 for an actual captured expired
  entry, live token and zero first polls with the oracle control, plus named
  pending modes, immediate failed-premise reporting and bounded draining. Do not
  credit eventual Interrupted alone as an ownership-refusal witness or allow a
  lease/token termination to stand in for the pending run-deadline case.
- Group C handoff checks: inspect the frozen complete manifest, guidance revision,
  current binary/toolchain and task DB identity, exact selected/ignored counts,
  causal negative controls including `prior_bounds_`, load figures and residue.
  Full default-parallel stock-2-MiB library and both budget scripts must succeed;
  “no stack overflow” with a nonzero script exit is not a passing gate. Bind static,
  SQLx, migration/schema/checksum and graph evidence to the final artifact or
  explicitly justify reuse of unchanged evidence. Original C records an unverified
  CI egress-isolation wrapper; retain that limitation in the final disposition.
- Checks/limits: S performed graph discovery, targeted baseline Git reads,
  record reads and manifest/guidance hashes only. Graph API discovery reflected
  current symbol signatures; no C1 body/diff assessment followed. No builds,
  runtime tests, SQLx, DB access, fixture changes or independent validation of
  D's execution logs. Only this onboarding note was appended; no new DEC/F ID.
- Result: **READY FOR C1 REVIEW; NO VERDICT.** Existing guidance is sufficient
  for the authorized batch. Await D2's completed, frozen artifact and evidence;
  F-006 closure, C1 review and final combined group C acceptance remain pending.

### R-014 — C1 whole-batch review and F-006 recheck

- Reviewer: S3 `thr_4e8he92jaz`, assigned Codex / `gpt-6-astra` / high; settings
  attribution as R-013. S-014; root reports D2 idle and project edits frozen.
  No workers spawned; no source/test/fixture writes, builds, tests or DB activity.
- Artifact: HEAD `94c77e0893248a45f338de5701649cea8f9b3c3d`,
  [C1 manifest](logs/groupC/source-manifest-C1.json) SHA256
  `e59bb3f02615d685854ae03bae674f57017acc0b66bbdb9ee89c12d230be7dfd`.
  S verified all 30 worktree hashes and byte equality with HEAD, the exact source
  change set since `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006`, empty index,
  no untracked project source, and exact saved/current source diff SHA256
  `18eb1398d8334ef65763858c1b245e4571baf1ca3919999ecc269a02b364d0ce`.
  All 24 accepted-B3 file hashes are unchanged; C1 adds six paths to that manifest.
  Other changes since base are workflow coordination/evidence artifacts only;
  preserved unrelated content was not altered. Guidance SHA256 remains
  `2a6f4b5c92da1da4da93f05bc897ed72faa2c3784ad89d6dce1d37295c1a2f55`.
- Coverage: DEC-013/014, F-006, repository resource/SQLx requirements and D-011.
  Read the complete six-file C1 diff and needed surrounding bodies/callers, the
  recorded negative check, targeted/static/full-suite evidence, and actual lease
  window construction/renewal/entry/supervision. R-013's original-requirement and
  caller preparation remains applicable. Graft traced `park_on` to `park_wait`;
  the ambiguous trait-method caller inventory from R-013 was retained.
- Precision fixtures: the two attention tests now bind microsecond-exact expiry/
  generation instants, preserving exact priority/responsibility/due-date and
  reversed-row-age ordering assertions. Outreach still runs genuine competing
  extension and timeout sweep with exact persisted expiry and both waiting states.
  Schedule changes are confined to tests: exact input slot, claimed scheduled_for
  linkage, positive whole-hour advance at microsecond resolution, future and
  roughly-half-hour checks remain. No tolerance, production normalization or
  schema alteration is introduced.
- F-006: `waits.rs:189–196` obtains the actual INSERT-returned deadline. Binding
  order, original UUID, run-deadline cap, invalid-input settlement, lock order,
  job completion, run state and audit writes are unchanged. `park_wait` returns
  only after transaction commit; concurrent calls serialize and replay the stored
  row. The application parser still accepts nanoseconds; worker still uses only
  response presence. The regression's explicit 477 ns input cannot accidentally
  become microsecond-exact on another clock platform. It checks both competing
  responses, stored min(requested, run deadline), and identical park replay;
  signal/resume/lost-ack, one wait/event/successor and conflict checks remain.
- F-006 negative evidence: [negative-check log](logs/groupC/F006-negative-check-C1.log)
  records the pre-fix INSERT restored temporarily, binary `b674d294…`, and an
  expected exit 101 with identical wait IDs but `.611631477Z` versus `.611631Z`
  deadlines. It records restoration to fixed hash `3975761c…`, independently
  matched to current HEAD/worktree. This is D's executed countercheck, not an S
  reproduction. Final positive executions use the different fixed binary below.
  **F-006 resolved** on this artifact.
- Delayed first poll: the delegating writer obtains a genuine authorized entry,
  awaits its own captured monotonic deadline, and returns it unchanged. Genuine
  quiescent renewal replaces artificial 300 ms lease compression; no renewal
  extends an already frozen reservation. The oracle requires entry reached,
  expired hand-back, live invocation token, Interrupted and zero provider calls;
  the new synthetic control rejects a poll, no entry, early hand-back, token
  cancellation and wrong outcomes. Existing marker/effect/receipt facts remain.
- Pending modes: named cases and a select against premature dispatch completion
  expose the former hidden first-poll premise. Futures are owned/polled directly,
  with no detached task; the four-second timeout drops owned work on failure.
  Ownership/cancel modes require a completed real false probe after mutation and
  return before the captured deadline. Token mode rejects recorded ownership
  refusal and return at/after deadline; the only mutator cancels the token after
  first poll. Exact one poll/drop-before-return and durable action facts remain.
  Actual CancelCommand and stronger token durable-state coverage continue to be
  supplied by the unchanged accepted B1 tests, not these raw-SQL fixture modes.
- **F-007 opened:** the RunDeadline mode's upper-bound comparison is insufficient
  to distinguish a run-limited entry from an earlier lease-limited entry. A delay
  between renewal and setting the run deadline can reverse those deadlines while
  every current assertion still passes. See the concrete trace and correction
  criterion in the finding. This is source-derived, not an attribution of the
  recorded successful runs or a production error.
- Raised-window assessment: 0.4 s to 1.5 s is fixture setup allowance, not a
  production latency promise; DEC-014 permits removing that compression. Current
  production policy, persistence caps, heartbeat/guard, four-second watchdog,
  stock-stack and CI 50% work guards are unchanged. The required replacement
  failure signal is a genuine pending run-deadline witness, plus the existing
  early failed-premise and poll/drop assertions. That signal needs F-007's
  correction; merely retaining the watchdog and adding an upper bound is not
  enough. No broader bound change or reversion to guessed sleeps is required.
- D evidence inspected: all eight changed/new tests pass exact isolated x3
  (**24/24**), matching each raw log and fixed-binary hash. Affected filters pass
  attention 19, task 113, schedule 2, wait 16, dispatch 21, control 29, polling 13,
  maintenance 12, poison 12 and activation-budget 9; selections match listed counts,
  zero ignored and zero DB/client residue. fmt/diff/offline locked all-targets/
  strict Clippy/live SQLx check each record exit 0. All 45 SQLx cache hashes are
  unchanged from group C and independently match current files. Runtime-query
  correctness is supported by real DB execution, not inferred from cache equality.
- Binary: actual fixed lib SHA256
  `8fd635ae795e0f8e60c78b4746a5ae1a13635d99e7dd4878564fccaa86d4a4f4`
  matches build/freshness/targeted/combined records. Shared-target lock waiting is
  not a diagnostic. Full/budget evidence is assessed in R-015; green runs do not
  establish the unasserted RunDeadline premise.
- Findings: F-006 resolved; **F-007 open**; F-001–005 remain resolved. No new
  production defect or scope reopening. No changes to existing guidance IDs.
- Result: **CHANGES REQUIRED — C1**, manifest SHA256
  `e59bb3f02615d685854ae03bae674f57017acc0b66bbdb9ee89c12d230be7dfd`.
  Correct/recheck F-007 before batch acceptance. Precision/F-006 and unaffected
  accepted A/B assessments can be reused if their bytes remain unchanged.

### R-015 — final combined group C integration review

- Reviewer: S3 `thr_4e8he92jaz`, same assigned role/settings and read-only limits
  as R-014. S-014 part 2; root retains final acceptance authority.
- Artifact: HEAD `94c77e0893248a45f338de5701649cea8f9b3c3d`, C1 manifest SHA256
  `e59bb3f02615d685854ae03bae674f57017acc0b66bbdb9ee89c12d230be7dfd`,
  same exact source/guidance identity checks as R-014, rechecked before disposition.
  Earlier A/B reviews are reused only for unchanged covered content; all 24 B3
  manifest files match, and C1's complete six-file delta is reviewed in R-014.
- Requirements/coverage: original phase 04 execution item 7/relevant acceptance,
  BRIEF-04.8 sections A–C and GUIDANCE group C checklist. Matched all **64** exact
  boundary-map names in IMPLEMENTATION.md/group C coverage file to successful
  current full-suite results. The additional C1 oracle is present separately.
  Reviewed actual central authority/dispatch/receipt/completion/control/wait and
  reconciliation continuation paths, alongside prior A/B actual-code reviews and
  the current causal/ambiguity/void negative-control execution evidence.
- Combined behavior: action authority still prelocks company using an unlocked
  actor hint, reloads actor under run lock, and verifies current fence, frozen
  scope/digest and policy before effects. Receipt commit and completion remain
  run-first without an added company acquisition. Receipt return acquires company
  before run and propagates operational errors; known lost ownership suppresses
  success only after releasing its read transaction. F-006's wait INSERT changes
  only returned precision inside its existing run-first transaction and introduces
  no company lock, action permission or new scheduling path.
- Cancellation/late truth: actual observed provider results settle before caller
  suppression; dropped unobserved work remains uncertain. Current authorization
  precedes replay permission, so SafeRepeat/ProviderIdempotency cannot bypass
  terminal state. Completion checks committed replay before live refusal, while
  controls check expected revision before terminal outcome. Reconciliation
  continues only reconciliation-waiting runs, so terminal recovered truth is
  audit-only. C1 does not change these predicates. Accepted B1 rollback/same-fence
  usability, B2 exact receipt/cancel blockers and restart truth, B3 completion
  winner/ordinary-versus-resolved outcome and hung-history recovery evidence remain
  applicable; current combined runs re-exercise their integration. F-007 affects
  the separate pending deadline test's proof, not these inspected production paths.
- Full execution evidence inspected, not run by S: full library **2558 passed /
  0 failed / 22 ignored** out of 2580; bin **5/5**. Raw per-test results reconcile
  with summaries (including should-panic cases). Runner sets stock **2097152**
  byte libtest stacks, explicit task DB URLs, unsets missing-DB allowance, and
  retains default parallelism. Exact listed ignored tests match the documented
  optional HydraDB/live-DNS/developer-aid exclusions. All 64 map entries and the
  F-004/F-005/completion ambiguity controls pass; `prior_bounds_` is **9/9**, including
  the main proof and exhaustion/error discriminators. Full/filter records show
  zero own-DB/client residue. No new live fixture probe is claimed by S.
- Resource gates: unchanged script hashes match current files and logged hashes.
  `stack-budget.sh --offline` exits **0**, 2558/0/22 at **2048 KiB**, without an
  overflow. `lease-budget.sh --offline` exits **0**, **88/88** (84 inline results
  plus four results with interleaved diagnostic output). Independently inspected
  1164 non-injected JSONL events and 92 excluded injections: required operations
  are present; maximum individual work share is `budget.reserve` **352/1000 ms**,
  below the unchanged 50% threshold. All action maxima are <=45 ms. Test count
  rises by one due to the new first-poll oracle. Scripts/CI/config/production
  limits are unchanged; C1's fixture-window assessment is in R-014/F-007.
- Load/identity: recorded one-minute load at full run **3.93 → 2.84**, stack
  **2.84 → 5.25**, lease **5.25 → 2.47**, with full three-average figures retained
  in `load-C1.log`. Boundary process scans report no foreign cargo/rustc; they
  do not establish continuous host exclusivity. Both actual binaries match the
  logs: lib `8fd635ae…a4f4`, bin
  `b5b4ea2719cc280fe52134bfa0c362e561457b00a9d73d620db0341aff79a134`.
  D records PostgreSQL 18.6 system `7696100747480141867`, rustc 1.99.0 and sqlx-cli
  0.8.6. S independently verified file hashes, not running process/DB identity.
- Static/schema reuse: current fmt/diff/offline all-targets/strict Clippy/live
  SQLx evidence passes, and all 45 cache hashes match B3-era C and current files.
  C1 changes runtime INSERT/SELECT strings but no cached macro query, schema,
  migration, dependency, scripts or CI. Fresh/task schema dumps are byte-equal,
  SHA256 `cfd8c4bae0f9cdd112df77f48b49340babc8b76e12fa5d5f5cd5762dc11c90ef`.
  S independently compared all 53 saved fresh/task catalog rows and SHA-384
  checksums against current migration files; all match and report success.
  Therefore the prior fresh migration/schema gate is valid reusable evidence
  for these unchanged schema inputs. The edited runtime query is additionally
  exercised by C1's actual wait tests and renewed SQLx command; unchanged cache
  alone would not validate it.
- **F-008 opened — final graph refresh still needed.** The only recorded full
  `graft build`/zero-delta evidence is the B3-era C run before C1. New test symbols
  and changed spans require a final refresh/record after corrections. Incremental
  graph discovery is not a replacement for that explicit final gate. No need to
  rerun runtime tests solely for an ignored graph-cache refresh.
- Network-isolation deviation: local runs omit CI's `test-network-isolation.sh`
  wrapper (`sudo iptables`); the suites therefore do not prove correct behavior
  with external egress denied, nor prove accidental live-network calls impossible
  locally. Cargo offline mode restricts dependency access, not runtime sockets.
  CI still invokes the wrapper for all-targets, stack and lease suites. This does
  not negate the observed task-DB/fixture results or show a C1 regression, but
  root must retain the limitation rather than describe local verification as an
  exact reproduction of CI. No new external provider integration is credited.
- Finding reconciliation: **F-001–006 resolved**, with original histories retained;
  **F-007 and F-008 open**. No new combined production defect established. S
  appended findings/review records only; no execution, fixture mutation or final
  graph build was performed. Raw logs and records were inspected rather than
  replaying accepted work or running competing checks.
- Result: **CHANGES REQUIRED — final group C**, manifest SHA256
  `e59bb3f02615d685854ae03bae674f57017acc0b66bbdb9ee89c12d230be7dfd`.
  The supplied runtime/static/schema evidence is green as detailed above, but
  F-007's missing semantic proof and F-008's outstanding final gate prevent an
  artifact PASS. Root should assign the narrow test correction and graph refresh
  to D, then request a finding-based delta recheck with affected current-artifact
  verification. No 04.9, migration squash, commit or deployment is authorized.

### R-016 — F-007 correction recheck and C2 gate-evidence judgment

- Reviewer: S3 `thr_4e8he92jaz`, assigned Codex / `gpt-6-astra` / high, settings
  attribution as R-013. Assignment S-015; D2's gate orchestrator remains active
  and source is frozen. S performed read-only source/Git/hash/log inspection;
  no tests, builds, SQLx, DB access, process interruption or source edits.
- Artifact: HEAD `94c77e0893248a45f338de5701649cea8f9b3c3d` plus only
  `src/adapters/persistence/workflow/action_dispatch_remote_tests.rs` modified,
  SHA256 `36e0f1a287be1c7720809e2d46700861558fe02196bccba1747d49d50d3aafb4`.
  Compared all 30 paths with C1 manifest SHA256
  `e59bb3f02615d685854ae03bae674f57017acc0b66bbdb9ee89c12d230be7dfd`:
  exactly that path differs; the other 29 retain their C1 hashes. Source status
  likewise has only that modification and no additional source files. C2 manifest
  is not yet supplied; this identifies the reviewed correction without pretending
  the final handoff is complete. Coordination/evidence files are separately active.
- Coverage: complete correction diff against C1, F-007 criterion, actual entry/
  lease-window semantics retained from R-014, current positive/control execution,
  and S-015's direct-full versus stack-full evidence question. IMPLEMENTATION.md
  had no C1-correction section yet at inspection; inspected actual code, D-012/013,
  `quiet-rerun-C2.sh`, its stdout, load records and raw logs instead.
- **F-007 resolved.** The delegating seam now records persisted run and job lease
  remaining durations immediately after real entry, with before/after monotonic
  timestamps around that query. Each DB clock sample lies within those brackets;
  the comparison does not need the two clock_timestamp calls to be identical.
  `run_limits_entry` rejects absent/lapsed/reversed windows, requires run upper
  bound + **250 ms** <= lease lower bound, and entry deadline <= run upper bound.
  Together these establish a clear run-before-lease premise with the unchanged
  entry min-window implementation, rather than relying on the post-UPDATE 1.5 s
  upper bound alone. No concurrent fixture renewal or mutation changes the two
  windows. First poll must remain live; return/drop/outcome and probe assertions
  still distinguish pending deadline, ownership loss and token interruption.
- Correction control: `RunPremise::LeaseFirst` delays two seconds into the genuine
  three-second renewed lease before setting the same 1.5 s run window. It runs the
  same actual pending service, requires Interrupted, one poll, drop-before-return,
  unchanged action facts, live token, old post-update upper bound, return at/after
  captured expiry and no recorded refusal. The real control is then rejected by
  the run-limited oracle. Constructed controls separately reject equal/lapsed/
  close-window/missing observations and entry later than run, with a valid positive.
  No production bound, watchdog, common policy or CI resource threshold changed.
- D evidence inspected: a1 positive and live negative each pass exact x3 (**6/6**)
  at stock 2 MiB on binary
  `94a422dc15bc09486aeb99872f5ecf140982df93183eec696e0db072a0c8fd5e`, whose
  current bytes match build and run hashes. Positive RunDeadline logs show about
  **1.434 s run / 2.929 s lease** remaining, live poll, affirmed ownership and
  return at captured expiry. Negative logs show about **1.433 s run / 0.926 s
  lease**, empty probe history, otherwise successful Interrupted/drop, and exact
  rejection “the lease ends no later than the run deadline.” Dispatch filter is
  **22/22**, zero ignored/residue. Both cases also pass in a1 full and stack logs.
  Earlier `loaded-aborted` artifacts were read as history, not substituted for
  the completed a1 targeted evidence.
- A1 gate facts: direct full suite on that binary reports **2558 passed / 1 failed /
  22 ignored**, 2581 selected; bin **5/5**. Subsequent unchanged
  `stack-budget.sh --offline` runs the same lib binary, all **2581** tests at
  **2048 KiB/default parallelism**, and exits **0** with **2559/0/22**. S parsed
  raw names/statuses and reconciled both counts, including should-panic cases.
  Stack script/hash and binary before/after are unchanged. Explicit task DB URLs
  and missing-DB allowance unset match the direct runner. The lease gate later
  recorded **89/89**, exit 0, same binary; this note does not replace the final
  completed gate-table/SQLx/manifest/graph audit.
- Load evidence: a1 began targeted at 0.54 one-minute load/no foreign processes,
  direct full at 3.62/no foreign. Full ended 3.43 with four foreign processes;
  stack began then and ended 8.28 with two. Intermediate sampling also records
  foreign activity. Thus the failed direct run overlapped other work, but this
  alone cannot explain its particular assertion; the green stack run was not
  itself a quiet-host run. S did not interfere with the orchestrator or other work.
- Failure diagnosis: direct full fails
  `stop_task_serializes_with_final_dispatch_without_duplicate_customer_reply`
  at `task/tests.rs:7532:75`, **Processing versus Completed**, in the branch where
  stop returned Conflict and publication returned Committed. This is a legitimate
  dispatch-first serialization, not a generic timeout/lease-loss report. The
  helper claims a five-minute task lease. Inspection of `commit_agent_dispatch`
  (`task/operations.rs:1524–1780`) shows that publication commits reply/deliveries
  and outreach completion/version, while task completion is a separate fenced
  operation (`:2381–2411`) called by worker `close_out_task` (`task_worker.rs:
  598–713`). The test bypasses that worker and never calls completion. Stop's
  stale outreach version is refused before applying a stop mutation. The observed
  Processing state is therefore consistent with the tested operation; the
  expectation of Completed is not. **F-009 opened: inherited test-oracle defect.**
- History/classification: the failing test body and both task persistence
  implementations equal `8533a79`. C2 only alters workflow test helpers; C1's
  other test timestamp edit does not change this test. This exact test passed in
  group C direct/stack, C1 task-filter/direct/stack and a1 stack. Those logs do
  not identify winner branches; they cannot prove the dispatch-first assertion
  was previously valid. Scheduling/load can change race order, but a quiet host
  cannot guarantee the asserted contract. No production correctness defect,
  duplicate customer effect or new C2 regression is established by this failure.
- **Full-suite evidence judgment:** a clean stack-budget invocation is acceptable
  evidence for the full-stock-stack library gate as well as the stack gate when
  binary/source, selection, stack, default parallelism and DB environment match,
  as they do here. The wrapper actually runs `cargo test --locked --lib --offline`;
  it is not merely a stack-size probe. BRIEF section C does not require a second
  differently named direct invocation to be clean for the same suite. Preserve
  the failed direct result and the local egress-isolation limitation; do not
  relabel a1 as wholly passed or claim quiet-host conditions. Root may account
  for the green stack run as one valid complete execution rather than demand a
  duplicate direct run solely for its command name. D-013's current retry pipeline
  remains unchanged by S.
- **Acceptance consequence:** that equivalence does not close F-009. A known
  invalid winner assertion remains a CI failure waiting for another schedule,
  so neither a green stack run nor another quiet direct rerun justifies final
  PASS. Root should scope a narrow test correction preserving both serialized
  outcomes/no-duplicate truth and the real worker-closeout boundary; source
  criteria are in F-009. Production behavior must not be changed just to make
  the mistaken Completed assertion pass. Corrected-source evidence is then
  required under the existing review/verification rules.
- Dispositions/limits: **F-007 resolved; F-009 open. F-008 unchanged/pending**,
  with no graph verdict in this round. F-001–006 remain resolved. C2 final manifest,
  completed gate/SQLx evidence and integration disposition are not yet supplied.
  Only findings and this note were appended; no new guidance ID or source work.
- Result: **PASS for the F-007 correction** at the exact source identity above;
  **final group C PASS remains blocked**, now by F-009 in addition to F-008 and
  the unfinished final handoff. No requirement for a redundant direct full run is
  added; the identified invalid test assertion requires correction, not retries.

### R-017 — final C2 finding recheck and combined 04.8 integration PASS

- Reviewer: S3 `thr_4e8he92jaz`, assigned Codex / `gpt-6-astra` / high;
  assignment S-016. Settings attribution and role limits as R-013. D2 idle,
  source frozen. S inspected source, Git state, hashes and retained evidence;
  no runtime tests, builds, SQLx, fixture access or source edits. This round
  extends R-015's combined review and R-016's F-007 review to the final artifact.
- Artifact: HEAD `94c77e0893248a45f338de5701649cea8f9b3c3d` plus worktree,
  [C2 manifest](logs/groupC/source-manifest-C2.json) SHA256
  `5f8181076864e1fe0e1184380bdac8949987b8685faca8333e7bc51c389debf0`.
  Independently verified all 30 hashes and the exact changed project path set
  against base `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006`; no staged changes,
  untracked project source, source mode changes or additional project deltas.
  Coordination docs/logs and ignored graph cache are excluded. Only two source
  paths differ from both C1 and HEAD: task tests SHA256
  `9356e0e2280c63745e9a4221fa74c1eb298e8fffc3eb0971f95d3c7a72614254`,
  remote dispatch tests SHA256
  `36e0f1a287be1c7720809e2d46700861558fe02196bccba1747d49d50d3aafb4`.
  The latter is byte-identical to R-016. Saved `source-C2-tracked.diff` exactly
  matches the actual source diff, SHA256
  `ebf95bfc008ff6298c65bbbc647fad491b7555f84177249665fab122007d4b7e`.
- **F-009 resolved.** Inspected the actual task-test diff, `stop_race` at
  `task/tests.rs:7454–7642`, and existing barrier helpers at
  `test_support.rs:588–609`. Each order holds the real outreach row, starts one
  operation, proves one backend waiting, starts the second and proves two
  waiting before release. The dedicated database makes those backend counts
  specific to this test. Both futures remain polled, premature return fails,
  and the post-release timeout is only a hang guard. The assertions distinguish
  stop-first refusal from dispatch-first publication and preserve the latter's
  exact Processing fence, completed outreach and one customer delivery. Only
  then does genuine fenced worker closeout produce Completed, with no second
  delivery. No blanket status allowance, polling-based success or production
  change hides the old error. Final `C2-f2-isolated-stop_task…-{1..5}.log` each
  records both orders, and affected task tests pass 113/113.
- **F-007 remains resolved.** Unchanged R-016 correction; final positive x3 and
  genuine lease-limited control x3 pass on the final binary. The control logs
  retain ordinary interruption/poll/drop facts but reject lease remaining
  approximately 0.923–0.928 s versus run remaining 1.430–1.434 s. This keeps a
  semantic failure signal behind DEC-014's raised run window; production
  bounds, common lease policy and the original dispatch watchdog are unchanged.
  Dispatch filter passes 22/22. Existing cancellation-cause, ordinary-receipt
  and completion-resolution negative controls remain in the full suite.
- **F-008 resolved.** Final full graph build at 2026-10-16T01:07:00Z exits 0;
  16639 nodes, 26557 edges, 862 cards. Independently compared the saved graph
  manifests: 880 files before/after, zero added/removed, three changed (task
  tests card plus extraction/fingerprint caches), matching the recorded delta.
  Query-time F-007 refresh explains its absent delta; the explicit final build
  now fulfills the checklist. Graph-only changes do not invalidate runtime gates.
- Final binary independently hashed as
  `44ada14f77c70b30889bd0b1bea51a8047664e9b944b7faa8dea323dac13afff`;
  final build, f1/f2 logs, before/after stack hashes and post-gate freshness
  rebuild agree. Bin SHA256
  `b5b4ea2719cc280fe52134bfa0c362e561457b00a9d73d620db0341aff79a134`
  also matches actual bytes. No evidence from the superseded F-007-only binary
  is substituted for required corrected-source gates.
- **f1 disposition: failed, retained, not the passing full-suite gate.**
  `stack-budget-C2-f1.log` is 2558/1/22; its single panic is at
  `action_dispatch_tests.rs:94`, the fixture's `settled().claim_io(...).unwrap()`.
  The named snapshot test has not yet reached reserve/entry or provenance
  assertions (`action_reconciliation_snapshot_tests.rs:56–94`). Inspection of
  `lease.rs:28–66` and `persistence_bound.rs:263–355` confirms an ambiguous claim
  whose bounded resolution fails/times out returns this honest Unknown error.
  That is compatible with host contention and the recorded resumed bizi44
  workload (84 foreign samples), but the panic alone cannot prove the precise
  latency/error cause. No contradictory durable-state assertion or new source
  defect is established. Unlike a1/F-009, there is no demonstrated incorrect
  expected state to repair. Accept the prescribed single rerun and preserve f1
  as load-associated failure evidence; do not convert it into a pass or a proof
  that load was the sole cause. No bound, retry policy or assertion was weakened.
- **f2 full-suite gate accepted**, as already permitted in R-016:
  [stack log](logs/groupC/stack-budget-C2-f2.log), exit 0, stock 2048 KiB,
  default parallelism, actual task DB, 2559 passed / 0 failed / 22 ignored,
  0 filtered, 968.30 seconds. Independently counted all 2581 result lines,
  including nine `should panic` tests. This is the full library suite on the
  final binary, not merely a selected stack smoke test. A duplicate direct
  invocation is unnecessary. All 64 exact coverage-map names and all nine
  prior-bounds controls passed. Bin tests 5/5; lease-budget 89/89, exit 0,
  341.63 seconds. Required operation rows have healthy headroom: reserve-budget
  work 349/1000 ms, renew 323/2000, claim 264/2000, completion 237/2000;
  all action work maxima at most 49 ms. Stock-stack and lease scripts hash
  identically to the reviewed versions; no threshold change explains the pass.
- Host accounting: f2 began at load 0.49/1.17/3.74 with no foreign compiler
  processes. Stack boundaries were 6.85/2.79/3.98 to 1.70/5.08/5.80; lease ended
  at 2.82/3.59/4.85. Mid-run sampling recorded foreign work in 15/100 samples,
  peak one-minute load 10.74. Thus f2 is a successful complete run that began
  quiet, not proof of continuous host exclusivity. Its zero failures stand
  despite that interference. Targeted/bin runner residue records are 0|0.
- Static/schema gates: inspected final fmt, diff check, locked offline
  all-targets check, strict Clippy and f2 live SQLx prepare/check logs: exit 0.
  All 45 current SQLx cache hashes equal both C and C1 snapshots. No migrations,
  cached queries, dependencies, CI or gate scripts changed since the reviewed
  schema artifact. Fresh/task schema dumps remain byte-identical, SHA256
  `cfd8c4bae0f9cdd112df77f48b49340babc8b76e12fa5d5f5cd5762dc11c90ef`;
  R-015's independent 53 migration/catalog checksum verification remains valid
  for unchanged inputs. C1's runtime wait query and C2's runtime test queries
  have actual execution evidence; unchanged cache alone is not their validation.
- Combined behavior/coverage: R-015's A/B/C1 architectural review remains
  applicable because C2 changes only these two tests. Company-before-run
  authority locking and revalidation, run-only receipt/completion truth writes,
  return-time authorization, cancellation revision ordering, immutable late
  truth without terminal advancement, and completion replay priority compose
  as reviewed. C1's canonical stored wait deadline changes precision, not
  eligibility/locking/scheduling. IMPLEMENTATION Group C's 64-name map covers
  prepared, lock competition, marker/entry, in-flight work, receipt/result,
  duplicate/restart, completion, parked/reconciled and rollback boundaries;
  each is present and successful on f2. Existing independent negative controls
  and C2's new lease-first control preserve causal discrimination. No new
  combined production defect or coverage blocker was found.
- Finding reconciliation: **F-001–003 resolved R-005; F-004 R-007; F-005 R-009;
  F-006 R-014; F-007 R-016; F-008 and F-009 R-017.** Original observations and
  failed runs remain in their records. No open finding and no new finding.
- Remaining limitations: local verification still omits CI's
  `scripts/test-network-isolation.sh` sudo-iptables wrapper. Runtime external
  egress denial and exact CI network parity are unverified; cargo offline is
  not network isolation. The 22 ignored optional/live-service/developer tests
  are the existing exclusions, not additional coverage credited here. Retain
  these limits and the failed-run/load history in root's acceptance record.
  They do not negate the passing fixture-based gates or establish a regression.
- Result: **PASS — C2 corrections and final combined 04.8 integration**, for
  manifest SHA256
  `5f8181076864e1fe0e1184380bdac8949987b8685faca8333e7bc51c389debf0`.
  This supersedes R-015's changes-required disposition for the final artifact.
  Root owns formal acceptance. No 04.9 work, migration squash, commit or
  deployment is implied; S changed only the append-only review records.


## 04.9-R001 — Architecture gate D1 — CHANGES REQUIRED
- 2026-10-17; workflow implement-astra-reviewed-2.1; Reviewer B requested gpt-6-astra/medium, independent context; authoritative host model/effort/context occupancy unavailable. Base 2729f2caaf58830bea8291bdcc6f9882a3ad03ee; decision 04.9-D1, no implementation digest. TASK.md / REVIEW.md; original D1 preserved in logs/04.9/review-D1.txt.
- Independently read RESUME latest acceptance, original phase04 HTTP/tool/shared acceptance, EXPANSION04.9 and reconciliation, and current D1. Inspected actual SQL action prelock/dispatch/authority and transitive callers, RemoteAction cancellation contract, ToolSnapshot and resource validation/readiness, existing publication rejection, credential context and inventory/CAS rotation. Checked fixed-origin/DNS pinning/proxy/redirect/retry boundaries, frozen recovery/no automatic replay, receipt-before-return integration and failure/verification matrix by inspection only.
- Findings: 04.9-F001 lock-order contradiction; 04.9-F002 actor role mismatch; 04.9-F003 authoritative publication boundary missing from gated decisions. All are decision/integration issues, not claims of new code defects. No tests run and no fixtures mutated; implementation is not present or reviewed. No source files edited.
- Gate justified for new schema/authorization/transport contracts; one cohesive batch remains reviewable. No other architecture blocker identified in the proposed tenant-bound immutable origin, conservative reconcile-only registration, current secret/contract checks, transport limits or redaction decisions. Detailed transport implementation and full mandatory gates remain for code review.
- Process feedback: useful graph spans exposed actual authority and prelock behavior. Candidate narrow instruction: reconcile abbreviated architecture descriptions with current authorization and lock helpers, and name existing fail-closed integration gates when enabling a provider. Supported by F001-F003; no instruction edits authorized or made.


## 04.9-R002 — Architecture correction recheck D2 — PASS
- 2026-10-17; implement-astra-reviewed-2.1; B requested gpt-6-astra/medium, independent retained context; host settings/occupancy not independently exposed. Base 2729f2caaf58830bea8291bdcc6f9882a3ad03ee; exact decision 04.9-D2 saved logs/04.9/review-D2.txt; no implementation digest. TASK.md / REVIEW.md.
- Rechecked full D2 with focus on F001–F003. Reused unchanged R001 actual authority/dispatch/resource/credential inspection. Independently traced `supported_publication` callers (publish, binding selection, admission replay/current selection) and read actual production publication transaction/replay at definitions.rs170–205. D2's immutable replay clause matches existing ordering and does not grant current dispatch access. Verified git status contains only preserved AGENTS/skills and workflow records/logs; no project implementation edits.
- **PASS for architecture decision revision 04.9-D2.** F001–F003 closed at decision level; no new finding. Current authority, lock order, registration scope, child validation, alias handling and real integration coverage are now explicit. Unchanged transport and uncertainty decisions retain R001 coverage; implementation and executable tests are not yet reviewed. No tests run or fixture writes performed.
- Batch size remains appropriate; recheck properly covers corrected decisions instead of repeating unchanged review. No additional instruction change supported beyond R001 feedback. Context continuity retained. Graft reported 8,065 estimated tokens saved this recheck (cumulative review discovery 3,455,153).
