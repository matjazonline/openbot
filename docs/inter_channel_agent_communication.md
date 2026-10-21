# Inter-Channel Agent Communication

Same-company agent channels can delegate work to one another without using SMTP. The application
addresses the destination with a transport-neutral `ChannelSelector`, resolves it to a canonical
channel ID, and then performs internal delivery. A platform email address is one email-adapter
syntax for producing that selector; it is not the identity of the destination channel.

Internal delivery uses canonical message, thread, task, outreach, target, and delivery records.
Email metadata is an optional protocol extension only when mail actually carried the message.
Application contracts follow the [Transport Architecture Contract](transport_architecture.md) and
do not manufacture an email address merely to call another channel.

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
| Agent A conversation | `threads`, `messages`, `thread_messages` |
| Agent A wait state | `task_outreaches`, `task_outreach_targets` |
| Durable A to B request | `message_deliveries` + `message_delivery_parts` |
| Agent B conversation | A new row in `threads` |
| Agent B execution | A normal `background_tasks` row |
| Agent B external wait | `task_outreaches`, `task_outreach_targets` |
| Agent B result in Agent A context | `messages`, `thread_messages` |

The A to B relationship can be followed through the existing identifiers:

```text
A outreach target
  -> outreach delivery
  -> delivery part provider_message_key
  -> B background_tasks.source_message_id
```

## Starting a Call

Phase 06C removed the agent-runtime tools that used to start a call (`outreach_and_await_quorum`)
and list callable siblings (`list_company_agents`), together with per-agent tool grants and
`native_tool_policy`. Phase 08 rebuilds both as workflow tools. The durable machinery below them
is unchanged: an outreach request names its targets (same-company channel selectors such as
`supplier` or `acme/supplier`, or external addresses), `create_outreach_and_pause` records it and
queues the deliveries in one transaction, and the reply path resumes the caller.
`src/application/use_cases/thread/inter_channel_tests.rs` drives exactly that path against a real
database, calling `create_outreach_and_pause` with the arguments a request carries.

A single target and a 100 percent threshold represent one channel call. Platform channel email
addresses are never valid external targets; internal targets are always channel selectors.

An internal target must satisfy all of these conditions after selector parsing and canonical
channel resolution:

- It resolves to a direct channel selector (e.g. `supplier` or `acme/supplier`).
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
2. Agent A's run records an outreach with Agent B as the only target.
3. `M1` is queued in `message_deliveries`, and `tA` changes to `waiting_for_third_party_reply`.
4. The email sender recognizes B as a same-company channel and performs trusted in-process delivery instead of SMTP.
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
- The sender principal and resolved selector match the source channel; the email adapter also
  verifies its projected sending address.
- The destination is a direct agent channel.
- The hop limit has not been reached.

This prevents an external sender from forging channel headers to bypass authentication or channel authorization.

## Cycle Handling

Normal repeated-channel traces are rejected. A return from B to A is allowed only when it is a trusted internal message that exactly matches an outstanding A outreach:

```text
destination company + channel + thread
sender principal/channel == resolved outreach target channel
In-Reply-To or References contains the provider key the A-to-B delivery went out under
```

The accepted return is context-only and resumes the existing task, so it cannot start an A to B to A task loop.

## Idempotency

Outreach requests retain the existing `(task_id, outreach_key)` idempotency behavior. Every delivery derives its `Message-ID` from its own stable idempotency key, so a retried send goes out under the `Message-ID` the first attempt chose and the message recorded in the thread still matches the mail that arrives.

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
