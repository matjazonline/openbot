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
