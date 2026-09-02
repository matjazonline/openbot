# Step 12 — Add Slack Configuration, Client, Installation Persistence, and OAuth

## Outcome

Allow a signed-in company manager to install the Slack app into exactly one Slack workspace, with
least-privilege bot scopes, single-use OAuth state, encrypted token persistence, and no Slack types
leaking into canonical use cases.

## Configuration and HTTP client

- Add a narrow `SlackAppConfig`, loaded all-or-none from `SLACK_CLIENT_ID`,
  `SLACK_CLIENT_SECRET`, `SLACK_SIGNING_SECRET`, `SLACK_APP_ID`, and
  `SLACK_OAUTH_REDIRECT_URL`. Keep secrets in `SecretString`; do not copy them into broad debug
  structs or spans.
- Require HTTPS redirect URLs outside explicit local test mode and require the configured app ID to
  match Events API envelopes later. Validate lengths and syntax at startup, but do not claim token
  validity until Slack is contacted.
- Extend `.env.example` and `docs/deploy.md` with only keys the application reads. Document Slack
  dashboard redirect URL and event URL setup separately from startup validation.
- Reuse one bounded `reqwest::Client`: connect/request/read timeouts, HTTPS-only production base
  URL, response-body byte cap, no automatic secret-bearing redirects, safe user agent, and no
  request/response body tracing.
- Build a small `src/adapters/protocols/slack/client.rs`; do not add an unpinned git SDK. Parse
  Slack's HTTP status and JSON `ok/error` independently and return typed errors including `429` and
  `Retry-After`.

## OAuth state storage

Add `migrations/20260902060000_add_integration_oauth_states.sql` for
`integration_oauth_states`, containing hashed random token, company/user IDs, provider, exact
redirect/scope snapshot, status, exchange execution fence, expiry, consumed time, and creation
time. State is:

- generated with a CSPRNG and at least 128 bits of entropy;
- stored only as a one-way hash and sent in the authorization URL as the opaque secret;
- bound to initiating signed-in user, managed company, exact redirect URI, requested scope set,
  expiry, and nonce;
- claimed atomically once for exchange, even when two callbacks race; and
- periodically removed in bounded batches.

This is separate from browser CSRF protection. The callback must validate state before exchanging
the code; Slack documents that a mismatched state should be treated as a forged authorization.

## Scope policy

Request only the Slack v1 features being shipped:

- `chat:write` to post as the channel-interface bot;
- `groups:read` to list/validate private conversations;
- `groups:history` to receive/read private-channel message events; and
- `users:read` only if display-name enrichment ships in step 15.

Do not request `channels:*`, DM/MPIM, `files:read`, `users:read.email`,
`chat:write.customize`, or reactions scopes. If display enrichment is deferred, omit `users:read`
too. The callback checks the required subset and stores the returned scope snapshot; unexpected
historical scopes are surfaced to a manager because Slack scope grants are additive and require
revocation/reinstall to reduce.

## Routes and use case

- Add a manager-authenticated start route in `src/adapters/http/routes/slack_oauth.rs`. The callback
  is public and derives its authority only from the single-use state; it must not require a session
  cookie to survive a provider redirect. Put company/manager decisions in a new
  `src/application/use_cases/integration.rs`, not in the handlers.
- The callback atomically moves state from `pending` to `exchanging` under a fresh execution UUID,
  calls `oauth.v2.access`, then verifies returned team/app/bot identifiers and atomically encrypts
  the bot token through `InstallationCredentialStore`, upserts the installation/audit event, and
  marks state consumed under the same fence.
- Bind the installing Slack `authed_user.id` to the initiating app user's person principal because
  the authenticated state proves who completed this explicit link. Do not link any other Slack
  identity by profile email.
- A second company attempting to install an already-owned workspace gets a non-enumerating
  conflict. Reinstall for the same company rotates credentials and records old/new scope and bot
  identity without exposing tokens.
- A definitive pre-exchange failure may release the state with bounded backoff. Once request bytes
  may have reached Slack, a network/timeout ambiguity marks the state `outcome_unknown` and asks the
  manager to restart installation; OAuth authorization codes are one-use, so never blindly retry
  an ambiguous exchange. Expired `exchanging` leases follow the same conservative rule.
- Add a reproducible Slack app manifest under `docs/` naming the exact bot scopes,
  `message.groups` event subscription, request URL, bot identity, and registered delivery metadata
  schema. Keep deployment-specific IDs/URLs as documented placeholders, never secrets.

Wire the config/client/use case in `src/infra/setup.rs`, expose only the use-case handle through
`src/adapters/http/app_state.rs`, and register public/protected routes on the correct side of
`src/adapters/http/routes/mod.rs`.

## Tests

- State is single-use, expires, is company/user/redirect/scope bound, and concurrent callbacks have
  one winner.
- Missing/invalid state makes zero Slack calls; foreign-company managers cannot start or complete
  installation.
- Token ciphertext is absent from list/detail/debug/HTML/task payloads and decrypts only through the
  narrow sender path.
- Wrong team/app identity, missing required scope, OAuth denial, 429, oversized JSON, timeout, and
  Slack `ok:false` are classified.
- Reinstall is idempotent for installation identity and auditably rotates token/scope metadata.

## Acceptance criteria

- A manager can install Slack without manually pasting a bot token.
- The persisted installation is tenant-scoped, audited, and safe to show without secret fields.
- The requested permissions exactly match private-channel text ingress/egress shipped by this plan.
