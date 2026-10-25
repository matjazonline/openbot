# 06 — Debriefs and the improvement backlog

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependencies

Collect evidence about a procedure after work ends and turn it into a human-owned improvement
backlog. Depends on [01](01-shared-contracts-and-workflow-integration.md); can run alongside 02–05A.
Owns debrief and improvement records, commands, and UI. It consumes flow measures when 05A is ready
but does not need analytical projections to accept an observation.

## Batch A — Bounded debriefs after terminal work

1. Configure debriefs as off, deterministic sample, exception-only, or always, with a small question
   and time budget. Trigger from authoritative terminal outcomes and known events such as rework,
   missed deadlines, failed checks, waivers, or high recorded cost. Pin the debrief policy from the
   run's published version; retries must not resample the same case.
2. Ask whether the intended outcome was achieved, what helped, what caused delay/error/uncertainty,
   what should change, and whether the observation appears case-specific or repeatable. Link
   answers to canonical evidence under current ACLs. Treat inferred causes as attributed claims.
3. Add bounded tenant/run-linked debrief records with author, immutable submitted content, stable
   command key, and timestamps. Corrections append a revision; they do not overwrite attribution.
   Eligibility/visibility is checked at submission, not inferred from receiving a notification.
4. Debriefs do not block completion, change success, or reopen their source run. Create invitations
   idempotently through a bounded existing maintenance path or a durable terminal-commit intent;
   notifications use the delivery outbox. Expired invitations close visibly. Avoid a new business
   execution queue: optional agent analysis is a separately admitted ordinary workflow.

Focused checks: add `sop_improvement_` tests for terminal eligibility, stable sampling, duplicate
terminal notifications, concurrent duplicate submissions, revoked access, content bounds, and
source-run immutability. Reconstruct the invitation worker after a committed source transition;
test poison-batch backoff and deadline expiry if polling/maintenance is extended.

## Batch B — Triage and proposed changes

1. Store improvement items with category, source observations/runs, impact/frequency estimates,
   accountable owner, triage state, and optional proposed workflow draft/experiment reference.
   Deduplicate exact submission identities; human reviewers decide whether similar observations
   should merge. Merge/split/reprioritize commands are revision-fenced and retain source history.
2. Show a company backlog with authorized evidence drill-down, concise summaries, and links to
   existing workflow drafts. Participants may report that the procedure is wrong or inapplicable
   without giving up current responsibilities. Avoid blame narratives and participant rankings.
3. Optional agent clustering or draft suggestions use bounded authorized input in an ordinary
   workflow, with provenance and uncertainty. Recheck source visibility before exposing generated
   content. Suggestions cannot merge items, waive checks, publish, or activate changes themselves.
4. Keep evidence retention/access separate from the backlog summary. A derived summary must not
   expose content whose source is now restricted; define invalidation/redaction behavior and audit
   retention before storing generated summaries. Do not push private observations into customer
   threads, delivery bodies, logs, or deployment-wide metrics.

Focused checks: add `sop_improvement_` DB/route tests for concurrent triage, source revocation,
cross-company references, merge history, suggestion provenance, publication authority, and stale
draft links. Browser tests cover submitting a debrief and proposing a draft from a triaged item.

## Acceptance

- Debriefs arrive after work without altering its execution or requiring another SOP runtime.
- A human can trace a proposed change to visible source evidence and decide what to do with it.
- Agent suggestions remain proposals, and private evidence retains its original access boundaries.
