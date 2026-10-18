# 05 — Human decisions, comments, feedback, and waits

## Outcome and dependencies

Depends on phases 1–4. Implement a general human task returning a choice, feedback, and optional
structured data. Approve/reject is one configuration, not the underlying domain model.

## Schema and data

Pre-release: nothing is deployed, so existing data is not preserved. Schema changes in this phase
edit the squashed init migration (`migrations/20260817000000_init_schema.sql`) in place, and the
development and test databases are dropped and recreated rather than upgraded. Do not add additive
migrations, backfills, compatibility shims, or populated-upgrade tests; verify against a freshly
migrated schema.

## Decision contract

`decision.human` declares the proposal/context to review, an explicit list of 1–16 company members
as reviewers, named choices, editable data schema, feedback requirements per choice, and deadline. Choices map
to workflow-defined routes. Reviewers choose actions; they cannot submit arbitrary next-step IDs.

A submitted result has this shape:

```json
{
  "choice": "revise",
  "feedback": "Remove the delivery guarantee and offer two alternatives.",
  "data": { "priority": "high" }
}
```

Store reviewer identity, submission time, decision revision, and reviewed artifact identity/version
as authoritative metadata. Decisions can return an edited artifact in `data`; original agent
outputs remain immutable. Require schema-valid edited data before accepting the submission.

| Action | Meaning |
| --- | --- |
| Accept | Continue with the accepted/edited artifact |
| Request changes | Return feedback for a new revision |
| Escalate | Route proposal and feedback to another human or agent |
| Choose alternative | Follow another declared route with structured data |
| Cancel | Follow the workflow's cancellation/end outcome |
| Add comment | Append internal discussion without settling the decision |

## Comments versus submission

Provide separate `AddComment` and `SubmitDecision` commands. Comments are append-only discussion
events and leave the decision open. Submission explicitly supplies the final choice, feedback,
and data. Discussion is not implicitly concatenated into agent input; the submitted feedback is
the handoff. A reviewer can add comments without inadvertently restarting execution.

Use expected decision/artifact revisions and command idempotency. Exactly one valid submission,
timeout, or cancellation wins. An accepted decision and its continuation event commit together.
Invalid edits leave the decision open. Late responses for old review rounds cannot affect a new one.

Comments and feedback are internal by default, governed by run/thread visibility. Prompt input
maps feedback explicitly; outbound message steps map the accepted artifact, never the entire
decision object. This prevents structural disclosure, although model output still requires the
configured review and authorization controls.

## Assignment, authorization, and notification

Reviewers are an explicitly named list of company members — no reviewer groups, no
membership-derived pools and no external reviewers; the first valid authorized response settles
the task. Recheck current membership and resource access on submission. Keep
deterministic assignment separate from model choice. Surface missing eligible reviewers as a
configuration/runtime failure, not automatic approval.

Reviewers act only through authenticated member routes; email-only (non-member) reviewers and link
credentials are not supported (user decision 2026-10-18). Notification intents are durable
and contain the precise decision identity; ordinary email replies never settle the newest open
decision heuristically. Notification delivery failure does not imply approval.

Business decisions and protected-action approvals share the decision infrastructure but retain
different subjects. An action approval authorizes one frozen invocation/digest. An accepted draft
does not grant unrelated tool permissions. Protected actions default to rejection on expiration.
Business decision timeouts take an explicitly declared route, or fail the run if none is declared.

Action steps may declare `approval: {reviewers, deadline}`. Reviewers must be explicit users
selected by literals or binding parameters, and must be owners or admins both when the action
parks and when they submit. Self-approval is allowed. The fixed choices are `approve` and
`reject`, with empty-object data and optional feedback. The deadline cannot outlive the run.
Missing configuration, an ineligible reviewer, or no remaining attempt fails with
`<family>.approval_required` without parking.

Approval binds the frozen invocation, argument digest, and operation digest. Parking closes
the current attempt; approval consumes that attempt and reopens the job for a fresh claim.
Current resource and actor access are still checked before dispatch. One approval permits one
remote entry, including when reconciliation later proves that entry had no effect; an existing
receipt remains readable without another dispatch. Rejection and expiry take the action's
normal final-error route. An approved message may subsequently wait for delivery acceptance,
retaining both the settled approval wait and the separate effect wait.

## Other waits

Implement timer/event waits with scoped correlation, payload schemas, deadlines, and atomic
consumption. Persist correlated events before signaling workers so an event arriving before the
worker parks is not lost. Duplicate signals cannot resume an execution twice. Unmatched ordinary
messages remain independent ingress events.

## Acceptance

- Comments do not advance execution; submitted feedback reaches only explicitly mapped steps.
- Test competing reviewers, duplicate submissions, timeout/submit and cancel/submit races.
- Test revoked membership, non-member and non-reviewer refusal, cross-company access, and stale
  artifacts.
- Edited artifact validation, missing required feedback, and unknown choices fail without settlement.
- No approved action dispatches with arguments different from those reviewed.
- Restart and event-before-park tests prove waits do not depend on in-memory listeners.
