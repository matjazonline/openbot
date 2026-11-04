# Concurrent phases and integration

Read before starting a phase alongside another, adopting legacy records, or
integrating independently accepted phases. Separate progress records prevent
bookkeeping collisions; separate worktrees and resources prevent code and test
collisions. This workflow supports independent sessions as well as one coordinator.

## Plan coordination

Every implementation phase uses one `<plan-dir>/COORDINATION.md` at the plan root,
including phases run sequentially or in independent sessions. Honor an existing
plan directory name (`plan/<plan-id>/` or `plans/<plan-id>/`); do not create a
parallel plan tree. It holds shared instructions and changes, not duplicate phase
progress logs. Phase TASK/REVIEW/HANDOFF records remain authoritative for execution
and acceptance.

At startup, before assigning implementation, resolve the authoritative file's
absolute path in the designated plan worktree and its single named writer.
Consult existing plan/handoff pointers before creating it. If absent, the first
phase root establishes it and becomes its initial writer; later phase roots reuse
that authority. Resolve conflicting copies or ownership before shared-contract
work. Never assume a local worktree copy is current. If the authoritative file is
unavailable, report the gap and pause dependent work rather than treating it as
empty; independently verified unaffected work may continue.

Use this minimal structure; empty tables are valid for a new plan:

```markdown
# Plan coordination: <plan-id>

- Authority: <absolute path to this file>
- Writer: <root/session identity and worktree>
- Revision: <monotonically increasing integer>

## Plan-wide instructions
<!-- Stable IDs/revisions for shared requirements, or “None”. -->

## Phase directory
| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |

## Cross-phase notices
<!-- One entry per notice; preserve prior revisions and dispositions. -->
### <plan-id>-C001 / r1
- State: proposed | active | superseded | withdrawn
- Origin: <phase, owner, TASK decision/finding link>
- Targets: <explicit phase IDs, or all current and future phases>
- Instruction/change: <contract, invariant, or required action and rationale>
- Dependencies/order: <producer artifact, effective point, blocking scope>
- Evidence/review: <decision revision, reviewer pass if gated, supporting paths>
- Verification: <observable acceptance criteria and focused checks>
- Responses: <phase -> pending | acknowledged | blocked | satisfied | not applicable;
  exact notice revision and link to that phase's own TASK/REVIEW evidence>
```

The named writer alone serializes updates and increments the file revision.
It may be a phase root; no extra agent or permanent coordinator is required.
Record an explicit ownership handoff before another root writes. Other roots
record publication requests or responses in their own TASK/HANDOFF and pass their
paths to the writer through available authorized coordination. The writer checks
registered phase requests at batch/acceptance boundaries and publishes them;
do not silently edit another phase's files or send unauthorized external messages.

When a phase discovers an instruction or change affecting another phase:

1. Record its cause, affected contracts/targets, required action, ordering, and
   verification in the originating TASK. Request a stable notice ID from the writer
   before dependent edits. Pure retrospective lessons remain candidates for later
   instruction improvement; a live plan contract belongs here.
2. The writer publishes a proposed notice. Resolve conflicts against original
   requirements with affected owners. Consequential architectural changes require
   the owning phase's B to pass the decision revision before activation. Notices
   do not expand user authorization or override higher-priority instructions.
3. Activate the agreed revision with explicit targets and its effective/blocking
   point. Receivers recheck at startup, resume, batch start, before shared-contract
   edits, and before review/acceptance. Each receiver records the exact revision,
   impact, and response in its own TASK/HANDOFF; the writer links that response.
   Reading is not acknowledgement, and acknowledgement is not implementation.
   Future phases evaluate notices targeting all phases when they register.
4. Before affected work, the receiver acknowledges the active revision or records
   a conflict/blocker for resolution. Independent work continues; affected work
   waits for the required agreement, reviewed decision, or producer artifact.
   Mark satisfied only with verification evidence, or record an agreed
   not-applicable disposition with rationale. Do not wait for all future phases
   to acknowledge before completing an otherwise satisfied current phase.
5. Publish changes as new notice revisions; retain prior text and responses with
   their original identities. A new applicable revision needs a new impact check
   and acknowledgement. Reopen only affected decisions, implementation, checks,
   and review; a change can affect an accepted phase's integration readiness
   without rewriting its historical pass. Never resolve conflicts by last writer.

Root supplies A and B the authority path and exact applicable notice revisions.
Retain an immutable copy or extract of those revisions in phase logs with each
review, including plan-wide instructions. B checks them against original
requirements and confirms freshness before returning; root checks again before
acceptance. Preserve applicable decisions and evidence for dependent phases.
Coordination records cannot replace reviewed producer artifacts. Do not introduce
polling services, heartbeat files, or a scheduler for this protocol.

## Establish the workstream

1. Confirm the requested phase is authorized. Read its prerequisites and trace
   affected contracts against current code. Distinguish a hard dependency from an
   optional follow-up. Record any scope deferred until another phase is accepted;
   a partial implementation is not an accepted complete phase.
2. Record phase ID, owner/session, worktree, branch, immutable starting commit,
   prerequisite artifact identities, owned code areas and shared integration files
   in the phase's TASK. Independent phases can start from a suitable committed
   baseline; a dependent phase needs the required accepted artifact, not merely a
   status label. Do not copy another session's evolving dirty tree. If needed work
   is uncommitted, coordinate a stable export with its owner and record its identity.
   Never stash, reset, commit or otherwise reshape another phase's work for setup.
3. Use a separate worktree/branch for each concurrently implemented phase, normally
   outside the primary working tree so it cannot enter that tree's snapshots.
   Branch/worktree creation is routine setup; it does not authorize commits or
   merges. Record the absolute cwd in every assignment and run every command there.
   With multiple agents sharing one checkout, serialize implementation and frozen
   reviews; do not describe simultaneous source edits as isolated async work.
4. Allocate distinct test databases (or clusters when tests alter cluster-wide
   state), application ports, temporary/output directories and build caches such
   as `CARGO_TARGET_DIR`. Test database prefixes must remain disjoint even if a
   harness enumerates or cleans up databases. Isolate browser/server processes and
   frontend output through the worktree. Record resource identities and ownership;
   serialize checks when isolation or machine capacity cannot support concurrency.

Keep author/reviewer roles separate within each phase. A reviewer must not review
code it authored in another phase. Do not transfer an active reviewer between
phases with ambiguous records or resource ownership. Respect the host's agent
limit: queue reviews or use separate sessions instead of dropping independence,
changing model settings, or terminating another phase's workers to free a slot.

## Progress without shared writers

For example, two new phase workstreams use:

```text
plans/business-event-gaps/
  COORDINATION.md
  phases/
    p02/docs/{TASK,REVIEW,HANDOFF,agent-review-notes,agent-review-findings}.md
    p02/docs/logs/
    p07/docs/{TASK,REVIEW,HANDOFF,agent-review-notes,agent-review-findings}.md
    p07/docs/logs/
```

Each path is resolved within that phase's worktree. Worktrees do not automatically
synchronize files: a coordinator must consult the owning worktree's authoritative
records or an explicitly handed-off immutable copy. A stale copy in its own
checkout is not current phase status. Exported evidence must remain retrievable
after a worktree is removed; preserve logs and reviewed snapshots with their IDs.

Use the required plan-root coordination file above instead of a second overview.
If an older `<plan-dir>/docs/COORDINATION.md` exists, preserve it while active
sessions rely on it; the named writer establishes the root file and records an
explicit authority pointer/handoff at a safe boundary. Preserve history and make
the authoritative path explicit in phase assignments; do not leave two live
authorities or move another session's records silently.

Use distinct states for work in progress, awaiting dependency, awaiting review,
changes required, accepted in phase, and integrated. Record the specific blocker
and next action. Acceptance applies to the reviewed artifact and environment;
it neither accepts a sibling phase nor proves the combined result.

## Shared contracts and files

Identify overlap before edits: module/route registration, common persistence
interfaces, shared fixtures, migration ordering, generated CSS, root README and
lock documentation are typical integration points. Record the intended merge
order and who resolves each overlapping contract. A phase can edit such files in
its own worktree; a contract conflict affecting correctness or independence needs
coordination before dependent implementation, not just a later textual merge fix.

Choose distinct new migration identities. Do not concurrently rewrite a migration
another phase is relying on. Review final ordering, constraints and reset needs
on a fresh database after integration. Regenerate shared derived assets from the
combined sources instead of choosing one phase's generated output.

During a review, freeze that phase's entire implementation worktree. Workflow
record writes may continue under the exact recorded exclusions. Do not broaden
exclusions to hide source changes, generated assets or deliverable documentation.
If the live plan coordination record changes locally, exclude only that exact
workflow-only file and record it before freezing. Pin applicable notice revisions
and immutable review evidence separately: excluding the mutable file from the
source digest never exempts changed requirements from affected review. Another
worktree's changes do not affect this digest. Rebasing or importing a dependency into the reviewed
worktree changes artifact identity and requires affected checks and review again.

## Existing 2.1 records

Do not move or rewrite live `<plan-dir>/docs/` logs or a shared implementation
handoff while another session uses them. A new phase immediately uses its own
`phases/<phase-id>/docs/`; the existing phase may finish under 2.1 at its existing
paths. Record those exact paths when referencing it.

When the owner adopts 2.2 at a safe checkpoint with no workers writing, choose one:

- **Retain paths:** bind the existing record set exclusively to that phase and
  record the resolved path map in its TASK/HANDOFF. Other phases use new paths.
- **Start phase-local continuation:** retain historical records unchanged; create
  the new TASK/HANDOFF with precise pointers to prior acceptance, open findings,
  resources and next action. Announce the authoritative-path switch in the old
  handoff once the owner can safely write it. New review entries go only to the
  new files; findings keep their original IDs and link dated follow-ups.

Never bulk-copy histories into competing authoritative logs or discard old
evidence. Preserve previous workflow names, accepted digests and exclusion lists.
If new records change the current snapshot exclusions, record a new identity and
verify the implementation is unchanged before carrying forward acceptance.

## Integration and final acceptance

Phase root accepts its phase only under SKILL.md's independent-review and required
verification rules. Record its exact base/HEAD/digest, dependency versions,
remaining external dependencies and evidence. A handoff can be ready for
integration without claiming integration has happened.

One integration owner assembles accepted artifacts in the designated integration
worktree when authorized. This skill adds no merge/commit authorization; prepare
a concrete reviewed result before asking for any missing authorization. Keep
combined records in `<plan-dir>/integration/<integration-id>/docs/` using the same
TASK/REVIEW/HANDOFF/notes/findings layout. Record input phase identities, target
base, conflict resolutions and final combined artifact identity. Integration work
does not overwrite phase histories or confer a new pass on changed phase code.
The integration owner also checks current plan coordination and dispositions for
every affected input phase; unresolved applicable contracts block affected
integration even when the individual phase artifacts previously passed.

Run focused checks for conflict resolutions, affected behavior, and shared
contracts on the combined tree under SKILL.md's verification scope, including
fresh-database migrations and CSS generation when affected. Leave confirmed
unrelated failures unchanged and report them separately. Earlier phase passes
alone do not satisfy this gate.
Have an independent B review integration changes and their affected behavior;
reuse unchanged phase evidence where its code, dependencies and environment still
apply. A reviewer who authored an input phase cannot provide independent review
of that input or integration changes affecting it. Resolve findings, freeze and
accept the combined artifact before marking the input phases integrated into it.
Record missing checks as gaps, not passes. No repository-required phase gate is
waived merely because a later combined gate is planned.

At a pause or completion, stop only owned workers/processes. Retain unmerged work,
review records and resources needed for continuation. Remove only verified
disposable resources belonging to this phase; never run broad database, worktree,
cache or process cleanup against shared names.
