# 03 Principal addresses: people and agents are addressable without a desk

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed, 2026-11-09. Third of seven organization plans; depends on
[01-departments](../01-departments/README.md) and
[02-workflow-roles](../02-workflow-roles/README.md). Followed by
[04-root-unit](../04-root-unit/README.md).

## Intended behavior

Every principal in a company has an address: `slug@{company}.{application-domain}` for agents and
people alike, in the same namespace as desk addresses. A message to a principal's address is a
direct message to that principal, not traffic on a desk. An agent answers its direct messages
through a workflow binding on itself, so an agent no longer needs a desk of its own; a person reads
theirs in the product and in notifications. Desks are reserved for shared intake.

[01-departments](../01-departments/03-leads-and-coordination.md) first gives every company agent an
automatic default desk, which remains the addressing model through 02-workflow-roles. This plan
replaces automatic agent desks with direct principal addresses after a database reset. Agent
creation then writes a per-principal binding; department and other explicitly created shared
desks remain desks. Do not convert old desk rows or keep a parallel default-desk address path.

## Decisions (user, 2026-11-09)

1. **One address namespace per company.** `channel_slugs` becomes `addresses (company_id, slug,
   target_kind, target_id, is_primary)` with `target_kind` in `desk`, `principal`. A slug names
   exactly one target. Desk aliases keep working; a principal has exactly one address, its slug.
   Reserved `_` prefixes and the context suffixes (`RESERVED_SLUG_SUFFIXES`,
   `src/domain/entities/channel.rs:21`) apply unchanged.
2. **Threads have a target, not always a channel.** `threads.channel_id` becomes nullable beside
   `threads.principal_id`, with a check that exactly one is set. A direct thread's participants are
   the target principal and whoever wrote; access rules are in
   [02](02-direct-threads-and-bindings.md). Every table that keys on `(company_id, channel_id,
   thread_id)` (`thread_messages`, `thread_principals`, `external_threads`) follows the same shape.
3. **Workflow bindings have a target, not always a channel.** `workflow_bindings.channel_id`
   becomes nullable beside `workflow_bindings.principal_id`, exactly one set, or neither for
   company-wide. A binding on an agent principal runs that agent's direct-message procedure. A
   binding on a person principal is refused: people answer their own mail.
4. **The sender of a relayed message is the sending principal.** `authorize_relay` checks that the
   sender address is the running agent's own address or a desk address the run may speak for; the
   receiving side's `participant_access` sees the agent principal. This settles the open question
   in [01-departments](../01-departments/02-desks-ownership-and-access.md), batch B item 5.
5. **Access to direct threads.** A person's direct threads are readable and writable by that person
   and the company owner, nobody else. An agent's direct threads are readable by the company team
   and writable by anyone who may write to a team desk; the agent's department membership and roles
   do not change this. External senders may write to an agent only if the agent's `open_to_public`
   flag is set, the direct analogue of a public desk.
6. **The per-agent desk is gone.** Creating an agent provisions a published autonomous workflow and
   an active binding on the agent principal, in the agent's creation transaction, by the same
   provisioning code with the channel parts removed. "Create assistant channel" remains for desks
   an agent should staff. Deleting an agent with no direct-thread history removes the binding and
   workflow with it.
7. **A public desk and a public agent address are independent entry points.** A public desk admits
   external messages through its own policy and runs its bound agent even when that agent's direct
   address has `open_to_public = false`. The thread and reply stay on the desk. Opening the desk
   does not open the agent's direct address, and opening the agent does not open any desk it staffs.
   Both use the existing company mail transport and domain configuration; no per-address mailbox,
   DNS record or new credential is required under an already routed company domain. External
   access retains the existing authentication, spam and audience/egress checks.

## What does not change

- Desk addressing, routing and access (`Channel::participant_access`, `viewer_access`).
- The hop budget and relay trace; a principal target counts as a hop like a desk.
- Publication immutability; the per-agent binding is an ordinary binding on an ordinary version.

## Documents

1. [Unified addresses](01-unified-addresses.md).
2. [Direct threads and bindings](02-direct-threads-and-bindings.md).
3. [Agent default binding and relay identity](03-agent-default-binding-and-relay.md).
4. [Integration and final verification](04-integration-and-final-verification.md).
