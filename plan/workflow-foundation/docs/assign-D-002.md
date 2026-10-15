# Assignment D-002 — strict Clippy remedy (user decision)

Same role and settings (claude-code / claude-opus-5-5 / high).

User decided: add a crate-level `#![allow(clippy::double_must_use)]` at the crate
root(s) that need it (library and any test/bin targets covered by
`cargo clippy --locked --all-targets -- -D warnings`), with a short comment that
the lint fires on `#[async_trait]` expansions under Rust/Clippy 1.99 and should
be removed when async-trait stops triggering it. Rename the 5 deprecated
`fetch_update` calls to `try_update` (verify semantics are identical).

Do NOT yet address the `workflow_action_` load failures — S is diagnosing those
(S-002); root will send the correction separately.

Run: strict Clippy as CI defines it (SQLX_OFFLINE=true), `cargo fmt --all -- --check`,
`git diff --check`, offline all-targets check, and rerun any tests touching the
renamed `try_update` sites (stock 2 MiB, task fixture). Record in IMPLEMENTATION.md
(new section), logs under docs/logs/groupA/, and the new source manifest
(SHA256 of every changed file) in `docs/logs/groupA/source-manifest-D002.json`.
Also delete the disposable A/B artifacts you listed (target/ab-baseline,
/tmp/claude-1000/baseA, base0) — root no longer needs them; keep their logs.
Stop after; final response with results and changed files.
