# Step 3 — Add Principals, Qualified Identities, and Principal ACLs

## Outcome

Introduce a company-scoped actor model and move channel/thread participation away from email keys.
The email UI can continue editing addresses during this step, but persistence resolves those
addresses to principals and identities before it writes an ACL or thread participant.

## Additive migration

Create a new migration such as
`migrations/20260902000000_add_principals_and_identities.sql`; never edit the baseline. Add:

### `principals`

- `id`, `company_id`, `kind`, optional `user_id`, optional `agent_id`, display label, timestamps.
- Kinds: `person`, `agent`, `external`, `system` with a check constraint.
- Composite uniqueness and FKs prove a referenced user/agent belongs to the same company.
- Partial unique indexes allow at most one principal per `(company_id, user_id)` and
  `(company_id, agent_id)`.
- A shape constraint requires exactly the reference appropriate for the kind; external/system
  principals cannot smuggle a user or agent ID.

### `participant_identities`

- `id`, `company_id`, `principal_id`, `transport`, `namespace`, `subject`, `display_label`, status,
  `claim_metadata JSONB`, provenance, and timestamps.
- `UNIQUE (company_id, transport, namespace, subject)` and `UNIQUE (company_id, id)`.
- A composite FK `(company_id, principal_id)` prevents cross-tenant identity attachment.
- Store normalized email subjects as `CITEXT` only in an email extension or enforce the canonical
  lower-case form in the email writer. Do not case-fold the generic subject column in queries.
- Persisted JSON has a version/discriminator and a bounded database check; decode it fallibly.

### Principal grants and thread participants

- Add `channel_principal_grants(company_id, channel_id, principal_id, capability, provenance, ...)`.
  V1 capabilities are `participate` and `view`; create both when the legacy address allowlist meant
  both, and retain the existing owner/team rules in the domain policy.
- Add `thread_principals(company_id, channel_id, thread_id, principal_id, role, ...)` with composite
  FKs. `role` distinguishes author/participant without using an email array.
- Keep `channel_participants` and `thread_participants` only as migration sources until step 11.

## Application and persistence work

- Add `src/domain/entities/participant.rs` with `Principal`, `PrincipalKind`,
  `ParticipantIdentity`, `IdentityStatus`, and typed provenance. Give all persisted additions
  `#[serde(default)]` only where old durable payloads genuinely need a semantic default.
- Define cohesive application-owned `IdentityDirectory` and `PrincipalAccessPersistence` ports in
  the consuming use-case module. Correctness methods have no silently-successful defaults.
- Implement them in new `src/adapters/persistence/participant.rs` using tenant-scoped queries.
- Centralize `resolve_or_create_external_identity`: lock or upsert the qualified identity, reuse its
  principal, and never merge based on display name or claimed profile email.
- Update channel create/update to resolve entered email allowlist rows transactionally. A malformed
  address remains a form error; a database error propagates instead of becoming “not authorized.”
- Replace `Thread::participant_emails` with principal IDs in the canonical entity. Add a separate
  read projection that resolves display identities for email delivery and UI rendering.
- Rewrite `Channel::participant_access`, `viewer_access`, and `preferred_approver` around named
  `PrincipalAccessContext`; do not add transport conditionals to these methods.

## Backfill

In a bounded, restartable backfill or a migration proven safe for the measured row count:

1. Create person principals for company members and agent principals for company agents.
2. Create email identities for verified user emails and every observed legacy participant/sender.
3. Map channel allowlist addresses to principal grants without widening `@public` into a view grant.
4. Map thread participant addresses to `thread_principals`.
5. Report ambiguous address-to-user collisions and stop rather than choosing silently.

Record counts before/after and add queries that prove no cross-company reference and no dropped
legacy participant. If volume requires an online backfill, use a durable cursor rather than OFFSET.

## Tests

- Database rejects cross-tenant principal, identity, channel grant, and thread participant rows.
- Concurrent observation of one qualified identity creates one identity and one external principal.
- The same external subject in two namespaces does not collide.
- Profile-email claims do not merge principals or grant access.
- Existing team/public/allowlist/owner access matrices remain behaviorally equivalent after
  backfill, including the rule that `@public` never grants UI read access.
- A channel form round-trip shows its email allowlist even though storage is principal-based.

## Acceptance criteria

- No authorization or thread-participation decision is keyed by a mutable email string.
- Every identity-to-principal association is tenant-scoped and auditable.
- Email remains usable as one qualified identity, without becoming the identity model for Slack.

