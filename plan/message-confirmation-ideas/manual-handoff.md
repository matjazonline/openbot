# Manual Handoff for Outside Replies

## Summary

Add a durable, shared mailbox handoff workflow instead of per-user unread state.

When an ordinary reply from someone outside the company reaches an existing thread, the channel's effective policy decides whether to run the agent immediately or store the message and mark the thread **Needs instruction**. Opening the thread does not clear it; only a team action does.

## Configuration and Policy

- Add `ExternalReplyHandling::{Automatic, ManualHandoff}`.
- Store `external_reply_handling` on companies, defaulting existing and new companies to `Automatic`.
- Store nullable `external_reply_handling_override` on channels:
  - `NULL`: inherit the company policy live.
  - `Automatic`: always run immediately.
  - `ManualHandoff`: always hold for the team.
- Expose the company policy in company settings and a three-option inheritance selector in channel settings and JSON representations.
- Treat a sender as outside when their resolved `CompanyMembership` is `None`; allowlisted customers remain outside.
- Hold only messages that:
  - Continue an existing thread.
  - Would otherwise cause that channel to answer.
  - Were not explicitly marked `FileOnly`/quiet.
  - Are not correlated outreach/quorum replies.
- New outside conversations and outreach replies retain their current automatic behavior.
- For multi-channel messages, create handoffs only for manual channels while the existing task targets automatic channels normally.

## Shared Attention and Release Flow

- Add a tenant-scoped `thread_handoffs` table with one current row per thread, a generation UUID, source message, state, optional task/draft references, and timestamps.
- States are `needs_instruction`, `drafting`, `replying`, and `draft_ready`, with database constraints enforcing which references are valid.
- Persist the message, handoff transition, task targets, and outreach transitions in the existing atomic inbound commit.
- A later outside reply replaces the current handoff generation. Completion from an older task must never clear or overwrite the newer handoff.
- An authorized team message with normal `Answer` disposition releases the current handoff:
  - `InAppOnly` moves it to `drafting`; successful dispatch moves it to `draft_ready`.
  - `Send` moves it to `replying`; successful dispatch and durable delivery enqueue resolve it.
  - A quiet/FileOnly team note leaves it untouched.
- Terminal task failure or an explicit task stop returns a matching `drafting`/`replying` handoff to `needs_instruction`; retries keep the in-progress state.
- Add an explicit **Generate draft** action. It creates a visible, team-attributed instruction such as "Generate a draft response for team review," queues an `InAppOnly` run, and follows the same fenced state transitions.
- Extend the version-1 inbound task payload with a bounded, defaulted list of handoff generation IDs so old queued payloads remain readable.
- A draft-ready agent message is labeled **Draft — not sent** and offers:
  - **Send draft**: deliver the stored text unchanged.
  - **Edit and send**: open an editable review composer, store the approved text as a new teammate-authored outbound message, and deliver it without rerunning the agent.
  - **Dismiss**: clear the handoff and add an auditable system note.
- Sending and dismissal lock and validate the handoff generation, then commit message metadata, provider correlation, delivery enqueue, and handoff removal atomically. Stable handoff-based idempotency keys prevent duplicate sends.

## Mailbox Notification

- Show durable action badges on thread rows and a prominent banner in the open thread:
  - **Needs instruction**
  - **Draft ready**
- Show actionable counts beside each readable channel and the company mailbox selector. Only `needs_instruction` and `draft_ready` contribute to counts.
- Add a PostgreSQL notification trigger for handoff changes and extend mailbox SSE events so badges and counts update across processes and connected teammates.
- Re-query current state on connect, reconnect, or lag; events remain wake-ups rather than state.
- Do not clear attention when a member merely opens the thread.
- Do not add team notification emails in this version; email/push notifications can later consume the same durable handoff state.

## Test Plan

- Verify company inheritance, both channel overrides, migration defaults, and JSON/form round-trips.
- Cover the policy matrix: team sender, outside sender, new thread, existing reply, quiet reply, passive CC, outreach reply, and mixed multi-channel routing.
- Database-test that the external message and handoff commit together and that no task is created for held targets.
- Add competing-claimant tests for duplicate inbound delivery, two simultaneous Generate draft actions, outside-reply versus team-instruction ordering, and stale task completion versus a newer handoff generation.
- Verify drafting/replying success, retry, dead-letter, and stop transitions.
- Verify exact send, edited send, dismissal, idempotent retries, outbound threading metadata, and authorization against another tenant or unreadable channel.
- Verify initial mailbox rendering, live SSE updates, reconnect reconciliation, counts, and that opening a thread does not clear its badge.
- Regenerate SQLx metadata and run formatting checks, offline compilation, migrations, the database-backed suite, and the repository stack-budget check.

## Assumptions

- "Reply" means an accepted outside message attached to an existing thread, not a brand-new conversation.
- Company policy uses live inheritance; changing it immediately affects channels without explicit overrides.
- Shared action state is authoritative across the team; there is no per-member unread state.
- Existing uncommitted outreach-allowlist work touching company and dispatch code must be preserved and integrated rather than overwritten.
