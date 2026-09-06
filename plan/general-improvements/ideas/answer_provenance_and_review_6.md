# Answer Provenance and Human Review Gate

## Expected result

Before an external response is sent, authorized reviewers can inspect a draft and understand which messages, attachments, notes, delegated findings, and tool results support it. A configurable review gate allows approval, editing, rejection with feedback, or reassignment. Approved content is dispatched exactly once through the original thread.

Provenance is useful and honest: it distinguishes direct evidence from agent inference and never fabricates precise citations that the system cannot trace.

## Plan summary

1. Define the provenance model for sources, extracted claims, draft spans or sections, and inference labels.
2. Decide the first-release granularity: response-level source list is safer and simpler than unsupported sentence-level certainty.
3. Capture stable source references during prompt construction and delegated-result ingestion.
4. Store response drafts separately from sent messages, with immutable versions and reviewer actions.
5. Add configurable review policies by company, channel, action risk, recipient type, or agent.
6. Pause tasks in a review state and make approve/edit/reject operations authorization-safe and idempotent.
7. Build a review UI showing draft, recipients, visibility warnings, evidence, unsupported-claim warnings, and prior versions.
8. Dispatch only the approved version and preserve the final provenance/audit snapshot.
9. Test stale approvals, concurrent edits, owner transfer, private-data leakage, retries, and approval expiry.

## Scope decisions for the detailed plan

- Choose response-level versus sentence-level provenance for the initial release.
- Define what reviewers may edit and whether edits require revalidation.
- Determine which channels require review and which may send autonomously.
- Define attachment and external-link evidence handling.
- Specify retention and access rules for drafts, reasoning summaries, and source excerpts.

## Dependencies and sequencing

Depends on ownership and visibility. It can consume delegation status and notes once available. Notification delivery for review requests belongs in plan part 7.

## Acceptance signals

- A draft cannot be sent while required approval is pending.
- The exact approved version and recipient set are the ones dispatched.
- Reviewers can open every retained source they are authorized to see.
- Private source material is not copied into external output without an explicit safe draft.
- Approval retries cannot send duplicate messages.

