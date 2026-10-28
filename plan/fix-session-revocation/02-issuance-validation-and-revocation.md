# 02 — Validate once per request and revoke on logout

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-session-store-and-token-contract.md).

## Implementation

1. Change `require_auth` to perform the asynchronous session-store check once and put the verified
   principal/session ID in request extensions. Make `AuthenticatedUser` and `Viewer` consume that
   verified context, rather than independently believing the cookie. Preserve existing API 401 and
   page/HTMX redirect behavior for invalid sessions.
2. Issue a persisted session from every successful password and OAuth login/registration path.
   Make `/logout` revoke the current session ID before clearing its cookie. A copied cookie must
   fail on its next request, while an unrelated session remains valid.
3. Handle concurrent logout/revocation and request admission deterministically: a request admitted
   before revocation may finish, but any request whose validation begins after the revoke commits
   must fail. Avoid an unbounded in-memory session cache that would delay revocation.

## Focused checks and acceptance

Run `cargo test --locked --offline --lib security_session_` and existing auth, logout, Google, and
Apple route tests. Add database-backed tests for two sessions of one user and a revocation versus
validation race. Use a disposable migrated database; record commands and results. This phase is
complete when logout invalidates the exact copied token server-side.
