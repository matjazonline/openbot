# 03 — Gate email-based privileges and verify the whole journey

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-verified-email-contract.md) and [02](02-registration-and-address-changes.md).

## Implementation

1. Require verified current email in invite listing/acceptance, channel participant authorization,
   operator checks for monitoring/dashboard/library actions, and any other authorization keyed by
   account email. Place the rule at a shared boundary and retain tenant and object scope checks.
2. Update `.env.example`, `fly.toml`, and deployment documentation to describe the enforced public
   registration behavior. Validate only settings the application actually reads. A deployment with
   no confirmation transport must clearly expose that password registration and address changes
   are unavailable, rather than silently granting unverified identities.
3. Add end-to-end negative journeys: unverified or forged email cannot accept another recipient's
   invite, read participant-restricted mail, or obtain operator data; verified identities can. Cover
   registration, OAuth, email change, and session reuse after a changed address.

## Final verification gate

Run focused `security_email_identity_`, invitation, channel authorization, and operator tests with
a disposable migrated database. Then run `scripts/check.sh` once after the complete finding is
implemented; it covers formatting, offline compilation, migrations, SQLx metadata, and the
database-backed suite. Run any affected browser protocol checks from CI. Record every command and
result, explicitly name unavailable checks, and do not claim full verification if a required gate
could not run.
