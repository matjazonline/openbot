# 01 Unified addresses

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A local part resolves to a desk or a principal through one table, and the email adapter classifies
a principal address as a deliverable target. Continues in
[02](02-direct-threads-and-bindings.md).

## Batch 01A Schema

1. Replace `channel_slugs` (`init_schema.sql:3600`) with `addresses (company_id, slug citext,
   target_kind text, target_id uuid, is_primary boolean, created_at)`,
   `PRIMARY KEY (company_id, slug)`, `addresses_target_kind_check` (`desk`, `principal`),
   `addresses_slug_format` as today, composite foreign keys to `channels (company_id, id)` and
   `principals (company_id, id)` chosen by kind through two partial foreign-key-backed check
   triggers (house style: `guard_*`). Exactly one primary per target.
2. `principals` gains `slug citext NOT NULL` with the same format check and `UNIQUE (company_id,
   slug)`; the `addresses` primary row for a principal is written with it in one transaction.
   Agent principals take the agent's slug; person principals take a slug derived from the account
   name on first membership, editable by the person in their profile; `external` and `system`
   principals have no address and the trigger refuses them.
3. The agent slug and the principal slug are the same value by construction
   (`create_agent_principal_on`); renaming an agent's slug renames its address.

## Batch 01B Resolution

1. `EmailChannelSelectorParser` (`src/adapters/protocols/email/selector.rs:62`) becomes
   `EmailAddressParser`, returning an `AddressSelector` that is a desk selector or a principal
   selector. `classify` (`src/adapters/protocols/email/ingress.rs:311`) maps a principal selector
   to a new `AddressedTarget::Principal`, beside `Channels`, `System` and `Outsider`
   (`src/application/transport/ingress.rs:429` for the recipient shape).
2. Address grouping (`src/application/use_cases/thread/address_grouping_tests.rs`) produces one
   intent per principal recipient, as it does per desk.
3. `Channel::inbound_address` and friends (`channel.rs:224`) move to a shared `address_for(slug,
   company, domain)` used by desks and principals; the `_help` reply and bounce directory
   (`writable_channel_directory`) list principals the sender may write to under a "People and
   agents" heading after desks.

## Focused checks

- `address_namespace_`: a desk slug and a principal slug cannot collide within a company and may
  across companies; an external or system principal cannot have an address; one primary per target.
- `address_resolution_`: platform addresses classify to desk, principal, system or nobody; the
  reserved suffixes still strip for desks and are refused for principals; `_help` lists both.
