# Answer Provenance and Human Review Gate

## Expected result

Before an external response governed by review policy is published, an authorized reviewer can
inspect the exact draft, recipient snapshot, and response-level evidence list. Approval applies to
one immutable version. Editing, recipient changes, or reassignment cannot accidentally publish an
older decision.

Provenance records evidence and concise decision rationale, not hidden model reasoning. The system
does not fabricate sentence-level certainty or promise provider-level exactly-once delivery.

## Draft and provenance model

- Expand the shared `ResponseDraft` foundation with immutable versions, lifecycle status,
  reviewer principal, source handoff generation when applicable, and created/updated provenance.
- Store a recipient and transport/binding snapshot per version. Any body, attachment, recipient, or
  destination change creates a new version and invalidates approval of the prior version.
- V1 provenance is a response-level list of stable source references: canonical message or thread
  association, note, attachment object/hash, delegated result, or retained tool result. Each
  reference records source version/hash, audience/access scope, and `DirectEvidence` or `Inference`.
- Do not implement sentence-span attribution or unsupported-claim warnings in V1. Do not store
  chain-of-thought, hidden reasoning, or free-form "reasoning summaries." An optional bounded
  reviewer rationale records only the decision made.
- Retain source identifiers rather than copied excerpts by default. An external URL records URL,
  retrieval time, and content digest; an unavailable or no-longer-authorized source is shown
  honestly instead of reconstructed.

## Review policy and actions

- Add `ExternalResponseReview::{Autonomous, ReviewAllExternal}` at company level with a nullable
  channel override. Existing and new records default to `Autonomous`; risk classifiers and
  recipient-specific policy are deferred until measurable requirements exist.
- Resolve an explicit reviewer principal when the draft is created: current eligible human task
  owner first, then the channel's configured preferred approver, then the company owner. Fail closed
  if review is required and none is eligible.
- Keep response review separate from existing tool-action `HumanApproval` records. They may share
  UI vocabulary and notification infrastructure, but not lifecycle rows or tokens.
- Support approve, edit, reject-with-feedback, and reviewer reassignment. Reassignment changes the
  review responsibility, not task ownership. Editing creates a new version that requires a new
  approval.
- Every action carries expected draft version and command UUID. Stale forms conflict; idempotent
  retries return the original decision.

## Publication and UI

- Approval atomically records the decision and publishes the exact approved version into one new
  `ExternalConversation` canonical message plus one logical delivery. A retry cannot enqueue a
  second logical delivery.
- Treat provider `outcome_unknown` honestly: surface a delivery decision and do not blindly retry.
  Exactly-once guarantees apply only to internal logical message/delivery creation.
- Show draft, recipients, destination, evidence list, inaccessible-source warnings, visibility
  warnings, feedback, and version history. Remove V1 UI promises about claim-level support that is
  not captured.
- Notification delivery for review assignment belongs to the final notification plan; the pending
  review itself is an authoritative operational work item.

## Test and acceptance plan

- Test edit/approve, recipient-change/approve, reassignment, approval expiry, owner transfer,
  concurrent reviewers, stale forms, and duplicate commands.
- Prove only the approved version and recipient snapshot can publish and that private source
  material cannot be copied into delivery by reference.
- Verify every retained source is either openable under the reviewer's current authorization or
  explicitly shown as unavailable, without leaking labels or excerpts.
- Cover delivery retry, outcome unknown, cross-company references, restricted channels, deleted
  attachments, and manual-handoff drafts.
