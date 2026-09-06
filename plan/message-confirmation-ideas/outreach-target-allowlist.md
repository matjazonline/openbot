# Outreach Target Allowlist

## Status

Implemented. External outreach now defaults to addresses the server knew before the model ran.
Operators can deliberately widen an agent to company-approved domains or to unrestricted cold
outreach. The domain list is company-wide rather than channel-specific.

## Security Outcome

`outreach_and_await_quorum` cannot turn an address supplied only by model output or message-body
text into a delivery when the default `known_participants` policy is active. A forwarded prompt
injection may still cause the model to attempt the call, but the recipient is rejected before an
approval request, outreach row, or delivery row is created. The tool failure is returned to the
model so it can explain or recover from the refusal.

The exception is explicit operator policy:

- `known_participants`: allow only exact addresses present in trusted pre-run state.
- `known_plus_domains`: additionally allow any mailbox at a domain on the owning company's list.
- `any`: allow unknown external addresses for deliberate cold-outreach use cases.

Absent, null, object, numeric, and other non-string mode values fail closed to
`known_participants`. An unknown string is a configuration error rather than a fallback.

## Trusted Address Snapshot

Dispatch assembles the external-address set before invoking the model and carries it in
`OutreachToolContext`; it is never accepted as a tool argument. The set contains:

- The triggering envelope's email sender, To, and Cc identities.
- Email identities in the stored thread participant projection, covering prior messages.
- Explicit `channel.participant_emails`, excluding the special `@public` ingress grant.
- The owning company's owner and team-member email addresses.

Message bodies and model-generated addresses are deliberately excluded. Team addresses are loaded
eagerly during dispatch and cached per company for a multi-channel run. Addresses are compared in
trimmed lowercase canonical form.

Same-company agent channels remain a separate target kind. They continue through
`resolve_internal_target` and the existing `allowed_target_scope` and sub-agent checks; the
external-email allowlist neither replaces nor weakens those checks.

## Company-Wide Domains

The `companies` table stores `outreach_allowlist_domains CITEXT[] NOT NULL`, defaulting to an empty
array. The database and application bound the list to 64 entries. Application normalization:

- Accepts `example.com` or the operator-friendly `@example.com` form.
- Trims and lowercases entries.
- Deduplicates entries.
- Requires a DNS-shaped multi-label domain with valid label lengths and characters.

The migration is `migrations/20260905010000_company_outreach_allowlist_domains.sql`.

Both company settings interfaces expose the list and warn that it applies company-wide and makes
every mailbox at each domain reachable by agents configured for `known_plus_domains`. Public
mailbox providers are not automatically rejected in this version; the UI warns operators not to
add them.

## Agent Policy and Configuration

`OutreachToolPolicy` carries `target_allowlist_mode`, represented by
`OutreachTargetAllowlistMode`. The compiled base `tool_security` block declares the closed default:

```yaml
tool_security:
  tools:
    outreach_and_await_quorum:
      config:
        allowed_target_scope: external_only
        target_allowlist_mode: known_participants
```

Agent create/edit forms and agent-library JSON preserve the field. The settings UI renders `any`
as an explicitly unsafe cold-outreach option rather than an ordinary permissive checkbox.

## Enforcement and Approval Ordering

Target resolution in `src/application/services/outreach_tool.rs` parses, canonicalizes,
deduplicates, classifies, and authorizes every requested destination before target requests are
built or durable state is written. If any target is disallowed, the entire call fails; it never
silently sends to the remaining targets.

The installed agent runtime asks for HITL approval before invoking a tool. To prevent a hostile
address from becoming a plausible approval prompt while still returning a normal tool failure to
the model, the implementation uses an `OutreachApprovalGate`:

1. The approval gate parses and resolves the same call against the same immutable context and
   policy.
2. If the call is valid, the existing human-approval flow continues unchanged.
3. If the tool will reject it, the gate skips creation of a human approval and permits only that
   invocation to reach the tool boundary.
4. The tool repeats the same resolution before any write or delivery, rejects the call, and returns
   the policy error to the model.

Direct or non-HITL tool invocations are therefore protected by the tool boundary itself; the
approval gate is an additional ordering adapter, not the source of authorization.

## Observability

An allowlist rejection:

- Emits a warning containing the channel ID, rejected canonical address, and a fixed reason.
- Increments `outreach_target_rejected_total` with either `unknown_participant` or
  `unknown_participant_or_domain` as its low-cardinality reason label.

External targets admitted under `any` increment `outreach_target_accepted_total` with reason
`allowlist_any`, making deliberately widened channels observable.

## Tests and Acceptance Evidence

Coverage includes:

- Exact known-address acceptance across case and display-name variations.
- Rejection of an address appearing only in the body.
- Whole-call rejection when one of five targets is unknown.
- Company domains being consulted only under `known_plus_domains`.
- `any` accepting an unknown address and recording acceptance.
- Fixed-label rejection metrics.
- Exclusion of `@public` from the trusted snapshot.
- Closed parsing for absent and malformed modes and errors for unknown strings.
- Company domain normalization, bounds, and PostgreSQL create/update/read round-trips.
- A real-runtime forwarded-message attack test asserting that the model receives the tool failure
  and that no human approval, outreach, or outreach delivery row is created.
- Existing same-company delegation tests, unchanged in behavior.

Implementation verification completed with:

- The full database-backed library suite: 1,230 passed and 2 ignored.
- The repository's stock 2 MiB stack-budget suite.
- `cargo clippy --all-targets -- -D warnings` with SQLx offline metadata.
- Formatting, all-target compilation, migration inspection, and SQLx metadata checks.

## Operational Posture

Existing and new agents default to `known_participants`, so legitimate cold-outreach agents must be
updated deliberately. Company domains are inert until an agent selects `known_plus_domains`.
Selecting `any` is the explicit opt-out from the non-representability guarantee and should be
treated as an auditable security-policy change.
