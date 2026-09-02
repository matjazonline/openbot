# Step 17 — Deliver Slack Parts with Fenced Recovery and Rate Limiting

## Outcome

Send frozen Slack parts through `chat.postMessage` without blind duplicates, stale-worker commits,
or one workspace consuming the worker pool. Make ambiguous outcomes explicit and reconcilable.

## Provider call protocol

For each eligible delivery/next part:

1. Claim with a fresh execution UUID and live lease.
2. Reserve a durable conversation/workspace rate slot. If no slot is available, release to its
   `not_before` time without consuming an attempt or sleeping while holding a worker.
3. Fenced-commit `request_started_at` immediately before the external call. Lease loss cancels and
   awaits the real HTTP future.
4. Load the bot token through the narrow credential store only after ownership is rechecked.
5. Call `chat.postMessage` with explicit request/response deadlines and response byte cap.
6. Classify HTTP status, Slack `ok/error`, and transport failures into delivered, retry-after,
   definite retryable, terminal, or outcome unknown.
7. Fenced-commit the result. Success stores returned conversation/`ts`, external maps, and parent
   aggregation in one transaction.

The parent delivery is the only leased object. Part status updates include the parent's execution
UUID in their SQL predicate; a part is never independently claimable by a second worker.

A reaper treats an expired part with no `request_started_at` as retryable and one with a started
request as `outcome_unknown`. It never assumes a crashed call failed before Slack accepted it.

## Rate limiting and fairness

- Create a small `transport_rate_limits` table or equivalent transactional reservation keyed by
  installation and binding/conversation. Slack documents a general limit around one message per
  second per channel plus workspace-wide limits; defaults must be conservative and configurable.
- Parse `Retry-After` on HTTP 429, validate/cap it, and advance both the affected conversation and
  workspace gates when the response indicates workspace limiting.
- Use bounded global, per-company, and per-installation concurrency. Queue claims remain global;
  scheduling prevents one tenant's ready rows from filling all active slots.
- Do not rely on in-process token buckets for correctness across multiple app instances. Local
  buckets may reduce database traffic but durable reservations are authoritative.

## Ambiguous outcome policy

There is no assumed provider idempotency key for `chat.postMessage`.

- A normal success or authenticated own-bot echo confirms the part.
- A timeout/connection loss after `request_started_at`, malformed/oversized success response, or
  lease loss during the request becomes `outcome_unknown`, not retryable.
- An unknown reconciler first waits a visibility delay, then performs bounded, rate-limited
  `conversations.history`/`conversations.replies` reads with `include_all_metadata`, matching exact
  installation, binding, part ID, and digest. History API limits are respected as a separate gate.
- If found, record delivery exactly like a normal response. If absent, retry reconciliation with
  backoff for a bounded time/count. Exhaustion becomes operator-visible dead letter; it does not
  blindly repost. A manual “retry despite duplicate risk” action requires company-manager
  confirmation and an audit reason.
- An own-bot echo arriving before/after the HTTP response is idempotent and can reconcile an
  unknown part.

## Error classification

- `429`: retry at provider deadline without consuming a normal failure attempt.
- Temporary Slack/server errors known not to have accepted a message: retryable with backoff.
- `invalid_auth`/revoked token: installation requires reauthorization; pause its deliveries.
- `not_in_channel`, archived/not-found/restricted conversation: orphan/pause binding and terminally
  classify affected delivery.
- Invalid payload/metadata/too-long: poison/terminal; renderer tests should catch it before enqueue.
- Unknown error strings fail closed as retryable only when acceptance is definitely impossible;
  otherwise classify unknown.

## Tests with a local Slack stub

- 200 success; `ok:false`; 429 with valid/malformed/huge `Retry-After`; connect failure; delayed
  headers/body; truncated/oversized JSON; and connection drop after request bytes.
- Crash before and after `request_started_at`, during provider call, after Slack success, after part
  commit, and between multipart sends.
- Two claimants, lease takeover during active request, stale completion, and shutdown cancellation.
- Workspace/channel gates coordinate across two workers/instances and preserve tenant fairness.
- Unknown outcome found by echo, found by history, not found then exhausted, and manual risky retry.
- Partial multipart success resumes at the first unfinished part and never resends confirmed parts.

## Acceptance criteria

- No code path automatically retries an ambiguous Slack post.
- A stale worker cannot write any provider outcome or continue work after ownership loss.
- Rate limiting is durable across instances and 429s do not create hot loops.
- Every provider message timestamp is tied to exactly one delivery part and canonical message.
