# 01 — Make verified email an explicit identity property

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

An account's email is usable for invitation, participant, and operator authorization only after
the application has evidence that the account controls it. This phase establishes that contract;
[02](02-registration-and-address-changes.md) enforces it at account entry points and
[03](03-authorization-and-release.md) closes the consumers.

## Implementation

1. Add `users.email_verified_at` to the single init schema and carry it through `User`, persistence
   reads/writes, and the authentication-facing account projection. Reset the database when the
   changed schema cannot apply cleanly; do not backfill or infer verification from old rows.
2. Define one application-level rule for who may mark an address verified: a successfully consumed
   confirmation code for the exact address, or a validated OAuth identity with an explicit
   provider-verified email claim. Inspect the current Google userinfo and Apple identity-token
   checks; reject missing, false, or ambiguous verification claims. A signed session alone is not
   email proof.
3. Keep the verified state tied to the current address. An email change clears or withholds it until
   the new address is proved in the same transaction that installs it. State whether a user may
   change only display fields while email proof is pending.

## Focused checks and acceptance

Add `security_email_identity_` unit and database tests for the verified-state round trip, unverified
OAuth claims, and old-address proof not verifying a new address. Run
`cargo test --locked --offline --lib security_email_identity_` against a freshly migrated disposable
PostgreSQL test database; first confirm the filter selects a nonzero test count. For schema changes,
run `SQLX_OFFLINE=false cargo sqlx migrate run` and refresh/check committed `.sqlx/` metadata.
Record commands and results. This phase is complete when verified state cannot be created by a
generic profile write or a session cookie.
