# 05 — Flow measures, bottlenecks, and capacity

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependencies

Show where procedures spend time, require correction, or overload people. Batch A depends only on
[01](01-shared-contracts-and-workflow-integration.md) and can proceed alongside 02–04 and 06.
Batch B additionally needs 04's responsibility/handoff semantics. This group owns measure semantics
and flow/capacity views; shared admission/activation SQL has one integration owner with 07.

## Batch A — Measures, projections, and advisory WIP

1. Define a controlled, versioned measure catalogue: outcome rate, first-pass acceptance, reported
   correction/defect rate, lead time, work time, waiting/queue time, human-review and handoff
   latency, rework, overdue work, active work, and automation cost/intervention. An unobserved
   defect is missing evidence, not evidence of zero defects. No user-authored SQL or unbounded
   metric keys. Label estimated provider costs and unavailable touch-time measurements explicitly.
2. Derive measures from authoritative run/execution/decision/action timestamps and 01's coordination
   events. Distinguish technical retries from business revision rounds; exclude sample, debrief,
   and improvement-analysis runs from production cohorts. Specify how descendants roll up to a root
   case without double-counting. Concurrent intervals use a documented union/critical-path rule
   rather than adding overlapping durations and calling the result total elapsed time.
3. Define advisory WIP by workflow, step, role, and resolved human principal. Publish exact count
   semantics: workflow WIP counts nonterminal production root cases; human WIP counts open assigned
   business decisions; step views distinguish runnable/executing work from parked waits. Group
   reviewer assignment is displayed as group work, not multiplied into one task per member.
4. Provide authorized, paginated flow views by workflow version: counts, oldest work, work versus
   wait, first-pass acceptance, rework, and transition latency. Show distributions/sample sizes
   alongside averages. Do not create individual performance scores. Case drill-down rechecks source
   ACLs; aggregate access cannot reveal a hidden case through labels, small cohorts, or filters.
5. Use bounded queries with statement/lock deadlines. If a cached projection is necessary, give it
   a source watermark, idempotent refresh, lag indicator, bounded catch-up, and a source rebuild
   path. It is read-only with respect to run progress. Keep operational `/metrics` free of company,
   workflow, run, or user identifiers; business slices belong behind authorized application reads.

Focused checks: add `sop_flow_` tests using known-timestamp DB fixtures for overlapping work/waits,
retries versus revisions, descendant roll-up, missing observations, multiple versions, restricted
cases, pagination, and stale/rebuilt projections. Run existing workflow metrics/fairness tests.
Verify query bounds with realistic backlog and poison/slow sources, not only an empty database.

## Batch B — Enforced human/step WIP in the existing engine

1. After advisory evidence, support explicit published limits on open human business decisions and
   active step executions. Workflow-level WIP remains advisory in this increment; existing tenant
   admission quotas remain in force. Treat idle parked waits separately from leased worker slots.
2. Reserve capacity transactionally at the existing decision-open/activation boundary. If full,
   park eligible work as a workflow-owned capacity wait with deadline and fair recheck/backoff.
   A committed case is never dropped, silently skipped, or represented only in an in-memory queue.
   Continue to charge original run deadlines and budgets while waiting.
3. Store authoritative capacity reservations in the owning transaction, not in delayed analytical
   projections. Release them on completion, timeout, cancellation, and the appropriate terminal
   path. Recovery reconciles against the source execution/decision without releasing live work.
   Handoff acceptance moves any principal reservation atomically or leaves the old assignment and
   reservation intact when the destination is full.
4. Extend existing activation/wait maintenance and fair polling; add no WIP scheduler. Bound claimed
   batches and isolate failures. An explicit coordinator override requires current authority,
   expected revision, bounded extent, reason, and audit; it does not bypass tenant/worker/provider
   resource limits or protected-action policy.

Focused checks: DB-backed `sop_flow_` tests must race claimants for the last slot and handoff versus
slot acquisition/cancel. Assert no over-allocation, lost work, or leaked reservations across restart.
Exercise a full poison batch twice without advancing time and a quiet tenant under sustained load.
Run shared admission/capacity, handoff, worker fairness, and deadline regressions after integration.

## Acceptance

- A team can locate waiting, rework, and overload using documented measures with visible data gaps.
- Read projections never decide authorization or allocate capacity.
- Enforced limits preserve durable work, deadlines, fairness, and reservation accounting.
- Raising a bound retains an early-failing CI regression or resource-budget alarm.
