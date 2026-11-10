# p05 agent review notes

## MEM-P05-B2-R1 — batch review of p05-B2 (IN PROGRESS; working notes, final outcome appended below)

- Date 2026-11-08; workflow `implement-opus-reviewed-2_2`; stage: batch review; batch p05-B2.
- Reviewer B: claude-opus-5-5, effort xhigh (requested); phase `p05`; worktree
  /Users/mac03/.codex/worktrees/60a0/openbot (detached HEAD a6bed48a89d986204d7df13d4b4c5a58458176cf).
- Records: TASK /Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p05/docs/TASK.md,
  REVIEW /Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p05/docs/REVIEW.md,
  findings /Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p05/docs/agent-review-findings.md.
- Review base: accepted P7b tree 113801e442204b9277285d0be088f0492757d7a6; phase start a6bed48a; HEAD a6bed48a.
- Artifact digest d20858a7e356ccd1cce9dbad6bff214a2bdf37dd60dd3ad00c7e8fab10c2f314 (exclusions: tmp,
  plan/memory-service/phases/p05/docs, plan/memory-service/COORDINATION.md) — recomputed at start: MATCH.
  Non-record tree recomputed = acbda41e minus records (only record files differ). Frozen diff
  tmp/memory-service-run/frozen/p05-b2-incremental-from-p7b.diff sha256 c8def7fb…0398: MATCH.
  `services/memory/` diff 113801e4 → acbda41e: 0 lines (unchanged, confirmed).
- Coordination: plan/memory-service/COORDINATION.md revision 1; I001/r1, I002/r1, I003/r1; no cross-phase notices.
- Gated decisions: phase-05 E1–E12 (gate PASS r4, record tree 6238f2bc) + U2 addendum + P7b notes + P7c notes.
- Evidence manifest logs/MEM-P05-B2-R1.evidence.json: check-evidence.py valid (9 PASS / 2 FAIL / 2 NOT EXERCISED).

### Working candidate list (in progress; not final)
- C1 memory.save NOT added to `marker_owned_effect_kind` (E4 says it joins); test
  `workflow_action_marker_owned_kinds_are_exactly_the_action_service_owners` still asserts the opposite and
  memory.save stays in the `unknown` retry set → a lease expiry/crash during a claimed memory.save attempt
  yields `EffectOutcomeUnknown` → `RecoveryDecision::Reconcile` (recovery.rs:184). Undeclared deviation.
- Checked OK so far: single committed_output writer (batch_commit.rs:76); 6 callers of `complete` + commit_wake
  (wait_commit advance, child_waits finish) all pass CompletedStep; single INSERT INTO messages
  (thread/message.rs:810); InvocationResult producers: harness rig workflow.rs:201 (unknown tool), :218
  (dispatch), knowledge_checkpoint.rs:119 — all through `checkpoint_updates::update_on`. Approval rejection
  routes a fixed-code failure (decision_approvals.rs:179, no human text). Child absorb in child_waits and
  workflow_tool_calls::call_result (continue_call wake tx commits the lowering before replay).
- C2 `wait_commit::memory_effect_failed` (terminal intent → step failure: disabled/expired/cancelled/owner_removed)
  and the polling.rs memory arm have NO test (grep: no test reaches MemoryResume::Failed; round trip only covers
  Delivered via direct resume_wait). E2/E9 require waits to wake with a terminal failure. → probe planned.
- C3 E6 race test uses stand-ins on both sides (raw raise_content_level_on tx vs enqueue_save_on in a bare tx),
  not memory_steps::observe_on/park_on; mutation A is "freeze without run lock" (not E6's "claim-time
  RunKnowledge"); mutation patches not retained. Undeclared stand-in limits.
- C4 delivery worker is single-flight FIFO (one claim per pass, sequential): MemoryPool::Delivery global/per-company
  permits never contended; one tenant's burst delays all tenants (src/application/AGENTS.md fairness rule).
- C5 claim query OR predicate (pending∧due ∨ claimed∧lease-expired) not served by the partial pending index;
  memory_deliveries has no retention → seq scan per pass grows with settled rows; run-cancel trigger scans the
  company's deliveries (no run_id index).
- Noted OK: freeze order run→tenant FOR SHARE→namespace FOR SHARE→insert; disable FOR UPDATE→fail_disabled_on
  (new snapshot sees committed intent); owner removal after namespace UPDATE; consumed_for = exact only when
  content_exact ∧ scope≠company (U2 addendum); producing_ceiling = admitted knowledge_ceiling.
- C6 `memory_access_level_history_read_lowers_for_conversational_or_inexact_messages`: the snapshot always holds the
  human trigger (Conversation fixture inserts role 'human'; existing test asserts "triggering message included"),
  so `expected` is false in all 3 cases → stored agent bit (exact vs inexact) not discriminated; "exact agent
  messages keep it" never asserted. E11 "reading an inexact run's message through context.history" unproven.
- C7 channel-reply test: review gate is `decision.human` with non-empty feedback/data, so hook 1 lowers in both
  choices; complete_saved_on's reviewed_output_exact lowering (B2a-5) is not discriminated by the cited test.
- Hook tests verified discriminating: admission (5 sources), child copy+absorb, decision feedback, event wake,
  pure batch keep, http lowers (complete_io), unknown-tool lowers, approved HTTP tool result lowers, knowledge keeps.
- C8 claim_delivery: `open_request(&row)?` propagates an envelope-open error out of the claim tx (rollback) →
  row stays pending/due and first in ORDER BY → re-picked every pass (5 s backoff) → head-of-line poison for all
  companies (single-flight worker). AGENTS: poison rows must not monopolize the queue.
- C9 settled memory_deliveries keep the full frozen request (facts text, ≤3.5 MiB) forever; no run FK so run
  retention (storage_retention.rs, 90 d) doesn't cascade; no retention; 05A.6 "corpus stays in the service DB";
  not recorded as a 06A carry. Combine with C5.
- C10 editor `step_catalogue` (services.rs:93-104) is a binary-static LazyLock over UNHANDLED_STEP_KINDS: memory
  kinds never listed even when configured; UI test only covers unconfigured (declared E10 split gap).
- C11 missing step-level E3/05A cases: no test for memory.load in a thread run (readers 6/8/9 → ceiling 6),
  destination below content refusal (drop to 3) before any request, missing ceiling; no competing test for owner
  removal vs freeze (B2b-5 claim) or disable/cancel vs an in-flight claimed delivery (completion Lost).
- Accepted after inspection: no-heartbeat deviation (permit+HTTP share request_timeout ≤30 s < 45 s < 60 s lease;
  late duplicate is an idempotent replay); no release `memory` arm (no ActionService path for steps; B3 must recheck
  for tools); mark_exact replica fixture (precedent child_output_tests; superuser); no run FK (but see C9).
- Optional: observe_on raises to service `exposed_level` only; raise max(exposed, item levels) as defense in depth.
- Full lib suite started in background → logs/MEM-P05-B2-R1-B-lib-full.txt (own DB open_bots_b2rev_test).
- Probe drafted (scratchpad probe_tests.rs): terminal intent (disabled/expired/refused) → poll → resume → retry.
- Full suite in progress: 4 FAILED so far — rendered_markup_carries_no_inline_javascript,
  security_auth_limits_unicode_registration_aliases_share_budget_and_code_digest,
  security_auth_limits_login_counter_is_shared_and_success_resets_it (relationship TBD vs base),
  memory_integration_delivery_fences_competing_claimants_and_bounds_attempts (IN SCOPE — diagnose).
- Findings file drafted (F2–F12); F1 reserved for the terminal-intent wake gap pending the probe.

### MEM-P05-B2-R1 — final outcome (2026-11-08 ~23:40Z)

- **Outcome: CHANGES REQUIRED** for digest d20858a7e356ccd1cce9dbad6bff214a2bdf37dd60dd3ad00c7e8fab10c2f314.
  Actionable (all Minor): MEM-P05-B2-F1, F2, F3, F4, F5, F6, F7, F8, F9, F13. Optional: F10, F11, F12. No Major.
  Details/dispositions: agent-review-findings.md (link by ID; not duplicated here).
- **Freshness (return-time recheck): MISMATCH.** Current snapshot digest is
  f808b0a4c4f765be54ad05e5e1702cd9d14ac21b34cba6b81ad4da5cab2182f4 (HEAD still a6bed48a; frozen diff sha still
  c8def7fb…). The only non-record changes vs tree acbda41e: new untracked `plan/agent-workflow-call/` (README.md +
  01–06, mtimes 23:22–23:23) and `src/main.rs` (+7 `NO_BACKWARD_COMPATIBILITY_NOTES` entries for those files,
  23:22:58). Not written by this reviewer (graft/ is gitignored, so graft refreshes do not affect the digest). They
  are unrelated to the memory batch's behavior, and every build/probe of mine predates them (the lib test binary was built for the `--list` run
  before the full suite started; the probe copy was taken before 23:22; `--lib` reruns do not compile main.rs). The findings stand against
  d20858a7; root must re-freeze before the correction recheck and decide how this sibling write into a frozen
  phase worktree is handled (skill: freeze the whole implementation worktree during review).
- **Coverage (inspected):** full batch diff 113801e4→acbda41e (90 files) read: schema (content_exact columns/guards,
  memory_parked guard branch, effect-wait guard, memory_deliveries + CHECKs + indexes + run-cancel trigger,
  workflow_memory_consumptions); domain `human_input_exactness`; classifier `exactness.rs`; hook 1 (batch_commit
  + all 6 callers + commit_wake via advance/child_waits::finish); hook 2 (checkpoint_updates + all 3
  InvocationResult producers); admission by source; message stamping (single INSERT); child copy/absorb
  (children, child_waits, workflow_tool_calls); context.history + history freeze + exchange updates and triggers;
  channel_reply_resume lowering; memory steps (authorize/observe/park, lock order, read ceiling, output budget,
  failure mapping); deliveries (freeze, U2 label, replay, claim/complete/fencing/backoff/expiry/epoch,
  disable/owner/cancel settlement); worker; adapter save/search/fetch + contract fake; Delivery pool;
  RESERVED_DATABASE_CONNECTIONS + worker_slots test + docs; publication gate + agent-scope resolution; registry
  schemas/help/examples; samples + boundary script; schema allowlist test; docs.
- **Model-visible path audit (fail-closed):** step outputs (single committed_output writer), tool results (single
  funnel), run input (4 trigger sources), history (exchanges via freeze; messages via context.history), child
  results (both absorb sites, wake tx commits before replay), approvals (rejection is a fixed-code failure; approved
  result re-enters hook 2), recovery (null output keyed by failing kind), channel replies (decision.human gate +
  complete_saved_on), attachments (ride on role-stamped messages). No bypass found. U2 refinements implemented as
  gated (ConversationalInput inexact for every target; AuthoredConfiguration exact for Run/Agent/User, inexact for
  CompanyScope → company saves always `consumed: unknown`; `producing_ceiling` = admitted knowledge_ceiling).
- **E6:** lock order run (runtime::facts FOR UPDATE OF run) → fence → tenant FOR SHARE → namespace FOR SHARE → intent
  matches E6; raise commits before output (mutation B evidence is meaningful); race test stand-ins → F4.
- **Delivery vs D5:** retry resends the stored body (key, expires_at, occurred_at frozen); D5 concurrent identical
  saves replay rather than 409; saves only, never generation. Deviations judged: no heartbeat — accepted (permit +
  HTTP share request_timeout ≤ 30 s < 45 s deadline < 60 s lease; a late duplicate is an idempotent replay); no
  release `memory` arm — accepted for B2 (steps never reach ActionService; B3 must re-check for tools); no run FK —
  accepted but see F8; `mark_exact` replica fixture — accepted (test-only, precedent; needs a superuser test role).
  Declared gaps: recovery caller with accounted kind only — accepted (shared writer + classifier test);
  workflow-tool child path — folded into F5; E9 capture half — 05B; authoring split — F9.
- **RESERVED_DATABASE_CONNECTIONS 13→14:** a reservation increase (tightens, not loosens); the early-failing guard
  remains `worker_slots` startup refusal with its test updated (worker_slots_respect_the_pool PASS) and both docs;
  the delivery worker holds one connection at a time (claim tx, then pool completion). Compliant with AGENTS.md.
- **Checks executed by B (fixture: private cluster 127.0.0.1:55432, own DB open_bots_b2rev_test, dropped; tests'
  own_database DBs auto-dropped):**
  - `cargo test --locked --offline --lib` (full lib, 3632 listed): 3618 passed, 4 failed, 10 ignored —
    logs/MEM-P05-B2-R1-B-lib-full.txt. Isolated rerun of the 4 — logs/MEM-P05-B2-R1-B-failures-isolated.txt.
    - In scope: `memory_integration_delivery_fences_competing_claimants_and_bounds_attempts` failed under load at
      delivery_tests.rs:356, passed alone → F13 (order-assumption flake).
    - Confirmed unrelated, left unchanged: `rendered_markup_carries_no_inline_javascript` (deterministic source scan;
      offender src/adapters/http/pages/workflow_authoring/sample.rs:153, a file unchanged vs a6bed48a and 113801e4);
      `security_auth_limits_unicode_registration_aliases_share_budget_and_code_digest` and
      `security_auth_limits_login_counter_is_shared_and_success_resets_it` (code and tables untouched by the batch;
      fail alone; the private cluster is `--locale=C`: `lower('ÄLICE')` = 'Älice', so citext Unicode folding fails —
      environmental).
  - All 77 `::memory_*` tests except F13's passed in the full run; named regressions passed (knowledge_run_level_http…,
    workflow_action_receipt_actual_result…, channel_reply_authoring_native_real_review…, workflow_agent_run_protected…,
    workflow_agent_run_refuses_model_invented…, knowledge_search_driver…, production_publication_refuses_memory…,
    production_handler_covers_every_io_kind…, the_memory_provider_schema_is_gone, worker_slots_respect_the_pool).
  - `cargo test --locked --offline --test source_modules`: 8/8 — logs/MEM-P05-B2-R1-B-source-modules.txt (ran at
    23:28, i.e. on the tree that already had the sibling main.rs/plan edits; they add no Rust module).
  - `cargo fmt --all -- --check`, `git diff --check`, `scripts/workflow-sample-boundary.py`: pass — logs/…-B-fmt.txt,
    …-B-diff-check.txt, …-B-sample-boundary.txt.
  - Probe (isolated copy under tmp/memory-service-run/review-b-probe with its own CARGO_TARGET_DIR, debuginfo off;
    deleted): terminal-intent wake for disabled/expired/refused works — logs/MEM-P05-B2-R1-B-probe-terminal-wake.{rs.txt,txt}
    (F1 is therefore a test gap, not a behavior defect).
  - Inspected, not re-run: author's clippy, sqlx prepare check, E6 mutation runs (patches not retained → F4).
  - Not exercised (by anyone): live-service tests, stack budget, Docker/CI, browser check (05 gate / 06C scope).
- **Evidence manifest:** logs/MEM-P05-B2-R1.evidence.json validates (9 PASS / 2 FAIL / 2 NOT EXERCISED). Its FAIL
  entries are the expected mutation failures and the pre-fix regression run; the post-fix correction re-ran only a
  subset — superseded by B's full run above. Mapping mismatches recorded as F1, F3, F4, F5, F10.
- **Reviewed lightly:** contract-fake internals beyond save/search/fetch; docs prose beyond the memory sections;
  registry help strings (checked for the claims they make, not wording).
- **B3 (agent tools) design notes, not defects of this batch:** (1) `freeze_save_on` is pub(crate) and trusts a
  caller-supplied `LockedRun` — the tool path must read levels from the locked run row (E6 mutation A belongs there);
  (2) with several open intents per run (tool saves don't park), disable (`fail_disabled_on`) and run-cancel
  (trigger) update overlapping intent rows in unspecified order → possible deadlock; order both by id or lock the
  tenant first; (3) release `memory` arm must be re-justified if tools route through ActionService; (4) F12
  (`sources_sha256`) before tool consumption rows; (5) tool-level E6 race on production entry points.
- **Process and instruction feedback:** batch size was large but reviewable in one pass (~4.9 k lines); the gate
  covered the right decisions. Missing in the author's records: the E4 marker-owned step (F2) and stand-in
  declarations (F4); TASK claimed an open B2b gap would close in B2c without a test (F1). Useful: the explicit
  "Unproved by stand-ins" lines and the frozen incremental diff. Candidate instructions: (a) "before review, grep
  every gated decision for named predicates/tests to extend and list each in the contract-impact table" (scope:
  author, all phases; evidence F2); (b) "a computed `expected` in a table-driven test must be shown to take each
  value across cases" (scope: verification-contracts; evidence F3); (c) "retain mutation patches with mutation
  evidence" (scope: verification-contracts; evidence F4); (d) "a shared worktree under review must not receive
  sibling writes; concurrent plan work uses its own worktree" (scope: root/concurrency; evidence: digest mismatch
  above). No change to gated decisions is supported by the evidence; F2 is an implementation divergence from E4.

## MEM-P05-B2-R2 — correction recheck of p05-B2 by finding ID (IN PROGRESS; outcome appended below)

- Date 2026-11-09; workflow `implement-opus-reviewed-2_2`; stage correction recheck; batch p05-B2; reviewer
  claude-opus-5-5 / xhigh; phase p05; worktree /Users/mac03/.codex/worktrees/60a0/openbot (HEAD a6bed48a).
- Artifact digest a369b44e6526a7e542e81d78108877c0874d3308185c17d9e3ffb62cff2cfc75 recomputed at start: MATCH
  (exclusions tmp, plan/memory-service/phases/p05/docs, plan/memory-service/COORDINATION.md). Non-record tree
  equals 8e4baa51 apart from record files. Delta file p05-b2-r2-incremental-from-r1.diff sha256 6c45ee51…710f MATCH.
  `services/memory/` diff 113801e4→8e4baa51: 0 lines. Coordination r1 unchanged (I001–I003, no notices).
- Sibling (non-p05) content inside the digest: plan/agent-workflow-call/ (7 files), plan/agent-default-channel/
  (4 files: README, 01, 02, 03; mtimes 2026-11-09 00:15–08:49) and 11 NO_BACKWARD_COMPATIBILITY_NOTES entries in
  src/main.rs (mtime 09:00:10). The coordinator's brief names only the 7 agent-workflow-call entries — the
  agent-default-channel additions are a second sibling plan. All documentation/list-only; ignored for review.
- Delta read (36 files): schema (nullable request + open⇔body CHECK, 3 partial indexes, id-ordered trigger),
  failure.rs (memory.save marker-owned; tests updated), deliveries.rs (id-ordered settle CTE, request cleared on
  every terminal write, in-flight company exclusion, credential_unavailable settle), delivery.rs (lanes),
  infra (lanes = background_in_flight; two static catalogues by refused_kinds), memory_steps.rs (F11 carried max,
  F12 key rename), tests (F1, F2, F3 rewrite, F4 production park race, F5 ×6, F6, F7 ×2, F9 ×2, F13, ordering ×2),
  retargeted base recovery tests + unresolved_marker helper.
- Restore check: every retained mutation patch (A–F) `git apply --check`s cleanly against the current tree
  (deliveries.rs B/E/F, failure.rs D, memory_steps.rs A, knowledge.rs C) → restored files carry the code the
  mutations ran against. failure.rs R1→R2 diff contains only the F2 lines; deliveries.rs diff only the F6/F7/F8/
  ordering hunks (freeze_save_on untouched). Nothing lost found.
- Test inventory: 3632 (R1) → 3647 listed; `comm` shows no removed name.
- Candidate new: lanes (up to MEMORY_SERVICE_BACKGROUND_IN_FLIGHT=16) each take a pool connection for claim and
  completion, but RESERVED_DATABASE_CONNECTIONS still counts the delivery worker as 1 and docs/deploy.md:765 says
  "holds one database connection" (not updated in R2) → F14.
- Optional: invariant docs (worker.rs:23-25, failure.rs header, infra/workflow.rs ProductionHandler comment) still
  say marker-owned kinds act only through ActionService → F15.
- Retargeted tests: assertions unchanged (diffed); they now reach "unknown" via the SQL marker predicate instead of
  the kind default; the kind-default branch is unreachable for every current I/O kind (unit-tested predicate only).
  Accepted.
- Full lib suite (R2) running in background → logs/MEM-P05-B2-R2-B-lib-full.txt (DB open_bots_b2rev2_test).

### MEM-P05-B2-R2 — outcome (2026-11-09)

- **Outcome: CHANGES REQUIRED (one new Minor).** F1–F9 and F13 FIXED; optional F11, F12 FIXED (taken), F10
  DEFERRED by the author (accepted, optional). New: **MEM-P05-B2-F14** (Minor, actionable — delivery lanes hold up
  to `MEMORY_SERVICE_BACKGROUND_IN_FLIGHT` pool connections while `RESERVED_DATABASE_CONNECTIONS`/`worker_slots`
  and docs/deploy.md:765 still count one; introduced by the F7 fix) and **MEM-P05-B2-F15** (optional — invariant
  docs for marker-owned kinds still say "only through ActionService"). Per-finding resolutions are appended in
  agent-review-findings.md.
- B3 ordering note from R1: RESOLVED — `settle_open_on` and the trigger lock in id order; the forced overlap test
  deadlocks under mutation F (DESC) and passes in id order (also under B's full-suite load).
- Root's retargeting of the three base recovery tests: accepted. Their assertions are byte-identical; they now
  reach "effect unknown" through an unresolved ActionService marker (SQL predicate) instead of the kind default,
  which no current claimed I/O kind can reach any more (it stays unit-tested in failure.rs). They passed in B's run.
- Restored files: no loss found (diffs limited to intended hunks; all six retained mutation patches apply cleanly).
- **Checks executed by B** (private cluster 127.0.0.1:55432; own DB open_bots_b2rev2_test, dropped; own_database
  DBs auto-dropped, 0 left):
  - `cargo test --locked --offline --lib`: 3647 listed (no name removed vs R1's 3632), 3634 passed, 3 failed,
    10 ignored — logs/MEM-P05-B2-R2-B-lib-full.txt. The 3 failures are the R1-confirmed unrelated ones
    (`rendered_markup_carries_no_inline_javascript`, two `security_auth_limits_*` on the C-locale cluster). All 91
    `::memory_*` tests, the three retargeted tests and every R2 test named in TASK passed, including the
    load-sensitive fences, fairness, lanes and ordering tests.
  - `--test source_modules` 8/8; `cargo fmt --check`, `git diff --check`, sample-boundary script: pass —
    logs/MEM-P05-B2-R2-B-{source-modules,fmt,diff-check,sample-boundary}.txt.
  - Inspected, not re-run: author's clippy (pass, twice), sqlx prepare check, mutation runs A–F (retained logs show
    each discriminating test failing for the expected reason; F's barrier test surviving is explained and covered by
    the forced test).
  - Evidence manifest logs/MEM-P05-B2-R2.evidence.json validates against a369b44e… (14 PASS / 4 FAIL / 2 NOT
    EXERCISED); its FAILs are the caught mutations and the superseded pre-retarget regression run.
  - Not exercised (05 gate / 06C): live-service tests, stack budget, Docker/CI, browser check.
- Process: correction delta was well organized (per-finding table, stand-ins, retained patches). The disk-full
  incident from an in-place mutation run is the evidence for the coordinator's isolation rule. Candidate
  instruction (scope: author, configurable worker concurrency): derive the connection reservation and its startup
  check from the same setting (F14).
- Return-time freshness (2026-11-09): digest a369b44e…fc75 MATCH, HEAD a6bed48a, COORDINATION r1 with no notices,
  manifest still validates. Sibling docs unchanged since start of R2.

## MEM-P05-B2-R3 — correction recheck limited to F14, F15 and the F11 nit (IN PROGRESS; outcome below)

- 2026-11-09; reviewer claude-opus-5-5 / xhigh; digest 9c7929bf1e3b9fce84664264e0530133669cf078bc77ff45a8ad587dafd51abc
  recomputed at start: MATCH (same exclusions); HEAD a6bed48a; non-record tree = dddff33c (only the R3 manifest
  differs); delta p05-b2-r3-incremental-from-r2.diff sha256 b903c8cf…ff18 MATCH; COORDINATION r1, no notices.
- Sibling content NOT previously noted (coordinator said none): new untracked plan/departments/ (README + 01–05)
  and 6 matching NO_BACKWARD_COMPATIBILITY_NOTES entries in src/main.rs. Docs/list only; ignored for review.
- Delta read (code): delivery.rs (store Mutex around claim and completion), application/workflow/memory.rs
  (`carried_level`, used for output), memory_steps.rs (observe uses `carried_level`), failure.rs / worker.rs /
  infra/workflow.rs (F15 + reservation comments), docs/deploy.md:765, tests (Counted store peak = 1; UnderReported
  client in the round trip; F11 unit test). Mutation G patch + failing run retained in logs/ (peak 2 ≠ 1).
- Mutex analysis (coordinator's questions): single lock, never nested, never held across the send → no deadlock;
  FIFO fairness → no lane starvation and a queued completion beats a later same-process reclaim; no heartbeat exists
  to starve; a completion delayed past the 60 s lease is applied (fence unchanged) or `Lost` to another instance's
  reclaim (idempotent resend), so fencing holds; the 45 s deadline bounds only the send and a lock wait before claim
  risks nothing; the claim's lock waits are bounded (`lock_timeout 2s`). Residual: a completion has no lock_timeout,
  so a long-held intent row lock (disable/owner removal/run cancel transaction) now pauses all lanes → optional F16.

### MEM-P05-B2-R3 — outcome (2026-11-09)

- **PASS** for digest 9c7929bf1e3b9fce84664264e0530133669cf078bc77ff45a8ad587dafd51abc, covering the whole batch
  p05-B2 across R1–R3. F14 and F15 FIXED; the F11 nit FIXED. All actionable findings F1–F9, F13, F14 are resolved;
  optional F10 deferred (accepted), F11/F12/F15 taken; new F16 optional only. No open acceptance-blocking defect.
- Coverage and evidence relied on: R1 full-batch review and probe; R2 full lib run on tree 8e4baa51 (3634/3647,
  3 confirmed-unrelated failures) for everything outside the R3 delta; R3 delta read in full; B's R3 checks on tree
  dddff33c (own DB open_bots_b2rev3_test, dropped): focused `memory_ persistence::memory:: actions::failure::
  infra::workflow:: application::workflow::memory workflow::sample:: registry:: the_catalogue_ credential_rotation`
  210/210 (logs/MEM-P05-B2-R3-B-focused.txt); delivery tests ×10, 12/12 each (logs/MEM-P05-B2-R3-B-lanes-repeat.txt);
  clippy -D warnings, fmt, diff-check pass (logs/MEM-P05-B2-R3-B-{clippy,fmt,diff-check}.txt). Manifest
  logs/MEM-P05-B2-R3.evidence.json validates (5 PASS / 1 FAIL = caught mutation G / 1 NOT EXERCISED = wider
  regression and sqlx prepare: accepted, the delta has no query macro, schema or shared-path change and its
  surfaces are all in the focused selection).
- Still not exercised (05 gate / 06C, recorded since R1): live-service tests, stack budget, Docker/CI, browser.
- Return-time freshness: see the line below.
- Return-time freshness (2026-11-09): digest 9c7929bf…1abc MATCH, HEAD a6bed48a, COORDINATION r1, no notices.

## MEM-P05-B3-R1 — batch review of p05-B3, the four agent memory tools: CHANGES REQUIRED

- 2026-11-09; workflow `implement-opus-reviewed-2_2`; stage batch review; reviewer claude-opus-5-5 / xhigh (B
  abdd866fb428eea05); phase p05; worktree /Users/mac03/.codex/worktrees/60a0/openbot (HEAD a6bed48a).
- Artifact digest 0d0b4622ed5d78726877bfccc1604450e4a67eff42ddff189957a58090336c3d recomputed at start: MATCH
  (exclusions tmp, plan/memory-service/phases/p05/docs, plan/memory-service/COORDINATION.md). Current tree =
  freeze tree dfeeca67 except record files. Review base: B2 accepted tree dddff33c (PASS MEM-P05-B2-R3).
  Delta `git diff dddff33c dfeeca67 -- src migrations`: 52 files +3459/−82; services/memory unchanged (0 lines);
  outside src/migrations only sibling plans (plan/01-departments, 02-workflow-roles, 03-principal-addresses,
  04-root-unit, agent-workflow-call; plan/departments renamed) and their main.rs entries — ignored.
- Coordination r1 (I001–I003), no notices. No docs/ or decision-record changes in this batch.
- Working notes (saved during review; the outcome below supersedes the candidate list):
  - Read in full: memory_tool_checkpoint.rs, memory_tools/{builtin,ports,runtime}.rs, memory_tool_tests.rs, both
    mutation patches and logs; diffs of authority.rs, resources.rs, memory_steps.rs (namespace_on/read_ceiling_for
    shared), deliveries.rs (SaveScope::User), memory/mod.rs (memory_enabled_on), admission_binding.rs (is_manual),
    catalog.rs/reads.rs, agent_publication/tools.rs, workflow_agent/mod.rs, mcp/policy.rs, workflow_http, setup
    wiring (clients.rs, mail.rs, infra/workflow.rs), memory_service/mod.rs (timeline), ports.rs, fixtures.
  - Checked and holding: E6 save order (load_call prelock → lock_fence/run row → eligible_on → recalled_on →
    enqueue_save_on tenant/namespace FOR SHARE → invocation result → live, one tx); reads commit result + raise +
    consumption + live in one tx before return; replay refuses an exposure above the current ceiling; refusal
    records only the error; key ob.t.<invocation simple>; save sources from all attempts of the execution;
    company save refused at parse and in save_on; MCP/HTTP origin reservation before exceptions, shared MCP client
    reaches the workflow MCP runtime, no redirects; tools dispatch outside ActionService (no release arm needed);
    B2 design notes (1) freeze from the locked row (mutation A), (2) settled in B2 R1, (4) F12 settled.
  - Candidates: (a) a search over several scopes takes one interactive permit per scope concurrently (E1: a tool
    call is one interactive operation; per-company share 2); (b) E11 user-scope refusal cases (thread run's child,
    attached session, foreign-authored manual workflow) not exercised / not addressed by D4; (c) catalog and new
    publication not gated on "configured" (E1/E5), D10 records only the already-published edge; (d) mutation logs'
    line numbers predate the frozen test file (manifest says pre-fmt copy) — re-run myself; (e) workflow-bundle
    Publish gate (authority::builtin_memory_tool) untested.
- Coverage (whole batch): requirements taken from 05A.2–.5 (scopes, tools, live checks, atomic accounting), the 01
  access-level contract, E1 (operation accounting, URL-unset gating), E2 (+ architecture review M2 in
  tmp/memory-service-run/reviews/p7a-arch-r1.md and the E11 `memory_scope_`/`memory_integration_` cases), E3, E5, E6
  (lock order, protocol, competing test, mutations A/B), the U2 company-scope addendum (tool saves are agent/user only,
  so they follow `content_exact`), 06-D5 (frozen at publication, `memory.not_enabled` at runtime), and my B2-round B3
  notes (1)–(5). Files: everything in `git diff dddff33c dfeeca67 -- src migrations` (52 files), plus
  agent_history_sessions::capture, manual::start, lock_owner/authorize_on, deliveries::enqueue_save_on, the
  memory_service limits, and the knowledge checkpoint tests the tools copy.
- Reviewer evidence (all on the frozen tree / unchanged digest):
  - Focused selection, the author's filter, fresh own DB `open_bots_b3_test` (private cluster 127.0.0.1:55432,
    DATABASE_URL unset, init schema migrated by test_support): 426 listed — identical to the author's list — 425 passed,
    0 failed, 1 ignored (logs/MEM-P05-B3-R1-B-list.txt, -B-focused.txt). No test removed by the B3 delta (no removed
    test attributes; removed `fn` lines are signature changes). `cargo fmt --check` clean (-B-fmt-check.txt);
    `git diff --check dddff33c dfeeca67 -- src migrations` clean. Clippy and sqlx prepare not re-run (author's evidence;
    no query macro changed).
  - Manifest: `check-evidence.py … --expect-artifact 0d0b4622…6c3d` → valid {PASS 7, FAIL 0, NOT EXERCISED 1}.
  - Mutations A and B re-run by me on the frozen tree (git archive dfeeca67 into tmp/memory-service-run/mut-b3-rev,
    own CARGO_TARGET_DIR, 30 GiB free, deleted after): A fails the race test (memory_tool_tests.rs:768, exact
    rise-first freezes 4, want 6); B fails the read test (:827 "no result before the observation commits") and the e2e
    (:313, level 0 want 5); each leaves the other test green (-B-mutation-A.txt, -B-mutation-B.txt). The author's logs
    cite lines 720/779/291 because their copy predates `cargo fmt` (as the manifest says); same failures.
  - Mutation C (mine; same isolation, -B-mutation-C-invocation-binding.patch.txt): `require_memory_invocation` without
    the argument binding and the trusted-snapshot check — all 116 `memory_` tests pass (-B-mutation-C.txt) → F2.
  - Disk: 30 GiB free before and after each copy; tmp/memory-service-run/mut-b3-rev and mut-b3-rev-c deleted;
    the author's tmp/memory-service-run/mut-r2 (patches only) left alone. Own DB open_bots_b3_test dropped at the end.
- Checked and holding: E5 catalogue/publication/dispatch wiring (enabled-gated; frozen at publication; `with_memory`
  composed only with the service; `Box::pin` at the dispatch seam); trusted exact snapshots and reserved names
  (schema CHECKs, `is_reserved_tool_name`; an HTTP op named like a memory tool is classified Memory and then
  refused as untrusted — fails closed); inputs carry no authority fields and parse strictly; E3 ceiling with
  replay refusal after a grant drop; E6 read = result + raise + consumption + live in one tx before return (mutation
  B), save = prelock → lock_fence (run row) → eligibility → recalled sources → tenant/namespace FOR SHARE → intent →
  invocation receipt → live in one tx with levels read under the lock (mutation A); key `ob.t.<invocation simple>`;
  sources distinct, level desc then id, ≤ 64, all attempts of the execution; company saves refused (U2 addendum not
  reachable from tools); 06-D5 runtime revocation (`memory.not_enabled`, replayed); D8 reservation before operator
  exceptions on MCP (writes and calls, shared client reaches the workflow MCP runtime) and workflow HTTP, no redirects;
  timeline/search adapter refuses any answer above the ceiling; details order, no existence oracle, omitted count;
  service refusal mapping; `TooLarge` unreachable from tools (8 × 4,096 chars ≪ 3.5 MiB). B2-round B3 notes: (1) the
  tool freeze reads the locked row — holds (mutation A); (2) settled in B2; (3) no release `memory` arm needed — tools
  dispatch through `MemoryToolDispatch`, never ActionService; (4) settled by F12; (5) tool-level race on production
  entry points — the save side is production `save_memory`, the read side's commit-before-exposure is production
  `call_memory_tool`; the competing rise is a raw `raise_content_level_on` transaction (declared stand-in, B2
  precedent) — accepted.
- Accepted as declared: schedule-root stand-in (admission row replaced); contract fake only; setup wiring untested
  (composition, small); D9 deferrals; D8's unvalidated HTTP connection writes (refused at call; defense in depth);
  transient service errors recorded as the call's final tool error (the model may call again).
- Outcome: **CHANGES REQUIRED** — F1 Major (user scope reachable from a foreign-authored manual workflow; E11/M2 cases
  missing), F2–F4 Minor (adversarial tool tests; E1 operation accounting; configured gating), F5 optional.
- Not run by me: full lib suite and stack budget (05 gate / 06C), clippy and sqlx prepare (author's evidence; no
  macro query changed), live service, browser, Docker/CI. No unrelated failures in the focused selection.
- Process and instruction feedback: the TASK mapping and stand-in list were accurate and made the review fast; the
  retained mutation patches applied cleanly to the frozen tree. Missed: D4 reinterpreted a decision term inside TASK.md
  without tracing it to the architecture review that created it (F1), and the copied checkpoint did not bring its
  adversarial tests (F2). Candidate instructions are in F1–F4.
- Return-time freshness: HEAD a6bed48a unchanged; digest now 834ac9d91e86b4e36890e685c034611b940afaaeae44d7f3c8ff005de58dd0fe,
  not 0d0b4622…6c3d. Every digest file was compared with the freeze tree dfeeca67; only src/main.rs differs (mtime
  2026-11-09 12:27:20, during this review, not by me). The change removes four entries from
  `NO_BACKWARD_COMPATIBILITY_NOTES`: plan/hindsight_remaining_work.md, plan/hydradb/01-…, plan/hydradb/06-… and
  plan/shodh_memory_provider.md, all still on disk. That list is the sibling sessions' surface, outside p05-B3. My
  tests built the lib target only (main.rs not compiled), and the mutations ran on the freeze tree, so the evidence
  above is for the reviewed content. Stopped and reported per the brief; re-freeze before the recheck.

## MEM-P05-B3-R2 — correction recheck of p05-B3 F1–F5: CHANGES REQUIRED (F6)

- 2026-11-09; workflow `implement-opus-reviewed-2_2`; stage correction recheck by finding ID (broadened to the new
  port method `MemoryClient::search_scopes` and the eligibility refactor); reviewer claude-opus-5-5 / xhigh (B
  abdd866fb428eea05); worktree /Users/mac03/.codex/worktrees/60a0/openbot (HEAD a6bed48a).
- Artifact digest 50d052b7858ba6aab6cdad37ff2e0022d73d4a7f465bbe3d235146a802b23457 recomputed at start: MATCH. Freeze
  tree d9d63035; R2 delta `git diff dfeeca67 d9d63035` (src: memory_service adapter/limits/fake/tests, memory ports,
  memory_tools ports/runtime/builtin_tests, memory_tool_checkpoint + tests, checkpoints.rs visibility,
  agent_history_children_integration_tests, two test doubles; docs/deploy.md, docs/workflow-operations.md,
  implementation-decisions.md E11/E12; src/main.rs and plan/05-scopes, 01-departments, 04-root-unit,
  agent-workflow-call are sibling work — ignored). Coordination revision 2 (I003 + U4/U5) read.
- Working notes: U4/U5 text read in E12 and COORDINATION r2; docs checked against them (manual starts are owner/admin
  only — `manual::start` test "Only an owner or admin may start a run"; disable is allowed while unconfigured —
  `MemorySettings::disable`, company_memory page shows the switch). Eligibility refactor equivalent to R1's SQL (facts
  read unfiltered, decided by `user_scope_owner`; ambiguous root → none). F2 tests read; author's mutation C log fails
  at the changed-arguments assertion. F5 bundle-gate reasoning verified (bundle tools = message built-ins + HTTP/MCP
  selections + workflow pins; registrations refuse reserved names; a stray memory-named snapshot is untrusted →
  refused). Suspected: the F3 decisive test passes under per-request permits too (3rd request arrives after the first
  two release, within the 5 s wait) → reviewer mutation D running.
- Per finding (resolution entries in the findings file): F1 RESOLVED by ruling U4 (records, docs and tests match
  it); F2 FIXED; F3 code FIXED but its decisive test does not discriminate → F6 (Minor); F4 RESOLVED by ruling U5
  (docs match; the remedy works because disable is allowed unconfigured); F5 RESOLVED (catalog test; bundle-gate
  reason verified). New optional F7: attached/thread refusals proven only in the pure decision.
- Broadened checks: `search_scopes` — one acquire for the operation, `join_all` of `send` under the shared deadline,
  answers in request order, mixed tenants refused, empty input empty; the tool refuses a count mismatch; every
  `MemoryClient` double delegates (no default method). Eligibility refactor equivalent to R1. Records: E11 row and E12
  U4/U5 worded as COORDINATION r2 states; docs/workflow-operations.md "Agent memory tools" and docs/deploy.md "Memory
  service" state the rulings' behaviour and risks, and name only real keys and behaviour (owner/admin-only manual
  start, disable allowed unconfigured, publication refused only while disabled).
- Reviewer evidence (freeze tree d9d63035, unchanged digest):
  - Focused selection, the author's R2 filter, fresh own DB `open_bots_b3r2_test` (127.0.0.1:55432, DATABASE_URL
    unset; dropped after): 433 listed — identical to the author's list; R1's 426 plus the 7 new tests, none removed —
    432 passed, 0 failed, 1 ignored (pre-existing) (logs/MEM-P05-B3-R2-B-list.txt, -B-focused.txt). fmt check and
    `git diff --check dfeeca67 d9d63035 -- src docs` clean (-B-fmt-check.txt). Clippy not re-run (author's evidence).
  - Manifest `MEM-P05-B3-R2.evidence.json`: valid {PASS 5, FAIL 0, NOT EXERCISED 3}; artifact match. The author's
    re-run of mutation C fails at `call_memory_tool(original, changed).await.is_err()` (changed arguments).
  - Mutation D (mine; isolated copy tmp/memory-service-run/mut-b3r2-rev-d of d9d63035, own CARGO_TARGET_DIR, 21 GiB
    free before and after, deleted): `search_scopes` back to one permit per request — the F3 test passes 5/5 and the
    adapter module + tool e2e stay green (-B-mutation-D-per-request-permits.patch.txt, -B-mutation-D.txt) → F6.
  - Mutations A/B not re-run: R2 does not touch `save_on`, `observe_on` or the runtime's observe path (accepted with
    the manifest's reason; A/B target tests pass in my focused run).
- Outcome: **CHANGES REQUIRED** — F6 (Minor, actionable): strengthen the one-operation test so per-request permits
  fail it. Optional: F7. Everything else in p05-B3 holds (R1 coverage plus this round).
- Not run: full suite, stack budget, clippy, sqlx prepare (no query macro or schema change), live service, browser,
  Docker/CI.
- Process feedback: the resolution table and its named decisive tests made the recheck direct. The one gap is the
  pattern F6's lesson names: a decisive test argued against the old behaviour but never run against it.
- Return-time freshness: HEAD a6bed48a, digest 50d052b7…3457 recomputed at return: MATCH (no sibling drift this round). Own DB dropped; mutation copy deleted; tmp/memory-service-run/mut-r2 (author's) untouched.

## MEM-P05-B3-R3 — recheck of p05-B3 F6 (and optional F7, taken): PASS

- 2026-11-09; workflow `implement-opus-reviewed-2_2`; stage correction recheck by finding ID; reviewer
  claude-opus-5-5 / xhigh (B abdd866fb428eea05); worktree /Users/mac03/.codex/worktrees/60a0/openbot (HEAD a6bed48a).
- Digest at start: 090ffbc7d3793cfa2fc0aed9e626ac72b6ca457351fcbba89af23652ef0b6cfa, not the brief's 8857f538…a777.
  Every digest file was compared with freeze tree a69f88c5. The drift is sibling-only: 20 sibling plan files and
  src/main.rs, all mtime 14:20:33. The main.rs change adds plan/06-delegated-administration/* and
  plan/07-unit-resources/* entries to NO_BACKWARD_COMPATIBILITY_NOTES. No p05 file differs, so the review runs on
  the freeze tree content; mutations use `git archive a69f88c5`.
- R3 delta `git diff d9d63035 a69f88c5 -- src`: contract_fake.rs (held_now/held_peak replace arrivals),
  memory_service/tests.rs (F6 test), message_tests.rs (F7 test). No production code changed (memory_service/mod.rs and
  memory_tool_checkpoint.rs are byte-identical to R2's).
- Manifest MEM-P05-B3-R3.evidence.json valid {PASS 5, FAIL 0, NOT EXERCISED 2}; author's mutation D log fails the F6
  test with `Elapsed` at the 5 s wait.
- F6 FIXED: the test now requires three searches held by the fake at the same time within a 1.5 s hold, which a
  company share of 2 cannot produce with per-request permits; then one slot in use, a concurrent fetch completing,
  peak 3, 0 after. No flakiness risk seen: arrivals are milliseconds apart against a 1.5 s hold and a 5 s wait.
- F7 TAKEN: real manual runs of one binding by one actor; the channel run's stored session is attached → no user
  scope, the company-wide run → the actor. Uses the existing `admitted`/`admitted_as` manual fixtures.
- Reviewer evidence (freeze tree a69f88c5):
  - Mutation D (one permit per request; same patch as R2) in an isolated copy (tmp/memory-service-run/mut-b3r3-rev,
    own CARGO_TARGET_DIR, 23–24 GiB free, deleted): the F6 test fails 3/3 with `Elapsed` at the wait
    (logs/MEM-P05-B3-R3-B-mutation-D.txt). Mutation E, D reverted (`false AS attached`;
    -B-mutation-E-attachment-ignored.patch.txt): the F7 test fails at message_tests.rs:640 (Some(actor) vs None) while
    the matrix, child-inheritance, foreign-authored and unmutated F6 tests pass (-B-mutation-E.txt).
  - Focused selection, the author's R3 filter, fresh own DB `open_bots_b3r3_test` (127.0.0.1:55432, DATABASE_URL unset;
    dropped after): 813 listed — identical to the author's list, no R2 test removed — 811 passed, 0 failed, 2 ignored
    (pre-existing: browser fixture exporter; `history_crash_child_entry`, run by its parent process)
    (-B-list.txt, -B-focused.txt). fmt check and `git diff --check d9d63035 a69f88c5 -- src` clean (-B-fmt-check.txt).
  - Manifest MEM-P05-B3-R3.evidence.json valid {PASS 5, FAIL 0, NOT EXERCISED 2}; artifact match.
  - Not run: clippy (author's evidence; test-only change), mutations A–C (no production change since they were last
    run), full suite and stack budget (05 gate / 06C), live service, Docker/CI.
- Outcome: **PASS for digest 8857f53867e70b2edb79a9b13a0d21d4b9f05f5729f129daec690f5744bea777 (freeze tree a69f88c5) for
  the whole batch p05-B3 (R1–R3)**: F1 resolved (U4), F2 fixed, F3 fixed with F6's test, F4 resolved (U5), F5 resolved,
  F6 fixed, F7 taken. No open B3 findings.
- Return-time freshness: HEAD a6bed48a; digest now de7efd2c2d7d8e04a84df686695e1a1fd8ce79e179dd48ce6dc09c8cfa573f3c.
  Every digest file was compared with a69f88c5; only sibling files differ (mtimes 14:20:33 and 14:22:29):
  plan/01-departments/{README,03-leads-and-coordination,04-ui-and-docs,05-integration-and-final-verification}.md,
  plan/02-workflow-roles/04-escalation-and-reassignment.md,
  plan/03-principal-addresses/{README,03-agent-default-binding-and-relay,04-integration-and-final-verification}.md,
  plan/04-root-unit/{README,01-root-department-and-membership}.md, plan/05-scopes/README.md,
  plan/06-delegated-administration/{README,01-predicate-and-owners,02-routes-ui-and-audit,03-integration-and-final-verification}.md,
  plan/07-unit-resources/{README,01-ownership-and-selection,02-models-budgets-and-usage,03-integration-and-final-verification}.md,
  plan/evals/README.md, plan/slack-integration/README.md, plan/standard-operating-procedures/README.md, and src/main.rs
  (NO_BACKWARD_COMPATIBILITY_NOTES: +8 entries for plan/06-delegated-administration and plan/07-unit-resources, −4 for
  plan/agent-default-channel, whose directory no longer exists). No p05 file differs, so the PASS covers the content
  in this worktree.

## MEM-P05-B4-R1 — batch review of p05-B4, automatic recall (05B.1–4, E7): CHANGES REQUIRED

- 2026-11-09; workflow `implement-opus-reviewed-2_2`; stage batch review; reviewer claude-opus-5-5 / xhigh (B
  abdd866fb428eea05); worktree /Users/mac03/.codex/worktrees/60a0/openbot (HEAD a6bed48a).
- Digest a998bca3b5a3688a535b43b4cbd49d62e1cbe2d3601921a78679b049d21eece1 recomputed at start: MATCH. Freeze tree
  3749a60c; review base a69f88c5 (B3 accepted); delta `git diff a69f88c5 3749a60c -- src` (27 files; src/main.rs
  sibling NOTES only). No decision-record or docs change in the batch. Coordination r2, no notices.
- Disk 17 GiB free at start: below the 20 GiB floor, so no reviewer mutation probes this round unless space returns;
  the author's mutation F/G patches and logs were read instead.
- Working notes: read memory_recall.rs, agent_memory.rs, agent.rs/agent_context.rs diffs, agent_history/recall.rs,
  frozen.rs, preparation.rs, agent_preparation.rs, workflow.rs `run`, infra composition, adapter context_pack/
  context_validate, fake pack/validate, memory_recall_tests.rs, the openapi ContextPack/Validate schemas and the
  service's packing accounting (64 + 2 tokens per byte, compact snippet ≤ 320 bytes). Candidates: tool items not
  revalidated (fetch-based alternative exists); resumed attempt unvalidated when no recall is composed (URL removed);
  E11 `memory_preparation_` "recall deadline" and "two concurrent recalls" cases missing; D1/byte-term deviations only
  in TASK; validation sequential outside the recall deadline (optional).
- Coverage: requirements from 05B.1–4 and its focused checks, E7, E11 `memory_preparation_`, E1 (one operation under
  the recall deadline), E2/U4 (eligible_on reused), E3 (ceiling re-read at freeze, exposure ≤ ceiling, revalidation
  refuses a ceiling below the frozen exposure), E6 (raise and consumption commit with the preparation before
  `run_ready`), and the openapi ContextPack/ContextValidate contract.
- Challenges asked for:
  - D1 (cap 8,192): justified on substance — under the service's 64 + 2 tokens/byte accounting a compact item
    (≈ 400–700 bytes) cannot fit E7's 512-token quarter. It still yields to the request quarter, and about 10–12 items
    at most keep the recall row under the 64-item CHECK. It must be recorded in E7 → F4.
  - Tool items not revalidated: a fetch-based check by recorded id/revision is available → F2.
  - D2 (resumed = plan found the preparation frozen): sound under the owner lease — another owner cannot freeze
    between this claim's plan read and its preparation. The Frozen check precedes the enabled check, so after a
    disable a resumed attempt still validates and revokes (scopes absent). But it is skipped entirely when no recall
    is composed → F1.
  - D6 (`prepare_agent_with_recall` beside `prepare_agent`): accepted; one implementer, no default method, and
    production calls only the new method (`prepare_agent` remains for fixtures).
  - D5 (recall rows in tool-save provenance): accepted; they are observations this execution consumed (E5 wording),
    provenance only, the watermark already covers them, and a `memory.load` step never shares the execution.
- Also checked and holding: plan is a rolled-back read through `candidate_on`; not-ready scopes provisioned in their
  own transaction; empty eligible set → `unavailable{not_ready}`; budget split and `skipped_budget` floor; query
  order (trigger message body, current input, recency) cut at a character boundary; an existing freeze wins (fetched
  pack discarded unseen); freeze re-reads the ceiling and discards on change; `with_memory` binds before history and
  restore re-binds before the whole-document comparison; the memory key enters the history budget only when items
  exist; prompt construction pure; the usage sentence alone goes to the preamble; refusal (401/403/protocol) fails
  closed with nothing frozen; adapter refuses any pack above the ceiling and checks validate answers by id/order;
  `Box::pin` at both new seams; the 2 MiB stack run includes the six DB recall tests.
- Reviewer evidence (freeze tree 3749a60c, unchanged digest): focused selection, the author's filter, fresh own DB
  `open_bots_b4_test` (127.0.0.1:55432, DATABASE_URL unset; dropped after): 862 listed — identical to the author's —
  860 passed, 0 failed, 2 ignored (pre-existing: browser fixture exporter; history_crash_child_entry)
  (logs/MEM-P05-B4-R1-B-list.txt, -B-focused.txt). No test removed by the delta. fmt check and `git diff --check
  a69f88c5 3749a60c -- src` clean (-B-fmt-check.txt). Manifest valid {PASS 6, FAIL 0, NOT EXERCISED 2}; the author's
  F and G logs fail exactly their target tests (F: changed-ceiling test :348; G: revocation test :252).
- Not run: my own mutation probes (17 GiB free < 20 GiB floor), clippy (author's evidence), full suite and stack budget
  (05 gate / 06C), live service, Docker/CI.
- Outcome: **CHANGES REQUIRED** — F1–F4 Minor (actionable), F5 optional.
- Process feedback: the design and deviation list were clear and made the challenges direct; the gaps were E11 cases
  not mapped (F3) and deviations from decided bounds kept in TASK (F2, F4).
- Return-time freshness: HEAD a6bed48a, digest a998bca3…eece1 recomputed at return: MATCH (no sibling drift). Own DB dropped; no mutation copy created.

## MEM-P05-B4-R2 — correction recheck of p05-B4 F1–F5: CHANGES REQUIRED (F6)

- 2026-11-09; workflow `implement-opus-reviewed-2_2`; stage correction recheck by finding ID; reviewer
  claude-opus-5-5 / xhigh (B abdd866fb428eea05); worktree /Users/mac03/.codex/worktrees/60a0/openbot (HEAD a6bed48a).
- Digest 507e893896000e551a847db69b4179ba7b399d7683e4feb2e82fd9a8401f8f1b recomputed at start: MATCH; freeze tree
  ff1607fd; delta `git diff 3749a60c ff1607fd -- . ':(exclude)plan/memory-service/phases'` (12 files: runtime,
  memory_recall.rs, agent_memory.rs, recall.rs, fixtures/tests, docs/deploy.md, docs/workflow-operations.md,
  implementation-decisions.md E7/E12 U6, COORDINATION r3). 33 GiB free.
- Working notes: F1 structural guard read (recall non-optional; `new` composes `MemoryRecall::unconfigured`; `run`
  calls `validate` unconditionally; `run_ready` has no other caller; `new`'s bound admits only PostgresPersistence;
  production composition replaces it in `with_memory`). F2 fetch-based tool-item check read (dedup by id+revision,
  chunks of 64, absent/changed → revoked, outage → unavailable). Noticed: the new F2 test covers only a corrected
  revision, not a deleted item (absent from fetch) → reviewer mutation L running. Disable now revokes resumed runs that
  were shown memory (scopes absent → revoked) — docs silent.
- Per finding (entries in the findings file): F1 FIXED (structural guard; every construction site goes through
  `new`; `validate` is unconditional before the only `run_ready` call); F2 FIXED, but the deleted (absent-id) branch is
  untested → F6 (Minor); F3 FIXED (deadline and concurrent-recall tests, both discriminating per the author's H/I);
  F4 RESOLVED by ruling U6; F5 not taken, reason accepted.
- Points raised by the coordinator:
  1. F1 construction sites: production (`infra/workflow.rs` → `with_memory`), the autonomous e2e support and all
     fixtures use `new`, which always composes a recall. No runtime can skip `validate`.
  2. F2 residual (tool-item provenance/expiry) recorded in E7 "As implemented": no decision-owner ruling needed. It
     is the fetch route F2 offered, with its residual named there.
  3. `ToolReadsOnly` hides nothing B3 asserted. It switches off recall planning only, so B3's consumption, level and
     call counts stay about tool reads; revalidation still runs, and B3's "no observations/ call" assertions sit
     where no earlier read exists.
  4. F5 accepted as not taken.
- New: F6 (Minor) — mutation L (an id absent from the fetch passes) survives all 32 `memory_preparation` +
  `memory_tools::` tests; F7 (optional) — docs do not say that disabling memory revokes in-flight runs that were shown
  memory, which now end `memory.context_revoked` on their next claim (scopes absent → revoked).
- Reviewer evidence (freeze tree ff1607fd, unchanged digest):
  - Mutation L in an isolated copy (tmp/memory-service-run/mut-b4r2-rev, own CARGO_TARGET_DIR, 33 GiB free before and
    after, deleted): 32 passed, 0 failed (logs/MEM-P05-B4-R2-B-mutation-L.txt, -B-mutation-L-deleted-tool-item-passes.patch.txt).
  - Focused selection, the author's widened filter, fresh own DB `open_bots_b4r2_test` (127.0.0.1:55432, DATABASE_URL
    unset; dropped after): 931 listed — identical to the author's — 929 passed, 0 failed, 2 ignored (pre-existing)
    (-B-list.txt, -B-focused.txt). The two timing-sensitive tests (concurrent recalls, multi-scope search) repeated
    3× alone: all pass (-B-timing-repeat.txt). No test removed; fmt check and `git diff --check 3749a60c ff1607fd --
    src docs` clean (-B-fmt-check.txt).
  - Manifest MEM-P05-B4-R2.evidence.json valid {PASS 6, FAIL 0, NOT EXERCISED 2}; the author's H, I, J, K logs each
    fail exactly their target test.
  - Docs checked against code: workflow-operations "Agent memory tools" (recall bound 8,192 / quarter, deadline,
    resume revalidation) and deploy.md U5 sentence match the implementation.
  - Not run: clippy and the 2 MiB stack run (author's evidence), full suite, live service, Docker/CI.
- Outcome: **CHANGES REQUIRED** — F6 (Minor, actionable). Optional: F7.
- Return-time freshness: HEAD a6bed48a, digest 507e8938…8f1b recomputed at return: MATCH (no sibling drift). Own DB dropped; mutation copy deleted; open_bots_root_test not used.

## MEM-P05-B4-R3 — recheck of p05-B4 F6 (and optional F7, taken): PASS

- 2026-11-09; workflow `implement-opus-reviewed-2_2`; stage correction recheck by finding ID; reviewer
  claude-opus-5-5 / xhigh (B abdd866fb428eea05); HEAD a6bed48a. Digest 8490e2e6c999eea62a7738e2e762b18c1fbda91cbec21f106612bc761d4f78ee
  recomputed at start: MATCH; freeze tree 2a0d2550; delta `git diff ff1607fd 2a0d2550` = memory_recall_tests.rs,
  docs/deploy.md, docs/workflow-operations.md (memory_recall.rs unchanged, so mutation L's patch applies as is).
- Manifest MEM-P05-B4-R3.evidence.json valid {PASS 4, FAIL 0, NOT EXERCISED 1}; the author's mutation L log fails the
  tool-item test on "deleted" (31/32).
- F6 FIXED: the tool-item test now has a deleted case (the fake drops the id) beside corrected and unchanged, with
  terminal `memory.context_revoked`, no provider entry and one fetch each. Reviewer mutation L, re-run on freeze tree
  2a0d2550 (isolated copy tmp/memory-service-run/mut-b4r3-rev, own CARGO_TARGET_DIR, 32 GiB free, deleted), fails it at
  memory_recall_tests.rs:481, 31/32 (logs/MEM-P05-B4-R3-B-mutation-L.txt).
- F7 TAKEN: both doc sentences are accurate against `scopes_on`/`ready_scopes_on` and `validate` (no scopes → revoked).
- Reviewer evidence: `memory_preparation` + `memory_tools::` on a fresh own DB `open_bots_b4r3_test`
  (127.0.0.1:55432, DATABASE_URL unset; dropped after): 32 passed, 0 failed (-B-focused.txt); fmt check and
  `git diff --check ff1607fd 2a0d2550 -- src docs` clean. Not re-run: clippy (author's evidence; test-only code change)
  and the wider R2 selection (no production code changed since R2, where 929/931 passed).
- Outcome: **PASS for digest 8490e2e6c999eea62a7738e2e762b18c1fbda91cbec21f106612bc761d4f78ee (freeze tree 2a0d2550)
  for the whole batch p05-B4 (R1–R3)**: F1 fixed, F2 fixed (with F6's test), F3 fixed, F4 resolved (U6), F5 not taken
  (accepted), F6 fixed, F7 taken. No open B4 findings.
- Return-time freshness: HEAD a6bed48a, digest 8490e2e6…78ee recomputed at return: MATCH (no sibling drift); own DB dropped; mutation copy deleted.

## MEM-P05-B5-R1 — batch review of p05-B5 (capture policy, generation intents, delivery, E9 races): CHANGES REQUIRED

- 2026-11-09; workflow `implement-opus-reviewed-2_2`; stage batch review; reviewer claude-opus-5-5 / xhigh (B
  abdd866fb428eea05); HEAD a6bed48a. Digest be48350f22f124546d81d9d9abc175039d0c971b8580c817ac32fe103629a5c3 recomputed at
  start: MATCH; freeze tree 8f5f0f97; review base 2a0d2550 (B4 accepted); delta 29 files (schema, capture freeze,
  acceptance hook, capture targets, delivery by kind, settings use case/store/UI, docs, E8 "As implemented").
  20.3 GiB free at start: no reviewer mutation probe this round (a copy build would take the disk well below the
  floor); the author's M1–M5 patches and logs were read.
- Working notes: capture eligibility (thread → agent scope; unattached private session + U4 → user scope; child and
  attached never; only `Some(output)`), freeze order run → tenant → namespace → policy (agent delete: company KEY SHARE
  → agent FOR UPDATE → namespace FOR UPDATE → cascade on policy, so policy-before-namespace would deadlock — sound;
  in production an agent with conversations cannot be deleted at all, the conversation FK refuses it), settings
  serialized on the company row (authorize_company FOR NO KEY UPDATE), so the first-insert revision race is closed;
  delivery by kind, `generation_disabled` terminal, run-cancel trigger kind-agnostic. Candidates: docs say
  message-triggered/scheduled runs are never captured (thread capture captures them); with the URL removed, capture
  keeps freezing new intents that nothing sends or settles (docs say only already-queued ones); `validate`'s doc
  comment now sits on `provision` (nit).
- Coverage: 05B.5–7 and the focused-check paragraph; E2 (tenant FOR SHARE, epoch, cap, U4 eligibility), E3
  (ceiling = run knowledge ceiling, consumed via `consumed_for`), E8 (capture policy, intents, delivery, and its
  "As implemented" paragraph), E9, E10 (samples), E11 `memory_capture_`, E12 U3/U4/U5. Code read: the whole 29-file
  delta.
- Coordinator's points:
  1. Lock order run → tenant → namespace → policy: sound. Agent delete holds the agent and then the namespace FOR
     UPDATE before its cascade reaches the policy rows, so a capture holding the policy while waiting on the namespace
     would deadlock. Taking the namespace first makes the capture wait and then see `removing`. The settings change
     (company NO KEY UPDATE → agent FOR SHARE → tenant FOR SHARE → policy FOR UPDATE) holds nothing the capture waits
     for. Note: in production an agent that has conversations cannot be deleted at all (immediate conversation FK),
     so this race matters only for agents without history; the order is still correct.
  2. Ready-namespace-only capture with provisioning at preparation: accepted as a documented routine choice (the
     claim requires a ready namespace, B2).
  3. Request content: two events (logical input `prepared["context"]`, accepted output) at the run content level,
     producing ceiling = knowledge ceiling, consumed via `consumed_for` (threads → unknown → thread lowest-reader
     level, E8). No recall pack, history pairs, tool rounds or session id. 512 KiB text and 3.5 MiB frozen bounds.
     Key `ob.g.<exchange>`, policy numbers frozen.
  4. Eligibility: matches E8/U4; attached private sessions are refused twice (match arm and U4 fact); failed
     exchanges never reach `capture_on` (`Some(output)` only); samples run in the sealed application path and never
     reach acceptance.
  5. Delivery by kind: decode by stored kind (a mismatch is invalid); the untagged receipt keeps the save shape for
     effect waits; `generation_disabled` terminal; same key and body on retry; Delivery pool.
  6. Switch-off leaves queued intents (documented); the run-cancel trigger has no kind filter. The U5 capture text is
     inaccurate → F2.
  7. generation_cap refreshed at lifecycle settle: accepted as documented, with its remedy and warning.
  8. Settings: owner/admin via `lock_authority`, serialized on the company row (no first-insert revision race),
     agent FOR SHARE, tenant must be enabled to turn on, 4 KiB body limit, route in WRITE_ROUTES, reach texts match
     E8; covered by the settings test.
  9. Stand-ins accepted. Seeded rows are what acceptance reads. Synthetic runs at freeze level are the same
     precedent as saves; the acceptance tx already holds the run lock.
- B6 split: acceptable. The gate is verification plus new deliverables (`scripts/memory-integration.sh` and its CI
  step, live tests), which should be reviewed as code. Phase 05 is accepted only after B6 reruns all five prefixes,
  `memory_capture_` included, on the final tree; until then B5's PASS is batch-level.
- Reviewer evidence (freeze tree 8f5f0f97, unchanged digest): focused selection, the author's filter, fresh own DB
  `open_bots_b5_test` (127.0.0.1:55432, DATABASE_URL unset; dropped after): 1180 listed — identical to the author's —
  1173 passed, 1 failed (confirmed unrelated: `rendered_markup_carries_no_inline_javascript`, offender
  workflow_authoring/sample.rs:153, file unchanged since base a6bed48a), 6 pre-existing ignored
  (logs/MEM-P05-B5-R1-B-list.txt, -B-focused.txt). No test removed; fmt check and `git diff --check 2a0d2550 8f5f0f97
  -- src migrations docs` clean. Manifest valid {PASS 6, FAIL 0, NOT EXERCISED 2}.
- Not run: own mutation probes (disk at the 20 GiB floor), clippy (author's evidence), stack budget, live service,
  browser, sqlx prepare (B6).
- Outcome: **CHANGES REQUIRED** — F1, F2 Minor (actionable); F3, F4 optional.
- Return-time freshness: HEAD a6bed48a, digest be48350f…29a5c3 recomputed at return: MATCH (no sibling drift); own DB dropped; no mutation copy created.

## MEM-P05-B5-R2 — correction recheck of p05-B5 F1–F4: PASS

- 2026-11-10; workflow `implement-opus-reviewed-2_2`; stage correction recheck by finding ID; reviewer
  claude-opus-5-5 / xhigh (B abdd866fb428eea05); HEAD a6bed48a. Digest 6041e394d913523eb06c8aaac8348c675134643e8947fe1281a6a20eca9e4f1e
  recomputed at start: MATCH; freeze tree 8e18b451; delta `git diff 8f5f0f97 8e18b451` (22 files). Coordination r4
  (U7). 37 GiB free.
- Working notes: U7 design read end to end. The runtime passes `capture: self.recall.configured()`, which is true
  only after `with_memory` composes a client. `freeze_on` arms only on the first freeze; `restore_on` re-binds the
  saved flag before the whole-document comparison, so a restore cannot drop or invent it (a mismatch fails the
  restore). `with_capture` arms only with a history exchange. `accepted_on` requires the frozen `capture: true`;
  the only production caller is the agent runtime, and `prepare_agent` (fixtures) passes `Default` (unarmed). The U7
  residual is recorded. `capture_targets` readiness is state-only. Disable settlement drops the text
  (`settle_open_on` sets `request = NULL`), as the new deploy.md text says. New nit: `configured()` was inserted
  between `unconfigured`'s doc comment and `unconfigured`.
- Per finding: F1 FIXED (docs and settings text match `capture_on`; asserted in the settings test); F2 RESOLVED by
  U7 and implemented as above (author's M6/M7 each fail the U7 test; residual recorded); F3 FIXED (new nit F5,
  optional); F4 TAKEN (in-flight generation delivery vs agent delete, completion fenced out).
- The U7 design held up under challenge. The flag is frozen with the preparation and kept by every restore. Only
  the configured composition arms it (the runtime's recall is always present since B4 R2 and unconfigured by
  default). Acceptance has no other entry, and `ai.classify`/`decision.agent` have no history exchange to arm. Its
  failure direction is safe: a preparation frozen while unconfigured stays unarmed after the URL returns.
- Reviewer evidence (freeze tree 8e18b451, unchanged digest): the author's filter on a fresh own DB `open_bots_b5r2_test`
  (127.0.0.1:55432, DATABASE_URL unset; dropped after). 1182 listed, identical to the author's list, and none of the
  R1 tests removed. 1175 passed, 1 failed, 6 pre-existing ignored. The failure is the confirmed unrelated
  `rendered_markup_carries_no_inline_javascript` (workflow_authoring/sample.rs:153, file unchanged since base)
  (logs/MEM-P05-B5-R2-B-list.txt, -B-focused.txt). fmt check and `git diff --check 8f5f0f97 8e18b451 -- src docs` are
  clean. The manifest is valid {PASS 6, FAIL 0, NOT EXERCISED 2}. Not run: my own mutation probes (the author's M6/M7
  run on the final tree cover both arming branches, and the restore branch is covered implicitly because a dropped
  flag fails the restore comparison in the resumed recall tests), clippy, and B6 items.
- Outcome: **PASS for digest 6041e394d913523eb06c8aaac8348c675134643e8947fe1281a6a20eca9e4f1e (freeze tree 8e18b451) for
  the whole batch p05-B5 (R1–R2)**. Optional only: F5 (doc-comment placement nit). Phase-05 acceptance still waits on
  the B6 gate.
- Return-time freshness: HEAD a6bed48a, digest 6041e394…4f1e recomputed at return: MATCH (no sibling drift); own DB dropped; no mutation copy created.

## MEM-P05-B6-R1 — gate review of p05-B6 (the phase-05 gate, E11), reviewed as code: PASS

- 2026-11-10; workflow `implement-opus-reviewed-2_2`; stage gate review, reviewed as code under my B6 split condition;
  reviewer claude-opus-5-5 / xhigh (B abdd866fb428eea05); HEAD a6bed48a. Digest
  88808f5ad06857e86e417fe9593fd6b72185bdc01b42ad52871054c77e81b0a1 recomputed at start: MATCH; freeze tree 7388d1c2;
  delta `git diff 8e18b451 7388d1c2 -- . ':(exclude)plan/memory-service/phases'` (11 files). Coordination r4,
  unchanged. 24 GiB free.
- Script (scripts/memory-integration.sh, read in full).
  - Both URLs are required (`:?`), so there is no default server.
  - The database, role and password are random per run.
  - The EXIT trap is installed before anything is provisioned and turns on `set +e`. It stops the service and the
    stand-in, drops the database `WITH (FORCE)` and the role, and shows the service log only on failure.
  - Credentials are never echoed. The owning role's URL is built and exported but never printed. The bootstrap log is
    shown only when no `msk_adm_` token came back.
  - The tests run with `--test-threads=1`, DATABASE_URL unset, and pipefail set. The script requires
    `test result: ok. N passed` with N ≥ 1, and requires the stand-in's request log to hold a `/v1/messages` call.
  - Every wait is bounded: readiness 240 × 0.5 s; in the tests, `eventually` 240 × 250 ms, a 10 s request timeout and
    capped worker passes.
  - A port collision fails closed: a foreign listener rejects the operator token, and a foreign stand-in leaves our
    request log empty.
  - Residuals: F2 (how the service URL is derived from the admin URL) and F3 (the token on curl's argv).
- CI job.
  - The pgvector image digest is the same as memory-service.yml:13.
  - The model cache key and `prepare-model.py --download` are the same as memory-service.yml:44-50, so both workflows
    share one cache entry.
  - `localhost:5432` is the job's own service container. The verify job already runs the runner's psql against the
    same PG18 image (ci.yml:144).
  - SQLX_OFFLINE is set.
  - There is no `timeout-minutes`, as in every other ci.yml job; the script's waits are bounded.
  - Toolchain: F1.
  - The verify job's new browser step follows the knowledge-browser step (dump fixtures, then `node --test`).
- Live tests.
  - Point 3, the cap refresh. The test raises the cap through the operator API, then disables, enables and
    reconciles. It asserts the cached `generation_cap` is 100,000 before capturing, so the capture cannot be skipped
    as "generation off". This is the documented route (docs/deploy.md:772-776: a raised cap "is picked up only at the
    next disable and enable"). The client credentials survive the cycle: the searches after it use them.
  - Point 4, generation verified through search. OpenBot's client credentials are not granted `generations.status`.
    In this test only generated memories land in the agent namespace (saves go to company scope). Every fetched
    detail must carry the stand-in's LIVE-GENERATED marker and be labelled at the producing ceiling (6). The delivery's
    `operation_id` is asserted, and the script separately requires the stand-in to have been called. Together these
    prove generation end to end; polling status would add only the operation's terminal state.
  - Competing workers. Two delivery workers race four intents. Each intent is `delivered` with `replayed: false`,
    with one observation per intent. The test does not assert that both workers claimed something, so it proves
    exactly-once delivery, not that a race took place. That is acceptable: B2's persistence-level competing-claimant
    tests carry the protocol proof.
- Browser test (point 5).
  - Fixtures are served with the production CSP parsed from security.rs, the real app.css and htmx, and app.js and
    theme-init.js from the fixture dump.
  - The theme is switched by setting `localStorage['ui_theme']`, then reloading and waiting for `data-theme`. That is
    the key the init script reads (mailbox.rs:794) and the toggle writes (mailbox.rs:265; asserted in
    pages/tests.rs:257-258). The toggle click itself is covered by that pages test.
  - Both screenshots read clearly: labels sit above their fields in the two-column grid.
- B5 acceptance impact.
  - The only production-code change in the delta outside the gate's own files is the per-part `source_ref`
    (deliveries.rs:408-412).
  - Its only producer is that closure (`graft grep "openbot.exchange"`: 1 hit). No OpenBot code reads a source_ref
    back, and the frozen body is replayed unchanged.
  - Two independent guards: the fake now mirrors services/memory/src/application/generation.rs:211-219 (one
    `(source_ref, revision)` with a different text or level → 409), and capture_tests asserts distinct sources.
  - The rest is non-functional or test-only:
    - the memory_recall.rs doc move (B5-F5 fixed);
    - the company_memory.rs relayout (field names unchanged; the browser test asserts the posted body; the
      settings tests in the prefix runs pass);
    - the live-test module registration.
  - So B5's acceptance stands, amended only by the source_ref fix.
- Gate evidence against E11.
  - The manifest is valid {PASS 8, FAIL 0, NOT EXERCISED 2}.
  - Split condition (all five prefixes on the final tree): met.
    - The author's 2 MiB stack-budget full-lib run lists every prefix with no failures: scope 43, access_level 25,
      integration 51 (+2 live, ignored), preparation 13, capture 16.
    - That run came before a fmt-only reflow, after which capture and integration were rerun (67 passed).
    - I reran all five on the frozen final tree: 148 passed, 0 failed, 2 ignored (the live tests).
  - Live tests: 2/2, in the author's run and in mine.
  - Stack budget: 3689 passed, no overflow, 3 failures, all pre-existing and unrelated:
    - workflow_authoring/sample.rs:153 (inline JavaScript);
    - two security_auth_limits_* tests (the cluster runs with `--locale=C`).
  - Boundaries: pass, except the pre-existing transport offender channel_reply_continuation_views_support.rs:59
    (unchanged).
  - Also pass: the credential-rotation script, `sqlx prepare --check` (local sqlx-cli 0.9.0; CI pins 0.8.6), clippy
    `--all-targets`, fmt and diff-check.
  - NOT EXERCISED: CI execution (the new job has never run) and the all-target full suite (E11 assigns it to 06C).
- Reviewer evidence. All runs used my own fresh database `open_bots_b6r1_test` on 127.0.0.1:55432, with
  DATABASE_URL unset and ports 18185/18186.
  1. Live script: 2 passed, exit 0. Afterwards no memory_live_* database or role remains, and the log holds no
     token or URL (logs/MEM-P05-B6-R1-B-live.txt).
  2. Failure path: MEMORY_MODEL_DIR pointed at an empty directory, so the service exits during startup after
     provisioning. Exit 1, the service log tail is shown, and no memory_live_* database or role remains
     (logs/MEM-P05-B6-R1-B-live-failure-path.txt).
  3. Five prefixes: 148 passed, 0 failed, 2 ignored (logs/MEM-P05-B6-R1-B-prefixes.txt).
  4. Browser test, with the main checkout's node_modules linked temporarily and then removed: fixture dump and
     `node --test` both pass (logs/MEM-P05-B6-R1-B-browser.txt).
  5. Browser-test probes, run on copies of the generated fixture in my scratchpad
     (logs/MEM-P05-B6-R1-B-browser-probes.txt):
     - one label's `for=` detached → fails (unlabelled count 1);
     - `required` removed → fails (a blank reason is posted);
     - control → passes.
  - Not run:
    - a Rust mutation copy reverting the source_ref fix: a cold target would take the 24 GiB free below the
      20 GiB floor, and the two guards above cover the regression;
    - a socket-style admin URL (it would reach the default server);
    - CI.
- Findings: F1–F3, all optional (see the findings file). B5-F5: FIXED.
- Outcome: **PASS for digest 88808f5ad06857e86e417fe9593fd6b72185bdc01b42ad52871054c77e81b0a1 (freeze tree 7388d1c2)
  for p05-B6. Phase 05 can be accepted.**
  - Every E11 05-gate item has passing evidence on the final tree. The three failures are pre-existing and
    unrelated.
  - Residuals: the new CI job's first run is the only proof of its wiring (F1 is the likeliest CI-only difference),
    and the all-target full suite belongs to 06C.
- Return-time freshness: HEAD a6bed48a; digest 88808f5a…b0a1 recomputed at return: MATCH, no sibling drift.
  - My database `open_bots_b6r1_test` was dropped; no memory_live_* database or role remains.
  - The node_modules link was removed, and the scratch fixtures were deleted.
  - No mutation copy was created.
