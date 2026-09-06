# Human Internal Notes in Existing Threads

## Expected result

An authorized teammate can add private context to an existing thread without triggering an agent,
sending through a transport, changing ownership, resolving a handoff, or notifying a customer. A
separate explicit command can ask the current owning agent to use selected notes.

Email quiet suffixes and body commands remain compatibility ingress mechanisms, but ordinary
mailbox and API collaboration uses the first-class note operation.

## Note contract

- Add one transport-neutral `AddInternalNote` application command containing company, channel,
  thread, bounded text, and a client-generated command UUID. The authenticated principal is
  supplied by the application boundary rather than accepted from the request body.
- Persist the content as an `InternalOnly` canonical message with `ThreadEntryKind::Note`, no
  transport identity, no recipient projection, and no delivery.
- Make notes immutable. A correction creates a new note with `supersedes_note_id`; removal creates
  an auditable tombstone while retaining author, timestamps, and the supersession chain.
- Route human, API, and integration-created notes through the same use case. Non-human callers must
  carry explicit creation provenance and the same company/channel authorization checks.
- V1 supports bounded text only. Add note attachments only after quarantine/malware scanning,
  authorized download, retention, and prompt-ingestion behavior are implemented end to end.
- Do not parse note text for mentions, assignments, or commands in V1. Ordinary notes are silent by
  default.

## Ask-agent action

- Add a separate `AskOwnerToAct` command containing the active task ID, expected ownership version,
  selected note IDs, and a command UUID. Every note must belong to the same readable thread.
- If the active task is agent-owned, append an auditable instruction linked to that task. Wake a
  pending task; fence, cancel, and requeue a processing task so it rebuilds its prompt; leave an
  approval/outreach-waiting task parked and include the instruction when its existing wait
  resolves. An idempotent retry cannot start a second execution.
- If there is no active non-terminal task, expose a separate `StartAgentTask` action that creates
  one for the channel's current position-zero agent. If no eligible agent exists, leave an
  unassigned operational work item rather than silently choosing one.
- If the active task is human-owned or unassigned, require claim/transfer first. A note itself never
  changes ownership or task state.
- Prompt assembly loads notes through a dedicated projection and labels author, timestamp,
  supersession state, and `InternalOnly` audience inside the untrusted-data fence. Superseded or
  tombstoned text is excluded by default.

## UI and authorization

- Provide a dedicated internal-note composer visually separated from external reply and draft-send
  controls. Show present-tense progress only while the write is running and restore controls on an
  in-place error.
- Restrict creation, reading, correction, tombstoning, and ask-agent actions through the exact
  company/channel/thread access predicate.
- Render notes with an unmistakable internal treatment and never include their text in customer
  previews, email notification bodies, or public deep-link metadata.

## Test and acceptance plan

- Adding, correcting, or tombstoning a note creates no agent task, external delivery, ownership
  change, handoff resolution, or customer notification.
- Concurrent retries of one command UUID produce one note; reuse of the UUID with different
  content conflicts.
- `AskOwnerToAct` is fenced against ownership transfer, concurrent resume, and task completion.
- Tests cover cross-company IDs, unreadable channels, wrong-thread note IDs, supersession chains,
  disabled users, bounded text, and hostile rendered content.
- The next explicit agent run receives each active selected note once, with accurate principal
  attribution and internal-only labelling.
