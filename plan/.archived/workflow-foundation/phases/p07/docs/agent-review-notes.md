# P07 agent review notes

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Reviewer-owned log (B). One entry per review round. Findings detail lives in
`agent-review-findings.md`; this file links them.

## WF-P07-R001 — architecture gate P07-D1 rev 1 (batch B1) — CHANGES REQUIRED

### Identity

- Date 2026-10-21. Workflow `implement-opus-reviewed-2_2`. Stage: architecture gate for batch B1.
  Reviewer B: Claude Opus 5.5 (`claude-opus-5-5`), independent fresh-context reviewer; effort as
  configured by the spawner (TASK records opus/xhigh). No agents spawned.
- Records: TASK `TASK.md`, review `REVIEW.md`, findings `agent-review-findings.md`, decision
  `logs/decision-D1.md` revision 1, sha256
  `7c3245e7b53867b47ce70750bf96cdb5503e5b298eb95b0490d091bb7e8f3c92` (verified at start and
  before return).
- Phase `p07`, worktree `/Users/mac03/dev/openbot`, branch `workflows`; phase start = review base
  = HEAD `921079d125ec0f768eee53f603a1db648ff356bc`; code digest
  `e2714983beb52a28eb0432b58be69d6652ea56a6bc0a07566918118138c2e3fc` with exclusions
  `plan/workflow-foundation/phases/p07/docs plan/workflow-foundation/COORDINATION.md marketing website`
  (snapshot.sh run at start and again before return: identical). No p07 source edits exist.
  Preserved pre-existing work (440 `plan/workflow-foundation/` deletions, `marketing/`,
  `website/`) ignored as instructed.
- Gated decision: `P07-D1` rev 1 (D1.1–D1.9, D1.13).
- Coordination: `plan/workflow-foundation/COORDINATION.md` revision 1 — no plan-wide
  instructions beyond AGENTS.md/README, no cross-phase notices; re-read before return, unchanged.
  No immutable notice copy needed.

### Coverage and evidence (inspection only; no tests or DB checks were needed or run)

- Requirements read from originals: plan `07-child-workflows-and-review-revisions.md` (all
  sections), EXPANSION 07.1–07.4, README agreed decisions, REPLACEMENT-MAP rows naming 07 (63,
  64, 80, 84 and removal ledger), NOT_PLANNED (acyclic children), phase 08 entry-point text (for
  the deferrals), AGENTS.md (root, src, application, persistence; infra not material).
- Code verified against decision claims:
  - Schema (`migrations/20260817000000_init_schema.sql`): `workflow_waits` columns/checks,
    `workflow_wait_execution` / `workflow_wait_one_active` indexes, `check_workflow_wait_commit`,
    `workflow_runs` checks/keys/triggers (no `parent_run_id` index), `workflow_run_parents` keys and
    FKs (the D1.3 FK target key exists), `workflow_admissions` PK/FK, `insert_workflow_parent_link`,
    `insert_workflow_budget_link`, `allocate_workflow_wakeup_sequence`,
    `schedule_workflow_parent_wakeup`, `validate_workflow_wakeup_insert`, `workflow_wakeup_shape`,
    `debit_workflow_budget_receipt`, root budget tables, token refusal triggers,
    `workflow_control_retry_safe`.
  - Runtime: `waits.rs` (`lock`, `row`, `park_on`, `select_due`, timeouts), `wait_commit.rs`
    (`resume_on`, `advance`, `expire`, `fail`, `saved`), `pending_recovery` (`activate`, `settle`),
    `activation.rs` (`activate_on`, `resolve`, `parent_run`), `batch_commit::complete`,
    `recovery::route_parked_failure`/`route_failure`, `polling::CANDIDATES_SQL`, worker dispatch
    (`worker.rs` park→resume pattern), `controls::control`/`cancel_on`, `control_retry`,
    `authority` lock modes, `inspection::HEAD`.
  - Admission: `admission.rs`, `admission_write.rs` (24 h deadline), `admission_source::child`,
    `admission_binding::SourceKey`/`read_saved_binding`, `admission_history::capture`,
    `history_reader`, `WorkflowService::admit`/`verify_binding` (no pinning check).
  - Language/publication: registry child family (`families.rs` child/decision), static reviewer
    roots (`registry/mod.rs:177`), `ChildContract`, publication `dependencies.rs`, root budget
    compile defaults, existing revision fixtures, `ConfiguredBinding::new`, action authority
    ceiling (`http_runtime::context`, `authorization::validate_run_authority`).
- Confirmed sound (no finding): atomic child+wait creation and replay identity; wait→child FK and
  uniques; excluding `child_run` from `workflow_wait_execution` while keeping one active wait;
  settling round N before inserting round N+1; reading the immutable wakeup fact and terminal
  execution output without locking the child; parent-first lock order and absence of child→parent
  locks (budget/parent-link triggers lock only own rows, root usage, and KEY SHARE on the parent
  execution already held); cancel vs park/resume serialized by the parent run lock; deadline
  inheritance and expiry precedence (`resume_on` expires before consuming); repetition receipts
  per round with root-only accounting; `done` before `exhausted`; static child params (consistent
  with existing `static_roots` reviewer rule; keeps reviewers out of run input transitively);
  by-name resources restrict the child to the parent binding's resources with the parent actor;
  deferred `next_input`/`exit_when` are deterministic because they commit with the transition;
  `choices {done, exhausted}` gives the plan's exhaustion route without auto-approval.
- Assumptions challenged: static params (accepted), by-name resources (accepted; optional
  alternative in F007), deferred bindings with self-reference (accepted with clarification F007.3),
  `done`/`exhausted` routes (accepted), rerun-link deferral (F003), keeping binding-based child
  admission (F004), history/association copy (F001), park as a no-attempt transition (F002),
  cascade lookup (F006), retry obligation (F005).
- Lightly reviewed / not covered: compiler internals for the new `repeat` block and fixed choice
  routes (wire/graph changes) were assessed only for feasibility; B2 agent workflow tools out of
  scope except D1.13 (no conflict found); fixture YAML content of the future template judged only
  against the plan outline. No DB-backed or compile checks were run (decision review; schema
  claims verified by reading the init schema).

### Outcome

- **Changes required** for P07-D1 rev 1. Actionable: WF-P07-F001 (High), WF-P07-F002 (Medium),
  WF-P07-F003 (Medium), WF-P07-F004 (Medium), WF-P07-F005 (Low), WF-P07-F006 (Low).
  Optional: WF-P07-F007. A corrected decision revision needs a recheck of the changed sections;
  unaffected D1 sections listed as sound above can be reused while code base and digest are
  unchanged.

### Process and instruction feedback

- Batch/gate sizing was right: one gate covering language, runtime, schema, concurrency and
  cancellation was reviewable in one pass; B2 correctly held back.
- The decision was precise and code-anchored, which made verification fast. Misses were of two
  kinds: inherited side effects of reused helpers (F001) and failure classification for a new
  no-lease transition (F002) — both are gated-decision defects, not implementation errors.
- Requirement deferrals (F003) and keep-as-is choices (F004) were recorded only inside the
  decision; candidate instruction (skill scope): deferrals of stated phase requirements need an
  agreed disposition in COORDINATION/TASK and the receiving plan. Candidate (phase investigation
  scope): grep for comments naming the current phase (`Phase07`) and address each in the gate
  (F005). Evidence: finding entries above.

### Follow-up 2026-10-22 (WF-P07-R002)

- R001's WF-P07-F004 "required change" named only `budget_race_tests` and the wakeup tests as
  fixtures to move. `budget_activation_tests` and `budget_schema_tests` also create children
  through the binding-based `admit_child` helper; revision 2 inherited the incomplete inventory.
  Logged as WF-P07-F008 (Low). The R001 outcome stands; this records the missed inventory step.

## WF-P07-R002 — correction recheck of R001, P07-D1 rev 2 (batch B1) — IN PROGRESS

- Started 2026-10-22 by a fresh replacement reviewer B (session 2). Identity verified at start:
  branch `workflows`, HEAD `2d608f269761717de7706912bd99ddd325d9d352`, digest
  `31c52a4a554512444396a39b77d8b2b5142231232cf0ee3027a5d4855fc91b21` (exclusions
  `plan/workflow-foundation/phases/p07/docs plan/workflow-foundation/COORDINATION.md marketing website`),
  `logs/decision-D1-r2.md` sha256 `e5ceea98…8c93` and `logs/decision-D1.md` sha256 `7c3245e7…8f3c92`
  match; `COORDINATION.md` equals `logs/coordination-r2.md` (sha256 `c6b0a521…326ee`).
  This stub is replaced in meaning (not rewritten) by the completed entry appended below.

## WF-P07-R002 — correction recheck of R001, P07-D1 rev 2 (batch B1) — CHANGES REQUIRED (Low only)

### Identity

- Date 2026-10-22. Workflow `implement-opus-reviewed-2_2`. Stage: architecture-gate correction
  recheck of WF-P07-R001, plus review of the changed/new parts of revision 2 as a whole.
  Reviewer B: Claude Opus 5.5 (`claude-opus-5-5`), fresh replacement for the unreachable
  session-1 reviewer (same role/settings; TASK records general-purpose, opus, xhigh, no fork).
  No agents spawned; `graft` unavailable (grep/read used).
- Records: TASK `TASK.md` (Decisions: P07-D1 rev 2 entry), review `REVIEW.md`, findings
  `agent-review-findings.md`. Decision `logs/decision-D1-r2.md` revision 2, sha256
  `e5ceea98409dc8a4e6dc5a48931305c57d99658877d718e87404a6e8d5738c93`; revision 1
  `logs/decision-D1.md` sha256 `7c3245e7b53867b47ce70750bf96cdb5503e5b298eb95b0490d091bb7e8f3c92`
  (immutable history). Both verified at start and before return.
- Phase `p07`, worktree `/Users/mac02/dev/openbot`, branch `workflows`; phase start `921079d`,
  review base = HEAD `2d608f269761717de7706912bd99ddd325d9d352`; digest
  `31c52a4a554512444396a39b77d8b2b5142231232cf0ee3027a5d4855fc91b21` with exclusions
  `plan/workflow-foundation/phases/p07/docs plan/workflow-foundation/COORDINATION.md marketing website`
  (snapshot.sh at start and before return: identical). `git diff 921079d 2d608f2` touches only
  `plan/`, `marketing/`, `website/` (re-verified), so the code base equals the R001 one and
  R001's code evidence is reusable. Working-tree changes are records only (TASK, COORDINATION,
  `logs/coordination-r2.md`, `logs/decision-D1-r2.md`).
- Gated decision: `P07-D1` rev 2 (D1.0–D1.11, D1.13).
- Coordination: authoritative `plan/workflow-foundation/COORDINATION.md` revision 2, identical to
  immutable `logs/coordination-r2.md` (sha256 `c6b0a521b785a323290142164a46a8699b3da26a80978eddba284d27878326ee`),
  checked at start and before return. Plan-wide instructions: none beyond AGENTS.md/README. No
  notice targets p07. WF-C001 r1 (→ p08) and WF-C002 r1 (→ p09) are proposed by this phase and
  were reviewed as part of the decision (see below).

### Coverage and evidence (inspection only)

- Requirements re-derived from originals: plan 07 (all sections), EXPANSION 07.1–07.4, README
  (sequential graphs, bounded repetition), REPLACEMENT-MAP rows 63, 64, 80, 84, NOT_PLANNED
  (acyclic children; no parallel branches — premise of D1.5's ≤ 1 row), plan 08 §Other entry
  points/§Public interfaces and plan 09 run views (notice targets), TASK acceptance criteria,
  AGENTS.md root/src/application/persistence (infra not material to this gate).
- R001 findings rechecked by ID against code (details in each finding's resolution history):
  F001 (`admission_history.rs`, `history_reader.rs`; only non-test writer/reader of members),
  F002 (`fair_polling.rs` cursor/horizon, `worker.rs` per-candidate logging and unconditional
  sleep, `polling.rs` CANDIDATES_SQL classes, `pending_recovery::activate/settle`,
  `wait_commit::fail`, `recovery::route_parked_failure`), F003 (`workflow_runs` columns, keys and
  FKs, `preserve_workflow_parent` and its `UPDATE OF` trigger, `admission_replay`,
  `inspection::HEAD`, `TriggerSource::Manual`, absence of run deletes), F004 (`admission.rs`,
  `admission_write.rs`, `admission_source.rs`, `check_workflow_parent_source`, every
  `TriggerSource::Child`/`ChildCause` use and helper callers), F005 (`control_retry.rs`,
  `workflow_control_retry_safe`), F006 (`controls.rs`, `maintenance::retire_jobs`,
  `action_uncertainty::record`, `workflow_wait_one_active`/`workflow_wait_execution`, wakeup
  triggers, transaction isolation), F007 dispositions.
- Changed/new parts of rev 2 reviewed as a whole: D1.1 publication capture (S1) against
  `PublicationDirectory`, `AgentPublicationDirectory`, `definitions::verify_children`,
  `publication/dependencies::capture` (missing child → `publication.missing` at
  `/steps/<step>/child_version_id`; ownership/depth/duplicate checks), absence of any production
  `DefinitionService` composition (so WF-C001 is the right carrier), `ChildContract` extension,
  `static_roots` generalization; D1.2 side-effect table and source identity (incl. runs CHECKs
  `deadline > created_at`, bundle ≤ 4 MiB, `read_saved_binding` run-level only, root token policy
  via `insert_workflow_budget_link`); D1.2a; D1.3 rerun schema; D1.4 changed output-mismatch
  handling and polling lookup; D1.5 cascade and lock order; D1.6 round-exhaustion writer; D1.7;
  D1.9 table row by row; D1.10 test plan; D1.11 notices against plans 08/09.
- Assumptions challenged: multiple `child_run` waits per execution vs `waits::row(Execution)`
  (sound: it orders `state='waiting'` first, then newest); per-round audits vs
  `workflow_run_event_execution_kind` uniqueness (sound: rounds audit with `wait_id`, allowed by
  `workflow_run_event_wait_shape`); cascade ≤ 1 row (sound for B1; B2 note F010.2); invariant
  "every live child is named by its parent's waiting wait" (sound given D1.2a, atomic child+wait,
  and that only cancel/deadline/settled-wait paths change a waiting parent); admission-side
  `FOR SHARE` of the rerun original (same authority → run order as the deleted child branch);
  unclassified-error bound (fair-poll cadence + deadline is the established pattern for no-lease
  transitions and satisfies the AGENTS poison rule as tested).
- Checks actually run: `snapshot.sh` (start/end), `shasum -a 256` of decision r1/r2 and
  coordination files, `git diff` scope check. No compile, test or database checks: none are
  meaningful for a decision review and no phase database exists (instructed not to create one).
- Reused from R001 without re-review (code unchanged): child+wait atomicity and keys, the
  `child_run` wait schema (D1.3 first block), deferred bindings/self-reference semantics, D1.6
  receipt scheme, D1.8 template outline, `done` before `exhausted`.
- Lightly reviewed: compiler/registry internals for the `repeat` block (feasibility only, as in
  R001); D1.8 template YAML not yet written; D1.13 only for consistency with B1 schema/ports.

### Outcome

- **Changes required** for P07-D1 rev 2, Low severity only. All R001 actionable findings
  F001–F006 are fixed at decision level and F007 dispositions are accepted (resolution history
  appended). New actionable: WF-P07-F008 (Low — D1.2a move list and B1 focused filters miss
  `budget_activation`, `budget_schema`, `wakeup_guard`, `wakeup_parent_link`, which use the
  binding-based `admit_child`; grandchild cases need a nested publication fixture), WF-P07-F009
  (Low — no planned test of the non-disclosing refusal for an inaccessible rerun original).
  Optional: WF-P07-F010 (five items). No architectural defect remains; a recheck can be limited
  to the D1.2a inventory, D1.10 tests and the TASK focused filters (plus any optional item A
  adopts). WF-C001/WF-C002 correctly carry the cross-phase consequences (p08 owns manual
  starts, delegation and publish routes/composition; p09 owns run lists and inspection views).

### Process and instruction feedback

- Batch/gate size remained reviewable in one pass. The D1.0 resolution table mapping each finding
  to a section made the by-ID recheck direct and is worth keeping as a pattern (evidence: this
  round).
- Defect class this round: inventory/verification-plan omissions, not architecture. F008's cause
  is observable: the move list equals the direct-type grep and misses helper callers. Candidate
  instruction (phase-plan scope): "When removing a creation path, inventory affected tests through
  helper call graphs, not only direct uses of the removed type." F009: a new authorization
  decision without a planned refusal test; existing AGENTS guidance covers it, no new instruction.
- Reviewer continuity: the replacement reviewer could continue from the saved notes/findings
  without the original session; recording code anchors per finding (R001) made reuse safe.

### Freshness at return

- Rechecked before return: HEAD `2d608f2`, digest `31c52a4a…1b21` (same exclusions), decision r2
  sha256 `e5ceea98…8c93`, r1 `7c3245e7…8f3c92`, `COORDINATION.md` = `logs/coordination-r2.md`
  (`c6b0a521…326ee`): all unchanged. During the review the root appended a baseline section to
  TASK "Checks" (unchanged-tree `cargo test --lib workflow`: 1109 passed, 1 load-sensitive
  failure recorded by A as a pre-existing flake) and added `logs/baseline-build.log`,
  `logs/baseline-workflow-tests.log`; these records do not change the decision, requirements or
  the B1 verification plan reviewed here, and were not assessed by this round.

### Follow-up 2026-10-22 (WF-P07-R003)

- R002's WF-P07-F008 evidence traced only callers of `admit_child`. It missed (a) other
  wrappers of the removed path reached from eight more files (nested eligibility modules,
  `tokens::scheduling`, `tokens::updates`), (b) that the TASK filters name files rather than
  test module paths (several select zero tests), and (c) that the runtime chain fixture cannot
  reproduce concurrent-descendant or child-of-active-execution preconditions. Rev 3 fixed exactly
  what R002 asked for; the remainder is recorded on F008 in R003. The R002 outcome stands.

## WF-P07-R003 — correction recheck of R002, P07-D1 rev 3 (batch B1) — IN PROGRESS

- Started 2026-10-22 (same reviewer B session as R002). Identity verified at start: HEAD
  `2d608f2`, digest `31c52a4a…1b21` (same exclusions), `logs/decision-D1-r3.md` sha256
  `96d201af8768e57a05b5796c1e4bd07d1370ddd914d554900eaa127b42cfe0b6`, r2 `e5ceea98…8c93`,
  `COORDINATION.md` = `logs/coordination-r2.md` (`c6b0a521…326ee`). Delta derived independently
  with `diff -u decision-D1-r2.md decision-D1-r3.md` (author's `logs/decision-D1-r2-to-r3.diff`
  is the same delta in normal-diff format). The completed entry follows below.

## WF-P07-R003 — correction recheck of R002, P07-D1 rev 3 (batch B1) — CHANGES REQUIRED (Low only)

### Identity

- Date 2026-10-22. Workflow `implement-opus-reviewed-2_2`. Stage: architecture-gate correction
  recheck of WF-P07-R002 (F008, F009), review of the adopted F010 items and of the author's D1.7
  refinement. Reviewer B: Claude Opus 5.5 (`claude-opus-5-5`), same session-2 reviewer as R002
  (opus/xhigh per TASK); no agents spawned; `graft` unavailable.
- Decision `logs/decision-D1-r3.md` revision 3, sha256
  `96d201af8768e57a05b5796c1e4bd07d1370ddd914d554900eaa127b42cfe0b6` (verified at start and before
  return); r2 `e5ceea98…8c93` unchanged. Delta derived independently with `diff -u` (scratchpad
  copy, not retained); the author's `logs/decision-D1-r2-to-r3.diff` is the same delta in
  normal-diff format.
- Phase `p07`, worktree `/Users/mac02/dev/openbot`, branch `workflows`; review base = HEAD
  `2d608f269761717de7706912bd99ddd325d9d352`; digest
  `31c52a4a554512444396a39b77d8b2b5142231232cf0ee3027a5d4855fc91b21` (exclusions
  `plan/workflow-foundation/phases/p07/docs plan/workflow-foundation/COORDINATION.md marketing website`),
  identical at start and before return. No source edits.
- Coordination: `COORDINATION.md` r2 = `logs/coordination-r2.md` (`c6b0a521…326ee`), no notice
  targets p07; WF-C001/WF-C002 r1 text unchanged and still consistent with rev 3 (rev 3 adds no
  cross-phase consequence: the deferred constraint and the D1.7 relocation are p07-internal).
- Gated decision: `P07-D1` rev 3.

### Coverage and evidence (inspection; one read-only log analysis)

- F009: D1.10 rerun cases checked; feasibility verified in `association.rs:6-51` and
  `workflow/mod.rs:68-70` (denial and missing row both `NotFound("Workflow resource")`, database
  errors propagate).
- F010.1–.5: each adopted text checked against the wakeup triggers, `workflow_waits`
  delete/`SET CONSTRAINTS` usage (none in non-test code; two SQL fixtures without run inserts),
  immutable parent links, raw-SQL negative inserts, and the publication dependency limit.
- D1.7 refinement (checks only in `admission_source::authorize`, also on replay): order
  authority → new-run association → original `FOR SHARE` → original association matches the
  deleted phase-03 child branch (`admission_source.rs:87-121`), so it introduces no new lock
  order; `channels` are only locked `FOR SHARE` there and no non-workflow code touches
  `workflow_runs` except `model_usage`; replay re-authorization matches the existing "authorize
  every admission/replay" contract; view-level association authorization is the right test for
  linking. Wording overstatement → optional F011.
- F008 recheck: recomputed the transitive inventory of tests reaching the removed path (every
  test-file function constructing `TriggerSource::Child`/`ChildCause::Execution`, every wrapper,
  every cross-file and nested-module caller via `#[path]` mounts); mapped TASK filters to real
  test paths using the 856 adapter test paths in `logs/baseline-workflow-tests.log` (read-only;
  no build or database used, to avoid contention with the root's target dir and cluster); read
  the setup of the competing-descendant, sibling, token and claim cases.
- Not re-reviewed (unchanged since R002): all other rev 2 content, which R002 covered.

### Outcome

- **Changes required**, Low only. WF-P07-F009 fixed; WF-P07-F010 adopted (closed). WF-P07-F008
  remains open (Low): (a) eight more files reach the removed path through other wrappers,
  (b) TASK filters name files, and several select zero tests (complete module-path filter list
  with counts recorded in the finding), (c) D1.10's "every file runs on the runtime-child
  fixture" is unachievable for cases needing concurrently live descendants or a child of a
  non-parked execution; a per-case disposition rule is needed. New optional: WF-P07-F011.
- No architectural defect remains in rev 3. A recheck can be limited to D1.2a, D1.10's
  moved-fixture text and TASK's filters.

### Process and instruction feedback

- Reviewer miss, recorded with evidence: R002 asked for an incomplete fix (traced one helper
  only), causing an extra round. Candidate instruction (reviewer scope): "When a finding asks for
  an inventory, compute the complete transitive set (wrappers, nested `#[path]` modules) and the
  exact runnable filters in the finding itself, so the correction can be done in one round."
- Candidate (phase-plan scope): "Name focused test filters by real test paths and record the
  selected-test count per filter; a filter that selects zero tests is a failed check, not a
  pass." Evidence: every prefixed TASK filter selects 0 tests in the baseline log.
- Candidate (phase-plan scope): "When a decision makes a state unreachable, give existing tests
  that construct it an explicit disposition (rebuild through the supported writer, move to the
  phase that makes it reachable, or retire with a reason)." Evidence: F008 (c).

### Freshness at return

- Rechecked before return: HEAD, digest, decision r3/r2 sha256 and coordination identity
  unchanged (values above).

## WF-P07-R004 — correction recheck of R003, P07-D1 rev 4 (batch B1) — IN PROGRESS

- Started 2026-10-22 (same reviewer B session). Identity verified at start: HEAD `2d608f2`, digest
  `31c52a4a…1b21` (same exclusions), `logs/decision-D1-r4.md` sha256
  `5c076606580e887de3d12203fa31e3a1d3a73b1112b169840b56938698a4adc4`, r3 `96d201af…e0b6`,
  `COORDINATION.md` = `logs/coordination-r2.md` (`c6b0a521…326ee`). Delta derived independently
  with `diff -u decision-D1-r3.md decision-D1-r4.md`. The completed entry follows below.

## WF-P07-R004 — correction recheck of R003, P07-D1 rev 4 (batch B1) — PASS

### Identity

- Date 2026-10-22. Workflow `implement-opus-reviewed-2_2`. Stage: architecture-gate correction
  recheck of WF-P07-R003 (F008 a/b/c, optional F011 adopted). Reviewer B: Claude Opus 5.5
  (`claude-opus-5-5`), same session-2 reviewer (opus/xhigh per TASK); no agents spawned.
- Decision `logs/decision-D1-r4.md` revision 4, sha256
  `5c076606580e887de3d12203fa31e3a1d3a73b1112b169840b56938698a4adc4` (verified at start and before
  return); r3 `96d201af…e0b6` unchanged. Delta derived independently with `diff -u`.
- Phase `p07`, worktree `/Users/mac02/dev/openbot`, branch `workflows`; review base = HEAD
  `2d608f269761717de7706912bd99ddd325d9d352`; digest
  `31c52a4a554512444396a39b77d8b2b5142231232cf0ee3027a5d4855fc91b21` (exclusions
  `plan/workflow-foundation/phases/p07/docs plan/workflow-foundation/COORDINATION.md marketing website`),
  identical at start and before return. No source edits.
- Coordination: `COORDINATION.md` r2 = `logs/coordination-r2.md` (`c6b0a521…326ee`), no notice
  targets p07; WF-C001/WF-C002 r1 remain correct for rev 4 (rev 4 changes are p07-internal: a
  test-support helper and a rerun trigger). They may be activated on this pass.

### Coverage and evidence

- F008 (a): compared the D1.2a inventory with my R003 transitive set (constructors, seven
  wrappers, cross-file and nested-module callers): identical, 24 persistence + 2 application
  files ("18" in the text is a miscount).
- F008 (b): re-derived every TASK filter count from the 1111 unique test paths in
  `logs/baseline-workflow-tests.log` (read-only): all 14 match.
- F008 (c): A/B/C rule and per-test record checked; every cited test name exists in the baseline.
  Category-B claim verified by tracing every Rust and schema reader of `workflow_waits`
  (including `fairness.rs::VALID_HINT`, waiting-only; `waits::row(Execution)` callers
  `park_on`/`resume_on`; `select_due`, CANDIDATES_SQL, cancel/expire/decision/effect updates;
  schema triggers and functions at `init_schema.sql:215-290, 3300-3335, 420-440, 14330-14540`)
  and the table CHECKs. Holds on all normal paths; caveats in optional F012.
- F011: trigger placement and immutability argument checked against `preserve_workflow_parent`
  and the D1.3 immutability of `rerun_of_run_id`.
- Not re-reviewed: rev 3 content unchanged by rev 4 (covered in R002/R003).
- Checks run: snapshot.sh, sha256, `diff -u`, read-only log analysis. No build, tests or
  database (decision review; root's cluster and target dir left untouched).

### Outcome

- **PASS for P07-D1 revision 4** (sha256 above), at HEAD `2d608f2`, digest `31c52a4a…1b21`.
  All actionable findings are resolved at decision level: F001–F006 (R002), F009 (R003), F008
  (R004). Optional F007, F010, F011 dispositions accepted/adopted. New optional WF-P07-F012
  (category-B helper caveats and a miscount) does not block.
- Carried to the B1 batch review (implementation evidence still required): every D1.10 test,
  including the F001 late-message, F002 poison, F006 competing-cancel, F009 refusal, deferred
  constraint and rerun-trigger tests; `logs/b1-fixture-moves.md` accounting for every affected
  baseline test with its category; filter counts per run.

### Process and instruction feedback

- Rounds R002–R004 were spent on test-inventory precision rather than architecture. Root cause
  observed: inventories derived from a type-name grep (rev 2) and from the reviewer's partial
  evidence (R002). Candidates already recorded in R003 (complete transitive inventory in the
  finding; filters by real test paths with counts; disposition for tests of unreachable states).
- The author's practice of stating an unverified claim and asking the reviewer to check it
  (fairness.rs) was useful and should be kept.

### Freshness at return

- Rechecked before return: HEAD, digest, decision r4/r3 sha256 and coordination identity
  unchanged (values above).

## WF-P07-R005 — B1 batch review against P07-D1 rev 4 — IN PROGRESS

- Started 2026-10-22 (same reviewer B session). Identity verified at start: HEAD `2d608f2`
  (no B1 commits; batch = working-tree delta incl. 11 untracked source files), digest
  `1e1b0a032b4d93b16d2a09774ab5a13cb925741860d16358f334f2698f5ec9e4` (exclusions
  `plan/workflow-foundation/phases/p07/docs plan/workflow-foundation/COORDINATION.md marketing website`),
  `COORDINATION.md` r3 sha256 `0c58cfd3…4521a` = `logs/coordination-r3.md`, gate decision r4
  `5c076606…adc4`. Completed entry follows below.

## WF-P07-R005 — B1 batch review against P07-D1 rev 4 — CHANGES REQUIRED

### Identity

- Date 2026-10-22. Workflow `implement-opus-reviewed-2_2`. Stage: batch review of B1 (child
  calls, bounded repeat, rerun link, child publication capture, single child-creation path,
  fixture moves). Reviewer B: Claude Opus 5.5 (`claude-opus-5-5`), same session-2 reviewer
  (opus/xhigh per TASK); no agents spawned; skill files re-read (unchanged since `ef6bc98`).
- Phase `p07`, worktree `/Users/mac02/dev/openbot`, branch `workflows`; phase start `921079d`;
  review base = HEAD `2d608f269761717de7706912bd99ddd325d9d352`; no B1 commits — the batch is the
  working-tree delta (78 modified files incl. the init migration, 11 untracked source files;
  `logs/b1-snapshot.txt`). Digest `1e1b0a032b4d93b16d2a09774ab5a13cb925741860d16358f334f2698f5ec9e4`
  with exclusions `plan/workflow-foundation/phases/p07/docs plan/workflow-foundation/COORDINATION.md marketing website`,
  identical at start and before return. `marketing/`, `website/` not reviewed (user's areas).
- Gate: P07-D1 rev 4 (`logs/decision-D1-r4.md`, `5c076606…adc4`, passed R004, F012 adopted).
- Coordination: `COORDINATION.md` r3, sha256 `0c58cfd32bdf10f3a4cdcae6970792d07e885278ab8b306e3e1bbd1bb0b4521a`
  = `logs/coordination-r3.md`; WF-C001 r1 (→ p08) and WF-C002 r1 (→ p09) active, none target p07;
  rechecked before return.

### Coverage and evidence

- Requirements: plan 07 (all), TASK criteria 1–8 (criterion 3 is B2), D1.0–D1.13 rev 4, AGENTS.md
  root/src/persistence.
- Production code read in full or as complete diffs: migration (all new constraints, triggers,
  functions); domain `causality`; compiler `wire`/`analysis`/`compile`; registry
  `families`/`facts`/`mod`/`help`; `publication/dependencies`; application `children`, `waits`,
  `worker`, `service`, `contracts`, `activation`; persistence `children`, `child_waits`,
  `child_publication`, `wait_commit`, `waits`, `polling`, `budget::repetition`, `controls`
  (`cancel_tree_on`), `control_retry`, `admission`, `admission_write`, `admission_source`
  (`rerun`), `admission_replay`, `admission_binding`, `binding_rows`, `inspection`,
  `activation` (`round_context`); fixtures (three YAML templates, `fixtures/mod.rs`, tests).
- Checked against D1: park order (lock → replay → state/deadline → step kind → activate → child
  config + input before `round:1` → `create_on` → finish/park/audit); single writer and no
  history capture; inherited absolute deadline; source key / inspection 6-element shape and DB
  check; deferred `workflow_child_parent_wait`; resume via `terminal_wakeup_sequence` PK; exit
  before bound; invalid round → Terminal `workflow.invalid_input` routed; round budget
  exhaustion persisted on the wait (deviation 1); cancel cascade over every waiting child wait,
  parent-first locks, boxed recursion; retry refusal; rerun checks only in the admission
  transaction (non-disclosing order, DB errors propagate); `insert_run` bind order ($18–$22);
  publication cap/batched reads/byte bound.
- Author deviations 1–7 judged acceptable: (1) wait `failed/root_budget_exhausted` makes replay
  answer the same unrouted failure; (2) deferred `workflow_wait_child_link` still enforced at
  commit; (3) `BadRequest` for directory limits matches `AgentPublicationDirectory`; (4)/(7) test
  composition includes the production directory chain; (5) category-B reasoning sound; (6)
  memory store mirrors SQL refusal.
- Tests read: `child_runtime_tests`, `child_repeat_tests`, `child_schema_tests`,
  `child_publication_tests` (template e2e), `rerun_tests`, `child_test_support`
  (`lineage_child` implements the F012 contract), control race tests, and the
  assertion-count delta of every moved test file (removals are setup-only or category-C
  replacements); `logs/b1-fixture-moves.md` (60 tests) spot-checked.
- Checks I ran (phase cluster `127.0.0.1:55470`, `SQLX_OFFLINE=true CARGO_INCREMENTAL=0`, `-j 4`;
  the build was already current, so the binary matches the reviewed tree):
  - `logs/r005-focused.log`: child_runtime 11, child_repeat 4, child_schema 2,
    child_publication 3, rerun 4, `lease_tests::control_tests::` 37, admission_source 5,
    admission_token 4, activation 17, causality 9, fixtures 12, registry 32, compiler 33 —
    **172 passed, 0 failed**.
  - `logs/r005-moved-and-races.log`: category-B/moved modules `sibling_tests::eligibility_tests::`
    + `model_owner_tests::store::tokens::` + `batch_tests::wakeup_tests::` — **108 passed**
    (baseline counts 53/25/30); five repetitions of the 17 competing-claimant tests matching
    `competes_with|competing_parkers|success_completes_parent_once|both_deterministic_orders` —
    **5 × 17 passed, 0 failed**.
  - Inspected author logs: clippy exit 0, workflow selection 1137/0, stack budget 102/0, fresh
    migration exit 0 with the new deferred constraints present. Not re-run by me: fmt, clippy,
    full `workflow` selection, stack budget. `cargo sqlx prepare --check` unavailable (no
    sqlx-cli); the author's evidence (no compile-time macro touches a workflow table, `.sqlx`
    unchanged) is consistent with the diff — recorded as an unavailable check, not a pass.
- Lightly reviewed: per-test content of the 60 moved tests beyond the assertion-delta check and
  sampled diffs; the template YAML semantics beyond routing and mappings.

### Outcome

- **Changes required.** No correctness, concurrency or authorization defect found in the
  production code; the gate's invariants are implemented as decided. Actionable:
  WF-P07-F013 (Medium, template escalation lacks specialist/artifact/feedback, unrecorded D1.8
  deviation), WF-P07-F014 (Medium, child params/resource restriction untested — criterion 1),
  WF-P07-F015 (Medium, decision-round acceptance only via `wait.event` stand-ins; no "no replay"
  check), WF-P07-F016 (Low, invalid child output and real cross-tenant schema rows untested).
  Optional: WF-P07-F017.
- A recheck can reuse this round's code review where unchanged and should cover the template,
  the new tests and any code they touch.

### Process and instruction feedback

- Batch size was large but reviewable; the per-test fixture-moves table and the per-filter
  counts made the moved-fixture part fast to verify. The deviation list was useful.
- Defect class: coverage gaps against an explicit D1.10 list (params/resources, invalid output,
  decision rounds) and one template route. Candidate (phase-plan scope): "Before requesting batch
  review, check off each D1.10/verification-plan item against a named test, and list any
  stand-in with what it does not prove."

## WF-P07-R006 — B1 correction recheck of R005 (F013–F017) — IN PROGRESS

- Started 2026-10-22 (same reviewer B session). Target digest
  `d82fa02802bfd4c2056f89ffa737282cdbb3573e8364261088c6207d2a1b5c11` at HEAD `2d608f2` (same
  exclusions); gate rev 4 and coordination r3 unchanged. Completed entry follows below.

## WF-P07-R006 — B1 correction recheck of R005 (F013–F017) — PASS

### Identity

- Date 2026-10-22. Workflow `implement-opus-reviewed-2_2`. Stage: correction recheck of batch
  review WF-P07-R005 (batch B1). Reviewer B: Claude Opus 5.5 (`claude-opus-5-5`), same
  session-2 reviewer (opus/xhigh per TASK); no agents spawned.
- Phase `p07`, worktree `/Users/mac02/dev/openbot`, branch `workflows`; review base = HEAD
  `2d608f269761717de7706912bd99ddd325d9d352` + working tree; digest
  `d82fa02802bfd4c2056f89ffa737282cdbb3573e8364261088c6207d2a1b5c11` (exclusions
  `plan/workflow-foundation/phases/p07/docs plan/workflow-foundation/COORDINATION.md marketing website`;
  file list `logs/r006-snapshot.txt`), identical at start and before return. Gate P07-D1 rev 4
  (`5c076606…adc4`) and `COORDINATION.md` r3 (`0c58cfd3…4521a`, WF-C001/C002 active, none target
  p07) unchanged.
- Correction delta: the snapshot lists only names, so the delta was derived from files newer than
  `logs/b1-snapshot.txt` plus names new to `logs/r006-snapshot.txt` (13 files): production
  `registry/families.rs` (`default_params`), `fixtures/repeated-human-revision.yaml`,
  `fixtures/mod.rs`; tests `fixtures/tests.rs`, `registry/runtime_tests.rs`, `batch_tests.rs`,
  `binding_tests.rs`, `child_test_support.rs`, `rerun_tests.rs`, `child_schema_tests.rs`, new
  `child_configuration_tests.rs`, `child_decision_round_tests.rs`, `child_output_tests.rs`.
  Matches TASK "B1 corrections for WF-P07-R005". (Approximation: an edit preserving an older
  mtime would be missed; none is suggested by the TASK record.)

### Coverage and evidence

- Each finding rechecked by ID against the changed files (details in findings history): F013
  template routes and specialist mapping; F014 registry test + runtime configuration tests; F015
  decision-round test (read in full); F016 output tests and real-row FK tests; F017.1
  `default_params` code; F017.2 decline reason.
- Deviation 8 (params the child refuses fail activation unrouted): **accepted**. The step's
  `params` contract embeds the child's parameter schema, so the value is refused while freezing
  inputs, before activation; `pending_recovery::settle` routes only activated executions (no
  frozen inputs can be fabricated for a successor), which is the established behaviour of every
  step kind. The gated D1.9 row assumed routing there; the post-activation child refusal that
  remains reachable (child context limit) still routes (`workflow_call_child_limits_reject_input_at_park_and_route_it`),
  and F017.1 moves the omitted-params case to publication. The asserting test exists.
- Checks I ran (phase cluster, `SQLX_OFFLINE=true CARGO_INCREMENTAL=0`, `-j 4`; build current),
  `logs/r006-review-checks.log`:
  - `batch_tests::child_` (runtime 11, repeat 4, schema 2, configuration 2, decision_round 1,
    output 2), `child_publication_tests::` 3, `rerun_tests::` 4, `application::workflow::fixtures::`
    13, `registry::` 33, `compiler::` 33, `lease_tests::control_tests::` 37 — **145 passed, 0 failed**.
  - `control_serialization_tests::` six times — **6 × 6 passed**.
- Author's full-selection failure (`logs/r006-workflow-tests.log`, 1143 passed / 1 failed):
  `control_serialization_tests::workflow_action_reconciliation_control_serialization_command_before_expire`
  panicked at `action_reconciliation_control_serialization_lock_support.rs:284` ("both owners and
  observer drained without active transactions or queries", a `pg_stat_activity` quiescence
  check on pooled backends). Judged **confirmed unrelated and load-sensitive**: the failing
  variant is `Terminal::Expire` (`maintenance::expire_on`) with a reconciliation command, and
  `maintenance.rs`, `recovery.rs`, `lease.rs`, `action_uncertainty.rs`, the action-reconciliation
  module and the test's lock support are unchanged by B1 (git status); it passed in the R005 B1
  run (1137/0) and at baseline, passed 4/4 alone and 241/241 in its group for the author, and
  6/6 module runs for me, while another project's release test run loaded this machine. It is
  reported separately, not as part of a clean full run.
- Inspected author logs: fmt 0, clippy 0, stack budget 108/0.
- Not re-reviewed: B1 production code unchanged since R005 (reused R005 review).

### Outcome

- **PASS** for B1 at digest `d82fa02802bfd4c2056f89ffa737282cdbb3573e8364261088c6207d2a1b5c11`.
  F013, F014, F015, F016 fixed; F017 closed (item 1 adopted, item 2 declined with accepted
  reason); deviation 8 accepted. No open actionable B1 findings. Outstanding beyond this batch:
  `cargo sqlx prepare --check` remains unavailable (recorded, not passed); the full suite and the
  final phase gate follow B2 per TASK.

### Process and instruction feedback

- The correction round was complete and well-documented (per-finding test names, a stated
  deviation with an asserting test, and a quantified judgement of the unrelated failure); it
  needed one recheck, as the skill intends.
- Candidate (skill scope): "Snapshot files should record a per-file hash so a recheck can derive
  the correction delta exactly instead of from mtimes." Evidence: this round's delta derivation.

## WF-P07-R007 — B2 architecture gate, P07-D2 rev 1 — IN PROGRESS

- Started 2026-10-22 (same reviewer B session). Decision `logs/decision-D2.md` rev 1, target sha256
  `1e11108ac016322ab1be1d293e1d32ca7bc97c29c953936d42918142cd50ff27`; baseline B1 digest
  `d82fa028…5c11`; coordination r3 `0c58cfd3…4521a`. Completed entry follows below.

## WF-P07-R007 — B2 architecture gate, P07-D2 rev 1 — CHANGES REQUIRED

### Identity

- Date 2026-10-22. Workflow `implement-opus-reviewed-2_2`. Stage: architecture gate for batch B2
  (agent workflow tools). Reviewer B: Claude Opus 5.5 (`claude-opus-5-5`), same session-2
  reviewer (opus/xhigh per TASK); no agents spawned.
- Decision `logs/decision-D2.md` revision 1, sha256
  `1e11108ac016322ab1be1d293e1d32ca7bc97c29c953936d42918142cd50ff27` (verified at start and before
  return). Baseline: B1 accepted in phase, HEAD `2d608f2` + working tree, digest
  `d82fa02802bfd4c2056f89ffa737282cdbb3573e8364261088c6207d2a1b5c11` (same exclusions), unchanged
  at start and before return. No B2 code exists.
- Coordination: `COORDINATION.md` r3 (`0c58cfd3…4521a`) unchanged; proposed WF-C003 → p09 and
  WF-C001 r2 → p08 reviewed as part of the decision.

### Coverage and evidence (inspection only)

- Requirements: plan 07 §Child calls and workflow tools (last paragraph) and §Acceptance; TASK
  criterion 3, B2 row; P07-Q1 (user product decision and company-test requirement); plan 09
  §Authoring/§Runs (WF-C003 target); AGENTS.md root/src/persistence.
- Code claims verified: `ModelToolCallId` is the stringified harness `InvocationId` saved by
  `PrepareInvocation` before dispatch, and `pending_call` replays it (`harness/rig/workflow.rs:
  175-215`, `runs/state.rs:440`); `AgentActionTools`/`AgentToolDispatch` routing
  (`workflow_agent/mod.rs`); `granted_tool` order (`agent_publication.rs:261-309`);
  `supported_publication` walks the frozen closure (`authority.rs:94-128`); `current_on` and
  `targets` (`agent_preparation.rs`); `authorize_company` = owner/admin; message park/continue
  (`message_publish.rs:262-272`, `wait_commit::continue_delivery`, `reopen_job` ≤ 63);
  `check_workflow_fenced_completion` arms and its deferred trigger; continuation counter and
  model-tool continuation guards; `confirm_suspension` and supervisor handling; approval source
  (registration flag + `agent.run` `approval` reviewers; no effect-class policy); marker-owned
  kinds; `agents.workflow_tools` has no route; every reader of `contract.connection`.
- Challenges answered: (1) bypassing ActionService is sound — the call is never uncertain
  (child + park in one fenced transaction), replay is idempotent by the saved call, approval is a
  per-registration flag so no author policy is bypassed, reconciliation needs no marker;
  rationale should be written down (F023.1); sample execution correctly delegated (WF-C003).
  (2) park/reopen arms and continuation guard mirror the effect/approval arms and the deferred
  commit-time lease check; consistent. (3) identity keyed by the saved invocation UUID is stable
  across continuation, retry and restart; unique per execution. (4) typed error envelopes match
  the existing `unknown_tool` shape and the plan's "typed results or errors … never fabricated
  success"; all are deterministic on replay; invalid child output correctly fails the step.
  (5) registration authority fine; revocation/re-pin semantics → F018, dispatch recheck → F020.
  (6) verification plan → F019 (races/recovery), plus readiness (F021) and ambiguity (F022) tests.
- Not covered: Rig provider/tool declaration internals beyond the dispatch seam; the exact SQL of
  the new arms (to be reviewed at batch review).

### Outcome

- **Changes required.** Actionable: WF-P07-F018 (Medium, re-pin/archive semantics vs P07-Q1),
  WF-P07-F019 (Medium, competing-claimant and recovery tests), WF-P07-F020 (Low, no dispatch-time
  registration recheck), WF-P07-F021 (Low, `connection: None` readers incl. admission readiness),
  WF-P07-F022 (Low, MCP name shadowing). Optional: WF-P07-F023. The core runtime design
  (marker-free child as the record, saved-call identity, park/continue arms, typed errors) is
  sound; a revision can be rechecked against these items.

### Process and instruction feedback

- The author's explicit list of B1 contracts changed by D2 and the challenge list made the gate
  efficient. The code map was accurate in every claim I checked; its one gap (readiness as
  another HTTP-by-default reader) is the overloaded-field pattern recorded in F021.

## WF-P07-R008 — B2 gate correction recheck, P07-D2 rev 2 — IN PROGRESS

- Started 2026-10-22 (same reviewer B session). Target `logs/decision-D2-r2.md` sha256
  `46301f5ec1a6f67a8c6a9cd11f1eff98b0225d9a595b330561fecfdba2143451`; rev 1 `1e11108a…ff27`;
  baseline B1 digest `d82fa028…5c11`; coordination r3. Completed entry follows below.

## WF-P07-R008 — B2 gate correction recheck, P07-D2 rev 2 — completed entry

- Context: date 2026-10-22; workflow `implement-opus-reviewed-2_2`; task `TASK.md`, review
  `REVIEW.md`; batch B2; stage architecture-gate correction recheck; reviewer Claude Opus 5.5
  (`claude-opus-5-5`), reviewer B (session 2); phase `p07`; worktree `/Users/mac02/dev/openbot`,
  branch `workflows`, HEAD `2d608f269761717de7706912bd99ddd325d9d352`.
- Identity: `logs/decision-D2-r2.md` sha256
  `46301f5ec1a6f67a8c6a9cd11f1eff98b0225d9a595b330561fecfdba2143451` (read in full; delta
  `logs/decision-D2-r1-to-r2.diff`); rev 1 `logs/decision-D2.md` `1e11108a…ff27` unchanged;
  baseline B1 snapshot digest `d82fa02802bfd4c2056f89ffa737282cdbb3573e8364261088c6207d2a1b5c11`
  (same exclusions); `COORDINATION.md` r3 sha256 `0c58cfd3…4521a` unchanged. Rechecked at the
  start and before return.
- Requirements re-derived from: plan `07-child-workflows-and-review-revisions.md` (agent workflow
  tools), TASK.md criteria and the user decisions P07-Q1 and the new P07-Q2 ("revoke stops,
  re-pin doesn't"; alternatives fully-frozen and any-change-stops declined).
- Coverage (by finding ID):
  - F018 (Medium): fixed. Two `registered_on` modes match P07-Q2 sentence by sentence: re-pin
    and archive leave published workflows on the frozen pin (Run mode needs only the name),
    republish captures the new pin (Publish mode, exact pin and selectable), revoke is the kill
    switch for new runs and new calls (`workflow.tool_revoked`), a parked call still delivers
    (dispatch step 2 precedes step 3). Preparation checks the agent grant by name only.
  - F019 (Medium): fixed. All four competing-claimant/recovery groups are in D2.7, with both
    orders and a racing variant where applicable, and a reopened attempt failing before saving.
  - F020 (Low): fixed. Registration `FOR SHARE` at step 0 before any run lock; revocation takes
    no run lock, so no lock-order cycle with cancel/lease expiry (which serialize on parent
    locks, D2.4).
  - F021 (Low): fixed. `ToolKind` classifier matched exhaustively at the five readers; the
    remaining name-keyed HTTP/message lookups are reached only after classification and are
    safe with unique tool names.
  - F022 (Low): fixed. Ambiguity refusal at capture step 1, tested.
  - F023 (Optional): all four items adopted; closed.
- New issues introduced by rev 2:
  - WF-P07-F024 (Low, actionable): the Publish mode is placed "in
    `DefinitionPersistence::publish`", where `supported_publication` walks the whole closure,
    stored child bundles included (`authority.rs:94-128`). Applied to children, it would refuse a
    new parent that pins an existing child whose own workflow tool was re-pinned or archived —
    contrary to P07-Q2 and to the existing rule that stored child bundles are not re-derived at
    publish (`agent_publication.rs:149-150`). Needs one sentence of scope and a test.
  - WF-P07-F025 (Optional): "Re-registering the same name re-enables them" lets a name reused
    for a different workflow revive revoked frozen pins; key Run mode on name + workflow, or
    record it for p09's UI.
- Checks: decision-level recheck; no code changed since R006 (digest unchanged), so no tests run
  this round. Read-only source checks: `authority.rs:90-128`, `agent_publication.rs:145-152`,
  callers of `supported_publication` (`definitions.rs:192`, `admission_replay.rs:92`,
  `bindings.rs:117`).
- Outcome: **changes required (Low only)** — F024. F025 optional. F018–F022 fixed, F023 closed.
  If F024 is fixed by stating the scope (Publish mode for the root's own snapshots, Run mode for
  stored children) and adding the test, I expect to pass rev 3 on a short recheck.
- Process feedback: the resolution table and the self-contained revision with a diff made the
  recheck fast; quoting P07-Q2 next to each mode made F018 directly checkable.

## WF-P07-R009 — B2 gate correction recheck, P07-D2 rev 3 — IN PROGRESS

- Started 2026-10-22 (same reviewer B session). Target `logs/decision-D2-r3.md` sha256
  `b13f51e92d98a69d4f83dd30c7d321eb91b991386940a04e66350afedd0ab335`; rev 2 `46301f5e…3451`;
  baseline B1 digest `d82fa028…5c11`; coordination r3. Completed entry follows below.

## WF-P07-R009 — B2 gate correction recheck, P07-D2 rev 3 — completed entry

- Context: date 2026-10-22; workflow `implement-opus-reviewed-2_2`; task `TASK.md`, review
  `REVIEW.md`; batch B2; stage architecture-gate correction recheck; reviewer Claude Opus 5.5
  (`claude-opus-5-5`), reviewer B (session 2); phase `p07`; worktree `/Users/mac02/dev/openbot`,
  branch `workflows`, HEAD `2d608f269761717de7706912bd99ddd325d9d352`.
- Identity: `logs/decision-D2-r3.md` sha256
  `b13f51e92d98a69d4f83dd30c7d321eb91b991386940a04e66350afedd0ab335` (delta
  `logs/decision-D2-r2-to-r3.diff` read in full, plus the unchanged D2.0 and D2.3 dispatch and
  the D2.7 test list in rev 3); rev 2 `46301f5e…3451` unchanged; baseline B1 digest
  `d82fa02802bfd4c2056f89ffa737282cdbb3573e8364261088c6207d2a1b5c11`; `COORDINATION.md` r3
  `0c58cfd3…4521a` unchanged. Rechecked at the start and before return.
- Coverage (by finding ID):
  - F024 (Low): fixed. Publish mode scoped to the root's freshly captured snapshots; stored
    children (step-control and tool children alike) in Run mode, per bundle in
    `supported_publication`, matching `verify_on`; both re-pinned and revoked child cases tested.
    Consistent with P07-Q2: a new parent can reuse an old child frozen on an older tool pin.
  - F025 (Optional): adopted; Run mode keys on name + `workflow_id`; closed.
- New issue introduced by rev 3:
  - WF-P07-F026 (Low, actionable): D2.3 dispatch step 3 still refuses only when no registration
    of the name exists, so after a name is re-pointed to another workflow a running agent keeps
    calling the old workflow while admissions are refused — inconsistent with D2.2's Run-mode
    scope ("dispatch (D2.3)") and with "for old pins it acts as a revocation". The F025 test
    covers admissions only. One-sentence fix plus one test.
- Other checks of the change: Publish mode still implies Run mode for the root (exact pin
  ⊇ same workflow); a closure where a stored child freezes `T → W1` and the new root captures
  `T → W2` is refused at publish (child in Run mode), which is the stated "acts as a revocation"
  behaviour; archive is ignored by Run mode as P07-Q2 requires. No further issues.
- Checks: decision-level recheck; code unchanged since R006 (digest unchanged), no tests run.
- Outcome: **changes required (Low only)** — F026. F024 fixed, F025 adopted and closed. A rev 4
  that aligns D2.3 step 3 with Run mode and adds the running-agent re-point test should pass on a
  short recheck.
- Process feedback: when a rule changes in one section, the diff should also touch every
  protocol step that applies it (here D2.3 step 3); a quick grep for the old wording ("no
  registration") would have caught it.

## WF-P07-R010 — B2 gate correction recheck, P07-D2 rev 4 — IN PROGRESS

- Started 2026-10-22 (same reviewer B session). Target `logs/decision-D2-r4.md` sha256
  `ebf212db284e9a6e128d48d5ac8d97f57d45821db7e3e78c3dc096fc5e2dfab7`; rev 3 `b13f51e9…b335`;
  baseline B1 digest `d82fa028…5c11`; coordination r3. Completed entry follows below.

## WF-P07-R010 — B2 gate correction recheck, P07-D2 rev 4 — completed entry

- Context: date 2026-10-22; workflow `implement-opus-reviewed-2_2`; task `TASK.md`, review
  `REVIEW.md`; batch B2; stage architecture-gate correction recheck; reviewer Claude Opus 5.5
  (`claude-opus-5-5`), reviewer B (session 2); phase `p07`; worktree `/Users/mac02/dev/openbot`,
  branch `workflows`, HEAD `2d608f269761717de7706912bd99ddd325d9d352`.
- Identity: `logs/decision-D2-r4.md` sha256
  `ebf212db284e9a6e128d48d5ac8d97f57d45821db7e3e78c3dc096fc5e2dfab7` (delta
  `logs/decision-D2-r3-to-r4.diff` read in full; rev 4 grepped for leftover "no registration" /
  name-only wording — none); rev 3 `b13f51e9…b335` unchanged; baseline B1 digest
  `d82fa02802bfd4c2056f89ffa737282cdbb3573e8364261088c6207d2a1b5c11`; `COORDINATION.md` r3
  `0c58cfd3…4521a` unchanged. Rechecked at the start and before return.
- Coverage: F026 (Low) fixed. D2.3 step 3 now applies the D2.2 Run-mode rule (name and
  `workflow_id`) to the registration read at step 0 before any run lock; revoked or re-pointed →
  `workflow.tool_revoked`, nothing created; same workflow at another version or params passes and
  runs the frozen pin (P07-Q2). Step 2 still precedes step 3, so a parked call delivers after a
  revocation or re-point. Lock order unchanged (a re-point, like a revocation, takes only the
  company lock and the registration row). D2.4, D2.6 and D2.7 are consistent with it, and the
  new test covers both the re-point refusal (with the parked-call case) and the same-workflow
  re-pin.
- New issues: none. Observation for the implementer only (not a finding, pre-existing since rev
  1): D2.6's "the registration's static params" means the frozen pin's params, as D2.3 step 6
  states (`params = pin.params`); after a re-pin the current registration's params are not used
  by old pins.
- Checks: decision-level recheck; code unchanged since R006 (digest unchanged), no tests run.
- Outcome: **PASS** — P07-D2 revision 4, sha256
  `ebf212db284e9a6e128d48d5ac8d97f57d45821db7e3e78c3dc096fc5e2dfab7`. All D2 findings F018–F026
  are fixed or closed; none open.
- Process feedback: the targeted fix touched every place the rule is applied (protocol step,
  lock comment, invariant, recovery sentence, tests); quick to verify.

## WF-P07-R011 — B2 batch review (agent workflow tools) — IN PROGRESS

- Started 2026-10-22 (same reviewer B session). Target: B1+B2 tree at HEAD `2d608f2`, digest
  `201af80cd15074697362d56fe58ac51e6b070ca25bff09ea91b1737846241f98` (recomputed, matches);
  contract `logs/decision-D2-r4.md` `ebf212db…fab7`; COORDINATION r4 `89563c7e…0f5b` (= current
  `COORDINATION.md`). Completed entry follows below.

## WF-P07-R011 — B2 batch review (agent workflow tools) — completed entry

- Context: date 2026-10-22; workflow `implement-opus-reviewed-2_2`; task `TASK.md`, review
  `REVIEW.md`; batch B2; stage batch review; reviewer Claude Opus 5.5 (`claude-opus-5-5`),
  reviewer B (session 2); phase `p07`; worktree `/Users/mac02/dev/openbot`, branch `workflows`,
  HEAD `2d608f269761717de7706912bd99ddd325d9d352` + uncommitted tree.
- Identity: snapshot digest recomputed `201af80cd15074697362d56fe58ac51e6b070ca25bff09ea91b1737846241f98`
  (same exclusions; 114 paths; matches `logs/b2-snapshot.txt`), at start and before return;
  contract `logs/decision-D2-r4.md` `ebf212db…fab7`; `COORDINATION.md` = `logs/coordination-r4.md`
  `89563c7e…0f5b`. B2 delta located by path set (+28 paths vs `logs/r006-snapshot.txt`) and by
  modification time after the R006 acceptance.
- Requirements re-derived from: plan 07 "Child calls and workflow tools" (map the saved model call
  to the child identity before starting it; child may decide/effect; resume the exact pending
  call; typed results, no fabricated success) and acceptance line 107; D2 rev 4 (D2.1–D2.7);
  P07-Q1 (named registration, company test) and P07-Q2 (revoke stops, re-pin doesn't).
- Read (code): `tools.rs`, `publication/types.rs` (`ToolKind`), `publication/dependencies.rs`,
  `publication/mod.rs` / `compile.rs` (`tool_workflows`), `workflow_tools.rs`,
  `workflow_tool_calls.rs`, `children.rs` (`child_binding`, `create_on`), `child_waits.rs`
  (tool branch), `waits.rs` row, `authority.rs` (`supported_publication`, `builtin_message_tool`),
  `resources.rs`, `bindings.rs`/`admission.rs`/`admission_replay.rs` callers,
  `agent_preparation.rs` (`current_on`, `targets`), `agent.rs` (`prepared_tool`),
  `agent_publication.rs` (`granted_tool`), `child_publication.rs`, `lease.rs`,
  `inspection.rs`, `message.rs` trust check, `adapters/workflow_agent/mod.rs`,
  `adapters/harness/rig/workflow.rs` (Parked → Suspend); schema: registration table and FK,
  `workflow_wait_child` CHECK and unique index, `check_workflow_parent_source`,
  `check_workflow_child_parent_wait`, park/reopen arms, continuation guard. Tests: all four new
  modules (bodies of the acceptance, one-child, revocation, stored-children, company and race
  tests read in full).
- Verified against requirements:
  - Dispatch follows D2.3 steps 0–8 exactly, including F026 (`registration.is_none_or(|r|
    r.workflow != pin.workflow_id)` after the step-2 replay); child + wait + park in one fenced
    transaction; the SQL park arm independently requires the waiting tool-call wait for this
    attempt; reopen arm requires the completed wait and a terminal child.
  - P07-Q2 modes: `Publish` for the root's own snapshots only, `Run` for stored children
    (per-bundle in `supported_publication`), admission (`verify_selection`), replay, binding
    readiness and dispatch; archive ignored by `Run`. Preparation checks grants by name only.
  - P07-Q1 company requirement: composite FK, service `NotFound` for a foreign version, raw insert
    23503, foreign-only name cannot publish, own registration freezes an own-company child — all
    asserted in `workflow_tool_pins_and_grants_stay_within_the_agents_company`.
  - Acceptance line: `workflow_tool_resumes_the_pending_call_after_a_child_human_decision` parks
    on the child, the child's reviewer decides, the reopened job replays the same invocation
    (checkpoint `invocations[0].result` = child output), the scripted model is called exactly
    twice (no turn re-run) and the agent finishes; one child, continuation 1.
  - Classifier exhaustive at the five sites; message trust unchanged (a `message.*`-named
    snapshot must still equal the trusted built-in).
  - No compile-time sqlx macros added (offline-cache staleness impossible from B2).
- Judgement calls: note 9 accepted (same `child_binding` and saved-binding read as the B1 step
  child, which B1 tests), with an optional literal assertion in F028.3; note 10 accepted (the
  `([tool], Some(registration))` arm is kind-agnostic); the two `session_replication_role =
  replica` setups accepted (B1 precedent `child_output_tests.rs`; CI runs as `postgres`); the
  `granted_tool` built-in reuse regression fix is correct (`([tool], None)` with `Message`).
- My checks (`logs/r011-review-checks.log`, phase cluster, `SQLX_OFFLINE=true
  CARGO_INCREMENTAL=0 -j 4`): `cargo test --lib -- workflow_tool agent_ child_ publication
  admission binding message_ resources waits` → **1094 passed, 0 failed** (includes all 18 B2
  tests); race repeats `workflow_tool_race_tests` + one-child-per-call ×5 → 6/0 each.
- Findings: **WF-P07-F027 (Low)** — the gated F024 test stops at publication: no admission of a
  new parent over a stored child with an older tool pin, and no workflow tool call made from
  inside a child run (all B2 runtime tests dispatch from a root run). **WF-P07-F028
  (Optional)** — refuse `workflow` pins in `HttpOperation::validate`; one `head()` assertion on a
  tool-call child (inspection decode); literal resources assertion.
- Outcome: **changes required (Low only)** — F027. F028 optional. No defect found in the shipped
  dispatch, publication or authority code; the gap is verification of the nested path the gate
  promised.
- Process feedback: the B2 notes and the D2.7 → test mapping made the review efficient; mapping
  each D2.7 bullet to the exact assertion (not just the test name) would have surfaced F027
  before review.

## WF-P07-R012 — B2 correction recheck of R011 — IN PROGRESS

- Started 2026-10-22 (same reviewer B session). Target digest
  `16e65cb8170fea84ca86a10ffb8341f1b903d519c11305e73fa78a1d8f4faee9` (`logs/r012-snapshot.txt`);
  D2 rev 4 and COORDINATION r4 unchanged. Completed entry follows below.

## WF-P07-R012 — B2 correction recheck of R011 — completed entry

- Context: date 2026-10-22; workflow `implement-opus-reviewed-2_2`; task `TASK.md`, review
  `REVIEW.md`; batch B2; stage correction recheck; reviewer Claude Opus 5.5 (`claude-opus-5-5`),
  reviewer B (session 2); phase `p07`; worktree `/Users/mac02/dev/openbot`, branch `workflows`,
  HEAD `2d608f269761717de7706912bd99ddd325d9d352` + uncommitted tree.
- Identity: digest recomputed `16e65cb8170fea84ca86a10ffb8341f1b903d519c11305e73fa78a1d8f4faee9`
  at start and before return (115 paths; the only path added versus `logs/b2-snapshot.txt` is
  `src/application/workflow/http.rs`); D2 rev 4 `ebf212db…fab7` and COORDINATION r4
  `89563c7e…0f5b` unchanged. Files modified after R011 (by mtime): `application/workflow/http.rs`,
  `workflow_tool_support.rs`, `workflow_tool_registration_tests.rs`,
  `workflow_tool_revocation_tests.rs`, `workflow_tool_tests.rs` — all read.
- Coverage (by finding ID):
  - F027 (Low): fixed. The stored-children test now goes through the real binding service and
    `WorkflowService::admit` for the outer parent after the re-pin (Run mode over the stored
    child), dispatches the stored agent's call from inside the child run through the agent
    runtime, asserts the grandchild runs the frozen `setup.child`, continues and replays the call
    (same invocation, 2 model turns, 1 grandchild). Revoked case unchanged.
  - F028 (Optional): items 1–2 adopted and tested (HTTP pin refused with `BadRequest`; `head()`
    of a tool-call child decodes `ChildCause::ToolCall` with the wait's call id and the parent
    execution); item 3 declined consistent with my acceptance of note 9. Closed.
- New issues: none. `HttpOperation::validate` also runs when stored operations are read
  (`persistence/workflow/http.rs:43,281`, `http_publication.rs:45`); stored snapshots never
  carry `workflow` (it is skipped when `None`), so no stored row is newly refused. Moving `kind()`
  and `saved_result()` to support is behaviour-neutral.
- My checks (`logs/r012-review-checks.log`, phase cluster, `SQLX_OFFLINE=true CARGO_INCREMENTAL=0
  -j 4`): `cargo test --lib -- workflow_tool http` → 499 passed, 0 failed, 1 ignored (the
  pre-existing `#[ignore]` developer aid); `scripts/stack-budget.sh -- workflow_tool` at
  2048 KiB → 18/0 (includes the nested call).
- Outcome: **PASS** for B2 at digest
  `16e65cb8170fea84ca86a10ffb8341f1b903d519c11305e73fa78a1d8f4faee9`. All p07 findings
  F001–F028 are fixed or closed. Not rerun by me or the author for this delta: the full
  `workflow` selection (last full evidence `b2-workflow.log` 1162/0 at `201af80c…1f98`; the delta
  is test code plus one validation condition, covered by the focused runs above) — the phase's
  final full-suite gate still applies.

## WF-P07-R013 — Test-support drain fix recheck — IN PROGRESS

- Started 2026-10-22 (same reviewer B session). Target digest
  `af49b63b765cc6e1d54d0005c805023a3ead87c31ea092aa69ce6809d403f2cb` (`logs/r013-snapshot.txt`);
  only new path vs R012: `action_reconciliation_control_serialization_lock_support.rs`.
  Completed entry follows below.

## WF-P07-R013 — Test-support drain fix recheck — completed entry

- Context: date 2026-10-22; workflow `implement-opus-reviewed-2_2`; task `TASK.md`, review
  `REVIEW.md`; stage post-full-suite test-only fix recheck; reviewer Claude Opus 5.5
  (`claude-opus-5-5`), reviewer B (session 2); phase `p07`; worktree `/Users/mac02/dev/openbot`,
  branch `workflows`, HEAD `2d608f269761717de7706912bd99ddd325d9d352` + uncommitted tree.
- Identity: digest recomputed `af49b63b765cc6e1d54d0005c805023a3ead87c31ea092aa69ce6809d403f2cb`
  at start and before return (116 paths; the only path added vs `logs/r012-snapshot.txt` is
  `src/adapters/persistence/workflow/action_reconciliation_control_serialization_lock_support.rs`,
  whose whole diff against HEAD is the `inspect_finished_owners` change); D2 rev 4 and
  COORDINATION r4 unchanged.
- Does the cause hold? Yes by construction, not by reproduction:
  - sqlx-core 0.8.6: `Transaction::drop` only queues the ROLLBACK (`transaction.rs:264-273`);
    `PoolConnection::drop` spawns `return_to_pool` (`pool/connection.rs:200-210`), whose
    `Floating::return_to_pool` pings to "flush anything time-sensitive like transaction
    rollbacks" (`connection.rs:306-321`). So an owner future can return while its backend is
    still `idle in transaction` (or still running a statement dropped mid-flight) until a spawned
    task runs.
  - PostgreSQL reports `idle` and clears `xact_start` before sending ReadyForQuery, so an owner
    whose commit returned can never be seen busy afterwards; only a queued rollback or an
    abandoned statement awaiting that flush (or a genuinely leaked handle) explains the R006 and
    full-suite-run-1 failures at the instantaneous check.
  - Not reproduced: `logs/full-serialization-fix.log` (4×241/0, 48 inspections) and my 3 runs
    (36 inspections) all observed `first_busy=null`, drain 0–5 ms. The panics in that log are the
    `prior_bounds` tests' expected-panic controls (each run 241/0).
- Does the bounded drain weaken the test? Not materially:
  - The correctness signals are unchanged and still instantaneous: observed lock waits in the
    stated order, owner outcomes, and the `RunProbe::Free` probe right after both owners join.
  - The watched backends are only the owners' own connections and the observer; work on other
    connections (e.g. detached tasks) was never watched, so the old instant check only caught a
    lingering owner backend probabilistically. The new check still fails, with the busy rows, on
    anything that does not drain: a leaked transaction, a held lock, or a hung statement.
  - It follows the file's own hang-guard doctrine (`LOCK_WAIT_HANG_GUARD`, lines 147-151: slow
    flushes are measured, not failed). Per AGENTS.md "raised limit", what still fails is a
    non-draining backend at 10 s, and `drain_ms`/`first_busy` are logged for trend.
  - Residual watch item (not a finding): the `RunProbe::Free` probe stays instantaneous; if an
    owner ever drops a transaction that holds the run lock, the same release latency could flake
    there. Nothing suggests that happens today.
- Author's checks reviewed: `full-suite.log` run 1 2743/1 (this test, line 284); `full-suite-2.log`
  lib 2744/0/22 ignored, bin 5/0; `full-stack-budget.log` 2744/0/22 at 2048 KiB;
  `full-lease-budget.log` 130/0.
- My checks (`logs/r013-review-checks.log`, phase cluster, `SQLX_OFFLINE=true CARGO_INCREMENTAL=0
  -j 4`): `cargo test --lib -- control_serialization --nocapture` ×3 → 6/0 each.
- Findings: none.
- Outcome: **PASS** at digest
  `af49b63b765cc6e1d54d0005c805023a3ead87c31ea092aa69ce6809d403f2cb`.
