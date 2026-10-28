# 02 — Count failed attempts across application instances

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-admission-and-hashing-bounds.md).

## Implementation

1. Add bounded, expiring attempt records to the init schema for normalized login identifiers and
   for each pending registration/account-change code. Use a keyed digest for stored identifiers;
   do not store passwords or raw confirmation codes. Define retry windows, maximum failures,
   cleanup, and a response that does not reveal whether an account or code exists.
2. Reserve/consume an attempt atomically before checking a candidate code. A wrong code counts;
   a successful code is spent exactly once; an exhausted budget blocks later guesses for that
   pending operation. Reissuing a code must not make an attacker's budget unlimited. Keep the
   failure and pending-row changes in one transaction or one fenced database operation.
3. Track failed password logins by normalized identifier and source without allowing an attacker
   to permanently lock out a victim. Combine a short cooldown with the coarse admission limit
   from 01. Define the effect of a successful login and of concurrent attempts explicitly.

## Focused checks and acceptance

Add database-backed `security_auth_limits_` tests with two competing claimants for the last allowed
attempt and for the same correct code, plus expiry, reissue, and cross-instance counter behavior.
Run `SQLX_OFFLINE=false cargo sqlx migrate run`, refresh/check `.sqlx/`, then
`cargo test --locked --offline --lib security_auth_limits_` against a disposable migrated database.
Record executed count and results. This phase is complete when concurrent requests cannot exceed
the budget or spend one code twice.
