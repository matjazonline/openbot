# Critical and High Architecture Remediation

## Summary

The review remains substantially actionable against the current tree.

- Completed: H-4 scheduler livelock. Schedule materialization now has leased claims, execution fencing, bounded attempts, backoff, terminal failure, and concurrency tests.
- Partially completed: C-6. Some page values are escaped, but stored XSS remains in simulation/task pages and attribute contexts.
- Still open: C-1 through C-5, the remainder of C-6, H-1 through H-3, and H-5 through H-8.
- Implement this as four ordered changesets so ingress security lands before broader worker and UI restructuring.

## Implementation Changes

### 1. Secure every ingress and tenant boundary

- Introduce a serde-compatible `AuthVerdict` enum for `Pass`, `Fail`, `SoftFail`, `Neutral`, `TempError`, `PermError`, `Unavailable`, and `Unknown`. Parse strings only at adapters; preserve compatibility with queued payloads containing legacy strings or nulls.
- Apply the selected DMARC-aligned policy: external mail is accepted only when DMARC passes. Internal channel transport retains its explicit identity-based bypass.
- Make the DNS-backed SMTP verifier authoritative. Delete all SPF/DKIM/DMARC mutation from submitted `Authentication-Results` and `Received-SPF` headers.
- Add optional SendGrid ingress controlled by `SENDGRID_INBOUND_ENABLED=false`. Enabling it requires a startup-validated `SENDGRID_WEBHOOK_PUBLIC_KEY`.
- Verify SendGrid’s ECDSA signature and timestamp against the untouched, size-limited request body before multipart parsing. Require raw MIME and `sender_ip`, then run the shared local authentication verifier rather than trusting form verdicts. Reject signatures older than `SENDGRID_WEBHOOK_MAX_AGE_SECS=300`. This follows SendGrid’s native [Inbound Parse security mechanism](https://www.twilio.com/docs/sendgrid/for-developers/parsing-email/securing-your-parse-webhooks).
- Scope schedule-thread access through an exact persisted schedule run belonging to `(user, company, schedule, thread)`. Both rendering and replying must fail before loading history when that relationship does not exist.
- Add a scoped schedule-run lookup to the application/persistence boundary; never expose an unscoped thread read to these HTTP handlers.

### 2. Harden SMTP, outbound mail, and browser delivery

- Enforce one shared inbound message limit of 20 MiB. Give the webhook a 21 MiB HTTP envelope limit and reject SMTP `MAIL FROM SIZE`, streamed DATA, and over-limit input before buffering further.
- Bound SMTP command lines to 512 bytes, DATA lines to 1,000 bytes, recipients to 100, commands to 1,000, global connections to 256, and retain the configured per-IP limit.
- Add defaults of 60 seconds per command, 300 seconds for DATA/finalization, 600 seconds per session, and 5 seconds per DNS lookup. Apply the global semaphore before spawning connection work and supervise connection tasks in a `JoinSet`.
- Return a synchronous 5xx SMTP response for definitive destination, ACL, authentication, and size rejection instead of accepting and spawning a best-effort bounce.
- Move Lettre behind an application-owned `MailTransport` port and build one shared, timeout-configured transport. Remote relays always use authenticated TLS and certificate validation.
- Allow plaintext SMTP only when `SMTP_ALLOW_PLAINTEXT_LOCAL=true`, the target resolves to loopback, and no credentials are configured. Remove the production `builder_dangerous` system-mail path.
- Audit all named HTML sinks, escape text and attribute contexts exactly once, and add hostile-value tests. In particular, fix inbound simulation bodies, task badges, names, subjects, message IDs, confirmation prompts, and configuration displays.
- Vendor pinned Tailwind/daisyUI output, HTMX, and the SSE extension as embedded local assets; remove the browser Tailwind compiler and third-party runtime scripts.
- Replace inline scripts, event attributes, and `hx-on` expressions with a vendored `app.js` using event delegation and data attributes.
- Add central headers: `script-src 'self'`, `object-src 'none'`, `base-uri 'self'`, `frame-ancestors 'none'`, restrictive form/connect directives, HSTS in secure deployments, `X-Content-Type-Options`, `Referrer-Policy`, and `Permissions-Policy`. Permit inline styles initially because the templates depend on them.
- Protect cookie-authenticated unsafe requests with strict same-origin `Origin` validation and same-origin `Referer` fallback. Exempt the signature-authenticated webhook and non-cookie bearer clients.

### 3. Make task execution bounded, fenced, and cancellable

- Add `execution_generation UUID` to `background_tasks`. A processing row must contain worker, generation, lock timestamps, and expiry; every other state must clear them.
- Represent ownership as `TaskLeaseRef { task_id, worker_id, execution_generation }`. Require it for renewal, completion, failure, payload changes, approval/outreach suspension, and committing replies.
- Replace direct stealing of expired processing tasks with an atomic reaper. Lease expiry increments `retry_count`, closes the matching attempt as failed, applies the existing exponential backoff, and transitions to pending or dead-letter at `max_retries`. Claims then select pending rows only.
- Remove the detached `tokio::spawn(task.run())`. Run the actual provider future under the lease supervisor so lease loss, shutdown, or timeout cancels the real future.
- Add an explicit execution result: `Replied`, `Suspended`, `RetryableFailure`, or `TerminalFailure`. Provider errors never become response text, thread messages, or outbox rows, and prefix matching for `"Agent execution failed:"` is removed.
- Commit the reply message, outbox row, and payload transition through one lease-fenced transactional persistence operation. If any matched agent fails, commit no replies for that logical task and retry the task.
- Replace the single inline worker with a `JoinSet` bounded by `TASK_WORKER_CONCURRENCY=4`. Claim only available capacity and order candidates in fair company rounds before global age ordering.
- Wrap each complete task execution in `AGENT_RUN_TIMEOUT_SECS=300`; timeout is retryable and consumes an attempt.
- Track the task worker, SMTP listener, mailbox listener, and their child work in `main`. On shutdown: stop new claims/accepts, cancel active work, record retryable task interruption while leases are live, await handles within the drain window, then abort only non-durable connection work.

### 4. Preserve reproducible deployment

- Pin `ai-agents` to revision `9ea972e3e3a5b777496c6d6b1b471cac4513a1e4`.
- Run the production image as a dedicated non-root user; the container listens internally on ports 3001 and 2525, so no capability is required.
- Add CI covering `cargo fmt --check`, locked offline compilation for all targets, migration application, the full database-backed test suite, and deterministic frontend asset generation.
- Regenerate and commit SQLx offline metadata after the task/schedule SQL changes. Document every new configuration key in `.env.example`, Fly deployment documentation, and test fixtures.

## Test Plan

- SMTP: forged authentication headers cannot replace verifier failures; every auth verdict and legacy serialized form is covered; DMARC missing/error/failure rejects while pass accepts.
- SendGrid: invalid, stale, missing, and body-mismatched signatures fail before parsing; valid raw MIME succeeds; missing raw MIME/sender IP and oversized bodies reject deterministically.
- Tenancy/XSS: cross-company schedule thread GET and POST return not-found; hostile text, quotes, event handlers, URLs, and script tags remain inert in every corrected sink.
- Transport/browser: remote credentials can only reach a TLS relay; plaintext configuration with credentials fails startup; CSP has no inline-script violations; cross-origin cookie mutations are rejected.
- SMTP admission: test every limit at the boundary and one unit beyond it, slow clients, missing terminators, excess recipients, IPv4/IPv6 behavior, DNS timeout, and distributed clients exhausting the global semaphore.
- Queue concurrency: two claimants cannot own one task; expired leases consume attempts and dead-letter; a stale generation cannot renew, write effects, suspend, or close a replacement execution.
- Execution: lease loss and approval suspension cancel the provider future; a transient provider failure creates no customer-visible message and a later retry can succeed; concurrency never exceeds four and a busy tenant cannot monopolize each claim batch.
- Shutdown: no new work starts after cancellation, active durable tasks become retryable or remain safely leased for reaping, and all tracked handles finish within the drain bound.
- Final verification: migration reset, SQLx prepare, offline check, full database-backed tests, frontend asset reproducibility, Docker build, and non-root runtime smoke test.

## Assumptions and Deferred Work

- SendGrid will be configured for signed Inbound Parse requests and raw MIME delivery before `SENDGRID_INBOUND_ENABLED` is enabled.
- H-4 is treated as complete and its existing regression tests remain mandatory.
- Medium/Lower findings not required by the changes above remain a follow-up backlog: approval expiry, remaining notification durability, retention, prompt-token budgeting, rate limits/quotas, session revocation/login hardening, metrics/readiness/correlation IDs, index work supported by production query plans, and broad module/port cleanup.
