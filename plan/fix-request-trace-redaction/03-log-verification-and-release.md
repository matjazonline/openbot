# 03 — Verify log output and deployment configuration

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-sensitive-route-inventory.md) and
[02](02-central-span-and-handler-audit.md).

## Implementation

1. Exercise a production-style tracing filter with representative success and rejection requests.
   Inspect captured application logs for an OAuth code, state, webhook token, query string, and
   ordinary request identifiers. Add a regression test that fails if a marker appears anywhere in
   the emitted span/event output.
2. Review documented proxy/access-log settings for the same URLs. Document any upstream redaction
   requirement as a deployment action, without claiming the Rust app controls proxy logging.
3. Update operations guidance to use request IDs, static route names, status, and latency for
   incident triage. Check that redaction has not removed the fields needed to investigate failures.

## Final verification gate

Run `security_trace_redaction_` and affected HTTP route tests. Run `scripts/check.sh` once after
the complete finding is implemented, with its disposable PostgreSQL test database; it covers
formatting, offline compilation, migrations, and database-backed tests. Record every command and
result, and explicitly report any unavailable required gate.
