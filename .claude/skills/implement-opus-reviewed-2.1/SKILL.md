---
name: implement-opus-reviewed-2.1
description: "Execute complex implementation with an Opus 5.5/xhigh author and an independent Opus 5.5/xhigh reviewer subagent, reviewing large cohesive batches (plan phases or milestones), compacting context at 75%, and persisting review notes and findings in the plan's docs directory for later analysis. Use when an implementation request selects this workflow; not for standalone planning or review."
---

# Implement Opus Reviewed 2

Claude Code variant of `.agents/skills/implement-astra-reviewed-2`. It keeps
independent author/reviewer separation, but uses fewer, larger reviews and records
review notes and findings like `implement-astra-reviewed-3`. This skill is
self-contained; do not load the other variants. Savings and quality must be
measured, not assumed. User preferences override these defaults.

## Roles and settings

| Role | Model / effort | Ownership |
| --- | --- | --- |
| Author (A) | Opus 5.5 / `xhigh` | Investigation, consequential decisions, implementation, fixtures, tests, and every correction. |
| Reviewer (B) | Opus 5.5 / `xhigh` | Independent review of requirements, architecture, code, and verification; review notes and findings. Does not edit project files or author the design. |
| Root | Actual session settings | Scope, assignments, resources, acceptance, and user updates. |

- Root acts as A only when authoritative session information confirms it runs
  Opus 5.5 at xhigh effort (for example `/model` output or the effective
  `modelSettings`). Otherwise root spawns A and stays a thin coordinator that does
  not repeat implementation or technical review. Never infer settings from a role
  label or claim to change root's settings.
- Spawn workers with the Agent tool: `subagent_type: "general-purpose"`,
  `model: "opus"`, and `effort: "xhigh"` for both A and B. This
  skill explicitly requests those efforts, so passing them is authorized. Record
  the model/effort the runtime actually reports; if Opus 5.5 or the requested
  effort is unavailable, disclose it and obtain an authorized alternative before
  substituting a different workflow.
- **Never spawn B with `subagent_type: "fork"`.** A fork inherits root's
  conversation, including A's reasoning, which defeats independent review.
- Workers cannot spawn agents. They run in the background; wait for the
  completion notification and never predict or summarize a pending result.
- Keep each worker's agent ID/name and continue it with `SendMessage` for later
  batches and rechecks instead of spawning a new one. Spawn B when its first
  review is needed.
- Never combine authorship and independent review in one agent or swap roles.

Give workers compact assignments: original requirements and artifact paths,
applicable `AGENTS.md` files, scope, acceptance criteria, and the resolved record
paths below. Never copy conversation history or A's summaries into B. Use
[reviewer-assignment](references/reviewer-assignment.md) as B's prompt template.

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

Review is expensive at Opus/xhigh; spend it on fewer, larger artifacts.

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
- **No intermediate reviews.** A advances all dependent steps in the batch, then
  requests one batch review. A pauses for B mid-batch only when a gated decision
  must change or an assumption it relied on is contradicted; record the evidence
  and proposed change, and continue independent work meanwhile.
- **Corrections in one round.** A fixes every actionable finding from a review
  before requesting a single recheck. Optional suggestions never trigger a recheck.
- Accept each batch before building a later batch on it.

## Implementation, review, and acceptance

1. A implements the batch, audits the diff, and runs meaningful targeted checks
   and repository-required broader checks. Cover relevant failure paths; changed
   concurrency protocols need competing-claimant tests where applicable. Record
   commands, results, environment/fixture identity, and log paths in `TASK.md`.
2. Freeze the artifact: run [scripts/snapshot.sh](scripts/snapshot.sh) from the
   repository root with the plan docs directory as exclude, e.g.
   `bash .claude/skills/implement-opus-reviewed-2.1/scripts/snapshot.sh plans/<topic>/docs`,
   and record base commit and digest. Identify preserved pre-existing work
   separately. No project edits during review; unexpected changes require a new
   digest. Serialize project writes and checks that mutate shared fixtures.
3. B reviews the **whole batch** against original criteria, actual changes and
   callers, gated decisions, and evidence: coverage, integration, failure
   behavior, and security/concurrency/recovery paths. Challenge architectural
   assumptions even if previously gated. A's summary and passing tests are not proof.
4. B saves a review-notes entry for every round (including clean passes and
   rechecks) and logs all concrete findings together before replying. It returns
   note/finding IDs with severity and actionable evidence, or an explicit **PASS**
   naming artifact digest, coverage, and checks. Optional preferences do not block
   acceptance. A corrects defects and reruns affected checks.
5. B rechecks the correction delta and affected behavior by finding ID. Reuse
   unaffected evidence only while code, environment, and fixtures support it;
   broaden review when contracts or assumptions change. Material architectural
   changes reopen the gate. Do not repeat a full review of unchanged, covered work.
6. Root accepts only with B's explicit pass for the current digest, all review
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

## Context: compact at 75%

Root, A, and B compact when context occupancy reaches 75% of the window, then
continue in the same role, model/effort, and agent handle. Compaction is not a
retirement threshold; do not replace workers at 50% or 75%.

- The model cannot invoke `/compact` on itself. The threshold is enforced by
  Claude Code's `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75`, set in the `env` block of
  `.claude/settings.json` or `.claude/settings.local.json` (applies to root and
  its in-process subagents; takes effect for sessions started after the change).
  At task start, root checks the effective settings. If it is not set, tell the
  user it is required for the 75% rule, offer to add it, and continue with
  default auto-compaction disclosed, never claiming 75% compaction occurred.
- Because compaction can fire at any moment, keep `TASK.md`/`REVIEW.md` current at
  every assignment boundary: requirements, gated decisions, exact artifact digest,
  accepted and unfinished work, open finding IDs, evidence, resource identities,
  authorizations, role handles, and next action. After compaction, reconcile that
  checkpoint and the worktree, then resume the current assignment; compaction does
  not end the task or invalidate acceptance evidence.
- Keep tool output small: store long test/check output in `logs/` and surface only
  results and the evidence needed to judge a defect. Root receives status, IDs, and
  acceptance evidence, not every patch or log.

Replace a worker only when it is unavailable or context loss prevents reliable
continuation after checkpoint recovery. Stop the outgoing worker (`TaskStop`)
first. A replacement reads original sources and relevant records, checks the
worktree, and keeps its predecessor's role and settings. A fresh root reconciles
records and starts fresh workers instead of trusting stale handles. Never use A's
summary as B's requirements source.

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
