# 02 — Remove raw URI fields from application tracing

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-sensitive-route-inventory.md).

## Implementation

1. Replace `uri = %request.uri()` in `create_app` with the allowed span fields. If a matched route
   template is not available at span creation, record no route there; attach a static template at
   a safe later layer or retain request ID and method alone.
2. Audit route and use-case `#[instrument]` macros for `Path`, `Query`, form, body, and error
   values. Mark sensitive parameters skipped and add only safe, bounded fields explicitly.
   Include Google/Apple callbacks and Resend/SendGrid webhook handlers.
3. Preserve correlation between request failures and provider/worker events using request IDs or
   safe internal IDs. Ensure error logging does not print raw request URLs or credential-bearing
   provider responses.

## Focused checks and acceptance

Capture tracing output in `security_trace_redaction_` tests for OAuth callback queries and a
Resend token path. Assert request ID/method/status remain and exact code/state/token markers do not
appear. Run focused tests and existing HTTP tracing tests; record commands and results.
