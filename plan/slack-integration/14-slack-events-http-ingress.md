# Step 14 — Verify and Durably Acknowledge Slack Events API Requests

## Outcome

Expose a public Slack Events API endpoint that authenticates the raw request, durably deduplicates
event callbacks, and returns within Slack's three-second window without running canonical ingestion
inline.

## HTTP trust boundary

Create `src/adapters/http/routes/webhooks/slack.rs` with a route-specific body limit and no session
authentication. For every request:

1. Read at most the configured maximum bytes; reject one byte over the limit before JSON parsing.
2. Require `X-Slack-Request-Timestamp` and `X-Slack-Signature`.
3. Reject timestamps more than 300 seconds from the server clock.
4. Compute HMAC-SHA256 over exactly `v0:{timestamp}:{raw_body}` using the signing secret and compare
   in constant time. Never canonicalize JSON first.
5. Only after signature verification, minimally parse envelope type, `api_app_id`, `team_id`, and
   `event_id`; require the configured app ID. An `event_callback` also requires an active
   installation for the team; initial `url_verification` deliberately does not.
6. Mint/accept a correlation ID, hash the body, and call `InboundEventInbox::store_authenticated`
   with a strict database deadline that leaves response headroom.
7. Return 2xx for a newly stored event or duplicate `event_id`; return non-2xx when durability is
   uncertain so Slack retries.

Slack's retry headers are diagnostic only. `event_id` is the dedup key; neither retry number nor
`(binding, ts)` substitutes for delivery deduplication.

## Envelope variants

- `url_verification`: after signature/app checks, validate a small challenge and return it directly;
  it may occur before any workspace installation, is not a business event, and need not enter the
  inbox.
- `event_callback`: persist raw bytes before 2xx.
- Unknown envelope types: reject or persist-as-ignored according to an explicit closed enum; never
  deserialize into a default event callback.
- If installation lookup fails, respond with a non-secret authorization error and no row. Do not
  reveal whether another company owns the workspace.

## Time and resource bounds

- Target an internal storage deadline around two seconds, not a promise that Postgres will always
  answer inside Slack's three-second requirement.
- Apply route/IP/installation admission limits before database fan-out while allowing legitimate
  Slack retries. Overload returns a controlled non-2xx and optional `X-Slack-No-Retry: 1` only for
  permanent malformed/authenticated requests; transient capacity failures must remain retryable.
- Store only a small allowlist of safe retry facts. Do not persist raw signature/authorization
  headers.
- Measure handler duration, durable-store duration, duplicates, signature/timestamp/app/install
  rejects, body-limit rejects, and deadline failures.

## Tests

- Official signature vector, header case-insensitivity, body-byte sensitivity, wrong secret,
  malformed hex, missing headers, and timestamps at/over the replay-window boundary.
- Body at limit succeeds; limit+1 fails without JSON parse or database call.
- Database success precedes 2xx; timeout/error returns non-2xx; a retry then stores/returns 2xx.
- Concurrent duplicate event IDs produce one inbox row and both return success.
- Same Slack message timestamp under two distinct event IDs stores two delivery events (the worker
  later deduplicates the semantic message).
- `url_verification` never bypasses signature/app checks.
- Handler tests assert no message body, signing secret, signature, or bot token appears in logs.

## Acceptance criteria

- No 2xx event callback can be lost by a crash after response but before durable storage.
- The handler performs no Slack Web API, agent, attachment, or canonical-message work.
- Authentication happens on raw bounded bytes before trusted parsing.
