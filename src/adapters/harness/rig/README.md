# Rig adapter maintenance

Rig is the only agent runtime (06C removed ai-agents). Every model call is a workflow step:
`agent.run`, `ai.classify` and `decision.agent` use the shared durable driver in
`execution.rs`/`workflow.rs`, with structured-answer validation against the step's
`output_schema` (p08). This directory owns provider construction and the runtime bridge, not
company credentials or workflow policy.

See the [operator guide](../../../../docs/rig.md) for supported behavior, API examples and limits,
and the [fresh-database rollout](../../../../docs/deploy.md#rig-rollout-on-a-fresh-database) for
pilot and recovery procedures. The one-shot completion is `RigTextClassifier` (`classifier.rs`), used
only by the system-prompt generator and not metered.

## Pinned dependency surface

| Dependency | Exact pin / enabled features |
| --- | --- |
| `rig` facade | `=0.42.0`, defaults disabled; `agent`, `reqwest`, `rustls` |
| `rig-core`, `rig-agent` | Lockfile resolves both to `0.42.0` |
| `rmcp` | `=1.8.0`, defaults disabled; `client`, `reqwest`, `transport-streamable-http-client-reqwest` |
| `reqwest` | `=0.13.4`, defaults disabled; `json`, `stream`, `rustls`, `form`, `query`, `http2`, `charset`, `system-proxy` |

Commit `Cargo.lock` with dependency changes. Rust 1.96.0 / edition 2024 was verified during the
implementation; this is a tested toolchain, not an inferred upstream MSRV.

The provider registry explicitly chooses Gemini GenerateContent, OpenAI **Chat Completions**,
Anthropic Messages, Groq's own completion client and native xAI Responses. xAI continuation preserves
separate response-item and call IDs. No additional upstream providers/models are implicitly enabled.
The Gemini transport replaces URL-key authentication with `x-goog-api-key`. Factories accept the
company-authorized provider/model and borrowed secret; no SDK environment lookup or shared tenant
credential cache is involved. Endpoint overrides are trusted internal/test seams, not agent options.

Rig and MCP clients disable redirects, environment proxies and automatic retries even though the
shared reqwest dependency enables features needed by other adapters. Rig bodies are capped at
4 MiB. The completion boundary suppresses upstream raw-wire instrumentation in addition to turning
off content telemetry; application tracing exposes structural IDs, counts, usage and safe reasons.

Rig's automatic MCP integration is disabled; MCP calls are workflow actions dispatched through the
application, not runtime tools. An `agent.run` step declares the agent's workflow tools to the
model and dispatches each call as a workflow action; attached skills are prompt text. There is no
upstream tool registry, host-tool access or alternate delegation executor.

## Upgrade evidence

An upstream version bump alone is not compatibility evidence. Re-run the provider wire fixtures
(including tool results, reasoning/call IDs, malformed/partial usage and safe diagnostics), skill
fixtures, durable suspension/restart and competing claim tests, and structured
repair/review/delivery tests. Check request formats and telemetry after
an SDK change rather than relying only on compilation.

The [verification record](../../../../docs/rig-verification.md) is the release
baseline. CI runs formatting, patch whitespace, architecture, locked offline compilation, Clippy,
fresh migrations, SQLx metadata validation, network-isolated database-backed tests, frontend and
deployment checks, and `scripts/stack-budget.sh` at the unchanged 2,048 KiB budget. Local provider
fixtures do not authenticate a remote company's keys; controlled live pilots remain separate.

Focused local checks (no live account required):

```sh
SQLX_OFFLINE=true cargo test --locked --offline --lib adapters::harness
SQLX_OFFLINE=true cargo test --locked --offline --lib adapters::response_schema
```

For full release verification, follow CI with a disposable migrated PostgreSQL database and
`scripts/test-network-isolation.sh`; run database suites sequentially because their queue claims
share the fixture database. Keep the stock-stack gate when changing runtime stacks. Inspect
`scripts/stack-frames.sh` before increasing a bound and record what fails early instead.
