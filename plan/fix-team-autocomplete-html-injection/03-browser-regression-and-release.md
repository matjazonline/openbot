# 03 — Verify rendering boundaries in the browser

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-rendering-contract-and-server-alerts.md) and
[02](02-team-autocomplete-dom.md).

## Implementation

1. Add the malicious-member browser journey to the repository's existing browser protocol checks:
   create or load a company team, open participant autocomplete as a different member, type a
   matching token, and assert no injected element, event, or unintended HTTP request appears.
2. Verify HTML form errors, registration/login success alerts, and team autocomplete still display
   their intended text and operate with the production CSP enabled. Keep the CSP strict; do not add
   `unsafe-inline` or `unsafe-eval` to compensate for injected markup.
3. Document the rendering rule near shared HTML helpers and client DOM code: plain strings become
   text nodes; trusted templates are assembled separately. Audit the generated asset path so the
   checked-in script is the one the server serves.

## Final verification gate

Run `security_html_injection_`, affected page/route tests, and the new browser protocol. Run
`scripts/check.sh` once after the complete finding is implemented, with a disposable database;
it covers formatting, offline compilation, migrations, and database-backed tests. Run any affected
CI browser gates. Record commands and results, explicitly reporting unavailable checks.
