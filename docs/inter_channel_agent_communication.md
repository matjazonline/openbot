# Inter-Channel Communication

Same-company channels can address one another without SMTP. A channel's automation is a workflow
binding (p08), so one channel asking another is a workflow run sending a message to the other
channel's address with a `message.send` or `message.reply` step. The receiving channel's own
bindings decide what, if anything, runs.

The earlier protocol, where an agent recorded an outreach naming another channel, its task
suspended, and the callee's answer resumed it, was removed with the legacy task runtime in p08,
along with its tables (`task_outreaches`, `task_outreach_targets`, `task_outreach_replies`).

## The internal relay

The email sender reaches the internal relay (`InternalMailRelay::relay_internal`,
`application/use_cases/thread/mod.rs`) before SMTP:

- **Recipients.** A recipient that resolves to a channel of the sending company is ingested
  directly as an internal-origin message (`IngressOrigin::InternalChannel`). It is never rendered
  to RFC 5322 and parsed back. Any other recipient is sent over SMTP as usual.
- **Sender identity.** The sender address must be the source channel's own address
  (`authorize_relay`), so a relayed message cannot borrow another channel's identity. The ingress
  trusts an internal origin because of this check, not because of any verdict the message
  carries.
- **Correlation.** The correlation id is carried, never re-minted: the sending run and whatever
  the receiving channel starts form one chain.
- **Refusals.** If the receiving channel refuses the message (a spent hop budget, a disabled
  channel, an access policy), the refusal is definite. The delivery records it rather than
  retrying toward the same verdict.

## On the receiving channel

A relayed message goes through the same ingress as mail:

- thread resolution and access policy;
- one inbound commit, under the channel gate and its settings-revision check;
- one admission intent per association.

From there the receiving channel's workflow bindings decide, by their message triggers, whether
it starts a run. The relay itself starts no work. An intent that answers a message a run is
waiting on is offered to that run before ordinary admission (reply correlation,
`application/workflow/message_admission.rs`).

## Loops

Every relayed message carries a trace (`EmailRelayTrace`): its source channel, its hop count and
the channels it has passed through. A reply continues the trace its source arrived with; a new
send starts one at its own channel. Ingress refuses:

- a channel that already appears in the trace, as a routing cycle;
- a hop count past `MAX_INGRESS_HOPS` (5).

Over SMTP the same trace travels as the `X-MailAgents-Channel-ID`, `X-MailAgents-Hop-Count`,
`X-MailAgents-Correlation-ID` and `X-MailAgents-Trace` headers.

So no two channels can bounce a message between themselves indefinitely.

## Invariants

- A relay never leaves the deployment and never needs a DMARC verdict.
- A relayed message is admitted only by the receiving channel's own bindings; nothing on the
  sending side resumes automatically.
- Each delivery keeps its own stable idempotency key, so a retried send is the same message.
