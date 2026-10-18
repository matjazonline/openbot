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


## 04.9-R003 — Whole-batch code and integration review — CODE REVIEW PASS; final gates pending
- **Identity:** 2026-10-17, implement-astra-reviewed-2.1, Reviewer B `/root/reviewer049`, requested gpt-6-astra/medium with independent context; authoritative host model/effort and compaction occupancy unavailable. Records [TASK](TASK.md), [REVIEW](REVIEW.md). Accepted architecture 04.9-D2/R002. Frozen base `e634a82837d50e7ed4aba4bb003f0a98ec294b10`, digest `7b68a7fcdb1c96d8a1e683feae8bc0008b945573c6f846f72cd30b39aa31ab1a`. Reviewed full implementation delta from `2729f2caaf58830bea8291bdcc6f9882a3ad03ee` through the committed crash checkpoint, not only its empty implementation worktree diff. Scope inventory [crash-review-source-files.txt](logs/04.9/crash-review-source-files.txt). Exact exclusions: docs/TASK.md, docs/REVIEW.md, docs/agent-review-notes.md, docs/agent-review-findings.md, docs/logs/04.9 (all beneath this plan). Preserved unrelated guidance/main warning/helper changes are included in digest but outside 04.9 substantive review.
- **Original criteria:** phase04 HTTP/registered-tools and relevant shared action acceptance, BRIEF-04.9, and original D2 text. Read applicable root/source/application/persistence instructions and skill review records. Graft map, source spans, skeletons and caller/grep queries preceded source inspection. No implementation edits, tests or fixture writes by B.
- **Coverage:** inspected every changed production implementation in the source inventory, the migration and added test bodies. Traced authoritative publication calls through definitions, binding selection, admission and replay; exact selected HTTP resource checks; bounded child traversal and continued denial of unsupported snapshots. Checked company/actor-first locking against actual `authorize_company`, action prelock/load/dispatch and receipt return paths. Reviewed tenant composite FKs, immutable origin/authentication identity, revision competition, encrypted context separation, inventory enumeration/keyset pagination and ciphertext CAS. Management serializes connection caps on the company lock; credentials/policies acquire their parent connection before mutation. Rotation acquires no inverse connection/run lock.
- **Runtime integration:** both real WorkflowHandler and direct call_tool paths use ActionService, canonical intent/model-call mapping, current owner/admin authority, frozen capability/schema/recovery and exact bound-resource checks. Current connection/policy/credential reads repeat after DNS. Inspected shared remote supervision, actual-future drop, output schema validation, receipt settlement before return, return-time authorization, and reconcile-only provider contract. Unknown provider errors cannot establish not-applied or trigger blind replay. Shared unchanged reconciliation implementation was reviewed at relevant integration seams, not re-reviewed exhaustively. Legacy worker/harness replacement, MCP action implementation, management HTTP routes and UI remain their explicit later-phase scope.
- **Transport/security:** fixed origin plus separately assigned path/query, allowlisted bounded headers/body/arguments, no credential/header/routing override, HTTPS/default special-address denial, exact operator IP exceptions, all-answer validation and connector pinning, no proxy/redirect/retry, bounded DNS/connect/read/total duration and response/retained sizes. Examined text, JSON key/string and scalar secret redaction plus sensitive JSON/header handling. Shared MCP policy change preserves its prior validation while extracting a vetted pinned-client seam and adding address-port validation; new tests exercise both mixed prohibited answers and the real unresolvable-host connector.
- **Tests/evidence inspected:** current [HTTP log](logs/04.9/crash-http.log) 20/20 and [credential log](logs/04.9/crash-credentials.log) 12/12, including real publish→bind→admit→dispatch, competing dispatchers/revisions, receipt restart and direct-tool mapping, remote uncertainty, revoked connection/secret/operation, approval protection, ordinary-member/foreign authority, forged snapshot and exact bound contract/admission denial, tenant FK/immutable origin, context-bound encryption/rotation/CAS, private-address/DNS/header/origin rejection, redirect/oversize/read timeout, active-secret echo and request cancellation. New tests use actual lifecycle/persistence rather than replacing stored bundles. Inspected fresh migration output and catalog constraints in crash-migrations.log/crash-schema.log; SQLx check, offline all-target compile, strict Clippy and transport/regression/credential CLI logs pass. Gate script/status record formatting/diff and SQLx prepare passing. Previous-machine summaries are not credited as current executable evidence.
- **Outcome:** **PASS for whole-batch code/integration review of the exact digest above; no new concrete finding.** Decision-level F001–F003 fixes are reflected in implementation. This is **not final acceptance**: current full all-target suite (2601 library tests announced) is still running under the network guard, and stock-stack/lease-budget gates have not completed. Required broader results, ignored-test accounting and final snapshot verification remain for the same round's evidence follow-up. No failed/unavailable gate is called passing.
- **Process feedback:** one cohesive ~2600-line batch remained reviewable because authority/provider seams and required decisions were explicit. Crash-checkpoint source inventory was essential: HEAD already contained the implementation, so reviewing only worktree diff would miss it. Candidate narrow instruction for crash recovery: always identify the pre-feature comparison commit as well as frozen HEAD/digest, and rebuild unavailable verification evidence. This is supported by the committed WIP and absent prior-machine logs, not a newly found code defect. No instruction edits made. Context continuity retained; host compaction telemetry unavailable.


## 04.9-R004 — Final verification and documentation follow-up — PASS
- **Identity:** 2026-10-17; implement-astra-reviewed-2.1; same independent Reviewer B `/root/reviewer049`, requested gpt-6-astra/medium; host settings/compaction occupancy not independently exposed. [TASK](TASK.md) / [REVIEW](REVIEW.md). Base `e634a82837d50e7ed4aba4bb003f0a98ec294b10`; final digest `1ad9356a5a29195853d8a5be8e53e4e384465e3365db0ed456ea35f35c76f08b`, same five exclusions as R003. D2/R002 and R003 code/integration coverage retained. No new tests or fixture writes by B.
- **Delta:** reviewed RESUME's crash-recovery/current-record pointers and BRIEF-04.9's replacement of stale pending claims with R003 and TASK/REVIEW references. These change delivery status, not requirements or implementation. Source/migrations/SQLx cache remain unchanged from R003 (the pre-existing main warning and persistence guidance changes are still outside scope). No repeated whole-batch review needed. Final snapshot independently recomputed with crash-snapshot.sh and matches crash-final-snapshot.txt.
- **Final evidence:** independently inspected exact crash-gates.sh commands and complete START/PASS status sequence through graph at 15:06:54Z. Reused R003's current fresh migration/catalog, HTTP20/20, credential12/12 and static/build evidence. Mandatory full all-target check in crash-full-tests.log passes: library **2579 passed / 0 failed / 22 ignored**, 555.28s, and binary **5 passed / 0 failed**, under a guard reporting loopback allowed and external IPv4/IPv6 rejected. Stock-stack log explicitly uses **2048 KiB**, same library **2579/0/22**, 573.85s. No bound was raised. SQLx prepare/check, locked offline all-target compile, strict Clippy, fmt/diff, transport boundary/regression, credential CLI and final graph gates pass.
- **Lease evidence:** crash-lease.log reports **89 passed / 0 failed / 0 ignored**, 140.13s, 50% work-phase threshold. Independently parsed crash-lease.jsonl: 1256 events, 1164 non-injected and 92 injected exclusions, 13 operation names observed. Maximum non-injected work/bound ratio **0.355**, budget.reserve 355/1000ms, below 0.5. Existing admission/dispatch/receipt/uncertainty/lease competition checks therefore passed on the combined unchanged artifact. Some libtest result lines interleave with PROVEN diagnostics; summary totals and zero failed records are consistent rather than relying on a naive line-start count.
- **Ignored accounting and limits:** exact same 22 ignored names in full and stock-stack runs: 19 HydraDB optional/live adapter tests, two SMTP live-DNS smokes, one JavaScript dump developer aid. They are existing exclusions outside this HTTP batch, not credited as executed coverage. No task-required check remains unavailable. The macOS network guard pass is current local evidence; it does not claim a remote CI run or every platform's isolation implementation. Production deployment, later-phase harness/startup replacement, UI and MCP action work remain outside scope.
- **Finding disposition/outcome:** no new findings; 04.9-F001–F003 remain fixed at decision and implementation level per R002/R003. **PASS — final combined 04.9 code, documentation and verification for digest `1ad9356a5a29195853d8a5be8e53e4e384465e3365db0ed456ea35f35c76f08b`.** R003's outstanding verification conditions are now satisfied. Root owns formal acceptance and task-fixture shutdown; no implementation edits or resource cleanup by B.
- **Process feedback:** retaining the same reviewer and immutable code review while gates completed avoided redundant review and preserved gaps explicitly until evidence existed. No additional instruction change supported beyond R003's crash-recovery comparison-commit/evidence lesson. Original R003 pending outcome remains intact; this follow-up supplies final evidence.
- **Follow-up 2026-10-17 (04.10-R002, implement-opus-reviewed-2.1, Reviewer B Opus 5.5):** later-discovered miss. The accepted HTTP handler (`application/workflow/http_runtime.rs:106-123`) labels every non-committed outcome, including pre-marker failures, as Terminal/EffectOutcomeUnknown. With no marker, `retire_on` keeps that label, parks the run in waiting/reconciliation and records no reconciliation fact, so the park is permanent until the run deadline. Logged as [04.10-F008](agent-review-findings.md#0410-f008--http-handler-parks-pre-marker-failures-with-nothing-to-reconcile-align-with-d2-in-this-batch). R003/R004 did not assert run/attempt outcomes for HTTP revocation; the test `http_tests.rs:114-190` checks only that no network call was made. The original PASS text above is preserved.

## 04.10-R001 — Architecture gate 04.10-D1 — CHANGES REQUIRED
- Identity: 2026-10-17; workflow implement-opus-reviewed-2.1; batch 04.10 MCP, stage architecture gate. Reviewer B: Opus 5.5 (claude-opus-5-5), xhigh requested per the TASK workers table; the effective effort and context occupancy are not visible to the reviewer. Independent context, no author transcript. Artifact: base `8ce1fd4` (HEAD), decision 04.10-D1 `logs/04.10/decision-D1.md`, SHA256 `3f10553022b5d631876b8174d57d0ea270ff3c7d150b25d5812fd776e285035e`, verified before review. The copy under TASK.md "Architecture decision 04.10-D1" is identical after trailing-whitespace normalization. No code digest, because this is a pre-implementation gate. Records: [TASK](TASK.md) §"04.10 + 04.11", [REVIEW](REVIEW.md) §04.10.
- Requirements read independently: phase 04 "MCP tool calls over HTTP", "Action contract", "Execution and uncertainty" 1–7 and "Acceptance" (the MCP bullets plus direct/step parity and competing dispatchers); EXPANSION row 04.10 and its cross-point preservation paragraph; the deferred obligations in BRIEF-04.1 (direct MCP through the service), 04.2 (MCP contract authority), 04.4 (no replay from MCP metadata), 04.5 (`isError` classification), 04.6 (parking) and 04.9 (accepted HTTP owner/runtime pattern); root/src/application/persistence AGENTS.md.
- Code inspected (inspection only):
  - MCP catalog: DDL in init_schema (connections/credentials/grants and their FKs); `persistence/mcp.rs` (company FOR UPDATE lock order, endpoint/auth rebind, delete-all `replace_grants`, soft delete that keeps grants, `replace_mcp_token` bumping `credential_revision`); `use_cases/mcp.rs` (owner/admin `managed_company`, refresh grant retention); JSON/UI write builders, which reuse stored discovery only.
  - Credentials: `credential_rotation.rs` (ciphertext CAS, no revision bump, MCP rows already in the inventory).
  - Adapter: `adapters/mcp/{mod,http,worker,policy}.rs` (`EndpointPolicy` wrapped by 04.9 `HttpDestinationPolicy`, try-acquire 32 global/4 per endpoint, `reinit_on_expired_session=false`, `get_stream` disabled, 1 MiB per-operation budget, `notifications/cancelled` plus close on drop).
  - Legacy agent runtime: `services/mcp_runtime.rs` (discover-then-claim, `definition_revision`).
  - 04.9 HTTP owner: `persistence/workflow/{http,http_runtime}.rs` and `application/workflow/http_runtime.rs`.
  - Action dispatch: `action_dispatch.rs` (prelock → fence → attempt → run → `lock_current` → intent); `supervise_remote` and every caller of `validate_action_result`; the `workflow_action_effect_states` view.
  - Recovery: `workflow_action_retry_safe`; `recovery.rs` `retire_on`/`report`/`settle_run`; domain `RecoveryPolicy::decide`; `supervise_io` → `lease::retire`.
  - Publication and binding: registry `mcp.call` contract and MCP content schema; `resources.rs` `inspect_on`/`check_readiness`; `authority.rs` `authorize_company`/`supported_publication`; ToolSnapshot/ApprovedActionPolicy; TypeName rules.
- Assumptions challenged:
  1. Reuse of 04.9 rather than duplication: confirmed. The guarded transport, encrypted credential envelopes and rotation inventory are shared, the HTTP authority/runtime pattern is mirrored, the redaction helper is to be shared, and there is no parallel connection table.
  2. Lock order: catalog writes (company FOR UPDATE), registration (`authorize_company` NO KEY UPDATE, then connection) and dispatch (`prelock_on`, then fence/attempt/run, then connection/operation FOR SHARE) all take the company row first, and no catalog path takes run locks. No inversion found.
  3. No network I/O inside transactions in either the eager or the lazy path.
  4. At most one `tools/call`: it requires marker and entry. The eager session is single-use, reinit is off, and `get_stream` is disabled, so neither SSE resumption nor reconnect can resend. Losing claimants close unused sessions.
  5. The effect-state hint only skips I/O and reservation still decides. A stale hint allows discovery-only I/O (F005).
  6. The `isError` label is overridden by the DB predicate (F002).
  7. Pre-marker and non-committed outcomes are not specified (F001).
  8. An immutable endpoint with a mutable auth mode cannot reach another service, and the `credential_revision` check covers token replacement.
  9. Grants are limited to 32 per connection, so the 256-operation bound is moot but harmless.
- Checks: SHA256 recomputed (match). No tests, fixture/DB access or project edits. Lightly reviewed: rmcp internals (relied on the existing adapter tests that POST is never retried and that resumption only goes through `get_stream`), the legacy harness adaptation, and UI route details.
- Outcome: **CHANGES REQUIRED.**
  - Actionable: 04.10-F001 (medium, outcome table for pre-marker and non-committed paths), 04.10-F002 (low, `mcp.tool_error` safety label versus the shared predicate), 04.10-F003 (medium, race/recovery matrix and affected-suite gate).
  - Optional: 04.10-F004 (declared vs absent outputSchema), F005 (I3 wording), F006 (local capacity consumes attempts), F007 (operation-level CAS).
  - The core architecture holds up: a single MCP owner, immutable endpoint, operations table with grant-FK cascade, routing through publication/binding/admission, `McpActionAuthority` lock order, the eager/lazy possible-dispatch boundary, envelope validation and cleanup. The corrections are decision text and verification scope and should not reopen the structure. Details: [findings](agent-review-findings.md).
- Process feedback:
  - The gate is justified (schema, ownership, cross-subsystem result contract). The batch is large (adapter, persistence, schema, runtime, legacy adaptation, UI) but cohesive and reviewable as one unit.
  - D1's explicit invariants and failure-mode list were useful. Missing: an outcome table mapping each handler result to failure code/class/safety and the *effective* RecoveryDecision.
  - Candidate instruction (scope: phase 04/06 action providers): "A gate that introduces handler failure classifications tabulates every outcome, including pre-marker authority failures, non-committed observations and crash/lease loss, with the handler safety and the effective decision after `workflow_action_retry_safe`." Evidence: F001/F002. The accepted HTTP precedent (`http_runtime.rs:106-123`, `http_tests.rs:114-190`) never pinned the run outcome either.
  - Second candidate: "When a gated decision changes a shared function, list the suites of its graft callers in the batch gate." Evidence: F003 item 9.
  - No instruction edits made. Graft reported ≥4,805 estimated tokens saved in its unfiltered outputs this round.


## 04.10-R002 — Correction recheck 04.10-D2 — CHANGES REQUIRED
- Identity: 2026-10-17; workflow implement-opus-reviewed-2.1; batch 04.10 MCP, stage correction recheck (architecture gate). Reviewer B: Opus 5.5, xhigh requested; same retained context as R001. Artifact: base `8ce1fd4`, decision 04.10-D2 `logs/04.10/decision-D2.md`, SHA256 `84d532eec3476569a878bca7188ed914ecde7f8ff9f09e4b462679e55330ded5`, verified before review. D1 is unchanged at `3f10553022b5d631876b8174d57d0ea270ff3c7d150b25d5812fd776e285035e`. Author response: [TASK](TASK.md) "04.10-R001 response — D2".
- Correction delta reviewed by finding ID:
  - F001: item 10 table, rows 1–12.
  - F002: row 9 and I8.
  - F003: verification matrix items 1–8 and the batch gate.
  - Adopted optional findings: F005 (I3), F006 (bounded permit wait), F007 (operation revision CAS). Declined: F004.
- Shared recovery-default change reviewed beyond the delta, as asked. D2 introduces `marker_owned_effect_kind` = `mcp.call|http.request|tool.call`, consulted by `recovery::report` and `pending_recovery::unknown_effect`. The evidence it is sound:
  1. The marker is written in the same fenced transaction as the reservation (`action_dispatch.rs` `insert`; local effects also write marker, effect and receipt in one transaction at `action_dispatch.rs:206-213`).
  2. Every retirement path locks the job row through `lock_scope`/`lock_fence` before reading `workflow_action_retry_safe`. A committed marker is therefore always visible, and a later reservation is refused by its fence.
  3. The latest predicate (`20260930180000_workflow_action_evidence.sql:185-205`) returns NULL only when the execution has no marker.
  4. The SQL retirement guards (`check_workflow_fenced_retirement`) only check that labels are consistent; they hold no kind-based default, so there is no Rust/SQL mirror to keep equivalent.
  5. Maintenance deadline/cancel retirement keeps its unconditional 'unknown', but those runs are terminal.
  6. Existing DB tests use `context.load`/`memory.save`/`data.map` sources for unknown-default cases (one `http.request` source exists, in a publication test), so no accepted expectation flips.
  7. The only production handler for `http.request`/`tool.call` is `HttpRuntime`, which dispatches only through ActionService (`adapters/workflow_http/mod.rs:85-87`). D2 changes accepted 04.9 behaviour for those kinds: a crash or lease loss before the marker now retries instead of parking.
- Assessment of the author's HTTP follow-up: it is a real defect, logged as F008 with a dated follow-up appended to the 04.9-R004 note. In short:
  - The 04.9 handler maps every pre-marker refusal and ApprovalRequired to Unknown. That produces a waiting/reconciliation state with no reconciliation fact (`action_uncertainty.rs:15` inserts only unresolved remote markers). Operator retry is refused because the label is 'unknown', so only the run deadline ends it.
  - D2 makes http.request internally inconsistent: a crash before the marker retries, while the same refusal returned by the handler is a permanent wait.
  - Aligning the handler is local and the HTTP suites are already in the gate. Recommendation: align it in this batch.
- Problems introduced by adopted items:
  - F009: F006's bounded wait in the current global-then-endpoint order lets waiters on one hot endpoint hold every global permit.
  - F010: F007's revision restarts at 1 after a revoke or cascade delete, so a stale writer can hit an ABA match.
  - Optional F011: record the handler-composition invariant that keeps the classifier truthful.
- Checks: SHA256 recomputed (match). Inspection only; no tests, no DB/fixture access, no project edits. Unchanged R001 evidence reused for items 1, 3, 4, 5, 7 and 8.
- Outcome: **CHANGES REQUIRED**.
  - F001, F002 and F003 are resolved at decision level. F004's decline is accepted as a documented limitation. F005 is resolved.
  - F006 and F007 were adopted but introduced F009 and F010.
  - New actionable findings: F008 (medium), F009 (low), F010 (low). Optional: F011.
  - The remaining corrections are small decision-text deltas and the structure stays as gated. A recheck should cover only F008–F010 and the HTTP handler alignment, if adopted.
- Process feedback:
  - The outcome table fixed the classification ambiguity cleanly, and the predicate-as-sole-source rule (I8) is a good generalisation.
  - Lesson: adopting optional suggestions still needs a quick self-check for new hazards (lock order, ABA). Candidate instruction (scope: authors in this workflow): "When adopting an optional finding, state the new failure modes it introduces."
  - A recurring theme: run outcomes for pre-marker failures were never pinned by tests in 04.9. R001's candidate instruction (tabulate effective recovery decisions) would have caught F008 in 04.9.


## 04.10-R003 — Correction recheck 04.10-D3 — PASS (architecture gate; F012 carried to implementation)
- Identity: 2026-10-17; workflow implement-opus-reviewed-2.1; batch 04.10 MCP, stage correction recheck (architecture gate). Reviewer B: Opus 5.5, xhigh requested, same retained context as R001/R002.
- Artifact: `logs/04.10/decision-D3.md`, SHA256 `f89b0a89afa11ec9b789ce8e25fc3370a9678bbb43ff1e60c7dcb3e050e7b32b`, verified. It amends D2 (`84d532eec3476569a878bca7188ed914ecde7f8ff9f09e4b462679e55330ded5`, unchanged); D1 is unchanged. Base `8ce1fd4`. The effective gated decision is **D2 as amended by D3**. Author response: [TASK](TASK.md) "04.10-R002 response — D3". Root adopted the HTTP alignment into 04.10.
- Rechecked by finding ID:
  - F010 → D3 A: a random UUID v4 version per write, CAS for register/revoke, create-only `None`. Versions are never reissued across revoke, grant cascade or soft delete, so there is no ABA; competing tests added.
  - F009 → D3 B: endpoint permit first, then global, under one shared ≤ 5 s deadline. Waiters on a busy endpoint hold no global permit; starvation test added.
  - F008 → D3 C/D/E/F: the shared `action_step_failure` mapping is used by both the MCP and HTTP handlers. The predicate is the only source of Unknown; I9 is added; HTTP DB tests assert run/attempt/marker/receipt/fact state.
  - F011 → documentation on the classifier and `WorkflowHandler`, plus a 06/08 obligation.
- Side effect on D2 row 4 checked: a reservation refusal before the marker becomes `mcp.unresolved` Retryable, i.e. RetryAt costing one backoff attempt. On that next attempt the hint is still `prepared`, and the eager `mcp_for_action` check (readiness, grant, operation, actor, live run, credential, run before any network) rejects it as `mcp.rejected` with no session opened. If attempts are already exhausted the result is FinalError. No wait, no fact. Acceptable.
- Regression check of the shared mapping against accepted 04.9 HTTP behaviour:
  - Committed results and `call_tool` are unchanged.
  - Post-marker HTTP failures change class from Terminal to Retryable, but the effective decision is still Reconcile with a fact (predicate false). No SQL guard constrains the class for Reconcile (claim-budget/evidence guards check run state only).
  - Pre-marker failures change from a dead-end wait to FinalError or RetryAt (the intended change).
  - Exactly one accepted assertion flips by design: `http_tests.rs:100` asserts the handler-returned safety `EffectOutcomeUnknown` (optional F013 asks D3 to name its replacement).
  - Not as intended: slotted `http.request` preparation does not check connection readiness (F012).
- Checks: SHA256 recomputed (match). Inspection only; no tests, no DB/fixture access, no project edits. Reused unchanged R001/R002 evidence (classifier soundness, lock order, schema, transport).
- Outcome: **PASS for the architecture gate** on D3 SHA256 `f89b0a89afa11ec9b789ce8e25fc3370a9678bbb43ff1e60c7dcb3e050e7b32b`, with D2 as amended by D3 as the effective decision.
  - F008, F009 and F010 are resolved at decision level; F011 is adopted.
  - **04.10-F012 (low, actionable)** is a local classification detail that is cheap to fix and does not affect schema, ownership, transactions or concurrency. It is carried as a mandatory implementation obligation: it must be fixed and tested before batch acceptance (blocking batch acceptance, not the gate).
  - Optional: 04.10-F013.
- Watch items for the batch review:
  - Closing per-endpoint semaphores held in the Weak map on shutdown, and checking for shutdown before creating new ones.
  - The `action_step_failure` mapping by AppError variant (only NotFound, BadRequest and InvalidCredentials may become `rejected`).
  - The replacement assertion at `http_tests.rs:100`.
- Process feedback: amendment-style revisions (D3 = D2 + named replacements) kept the recheck small and traceable. No new instruction candidate beyond R001/R002.


## 04.10-R004 — Whole-batch code/integration review
- Identity: 2026-10-17; workflow `implement-astra-reviewed-2.1`; batch 04.10, whole-batch implementation and integration review. Independent Reviewer B, requested `gpt-6-astra`/medium, fresh context; no author reasoning inherited and no subagents. Host context/compaction occupancy is not exposed. Records: [TASK](TASK.md), [REVIEW](REVIEW.md).
- Artifact: base `8ce1fd49b5a961da705fddf0b92cac3182da427b`; digest `cb1b1dba9f6a803d7e35308917786c2863e89e73e7abddd4d2f722f2927a6210`; sole exclusion `plan/workflow-foundation/docs`. Independently recomputed before review using the specified snapshot script and task-only macOS realpath shim. Effective gated decisions: D2 amended by D3, preserving their historical text.
- Original requirements reviewed: phase04 action contract, execution/uncertainty and MCP/shared acceptance; EXPANSION row04.10; relevant BRIEF04.1–04.9 obligations (authority, marker-first identity, no metadata-based replay, receipt ownership, recovery, reconciliation and cancellation); stop handoff. Applicable root/src/application/persistence AGENTS and skill references read. Graft map, ask, skeleton and caller discovery preceded source inspection.
- Whole-batch coverage: immutable endpoint migration and tenant/grant foreign keys; operation registration/revocation with company-first lock order, catalog revision and fresh UUID CAS; grant set difference and refresh/output-schema changes; publication, binding and admission routing and exact current operation checks; frozen step/direct-tool request and identity; eager preparation versus lazy evidence-authorized provider entry; credential recheck and rotation; shared result-envelope compiler/runtime schema, redaction, inert links and tool-error receipts; adapter endpoint-first bounded permits, shutdown and tracked session cleanup; HTTP preparation/failure alignment; recovery and pending-recovery defaults. Traced shared result-validation callers into receipt read/write and evidence verification, publication/readiness callers into admission/replay/binding, and catalog writers into management/refresh. Production composition remains a seam for phase06/08, as originally scoped.
- Assertions independently inspected: real publish/bind/admit/worker typed-reference JSON/SSE execution; competing dispatchers and fresh-runtime receipt replay without any MCP request; model-call identity change/refusal; pre-marker refusal/protection/drift/unavailability and post-entry output/transport/session-expiry failures; tool-error receipt and effective terminal/safe retirement; stale/drop cancellation paths; revocation/credential rotation races; catalog competitor/CAS/cascade/tenant tests; HTTP run/attempt/marker/fact checks. Existing transport guards, legacy harness and broad shared action histories were reviewed at changed seams and through focused-suite evidence, not re-audited line by line.
- Gated-decision assessment: D2/D3 behavior is implemented. F001/F002/F008 use one Safe handler mapping with only the durable predicate promoting unknown. F005 current committed hints avoid network. F006/F009 use endpoint-first admission with a shared deadline, global-capacity and shutdown tests. F007/F010 use non-reused UUID versions with competing/stale-writer tests. F011 is documented at both code and phase06/08 seams. F004's accepted missing-structuredContent limitation remains explicit.
- F003 lazy-path concern from the stop handoff is resolved without weakening the planned DB boundary: `mcp_reconciliation_tests.rs:57–155` invokes the real reconciliation service/persistence with a test-host registered verifier, then the real lazy MCP runtime and HTTP fixture. It proves exactly one extra call after final-not-applied and no call on lazy drift. This is not an application-fake substitute and installs no production verifier.
- F012 implementation closure evidence: HTTP `ready_on` (`http_runtime.rs:104–110`) is called by both step and direct preparation; MCP `current_on` is shared by preparation and use. `workflow_http_current_revocation_and_protection_prevent_network` asserts disabled connection and missing bearer secret become `http.rejected`, failed run, terminal/safe, zero marker/fact and exactly one attempt. The focused real-DB log records this test passing. MCP refusal tests cover the corresponding readiness states.
- Verification inspected so far: fresh migrations and schema equivalence PASS (`fresh-migrations.log`, `schema-comparison.log`); production fixture migration/schema gates PASS; focused real-DB suite **504 passed, 0 failed, 0 ignored** (`gate-focused.log`, final result). The additional permit-timeout test postdates that binary and must be covered by the current stock run. Reviewer ran no DB/cargo checks, preserving serialized fixtures. Stock and subsequent SQLx/build/static gates are still running at this checkpoint; their completion must not be inferred from this note.
- Preliminary outcome: **CODE REVIEW PASS**, no new concrete finding or optional suggestion. Final per-batch evidence and post-review digest check will be appended to this same round before return; no final batch acceptance yet.
- Process feedback: batch size was cohesive and reviewable using the split source/test files and gated outcome table. Durable decisions and earlier findings made the intended HTTP behavioral change distinguishable from regression. No new instruction candidate is supported beyond the earlier outcome-table/caller-suite lessons. No project files or instructions edited.

- Verification update, 2026-10-17 17:49 UTC: initial scoped stock-2048-KiB gate **FAILED: 390 passed / 3 failed / 0 ignored**. `gate-stock.log` identifies completion-first persistence-bound timeout and two lease-release commit-outcome-unknown failures in existing reconciliation tests; it reports no stack-budget failure. The new `workflow_mcp_permit_timeout_retires_safely_then_retries_once` passed in this binary. Cause is not established by the log. Root is investigating and rerunning affected checks; subsequent SQLx/build/static gates have not run in this gate invocation. This failure blocks final verification acceptance, and a focused green rerun alone will not replace the required scoped gate.

- Final R004 outcome, 2026-10-17: **CHANGES REQUIRED for verification (04.10-F014, medium)**; code inspection remains PASS with no new code defect found. The focused stock recheck passed3/0, but the second full scoped run failed391/2 on different tests (resolution adoption counts and cancellation transaction-release oracle). Both failure logs are retained; no unsupported "pre-existing" or harmless-load conclusion is made. Subsequent static gates remain outstanding. F012 is fixed; earlier decision corrections are implemented, with F003 executed-gate closure pending F014. Reviewer independently recomputed the snapshot after review: base and digest unchanged at `cb1b1dba9f6a803d7e35308917786c2863e89e73e7abddd4d2f722f2927a6210`, exact exclusion unchanged. Root may unfreeze for investigation/corrections, then request one affected-delta/evidence recheck on the retained reviewer handle. Full task/final phase04 all-target, full-stock and lease-budget gates remain the planned04.11 gate, not coverage claimed here. No04.11 implementation was reviewed.


## 04.10-R005 — F014 correction and evidence recheck
- Identity: 2026-10-17; workflow `implement-astra-reviewed-2.1`; independent Reviewer B (requested gpt-6-astra/medium), same retained role/context. Batch04.10 correction recheck, [TASK](TASK.md), [REVIEW](REVIEW.md). Base `8ce1fd49b5a961da705fddf0b92cac3182da427b`; corrected digest `2be42e3e5d8b0651ad1d6bc523cf356dd6bb55ac254050b99914bede669ebc4c`, sole exclusion `plan/workflow-foundation/docs`, independently matched before review. D2+D3 unchanged. R004 unaffected whole-batch coverage retained.
- Delta inspected: `action_dispatch_resolution_tests.rs:249–272` replaces release with the existing `Gate::drain` before observing rows and removing backend-abort configuration. `lease_resolution_gate.rs:66–150` uses transaction-scoped advisory locking; reacquisition waits for the already blocked transaction's commit/rollback, so an elapsed resolution return is no longer mistaken for durable completion. Result classification, exact durable counts, provider-call assertions and mandatory classified replacement within the existing void ceiling remain unchanged.
- `action_cancellation_order_tests.rs:250–276` now polls a fresh pg_stat_activity snapshot for backend quiescence under the existing10-second hang guard. This is a post-result cleanup observation, not a substitute for ownership or timing proof. Traced every `assert_drained` caller into ordinary receipt/completion/cancellation races and their negative controls. Exact run NOWAIT probes, expected winning results, actual release deadlines and `completed` versus `resolved_applied` persistence-recording assertions remain intact. GUIDANCE DEC002/012/014 reviewed: no production/lease/stack bound, global test parallelism, ordinary-attempt oracle or broad retry policy is changed.
- MCP correction is only two automatic-dereference substitutions at catalog read calls (`mcp.rs:36,198`); no changed authority/SQL behavior.
- Evidence inspected: `synchronization-recheck.log` **12 passed / 0 failed / 0 ignored**, current stock stack; all recorded classified branches reached attempt1. Includes every changed cleanup-helper caller plus late-release receipt and resolved-COMMIT completion negative controls. Source inspection confirms these controls still require ordinary-oracle rejection, not merely eventual business success. The first R004 timeout/commit-ambiguity failures are not retroactively deemed harmless; their logs remain retained.
- Preliminary outcome: affected correction **CODE REVIEW PASS**, no new finding. F014 remains open until the corrected complete scoped/default-parallel stock gate and all remaining static gates pass. Reviewer performed no DB/cargo checks and no project edits. Final evidence and post-review snapshot check will be appended before return.
- Process feedback: this narrow correction preserves the difference between an operation result, transaction drain and ordinary primary success. Existing GUIDANCE already specifies the important distinction; no new instruction candidate is supported.

- Final evidence, 2026-10-17: inspected `corrected-gate-status.log` and underlying outputs. Corrected scoped/default-parallel stock2048KiB gate **393 passed / 0 failed / 0 ignored**,303.74s; all five tests that failed across R004's two runs are present and pass, as is the added MCP permit-timeout test. SQLx prepare and prepare-check, locked offline all-target compilation, strict Clippy, fmt, diff whitespace, transport boundary and regression, credential-key CLI regression and graph build all **PASS**. Initial fresh migration/schema equivalence and focused504/0/0 evidence remains valid for unchanged schema/provider behavior; affected correction12/0/0 evidence is current. No cache/source drift was introduced by checks. Reviewed commands are recorded in `corrected-gates.sh`.
- Final outcome: **PASS for batch04.10** on digest `2be42e3e5d8b0651ad1d6bc523cf356dd6bb55ac254050b99914bede669ebc4c`, independently recomputed after all checks with the same exact exclusion. **F014 resolved**; F012 remains fixed and F003's per-batch execution gate is now satisfied. No new findings or outstanding04.10 acceptance blocker. R004's original failures remain historical evidence; this pass does not assert timing independence or a diagnosed cause for every original transient failure. The corrected synchronization flaws and unchanged real-time oracles are explicitly documented above.
- Limits of this PASS: it accepts04.10's required scoped gate, not the entire user task. Combined full all-target tests, full stock-stack and lease-budget gates remain required after04.11 as originally planned. No04.11 architecture/implementation was reviewed in this round. Reviewer retains handle/context for the next assignment.


## 04.11-R001 — Architecture review interrupted by user STOP; INCOMPLETE
- Identity: 2026-10-17; workflow `implement-astra-reviewed-2.1`; independent Reviewer B, requested gpt-6-astra/medium; retained reviewer context. Actual host metadata/context occupancy unavailable. No subagents.
- Artifact: `logs/04.11/decision-D1.md`, SHA256 `ebb523dfdfcf5d7b937ddf48b47161bb79b70ad9bc2d890b511f63ef58b8ad83`, independently matched at review start and stop checkpoint. No04.11 implementation exists. Accepted04.10 remains R005/base8ce1fd49b5a961da705fddf0b92cac3182da427b/digest2be42e3e5d8b0651ad1d6bc523cf356dd6bb55ac254050b99914bede669ebc4c.
- Status: **INCOMPLETE, NO PASS, NO IMPLEMENTATION AUTHORIZATION**. User requested safe checkpoint and STOP before grouped review delivery. No further investigation performed after stop instruction. Findings below are provisional observations for the resumed architecture review, not a completed gate verdict.
- Requirements inspected: phase04 and EXPANSION04.11, accepted04.1–04.10 obligations, explicit users/channels only (groups rejected), exact-source reply, canonical-message/delivery/action atomicity, provider acceptance semantics, accepted siblings never resent, shared direct/step service.
- Discovery/inspection completed: graft map/ask/skeleton/callers before targeted source reads; delivery queue begin/complete/aggregate and cancellation callers; composer and deterministic email Message-ID; workflow wait/commit/completion/polling/worker/supervisor seams; deferred dispatch and retirement guards; shared failure/recovery/pending-recovery and retry-safe/effect-state SQL; registry content schema. No DB, cargo or implementation edits.
- Main open observations: F001 new delivery-kind/message recovery outcomes and owner integration need explicit coverage; F002 D1 assumes an existing action admission bound that author reports does not exist; F003 subject schema MAX_TEXT exceeds DB2048-byte limit; F004 optional cursor/producer-race invariant clarification. See findings for evidence and limits.
- Cursor concern is NOT an established lost-event defect: locking every required delivery parent and aggregating authoritative state before cursor advancement can make lower-sequence late commits safe. Resume reviewer should verify that exact invariant, not demand a replacement design by assumption.
- Author independently identified F002/F003 and proposed bounded16 action-consumption pages plus a bounded129 probe refusing new delivery marker admission at128 existing dispatches; this proposal is **not yet reviewed or adopted in D1**. Do not treat it as gated architecture.
- Remaining work on resume: finish action-count/recovery contract inspection as necessary, assess grouped findings, obtain author decision revision if needed, rehash artifact and return explicit architecture gate verdict. Keep R001 incomplete history; append disposition rather than overwrite. No new instruction candidate yet.

## 04.11-R002 — Architecture gate 04.11-D1 completed — CHANGES REQUIRED
- Identity: 2026-10-17; workflow `implement-opus-reviewed-2.1`; batch 04.11 messaging, stage architecture gate (pre-implementation), completing interrupted [04.11-R001](#0411-r001--architecture-review-interrupted-by-user-stop-incomplete). Reviewer B: Opus 5.5 (`claude-opus-5-5`), xhigh requested per [TASK](TASK.md) §"04.11 continuation"; effective effort and context occupancy are not visible to the reviewer. Fresh independent context (no author transcript; R001 was written by a different reviewer). Records: [TASK](TASK.md) §§"04.10 + 04.11", "04.11-D1 architecture gate proposed", "04.11 continuation"; [REVIEW](REVIEW.md) §04.11.
- Artifact: base `8ce1fd49b5a961da705fddf0b92cac3182da427b`; decision `logs/04.11/decision-D1.md`, SHA256 `ebb523dfdfcf5d7b937ddf48b47161bb79b70ad9bc2d890b511f63ef58b8ad83`, recomputed at review start (match). Supporting `logs/04.11/investigation.md` read as context only, not as proof. Gated decision ID 04.11-D1. No 04.11 implementation exists; no code digest.
- Requirements derived from originals: phase 04 "Action contract", "Execution and uncertainty" items 1–7, "Provider-neutral messaging", "Acceptance"; EXPANSION row 04.11 and cross-point preservation (04.11 atomically creates message/delivery/action/wait; aggregates each destination without sibling resend; combined phase04 gate); BRIEF-04.10 (F011 marker-owned invariant, shared failure classification); user decision users/channels only, groups rejected; AGENTS.md (no backward compatibility, CI parity, focused-then-full verification, raised-limit rule), `src/AGENTS.md` (bounds at boundaries), `src/application/AGENTS.md` (one logical commit, poison-batch tests, no default port methods), `src/adapters/persistence/AGENTS.md` (DB-boundary invariants, tenant FKs, guards preserved).
- Coverage (inspection only; every D1 section 1–6 reviewed):
  - §1 routing/authority: registry `actions.rs:7-16`, `schemas.rs:39-58` (group enum, `content()`, `message_result()`); `registry/mod.rs:34-36`; `CanonicalContent::parse` `transport/ingress.rs:36-43,109-118`; `messages` table `init_schema.sql:2767-2789`; `authorize_company` and `supported_publication` `workflow/authority.rs:9-80`; `action_authority.rs:35-120`.
  - §2 atomic publication: `ActionService` `actions/service.rs`, intents `persistence/workflow/actions.rs:16-120` (no admission bound), `authorize_prelocked_on` `action_dispatch.rs:74-146` (lock order), dispatch guard `20260930120000_workflow_action_dispatch.sql:56-88`; `DeliveryComposer::compose/interface` `transport/compose.rs:131-243`; `EmailRenderer::message_id_for/part_key` `email/egress.rs:207-345`; `insert_message_on`/`store_email_correlated` `thread/message.rs:435-682`; `external_messages`/`email_message_metadata` schema; `create_message_with_deliveries` `thread/mod.rs:610-635`; outreach precedent `services/outreach_tool.rs:370-446`.
  - §3 suspension: `supervise_io` `supervise.rs:23-101`, `WorkflowWorker::process/execute_io` `worker.rs:98-168`, `park_on` `waits.rs:145-210`, `resume_on/advance/expire` `wait_commit.rs:5-132`, waits schema `20260928070000`, `20260928144000`.
  - §4 receipts/progression: `CANDIDATES_SQL` `polling.rs:82-112`; `workflow_action_retry_safe` `20260930180000:185-206`; `workflow_action_effect_states` `20260930182000:102-118`; `action_uncertainty::record`; receipts/uncertainty/evidence effect_kind constraints across 20260930* migrations.
  - §5 cancellation/permission/callbacks: `begin_part`/`complete_part`/`aggregate_parent_on` `delivery/queue.rs:164-457` (confirmed `complete_part` has no `part.status='sending'` predicate at 229-240), reaper `queue.rs:566-600`, `cancellation.rs:68-227`; `remove_member` `company_invite.rs:349-384`.
  - Recovery: `recovery.rs:21-137` (`report()` default, retry-safe override), `pending_recovery.rs:46-145`, `failure.rs:1-20` (`marker_owned_effect_kind`).
  - Existing thread consumers: `find_outbound_reply_after` `thread/mod.rs:637-668` with callers `services/task_worker.rs:990-1010`, `use_cases/thread/dispatch.rs:416-420`; RFC fallback `use_cases/thread/mod.rs:1025-1041` → `views.rs:587-608`.
  - §6 verification plan against CI `.github/workflows/ci.yml:51-117` and `scripts/` (stack-budget, lease-budget, test-network-isolation, transport-boundary-check).
- Assumptions challenged: (a) "existing action-count bounds" — none at preparation (F002); (b) one canonical message with several email intents fits existing threading — it does not (F005); (c) new delivery marker fits shared recovery predicates unchanged — it does not (F001); (d) cursor high-water safety — safe if parent-lock invariant is explicit (F004, optional); (e) commit-time proof for an atomically closed job — capture provenance unspecified (F007); (f) effect waits fit existing resume/poll — only if deadline equals run deadline (F008).
- Sound as proposed (no finding): group removal/runtime rejection; never a default channel; refusal without run channel for email; exact-source reply and private-content non-promotion intent; trusted built-in ToolSnapshots instead of HTTP routing (`supported_publication` currently routes slotless tools to HTTP); reuse of `authorize_on` lock order (company/actor → fence → run → current owners → intent) then channels/threads → deliveries; no network I/O inside transactions; Message-ID stability from invocation-derived idempotency keys (`message_id_for` is a pure function of `email:<key>`); explicit suspended handler outcome instead of error/lease-loss; direct-tool Pending seam consistent with HTTP/MCP 06/08 boundary; run-before-parent lock order for cancellation and workflow begin_part with parent-only callbacks; the `complete_part` overwrite gap is real and correctly targeted; "No raised resource limits".
- Checks: none run (assignment forbids cargo builds and fixture start/stop). No tested behavior is claimed; all conclusions are code/schema inspection. Graft queries/skeleton/grep/callers and targeted reads only.
- Lightly reviewed / not covered: `fair_polling.rs` internals, workflow controls (cancel/pause/retry) code paths, `maintenance::expire_on`, `completion_job::finish` owner variants, the delivery worker service loop, and channel ACL effects on internal-user replies. These bear on F001/F008 implementation details; D2 should name the affected paths.
- Outcome: **CHANGES REQUIRED — no PASS for `ebb523df…ad83`.** Actionable: [04.11-F001](agent-review-findings.md#0411-f001--r002-disposition-confirmed-revised-medium) (medium), [F002](agent-review-findings.md#0411-f002--r002-disposition-confirmed-medium) (medium), [F003](agent-review-findings.md#0411-f003--r002-disposition-confirmed-revised-low) (low), [F005](agent-review-findings.md#0411-f005--canonical-message-classification-and-per-recipient-provider-identity-are-unspecified) (medium), [F006](agent-review-findings.md#0411-f006--verification-plan-omits-ci-gates-and-the-tests-f001f005-require) (low), [F007](agent-review-findings.md#0411-f007--commit-time-and-receipt-proofs-are-not-database-derived) (low), [F008](agent-review-findings.md#0411-f008--effect-wait-deadline-and-resume-semantics-unspecified) (low). Optional: [F004](agent-review-findings.md#0411-f004--r002-disposition-confirmed-optional) (low). Provisional F001–F004 from R001: F001 confirmed and expanded; F002 confirmed; F003 confirmed with the binding bound corrected to 998 bytes and destination count added; F004 confirmed as optional (no lost-event defect). None withdrawn.
- On unreviewed author proposals (16-action consumption pages; 129-row probe refusing new delivery-marker admission at 128 dispatches): not adopted in D1 and not accepted here. F002 requires a concrete enforced bound; either shape can satisfy it if refusal precedes any marker/publication, is a distinct terminal (not unknown) outcome, keeps the per-execution count at or below the existing 128-dispatch proof ceiling of `workflow_action_retry_safe`, and is tested at the boundary. Review on D2.
- Process and instruction feedback: batch size fine for one gate; the gate was warranted (schema, recovery, transaction and concurrency contracts). D1 is dense and terse; it omits two decisions that 04.10-F001 already showed are costly: the effective recovery outcome of a new effect kind across shared predicates (recurrence of 04.10-F001), and how new canonical messages are classified for existing thread consumers. Candidate instructions (scope: architecture gates in this plan): (1) "A decision introducing a new effect kind tabulates its outcome in every shared predicate — `marker_owned_effect_kind`, `workflow_action_retry_safe`, `workflow_action_effect_states`, `action_uncertainty::record`, reconciliation commands — for pre-marker, post-marker and terminal cases" (evidence F001, 04.10-F001); (2) "A decision that writes canonical messages states author/direction/role/audience/entry kind, provider-key mapping per delivery, and lists existing thread consumers it affects" (evidence F005). Tooling note: a caveman output-compression hook elided code bodies in Bash/Read output during this review; exact text was recovered via `caveman_retrieve` before relying on it.
- Follow-up 2026-10-17 ([04.11-R003](#0411-r003--correction-recheck-of-0411-d2--changes-required)): F001's required correction above offered "or justify a documented deferral against phase 04 items 5–6". R003 judged the deferral D2 then made, and found that the accepted 04.7/04.10 reading of item 6 does not support it: MCP met item 6 with a test-host verifier through the real reconciliation service, and no production verifier was required. That remedy option was therefore too permissive, and the remaining gap is now [04.11-F010](agent-review-findings.md#0411-f010--message-deliveries-with-unknown-outcome-have-no-authorized-reconciliation-path). The original R002 outcome stands.

## 04.11-R003 — Correction recheck of 04.11-D2 — CHANGES REQUIRED
- Identity: 2026-10-17; workflow `implement-opus-reviewed-2.1`; batch 04.11 messaging, stage architecture-gate correction recheck. Reviewer B: Opus 5.5 (`claude-opus-5-5`), xhigh requested; effective effort not visible. Same reviewer and context as [04.11-R002](#0411-r002--architecture-gate-0411-d1-completed--changes-required). Records: [TASK](TASK.md) §"04.11-D2 correction" (author status and root's note that it does not accept the §5.3 deferral); [REVIEW](REVIEW.md) §04.11.
- Artifact: `logs/04.11/decision-D2.md`, SHA256 `1dcd1a9e99e57cd32e15bf095490961722f185153c66945cd82d29bd4d943a44`, recomputed at start (match). Base `8ce1fd49b5a961da705fddf0b92cac3182da427b`. D2 supersedes D1 (`ebb523df…ad83`, retained). Gated decision ID 04.11-D2. No implementation exists.
- Scope: recheck by finding ID (F001–F008), broadened to every contract D2 changed:
  - one `message` effect kind for every message invocation;
  - one canonical message per destination; new tables and triggers;
  - `EffectParked` owner and `confirm_suspension`;
  - consumer discovery and poison handling;
  - the reaper lock-order change;
  - the `scripts/lease-budget.sh` change;
  - root's requested judgement on the §5.3 deferral.
- New evidence inspected this round:
  - control retry reopens the same execution's failed job only: `control_retry.rs:3-42`, so the same invocation is replayed;
  - recorder callers: `controls.rs:181`, `maintenance.rs:41`, `pending_recovery.rs:92`, `recovery.rs:86`;
  - reconciliation table, CHECK and run-state guard: `20260930170000:2-35`;
  - receipt triggers, none fence-bound: `20260930180000:229-256`;
  - `workflow_action_replay_supported` / `not_applied_available` remote-only first checks: `20260930150000:51`, `20261001090000:8`;
  - `waiting_reason` allows `effect`: `20260928133000:8`;
  - `message_deliveries_message_audience_check`: `init_schema.sql:2706`;
  - `thread_messages` entry kinds: `init_schema.sql:3577`;
  - `sanitize_string` strip-only: `domain/entities/unicode_sanitization.rs:128-156`;
  - email renders exactly one part, whose body digest is "what a reconciliation lookup can compare": `email/egress.rs:386-395`;
  - every `UPDATE message_deliveries` producer, with no dead_letter→pending redrive: `queue.rs`, `cancellation.rs`, `approval.rs:469`, `task/operations.rs:190,310`, `task/controls.rs:199`; `orphan_stuck_dependents_on` (`queue.rs:656-683`) applies only to dependent deliveries, and workflow deliveries have none;
  - `controls::control` lock order is `authorize_company` then run `FOR UPDATE` (`controls.rs:64-74`), consistent with D2's workflow `begin_part` order;
  - CI steps `ci.yml:47-117`;
  - `scripts/stack-budget.sh:30` passes arguments through;
  - `scripts/lease-budget.sh` in full;
  - BRIEF-04.7 (verifier port, no production verifier) and BRIEF-04.10:60 (test-host verifier through the real reconciliation service).
- Findings rechecked (details and dispositions are in the findings log):
  - F001: resolved at decision level, apart from the evidence-path deferral, which is split into F010.
  - F002, F003, F005, F006, F007, F008: resolved at decision level.
  - F004 (optional): adopted and resolved, but the "no exit path" premise must be revisited under F010.
- New findings: [04.11-F009](agent-review-findings.md#0411-f009--sql-backstops-admission-count-race-and-step-close-identity-optional) (optional, low), [04.11-F010](agent-review-findings.md#0411-f010--message-deliveries-with-unknown-outcome-have-no-authorized-reconciliation-path) (medium, actionable), [04.11-F011](agent-review-findings.md#0411-f011--lease-budget-measurement-of-new-message-operations-undefined) (low, actionable).
- §5.3 judgement for root: **(b) fails the criteria.**
  - Phase 04 item 5 is met.
  - Item 6 is not. It requires authorized commands to record applied or not-applied, and only proven not-applied permits retry. The Acceptance line requires tests covering reconciliation, and EXPANSION has 04.11 run the combined matrix "including all provider criteria".
  - The accepted 04.7/04.9/04.10 reading of item 6 needs no production verifier. The command accepts only attestations from a host-installed `ActionEvidenceVerifier`, and MCP (BRIEF-04.10:60) proved not-applied retry through the real service with a test-host verifier.
  - D2 instead refuses message invocations outright (`reconciliation.unsupported_effect`). An `outcome_unknown` delivery therefore cannot be settled even with proof. A proven not-applied delivery can never be retried, which is the "retry only unresolved safely retryable work" case, and the run can only fail at its deadline.
  - D2's rationale (no email ledger or verifier) applies equally to MCP, which still integrated the path.
  - It would only become (c) if the user explicitly reduced scope. The exact choice: "accept that in 04.11 an unconfirmed email delivery can never be settled by evidence and is held until the run deadline, with the verifier-attested per-delivery path deferred to a named later point" versus "require that path in 04.11, exercised with a test-host verifier and with no production verifier claimed". Root must not and does not decide this.
- Reaper change: sound.
  - Locking stranded parents `ORDER BY id FOR UPDATE SKIP LOCKED` before the part update matches the parent-before-part order of `begin_part`, `complete_part` and cancellation.
  - It keeps the expiry predicate on the locked rows. Rows the consumer holds `FOR SHARE` or a live writer holds are reaped on a later sweep, never blocked.
  - The reaper locks no run or action rows, so it adds no cycle.
  - It keeps the pre-existing unbounded stranded set; this is not widened, and is recorded only as an observation.
  - D2 §8.2 adds a reaper-versus-consumer no-deadlock test. The existing delivery and cancellation suites must remain green.
- Lease-budget change: sound in direction. No threshold is raised (`BUDGET_PCT` stays 50), and the required-op check makes the new operations fail loudly if a filter drifts. It is incomplete, though: the gate compares work against each event's `attempt_bound_ms`, and D2 defines neither the consumer transaction's client bound nor the leading-lock boundary for `message.publish`/`message.consume`. See F011.
- Sound as revised (no finding):
  - outcome table rows 1–14, cross-checked against `recovery.rs`, `pending_recovery.rs`, `retry_safe`, `control_retry`;
  - the marker-owned invariant (marker is the first write; provider I/O only after commit);
  - admission 128 with a pre-write refusal;
  - consumption one action per transaction, at most 20 parents, `LIMIT 41`, poison backoff to `stalled`;
  - event production by parent-status trigger with consumer locks taken before the read (F004 invariant holds);
  - one canonical message per destination, matching the `email_message_metadata` PK and the outreach precedent;
  - the classification table;
  - `message.send` never writing into existing threads; the external reply counting as the answer (explicit and intended);
  - DB-captured `fence_expires_at` with ordering enforced;
  - a receipt guard recomputing the result from terminal delivery states (stable, since no exits);
  - effect-wait deadline equal to the run deadline, plus an explicit `resume_on` branch and receipt-gated discovery (no hot re-poll);
  - CI-parity final gate and disk plan.
- Checks: none run (read-only assignment). Every conclusion comes from inspecting the code and schema.
- Lightly reviewed: the exact `completion_job` owner interplay (`EffectParked` at park versus `Parked` at resume), the claim/episode/budget SQL for message rows in `workflow_action_reconciliations`, `fair_polling` internals, and `inspection`. These are deferred to the batch review; D2 names them.
- Outcome: **CHANGES REQUIRED — no PASS for `1dcd1a9e…3a44`.** Blocking: F010 (medium) and F011 (low). Optional: F009. If F010 is fixed by adding the path, recheck the F004 "no exit path" premise, the event bound (`LIMIT 41` already allows two events per delivery), the state guard (`uncertain` → `pending`/`accepted`/`failed`) and the §5.1 rows 11–13.
- Process feedback: D2 is thorough and finding-indexed, and §0 made the recheck efficient. The miss in R002 is mine: F001's remedy allowed "or justify a documented deferral against phase 04 items 5–6" without first checking the accepted 04.7/04.10 reading. That reading shows item 6 is satisfied per provider kind with a test-host verifier. Candidate instruction (scope: reviewers in this plan): "When a remedy offers a deferral against an original criterion, check the accepted precedent for how earlier points met that criterion before offering it." Evidence: R002 F001 → R003 F010.

## 04.11-R004 — Correction recheck of 04.11-D3 (delta over D2) — CHANGES REQUIRED
- Identity: 2026-10-17; workflow `implement-opus-reviewed-2.1`; batch 04.11 messaging; architecture-gate correction recheck. Reviewer B: Opus 5.5 (`claude-opus-5-5`), xhigh requested; effective effort not visible. Same reviewer and context as R002/R003. Records: [TASK](TASK.md) §"04.11-D3 correction"; [REVIEW](REVIEW.md) §04.11.
- Artifact: the effective decision is D2 `logs/04.11/decision-D2.md` SHA256 `1dcd1a9e99e57cd32e15bf095490961722f185153c66945cd82d29bd4d943a44` plus D3 `logs/04.11/decision-D3.md` SHA256 `c462e4eb56cb76ac816fa107b7b01f7651706796b8723fed8f206c6315858a89`. Both recomputed at start (match). Base `8ce1fd49b5a961da705fddf0b92cac3182da427b`. No implementation exists.
- Scope:
  - F009, F010 and F011 by ID.
  - The premises F010 changes: the F004 assumption that "terminal deliveries never change", §5.1 rows 11–13 and the new rows 15–21, and the `ReconciliationTarget::MessageDelivery` integration with the 04.7 service and the accepted 04.10 MCP path.
  - The §L deferral (phase 06 reopens `model_tool`-origin agent executions).
  - Broadened to the 04.7 schema D3 extends.
- New evidence inspected this round:
  - `workflow_action_evidence` columns, CHECKs and the one-final-coverage unique index: `20260930180000:5-37`;
  - the evidence-conflict table: `20260930180000:89-103`, `20260930182000:18-30`;
  - the conflict predicate: `20260930180000:129-133`;
  - `workflow_action_receipt_finality_conflict`, with the comment "a finality breach … permanently vetoes automatic continuation even if a usable receipt now exists": `20260930180000:240-256`, redefined `20260930181000:83-108`;
  - `workflow_action_conflict_park_pending`: `20260930181000:109-126`;
  - the latest `workflow_action_evidence_commit_guard`: `20260930181000:127-151`;
  - `applied_request` attribution and its guard: `20260930181000:1-22`;
  - the continuation veto in `complete_io`: `completion.rs:106-125`, and its absence from `wait_commit.rs`, `batch_commit.rs` and `completion_job.rs`;
  - 04.7 `continue_on`, which reopens a reconciliation-parked job: `action_reconciliation/settlement.rs:154-202`;
  - `workflow_action_reconciliation_reopen_safe`: `20261001100000:281-306`;
  - `settle_cancelled_on` and `settle_stranded_cancellations_on`, which never move an `outcome_unknown` parent: `cancellation.rs:234-343`;
  - `LeasePolicy::attempt_bound/resolution_bound`: `lease.rs:48-54`;
  - the recorder's `locks_held` and `Injected`: `persistence_bound.rs:510-536`;
  - `workflow_action_effect_states` users: only `mcp.rs:342` and tests;
  - `supported_publication` refusing agent snapshots: `authority.rs:53-60`.
- Findings rechecked (details in the findings log):
  - F009: resolved.
  - F010: resolved at decision level. Remaining integration gaps are logged as F012 and F013.
  - F011: resolved.
  - F004 premise: restated correctly.
    - `delivered` and `dead_letter` are final and trigger-enforced.
    - `outcome_unknown` exits only through evidence; the new re-entry only through §I.
    - No existing path moves an `outcome_unknown` parent: the fenced settle requires a live lease, the stranded settle needs `status='sending'`, and the reaper marks unknown before settling.
    - The event bound is driven by commands; the consumer reads only `max(seq)`.
  - Rows 11–13 and 15–19, 21: consistent with the code.
  - Row 20: see F012. Row 16's effect-state column: see F013.
  - MCP/04.7 path:
    - remote branches are unchanged;
    - `workflow_action_not_applied_available` and `retry_safe` stay remote-only, and message proofs are consumed only by reopen rows;
    - message receipts have no `remote_entry_id`/`reconciliation_evidence_id`, so the remote finality-conflict trigger is not falsely triggered by a successful reopen and resend;
    - the request digest gains a versioned `target` for both kinds. That changes accepted 04.7/04.10 command bytes, which is acceptable under no-BC with a reset, and D3 §K reruns the `action_reconciliation` and `mcp_reconciliation` filters.
- New findings, both actionable:
  - [04.11-F012](agent-review-findings.md#0411-f012--effect-wait-resume-bypasses-the-finality-breach-continuation-veto) (low): the effect-wait resume bypasses the finality-breach continuation veto.
  - [04.11-F013](agent-review-findings.md#0411-f013--message-evidence-shape-and-projection-gaps-against-the-047-schema) (low): message evidence shape and projection gaps against the 04.7 schema.
- §L deferral: **acceptable against the phase 04 criteria**, with conditions.
  - Phase 04 items 4–6 hold for `model_tool` origin:
    - the receipt is recorded before any tool output;
    - the agent execution parks conservatively when interrupted;
    - its deliveries are reconcilable through the same command;
    - only proven not-applied resends, and only that delivery.
  - Acceptance "identical grants, approval, and idempotency" holds; one service serves both.
  - Resuming an agent execution is the phase 06 saved-call owner's job (phase 04 "Action contract" and EXPANSION 06). It is unreachable in phase 04, because publication refuses agent snapshots (`authority.rs:53-60`). That matches the accepted 04.10 boundary ("production worker/tool exposure remains a phase06/08 obligation").
  - Divergence to record: the 04.7 `continue_on` does reopen a reconciliation-parked owner for remote invocations, while D3's message settlement never does.
  - Conditions:
    1. BRIEF-04.11 records the obligation, with its trigger: reopen once every message marker of a `model_tool` execution has a receipt and `workflow_action_reconciliation_reopen_safe` holds.
    2. The implementation keeps message outcomes out of the shared `continue_on` (D3 §H.4), so `NotAppliedRecorded` for a delivery cannot reopen the agent job.
    3. A port-seam test shows the parked agent execution stays parked, its receipts are recorded, and `retry_safe` becomes TRUE once every marker is receipted.
- Checks: none run (read-only). Conclusions come from inspecting the code and schema.
- Lightly reviewed: the exact Rust shapes of `ReconciliationSnapshot`/`SubjectFacts` against `actions/reconciliation/{contracts,service,verification}.rs`. I relied on D3's statement that the remote fields are unchanged, and the batch review must verify it. The claim/episode/budget SQL for message rows also gets only a light pass; D3 keeps it remote-only.
- Outcome: **CHANGES REQUIRED — no PASS for D2 `1dcd1a9e…3a44` + D3 `c462e4eb…8a89`.** F012 and F013 are both low and should be cheap to fix in one delta. No other open actionable findings.
- Process feedback: the delta format (D3's header table) made a focused recheck practical. The remaining gaps come from extending an accepted subsystem (04.7) whose invariants live in migration comments and scattered guards. Candidate instruction (scope: decisions that extend accepted action machinery): "List the accepted invariants of the extended subsystem — guards, triggers, vetoes — and state each one's behavior for the new kind." Evidence: F012 (continuation veto) and F013 (applied_request guard, conflict orientation, projection precedence).

## 04.11-R005 — Correction recheck of 04.11-D4 (delta over D2+D3) — PASS
- Identity: 2026-10-17; workflow `implement-opus-reviewed-2.1`; batch 04.11 messaging; architecture-gate correction recheck. Reviewer B: Opus 5.5 (`claude-opus-5-5`), xhigh requested; effective effort not visible. Same reviewer and context as R002–R004. Records: [TASK](TASK.md) §"04.11-D4 correction"; [REVIEW](REVIEW.md) §04.11.
- Artifact: the effective decision is
  - D2 `logs/04.11/decision-D2.md` SHA256 `1dcd1a9e99e57cd32e15bf095490961722f185153c66945cd82d29bd4d943a44`;
  - plus D3 `logs/04.11/decision-D3.md` SHA256 `c462e4eb56cb76ac816fa107b7b01f7651706796b8723fed8f206c6315858a89`;
  - plus D4 `logs/04.11/decision-D4.md` SHA256 `6bf5efcb3de54949df516b45725c8c9d4fe8cc1ff968a38671a367862e1fa276`.

  All three were recomputed at start and at end, and all matched. Base `8ce1fd49b5a961da705fddf0b92cac3182da427b`. No implementation exists.
- Scope: F012, F013 items 1–4 and the three R004 §L conditions, by ID. Broadened only to the contracts D4 changes: the `applied_request='delivery'` value and its shape CHECK, and the reopen outcome renamed to `DeliveryReopened` with no job scheduled.
- Evidence inspected this round:
  - **Run settlement:** `recovery::settle_run` is an unconditional run update, so it can move `waiting/effect` to `waiting/reconciliation` (`recovery.rs:156-172`). `finish_job` writes `failed` for a Reconcile decision (`recovery.rs:174-192`).
  - **Wait CHECK:** the `workflow_wait_failure` CHECK currently allows only `activation_limit`/`invalid_output` (`20260928144500:5-6`). D4 extends it.
  - **Run lock:** `waits::lock` takes the run `FOR UPDATE` (`waits.rs:95`).
  - **Conflict veto:** `retry_safe` is false under a conflict (`20260930180000:189`). That makes both `workflow_action_reconciliation_reopen_safe` and `workflow_control_retry_safe` false (`20261001100000:281-330`), so neither `continue_on` nor control retry can reopen a §M park.
  - **Command table:** the outcome is free jsonb; its CHECK allows `scheduled_job_id` only with kind `scheduled` from `waiting/reconciliation` (`20260930180000:50-73`).
  - **Claim-episode binding:** `workflow_action_bind_schedule` raises for any `scheduled` outcome that lacks a current schedule witness (`20261001100000:69-80`). D3's `Scheduled{receipt_only:false}` reopen outcome would have hit this, so D4's `DeliveryReopened` with a NULL `scheduled_job_id` is required, not just a rename.
  - **Conflict-table anchors:** `20260930180000:89-103` and `20260930182000:18-30`, checked against D4 §Q's orientation.
- Dispositions (details in the findings log):
  - **F012:** resolved at decision level. D4 §M checks the conflict with the same `conflicted_on` under the run lock, before reading the receipt. On a conflict it applies the Reconcile park that `complete_io` would (wait `failed/evidence_conflict`, job `failed`, run `waiting/reconciliation`, uncertainty row, audit) and supersedes unsent sibling deliveries. Discovery wakes on a conflict without hot re-polling, because a handled wait is no longer `waiting`.
  - **F013.1:** resolved. `'delivery'` is added to the `applied_request` CHECK and the shape CHECK, and the commit guard enforces message applied ⇔ `'delivery'`.
  - **F013.2:** resolved. The SQL reopen guard checks for a conflict after taking the run lock.
  - **F013.3:** resolved. The message branch follows the receipt branch, ahead of the generic evidence branches.
  - **F013.4:** resolved. Existing orientation; the CHECKs are satisfied.
  - **§L conditions 1–3:** satisfied at decision level.
    - BRIEF-04.11 must state the phase 06 obligation, the reopen trigger and the divergence from 04.7.
    - Every message outcome returns before `continue_on`.
    - The port-seam test asserts the parked agent execution stays parked, its receipts are recorded and `retry_safe` becomes TRUE.
- New optional suggestion: [04.11-F014](agent-review-findings.md#0411-f014--clarify-the-null-shape-alternative-and-conflict-refusal-for-later-message-entries-optional) (low): clarify the NULL shape alternative, and add an execution-conflict refusal at publication and `begin_part`. Not blocking.
- Checks: none run (read-only). The verdict rests on inspection plus R002–R004 evidence that this delta does not affect. That evidence remains valid because D2/D3 sections not amended by D4 are unchanged (hashes verified).
- Outcome: **PASS for the effective decision D2 `1dcd1a9e…3a44` + D3 `c462e4eb…8a89` + D4 `6bf5efcb…a276`** at the architecture gate. No open actionable findings for 04.11 at decision level. Optional F004 and F009 are adopted and F014 is open as an optional suggestion.
- This PASS covers the gated decisions only. It does not cover implementation, tests, the combined phase 04 gate or the whole-batch review; all of those remain required before acceptance. The lightly reviewed areas from R003/R004 are carried into the batch review:
  - `completion_job` owner interplay;
  - the Rust shapes of the reconciliation contracts;
  - the claim/episode/budget SQL for message rows;
  - `fair_polling` and `inspection`.
- Process feedback: the delta chain D2 → D3 → D4 kept each recheck narrow and verifiable. The D3→D4 round surfaced one latent defect that a check of the accepted subsystem's triggers caught: the `scheduled` outcome CHECK and the claim-episode binding. This reinforces the R004 candidate instruction (state the accepted invariants of an extended subsystem for the new kind). No new candidate.

## 04.11-R006 — Whole-batch code, criteria and final phase-04 integration review — CHANGES REQUIRED
- Identity: 2026-10-18; workflow `implement-opus-reviewed-2.1`; batch 04.11 messaging plus the combined phase-04 gate; stage whole-batch review and final integration review. Reviewer B: Opus 5.5 (`claude-opus-5-5`), xhigh requested; effective effort not visible. Same reviewer and context as R002–R005. Records: [TASK](TASK.md) §§"04.11 implementation", "04.11 final gate", "04.11 frozen"; [REVIEW](REVIEW.md) §04.11.
- Artifact:
  - Base `5c1de32ecb3c1a7e59e1a60deb74ab5c42e755df`; digest `8891d74001e78bac6b65e0238c3d73ac25b92f3970178edb08f296417c75cabd`, recomputed with the skill script and the task `realpath` shim (match). Exclusion `plan/workflow-foundation/docs`.
  - Effective decision: D2 `1dcd1a9e…3a44` + D3 `c462e4eb…8a89` + D4 `6bf5efcb…a276`.
  - Scope separation:
    - `git diff 8ce1fd4 5c1de32` has 79 files. 26 of them are 04.11-only, and 53 match the 04.10 manifest.
    - I could not reproduce the 04.10 digest from the commit (8fb7…/f9bc… vs 2be42e3e). That is consistent with the post-acceptance administrative edits already recorded for RESUME/plan files.
    - A grep shows the 04.10-manifest files in the commit also carry 04.11 hunks: `failure.rs` (message kinds marker-owned), registry `actions.rs`/`mod.rs`, `worker.rs` (`PollWork::MessageReceipt`), and `recovery.rs`. So "commit = 04.10 + 26 files" is not exact.
    - I reviewed the message-related hunks of those shared files as 04.11 scope, plus the 26 commit files and the 51-file working-tree delta. Other 04.10 content was taken as accepted context.
- Requirements: phase 04 "Action contract", "Execution and uncertainty" 1–7, "Provider-neutral messaging" and "Acceptance"; EXPANSION 04.11 and cross-point preservation; users/channels only; the accepted D2+D3+D4; AGENTS.md (CI parity, competing-claimant tests, raised-limit rule) and `src/**/AGENTS.md`.
- Coverage (code read, with line-level checks):
  - **Migration:** `migrations/20261017120000_workflow_messaging.sql` in full (640 lines): vocabulary, the four tables, evidence subject and shape, admission/capture/aggregate/state/receipt/effect-wait/dispatch/fenced-completion/provenance/event/reopen/evidence/reconciliation guards, and the effect-state view. Compared against the prior definitions of `check_workflow_fenced_completion` (`20260928141500`) and `workflow_action_evidence_commit_guard` (`20260930181000:127-151`).
  - **Publication:** `message_publish.rs` in full (lock order, conflict refusal, replay, route recheck, admission probe, marker first, park, ambiguous resolution).
  - **Application runtime:** `message_runtime.rs` in full (plan, compose, classification table, handler outcome mapping) and parts of persistence `message.rs` (preparation, routes, member lock).
  - **Receipts:** `message_receipts.rs` in full (consumer, leading locks, cursor, poison, effect resume, conflict park, `confirm_suspension`, workflow `begin_part` admission, reopen proof).
  - **Waits and supervision:** `wait_commit.rs`, `polling.rs` (paren depth of the extended WHERE verified by script; the OR is inside the AND group, so fair-polling and cursor filters still apply), `supervise.rs`, `recovery.rs`.
  - **Delivery and reconciliation:** `delivery/queue.rs` (`begin_part` admission and expired proof, `complete_part` sending guard, reaper `ORDER BY id FOR UPDATE SKIP LOCKED`); `action_reconciliation/delivery.rs` and `settlement.rs` (no `continue_on` for message targets); reconciliation `contracts.rs` (target in a v2 request digest, `DeliverySubject`, delivery coverage).
  - **Publication, controls and docs:** `authority.rs` (trusted built-in check), controls/maintenance cancellation calls, BRIEF-04.11 and `main.rs` NO_BC entry.
  - **Tests:** every test inventoried by name across `message_{tests,sql_tests,thread_tests,delivery_tests,race_tests,reconcile_tests,recovery_tests}.rs`, plus application/registry unit tests. Key ones were read: recovery rows 5/6/14, the finality-breach and conflicted-execution tests, the admission backstop, the reopen double-use and conflict cases, the attribution shape and the model_tool seam.
  - **Gate evidence:** `final-gate-status.log`, `final-gate-rerun-status.log`, `rerun-full-suite-failures.log`, `fixture-ci-stat-settings.log`, failure lines in `final-gate-full-suite.log`, result lines in the rerun and stack logs; CI steps `ci.yml:47-117`.
- Checks run: digest recomputation and read-only SQL-text analysis only. No cargo build, test or fixture query; disk allowed one, but I preferred inspection per the instructions. Gate results are taken from the logs, not re-executed.
- Lightly reviewed: the bodies of `settle_unknown_delivery_on`, `cancel_workflow_deliveries_on`, `reply_target_on`/`send_target_on`; the deviation-17 accepted-test pin edits (spot-checked only); `fair_polling.rs` beyond its use of `CANDIDATES_SQL`; HTTP/MCP `run()` wrapping (deviation 12).
- Root Q1 — the 18 deviations: none reopens the gate.
  - **6 (fenced-completion guard):** the original condition survives verbatim as one disjunct. The new disjunct admits only `stop_reason='effect_parked'` with an incomplete execution, run `waiting/effect`, a waiting effect wait linked to its action, the same lease-expiry and deadline checks, and `attempt.status='completed'`. It implements the decided park.
  - **10 (ambiguous COMMIT read of the intent):** a read-only resolution of this operation's own outcome under run/execution/job locks. Replays on re-entry still go through `observe` → `authorize_on`. Within D2 §2.6.
  - **13 (40P01):** the bound "at most 128 committed" still holds. The decided 54000 interleaving (intent committed first; marker is the first write, as on the Rust path) is the one tested. 40P01 arises only for an SQL-only writer that writes its intent in the same transaction, and exactly one commits.
  - **1, 3–5, 7–9, 11, 12, 14–18:** implementation detail consistent with the text. Deviation 14's parenthesization was verified.
  - **2** is understated: it also applies the hop-0 relay to *replies*, against D2 §1.3 (F015).
  - The deviation list omits one behavioural departure from D2 §5.1 row 2 (F015).
- Root Q2 — known gaps. Required by D2 §8.2, D3 §K or D4 §S, and therefore blocking (F016):
  - the F012 variants "breach → every destination accepted → resume vetoed" and the failed-receipt variant;
  - per-recipient inbound reply resolution, and Slack-source refusal;
  - delayed COMMIT past the captured fence expiry and past the run deadline;
  - SQL-only reopen from expired, non-final or foreign evidence;
  - wrong-association actor;
  - the competing settle-versus-consumer/`begin_part`/workflow-cancellation races (also the AGENTS.md competing-claimant rule);
  - `DeliveryReopened` on a `model_tool` execution.

  Not blocking:
  - `message.approval_required`: unreachable, since `context().approval_required` is always false and messaging's current policy never requires approval.
  - The explicit lock-count assertion: structurally bounded by CHECKs (`destination_count` 1–20, ordinal 0–19); replace it with a schema assertion or record why.
  - Remote evidence with a delivery subject: impossible by the composite FK to message destinations.
- Root Q3 — first-run failures: handled acceptably.
  - **Query health:** aligning the task fixture with CI's `pg_stat_statements` settings (`ci.yml:80-91`) is correct and does not weaken a criterion. The test file is unchanged, the failure log is retained, and the full suite and stack budget were rerun green.
  - **Timing test:** the accepted test is unchanged and passed 5/5 isolated plus two full parallel reruns. Caveats (optional F018):
    - its 0.15 s deadline window spans a whole-state `snapshot`;
    - the claim "04.11 does not change … the delay trigger's path" is imprecise, since `recovery::report` and `maintenance::expire_on` gained work;
    - the persistence AGENTS.md baseline step (stash and rerun) was not done.
- Root Q4 — areas deferred from the gate: sound.
  - **`completion_job`:** unchanged. The park writes the close itself, `Parked` resume does nothing to the closed job, the failed and conflict paths close it `failed`, and the guard admits only the park shape.
  - **Reconciliation Rust shapes:** sound. Target in a v2 digest; `DeliverySubject` coverage over every snapshot fact; remote fields unchanged.
  - **Claim/episode/budget SQL:** sound. `DeliveryReopened` never writes `scheduled`/`scheduled_job_id`, so no episode binding fires, and budget eligibility does not read reconciliation rows.
  - **fair_polling:** sound. It shares the corrected `CANDIDATES_SQL`.
  - **inspection:** not wired to receipt discovery, contrary to the D3 §E text. Diagnostic only; optional F017.
- Criteria verified sound in code (no finding):
  - one canonical message, thread and delivery per destination, with the per-recipient predicted Message-ID recorded as `MessageCorrelation::Email`;
  - classification matching D2 §1.3;
  - `message.send` never writes into an existing thread;
  - marker-first publication with no I/O in the transaction;
  - admission at 128;
  - DB-captured fence;
  - the receipt result recomputed in SQL;
  - the state guard as the SQL aggregate;
  - an effect-wait deadline equal to the run deadline, with receipt/conflict-gated discovery;
  - the conflict veto before reading the receipt (D4 §M);
  - workflow `begin_part` refusing on revoked actor, recipient change, run end or conflict;
  - the `complete_part` sending guard;
  - reaper parent locks;
  - outcome_unknown exits only by evidence (triggers);
  - one-use reopen guarded by conflict and expiry;
  - no `continue_on` for message targets;
  - BRIEF-04.11 records the phase-06 obligation, trigger and divergence.
- Outcome: **CHANGES REQUIRED — no PASS for `8891d740…cabd`.**
  - Actionable: [04.11-F015](agent-review-findings.md#0411-f015--unlisted-departures-from-the-decided-outcome-table-and-reply-relay) (low), [04.11-F016](agent-review-findings.md#0411-f016--tests-required-by-the-accepted-decision-are-missing) (medium).
  - Optional: [04.11-F017](agent-review-findings.md#0411-f017--expired-proof-path-commits-on-lease-loss-and-inspection-is-not-wired-optional) (low), [04.11-F018](agent-review-findings.md#0411-f018--timing-oracle-and-baseline-evidence-for-the-first-run-failure-optional) (low).
  - Combined phase-04 gate: every CI-parity gate passed on rerun per the logs, with the frontend/CSS/Playwright/SSE steps and the queryid probe not run, as disclosed. This review does not repeat them. After corrections, rerun the affected focused areas plus the full suite, stack budget and lease budget, because tests change.
- Process feedback:
  - The known-gaps list in TASK made the required-test audit fast. Several of those gaps are tests the decision named explicitly, though. Candidate instruction (scope: authors in this plan): "Before freezing, diff the decision's required-test list against the test inventory, and either implement each item or record a reviewed amendment; a 'known gap' is not a substitute."
  - The commit made during implementation blurred the 04.10/04.11 boundary. Candidate: "When a commit lands mid-batch, record which hunks of shared files belong to which batch."

## 04.11-R007 — Correction recheck of the 04.11 batch (final 04.11 + combined phase-04 integration) — PASS
- Identity: 2026-10-18; workflow `implement-opus-reviewed-2.1`; batch 04.11 messaging plus the combined phase-04 gate; correction recheck by finding ID. Reviewer B: Opus 5.5 (`claude-opus-5-5`), xhigh requested; effective effort not visible. Same reviewer and context as R002–R006. Records: [TASK](TASK.md) §"04.11 R006 corrections"; [REVIEW](REVIEW.md) §04.11.
- Artifact: base `5c1de32ecb3c1a7e59e1a60deb74ab5c42e755df`; digest `e76ef46c07965bb7b233d21813ac413efb6c5ad763e207ac3bc242168235c2cf`, recomputed with the skill script (match). Manifest `logs/04.11/r006-snapshot.txt`, 56 files. Effective decision: D2 `1dcd1a9e…3a44` + D3 `c462e4eb…8a89` + D4 `6bf5efcb…a276`, plus the amendments A1/A2 accepted below.
- Scope:
  - F015.1/.2, F016.1–6, F017, F018, by ID.
  - Broadened to the contracts the corrections change: `MessagePublication::Rejected` and the `rejected_on` boundary; the reply relay sourced from the inbound task payload; the race harness.
  - R006 evidence for unaffected code is reused. The migration is unchanged per the author and the identical fresh/fixture schema dumps (14454 lines).
- Evidence inspected:
  - **Publication:** `message_publish.rs:278-443` (`publish_on`, `rejected_on`, `observe`), `message_runtime.rs` (Rejected mapping at 505, tool path at 78, reply relay at 360-371), against the agent-answer precedent `use_cases/thread/dispatch.rs:1436-1446`.
  - **Relay source:** persistence `message.rs:431-457` (`source_relay_on`); the lookup key is UNIQUE (`background_tasks_company_source_message_channel_key`, init schema 3765); an undecodable payload is refused.
  - **Delivery queue:** `delivery/queue.rs:206-216` (rollback on `LeaseLost`).
  - **New tests:** `message_reconcile_race_tests.rs:1-170` (harness and the first two races) and `message_reconcile_veto_tests.rs:45-160` (both receipted F012 variants), plus assertion lines of the reopen negatives, wrong-association actor and model_tool reopen seam. Also the delayed-COMMIT test (`message_race_tests.rs`: deferred `aa_delay_commit` trigger plus a non-transactional sequence, both bounds), the per-recipient inbound resolution test (`message_thread_tests.rs`, through the real `thread_for_message_keys`) and the authority tests' names in the lease log.
  - **Docs:** BRIEF-04.11 lines 10-23.
  - **Gate logs:** `r006-final-gate-status.log` (every gate PASS), `r006-focused-status.log` (A 63, B 36, C 91, D 386, E 100, F 199, H 267), result lines of the full suite (2669 + 5 passed, 0 failed, 22 ignored) and stack budget (2669/0/22), and the lease-budget table (`message.publish` 21 ms, `message.consume` 2 ms against 2000 ms; 92 injected events excluded).
- Checks: digest recomputation only. No cargo build, test or fixture query; gate results are taken from the logs.
- Dispositions (details in the findings log): F015 resolved; F016 resolved; F017 resolved (rollback adopted; the inspection claim withdrawn via A2); F018 addressed (wording corrected; oracle recorded; stashed baseline reported as not run). No new findings.
- **Row-2 boundary: correct.**
  - `rejected_on` returns Terminal `message.rejected` only for refusal-class errors (`NotFound`/`BadRequest`/`InvalidCredentials`) that leave the transaction usable, when the same transaction shows no marker for the invocation.
  - With a marker, or any other error (fence loss is `Conflict`, SQL or lock errors are `Database`), it stays a retryable dispatch failure, which the shared predicate then decides.
  - A refused publication has rolled back, so Terminal can never strand a written effect. A marker means the invocation may already own deliveries, so it must not be terminally failed by the handler.
- **Race tests: real competing claimants as AGENTS.md requires.**
  - Each competitor is a separate database transaction contending for the production locks (run `FOR UPDATE`/`FOR SHARE`, action, parents).
  - A third connection holds the run row until every competitor is observed queued behind it (`pg_blocking_pids`, no sleeps), then releases them.
  - The assertions are order-independent. Limitation: the realized order is decided by the PostgreSQL lock queue, so each run exercises an arbitrary, not exhaustive, interleaving.
- **Amendments:**
  - **A1: accepted as a reviewed amendment to D4 §S.** D4 §M step 4's writer `action_uncertainty::record` skips receipted markers. The accepted `workflow_action_reconciliation_guard` refuses an observation for a receipted invocation. In the receipted variants the first-uncertainty row already exists, and the table's PK is per invocation. So "an uncertainty row with `action.evidence_conflict`" was internally inconsistent for receipted markers. The substantive veto is asserted in full: no successor or output, wait `failed/evidence_conflict`, job `failed`, run `waiting/reconciliation`, receipt retained, a `finality_breach` conflict, `committed`+conflict, and `reopen_safe` false. The receipt-less variant still records `action.evidence_conflict`.
  - **A2: accepted as a reviewed amendment to D3 §E** (drop "and `inspection`"). `inspection.rs` is a run-head reader and never consumes candidate discovery; discovery is shared by `poll_work` and `fair_polling`; no phase 04 criterion needs a diagnostic view of receipt work.
- Outcome: **PASS for digest `e76ef46c07965bb7b233d21813ac413efb6c5ad763e207ac3bc242168235c2cf`.** This is the 04.11 whole-batch review and the combined phase-04 final integration review, for the effective decision D2+D3+D4 with A1/A2.
- Remaining disclosed limits: the CI frontend/CSS/Playwright/SSE steps and the `queryid` probe were not run (no frontend change); the stashed-baseline rerun for the F018 timing oracle was not run; F018's optional test hardening is left to a reviewed change.
- Process feedback: the correction round was complete and finding-indexed, and every required test now maps to a named test. No new instruction candidate beyond R006's.

## S-R001 — Batch review of Batch S (migration squash) — CHANGES REQUIRED
- Identity: 2026-10-18; workflow `implement-opus-reviewed-2.1`; Batch S (migration squash, before phase 05); batch review. Reviewer B: Opus 5.5 (`claude-opus-5-5`); effective effort not visible. Same reviewer and context as 04.11-R002–R007. Records: [TASK](TASK.md) §"Batch S — migration squash"; [REVIEW](REVIEW.md) §"Batch S".
- Artifact: base `4b54485830495dfeb68d45c61c0d6d8dd684e4c5`; digest `0aeeca933e66244420f6b05b37d645cc79facbc2307890591b187d0f9ac43fa5` (excluding `plan/workflow-foundation/docs`), recomputed with the skill script at the start and the end of the round (match). Manifest `logs/S/snapshot.txt`.
- Requirements, from the originals:
  - The user's 2026-10-18 instruction "squash migrations" (TASK "Phase 05 — 2026-10-18", "Scope and authorization").
  - Root AGENTS.md: no backward compatibility, reset instead of upgrade; CI exercises migrations and the database-backed suite.
  - Phase 05 plan "Schema and data" (and the same paragraph in phases 06–10): later phases edit `migrations/20260817000000_init_schema.sql` in place; no additive migrations, backfills, shims or populated-upgrade tests.
  - `src/adapters/persistence/AGENTS.md`. Its §1 still calls applied migrations immutable; judged against the user's explicit instruction and recorded as an observation below.
- Evidence inspected:
  - TASK Batch S record (method, equivalence proof, dependents, proposed AGENTS text, gates).
  - `logs/S/equivalence.sh`, `equivalence.log`, `squash-source-dump.sql`, `gates.sh`, `gate-status.log` (every gate PASS), result lines of `gate-full-suite.log` (2663 + 5 passed, 0 failed, 22 ignored), `focused.log`, `gate-stack-budget.log`.
  - The full diff of `migrations/20260817000000_init_schema.sql` against `logs/S/squash-source-dump.sql`: exactly the three header lines, `\restrict`/`\unrestrict`, the version banner, `SET transaction_timeout`, `set_config('search_path','',false)` and the four BETWEEN rewrites (24 changed lines).
  - Every source diff: `test_support.rs`, `workflow/tests.rs` (helper), the six repinned assertion files, `proof_tests.rs` and `mod.rs` (module removals), the conflict-immutable and chronology-fixture comments, `budget_schema_tests.rs`, `wakeup_guard_tests.rs`, README, and the three deleted files at base.
  - Dependents: `src/infra/db.rs:24` (plain `migrate!().run`, so an unreset database fails loudly on the missing versions and the changed init checksum), `.github/workflows/ci.yml:85,97` (generic `cargo sqlx migrate run`, PostgreSQL 18.6), `Dockerfile:16`, `scripts/reset-db.sh` (`--all` exists, as README cites), `scripts/tests/harness-query-plans.py:14-20` (its regex finds both functions in the new init; it is a manual tool, not in CI), `.sqlx` (no diff), phase 05–10 plans (already name the init file), `docs/query-evidence/2026-09-12-message-task-lookup.md` (historical). No code, script or CI reference to a deleted migration remains.
- Checks run (read-only; logs in `logs/S-R001/checks.log`):
  1. Ledgers. Presquash `_sqlx_migrations` checksums equal the SHA-384 of the 56 files at base. The author's `presquash-migrations.sha256` equals the base blobs. Postsquash holds one row, whose checksum equals the SHA-384 of the current init file. So both databases are built from exactly the artifacts under review.
  2. A deparser-independent catalog comparison (`logs/S-R001/catalog-nodes.sql`, 6236 rows per database). It compares stored node trees (CHECK, DEFAULT, index expression/predicate, trigger WHEN, view/rule, SQL-standard bodies) with `:location` stripped and user OIDs masked, plus `prosrc`. It also compares attributes the author's public-only fingerprint omits: reloptions, replica identity, RLS, attstorage/attcompression, collations, opclass names, inheritance/partitioning, policies, rules, extended statistics, event triggers, database role settings and pg_depend class counts. One difference (`logs/S-R001/catalog-nodes.diff`): the `'{}'::citext[]` default on `channel_schedules.recipient_emails`, whose empty-array datum embeds the citext element-type OID. That is not semantic.
  3. Emulated `init_function_body` against all 148 public functions in postsquash. It returns exactly `prosrc` for all 140 dollar-quoted bodies. None is overloaded, and all 16 pinned names occur once.
  4. Scanned the 56 base migrations: no role, database, privilege, `search_path` or no-transaction statements, so a schema-only dump loses nothing they configured.
- Judgements:
  - **Equivalence proof: sound.** The full pg_dump diff covers every schema in the database, with owners, ACLs and comments. The fingerprint uses the same deparser as pg_dump, so on expressions it is not independent; check 2 closes that gap. The BETWEEN rationale is correct. PostgreSQL turns BETWEEN into its own AND node, while the deparsed `(a >= x) AND (a <= y) AND r` re-parses into one flattened three-arm AND, a different stored tree. The rewrite restores the original tree, and the node comparison confirms it.
  - **The stripped items are correct.** `\restrict` is psql-only. The banner is cosmetic. `transaction_timeout` is a session setting. The empty `search_path` reset would persist on sqlx's connection and break its unqualified ledger insert. The dump's DDL is schema-qualified, and PostgreSQL searches pg_catalog first, so dropping the reset changes nothing that is created. That matches the previous baseline's shape.
  - Rows: none in either database. No migration's DML seeded data on a fresh database; its INSERTs are trigger bodies or backfills of existing rows.
  - **The fresh-ledger test** (`test_support.rs`, `a_fresh_database_applies_only_the_init_schema`) subsumes the old fresh-exact test. It checks: exactly one embedded migration, `(20260817000000, "init schema")`; the ledger equals it with its checksum and success; a rerun is a no-op; no table is seeded. Asserting a single migration also makes a future additive migration fail red, which is the plan's rule. It ran in focused, full and stack-budget runs.
  - **The removed tests (7): justified as upgrade tests, except one assertion.**
    - Upgrade-only: the three `action_reconciliation_upgrade_*` tests and the two catalog-diagnostics tests, which only controlled the upgrade comparison helper used by `upgrade_tests`. So is the wakeup migration replay; its post-replay runtime behaviour is covered by the 14 remaining `workflow_wakeup_*` tests ("noops do not repeat", "sibling progress … independent").
    - The budget replay also asserted that a sealed root's new `model_call` debit is `exhausted`. That is live behaviour of `debit_workflow_budget_receipt` and `insert_workflow_budget_link` in the init schema, and nothing else tests it: [S-F001](agent-review-findings.md#s-f001--the-removed-budget-replay-carried-the-only-assertion-of-the-sealed-root-debit-rule) (low).
  - **`init_function_body` and the six repins: correct.** The six are attribution, consumption identity, high bounds, native attribution, native coverage and state witness, with 16 names in all. Each compares live `prosrc` (or the `pg_get_functiondef` body) with the dollar-quoted body in the init file, which pg_dump emits verbatim, so equality is exact. On a fresh database the pins now mostly prove that nothing else redefines the function; the paired behaviour tests carry the substance, as before. Optional robustness: [S-F002](agent-review-findings.md#s-f002--init_function_body-reads-past-a-function-without-a-dollar-quoted-body-optional).
- Observations (no finding):
  - **AGENTS.md conflict, for the user.** `src/adapters/persistence/AGENTS.md` §1 (lines 186-194) contradicts the user's squash instruction and the phase 05–10 plans. Two more passages in the same file also prescribe the old model: line 30 ("use an additive migration and validate compatibility…") and line 160 ("A credential migration requires staged compatibility: support legacy reads, backfill…"). Both already conflicted with the root no-backward-compatibility rule. The author's proposed replacement covers only §1, so the user's decision should cover all three. I judged the batch against the user's explicit instruction.
  - 13 named `CONSTRAINT … NOT NULL` column clauses from pg_dump 18 are new in the init file. CI and the fixture run PostgreSQL 18.6, so no action.
  - `own_database_with_migrations` now has one caller with a fixed migrator; the parameter is vestigial but harmless.
- Outcome: **CHANGES REQUIRED** for digest `0aeeca933e66244420f6b05b37d645cc79facbc2307890591b187d0f9ac43fa5`. Actionable: S-F001 (low). Optional: S-F002. The schema squash itself (the init file, the deletions, the equivalence evidence, the dependents and the helper) is accepted as reviewed. The recheck needs only the S-F001 test, its focused run, fmt/clippy/no-run, and a new digest.
- Process feedback: the record was complete and reproducible. Instruction candidate: when deleting a test as upgrade-only, list each of its assertions and mark which ones exercise live schema or runtime behaviour, then replace those against a fresh schema.

## S-R002 — Correction recheck of Batch S (S-F001, S-F002) — PASS
- Identity: 2026-10-18; workflow `implement-opus-reviewed-2.1`; Batch S (migration squash); correction recheck by finding ID. Reviewer B: Opus 5.5 (`claude-opus-5-5`); effective effort not visible. Same reviewer and context as S-R001. Records: [TASK](TASK.md) §"Batch S — S-R001 correction"; [REVIEW](REVIEW.md) §"Batch S".
- Artifact: base `4b54485830495dfeb68d45c61c0d6d8dd684e4c5`; digest `33ae9028d28d3047de936b2e9580ffe267bd0187dafe2092601d0cef51514552`, recomputed with the skill script (match). Manifest `logs/S/sr001-snapshot.txt`: the same 74 paths as `logs/S/snapshot.txt`. Only `test_support.rs`, `workflow/budget_schema_tests.rs` and `workflow/tests.rs` changed after the S-R001 gate, matching the TASK record.
- The init schema is byte-identical to S-R001's: its SHA-384 equals the postsquash ledger checksum. The S-R001 equivalence evidence and its acceptance therefore carry over unchanged.
- Evidence inspected:
  - the full new test `workflow_budget_schema_sealed_roots_exhaust_every_debit` (`budget_schema_tests.rs:209-304`) and its helpers `limited`/`receipt` (`:4-20`);
  - the corrected `init_function_body` and its `should_panic` test (`workflow/tests.rs:31-58`);
  - the `migrated_own_database` change (`test_support.rs:287-319`);
  - `logs/S/sr001-focused.log` (29 run: 28 passed, 1 ignored; the test list includes the new test, the panic test, the fresh-ledger test and all six repinned catalog pins), and `sr001-checks-status.log` (fmt, Clippy `-D warnings`, no-run PASS; `diff` failed only on my trailing space).
- Checks (read-only; `logs/S-R002/checks.log`): digest recomputation; the file-change scan; the init checksum; and an emulation of the corrected helper against all 148 public functions of `workflow0410_postsquash`. The emulation gives 140 exact `prosrc` matches, refuses all 8 SQL-standard bodies, and finds no mismatch. No cargo run.
- **S-F001: RESOLVED.**
  - Two raw root runs are inserted beside an admitted one, on a fresh schema with no replay or reconstruction:
    - one bundle with `compiler_revision` `"1"`, which `->>` reads as text `'1'`, the same as a numeric 1, so it is the not-revision-2 branch;
    - one that is not JSON, so the `::jsonb` cast raises 22P02, which the `data_exception` handler catches.
  - Each is asserted `sealed_legacy` with `[null,null,null]` limits and `[0,0,0]` usage. Every resource's positive debit is `exhausted`, read from `RETURNING` (the stored disposition after the BEFORE trigger). Usage is re-asserted unchanged.
  - Control: the admitted current-revision root is `frozen_v2` `[2,2,2]` and grants a `model_call` (usage `[0,1,0]`).
  - This covers both sealing branches of `insert_workflow_budget_link` and the debit rule of `debit_workflow_budget_receipt`. It also catches a `SEMANTIC_REVISION`/SQL-literal drift, because the control would then seal.
  - Root's decision to keep the sealing branch for now is recorded in TASK.
- **S-F002 (optional): RESOLVED.**
  - The helper now scans only the function's own statement, up to the next pg_dump `\n--\n-- Name: ` header, and panics naming the function when there is no dollar-quoted body. The `should_panic` test pins this on `valid_creation_provenance`.
  - A body that itself contained such a header would truncate the statement and then panic on the missing closing tag, so it still fails loudly.
  - The vestigial migrator parameter is gone.
- **`migrated_own_database` (shared test infrastructure): no behavioural change.**
  - It is a private, single-caller inlining of the same `sqlx::migrate!("./migrations")` that `own_database` already passed, with the same create/connect/migrate/prewarm sequence and only the panic text changed.
  - The future stays boxed (`Box::pin`), so stock-stack frames are unaffected. The stack budget gate need not rerun.
  - It compiles under Clippy `-D warnings` and no-run. Every focused database test went through it (`AdmissionFixture` uses `own_database`, `workflow/tests.rs:98`).
- **Full-suite rerun: not needed for this PASS.**
  - The corrections are test-only, plus the equivalence-preserving inlining above, and every changed path ran in the focused set.
  - The S-R001 full gate already covered the rest of this artifact unchanged (2663 + 5 passed, 0 failed).
  - Root's network-isolated rerun (`logs/S/sr001-full-suite*`) is still running; when I last looked it had about 1267 of 2687 tests `ok` and no failure. It is welcome confirmation for root's acceptance record. If it reports a failure, that failure reopens the batch regardless of this PASS.
- Housekeeping: I removed the trailing space at `agent-review-findings.md:1073` (my S-F001 text). `git diff --check` is now clean.
- AGENTS.md observation from S-R001 stands for the user (§1 plus lines 30 and 160 of `src/adapters/persistence/AGENTS.md`). It is not a finding.
- Outcome: **PASS for digest `33ae9028d28d3047de936b2e9580ffe267bd0187dafe2092601d0cef51514552`.** It covers the Batch S batch review, with S-F001 and S-F002 resolved and no open findings.
- Process feedback: the correction was complete and finding-indexed. Root's decision on the alternative was recorded rather than left implicit.
