# Step 16 — Plan and Format Slack Delivery Parts

## Outcome

Turn canonical delivery intents into deterministic, bounded Slack parts while preserving thread
ordering and the distinction between a direct reply and a cross-interface mirror.

## Delivery policy

Centralize a pure matrix in the application layer:

- An inbound message is never mirrored back to its source binding.
- Its agent reply is delivered to the source binding, plus any other active bindings selected by
  channel policy.
- A mirror from email to Slack creates a Slack root when that canonical thread has no Slack external
  thread mapping. Later messages for that thread/binding wait for the root delivery and reply under
  its returned `ts`.
- A Slack-origin message may mirror to email/another Slack binding only when that destination's
  delivery policy allows it; source exclusion compares binding IDs, not transport kind.
- Context-only/quiet messages, system notes, approvals, schedules, and outreach each have an
  explicit policy row. No role/direction heuristic silently sends new categories.

Add `depends_on_delivery_id` or a per-thread/binding sequence to generic deliveries if step 9 did
not already do so. Claim SQL must skip a dependent delivery until its predecessor is delivered and
dead-letter dependents with a typed causal reason if the root can never be established.

## Renderer

Implement `src/adapters/protocols/slack/egress.rs` as a deterministic pure renderer:

- Render the agent/principal display name in message text; post as the one installed bot. Do not
  request customization scope or imply each agent is a native Slack identity.
- Escape Slack control sequences and user-supplied mrkdwn safely. Disable link/media unfurls by
  default to avoid leaking URL fetches to Slack.
- Include accessible top-level text even if blocks are introduced later.
- Keep each text part at or below 4,000 characters (Slack's recommended bound, comfortably below
  truncation). Split at paragraph/whitespace boundaries, then Unicode scalar boundaries; never
  split invalid UTF-8 or silently truncate.
- Preserve fenced code blocks across parts and prefix continuations clearly. Cap total parts and
  total canonical characters; over-limit content becomes a terminal render error or a bounded
  summary/link policy explicitly approved later—not an unbounded upload.
- Part 0 carries header/context; later parts carry continuation markers. All parts use one root
  thread: existing mapped `thread_ts`, or the `ts` returned by a newly posted part 0.
- Attach non-secret registered message metadata containing stable delivery ID, part ID/index, and
  content digest. Slack documents that metadata is visible to workspace members, so never put
  company-internal secrets, email addresses, bodies, or auth data in it.
- Freeze rendered parts in `message_delivery_parts` before the first provider call. Retries use the
  stored payload/digest rather than re-rendering changed display names or policies.

## Thread and mapping persistence

- On first-root success, one fenced transaction records part `ts`, inserts the external message
  mapping, upserts external thread mapping from the new root `ts`, and unblocks dependent delivery.
- On reply/continuation success, record one `(delivery_id, part_index, slack_ts)` mapping per part.
- A one-to-many part mapping points every Slack message to the same canonical message while
  retaining the delivery part for confirmation/reconciliation.
- Do not put one `slack_ts` column on canonical messages or deliveries.

## Tests

- Golden formatting for escaping, links, Unicode, long words, paragraphs, code fences, and exact
  4,000-character boundaries.
- Total/part limits fail before any send and never split UTF-8.
- Email-to-Slack root then reply uses the stored root; delayed reply cannot overtake root.
- Slack direct reply targets the source binding while a mirror excludes it.
- Multiple Slack bindings on one channel get independent thread roots/mappings.
- Deterministic re-render produces identical payload/digest/part keys.
- Metadata contains only allowlisted opaque IDs/digest and stays within Slack's bound.

## Acceptance criteria

- Formatting is pure, deterministic, bounded, and provider-specific.
- Thread establishment is durable and later delivery cannot race ahead of its root.
- Multi-part Slack output has a provider message key for every part.

