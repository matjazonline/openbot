---
name: implement-astra-reviewed-2
description: "Execute complex implementation with Astra authorship and independent Astra review, using cohesive batches, selective architecture gates, and lightweight handoffs. Use when an implementation request selects this workflow; not for standalone planning or review."
---

# Implement Astra Reviewed 2

Preserve independent scrutiny of architecture and code while minimizing repeated
onboarding, coordination, and review. Savings and quality must be measured, not
assumed. User preferences override these defaults.

## Roles

- **Author (A):** owns investigation, consequential decisions, implementation,
  fixtures, tests, and corrections.
- **Reviewer (B):** independently evaluates requirements, architecture, actual code,
  and verification. Returns findings or an explicit pass; does not edit project
  files or author the design. May maintain its review record and the findings log
  described below, and run checks.
- **Root:** owns scope, assignments, resource coordination, acceptance, and user
  updates. When separate from A, it does not repeat implementation or technical review.

When authoritative session information confirms root is `gpt-6-astra`, root also
acts as A and spawns only B. Otherwise root spawns A and B as direct children.
Keep root's actual settings; instructions cannot change its model. Spawn workers
with `gpt-6-astra`, medium effort, and `fork_turns="none"`. Workers do not spawn
agents. Never combine authorship and independent review in one agent or swap roles.
If Astra or delegation is unavailable, disclose the limitation and obtain an
authorized alternative before substituting a different workflow.

Give workers compact assignments with original requirements and artifact paths,
applicable instructions, scope, and acceptance criteria. Do not copy conversation
histories into B. Reuse eligible workers through related work and corrections;
spawn B when its first review is needed.

## Scope and batches

Record the starting commit, pre-existing changes to preserve, criteria, and ordered
dependencies. A checks the request against actual code and follows repository
discovery rules, including tracing affected shared symbols and callers.

Implement cohesive features or dependency groups, small enough for thorough review.
Do not require a return or acceptance gate for every internal step. Within a batch,
A may advance dependent steps except across a pending architecture gate. Accept
each batch before building a later batch on it. Split oversized batches at stable
contracts; honor user-specified ordering and checkpoints.

Use the existing plan when clear. Otherwise A records only the necessary decisions,
affected contracts, failure behavior, and verification approach. Clarify blocking
product requirements; resolve routine implementation choices from the system.

## Selective architecture gate

Before dependent edits, request B's review of consequential **new or changed**
decisions about transaction boundaries, authorization, concurrency, persistent
ownership, migration/recovery, or contracts spanning subsystems. Changed assumptions
under an existing design count too. Routine use of an established pattern needs no
separate gate; its correctness remains part of code review.

A supplies the proposed decision, invariants, failure modes, affected contracts,
and verification plan in the task record. B reads original requirements and relevant
code, challenges assumptions, and returns concrete findings together or an explicit
pass with evidence. A resolves findings; B rechecks affected decisions before
dependent edits. This is an internal review gate, not a user approval request.

## Implementation, review, and acceptance

1. A implements the batch, audits the diff, and runs meaningful targeted checks and
   repository-required broader checks. Cover relevant failure paths; changed
   concurrency protocols need competing-claimant tests where applicable. Record
   commands, results, relevant environment/fixture identity, and log paths.
2. Freeze the review artifact. Record its base commit and a reproducible snapshot
   or digest covering staged, unstaged, and relevant untracked content. Identify
   preserved pre-existing work separately. Serialize project writes and tests that
   mutate shared fixtures; do not edit the artifact during review.
3. B reviews the whole batch against original criteria, actual changes and callers,
   decisions, and evidence. Check coverage, integration, failure behavior, and
   relevant security/concurrency/recovery paths. Challenge architectural assumptions
   even if previously approved. A's summary and passing tests are not proof.
4. B persists all concrete findings together in the findings log below before
   returning them, and links their IDs from the review record. If there are no
   findings, record an explicit pass identifying coverage and evidence. Separate
   actionable defects from optional preferences; optional cleanup does not block
   acceptance. A addresses defects and reruns affected checks.
5. B rechecks the correction delta and affected behavior using finding IDs, changed
   files, and new evidence. Expand review when fixes affect other contracts or
   assumptions; material architectural changes reopen the relevant early gate.
   Reuse unaffected evidence only while code, environment, and fixtures support it.
6. Root accepts only with B's explicit pass for the current artifact, all findings
   and their dispositions saved, and all required checks passing. Failed or
   unavailable checks remain unverified. Do not repeat a full phase review of an unchanged, already covered artifact. At completion, run
   outstanding required broader checks and have B review any combined integration
   effects not covered by batch reviews. Later substantive edits invalidate affected
   acceptance.

Stop correction loops that yield no new evidence: identify the blocker, revise the
approach within scope, or request a missing decision. There is no fixed correction
count. Never weaken criteria or substitute self-review to finish.

## Durable reviewer findings

Use `docs/agent-review-findings.md`, relative to the repository root, as the shared
findings log across tasks using this skill. Create it on the first finding; preserve
existing entries. Root includes its resolved path in worker assignments and handoffs.
B owns entries; serialize writes across reviewers. This log is a review artifact,
excluded from the frozen implementation snapshot, so recording findings does not
invalidate the artifact under review.

Log every concrete issue from architecture review, batch review, and correction
rechecks, including optional suggestions labeled as such. Chat-only findings are
insufficient. Use one Markdown section per finding with these fields:

- **ID and context:** stable task-prefixed ID, date, task/review record links, batch,
  review stage, reviewer model/effort, and reviewed commit or snapshot identity.
- **Classification:** actionable defect or optional suggestion, severity, and category
  (for example requirements, architecture, correctness, security, or verification).
- **Evidence:** file/line or decision references, observed versus required behavior,
  impact, and reproduction or missing verification; link supporting logs.
- **Instruction lesson:** violated or missing guidance and a concrete candidate
  improvement when supported by evidence; otherwise mark the cause as unknown.
  Distinguish observations from hypotheses. Logging does not authorize edits to
  agent instructions.
- **Resolution history:** initial open status, then dated correction and recheck
  evidence with artifact identity. B records fixed, deferred, or dismissed outcomes
  with reasons; an author's claimed fix alone does not close a finding.

Preserve the original finding and append resolution updates, including dismissals;
never delete resolved issues. Reuse IDs for rechecks of the same issue, and link
recurrences in later tasks. Keep detailed findings here and link them from task and
review records rather than duplicating their text. Explicit passes and review
coverage remain in the review record. Before acceptance or handoff, root checks
that the log includes every reported issue and its latest disposition.

## Compact records and context

Maintain one task record, one review record, and logs beside established task
artifacts. Root owns scope/status/acceptance fields, A decisions/checks, and B findings
and review coverage. Serialize shared record updates. Link originals and logs rather
than copying them. Record current batch, artifact identity, open issues, role handles,
actual model/effort, resource ownership, and next action. Separate implemented,
reviewed, verified, and blocked states. Do not create separate per-step reports.

At substantial assignment boundaries, inspect available context signals and reserve
headroom for the next assignment, tool results, and checkpointing. During long work,
check again if context grows materially. Prefer continuity while relevant context
fits; rotate at a stable boundary when headroom is insufficient, history is mostly
unrelated, or context loss prevents reliable continuation. Do not invent occupancy
estimates, impose fixed percentage retirement, or restart solely because telemetry
is unavailable. With unknown headroom, use smaller assignments and durable records;
checkpoint if reliable continuation is no longer possible.

Create a handoff only for interruption or rotation, using the task/review records:
requirements, decisions, exact artifact, accepted and unfinished work, findings,
evidence, resource identities, authorizations, and next action. Stop the outgoing
worker before replacing it; a replacement reads originals and relevant records and
checks the worktree before proceeding. A fresh root reconciles state and starts fresh
workers. Do not use an author's summary as the reviewer's requirements source.

Detailed spend accounting is optional. When benchmarking is requested, read
[benchmarking](references/benchmarking.md). Otherwise use readily available usage
without building telemetry or blocking implementation on missing metrics.

## Completion

Carry existing authorizations into assignments; this skill adds no authorization
for commits, deployment, publishing, production resets, or external messages.
Before returning, stop outstanding worker activity and clean up only task-created
disposable resources. Report completed scope, independent review, verification,
and remaining gaps. Save resumable state if blocked; never call unverified work
complete.
