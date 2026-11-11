# p05 reviewer notes

## 01DEP-P05-B1-R1

2026-11-11, `implement-astra-reviewed-2-2`; whole p05 B1 plus combined integration evidence.
Task: [TASK.md](TASK.md); outcome: [REVIEW.md](REVIEW.md). Reviewer `/root/p05_reviewer`, requested
`gpt-6-astra` / `high`, fresh context. Effective model/effort and context occupancy/compaction
controls are not independently exposed in this reviewer session. No agents spawned and no
project/design edits made.

Identity: `/Users/mac02/.codex/worktrees/2050/openbot`, branch
`codex/departments-addressing-plans`; phase start/review base
`c42fc15dd9eb05c9f463703b26a809d42929ab16`; HEAD
`e7bc780ec7699051b99893d4f497bf2299c77738`; artifact
`b26116788fe4639df6d7a2a3dd8789a12a34736ca8450bcfb9e977f14bbaab50`.
Exact exclusions: `plan/01-departments/phases/p05/docs` and
`plan/01-departments/COORDINATION.md`. GNU-coreutils PATH used as assigned. Initial and final
snapshot match. Reviewed committed changes from phase base plus staged/working delta; preserved
checkpoint is included. No new gated decision in p05.

Coordination: authoritative `/Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/COORDINATION.md`,
writer `/root`, r12; [immutable pin](logs/coordination-r12.md). Read before review and compared
byte-for-byte again before return. Applicable I001 r1, I002 r2, I003 r2; C001–C004 r1.
No notice changed during review.

Coverage and evidence:

- Read original checkpoint HANDOFF via `git show e7bc780e:.../HANDOFF.md`, original plan README
  and plans 01–05 under the authoritative coordination overrides, root/source/application/
  persistence AGENTS guides, and skill review/verification references. Graft-first discovery
  used for production paths. The entire p05 deliverable diff is only the provisioning table
  comment, completion table and authorized p04 evidence repair. The comment retains both
  provisioning keys and matches the additional-desk wording. No executable schema, SQL query,
  concurrency protocol, budget or test changed in p05.
- Independently checked all nine prerequisite accepted-manifest identities and their review
  outcomes, and reused unchanged p01–p04 evidence rather than repeating their full reviews.
  [B1-R1-reviewer-identities.txt](logs/B1-R1-reviewer-identities.txt) records matching hashes,
  artifacts and round IDs. For both p04 repairs, retained original manifests exactly match
  checkpoint Git bytes, the JSON delta is exactly one path value, and the restored file hashes
  equal the original R1 hashes. Accepted R2 records remain untouched.
- Checked [TASK verification and contract mappings](TASK.md#verification-contract-mapping-resumed-b1-current-results)
  against [R1 evidence](logs/01DEP-P05-B1-R1.evidence.json), CI and check.sh. Independently ran the
  skill manifest validator (56 PASS, 7 FAIL, 3 NOT EXERCISED) and repository aggregate validator
  (46 manifests valid). These establish retention, not behavioral acceptance. Retained original
  failed checks and successful targeted correction evidence remain separate.
- Inspected actual full-gate results: 18 summary entries, library 4,142 passed / 2 failed /
  24 ignored, stock stack 4,143 / 1 / 24 at 2,048 KiB without overflow. Binary 5/5 and
  source reachability 8/8 are separately evidenced, accounting for Cargo stopping after the
  failed library target. Offline compilation, formatting, clippy, fresh migrate/SQLx prepare,
  network wrapper, inventory, lease operation families and unchanged 50% WORK threshold were
  checked in retained logs. Did not rerun these expensive tests.
- Confirmed unrelated-failure attribution: MCP cancellation test uses the direct client and
  scripted server, not department code; test, implementation, scripted scenario, dependency
  lockfiles and stack configuration match merged main `6091c3cd`. Its isolated failure and
  stock-run success are retained. Scanner points at the unchanged hostile sample-test markup;
  source equality independently checked. No affected assertion was weakened. Full/stock
  commands remain FAIL, not relabeled PASS. See
  [reviewer audit](logs/B1-R1-reviewer-audit.txt) and
  [author failure evidence](logs/B1-unrelated-failures-source-identity.txt).
- Inspected the 28 CI static/protocol command outputs and runner's nonzero Rust-test/inventory
  checks. Real browser runs report positive pass counts and zero skipped tests. Initial knowledge
  and memory missing-browser failures remain separate from successful phase-cache reruns.
  Plugin package/runtime logs show reproducible package output and locked production install
  under pinned Node/npm. Unchanged browser internals and unrelated plugin tests were reviewed
  through execution evidence only, not fully rereviewed line by line.
- Independently inspected integrated grants (`department_desks.rs:155–199,230–336`), caller
  graph into role/membership/update/delete/placement, role-write propagation
  (`department.rs:606–708`), and real one-step hierarchy assertions
  (`department_desks_tests.rs:403–477`). Tests cover parent/child admission, grandparent/
  grandchild refusal, lead replacement, reparent and role clearing. Current full and stock runs
  exercise these and the competing-writer families. No integration defect found here.
- For C003, followed `preparation.rs:220–265` to `department_roster.rs:239–278`, including
  public-desk roster omission; inspected frozen-retry test `agent_history_department_tests.rs:41–84`
  and production Deployment lead/default-desk assertions `provisioning_runtime_tests.rs:770–901`.
  Current full runs pass these cases. Combined automatic desk, role grant and frozen context
  paths agree with accepted evidence and documentation.
- For C001/C004, examined actual relay test assertions
  (`department_desk_relay_tests.rs:51–119`), planner `message_planner.rs:288–323`, writer and
  routing identity/delta, shipped department/relay/tool docs and fixture. Trust admits member
  and nonmember relays, repeated desk is refused; `message.send` still creates an outbound
  system note without target admission. Accepted messaging behavior is unchanged. Documentation
  properly defers dispatch/escalation/round-trip/loop checks to `channel.call`; no automated
  round trip is claimed. These notice dispositions hold independently of overall acceptance.

Outcome: **Changes required**, [01DEP-P05-B1-F1](agent-review-findings.md#01dep-p05-b1-f1), Medium:
the explicit CI query-statistics activation assertions have no evidence or unavailable
disposition. Also [01DEP-P05-B1-F2](agent-review-findings.md#01dep-p05-b1-f2), Medium: author
reported staged raw-evidence whitespace failures during review; independently reproduced exit 2
and retained bounded locations in `logs/B1-R1-reviewer-whitespace.txt`. Earlier whitespace PASS
does not establish the final staged archive is clean. No other actionable defect found.
The batch is suitably cohesive; no missing architecture gate. A correction recheck can reuse
the unchanged source and execution evidence and focus on the inventory/disposition delta.

Remaining limits: Docker production-image, Doop deployment and real memory-service acceptance
are NOT EXERCISED and block full p05/integrated-plan completion. The omitted query-statistics
check is also unverified. Retain p03's inspected-only no-draft-step/resource-slot refusals and
S != B branch, and the deleted-desk test's recorded plain-desk stand-in; this review does not
upgrade them to behavioral coverage. C004's dispatch tests are explicitly deferred by user
decision, distinct from unavailable CI gates. No full p05 acceptance or integration PASS given.

Process lesson: preserve immutable check outputs and map CI's named assertion steps, including
environment activation/privacy checks, to the final inventory. F1 supports this narrow candidate;
no skill or AGENTS change made. Graft context and retained prerequisite identities materially
reduced unnecessary source re-review. No claim of uniform fresh review of all prior phases.

## 01DEP-P05-B1-R2

2026-11-11; `implement-astra-reviewed-2-2`; p05 B1 correction recheck of F1 and F2 together.
Same reviewer `/root/p05_reviewer`, requested Astra/high, independent context retained; effective
runtime model/effort and compaction controls remain unexposed. Worktree/branch, phase start and
review base `c42fc15dd9eb05c9f463703b26a809d42929ab16`, HEAD
`e7bc780ec7699051b99893d4f497bf2299c77738` unchanged. New artifact:
`19a7c2b6b0a8349712e5180147b487590acd194bc32d95846d82030598519b3b`.
Exact exclusions still p05 `docs` and plan-root `COORDINATION.md`; the new p05 `.gitattributes`
is a deliverable inside the snapshot. No new architecture decision/gate.

Read authority `/Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/COORDINATION.md`,
writer `/root`, revision 13 and [pin](logs/coordination-r13.md). Compared r12/r13: only progress
metadata changed; I001 r1, I002 r2, I003 r2 and C001–C004 r1 unchanged. Initial/final snapshots
match the assigned R2 digest; final pin comparison succeeds.

Reviewed [TASK correction mapping](TASK.md#b1-r1-review-and-correction-batch-r2),
[R2 evidence manifest](logs/01DEP-P05-B1-R2.evidence.json), entire correction deliverable delta,
and actual check assertions/results. Reused R1's whole-batch/combined-contract review, accepted
prerequisite identities and unchanged execution evidence. All 66 original manifest entries are
reused verbatim and every referenced original file still matches its pinned SHA-256. No Rust,
schema/query, runtime, budget or concurrency change warrants repeating the full suite.

- F1: independently compared the retained owned-cluster helper/assertions with CI's named
  statistics activation step. Actual PostgreSQL 18.6 startup, fresh migration, six SHOW results,
  numeric queryid and normalized `SELECT $1 AS ci_pg_stat_statements_probe` establish the missing
  criterion. It is real PostgreSQL on an isolated loopback port, not a mock. Inspected server and
  resource logs; shutdown completed, and independently confirmed absence of the recorded
  cluster's PID file. Did not rerun the immutable helper or mutate the shared server.
- F2: inspected the four-line phase-local Git attributes and effective archive/source/Markdown/
  Python/JSON matrix. The archive policy preserves bytes, changes no behavioral assertion, and
  does not suppress source checks. Independently reran complete staged and unstaged whitespace
  checks, validated R2 artifact/retention, and validated all 47 tracked manifests. All pass.
  Retained independent output: [B1-R2-reviewer-checks.txt](logs/B1-R2-reviewer-checks.txt).
- Completion text now names query-statistics evidence and the narrow raw-archive policy. Failed
  attempts remain in R2 (61 PASS / 8 FAIL / 3 NOT EXERCISED); retention success is not interpreted
  as overall behavioral success. External Docker/image, Doop and live-memory jobs remain
  explicitly unexercised. Unrelated full/stock failures, inherited inspection-only/stand-in
  coverage, and C004's user-deferred dispatch checks retain their R1 dispositions.

**PASS for reviewed corrections/local artifact at the R2 digest above.**
[01DEP-P05-B1-F1](agent-review-findings.md#f1-resolution--2026-11-11-01dep-p05-b1-r2) and
[01DEP-P05-B1-F2](agent-review-findings.md#f2-resolution--2026-11-11-01dep-p05-b1-r2) are fixed;
no new actionable findings. This pass does not accept p05 as complete or mark the integrated
plan complete: three required external verification jobs remain gaps.

The cohesive two-finding correction batch and immutable R1 reuse were appropriate. No additional
instruction change is supported beyond R1's inventory and final-retention-check lessons.

## 01DEP-P05-B1-R3

2026-11-11, `implement-astra-reviewed-2-2`; p05 B1 final integration review. Same independent B
`/root/p05_reviewer`, requested Astra/high; effective runtime/compaction settings remain unexposed.
Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`;
phase/review base `c42fc15dd9eb05c9f463703b26a809d42929ab16`, HEAD
`e7bc780ec7699051b99893d4f497bf2299c77738`; frozen digest
`41b58b0262aabcca57dfe1f0ce401f65c99ddafdfcb03304bb54653ae7b56056`.
Exact exclusions remain p05 docs and plan-root COORDINATION.md. Initial/final GNU snapshots match.

Authority `/Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/COORDINATION.md`, sole
writer `/root`, r16, [immutable pin](logs/coordination-r16.md). Compared to r13: progress only;
I001 r1/I002 r2/I003 r2 and C001–C004 r1 unchanged. Rechecked pin freshness before return.
C001/C004 current-contract dispositions remain as R1/R2; no new architecture decision/gate.

Reused R1/R2 review of unchanged phase dependencies, source/contracts and execution evidence.
All R2 exercised entries are reused verbatim; all R2 file hashes still match. The three earlier
NOT EXERCISED entries are preserved in historical manifests and replaced by new R3 execution
evidence. Reviewed [TASK R3 mapping](TASK.md#r3-verification-mapping-and-contract-impact),
[R3 manifest](logs/01DEP-P05-B1-R3.evidence.json), full new source delta and completion text
against original plan 05A and actual CI commands. Evidence retention is separate from sufficiency.

New code and fixture review:

- Graft-first traced `doop_e2e_live_company_documents` and its production helper consumer,
  `known_documents`, `lost_receipt`, `Environment::uncertain/health`, scenario `finish/serve`,
  and the cfg(test) module root. The only Rust change moves completed model teardown before
  independent lost-receipt work. Known workflows are already terminal; uncertain runs contain
  only `create_canvas`, and health is `data.map`, so neither needs the closed model listeners.
  Exact five-exchange, secret absence, manual reconciliation/no blind replay, independent-worker
  progress and immutable guard assertions remain. Scenario lifetime stays 60 seconds; native
  helper remains 80 seconds. No runtime/schema/concurrency/budget change.
- Earlier failed execution shows models starting at 15,372ms, document terminal at 72,577ms,
  independent receipt/health work ending at 75,448ms, then model lifetime failure. The new order
  correctly limits model ownership to its actual scenario without extending timeouts. The later
  S3 deadline failure remains retained; final success follows an explicitly documented owned VM
  CPU allocation change from 4 to 8, with application/probe bounds unchanged.
- Focused tests execute two receipt/parser assertions, with the live helper explicitly ignored;
  actual live helper execution comes from all four official private profiles. Formatting,
  locked offline all-target compile and clippy logs pass. No renamed/removed test; old/new helper
  discovery retained. Full/stock repetition is unnecessary for this cfg(test)-only teardown move.

New integration evidence examined:

- Production image checker result names real immutable image `sha256:6f9bbcdc…a753c`, non-root
  `mailagents`, offline/read-only runtime, model identity/shape/norm, 16/16 retrieval cases,
  exact-first 3/3, missing/tampered startup guards and 1,522,753,536-byte peak RSS under the
  unchanged 1,536 MiB cap. Read-only Docker inspection independently matches image ID/user.
- Managed Doop source/frozen Bun install and format/typecheck/lint/doc-lint/build commands match
  CI. Actual full test command succeeds after the narrowly scoped Docker discovery wrapper;
  original fixture-discovery failures remain. It reports 2,711 passed and 17 skipped. Inspected
  the source skip guard, finding F3: these are Darwin-specific skips, whereas CI is Ubuntu.
- Official `acceptance.py` requires all four fresh profiles, matching helper/source/image
  identity and confirmed cleanup. Inspected report-validation and exact-helper invocation:
  nonzero exit/signal/missing marker fails; names/statuses/counts and bounded requests are checked.
  Independently hashed current immutable helper `fc79364e…a9a9`, changed Rust source, all four
  report/cleanup files and aggregate references. Final profiles pass 42/43/9/10 checks (104 total)
  with identical source `e0c063c4…4464` and private image `sha256:a926537d…a285`.
- Inspected actual lifecycle and E2E records plus their consumers: two distinct companies,
  frozen publications/current keys, disabled/disconnected requests without dispatch, retained
  remote assets, exact model exchanges, targeted edit fresh readback, real create with lost
  receipt parked as unknown, no blind replay, independent workflow progress and private human
  browser/export assertions. Real PostgreSQL/TLS/deployed Doop paths are exercised. Scripted
  provider HTTP is deliberate in CI; paid live inference and human visual review remain explicitly
  NOT EXERCISED within those records and are not requirements of this gate.
- Actual memory script builds/serves the real memory service, provisions isolated pgvector roles,
  verifies cross-database denial, starts its declared Messages API stand-in and requires both
  ignored test families plus an observed API request. Output exercises all six required tests.
  Inspected competing delivery-worker assertions (one nonreplayed observation per intent),
  revoked credential replacement, and real-service audience levels 6/8/9 with lower-reader/drop
  refusal. Pinned model verification and cleanup receipts retained. No mock memory service used.
- Retained failed attempts remain FAIL and historical unavailable entries remain in R1/R2.
  Docker inspection confirms no retained runtime containers; native-cluster cleanup is recorded.
  Wrapper names only the owned profile; global/shared resources were not mutated by this review.
  Read-only checks and identity receipts: [B1-R3-reviewer-identities.txt](logs/B1-R3-reviewer-identities.txt).
  Independent whitespace/skill/48-manifest retention checks:
  [B1-R3-reviewer-validation.txt](logs/B1-R3-reviewer-validation.txt).

Outcome: **Changes required**, [01DEP-P05-B1-F3](agent-review-findings.md#01dep-p05-b1-f3), Medium:
17 required CI clipboard assertions were skipped by the differing host platform. No additional
actionable defect found in the source correction or other reviewed integration evidence. Root
confirmed no equivalent focused evidence exists. Do not mark the integrated plan complete yet.
The completed checks can be reused for a focused F3 correction recheck; no broad rerun needed.

Inherited limits remain: full/stock runs retain confirmed unrelated scanner/MCP failures, not a
wholly green-suite claim; p03 stand-ins/inspection-only branches and user-deferred C004 dispatch
tests remain unchanged. The batch remains coherent. Candidate instruction lesson from F3:
compare platform-specific skips against CI discovery before treating a successful service command
as complete coverage. No skill/AGENTS edit made.

## 01DEP-P05-B1-R4

2026-11-11; `implement-astra-reviewed-2-2`; F3 correction plus combined final integration recheck.
Same B `/root/p05_reviewer`, requested Astra/high, independent context retained. Effective
runtime settings/compaction controls remain unexposed. Worktree
`/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`;
phase/review base `c42fc15dd9eb05c9f463703b26a809d42929ab16`, HEAD
`e7bc780ec7699051b99893d4f497bf2299c77738`; artifact
`b715976039bbe891c5771d5e7946621afe528baf95b4d57bfd6fd41f59b89e35`.
Exact exclusions unchanged: p05 docs and plan-root COORDINATION.md. Initial/final snapshots match.

Read authoritative `/Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/COORDINATION.md`,
sole writer `/root`, r18 and [pin](logs/coordination-r18.md); rechecked freshness before return.
I001 r1/I002 r4/I003 r2 and C001–C004 r1 acknowledged. I002 now records the user's readiness
clarification; no push/plan02 implementation occurred. No review-criterion or architecture change.

Reviewed [TASK R4 correction](TASK.md#r3-review-outcome-and-r4-correction),
[R4 manifest](logs/01DEP-P05-B1-R4.evidence.json), complete new source delta, actual assertions and
new package/helper/image consumers. Reused unchanged R1–R3 coverage and prerequisite identities;
independently verified all R1/R2/R3 pinned file hashes remain intact. No broad/full suite rerun.

F3 and managed fixture:

- Actual non-root Linux Chromium run exercises all 17 clipboard cases, with zero skips/failures.
  Command uses network isolation, dropped capabilities, no-new-privileges, the established browser
  seccomp profile and bounded container resources; there is no host pasteboard mount. All original
  clipboard tests/timeouts are byte-identical to the previous managed source, including external
  image/text replacement, native copy/paste focus, grouped undo and protection cases.
- Inspected the canonical managed.patch addition and source pins, actual HTML harness, existing
  `fixturePrivateMaterializer`, sibling rendering fixtures and the clipboard test consumer. The
  HTML-only provider corrects setup for a synthetic layout/interaction test. It does not prove
  server authorization, intrinsic decode/admission or server budgets; comments and TASK state
  those limits. Real deployment checks cover those independent boundaries. No managed production
  code or clipboard assertion was modified to obtain green output.
- Independently compared 1,159 old/new managed source files. Only
  `tests/fixtures/canvasClipboard.html` differs. Excluded dependency/build/Git directories and the
  generated `public/claude-extension/doop.mcpb` output (ignored and built by package.json), not
  source. Canonical patch SHA matches source.json. Reused the 2,711 full-run macOS passes for
  unchanged code; explicit Linux 17/17 supplements the platform-specific skips. Completion text
  accurately describes this mixed-environment coverage.

Native helper correction and contracts:

- Traced new `finish_model` caller, existing Driver `drive/poll/reached`, all five parent calls,
  known-document/model ordering, failure diagnostics and the document workflow graph. `edit` is
  the sole `agent.run`; its downstream steps are data checks, MCP readback and notification.
  Each model closes only after that company's persisted `edit.completed_at` appears in the
  existing run view, proving its final response was consumed. Scope/model order follows the same
  document vector; length assertion guards alignment. Every model must be consumed on success.
- Secret checks and exact five-exchange assertion are preserved in `finish_model`. Remaining
  models are retained for failed-run diagnostics. The two worker groups, positive poll delay,
  wake conditions, claim path, graph deadline, 60-second model and 80-second native helper limits
  remain unchanged. No runtime/schema/production concurrency contract changed; no architecture
  gate needed for this cfg(test)-only lifecycle correction.
- Follow-up to R3: its earlier whole-document teardown review did not establish enough model
  lifetime during slow downstream work. Retained R4 aggregate failure reached terminal at
  76,272ms after model start at 16,527ms; later teardown exceeded the model lifetime. New closure
  uses each persisted editor completion rather than whole-workflow terminal. A later concurrent
  focused attempt closed both models correctly but hit the independent outer deadline; its
  failure remains retained. Serialized focused S3 and the final aggregate both exercise the
  unchanged limits successfully. No timeout or assertion was relaxed.

Checks and integration identity:

- Inspected successful current formatting, two focused Rust receipt/parser units, locked offline
  all-target compilation/clippy and unchanged three exact helper test names. The ignored helper
  is behaviorally exercised by actual private profiles, not inferred from the two unit passes.
  Canonical managed formatting and 46 runner protocols pass; the earlier process-identity race
  is retained, with isolated and complete rerun successes and unchanged runner source.
- Independently hashed current immutable helper
  `1d94d46c28196318f202a1d0466d3aa917edc82e240068b4afb8ff950d31bda6` and both changed Rust sources.
  Final official aggregate references current source
  `351a2180b4ca8cf7489d21cbdc3aa3ce7d275fc5b08ba56eb045592a43e34e1f` and image
  `sha256:a50708c0d9a1a849d1453c586fba3242e8ae340091087303fea8e13896e321f1` consistently.
  All four report/cleanup hashes and successful assertion counts match: 42/43/9/10, total 104.
  This refreshes production-path evidence after package/helper identity changes. The real
  production image and six memory tests are unaffected and retain R3's reviewed evidence.
- Independent staged/unstaged whitespace, exact R4 skill-evidence and 49 tracked-manifest
  validation pass. Read-only owned-Docker inspection confirms no remaining containers.
  Native cluster cleanup is retained. No immutable helper/test was rerun in place, no shared
  resource modified and no source change made by reviewer.
- Supporting independent receipts: [B1-R4-reviewer-identities.txt](logs/B1-R4-reviewer-identities.txt),
  [B1-R4-reviewer-validation.txt](logs/B1-R4-reviewer-validation.txt). Retention validation is not
  used as a substitute for the actual fixture/assertion review above.

**PASS for artifact `b715976039bbe891c5771d5e7946621afe528baf95b4d57bfd6fd41f59b89e35`
and its verified affected integration scope.** F3 is fixed; F1/F2 remain fixed; no new actionable
finding or required unexercised criterion identified. Root retains final acceptance/commit authority.
No wholly green-suite claim: independently attributed full/stock failures remain, along with
inherited p03 inspection-only/stand-in coverage and user-deferred C004 dispatch. Paid live inference
and human visual review are still outside the declared automated CI gate.

Batch size/reuse were appropriate. R3's platform-skip lesson is reinforced; the additional
lifecycle evidence supports closing test resources at their actual last consumed operation,
without increasing test bounds. No instruction/skill edits made.
