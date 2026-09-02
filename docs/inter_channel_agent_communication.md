# Inter-Channel Agent Communication

Same-company agent channels can delegate work to one another without using SMTP. The application
addresses the destination with a transport-neutral `ChannelSelector`, resolves it to a canonical
channel ID, and then performs internal delivery. A platform email address is one email-adapter
syntax for producing that selector; it is not the identity of the destination channel.

The current compatibility path preserves email-style message metadata and reuses the existing
thread, task, outreach, target, and outbox records. New application contracts follow the
[Transport Architecture Contract](transport_architecture.md) and must not manufacture an email
address merely to call another channel.

No separate agent-call or parent-child task model is required.

## Use Case

Agent A receives a request, asks Agent B to acquire information, and waits. Agent B gets a new thread, contacts third parties from its own channel, and returns its result to Agent A. Agent A receives the result in its original thread and completes the original request.

```text
Human -> Agent A channel
             |
             | internal channel call
             v
         Agent B channel -> third party
             ^                 |
             |                 v
             +------------- response
             |
             | internal channel result
             v
         Agent A channel -> Human
```

## Storage Reuse

| Responsibility | Existing storage |
|---|---|
| Agent A conversation | `threads`, `email_messages`, `thread_messages` |
| Agent A wait state | `task_outreaches`, `task_outreach_targets` |
| Durable A to B request | `email_outbox` |
| Agent B conversation | A new row in `threads` |
| Agent B execution | A normal `background_tasks` row |
| Agent B external wait | `task_outreaches`, `task_outreach_targets` |
| Agent B result in Agent A context | `email_messages`, `thread_messages` |

The A to B relationship can be followed through the existing identifiers:

```text
A outreach target
  -> outreach outbox
  -> outbox provider_message_id
  -> B background_tasks.source_message_id
```

## Agent Configuration

Agent A uses the existing `outreach_and_await_quorum` tool with a same-company-only target policy.
A single target and a 100 percent threshold represent one channel call, and both are the defaults,
so the model need only supply the target selector, subject, and body.

Grant `list_company_agents` alongside it so Agent A can discover Agent B as a channel selector
instead of carrying adapter routing syntax in its prompt. During the email compatibility cutover,
the tool may also return the channel's email address as a display/delivery projection.

```yaml
tools:
  - name: list_company_agents
  - name: outreach_and_await_quorum

hitl:
  tools:
    outreach_and_await_quorum:
      require_approval: true

tool_security:
  tools:
    outreach_and_await_quorum:
      timeout_ms: 10000
      max_output_chars: 4000
      config:
        allowed_target_scope: same_company_channels
        max_targets: 1
        default_timeout_hours: 96
        max_timeout_hours: 168
        internal_requires_approval: false
```

`require_approval: true` with `internal_requires_approval: false` is the combination that lets one agent mail strangers under approval while delegating to colleagues freely. Approval is keyed by tool ID, so the tool alone cannot tell the two apart; the server resolves every recipient against the channel directory before the approval gate and lets the call through only when all of them are callable same-company channels. A single external or unresolvable recipient pulls the whole call back under approval, and a persistence failure does the same.

Agent B should normally retain the external-only policy and approval requirements for third-party email:

```yaml
tools:
  - name: outreach_and_await_quorum

hitl:
  tools:
    outreach_and_await_quorum:
      require_approval: true

tool_security:
  tools:
    outreach_and_await_quorum:
      config:
        allowed_target_scope: external_only
        max_targets: 50
        max_timeout_hours: 72
```

Supported target scopes are:

| Scope | Permitted targets |
|---|---|
| `external_only` | External email addresses only; this is the default |
| `same_company_channels` | Canonical agent channels selected within the current company only |
| `any` | External addresses and valid same-company agent channels |

| Setting | Default | Effect |
|---|---|---|
| `internal_requires_approval` | `true` | When `false`, a call whose recipients are all callable same-company channels skips human approval. Any value other than an explicit `false` means `true`. |

Use `same_company_channels` for coordinator agents that must not contact third parties directly.

## Tool Call

Agent A calls Agent B through a channel selector. This compatibility example uses the email
adapter's platform-address syntax:

```json
{
  "target_emails": ["agent-b@acme.mailagents.example"],
  "subject": "Acquire supplier capacity data",
  "body": "Contact the supplier and return available quantity and earliest delivery date."
}
```

`completion_threshold_percent` defaults to 100 and `timeout_hours` to `default_timeout_hours`, so a delegated request omits both. Spelling them out explicitly produces an identical idempotency key.

Agent A finds that channel by calling `list_company_agents` first, which returns one entry per
callable sibling channel. The compatibility projection below exposes email adapter syntax:

```json
{
  "agents": [
    {
      "address": "agent-b@acme.mailagents.example",
      "channel_name": "Supplier Desk",
      "agent_name": "VendorResearchAgent",
      "description": "Answers supplier capacity and delivery-date questions."
    }
  ],
  "count": 1
}
```

`description` comes from the agent's Description field on its settings page. The directory applies exactly the same eligibility rules as the send path, so it can never advertise a channel the tool would then refuse.

An internal target must satisfy all of these conditions after selector parsing and canonical
channel resolution:

- It resolves to a direct channel selector (the email adapter accepts
  `<channel>@<company>.<application-domain>` during the compatibility cutover).
- It belongs to the caller's company.
- It is not the caller's own channel.
- It has at least one configured agent.
- It does not use a channel pipeline or context-only suffix.

All agent channels in the same company are callable when the caller's target scope permits them. Channel participant ACLs are not applied to trusted internal delivery.

## Message Lifecycle

The complete exchange uses these logical messages:

```text
M0  Human -> A
M1  A -> B
M2  B -> third party
M3  Third party -> B
M4  B -> A
M5  A -> Human
```

1. `M0` creates Agent A thread `TA` and task `tA`.
2. Agent A calls the outreach tool with Agent B as the only target.
3. `M1` is queued in `email_outbox`, and `tA` changes to `waiting_for_third_party_reply`.
4. The outbox worker recognizes B as a same-company channel and performs trusted in-process delivery instead of SMTP.
5. `M1` is outbound in `TA`, inbound in a new Agent B thread `TB`, and creates task `tB`.
6. Agent B can use ordinary external outreach. `M2` and `M3` stay in `TB`, and only `tB` is suspended and resumed.
7. Agent B's normal response `M4` is addressed to the original sender, Agent A's channel.
8. `M4` references `M1`, matches A's outstanding outreach target, is added to `TA` as context-only, and resumes `tA`.
9. Agent A reruns with `M0`, `M1`, and `M4` in its thread history, then sends `M5` to the human.

Agent B's result never creates a second Agent A task. It resumes the task that originally created `M1`.

## Internal Transport

Same-company channel messages do not open an SMTP connection. The server prepares the same metadata used for email delivery:

```text
Message-ID
In-Reply-To
References
From
To
X-MailAgents-Channel-ID
X-MailAgents-Hop-Count
X-MailAgents-Trace
```

It then calls the trusted internal ingress path directly. The canonical message can be associated with an outbound thread message in the source channel and an inbound thread message in the destination channel.

External recipients continue to use SMTP.

## Trust Boundary

Only server-generated internal delivery is trusted. Public SMTP and webhook input is always treated as external, even when it contains `X-MailAgents-*` headers or uses a platform-looking `From` address.

The internal ingress path validates:

- Source channel ID exists.
- Source channel and destination channel belong to the same company.
- The sender principal and resolved selector match the source channel (the compatibility path also
  verifies its projected email address).
- The destination is a direct agent channel.
- The hop limit has not been reached.

This prevents an external sender from forging channel headers to bypass authentication or channel authorization.

## Cycle Handling

Normal repeated-channel traces are rejected. A return from B to A is allowed only when it is a trusted internal message that exactly matches an outstanding A outreach:

```text
destination company + channel + thread
sender principal/channel == resolved outreach target channel
In-Reply-To or References contains the A-to-B outbox Message-ID
```

The accepted return is context-only and resumes the existing task, so it cannot start an A to B to A task loop.

## Idempotency

Outreach requests retain the existing `(task_id, outreach_key)` idempotency behavior. Internal outbox messages use deterministic message IDs based on the outbox ID. Normal task responses use deterministic message IDs based on the task ID.

Outbound messages created by outreach are excluded from the worker's final-response idempotency check. Without this exclusion, the A-to-B request could be mistaken for A's final human response, or B's third-party outreach could be mistaken for B's final result.

## Timeouts

Agent A's wait should outlast Agent B's third-party wait:

```text
Agent B external timeout: 72 hours
Agent A channel-call timeout: 96 hours
```

If Agent A times out first, the existing timeout approval flow can proceed with partial context, extend the wait, or stop the task.

## Operational Invariants

- A to B creates a new B thread and a normal B task.
- B to A does not create a new A task.
- B to A resumes A's original task and adds context to A's original thread.
- Agent A's final response remains a reply to the original human message.
- Same-company channel traffic bypasses SMTP.
- External traffic continues through SMTP.
- Public message headers never establish internal trust.
