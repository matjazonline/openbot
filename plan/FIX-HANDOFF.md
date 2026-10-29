# Completed security fixes

The five completed security-fix plan directories were removed at the user's request:

- `fix-authentication-attempt-limits`
- `fix-request-trace-redaction`
- `fix-team-autocomplete-html-injection`
- `fix-email-identity-verification`
- `fix-session-revocation`

Their implementations, database tests and browser checks remain. Authentication admission and
attempt budgets are bounded; request traces omit credential material; HTML and autocomplete render
untrusted values as text; email-based privileges require proof of the current address; browser
sessions are persisted, revocable and validated once per protected request. Password setup/change
and email installations revoke all sessions in the same account transaction. Expired/revoked rows
have bounded supervised cleanup and observable metrics. Deploying the changed init schema requires
a database reset; old session payloads are rejected.

Operational contracts and configuration are documented in
[authentication operations](../docs/authentication-operations.md),
[request trace redaction](../docs/request-trace-redaction.md) and
[deployment guidance](../docs/deploy.md).

Before plan removal, session-revocation reviews through `SES-P03-B1-R2` passed with no open findings.
The full `scripts/check.sh` gate passed 15/15: 2,388 library tests and 5 binary tests passed, with
5 fixture generators ignored; all 2,388 library tests also passed at the stock 2 MiB stack size.
The final logging-filter correction separately passed 25 session tests, the new trace test at 2 MiB,
offline compilation, Clippy, SQLx, deployment-script, formatting and whitespace checks. Session,
HTML/team, run-count and navigation browser/protocol checks passed. Full review records and logs
were archived locally before removing the plans.

Use `RUST_TEST_THREADS=4` on this Mac and serialize database-backed checks. Graft was unavailable;
scoped repository searches were used. No development or production database was reset.
