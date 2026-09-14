# Channel response trigger: Always / Mentioned / Mentioned-or-reply-to-agent

## Context
Today, whether a channel's agent runs on an inbound message is hard-wired at
`src/application/use_cases/thread/ingest/routing.rs:607`:
`answers = role == To || cc_was_mentioned(..)`. Channels need a per-channel setting so an agent
can be told to speak only when addressed. Decided with the user:

- **3 modes, default `Always`**:
  - `always`: today's rule (To answers; Cc answers only when mentioned).
  - `mentioned`: answers only when the body mentions the channel (To or Cc alike).
  - `mentioned_or_reply`: mentioned, **or** the message's *direct parent* is an agent message
    in this channel's thread.
- **Direct parent only**: replying to a teammate's message stays silent even if an agent spoke
  earlier in the chain.
- The receiving channel's setting applies to every sender, including agent-to-agent relays
  (`IngressOrigin::InternalChannel`). There is no origin-based response-trigger bypass.
- Agent-owned channels expose and persist the same setting as standalone channels. They default
  to `Always`, but can be configured to require a mention or a direct reply too.
- `[quiet]` / `+quiet` still wins (unchanged `fold_disposition`). A non-answering channel is
  already `HoldDecision::NotEligible` via `channel_answers`, so nothing changes there.
- Backward compatibility is not required: no legacy payload defaults, wire aliases, or preservation
  of old database contents. Use the fresh-schema workflow below. `Always` is the product default
  for creation, not a compatibility fallback for deserializing stored entities.

## Domain
`src/domain/entities/channel.rs`
- `enum ChannelResponseTrigger { Always (Default), Mentioned, MentionedOrReplyToAgent }`, with
  `as_str` / `FromStr` (`"always" | "mentioned" | "mentioned_or_reply"`) and serde snake_case,
  mirroring `ChannelAccessMode`. Give `MentionedOrReplyToAgent` an explicit
  `#[serde(rename = "mentioned_or_reply")]`; snake_case alone produces the wrong wire value.
- `Channel.response_trigger: ChannelResponseTrigger`, without `#[serde(default)]`.
  Current `InboundTaskPayloadV1` payloads carry identifiers, not `Channel` snapshots; no task
  payload change is needed. Keep trigger evaluation at ingress rather than re-evaluating it when
  an already-created task executes.
- A pure decision, unit-tested with a table and no mocks:
  ```rust
  pub struct AnswerFacts { pub role: RecipientRole, pub mentioned: bool, pub replies_to_agent: bool }
  impl ChannelResponseTrigger { pub fn answers(self, facts: AnswerFacts) -> bool }
  ```
  `Always => role == To || mentioned`, `Mentioned => mentioned`,
  `MentionedOrReplyToAgent => mentioned || replies_to_agent`.
  (A named struct, not adjacent bools, per AGENTS.md.)

## Routing (the behaviour change)
- Add `InboundDraft.direct_parent_message_key: Option<ExternalMessageKey>` as an ingress fact,
  separate from the ancestor candidates used for thread correlation. It is consumed during channel
  preparation, before binding/commit; it need not enter the durable task payload.
- Normalize this fact in draft producers: email ingress, `internal_inbound_message`, and
  `ingest_canonical`. Put the email parent-selection rule in one synchronous
  `EmailMessageMetadata::direct_parent_id` helper: `in_reply_to`, otherwise `references.last()`.
  Convert that one id using each producer's existing external-key conversion. Never substitute an
  older ancestor when the selected parent cannot be resolved. Update test draft constructors too.

`src/application/use_cases/thread/ingest/routing.rs`
- Rename `cc_was_mentioned` → `channel_was_mentioned` (body unchanged, doc reworded; its only
  caller is `prepare_channels`).
- New helper `replies_to_agent(candidate, target, draft) -> AppResult<bool>`:
  - `ThreadTarget::Create` → `false` (a new thread has no agent message).
  - Parent id: `draft.directives.reply_to_message_id` (in-app reply target). Otherwise resolve
    `draft.direct_parent_message_key` through
    `correlation_store.message_for_external_key(candidate.binding_id, key)`.
    Do **not** use `reply_message_keys.first()` blindly: without `In-Reply-To`, that is the root.
  - `thread_persistence.get_thread_message(thread_id, id)` (already scoped to this channel's
    thread) → `role == MessageRole::Agent`.
    Missing keys, unmapped parents, or parents absent from this thread return `false`; persistence
    errors propagate. Never search another binding or fall back to an earlier agent message.
  - Note: `ingest_canonical` (`thread/mod.rs:1098`) already sets `In-Reply-To` to the thread's
    latest message when the composer gives no explicit target, so an in-app post right after an
    agent turn counts as a reply to it. That is intended.
- New helper `candidate_answers(candidate, target, draft, body, directory) -> AppResult<bool>`,
  which keeps `prepare_channels` from growing:
  - Preserve the `Always + To` short circuit: evaluate the pure decision with the remaining facts
    false and return without mention detection or parent lookups.
  - Otherwise evaluate `mentioned`. Query `replies_to_agent` only for `MentionedOrReplyToAgent`
    with no mention, then call `trigger.answers(facts)`.
- `prepare_channels` keeps its existing signature. Line 607 becomes one call to
  `candidate_answers`; neither helper needs `origin` for this decision. Existing authentication,
  authorization, auto-reply and hop-limit rules remain separate from response-trigger eligibility.

## Persistence / schema
- `migrations/20260817000000_init_schema.sql`: add
  `response_trigger text DEFAULT 'always' NOT NULL` plus
  `CONSTRAINT channels_response_trigger_check CHECK (response_trigger = ANY (ARRAY['always','mentioned','mentioned_or_reply']))`.
  Edit the squashed init in place, then recreate both local DBs (no additive migration). This is
  the explicitly chosen fresh-schema exception to the normal immutable-migration policy; existing
  databases must be recreated, and SQLx checksums must not be rewritten to bypass validation.
- `src/adapters/persistence/channel.rs`: `ChannelDb` field + mapping (~L33/L82), the SELECT column
  list (~L144), the inserts (~L324, ~L410, ~L754), `write_channel_settings_on` UPDATE (~L581).
- `src/adapters/persistence/agent_channel.rs:83`: owned-channel insert binds the default.
  Later settings updates must retain the chosen trigger, including on agent-owned channels.
- Append new positional binds to avoid renumbering existing values; parse the stored enum fallibly.
- Run SQLx preparation after applying the schema. These runtime queries generate no metadata of
  their own, so an empty `.sqlx` diff is expected but is not evidence that their SQL is correct.

## Use case + HTTP
- `src/application/use_cases/channel.rs` `ChannelWrite.response_trigger: ChannelResponseTrigger`.
- `src/adapters/http/routes/channel.rs`:
  - Form struct (~L105): `response_trigger: Option<String>`. Absent → `Always` (same
    replace-not-patch semantics as `enabled`/`add_3rd_party` from simple mode). An unrecognised value
    → validation error. Use one form parser shared by the legacy and `/ui` handlers.
  - JSON payload (~L265): `response_trigger: Option<ChannelResponseTrigger>`, omitted = `always`.
  - Thread it into every `ChannelWrite` built (~L512, ~L781, ~L1598, ~L1678). Existing JSON
    responses serialize `Channel`, so they expose the field through that entity.
- `src/adapters/http/routes/ui_channels.rs`: `SubmittedChannel::write` must parse and pass the
  submitted trigger for both create and update. `SubmittedChannel::draft` retains the selection on
  unrelated validation failures. Easy creation explicitly uses `Always`.
- `src/adapters/http/pages/channel_settings.rs`: `ChannelDraft.response_trigger` (default
  `Always`). Add a `<select name="response_trigger">` in the advanced form next to the
  "Add CC'd outsiders" checkbox (~L712), labelled "When the agent responds", with options:
  - Always (To, or when mentioned in Cc)
  - Only when @mentioned
  - When @mentioned or replying to an agent message

  Include help text stating that the rule also applies to messages from other agents. Render the
  select for agent-owned channels as well. Populate it in `stored_channel_draft` and all submitted
  draft paths. Saving an advanced form submits the actual selection.
- No company-level default (out of scope).

## Fixtures
Every `Channel { .. }` literal and `ChannelWrite { .. }` fails to compile until it gains the
field (≈ the `add_3rd_party` sites, e.g. `pages/tests.rs::mailbox_channel`,
`thread/tests.rs::use_cases_with_directory`, `services/outreach_tool.rs::channel`). Fix test
fixtures compile-driven with `ChannelResponseTrigger::Always`; production writes must carry the
submitted setting, using `Always` only for intentional creation defaults.
`thread/tests.rs::TestChannel` gains `response_trigger`. Update `InboundDraft` fixtures with the
actual parent key or `None`.

## Tests
- `channel.rs` unit: decision table covering all 3 modes × {To, Cc} × mentioned × replies_to_agent.
- Every trigger's `as_str`, `FromStr`, JSON serialization and JSON deserialization agree; unknown
  values are rejected. Test the shared direct-parent selector with conflicting headers and with
  References alone.
- `thread/tests.rs` (next to the existing Cc-mention tests, ~L3870):
  - `mentioned`: To without a mention → accepted, no tasks, `!answers()`; To with `@support`
    → tasks; a mention only in quoted history → no tasks.
  - `mentioned_or_reply`: first a To message with `@support` (agent runs; record an agent reply
    the way existing reply tests do), then an `In-Reply-To` that agent message → tasks. An
    `In-Reply-To` pointing at a human message → no tasks. A Cc'd reply to the agent message → tasks.
    A reply with only `References` whose last entry is the agent message → tasks.
  - `always`: existing tests stay green unchanged (regression guard).
    Assert `Always + To` does not query agent mentions or parent persistence.
  - A human `In-Reply-To` with an older agent in `References`, an unknown parent, a parent mapped
    only on another binding, and an agent parent in another thread all produce no tasks without
    a mention. Test multiple receiving channels with different trigger settings independently.
  - Internal relays to standalone and agent-owned channels obey the same matrix: `mentioned`
    without a mention produces no task; a mention runs it; `Always` retains normal To behavior.
    In `mentioned_or_reply`, a relay runs only with a mention or an agent parent in the target thread.
  - `[quiet]` + reply-to-agent → no tasks.
    Cover `+quiet` and an internal relay containing a qualifying mention too.
  - In-app `ingest_canonical` with `reply_to_message_id` = agent message in `mentioned_or_reply` → tasks.
    Cover an implicit reply immediately after an agent message and after a teammate's message.
- Extend the real-database outbound-reply test in `thread/external_reply_tests.rs` to verify that
  a published agent reply's external mapping triggers `mentioned_or_reply` on a follow-up.
- Persistence round-trip: create with `mentioned`, read it, update to `mentioned_or_reply`, then
  re-read. Cover both standalone and agent-owned channels and the SQL `always` default.
- HTTP tests: save and reload a non-default trigger through `/ui/channels` and JSON; reject invalid
  values, verify omitted-field defaults, and retain a submitted selection after another field fails
  validation. Pages mark the stored option selected, including for agent-owned channels.

## Verification
1. Stop local workers and recreate the dev and test databases from all migrations, including the
   edited init schema. This resets local data; the plan update itself does not reset any database.
2. Export `DATABASE_URL` for the dev database and `TEST_DATABASE_URL` for its isolated test sibling;
   `cargo test` does not load `.env`. Run `cargo sqlx prepare -- --all-targets` against the migrated
   schema and inspect any metadata changes.
3. Mirror CI: `cargo fmt --all -- --check`, locked offline compilation for all targets,
   `cargo clippy --locked --all-targets -- -D warnings`,
   `cargo sqlx prepare --check -- --all-targets`, the database-backed suite via
   `scripts/test-network-isolation.sh cargo test --locked --offline --all-targets`, and
   `scripts/test-network-isolation.sh scripts/stack-budget.sh --offline`.
   Run the transport-boundary checks and refresh the graph with `graft build` after implementation.
4. Run the server on :3001. Set a channel to "Only when @mentioned", then send via the simulator
   and mailbox: a plain message gets no agent run; an `@slug` message runs it. Switch to
   "@mentioned or reply" and reply in the mailbox to the agent's message: it runs. Reply to a
   teammate's message: no run. Repeat on an agent-owned channel, including an agent-to-agent relay
   with and without a mention. Verify the selected setting survives saving and reopening the form.
