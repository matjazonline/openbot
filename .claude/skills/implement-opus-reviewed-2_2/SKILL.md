---
name: implement-opus-reviewed-2_2
description: "Execute complex implementation in Claude Code with an Opus 5.5 author at `medium` effort or above and an independent Opus 5.5/xhigh reviewer subagent, using large cohesive batches, selective architecture gates, phase-local progress and review evidence, isolated concurrent workstreams, and context continuity with 75% auto-compaction. Use when an implementation request selects this workflow; not for standalone planning or review."
---

# Implement Opus Reviewed 2.2

Claude Code adaptation of `implement-astra-reviewed-2.2` and successor to
`implement-opus-reviewed-2.1`, retaining its Opus roles, large review batches,
selective gates, and context continuity. Each independently scheduled phase has
its own records, artifact identity, and resource ownership. This skill is
self-contained; do not load the other variants. Savings and quality must be
measured, not assumed. User preferences override these defaults.

## Roles and settings

| Role | Model / effort | Ownership |
| --- | --- | --- |
| Author (A) | Opus 5.5 / `medium` or above | Investigation, consequential decisions, implementation, fixtures, tests, and every correction. |
| Reviewer (B) | Opus 5.5 / `xhigh` | Independent review of requirements, architecture, code, and verification; review notes and findings. Does not edit project files or author the design. |
| Root | Actual session settings | Scope, assignments, resources, acceptance, and user updates. |

- When authoritative session information or the user confirms root runs Opus 5.5
  at `medium` effort or above (`medium`, `high`, `xhigh`, or `max`), root acts
  as A and spawns only B. Claude Code's system prompt names root's model but
  normally not its effort, and `/effort` can override the `effortLevel` setting or
  `CLAUDE_CODE_EFFORT_LEVEL` mid-session. If root's model or effort is unknown or
  uncertain, ask the user with `AskUserQuestion` to confirm whether root is Opus at
  `medium` or above (`/effort status` prints the level in effect) or choose a
  separate Opus author **before spawning any new sub-agent**. Wait for the answer;
  missing metadata is not evidence that a separate author is needed. Reuse
  confirmation already provided in the session. When root is confirmed not to be
  Opus or to run below `medium`, use a separate A unless the user authorizes root
  authorship. With a separate A, root stays a thin coordinator without repeating
  implementation or technical review. Keep and record root's actual settings; never
  infer them from a role label or claim instructions changed them.
- After resolving root's role, spawn B and, only if needed, A with the Agent tool:
  `subagent_type: "general-purpose"`, `model: "opus"`, and `effort: "xhigh"` for B
  or `effort: "medium"` for A. Do not pass `isolation`; phase worktrees are set up
  explicitly. This workflow explicitly requests delegation and those worker
  settings, so passing them is authorized.
- **Start workers without inherited conversation.** Never spawn a worker, least of
  all B, with `subagent_type: "fork"`: a fork inherits root's history and A's
  reasoning. Claude Code subagents can spawn agents, so every assignment states
  that workers do not. Never combine authorship and independent review in one agent
  or swap roles.
- Keep each worker's agent ID from its spawn result (`ListAgents` recovers it) and
  continue the same worker with `SendMessage` for later batches and rechecks. Spawn
  B when its first review is needed. Workers run in the background: wait for the
  completion notification before reporting results, never predict or summarize a
  pending result, and keep user updates flowing. Stop a worker that is still
  running but no longer needed with `TaskStop`.
- Record actual model/effort when reported. If Opus 5.5, the required effort (A at
  `medium` or above, B at `xhigh`), independent context, or delegation is
  unavailable, disclose the limitation and obtain an authorized alternative before
  substituting another workflow. Honor alternatives already authorized in the
  session.

Give workers compact assignments: original requirements and artifact paths,
applicable `AGENTS.md`/`CLAUDE.md` files, scope, acceptance criteria, and resolved
record paths. Include the authoritative plan coordination path and applicable
notice revisions in every assignment; workers must read them before affected work.
Never copy conversation history or A's summaries into B. Use
[reviewer-assignment](references/reviewer-assignment.md) as B's prompt template.
Resolve the installed skill directory once (Claude Code states it as the skill's
base directory on load) and pass its absolute path to workers; resolve scripts and
references relative to it, not to the project being changed. Workers start in
root's working directory, and Claude Code may reset a shell's cwd between Bash
calls and does not keep shell variables: assignments name the absolute worktree,
workers prefix each command with `cd <worktree> &&`, and commands and file tools
use literal absolute paths.

## Scope and phase-local records

Resolve the selected plan, authorized phase, and stable phase ID before assigning
work. Use the plan's identifier, e.g. `p02` or `p07`; for an unphased task use a
stable task slug. Do not silently start other phases because they are independent.
If no plan exists, establish `plans/<task-slug>/`. Default record root:

`<plan-dir>/phases/<phase-id>/docs/`

**Every phase checks `<plan-dir>/COORDINATION.md` at startup, before assigning
implementation work.** Resolve its authoritative absolute path and named writer;
worktree copies are not automatically synchronized. If absent, establish the
minimal file using [plan coordination](references/concurrent-phases.md#plan-coordination)
before proceeding. Read its plan-wide rules and notices targeting this phase,
record applicable IDs/revisions and dispositions in TASK/HANDOFF, and resolve
blocking contracts before affected edits. Recheck on resume, before each batch,
before changing a shared contract, and before review and acceptance. Publish
cross-phase instructions and changes through this file using the protocol below;
do not edit another phase's records. A single-phase plan starts with empty notices.

This is `<phase-docs>` below. Resolve its absolute path **inside the assigned
worktree** once and pass it to every worker. Use the same phase ID across resumed
sessions and batches; a batch gets a heading and IDs, not another progress tree.
Each phase owns:

- `TASK.md`: original requirements/paths, acceptance criteria, dependencies,
  batches, decisions, check results, and current status.
- `REVIEW.md`: review rounds, artifact identities, acceptance and linked IDs.
- `agent-review-notes.md` and `agent-review-findings.md`: durable evidence following
  [review notes](references/review-notes.md) and [findings](references/findings.md).
- `HANDOFF.md`: concise current state, authoritative record pointers, accepted and
  unfinished work, open findings, resources, authorizations and next action.
- `logs/`: long check output, immutable review manifests and supporting evidence.

Root owns TASK and HANDOFF; B owns REVIEW, notes and findings. A supplies decisions,
implementation status and evidence to root, or writes designated TASK sections
when root explicitly assigns that ownership. Serialize writes within each phase.
Use plan/phase/task-prefixed IDs, e.g. `BEG-P07-B1-R1` and `BEG-P07-B1-F1`.
Records are shared across batches and workflow variants **within this phase**;
other phases link to evidence and never append to these files.

TASK and HANDOFF must identify phase ID, workflow, record root, root/A/B agent IDs,
actual settings, worktree/branch, starting commit, preserved pre-existing changes,
current base/digest and exclusions, prerequisites with accepted artifact identities,
owned paths/contracts, resources, and next action.
Record the authoritative coordination path, last checked revision, applicable
notice revisions, acknowledgements, and pending cross-phase requests.
Distinguish implementation, review and verification status from `accepted in phase`
and `integrated`; never
infer plan completion from one phase's pass. List focused checks per batch and a
final gate covering affected behavior and interfaces. Retain failed/unavailable
checks and evidence distinguishing unrelated failures from regressions.

At phase startup, read [plan coordination](references/concurrent-phases.md#plan-coordination).
Before concurrent work or adopting existing shared records, also read the rest of
that reference for worktree/resource isolation, legacy adoption, shared integration
edits, and combined acceptance.
Do not migrate another running phase's records. A single phase uses this same
layout without requiring a plan-wide coordinator or an integration project.

A checks the request against actual code, follows repository discovery rules, and
traces affected shared symbols and callers before changing them (in this repo,
`graft callers <sym> --depth all`). Use the existing plan when clear; otherwise A
records only the necessary decisions, affected contracts, failure behavior, and
verification approach. Clarify blocking product requirements with the user;
resolve routine implementation choices from the system.

## Large batches and review cadence

Review is expensive at Opus/xhigh for B; spend it on fewer, larger artifacts that remain
reviewable.

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

**Verification scope.** Run only focused tests for behavior, interfaces, and
dependencies affected by the changes, including affected callers and shared
contracts. Select checks from the traced impact, not merely the edited files.
Do not run unrelated tests or a full suite by default; honor explicit user or
higher-priority requirements for broader verification. Apply this scope to author,
reviewer, correction, and final integration checks.

When a check fails, investigate enough to establish whether the changes caused
or exposed a defect in affected behavior. Fix regressions within scope. Leave
unrelated failures and their code, tests, fixtures, and configuration unchanged;
record the command, failure, and evidence that it is unrelated, and report it
separately. Do not skip or weaken assertions to obtain a pass. If the relationship
is uncertain, retain it as an unresolved verification gap rather than claiming
it is unrelated. Confirmed unrelated failures do not block acceptance of the
verified affected scope or justify claiming the whole suite passes.

1. A implements the batch, audits the diff, and runs meaningful focused checks
   under the verification scope above. Cover relevant failure paths; changed
   concurrency protocols need competing-claimant tests where applicable. Record
   commands, results, environment/fixture identity, and log paths in `TASK.md`.
2. Freeze the artifact: run [scripts/snapshot.sh](scripts/snapshot.sh) from the
   repository root with this phase's record directory as exclude, e.g.
   `cd "<worktree>" && bash "<skill-dir>/scripts/snapshot.sh" plans/<topic>/phases/p07/docs`
   with literal absolute paths, and record base commit and digest. Exclude only
   workflow records, never implementation or deliverable documentation; use narrower record paths if
   the docs directory also contains deliverables. Record the exact exclusion list
   with the digest and reuse it unchanged for A, B, and root's verification.
   Record the phase starting commit, review-base commit and current HEAD separately:
   the script identifies HEAD plus staged/unstaged/untracked changes; it does not
   select the complete batch diff. Review committed changes since the agreed batch
   base as well as working-tree changes. Identify preserved pre-existing work
   separately. Freeze this phase's entire implementation worktree during review;
   another phase may continue in its own worktree. Do not hide sibling source
   edits with exclusions. Unexpected changes, rebases or dependency updates require
   a new digest and affected review. Serialize writes and shared-fixture checks
   within the phase; use isolated fixtures across phases.
   Pin the applicable coordination notice revisions and retained evidence in the
   review record; a mutable shared file alone is not a review input identity.
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
   defects, and all required checks for affected behavior passing. Failed or
   unavailable affected checks remain unverified; document confirmed unrelated
   failures separately. At completion, run outstanding focused checks and have B
   review combined integration effects not already covered. Later substantive
   edits invalidate affected acceptance. For concurrent phases, follow the
   separate combined-integration gate in the concurrency reference before marking
   the combined result complete.
   Recheck plan coordination before acceptance: applicable active notices must be
   acknowledged and satisfied or have an explicit agreed disposition. Changed
   applicable requirements need affected checks and review, even if source digest
   is unchanged; unrelated coordination updates do not invalidate acceptance.

Stop correction loops that yield no new evidence: identify the blocker, revise the
approach within scope, or request a missing decision. There is no fixed correction
count. Never weaken criteria or substitute self-review to finish.

Retain notes, findings, gated-decision text, and supporting evidence after
completion or compaction; they are evaluation artifacts, not disposable logs.
Record candidate instruction improvements with evidence for later analysis without
editing skills, `AGENTS.md` or `CLAUDE.md` during the run.

## Context: preserve continuity, target compaction at 75%

Target compaction at 75% of the effective context window for root, A, and B,
continuing in the same role, settings, and agent ID. This is not a retirement
threshold; do not replace workers simply because they reach 50% or 75%.

- Claude Code enforces the target only through `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75`
  in a settings `env` block, which covers the main session and subagents. The model
  cannot run `/compact` or read live occupancy itself. At task start, root checks
  the effective value; read [Claude Code context handling](references/context.md)
  when verifying or configuring it. If it is unset or unverifiable, tell the user
  it is required for the 75% target, offer to add it, and continue with default
  auto-compaction disclosed, never claiming 75% compaction occurred, unless the
  user explicitly requires a strict threshold.
- Compaction gives no warning, so keep this phase's `TASK.md`/`REVIEW.md` current
  at every assignment boundary and refresh `HANDOFF.md` at batch boundaries and
  before a pause or session transfer: requirements, original gated decisions,
  artifact digest, accepted and unfinished work, finding IDs, evidence, resources,
  authorizations, agent IDs, actual settings, next action. After compaction,
  reconcile these records and the worktree and resume. Compaction does not end the
  task or invalidate unchanged acceptance evidence.
- Treat headroom as unknown unless the user reports a reading (`/context` is
  user-facing). Reserve headroom for tool results and checkpointing; never invent
  occupancy or equate cumulative spend with current context. Bound individual reads
  and work segments without adding unnecessary review rounds.
- Keep tool output small: save long output in `logs/` and surface results and the
  evidence needed to judge a defect. Root receives status, IDs, and acceptance
  evidence, not every patch or log.

Replace a worker only when unavailable or context loss prevents reliable
continuation after checkpoint recovery. Stop the outgoing worker with `TaskStop`
and confirm it stopped before replacing it. A replacement reads original sources
and relevant records, checks the worktree, and retains its predecessor's role and
settings. A fresh root reconciles records and starts fresh workers instead of
trusting stale agent IDs. Never use A's summary as B's requirements source.

Detailed spend accounting is optional. When benchmarking is requested, read
[benchmarking](references/benchmarking.md). Otherwise use readily available usage
without building telemetry or blocking implementation on missing metrics.

## Completion

Carry existing authorizations into assignments; this skill adds no authorization
for commits, deployment, publishing, production resets, or external messages.
Before returning, stop this phase's outstanding worker activity (`TaskStop`) and
clean up only its disposable resources. Preserve unmerged worktrees, changes and
review evidence; never stop another phase's workers or clean up its databases,
ports or caches. Report completed scope, independent review rounds (with note IDs),
verification, and remaining gaps. Save resumable state if blocked; never call
unverified work complete. When implementation is complete, end the report by
asking whether the user wants the full test suite run, naming the repository's
suite command when known. Only ask; do not run it.
