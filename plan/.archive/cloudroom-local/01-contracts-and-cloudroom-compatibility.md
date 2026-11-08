# 01 — Contracts and Cloudroom compatibility

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Establish the small integration boundary before adding persistence or a wire protocol. See the
[overview](README.md) for product decisions. All names here are proposed implementation contracts.

## Batch 01A — Prove the public plugin surface

Pin the inspected Cloudroom revision, plugin SDK version, supported Node version, and provider
plugin versions in an implementation evidence manifest. Git dependencies require a revision/tag
and a lockfile. Prefer an unmodified supported Cloudroom release plus our own plugin package.
If no released artifact contains the required public API, build the pinned Cloudroom source;
do not substitute an unrelated upstream BB package or import private `@cloudroom/*` modules.

The source baseline is `d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815`:

| Evidence | What it establishes |
| --- | --- |
| [Slack bot example](https://github.com/davidondrej/cloudroom/tree/d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815/gui/examples/plugins/slack-bot) | Plugin settings, secret settings, SDK thread creation/send, KV mappings, lifecycle events |
| [Connect background service](https://github.com/davidondrej/cloudroom/blob/d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815/gui/plugins/connect/src/server.ts#L125) | Abort-aware background plugin ownership |
| [Thread SDK](https://github.com/davidondrej/cloudroom/blob/d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815/gui/packages/sdk/src/areas/threads.ts) | Create, send, stop, output, timeline, interactions, and metadata surfaces |
| [ACP agents](https://github.com/davidondrej/cloudroom/blob/d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815/gui/plugins/provider-acp/src/known-agents.ts) | OpenCode's local provider ID is `acp-opencode`, with `opencode acp` |
| [Runtime defaults](https://github.com/davidondrej/cloudroom/blob/d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815/gui/packages/config/src/runtime.ts#L81) | This fork's profile is `.gui-cloudroom`, with production server port 39886 |
| [Startup listener](https://github.com/davidondrej/cloudroom/blob/d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815/gui/apps/server/src/start-server.ts#L274) | Public local-server API is unauthenticated; preserve its loopback boundary |

Some inherited BB docs name `~/.bb` and port 38886. Use the actual pinned runtime and SDK's
resolved server, not hard-coded ports/profile paths. Rust Core's HTTP API and receipts are a
different runtime; their guarantees do not establish local-server behavior.

Build a disposable public-SDK probe for:

1. Plugin installation/reload, background cancellation, secret persistence, and restart identity.
2. Provider/project/model discovery and one UI-visible native thread for `claude-code`, `codex`,
   and `acp-opencode`; capability discovery is distinct from successful authentication/inference.
3. Create/send request IDs, plugin metadata lookup, turn identity, durable event pagination and
   replay, queued/active/completed distinctions, and two clients manipulating the same thread.
4. Crash after SDK acceptance but before local mapping persistence. Determine what positive
   receipt or metadata can reconcile; do not infer idempotency from a field named `requestId`.
5. Stop semantics, including whether a specific turn can be stopped atomically. A thread-wide
   stop must not be advertised as a targeted turn cancellation.
6. Permission interaction visibility and recovery. V1 may direct the user to Cloudroom's UI to
   respond; it must surface the blocked state and never silently bypass permission policy.

Use fixtures for protocol/SDK behavior and separately label real-provider smoke evidence. A
missing public operation is a concrete gap. The default is honest uncertainty and UI-assisted
recovery, not a fork. If needed for an acceptance requirement, isolate and review the smallest
public SDK extension, pin the resulting build, and add a regression to that dependency boundary.

## Batch 01B — Application contract and fixed bounds

Place typed entities under a focused domain module and consuming ports under
`src/application/local_agents/`. Use validated newtypes for machine, installation, profile,
operation, Cloudroom project/thread/turn, credential generation, and event sequence identities.

| Concept | Responsibility |
| --- | --- |
| `LocalMachine` | Company, paired installation, enabled/revoked state, observed presence and capabilities |
| `LocalAgentProfile` | Company, machine, local project/provider/model and explicit Cloudroom execution policy |
| `LocalAgentSession` | Company, profile snapshot, installation, Cloudroom thread, authorized Openbot association |
| `LocalAgentOperation` | Stable command identity, immutable payload hash, deadline, dispatch evidence and outcome |
| `LocalAgentObservation` | Validated operation/turn evidence, bounded output and optional measured usage |

Profiles configure creation; each accepted operation freezes its effective inputs. Editing a
profile never mutates pending work or silently changes a session's provider/model. Use one concrete
Cloudroom adapter and a closed command enum: create, send, inspect, stop-thread, archive-thread,
and refresh-catalog. Do not add a general plugin registry, arbitrary shell command, filesystem
browser, raw HTTP proxy, or provider-token export.

Start with code-owned protocol bounds; enforce them at both peers and in admission/persistence:

| Boundary | Initial bound |
| --- | --- |
| Pairing | 128 bits random code entropy; 10-minute TTL; 5 outstanding codes/company |
| Connection | 1 active connection/machine; 10-second handshake; heartbeat every 15 seconds, stale after 45 |
| Wire frame / prompt | 64 KiB frame; 32 KiB UTF-8 prompt; reject before allocation/SDK dispatch |
| Result / event chunk | 256 KiB retained text/operation; 16 KiB chunk; 30-second bounded reassembly |
| Work admission | 8 unsettled effect operations and 32 queued commands/machine; reserve control capacity |
| Catalog / reads | 128 projects; 32 providers; 256 models/provider; pages of at most 100 records |
| Time | 30-minute default operation deadline; explicit maximum 24 hours |
| Plugin outbox | 64 MiB; bound unacknowledged backlog and backpressure new work before exhaustion |

Large catalogs/results use bounded pages/chunks and explicit truncation or refusal, not a larger
implicit frame. Openbot's prompt/result limits remain stricter if an affected workflow contract
requires it. An advertised provider is not a promise of login, subscription access, or tool safety.

**Focused checks:** public-SDK probe assertions; schema fixtures for unknown fields/commands,
wrong identities and all boundary limits; `cargo test --locked --offline --lib cloudroom_contract_`
and `cloudroom_bounds_` once those Rust tests exist. List selected tests first and reject a zero
match. The phase's exit artifact records supported versions and proven/uncertain SDK guarantees.
