# Private and Customer-Visible Message Boundaries

## Expected result

Every message and generated artifact has an explicit visibility classification. Internal notes, delegated requests, specialist results, tool traces, and draft responses remain private. Only intentionally published messages are eligible for external dispatch. Users can immediately distinguish internal context from customer-visible conversation in the mailbox.

Visibility is enforced by the domain and dispatch path rather than being only a presentation convention. Existing context-only behavior is incorporated into the model without changing historical messages unexpectedly.

## Plan summary

1. Define visibility states such as external, internal, draft, and system, and map current message roles and `is_context_only` behavior onto them.
2. Establish transition rules, including whether an internal note can be quoted or promoted into a draft without changing the original record.
3. Persist visibility in canonical message/thread associations and backfill existing data deterministically.
4. Enforce visibility in recipient resolution, prompt construction, outbox creation, exports, and API serialization.
5. Render clearly distinct internal and external timelines or message treatments in the mailbox UI.
6. Add safe composition controls with conservative defaults and a final external-recipient preview.
7. Test against accidental leakage through replies, quoted history, attachments, summaries, pipeline context, and delegated-agent results.

## Scope decisions for the detailed plan

- Decide whether visibility applies to the canonical email message, its association with a thread, or both.
- Define attachment visibility and derived-artifact visibility.
- Define how private context may inform an answer without being reproduced verbatim.
- Specify permissions for viewing internal notes and customer-visible correspondence.
- Determine migration rules for existing context-only and internal channel messages.

## Dependencies and sequencing

Implement with or immediately after ownership semantics. Human quiet notes, provenance, review gates, and notification rules should consume this shared visibility model.

## Acceptance signals

- No internal-only message can enter an external outbox through any send path.
- Prompt construction labels private evidence distinctly from customer correspondence.
- The UI makes visibility understandable without inspecting headers or special address suffixes.
- Existing quiet/context-only workflows continue to work after migration.
- Tests cover direct replies, pipelines, outreach returns, forwarding, and attachments.

