# Critical and High Architecture Remediation

## Summary

Ingress security and SMTP admission hardening are complete. The remaining critical/high work is
the unfinished outbound/browser portion of changeset 2, followed by task execution and deployment
reproducibility.

## Implementation Changes

### 2. Finish outbound mail and browser delivery hardening

- Move Lettre behind an application-owned `MailTransport` port and build one shared, timeout-configured transport. Remote relays always use authenticated TLS and certificate validation.
- Allow plaintext SMTP only when `SMTP_ALLOW_PLAINTEXT_LOCAL=true`, the target resolves to loopback, and no credentials are configured. Remove the production `builder_dangerous` system-mail path.
- Complete the HTML sink audit, escape text and attribute contexts exactly once, and add hostile-value tests for remaining names, subjects, message IDs, confirmation prompts, configuration displays, URLs, and data attributes.
- Replace the remaining legacy inline scripts, event attributes, and `hx-on` expressions with the same-origin `app.js` event-delegation and data-attribute mechanism.

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

- XSS: hostile text, quotes, event handlers, URLs, and script tags remain inert in every remaining corrected sink.
- Transport/browser: remote credentials can only reach a TLS relay; plaintext configuration with credentials fails startup; CSP has no inline-script or inline-handler violations across both current and legacy page shells.
- Queue concurrency: two claimants cannot own one task; expired leases consume attempts and dead-letter; a stale generation cannot renew, write effects, suspend, or close a replacement execution.
- Execution: lease loss and approval suspension cancel the provider future; a transient provider failure creates no customer-visible message and a later retry can succeed; concurrency never exceeds four and a busy tenant cannot monopolize each claim batch.
- Shutdown: no new work starts after cancellation, active durable tasks become retryable or remain safely leased for reaping, and all tracked handles finish within the drain bound.
- Final verification: migration reset, SQLx prepare, offline check, full database-backed tests, frontend asset reproducibility, Docker build, and non-root runtime smoke test.

## Assumptions and Deferred Work

- Medium/Lower findings not required by the changes above remain a follow-up backlog: approval expiry, remaining notification durability, retention, prompt-token budgeting, rate limits/quotas, session revocation/login hardening, metrics/readiness/correlation IDs, index work supported by production query plans, and broad module/port cleanup.
