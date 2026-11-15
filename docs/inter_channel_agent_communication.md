# Communication between desks and agents

Same-company mail to a desk or a person's or agent's direct address can use the internal relay
before SMTP. Desks and principals share one address namespace. Agents have their own principal
address and default workflow binding; creating an agent creates no desk. Shared desks retain
independent workflow bindings and access settings.

## Identity and access

`InternalMailRelay::relay_internal` resolves the recipient through the company's namespace and
accepts only a sender proven by the stored delivery: the run's source desk address or the running
agent's own principal address. A run cannot borrow another agent's identity. The receiving message
carries the authenticated agent principal, and current principal grants decide admission to a
department's restricted desk. Membership and role-derived grants apply to agent senders too.
A desk-only relay retains the desk's existing internal authorization; it cannot impersonate an
agent to gain the agent's grants.

Agent direct addresses accept company senders and authenticated internal agents. Outside senders
need that agent's **Allow external messages** setting. This control is independent of a desk being
public: a private agent can staff a public desk without exposing its direct address. Direct
submission grants no external read access. Person direct threads remain private to that person and
the company owner.

The relay carries canonical content directly into ingress. It preserves correlation and proves the
exact delivery part, source target and sender principal again in the inbound commit. Source or
target revocation after observation refuses the commit without storing a message; replay cannot
bypass that check. A definite internal refusal is recorded as a delivery failure. It never falls
through to SMTP. A transient database failure remains retryable.

## Threads and workflow admission

A relayed message follows normal conversation/reply correlation, current access policy, an atomic
inbound commit and one durable admission intent per target association. Direct messages form
conversation threads, rather than one permanent thread per person or agent. The receiving
binding decides whether to start work. A role refusal leaves the delivered message visible and
records a workflow refusal without retrying delivery.

An email delivery to an internal address can therefore start the target's message-triggered
workflow. A `message.send` whose explicit destination kind is `channel` instead posts an outbound
system note on that desk; it does not start the desk's bindings. Reply correlation offers a matching
message to an already waiting run before ordinary admission. Sending a message does not by itself
wait for or resume a caller; request/response dispatch remains the separate `channel.call` work.

Department context names the department, charter, role and nearby desk addresses; agent entries use
their principal addresses. The context is frozen with the prepared run. Agent escalation notices
also use the target's exact principal binding and revision. They retain their authenticated
notification origin, so work descended from a notice cannot recursively escalate another notice.
Only the narrowly authenticated platform-notice path has its notice-specific participation
exception; ordinary relays use ordinary current authority.

## Loop protection

`EmailRelayTrace` carries a typed source target, bounded hop count and visited desk/principal
targets. Reusing a UUID across the two target kinds does not make them the same hop. Ingress
refuses a previously visited target or a count beyond `MAX_INGRESS_HOPS` (5); trace length is also
bounded. Replies continue the source trace. A return along the chain to an earlier target is a
cycle and is refused.

The email adapter encodes these values in `X-MailAgents-Source-Target`, `X-MailAgents-Hop-Count`,
`X-MailAgents-Correlation-ID` and `X-MailAgents-Trace`. Application code carries typed targets and
does not parse those wire headers. Retries retain stable delivery and provider message keys.

External SMTP and signed provider-webhook ingress resolve the same desk/principal namespace.
They still require routed company domains, authenticated ingress and outbound sender-domain
configuration; internal relay tests do not establish those deployment prerequisites. See
[Transport architecture](transport_architecture.md#principal-addresses-and-direct-threads).
