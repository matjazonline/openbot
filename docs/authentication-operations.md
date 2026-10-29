# Authentication limits and incident response

Authentication admission runs before request-body extraction, password hashing, database-backed
attempt checks, or OAuth provider calls. These are fixed values in this build:

| Boundary | Limit | Rejection |
| --- | --- | --- |
| All classified authentication routes, per process | 600 admitted requests per 60 seconds | 429 |
| Login and OAuth, per resolved client source | 30 admitted requests per 60 seconds | 429 |
| Registration, confirmation, profile credential routes, per source | 12 admitted requests per 60 seconds per route kind | 429 |
| Concurrent authentication requests, per source across route kinds | 2; a cancelled request retains its slot until any blocking hash finishes | 429 |
| Password hashes or verifications, per process | 4 concurrent jobs | 503 |
| Authentication request body | 8 KiB, or 32 KiB for OAuth, with a 10-second read deadline | 413 or 408 |
| Password login, per normalized identifier and resolved source | 5 candidate reservations in a fixed 2-minute window | Same invalid-credentials response |
| Registration or account-change code, per operation across reissues | 5 candidate reservations in a fixed 1-hour window | Same invalid/expired-code response |

`AUTH_TRUSTED_PROXY_IPS` is the optional comma-separated list of exact TCP peer IPs trusted to
replace `X-Forwarded-For` with one client IP. The application ignores forwarding headers from
other peers. Configure the ingress proxy to overwrite any client-supplied header before adding its
own value. Without this configuration, every client behind one proxy shares a source bucket. The
limits above are constants; there is no environment key for changing them. Expired budget and
pending-code rows are cleaned in batches of at most 64 as authentication traffic continues.

## Reading the signals

The structured `authentication admission rollup` event has only `auth_route_kind`,
`accepted_count`, `limited_count`, and `window_seconds`. It emits at most five events for a
completed one-minute window, on the next authentication request to that process. The structured
`authentication attempt budget` event uses only `auth_budget_kind` (`login` or `code`) and
`auth_outcome=exhausted`. Early admission bounds these exhaustion events to at most 600 admitted
requests per process per minute. Events contain no source address, identifier, code, password,
cookie, full URI, or OAuth token. Aggregate them across application instances in the platform log
system; retain those logs for the incident-response window and restrict access to operators. A
process that stops before its next request can lose its final in-memory rollup, so use platform
request counts as the traffic baseline.

- Rising `limited_count` or exhaustion events with otherwise healthy dependencies suggests attack
  traffic or a misconfigured shared proxy source. Confirm the trusted peer and overwritten header
  contract before changing limits. A 429 occurs before password or provider work.
- Low limiting with registration failures suggests mail delivery or configuration trouble. Check
  the outbound SMTP transport and delivery errors using safe provider status fields. A send failure
  does not make a code optional. A code remains valid for 15 minutes; its guess budget lasts one
  hour across reissue and cancellation.
- Low limiting with Google or Apple failures suggests provider or network trouble. Check provider
  status, timeout and redirect configuration without copying callback `code` or `state` values into
  tickets or logs. A 5xx from the database or hash worker has a different cause from an invalid
  credential or exhausted guess budget.
- A legitimate user can retry a password login after its fixed two-minute source/identifier
  window; a successful login clears that pair's counter. A code operation can retry after its
  one-hour window with a freshly issued, unexpired code. Blocked calls do not extend either window.

Use the request ID, static matched route, status, latency and provider type to correlate failures.
Application tracing omits raw callback URIs. Review the ingress proxy's access-log format
separately; [deployment guidance](deploy.md#request-traces-and-ingress-access-logs) explains why
application redaction cannot alter an upstream log or remove a signed path token from it.

## Browser session storage

`REFRESH_TOKEN_TTL_DAYS` is the browser-session lifetime (default 30, startup-validated range
1–365 days). `JWT_SECRET` signs HS256 session tokens, and `COOKIE_SECURE` controls the cookie's
Secure flag; HttpOnly, SameSite=Lax and Path=/ are always set. Sessions have an unpredictable
UUID `jti` and a persisted `user_sessions` row. The database stores session identifiers and
account revisions, never cookies or JWTs. An expired, absent, revoked or mismatched record cannot
admit a request, even with a valid signature. Old session payloads are rejected; reset the database
when deploying this init-schema change.

Cookie issuance locks the account and compares the revision from authentication before writing the
session. A stale login cannot issue a session after a security change. All password, registration,
Google and Apple issuers persist successfully before returning a cookie. Session-store operations
have a five-second deadline; issuance transactions also have a three-second statement timeout.
A store outage returns 503 and does not clear the existing cookie. It is a dependency failure,
so retry after the database recovers rather than asking every user to sign in again.

Protected requests perform one live-session lookup at admission and pass the verified account and
session ID to extractors. A request admitted before revocation commits may finish; any validation
that starts after commit rejects the copied cookie. No in-memory session cache delays revocation.
`POST /logout` revokes only the current session before removing its cookie; other sessions for the
same account remain valid. A failed revocation returns 503 and leaves the cookie in place for retry.
Invalid sessions retain the API 401, page redirect, and HTMX `HX-Redirect: /login` behavior.

## Security changes and session cleanup

Committing a new password (including first-time password setup) or a different account email
increments a database-owned authentication revision and revokes every session in that same
transaction. This also applies when an emailed code installs the change. Requesting/cancelling a
code, updating display details, and case-only email edits leave sessions valid. A rollback leaves
both the previous credentials and sessions intact. Account-row locks fence concurrent logins: an
already-issued session is revoked by the change, and an authentication snapshot from before the
change cannot issue a session after it. A new password/OAuth login after the change can sign in.

All browsers must sign in again after a committed security change. The successful profile response
clears the current cookie and sends an HTMX navigation to `/login`, or a normal 303 redirect for a
document request. There is no automatically refreshed replacement session. Clients invoking these
HTML account endpoints directly follow the same sign-in-again rule. A pending/failed change does
not clear the cookie. Profile forms describe the sign-out before submission.

`serve` owns a session cleanup loop that runs immediately and every 60 seconds, skips missed ticks,
and is cancelled and joined during shutdown. Each sweep deletes at most 100 expired and 100 revoked
rows using the expiry/revocation indexes and `SKIP LOCKED`; it retains live sessions. Each statement
has a three-second timeout and the whole cleanup has a five-second deadline. Backlog measurement
examines at most 1001 candidates per index and reports at most 1001 distinct remaining eligible rows.
These limits are code constants, not environment settings. Sustained issuance can outrun cleanup;
alert on backlog saturation and inspect database capacity rather than increasing limits blindly.

The existing operator-only metrics endpoint and `monitoring::counter`/`monitoring::gauge` trace
events expose `session_validation_failures` with fixed reasons `invalid_token`, `inactive_session`,
and `store_unavailable`, plus `session_cleanup_removed`, `session_cleanup_failures`,
`session_cleanup_backlog`, `session_cleanup_backlog_capped` (1 means more than 1000 remain), and
`session_cleanup_last_success_unix_seconds`. Alert on cleanup failures, a stale last-success time,
or sustained saturation; a zero backlog only describes the last successful sweep. Aggregate and
retain the trace metrics across instances in the platform log system; process-local counters reset
on restart. Metrics never label tokens, cookies, session IDs or user IDs. A validation outage is a
503 dependency failure; an inactive session keeps the normal unauthenticated response.

The startup default, `fly.toml` and `.env.example` enable `monitoring::counter=info` and
`monitoring::gauge=info` in `RUST_LOG`. Keep both directives when supplying a custom filter;
`mail_agents=info` alone does not include these separate metric targets. These directives do not
enable the `monitoring::smtp` or `monitoring::ai` event targets.
