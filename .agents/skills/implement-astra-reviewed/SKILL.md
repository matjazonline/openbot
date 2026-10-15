---
name: implement-astra-reviewed
description: Execute complex implementation work with one Astra agent retaining ownership of plan expansion and implementation, and a separate Astra agent independently reviewing architecture and code. Includes measured context rotation and resumable handoffs. Use for implementation requests selecting this workflow, not standalone planning or review.
---

# Implement Astra Reviewed

Keep architectural reasoning with the implementer while obtaining independent
review against the original requirements. This is a workflow to measure, not a
promise of lower token usage, cost, or defect rates.

## Roles and continuity

| Role | Model / effort | Ownership |
| --- | --- | --- |
| Coordinator (root) | Current session settings | Scope, assignments, resources, acceptance, handoff |
| Author (A) | `gpt-6-astra` / medium | Expand the plan, implement, test, correct findings |
| Reviewer (B) | `gpt-6-astra` / medium | Independently review consequential architecture and completed code |

Root owns A and B as direct children, with at most one active worker per role.
Workers do not spawn agents. Spawn with explicit model and effort and
`fork_turns="none"`. Give compact assignments and original artifact paths; do
not copy root or author conversation history into the reviewer. User preferences
override defaults. Keep the actual root model; do not claim instructions changed
it. Record root usage as part of this workflow when comparing against single Astra.
If Astra or delegation is unavailable, report the exact limitation and obtain an
authorized alternative before replacing the independent review workflow.

Reuse A with `followup_task` across expansion, implementation, and corrections
while its measured context permits. Reuse B across related architecture/code
reviews and rechecks while eligible. B is a different agent from A; never swap
roles or let a worker review code or decisions it authored. Context retirement
overrides continuity: a replacement author must read original requirements and
recorded decisions. A fresh root also creates fresh workers.

Root coordinates and records acceptance; it does not duplicate implementation or
technical review. A owns project edits and test fixtures; B returns findings and
does not patch the project or author the expansion. Freeze the artifact under
review, including uncommitted changes, and serialize operations on shared mutable
resources. Capture the base commit and diff identity for each gate.

Read [context and handoffs](references/context.md) and measure root before
assigning work. Send both workers that reference and the bundled helper path.

## Establish scope and preparation

Read the original request/plans and applicable repository instructions. Record
starting commit, pre-existing changes to preserve, selected scope, dependencies,
and acceptance criteria. Follow repository discovery rules and trace shared
symbols/callers before changing them. Order work by user instructions and declared
dependencies; clarify only blocking scope/product decisions.

A checks the requirements against the actual system and meaningful acceptance
checks. When behavior and interfaces are sufficiently specified, proceed directly
to implementation; do not rewrite a clear plan for ceremony.

For complex or underspecified work, A records a compact expansion beside the plans
(or in the repository's established task-artifact location). Preserve original
criteria, explain consequential decisions once, identify affected contracts and
failure behavior, and map deliverables to verification. Link evidence and originals
instead of restating them. Separate genuinely unresolved requirements from routine
implementation choices that A can resolve from the code.

Before dependent edits, B reviews consequential architectural choices: concurrency,
authorization boundaries, migration/recovery, compatibility, persistent ownership,
or contracts spanning subsystems. B reads the originals and relevant system code,
challenges assumptions, and returns all concrete findings together or an explicit
pass with evidence. A corrects its expansion; B rechecks affected decisions and
cross-point effects. Root records the gate. This is an agent review gate, not a
new user approval requirement. Local implementation choices need no separate gate.

## Implement and independently review

A retains ownership after preparation. Implement cohesive, verifiable points in
dependency order. Honor repository requirements for tests, security, reproducible
builds, migrations, and resource budgets. A changed concurrency protocol needs
competing-claimant tests where applicable. Preserve unrelated work and avoid adding
speculative cleanup or infrastructure to the selected scope.

A audits its diff, runs meaningful targeted checks and required broader checks,
and records exact commands, results, environment/fixture identity, and full log
paths. Keep verbose output out of root context. Failed or missing checks remain
unverified; do not treat implementation completion as acceptance.

Root gives B original criteria, decisions, frozen base/diff, preservation constraints,
and evidence paths. B inspects actual changes and callers rather than relying on
A's summary or passing builds. Review requirements coverage, integration, failure
behavior, and relevant security/concurrency/recovery paths. Explicitly challenge
the architectural assumptions, including those B previously approved; conformance
to the expansion alone is insufficient.

B returns all concrete findings in one pass, with file references and required
behavior, or an explicit pass with evidence and remaining verification. A fixes
findings and reruns affected checks; B rechecks changes and their effects. Reuse
unaffected evidence only while code, environment, and fixture assumptions hold.
Do not require B to rediscover unchanged areas on every correction round.

Material changes to contracts or architecture reopen the affected preparation gate.
Stop repeating correction loops that produce no new evidence: identify the blocker,
revise the approach within scope, or ask for the genuinely missing decision. Never
weaken criteria or replace independent review with self-review to finish cheaply.

Root accepts a point only after B's explicit pass and required checks. Advance
dependent points after acceptance. At a phase boundary/final completion, A performs
required broader checks and B reviews combined integration effects, reusing valid
point evidence. Freeze and identify the final artifact; substantive later edits
invalidate affected acceptance.

## Progress, context, and measurement

Apply [context and handoffs](references/context.md) at assignment/completion
boundaries and during substantial work. Workers retire at 50% measured context;
root checkpoints and stops at 60%. Reserve headroom and rotate earlier when needed.
These are operational thresholds, not runtime-enforced limits. Preserve author
continuity within those bounds; do not raise limits to retain one agent.

Maintain a compact progress record beside the task artifacts: original scope and
criteria, ordered points/dependencies, decisions, current baseline/diff, status,
findings and resolutions, evidence paths, resource ownership, role/tool handles,
verified session UUIDs, actual model/effort, context samples, and next action.
Workers own separate expansion/test/review records; root owns acceptance. Distinguish
preparing, architecture-reviewing, implementing, code-reviewing, verified, blocked,
and implemented-but-unverified.

When runtime usage is available, report input + output across root and all author
and reviewer sessions, including retries, corrections, rotations, and fresh-root
continuations belonging to the task. Count unique calls or task-bounded cumulative
deltas; deduplicate replayed snapshots and inherited fork history. Cached input is
already within input and reasoning is already within output. Report per-model/cache
usage, verification outcome, correction rounds, and elapsed time separately. Missing
usage is unknown, not zero. Context percentages do not measure spend. Price only
with verified rates; do not build a telemetry system unless requested.

## Permissions and completion

Carry existing authorization and exact resource identities into assignments.
Routine local implementation and authorized tests need no repeated conversational
approval. Skill invocation does not authorize production resets, publishing,
deployment, commits, or messages to others. Honor actual host permission boundaries.
Do not cancel pending approvals just because they are slow; continue independent
work where useful. Context limits still require checkpointing.

Before finishing, quiesce workers and clean up only task-created disposable
resources. Report completed scope, independent review, verification, and remaining
gaps. When blocked or at the root threshold, save a resumable handoff with partial
work and accepted evidence; never describe unverified changes as complete.
