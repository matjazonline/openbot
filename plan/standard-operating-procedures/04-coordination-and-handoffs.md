# 04 — Responsibility, handoffs, and exception coordination

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependencies

Make responsibility, uncertainty, and escalation explicit while ordinary workflow decisions remain
the source of actionable human work. Depends on [01](01-shared-contracts-and-workflow-integration.md).
Can run alongside 02, 03, 05A, and 06; integrate shared decision/wait SQL sequentially with 03.

The current engine has frozen human-reviewer assignments and no live reassignment endpoint. This
group adds a narrowly defined business-decision reassignment protocol; it does not assume that
legacy task transfer or a general human/agent task-owner queue is available.

## Batch A — Brief, responsibility, and exception views

1. Derive a run brief from the published SOP and admitted role snapshot: objective, outcome,
   coordinator, responsibility map, known constraints, dependencies, uncertainties, and declared
   stop/escalation conditions. Display it on the existing run/thread surfaces. For a required
   acknowledgment, author an explicit human decision; opening the brief is not acknowledgment.
2. Show actionable work using existing open decisions and current reviewer permissions. Render
   role labels without widening company/channel visibility. A coordinator role does not confer
   access to otherwise restricted case content.
3. Declare bounded business exception reasons: missing input, conflicting evidence, unavailable
   reviewer, failed check, capacity, and other-with-detail. Route these through named workflow
   choices to coordinator review, bounded revision, or termination. Preserve engine failure classes
   for technical failures; do not reinterpret provider errors as business acceptance or customer
   messages. Group 03 owns quality-check waivers.
4. Coordinator actions use existing authorized cancellation and decision commands, or the new
   handoff command below. Do not mutate graph structure, published agent snapshots, or completed
   results on a live run. Agent changes require a newly published version and explicit replacement
   work after inspecting prior effects; do not transfer an in-flight agent execution.

Focused checks: add `sop_coordination_` tests for frozen role displays, missing/foreign principals,
coordinator ACLs, declared exception choices, no progress from comments/views, and bounded rework.
Run current run-visibility, decision-authorization, and conversation regressions.

## Batch B — Acknowledged handoff of an open human decision

1. Add a bounded handoff record referencing the run and current business decision: sender and
   intended receiver, situation, completed work/evidence, uncertainty, next action, due time, and
   contingency. References retain source ACLs; private handoff text never enters customer delivery.
   Initially support authorized human-to-human transfers of open business decisions only. Protected
   action approvals and token-allowance decisions retain their current assignment rules.
2. An authorized assigned reviewer/coordinator can offer a transfer to a concrete eligible company
   user. Only one offer may be active per decision. The current reviewer set remains responsible
   until acceptance. On acceptance, atomically update that decision's reviewer assignment, advance
   its assignment/decision revision, settle the offer, invalidate stale submission commands, and
   enqueue notices. Preserve the exact proposal/artifact; a handoff cannot change accepted data.
   For multi-reviewer decisions, define the whole replacement reviewer set explicitly and validate
   it; never silently remove other reviewers.
3. Permit accept, decline-with-reason, or one bounded clarification thread. Synchronous review is
   an optional meeting/channel reference on this protocol, not proof of mutual understanding.
   Decline/expiry leaves the existing assignment in place. Once offered, another transfer,
   completion, cancellation, or timeout must fence the offer through the same revision checks.
4. Give offers finite expiry within the original decision/run deadline, bounded reminders, and
   coordinator escalation. Extend existing durable wait/maintenance ownership, with indexed due
   work, leases/backoff, and supervised shutdown; no detached timers or competing handoff scheduler.
   Recheck receiver eligibility and actor authority in the accepting transaction.
5. Responsibility changes for future unopened decisions use explicit audited run-binding amendments
   defined in 01. They do not rewrite the admission snapshot, existing assignments, published roles,
   or tool grants. Record the effective revision so retries resolve the same intended assignment.

Focused checks: add DB-backed `sop_coordination_` tests with two competing acceptors, acceptance
versus decision submission/cancel/timeout/expiry, duplicate and conflicting command keys, receiver
revocation, and stale former-reviewer submission. Assert one effective assignment, one successor,
and durable notices. Test a full poison reminder batch followed by a second iteration at the same
time, then reconstruction after a crash. Browser tests cover offer/clarify/accept and closed offers.

## Acceptance

- A participant can identify the objective, current responsible people, uncertainty, and next step.
- An accepted handoff atomically transfers the declared decision assignment while preserving its
  artifact and history; unaccepted offers never leave work unowned.
- No new general task engine or agent-capability transfer is introduced.
- Pending handoffs and exceptions remain visible, bounded, recoverable, and private.
