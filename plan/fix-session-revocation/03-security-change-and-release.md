# 03 — Revoke sessions on account security changes

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-session-store-and-token-contract.md) and
[02](02-issuance-validation-and-revocation.md).

## Implementation

1. Revoke all of a user's sessions in the same database transaction that commits a password change,
   password setup, or email-address change. Apply this to both immediate changes and
   email-code-confirmed changes. Define whether the current browser receives a fresh session after
   the transaction or is sent to login; ensure the UI and API use the same rule.
2. Add a bounded cleanup path for expired/revoked session rows and metrics for validation failures
   and cleanup backlog without logging tokens. Document session lifetime and the new revocation
   behavior using actual configuration keys.
3. Test a stolen-cookie scenario: issue two sessions, perform a password change with one, then
   show both old cookies fail while a new login succeeds. Cover concurrent change and login, logout
   replay, OAuth-issued sessions, and a store outage.

## Final verification gate

Run `security_session_`, user/account-change, and auth route tests first. Run `scripts/check.sh`
once after the full finding is implemented, using its disposable PostgreSQL test database; this
covers formatting, offline compilation, migrations, and database-backed tests. Run affected browser
protocol checks from CI. Record commands and results and explicitly report any unavailable gate.
