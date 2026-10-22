# P07 — Child workflows and bounded human-feedback revisions

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Identity

- Phase ID: `p07` (IDs `WF-P07-…`). Workflow: `implement-opus-reviewed-2_2`
  (skill dir `/Users/mac03/dev/openbot/.claude/skills/implement-opus-reviewed-2_2`).
- Record root: `/Users/mac03/dev/openbot/plan/workflow-foundation/phases/p07/docs/`.
- Root = Author (A): this Claude Code session, Opus 5.5 / xhigh (user stated "you are
  Opus/xhigh"; `/effort` set xhigh this session). Reviewer (B): spawned at first gate; ID below.
- Worktree: `/Users/mac03/dev/openbot`, branch `workflows`, starting commit
  `921079d125ec0f768eee53f603a1db648ff356bc`. Source tree at HEAD equals the phase-06 commit
  `4dd01b3` for `src/`, `migrations/`, `.sqlx/`, `Cargo.*`, `scripts/` (verified 2026-10-21).
- Preserved pre-existing changes (not ours, never touched/staged): 440 working-tree deletions
  under `plan/workflow-foundation/` (earlier phase records removed by the user's retrospective
  cleanup). Snapshot exclusions must not hide them; they are listed as pre-existing.
- Plan coordination: `/Users/mac03/dev/openbot/plan/workflow-foundation/COORDINATION.md`
  (created by this phase, writer = this root), revision 1, no notices.
- Compaction: `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75` present in settings env (verified).

## Original requirements

- Plan: `plan/workflow-foundation/07-child-workflows-and-review-revisions.md` (authoritative),
  expansion rows 07.1–07.4 in `plan/workflow-foundation/EXPANSION.md`, README, REPLACEMENT-MAP
  rows naming phase 07, NOT_PLANNED (acyclic children).
- Instructions: `AGENTS.md`, `src/AGENTS.md`, `src/application/AGENTS.md`,
  `src/adapters/persistence/AGENTS.md`, `src/infra/AGENTS.md`.
- Prerequisite: phase 06 accepted 2026-10-21 (06-R025 PASS, digest `0d8bba5d…9b00`), committed
  as `4dd01b3`. Phase-08 obligations from 06 stay with phase 08 (worker wiring etc.).

### Acceptance criteria (from plan §Acceptance + body)

1. `workflow.call` invokes a pinned version with schema-validated explicit inputs; child creation
   and parent wait persist together; one child identity per logical call across retries; child
   gets only explicit inputs and allowed resource bindings; returns its declared output.
2. Children inherit root authorization ceilings, deadlines, budgets and cancellation; cannot gain
   access via a more privileged workflow; dependency graph frozen at publication; recursion
   rejected; child results/failures become durable parent continuation events; parent occupies
   no worker while waiting.
3. Published workflows exposed as agent tools (input/output schemas); saved model call mapped to
   child identity before start; parent agent resumes the exact pending call; rejected/failed
   children yield typed results/errors, never fabricated success.
4. Retry vs revision vs rerun semantics as tabled; never reset a completed agent step.
5. `flow.repeat`: child ref, initial mapping, next-iteration mapping, exit predicate, max
   iterations, exhaustion route; exit evaluated after each child completion before another
   round; last-round acceptance succeeds; exhaustion follows declared route, never
   auto-approves; final output = final child result + round count; prior rounds linked in trace.
6. Shipped support template: three rounds default, exhaustion → human escalation; authors may
   choose a lower bound within platform limits; deadlines/root budgets span all rounds.
7. Successors can explicitly map human results (`/steps/review/output/feedback|data`).
8. Tests: revision feedback reaches next round with prior artifact/decision intact; older draft
   acceptance cannot authorize newer; late links cannot settle current round; only final
   accepted artifact sent once; no replay of context loads/earlier effects; child-creation
   retries, duplicate completion, parent cancellation, child timeout, restart; last-round
   acceptance, exhausted revisions, inherited budget enforcement; agent workflow tools resume
   the original checkpoint after a child human decision.

## Batches

| Batch | Scope | Gate | Status |
| --- | --- | --- | --- |
| B1 | Child calls (`workflow.call`), bounded repeat (`flow.repeat`), cancellation/deadline/budget inheritance, review-round fixtures/template, successor feedback mapping, tests 1,2,4,5,6,7,8 (minus agent tools) | P07-D1 (architecture) | gate drafted |
| B2 | Agent workflow tools (registration, publication freeze, Rig tool target, child from saved model call, agent continuation, typed result/error), criterion 3 + its tests | P07-D2 (architecture, after B1 accepted; product questions may go to the user) | not started |

Final phase gate after B2: fmt, offline all-target check, strict clippy, fresh migration + SQLx
check, affected DB-backed suites (workflow), stack budget for affected tests; full suite only if
the user asks (AGENTS.md requires full verification before declaring completion — see Final gate).

## Investigation summary (2026-10-21, A)

Existing foundation (phases 02–03): registry/compiler accept `workflow.call`/`flow.repeat` with
pinned `child_workflow_id`/`child_version_id` and `max_iterations` (1..10000); publication pins
child `PublishedBundle`s transitively (`PublishedBundle::children`), rejects missing/duplicate/
foreign/recursive/unused children, depth ≤ 32. Runtime: `workflow_runs.parent_run_id/
parent_execution_id`, immutable `workflow_run_parents` lineage, root budgets shared by
descendants (`workflow_run_budgets`, `workflow_root_budgets` incl. `repetitions`), and a child
terminal-state trigger appending an immutable `parent_wakeup` fact (`schedule_workflow_parent_
wakeup`); `WorkflowParentWakeups::parent_wakeups` reads it. Waits support reason `child_run`
in schema only. Gaps: no handler/park/resume for child kinds (worker returns Unsupported), no
child params/resources rule, no repeat next/exit/exhaustion language, no cancellation cascade,
child admission exists only as binding-based external admission (`TriggerSource::Child`), the
wait index allows one non-decision wait per execution, activation/inspection read
`workflow_admissions.source_key`.

## Agents

- A/root: this session (Opus 5.5/xhigh).
- B: agent `a46b56e8565db0c84` (general-purpose, model opus, effort xhigh, fresh context),
  spawned 2026-10-21 for P07-D1.

## Decisions

- **P07-D1 (B1 architecture gate)** — `logs/decision-D1.md` revision 1 (sha256
  `7c3245e7…8f3c92`), code digest `e2714983…e3fc`. B round **WF-P07-R001: changes required**
  (2026-10-21). Findings (B-owned, see `agent-review-findings.md`):
  F001 High — child history snapshot leaks later thread messages / unbounded per-round insert
  (fix: children capture no history or share root snapshot; test late message invisible);
  F002 Medium — non-input park/round failures roll back and re-poll forever (define outcomes,
  poison tests); F003 Medium — rerun link deferred without agreed disposition (specify via
  manual admit link or record agreed disposition); F004 Medium — phase-03 binding-based child
  admission bypasses frozen graph/recursion/deadline (refuse/remove in B1, move fixtures);
  F005 Low — `control_retry.rs:8-9` child retry fact undefined (decide, comment, refusal test);
  F006 Low — cascade lookup by unindexed `parent_run_id` under 1 s timeout (use the parent's
  waiting `child_run` wait) and no cancel-vs-park/round race test; F007 optional suggestions.
- **A self-finding for revision 2 (not yet reviewed):** no production `PublicationDirectory`
  captures pinned child bundles (all return `children: vec![]`; `verify_children` only checks
  supplied ones), so a parent with `workflow.call` cannot be published. Rev 2 must add a
  child-capturing directory (company-scoped `workflow_versions` bundle by declared child
  workflow/version, unarchived) and place the by-name child resource check where both parent
  definition and child bundles are visible (publication validation). Reuse the registry
  `static_roots` check (`registry/mod.rs:177`) for `with.params` with a field-specific code.

## Checks

None (architecture stage; no source edits). Digest rechecked after R001: `e2714983…e3fc`.

## Status / next action

USER-REQUESTED STOP 2026-10-21 after R001 ("after sub-agent response write a handoff and
stop"). Next: write P07-D1 revision 2 (new file `logs/decision-D1-r2.md`; rev 1 stays
immutable) addressing F001–F006, chosen F007 items and the self-finding; then `SendMessage` B
(`a46b56e8565db0c84`) for a correction recheck by finding ID. No implementation before B's
explicit PASS of the current revision. See HANDOFF.md.
