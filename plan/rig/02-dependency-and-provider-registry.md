# Step 2: Pinned dependency and ProviderRegistry

Dependencies: step 1. Main files: `Cargo.toml`, `Cargo.lock`, proposed
`src/adapters/harness/rig/providers.rs` and provider-specific child modules when needed.

## Dependency proof

Compile a minimal adapter-side proof against the candidate Rig 0.42.0 release: construct a model
with explicit credentials and a controlled HTTP transport, create a dynamic native tool, execute
a bounded turn, and stop execution from a lifecycle hook. Confirm cancellation and tool call IDs.
Record the working crate names, features, Rust version, and API references in a short adapter note.

Use the published `rig` facade if this proof confirms its required surface. Pin the selected version
and commit `Cargo.lock`; keep the existing ai-agents revision. Enable only needed runtime/provider/
TLS features after inspecting that release's manifest. If a git patch is essential, pin a revision
or tag and explain it. Check transport compatibility with the application's existing reqwest version
instead of assuming clients from different major/minor versions are interchangeable.

## Registry design

The linked playbook defines its own `ProviderRegistry`: keyed factories construct provider-specific
agents. Adopt that pattern locally, with typed keys, fallible construction, and explicit resolved
credentials. Do not copy its environment loading, panic paths, or fixed model constants.
Source: [Rig dynamic model creation](https://book.rig.rs/playbook/dynamic-model-creation.html).

Use existing `ModelProvider` and `ModelName` newtypes. A proposed `ResolvedModelRequest<'a>` carries
provider, model, borrowed secret, and a trusted optional endpoint. Registry factories contain
provider construction only; prompt, tools, approvals, and per-run state are composed separately.
Return `Result` with typed unsupported-provider/configuration errors, not `Option` plus a fallback.
Reject duplicate factory registration when assembling the registry.

Prefer factories returning the pinned release's erased model handle so one runner serves every
provider. Current Rig documents `ModelHandle` as the runtime model abstraction; verify its exact
constructor in the dependency proof. A boxed provider enum confined to this adapter is acceptable
only if required by the selected release. Source:
[ModelHandle API](https://docs.rs/rig/latest/rig/agent/struct.ModelHandle.html).

## Provider mapping and credentials

| Existing logical key | Rig construction target to verify |
| --- | --- |
| `google` | Gemini client; keep the persisted `google` key |
| `openai` | Explicitly select the API matching tested tool-call semantics |
| `anthropic` | Anthropic completion client |
| `groq` | Groq completion client |
| `xai` | Native xAI client if tool fixtures pass; otherwise explicit official compatible transport |

Resolve supported models from company connections, not hardcoded demo names. Treat provider names
as a closed registered set and model names as company-authorized opaque identifiers. Do not blindly
reuse ai-agents' xAI transport workaround: its pinned backend limitation may not apply to Rig.

Reuse bounded HTTP connection pools where the selected API permits. Build lightweight credentialed
clients/model handles per run; do not globally cache tenant secrets or agents. Never mutate process
environment for a request. Concurrent tenants and rotated credentials must use their own supplied
key. Do not log the request struct or implement secret-revealing `Debug`/`Display`.

Production endpoints remain server-selected. The existing `provider_base_url` is populated by a
test-only scripted endpoint hook; retain that trust boundary. Agent JSON must not enable custom
URLs, headers, proxies, redirects carrying credentials, or arbitrary provider parameters.

## Verification and acceptance

- Registry fixtures cover all five logical keys, unknown/duplicate entries, missing credentials,
  model preservation, endpoint selection, and one actual stubbed provider request per mapping.
- Competing tenant requests to a stub receive distinct auth headers; a later request uses a rotated
  key. Secret values never appear in captured logs/errors.
- Tool-call request/response fixtures pass for every advertised provider.
- The locked dependency compiles offline with respect to SQLx metadata; registry construction
  performs no model requests and does not require every provider to have a global environment key.
