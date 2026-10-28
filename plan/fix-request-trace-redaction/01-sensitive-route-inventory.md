# 01 — Define safe request trace fields

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Request telemetry identifies a failing route without retaining credentials from URLs. This phase
sets the policy; [02](02-central-span-and-handler-audit.md) changes instrumentation and
[03](03-log-verification-and-release.md) verifies production-style output.

## Implementation

1. Inventory route path and query values that carry OAuth codes/state, Resend endpoint tokens,
   signed links, credentials, and user-entered search terms. Include reverse-proxy access logs in
   the deployment review, since application redaction cannot control an upstream logger.
2. Define allowed fields for a central HTTP span: request ID, method, protocol version, and a
   static route template only if the router supplies one safely. Never fall back to the raw URI,
   query string, `Host`, cookies, authorization header, or body. Keep safe identifiers explicit at
   handler spans.
3. Define which operational events need provider type, status, latency, and opaque internal IDs.
   State the log-retention and access assumptions for incident response without relying on logs to
   store webhook or OAuth tokens.

## Focused checks and acceptance

Add `security_trace_redaction_` tests for the field policy and representative sensitive URLs.
Run `cargo test --locked --offline --lib security_trace_redaction_` after confirming it selects
tests. Record commands and results. This phase is complete when the policy names every known
credential-bearing route and gives no raw-URI fallback.
