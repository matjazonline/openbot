# 03 — Cover every authentication route and release gate

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-admission-and-hashing-bounds.md) and [02](02-atomic-attempt-budgets.md).

## Implementation

1. Apply the same limits and uniform errors to form and JSON login, registration, registration
   confirmation, profile change confirmation, password change/setup, and OAuth endpoints that make
   provider requests. The admission boundary must run before expensive extractors or provider calls.
2. Emit bounded metrics for accepted, limited, and exhausted requests without logging passwords,
   codes, raw identifiers, or full callback URIs. Document tuning values and the trusted proxy
   source contract using the actual configuration keys implemented. Add an operator runbook for
   distinguishing attack traffic from an unavailable mail or OAuth provider.
3. Exercise a public-request burst and a legitimate user's recovery after cooldown. Verify that
   one noisy identity/source cannot exhaust the hash executor or prevent unrelated logins within
   the defined global bound.

## Final verification gate

Run focused `security_auth_limits_` and affected HTTP/user tests first. Run `scripts/check.sh`
once after the full finding is implemented, with a disposable PostgreSQL database; it includes
formatting, offline compilation, migrations, and database-backed tests. Run affected CI browser
protocol checks. Record commands, executed test counts, and results; explicitly report any
unavailable required check.
