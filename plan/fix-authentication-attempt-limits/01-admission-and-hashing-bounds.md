# 01 — Bound authentication work before it starts

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Public authentication requests cannot consume unbounded body, hashing, database, or provider
capacity. [02](02-atomic-attempt-budgets.md) adds account/code budgets and
[03](03-route-integration-and-release.md) covers every entry point.

## Implementation

1. Define limits for login, registration, confirmation, OAuth starts/callbacks, and credential
   changes, including small per-route body limits and a bounded number of concurrent Argon2 jobs.
   Run synchronous Argon2 work on a bounded blocking executor path so requests do not block Tokio
   worker threads. Return an explicit overload response before starting a hash.
2. Use one shared admission service across HTML and JSON routes. Define how a client source is
   identified behind the production proxy; do not trust arbitrary forwarded headers. Put coarse
   source/global limits before database lookups, password hashing, or provider calls. Document the
   proxy contract in deployment guidance, and make tests cover a spoofed source header.
3. For unknown login names and OAuth-only accounts, run a fixed dummy Argon2 verification under
   the same bounded path, so timing does not reveal account existence. Preserve a uniform external
   authentication error.

## Focused checks and acceptance

Add `security_auth_limits_` tests for body rejection, source spoofing, concurrent hash admission,
and equal verification paths for known/unknown accounts. Run
`cargo test --locked --offline --lib security_auth_limits_` after confirming it selects tests, plus
existing login/registration route tests. Record commands and results. A burst beyond the configured
budget must fail early without scheduling more hashing work.
