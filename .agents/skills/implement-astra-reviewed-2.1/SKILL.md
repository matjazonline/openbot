---
name: implement-astra-reviewed-2.1
description: "Execute complex implementation with Astra authorship and independent Astra review, using large cohesive batches, selective architecture gates, persistent plan-local review notes and findings, and context continuity. Use when an implementation request selects this workflow; not for standalone planning or review."
---

# Implement Astra Reviewed 2.1

Codex adaptation of `implement-opus-reviewed-2`, retaining its large review
batches, plan-local evidence, and context continuity. Uses the Astra settings of
`implement-astra-reviewed-2`. This skill is self-contained; do not load the other
variants. Savings and quality must be measured, not assumed. User preferences
override these defaults.

## Roles and settings

| Role | Model / effort | Ownership |
| --- | --- | --- |
| Author (A) | `gpt-6-astra` / `medium` | Investigation, consequential decisions, implementation, fixtures, tests, and every correction. |
| Reviewer (B) | `gpt-6-astra` / `medium` | Independent review of requirements, architecture, code, and verification; review notes and findings. Does not edit project files or author the design. |
| Root | Actual session settings | Scope, assignments, resources, acceptance, and user updates. |

- When authoritative session information or the user confirms root runs
  `gpt-6-astra` at `high` effort, root acts as A and spawns only B.
  If root's model or effort is unknown or uncertain, ask the user to confirm
  whether root is Astra/high or choose a separate Astra author **before spawning
  any new sub-agent**. Wait for the answer; missing metadata is not evidence that
  a separate author is needed. Reuse confirmation already provided in the session.
  When root is confirmed not to be Astra/high, use a separate A unless the user
  authorizes root authorship. With a separate A, root stays a thin coordinator
  without repeating implementation or technical review. Keep and record root's
  actual settings; never infer them from a role label or claim instructions
  changed them.
- After resolving root's role, spawn B and, only if needed, A as direct children
  with `model: "gpt-6-astra"` and medium effort.
  Use the available Codex tool schema: `spawn_agent` with
  `reasoning_effort: "medium"` and `fork_context: false`; on hosts exposing
  `fork_turns` instead, use `fork_turns: "none"`. Never pass unsupported fields.
  This workflow explicitly requests delegation and those worker settings.
- **Start workers without inherited conversation.** In particular, B must not
  inherit root's history or A's reasoning. Workers do not spawn agents. Never
  combine authorship and independent review in one agent or swap roles.
- Keep agent handles and reuse workers via `send_input` (or the host's equivalent)
  for later batches and rechecks. Spawn B when its first review is needed. Wait
  for actual completion before reporting results; use bounded waits and keep user
  updates flowing. Use `close_agent` when workers are no longer needed.
- Record actual model/effort when reported. If Astra, requested effort, independent
  context, or delegation is unavailable, disclose the limitation and obtain an
  authorized alternative before substituting another workflow. Honor alternatives
  already authorized in the session.

Give workers compact assignments: original requirements and artifact paths,
applicable `AGENTS.md` files, scope, acceptance criteria, and resolved record paths.
Never copy conversation history or A's summaries into B. Use
[reviewer-assignment](references/reviewer-assignment.md) as B's prompt template.
Resolve the installed skill directory once and pass its absolute path to workers;
resolve scripts and references relative to it, not to the project being changed.

## Scope and records

Root records original requirements (or their authoritative paths), acceptance
criteria, starting commit, pre-existing changes to preserve, ordered dependencies,
and the selected plan directory. Store all workflow artifacts in `<plan-dir>/docs/`,
for example `plans/business-event-gaps/docs/`; if no plan directory exists,
establish `plans/<task-slug>/`. Resolve these paths once and include them in every
assignment and handoff:

- `<plan-dir>/docs/TASK.md`: scope, batches, decisions, status, acceptance.
- `<plan-dir>/docs/REVIEW.md`: review rounds per batch, linking note and finding IDs.
- `<plan-dir>/docs/agent-review-notes.md`: one entry per review round, following
  [review notes](references/review-notes.md).
- `<plan-dir>/docs/agent-review-findings.md`: one entry per concrete issue,
  following [durable findings](references/findings.md).
- `<plan-dir>/docs/logs/`: check output and other long evidence.

Notes and findings logs are shared across batches, tasks, and workflow variants
within the plan. Preserve existing entries, use task-prefixed IDs, and record this
workflow's name in each entry. If `TASK.md`/`REVIEW.md` already belong to another
task, add a clearly labeled section for this task instead of overwriting them.

A checks the request against actual code, follows repository discovery rules, and
traces affected shared symbols and callers before changing them (in this repo,
`graft callers <sym> --depth all`). Use the existing plan when clear; otherwise A
records only the necessary decisions, affected contracts, failure behavior, and
verification approach. Clarify blocking product requirements with the user;
resolve routine implementation choices from the system.

## Large batches and review cadence

Spend independent review on fewer, larger artifacts that remain reviewable.

- **Batch size.** A batch is a whole plan phase, milestone, or cohesive feature
  spanning its domain, application, persistence, HTTP, migration, and test changes.
  Do not review individual steps or layers separately. Split only where one review
  could no longer be thorough, or at stable contracts a later batch depends on.
  Honor user-specified order and checkpoints.
- **Architecture gate (rare).** Hold one pre-implementation gate per batch only
  when it contains a new or changed decision that is costly to unwind after
  dependent code exists: schema/migration and recovery, persistent ownership,
  transaction boundaries, concurrency protocols, authorization models, or contracts
  spanning subsystems. Present all such decisions for the batch together in
  `TASK.md` (decision, invariants, failure modes, affected contracts, verification
  plan). Everything else, including routine use of established patterns, is
  reviewed only in the batch review. This is an internal gate, not a user approval.
  Resolve actionable findings and obtain B's explicit pass for the current
  decision revision before dependent edits. Reopen it if that decision changes.
- **No intermediate reviews.** A advances all dependent steps in the batch, then
  requests one batch review. A pauses for B mid-batch only when a gated decision
  must change or an assumption it relied on is contradicted; record the evidence
  and proposed change, and continue independent work meanwhile.
- **Corrections in one round.** A fixes every actionable finding from a review
  before requesting a single recheck. Optional suggestions alone do not require
  a recheck; implementing one changes the artifact and needs affected review.
- Accept each batch before building a later batch on it.

## Implementation, review, and acceptance

1. A implements the batch, audits the diff, and runs meaningful targeted checks
   and repository-required broader checks. Cover relevant failure paths; changed
   concurrency protocols need competing-claimant tests where applicable. Record
   commands, results, environment/fixture identity, and log paths in `TASK.md`.
2. Freeze the artifact: run [scripts/snapshot.sh](scripts/snapshot.sh) from the
   repository root with the plan docs directory as exclude, e.g.
   `bash .agents/skills/implement-astra-reviewed-2.1/scripts/snapshot.sh plans/<topic>/docs`,
   and record base commit and digest. Exclude only workflow records, never
   implementation or deliverable documentation; use narrower record paths if
   the docs directory also contains deliverables. Record the exact exclusion list
   with the digest and reuse it unchanged for A, B, and root's verification.
   Identify preserved pre-existing work
   separately. No project edits during review; unexpected changes require a new
   digest. Serialize project writes and checks that mutate shared fixtures.
3. B reviews the **whole batch** against original criteria, actual changes and
   callers, gated decisions, and evidence: coverage, integration, failure
   behavior, and security/concurrency/recovery paths. Challenge architectural
   assumptions even if previously gated. A's summary and passing tests are not proof.
4. B saves a review-notes entry for every round (including clean passes and
   rechecks) and logs all concrete findings together before replying. It returns
   note/finding IDs with severity and actionable evidence, or an explicit **PASS**
   naming artifact digest, coverage, and checks (decision revision for architecture
   gates). B checks the digest again before returning. Optional preferences do not block
   acceptance. A corrects defects and reruns affected checks.
5. B rechecks the correction delta and affected behavior by finding ID. Reuse
   unaffected evidence only while code, environment, and fixtures support it;
   broaden review when contracts or assumptions change. Material architectural
   changes reopen the gate. Do not repeat a full review of unchanged, covered work.
6. Root recomputes the digest and accepts only with B's explicit pass for it, all review
   notes, findings, and dispositions saved, no unresolved acceptance-blocking
   defects, and all required checks passing. Failed or unavailable checks remain
   unverified. At completion, run outstanding required broader checks and have B
   review combined integration effects not already covered. Later substantive
   edits invalidate affected acceptance.

Stop correction loops that yield no new evidence: identify the blocker, revise the
approach within scope, or request a missing decision. There is no fixed correction
count. Never weaken criteria or substitute self-review to finish.

Retain notes, findings, gated-decision text, and supporting evidence after
completion or compaction; they are evaluation artifacts, not disposable logs.
Record candidate instruction improvements with evidence for later analysis without
editing skills or `AGENTS.md` during the run.

## Context: preserve continuity, target compaction at 75%

Target compaction at 75% of the effective context window for root, A, and B,
continuing in the same role and handle. This is not a retirement threshold;
do not replace workers simply because they reach 50% or 75%.

- At task start, inspect available authoritative context and compaction settings.
  Read [Codex context handling](references/context.md) when verifying or configuring
  the threshold. A skill cannot force compaction by instruction. If the host cannot
  enforce or expose the target, disclose default/unknown compaction and continue
  with durable checkpoints unless the user explicitly requires a strict threshold.
- Keep `TASK.md`/`REVIEW.md` current at every assignment boundary: requirements,
  original gated decisions, artifact digest, accepted and unfinished work, finding
  IDs, evidence, resources, authorizations, role handles, actual settings, next action.
  After compaction, reconcile these records and the worktree and resume. Compaction
  does not end the task or invalidate unchanged acceptance evidence.
- Inspect available context signals before substantial assignments and during long
  work. Reserve headroom for tool results and checkpointing; never invent occupancy
  or equate cumulative spend with current context. With unknown headroom, bound
  individual reads and work segments without adding unnecessary review rounds.
- Keep tool output small: save long output in `logs/` and surface results and the
  evidence needed to judge a defect. Root receives status, IDs, and acceptance
  evidence, not every patch or log.

Replace a worker only when unavailable or context loss prevents reliable
continuation after checkpoint recovery. Stop the outgoing worker with the host's
interrupt/close controls and verify it stopped before replacing it. A replacement
reads original sources and relevant records, checks the worktree, and retains its
predecessor's role and settings. A fresh root reconciles records and starts fresh
workers instead of trusting stale handles. Never use A's summary as B's
requirements source.

Detailed spend accounting is optional. When benchmarking is requested, read
[benchmarking](references/benchmarking.md). Otherwise use readily available usage
without building telemetry or blocking implementation on missing metrics.

## Completion

Carry existing authorizations into assignments; this skill adds no authorization
for commits, deployment, publishing, production resets, or external messages.
Before returning, stop outstanding worker activity and clean up only task-created
disposable resources. Report completed scope, independent review rounds (with note
IDs), verification, and remaining gaps. Save resumable state if blocked; never
call unverified work complete.
