# Step 2 — Introduce Validated Transport Vocabulary

## Outcome

Replace loose strings and the misleading `ChannelType`/`ParticipantIdentity` pair with validated,
protocol-neutral value objects. This step changes types and pure parsing only; it does not change
persistence or runtime behavior yet.

## Domain design

Create `src/domain/entities/transport.rs` containing:

- `TransportKind` with the deliberately supported variants `Email` and `Slack`. Add future variants
  only when an adapter exists; unknown persisted values fail at the persistence boundary.
- `InstallationId`, `ChannelBindingId`, `PrincipalId`, `ParticipantIdentityId`, `ExternalEventKey`,
  `ExternalThreadKey`, `ExternalMessageKey`, and `DeliveryId` UUID/string newtypes as appropriate.
- `IdentityNamespace` and `IdentitySubject` with private fields, bounded validating constructors,
  byte-length limits, and no implicit lowercasing. Normalization belongs to the adapter because
  Slack IDs are case-sensitive while email comparison is not.
- `QualifiedIdentity { transport, namespace, subject }`. Do not expose a public constructor that
  can combine arbitrary unvalidated strings.
- `ChannelSelector`, a canonical intent such as `CurrentCompany(ChannelSlug)` or
  `Qualified { company: CompanySlug, channel: ChannelSlug }`. It identifies a business channel,
  never an SMTP mailbox or Slack conversation.
- `InboundSource { binding_id, event_key, message_key, thread_key }` and named structs for candidate
  reply keys. Avoid three-element tuples and adjacent bare strings.

Keep protocol-specific newtypes at their boundaries:

- `EmailIdentity::parse(EmailAddress)` and `EmailMessageKey::parse(MessageId)` in the email adapter.
- `SlackTeamId`, `SlackUserId`, `SlackConversationId`, and `SlackTimestamp` in the future Slack
  adapter. Constructors enforce conservative syntax and length; Slack IDs/timestamps remain opaque
  after validation rather than being parsed into numeric values.

## Refactor targets

- Replace `ChannelType` and `ParticipantIdentity` in
  `src/domain/entities/channel.rs` and `src/domain/entities/message_contract.rs`.
- Export the new module from `src/domain/entities/mod.rs` and the crate's existing entity re-export.
- Move the core of `parse_recipient_address_pipeline` out of
  `src/application/use_cases/channel.rs` into a pure `EmailChannelSelectorParser` owned by
  `src/adapters/protocols/email/`. The parser may understand `.quiet`, plus-addressing, and the app
  domain, but its result is `ChannelSelector` plus typed email-only delivery hints.
- Change `resolve_internal_target` and agent/outreach tool contexts to accept a resolved
  `ChannelSelector`/channel ID. Preserve external email recipients as an explicit
  `ExternalDestination::Email(EmailAddress)` rather than overloading the selector.
- Replace callers directly; do not add compatibility conversions between the email-shaped and
  transport-neutral types.

## Pure tests

- Email identity normalization is case-insensitive; Slack subjects retain exact case.
- Namespace/subject/event/thread/message keys reject empty, control-character, and oversized input.
- Two equal Slack user strings in different installations are different identities.
- The same Slack timestamp in two bindings is a different external message key.
- Platform email address syntax produces the expected `ChannelSelector`; an external address never
  does.
- `.quiet` and `+noagent` become typed delivery options rather than mutations of a channel slug.
- Serialization round-trips every newtype without exposing a way to deserialize an invalid value.

## Acceptance criteria

- Application and domain APIs no longer pass protocol IDs as sibling `String` arguments.
- The word `protocol` is used for behavior; `TransportKind` is used for the discriminator; business
  `Channel` is not conflated with either.
- No Slack value is represented by an `EmailAddress` or `MessageId`.
- `cargo test` proves all parsing and equality rules without network or database mocks.
