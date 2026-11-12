# Inter-Desk Communication

Same-company desks can address one another without SMTP. A desk's automation is a workflow
binding (p08), so one desk asking another is a workflow run sending a message to the other
desk's address with a `message.send` or `message.reply` step. The receiving desk's own
bindings decide what, if anything, runs.

The earlier protocol, where an agent recorded an outreach naming another desk, its task
suspended, and the callee's answer resumed it, was removed with the legacy task runtime in p08,
along with its tables (`task_outreaches`, `task_outreach_targets`, `task_outreach_replies`).

## The internal relay

The email sender reaches the internal relay (`InternalMailRelay::relay_internal`,
`application/use_cases/thread/mod.rs`) before SMTP:

- **Recipients.** A recipient that resolves to a desk of the sending company is ingested
  directly as an internal-origin message (`IngressOrigin::InternalChannel`). It is never rendered
  to RFC 5322 and parsed back. Any other recipient is sent over SMTP as usual.
- **Sender identity.** The sender address must be the source desk's own address
  (`authorize_relay`), so a relayed message cannot borrow another desk's identity. The ingress
  trusts an internal origin because of this check, not because of any verdict the message
  carries.
- **Correlation.** The correlation id is carried, never re-minted: the sending run and whatever
  the receiving desk starts form one chain.
- **Refusals.** If the receiving desk refuses the message (a spent hop budget, a disabled
  desk, an access policy), the refusal is definite. The delivery records it rather than
  retrying toward the same verdict.

Workflow admission happens after the desk accepts the message. If a workflow cannot resolve a
required department role, the message stays delivered and visible on the desk. Its admission
intent records a definite workflow refusal naming the step and role, and no run starts. The
message's workflow outcome shows that refusal; it does not cause another delivery attempt or
change the completed relay delivery into a delivery failure.

## On the receiving desk

A relayed message goes through the same ingress as mail:

- thread resolution and access policy;
- one inbound commit, under the desk gate and its settings-revision check;
- one admission intent per association.

The access-policy step does not apply the receiving desk's participant list to a relay: once
`authorize_relay` has matched the sender address to its source desk, ingress trusts
`IngressOrigin::InternalChannel`. A members-only desk, whose allowlist is derived from its
department's members, role holders and adjacent leads, therefore restricts people and outside
senders only; any desk of the same company may relay into it, still bound by the hop budget
and the loop rule below.

From there the receiving desk's workflow bindings decide, by their message triggers, whether
it starts a run. The relay itself starts no work. A `message.send` step whose destination is a
desk posts into a new thread on that desk as an outbound system note, which starts none of
its bindings: one desk cannot hand work to another this way (see "Department dispatch and
escalation" below). An intent that answers a message a run is
waiting on is offered to that run before ordinary admission (reply correlation,
`application/workflow/message_admission.rs`).

## Department dispatch and escalation

Departments ([departments](departments.md)) organize desks into a hierarchy with a lead, and a
lead's job is to hand work to its members' desks and to escalate to its parent department's desk.
What holds today:

- **The lead knows where to send.** An `agent.run` on a desk its department owns gets the
  department's name, charter, whether the agent is the lead, and — on a desk that is not public —
  the addresses of the department's own desk, its parent's, its direct children's and its agents'
  own desks, frozen with the run (coordination C003).
- **Relays are admitted.** A relay into a members-only desk is trusted like any other relay inside
  the company; members-only restricts people and outside senders (coordination C001). For people,
  a members-only desk admits its department's members and role holders, the parent department's
  lead and each direct child department's lead: one step up and one step down (coordination C002).
- **A send does not start the member's work.** A `message.send` to a member's desk is an outbound
  note on that desk; none of its bindings run. A member's reply travelling back along the trace to
  the lead's desk is refused as a cycle (see "Loops").

So dispatch that starts the member's run, and escalation that returns an answer to the lead, wait
for `channel.call` in agent-workflow-call, which the plan names as their successor (coordination
C004). Until then a lead can tell a person where work belongs, and people can forward it.

## Loops

Every relayed message carries a trace (`EmailRelayTrace`): its source desk, its hop count and
the desks it has passed through. A reply continues the trace its source arrived with; a new
send starts one at its own desk. Ingress refuses:

- a desk that already appears in the trace, as a routing cycle;
- a hop count past `MAX_INGRESS_HOPS` (5).

Over SMTP the same trace travels as the `X-MailAgents-Channel-ID`, `X-MailAgents-Hop-Count`,
`X-MailAgents-Correlation-ID` and `X-MailAgents-Trace` headers.

So no two desks can bounce a message between themselves indefinitely. It also means a reply
travelling back along a relay chain to the desk that started it is refused as a cycle, which is
one reason dispatch waits for `channel.call`.

## Invariants

- A relay never leaves the deployment and never needs a DMARC verdict.
- A relayed message is admitted only by the receiving desk's own bindings; nothing on the
  sending side resumes automatically.
- Each delivery keeps its own stable idempotency key, so a retried send is the same message.
