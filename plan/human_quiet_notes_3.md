# Human Quiet Notes in Existing Threads

## Expected result

An authorized teammate can add a private note, attachment, transcript, or instruction directly to an existing thread from the mailbox UI or API. The note is saved as context without triggering an agent, sending email, changing ownership, or notifying customers. A later explicit action can ask the owning agent to use the accumulated context.

Email suffixes and body commands remain supported for compatibility, but users no longer need to know them for normal collaboration.

## Plan summary

1. Define a first-class `add internal note` use case that targets a resolved company, channel, and thread.
2. Reuse canonical ingestion and thread-history rules where appropriate while avoiding fabricated SMTP identity or delivery events.
3. Record author identity, creation source, visibility, attachments, timestamps, and optional structured note type.
4. Add mailbox controls for note composition, attachment upload, and an explicit separate `ask agent` action.
5. Ensure adding a note does not enqueue a background task; make the operation idempotent for client retries.
6. Update prompt assembly so future runs include notes with clear provenance and internal-only labels.
7. Add authorization, audit, retention, attachment-security, and concurrency tests.

## Scope decisions for the detailed plan

- Decide whether notes can be edited or only superseded/deleted with an audit trail.
- Define supported note types: free text, instruction, decision, transcript, or evidence.
- Define mention behavior: informational mention versus an explicit trigger to act.
- Specify attachment limits, malware scanning, and access control.
- Decide whether API/integration-created notes use the same use case as human notes.

## Dependencies and sequencing

Depends on the visibility model. Ownership is needed for the later `ask owner to act` operation, but basic private note creation can be implemented independently once visibility is stable.

## Acceptance signals

- Adding a note creates no agent task and no outbound email.
- Notes appear in the correct thread with accurate human attribution.
- The next explicit agent run receives the note exactly once in context.
- Unauthorized company or channel users cannot create or read notes.
- UI actions make `add note` and `send/reply` difficult to confuse.

