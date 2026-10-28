# 01 — Give sessions a revocable server record

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

The server can reject a previously issued cookie before its JWT expiry. This phase defines the
record and token contract; [02](02-issuance-validation-and-revocation.md) wires it into requests and
[03](03-security-change-and-release.md) proves security-change behavior.

## Implementation

1. Add a `user_sessions` table to the init schema with user ID, a random session ID or digest,
   issued/expiry timestamps, and revocation timestamp. Index active lookup and expiry cleanup.
   Store no raw JWT or cookie value. Reset the database for the new schema; do not accept old
   session payloads through a compatibility decoder.
2. Put an unpredictable `jti` in `SessionClaims`. Validate algorithm, required claims, lifetime,
   and the matching unrevoked, unexpired database row. Persist the session row before returning its
   cookie after password, Google, or Apple authentication; never return a valid cookie for a failed
   row write.
3. Define behavior when the session store is unavailable: protected requests fail closed with a
   service error, without converting a database outage into a valid session or a silent sign-out.
   Bound lookup cost and expired-row cleanup so session growth cannot make every request slow.

## Focused checks and acceptance

Add `security_session_` database round-trip tests for issued, absent, revoked, and expired rows;
assert a validly signed JWT without a row is rejected. Run migrations and `.sqlx/` checks, then
`cargo test --locked --offline --lib security_session_` against a disposable migrated database,
confirming a nonzero test count and recording results.
