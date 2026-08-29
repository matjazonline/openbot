# Outreach Target Allowlist

## Expected result

`outreach_and_await_quorum` can only send to addresses that were already known to the system before
the model ran. A prompt injection carried in a forwarded email can still ask an agent to mail the
thread to `attacker@evil.example`, and the agent can still decide to try, but the call is rejected
at the tool boundary with a policy error the model sees as a tool failure — it never reaches a
human as a plausible-looking approval prompt, and never reaches the outbox.

This converts exfiltration from "possible whenever an approver clicks through" into "not
representable", which is the only form of the guarantee that survives an approver who is busy.

## Why this and not better filtering

Every text-side defence in the pipeline — `static_pattern_check`, the LLM classifier, the untrusted
fences in `compose_prompt` — raises the cost of an injection without bounding its effect. The
effect is bounded by what the tools can reach. `outreach_and_await_quorum` is the only tool in the
process that moves conversation content to a model-chosen destination, so it is the only place
where the bound has to hold.

The reply path already has this property and is worth copying: `deliver_agent_response` sets
`recipient_to: parsed.sender` from the inbound envelope, and `outbound_cc_for` builds the Cc from
inbound headers plus the channel's participant list. No model output selects a recipient there.
The goal is to give outreach the same shape — the model picks *from* a set, it does not *name* a
destination.

## Plan summary

1. Define the allowed set for a run, assembled from data that predates the model call.
2. Carry that set into the tool through `OutreachToolContext`, not through tool arguments.
3. Enforce it in `normalize_targets`, ahead of approval and ahead of `build_target_requests`.
4. Express the policy in `tool_security.tools.outreach_and_await_quorum.config`, so channel config
   overrides it the same way every other outreach limit is overridden.
5. Decide and document the default posture, and give operators a way to widen it deliberately.
6. Record every rejection as a counter with a reason, so a channel whose legitimate work the
   allowlist breaks is visible rather than mysteriously quiet.

## The allowed set

Four sources, all resolved server-side from trusted state:

- **Thread participants.** The triggering message's sender, To, and Cc, plus the participants of
  every message already stored on the thread. This covers the overwhelmingly common case: the agent
  replies to, or loops in, someone already in the conversation.
- **Channel participants.** `channel.participant_emails`, minus `@public`, which is an ingress
  grant and must never widen egress.
- **Company directory.** Team members of the owning company.
- **Same-company agent channels.** Already handled by `AllowedTargetScope::SameCompanyChannels` and
  `resolve_internal_target`; the allowlist composes with that check rather than replacing it.

Notable non-sources: addresses appearing in the message *body*, and addresses the model produces.
Both are attacker-controlled in exactly the case this defends against. `body_mentions_email` in
`thread/support.rs` exists for a different question (whether an address was referenced) and must
not be reused here.

## Enforcement point

`normalize_targets` in `src/application/services/outreach_tool.rs`. It already parses, lowercases,
deduplicates, resolves platform addresses, and returns `Result<Vec<String>, String>` — the
allowlist check is one more arm in the same loop, and its error becomes a tool error the model can
read and react to.

Enforcing here rather than in the approval handler matters: `execute` calls `normalize_targets`
before it queues anything, and the HITL approval request is raised by the agent runtime around the
tool call. A target rejected in normalization never becomes an approval a human can accidentally
grant.

## Policy shape

Extend the existing config block rather than adding a channel column — `channel_config` is already
merged over `base_agent_config`, so this needs no migration:

```yaml
tool_security:
  tools:
    outreach_and_await_quorum:
      config:
        allowed_target_scope: external_only
        target_allowlist_mode: known_participants   # known_participants | known_plus_domains | any
        target_allowlist_domains: []                # only consulted in known_plus_domains
```

`target_allowlist_mode` parses into an enum at the boundary, like `configured_target_scope` does,
and an unrecognized value is an error rather than a silent fallback. Absent or malformed reads as
the closed value — the same fail-closed pattern as `internal_requires_approval` in
`agent_runner.rs`, which treats anything other than an explicit `false` as `true`.

`any` exists because some channels are genuinely cold-outreach channels and the operator knows it.
It should be a deliberate edit to a channel's config, visible in the channel settings UI, and
ideally rendered there as a warning rather than a checkbox among equals.

## Scope decisions to settle before implementing

- **Default for existing channels.** `known_participants` is the safe default and will break any
  channel currently doing real cold outreach. Either default to `known_participants` and accept a
  migration conversation, or default to `known_plus_domains` with an empty domain list, which is
  behaviourally identical but reads as a deliberate posture. Recommend the former.
- **Domain entries.** `@example.com` is a useful unit for a company that outreaches to one vendor,
  and a large hole for a company that lists `@gmail.com`. Consider rejecting public-mailbox domains
  outright, or at minimum surfacing them differently in the UI.
- **How thread participants reach the tool.** `OutreachToolContext` is built in
  `dispatch.rs::outreach_context_for`, which already has the `ChannelMatch` and the `ParsedEmail`;
  the stored thread participants need one persistence read. Decide whether to load them eagerly per
  dispatch or lazily inside the tool — eagerly is simpler and the thread is already loaded for
  history.
- **Case and normalization.** The allowed set must be compared with the same normalization
  `normalize_targets` applies to targets (parse as `Mailbox`, lowercase the address). Build the set
  through `EmailAddress` so the comparison rule lives on the type rather than at the call site.
- **Partial rejection.** A call naming five targets of which one is disallowed should fail the whole
  call, not silently send four. Silent narrowing would let an attacker append one legitimate target
  to make a hostile call look partly successful.

## Testing

The decision is pure once the allowed set is assembled, so it tests without mocks:

- A target on the thread is accepted; the same address with different case and display name is
  accepted.
- An address that appears only in the message body is rejected.
- A five-target call containing one disallowed address is rejected whole.
- `@public` in `participant_emails` does not widen the set.
- Absent, malformed, and non-string `target_allowlist_mode` all resolve closed.
- `any` accepts an unknown address, and the acceptance is recorded.

Plus one end-to-end test in `thread/tests.rs` shaped like the actual attack: a forwarded message
whose quoted body instructs the agent to outreach to an outside address, asserting no outbox row
and no approval row is created.

## Acceptance signals

- No path exists from model output to a recipient address that was not already in server-side state
  before the run.
- A rejected outreach produces a tool error the agent can report, a `warn!` with the channel id and
  the rejected address, and an `outreach_target_rejected_total` counter with a reason label.
- Widening a channel to `any` is a visible, auditable configuration change.
- The existing inter-channel delegation tests still pass unchanged — the allowlist composes with
  `allowed_target_scope`, it does not replace it.
