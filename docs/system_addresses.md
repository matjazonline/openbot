# System Addresses

Some local parts are answered by the server itself instead of being routed to a channel. They are
prefixed with an underscore:

```text
_help@{company}.{application-domain}
```

System addresses are email-adapter routes, not business channels, principals, or channel
identities. Likewise, an ordinary platform email address is adapter syntax that resolves to a
transport-neutral channel selector and then to a canonical channel and email binding. See the
[Transport Architecture Contract](transport_architecture.md).

## Why the underscore

Both slug namespaces are constrained by the database:

```sql
-- channel_slugs_format
slug::text ~ '^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$'
-- companies_slug_format
slug::text ~ '^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$'
```

A leading underscore is therefore not a legal channel slug, alias, or company slug. Reserved names
cannot be shadowed by anything a customer creates, and the guarantee is a CHECK constraint rather
than an application-level blocklist that can drift out of sync.

Reserving a bare name like `help` would need a blocklist, and would collide with channels that
already hold that slug.

## `_help`

Replies with the channels the sender may write to, each with its description, followed by the
addressing syntax. It is answered only for a member of that company's team; anyone else is ignored.

The channel list comes from `ThreadUseCases::writable_channel_directory`, the same helper the
undeliverable-address bounce uses, so "which channels may this person write to" is decided once. It
filters on `Channel::enabled` and `Channel::participant_access(sender, membership).authorized` — the
predicate the delivery path itself applies — so the list can never name an address that would then
bounce.

## Rules

| Rule | Why |
|---|---|
| Matched against the **raw** local part | Context-suffix stripping turns a bare reserved suffix into an empty slug, so a future `_msg` would be eaten before `SystemAddress::parse` ever saw it |
| Never answered on the internal channel path | An agent has `list_company_agents`; keeping reserved addresses off that path keeps them out of the trace and the trust model |
| Answered only for the company's team | A reply would otherwise confirm that a company exists to anyone who guessed the domain |
| Skipped by channel routing, never bounced | Lets `_help` be CC'd on a real message without bouncing it; and a bounce's fuzzy suggestions could offer a stranger a real channel named `help` |
| The reply carries `Auto-Submitted: auto-replied` | `guard_ingress` refuses inbound auto-replies, so an auto-responder on the other end cannot ping-pong |

## Adding one

1. Add a variant to `SystemAddress` in `src/application/transport/ingress.rs` and its local part to
   `SystemAddress::ALL`. The name must start with `_` and must not collide with
   `RESERVED_SLUG_SUFFIXES` — `no_system_address_can_be_shadowed_by_a_channel_or_a_context_suffix`
   fails if it does.
2. Add the arm to the `match system` in `ThreadUseCases::send_system_reply`
   (`thread/ingest/routing.rs`). It is exhaustive, so a missing arm is a compile error.
3. Write the body as a pure `format!`-style function next to `format_help_email_body` in
   `thread/mod.rs`, so it unit-tests with no mocks.

Recognising which reserved name an address carries is the mail adapter's job — it owns the address
grammar — and *answering* one is the application's. The adapter classifies each recipient into an
`AddressedTarget::System { company, address }`; the ingest phase answers it.

Nothing else needs changing: delivery, the team-only rule, and the routing skip are shared.
