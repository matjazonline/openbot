# 04 — Provider adapters and Google authorization

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [02](02-vault-storage-and-management.md); attachment to connections uses 03.
An authenticated user can connect an app through a reusable authorization lifecycle. The first
production OAuth adapter authorizes a personal Gmail or Workspace account and saves a private
ConnectedAccount. Google sign-in continues to serve Openbot identity; app consent has a separate
purpose, callback, credential store, and UI. This stage does not add another Openbot login method.

## Batch 04A — Shared connect lifecycle and provider boundary

Implement a code-owned registry of AuthProvider, AuthConfig, and AppDefinition instances. The
registry links an app's requested capabilities to supported auth methods, a server-controlled
auth config, and provider-specific scopes. Shared connect orchestration handles bounded attempts,
owner/session binding, PKCE/state utilities, cancellation, status, and sanitized outcomes. Provider
adapters own authorization URL construction, token exchange, account identity, granted-permission
interpretation, and optional refresh/revocation. Do not require every provider to offer email,
OIDC, refresh tokens, or identical consent parameters.

Add bounded integration_connect_attempts persistence inside the module. An attempt records its
opaque ID, owner namespace/subject, provider/config/app, requested capabilities, browser binding,
safe return target, expiry, reconnect account/expected authorization revision, and completion
status. Store sensitive verifier material using a distinct encrypted connect-attempt context
bound to attempt ID, owner, provider/config, and purpose; include it in rotation/inventory and
erase verifier material on consumption/expiry. Never expose it in metadata. Start with a
10-minute expiry, at most 8 active attempts and 32 retained completed/expired attempts per owner,
enforced atomically with a competing-create test. Prune expired/old attempts during bounded
owner-scoped creation/maintenance; no unowned cleanup task is needed. Reject oversized state,
safe-return targets, and metadata using the existing OAuth/HTTP limits and 01's account bounds.

The callback consumes an attempt once. Polling get_connect_attempt returns only current metadata
and status and cannot complete, reassign, or revive it. Denied, expired, cancelled, and already
consumed attempts cannot write accounts. A callback query account/owner ID is never trusted.

Static API-key/bearer account creation uses the same account lifecycle and app compatibility
validation without inventing an OAuth redirect. Test registry extension with a non-Google adapter
whose external identity is a workspace and has no email. Stage 07 expands this fixture into the
required portability proof.

## Batch 04B — Google consent through the shared lifecycle

Reuse GOOGLE_OAUTH_CLIENT_ID and GOOGLE_OAUTH_CLIENT_SECRET, which the application already reads.
Resolve them through a Google AuthConfig distinct from Openbot's login flow. Register a separate
integration callback URI derived from the configured public base URL. Proposed generic routes
are an authenticated, CSRF-protected POST to /ui/profile/vault/apps/{app_id}/connect and a callback
at /auth/integrations/{provider_id}/callback. The server resolves the Google/Gmail config from the
registry; the browser cannot choose a token endpoint, client secret, or unregistered redirect.
Do not broaden sign-in scopes or turn mailbox consent into Openbot registration/login. Additional
dedicated integration client settings are deferred until their readers and validation are implemented.

Map the initial Gmail read/send capabilities to identity scopes needed to verify the selected
account plus gmail.readonly and gmail.send. Record required versus actually granted capabilities;
do not assume that connecting the Google provider authorizes all Google apps or API paths.
Use offline access. Request consent again only when intentionally obtaining a missing refresh grant
or reconnecting; ordinary workflow runs do not prompt. Gmail draft management is a separate scope
extension, because gmail.compose also permits sending.
[Scope definitions](https://developers.google.com/workspace/gmail/api/auth/scopes).

Use a short-lived, signed, browser-bound state and PKCE verifier linked to the stored attempt,
current Openbot user, purpose, nonce, safe return destination, and intended reconnect
account/authorization revision.
Reuse existing session/OAuth primitives where they fit. Consume/clear transient browser state
on success or failure, reject replay, validate the current session, and refuse cross-purpose
or cross-user callbacks. Do not place codes, tokens, or PKCE secrets in persistent workflow data.
[Authorization-code flow](https://developers.google.com/identity/protocols/oauth2/web-server).

Define provider authorization ports in the integrations application layer. The Google adapter
uses fixed HTTPS Google endpoints, a reused bounded client, capped responses, no redirects,
redacted errors, and explicit cancellation. The OAuth client secret stays in server configuration,
separate from user accounts.

## Batch 04C — Verified account and atomic account creation

Exchange the code server-side, inspect granted scopes, and obtain account identity from Google's
authenticated identity endpoint. Verify the account subject and verified email; never trust an
email or user ID supplied as a callback query field. The connected Google account may differ from
the user's Openbot login account.

Require the requested read/send scopes before marking the account ready. Save account subject,
client identity, verified mailbox address, granted scopes, refresh token, cached access token, expiry, and token
generation through the adapter-owned typed writer, not a public generic managed-secret endpoint.
Derive expiry from the provider response with a conservative request-start clock and validate
unreasonable or missing values.

A new ready entry requires a refresh token. Preserve an existing refresh token only for the same
owner, subject, client, and intended reconnect entry; a missing field must not overwrite it.
A reconnect to another external identity must be an explicit new account and fresh binding
consent. For other providers, the adapter verifies its declared subject/workspace/installation
identity without inheriting Google's verified-email requirement.

Serialize the final callback write with the account's management/refresh lock. Reconnect must match
the authorization revision captured when consent started. A delayed callback cannot resurrect
an entry disconnected or deleted in the meantime. A plain new-connect callback that finds an
existing account reports already-connected/conflict; it must not silently overwrite that account.
Atomic uniqueness also covers callbacks arriving concurrently. Broader capabilities require new
explicit consent and a new authorization revision; existing grants/approvals are not silently
upgraded. A second app may reuse an account only when its required permissions are already granted.

## Batch 04D — Owner experience and provider policy

Show the connected address, granted capabilities, status, and attached company connections in the
owner's Connected apps/Vault page. Consent alone saves a private account; attaching it to a
company remains an explicit owner action from 03. Add reconnect/disconnect controls, with
disconnect mechanics from 05.

Handle user denial, missing scopes, Workspace-admin blocking, invalid state, code-exchange errors,
and unavailable configuration as distinct sanitized outcomes. Configuration controls which buttons
are shown, but startup does not promise that an administrator or Google has approved the app.

Personal accounts require an OAuth audience that allows them. Document the authorized callback URI,
Gmail API enablement, Workspace app-access restrictions, test-user setup, and provider approval
prerequisites. Recheck Google's current requirements before publishing operator instructions.

## Focused checks and acceptance

Add integration_connect_, google_oauth_, and google_vault_callback_ tests using scripted
identity/token endpoints and real attempt/account persistence.
Cover both account types through fixtures, different login/mailbox identities, partial consent,
missing refresh token, wrong identity on reconnect, callback replay, switched sessions, duplicate
callbacks, and disconnect/delete versus callback competitors. Verify no secrets enter page HTML,
redirect locations, error logs, or general persistence projections. Also cover unknown/disabled
apps/configs, API-key creation, workspace identity without email, attempt caps/expiry, wrong
provider callbacks, and app capability upgrades that invalidate older grants. Include encrypted
attempt context substitution, verifier erasure, and attempt-row rotation versus completion races
in integration_connect_ and vault_rotation_.

Batch checks: 04A uses integration_connect_ and integration_contract_; 04B uses google_oauth_ and
the existing login tests; 04C uses google_vault_callback_ and grant revision tests; 04D uses
integration_connect_ and affected owner route/page tests. Apply 02's SQL checks after query changes.

~~~sh
cargo test --locked --offline --lib integration_connect_ -- --list
cargo test --locked --offline --lib integration_connect_
cargo test --locked --offline --lib google_oauth_
cargo test --locked --offline --lib google_vault_callback_
cargo test --locked --offline --lib google_buttons_are_only_rendered_when_google_is_configured
cargo test --locked --offline --lib security_email_identity_application_rejects_unverified_oauth_claims
cargo check --locked --offline --all-targets
~~~

List new filters first and record nonzero counts/results. Acceptance: authorization produces one
private, correctly identified managed entry, and failed or stale callbacks cannot alter another
account or revive revoked authority. This stage does not claim working automatic renewal yet.
