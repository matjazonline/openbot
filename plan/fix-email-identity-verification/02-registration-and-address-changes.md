# 02 — Require proof at registration and address change

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-verified-email-contract.md).

## Implementation

1. Remove the public password-registration path that calls `UserUseCases::add` when email
   confirmation is unavailable. With a working outbound confirmation transport, create only a
   pending registration and mark the final user verified when its exact code is spent. Without that
   transport, reject password registration with a clear response; OAuth sign-in may remain available
   after provider email verification. Keep internal fixture creation separate from public routes.
2. Make `UserUseCases::update_profile` preserve the stored email until the new address is proved.
   Refuse an email-change request when confirmation cannot be delivered. Saving name/avatar fields
   may still succeed without moving the address. Ensure confirmation and address installation are
   atomic, including a competing claimant for the same destination address.
3. Reject unverified sessions at routes that depend on an email identity. Keep account-recovery and
   pending-confirmation UX reachable under an explicit narrow policy. Do not let API and HTMX routes
   choose different verification rules.

## Focused checks and acceptance

Extend `security_email_identity_` tests for default/local SMTP configuration, public form and JSON
registration, email changes, code expiry/replay, and two accounts racing to claim one address.
Run `cargo test --locked --offline --lib security_email_identity_` plus existing user route and
database-backed account-change tests. Use a disposable migrated database and record the command,
executed count, and result. This phase is complete when an arbitrary address cannot be installed by
registration or profile editing without proof.
