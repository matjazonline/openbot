# Rig verification and CI evidence

This record describes the Rig release verification performed on 2026-09-10. Required model-backed
tests use synthetic credentials, explicit local scenarios and the real adapters. Live provider
smoke checks are optional and were not run for this verification.

## Scenario ownership and protocols

`application/services/test_support/scenario.rs` owns its listener and connections, bounds the
exchange queue, request/response/header bytes, exchange waits and total lifetime, and cancels and
awaits on `finish()`. Cancelling `finish()` or dropping the fixture still aborts the worker.
`finish()` rejects unused, mismatched and extra requests; it must be awaited after execution settles.
A response barrier signals actual request arrival and requires explicit release. Fixture failure
is independent of the error a runtime receives, so catching a provider error cannot pass a broken
scenario. Agent registrations and allowed model origins are removed with the listener.

`test_support/provider.rs` encodes each factory's real protocol, authentication, models, tool
schemas, assistant calls and correlated results. Gemini fixtures carry a thought signature through
continuation. xAI retains distinct output item and call IDs in saved state; its native input
serializer sends the call ID. The short `LlmTurn` API remains available over this same server.

The actual Rig HTTP boundary and ai-agents execution reject model destinations without an active
fixture registration before connecting. Classifier execution requires an explicit deterministic
`TextClassifier` test double. OS isolation covers all clients, including MCP, web fetch and delivery.

## Release matrix

| Area | Required evidence |
| --- | --- |
| Shared dispatch | `agent_runner/mod_tests.rs`: both registered harnesses, both configured defaults, real todo invocation, exact assistant/result correlation and conditional final answer |
| Provider protocol | `rig/provider_tests.rs`, `protocol_tests.rs`, `batch_tests.rs`: five factories, tenant/credential isolation, grants, usage, call IDs, ordered batch results, durable continuation |
| Provider failures | `rig/provider_failure_tests.rs`: every provider rejects malformed bodies, rate limits and disconnects without undeclared retry; barrier-controlled cancellation; missing registration; missing/partial usage |
| Fixture lifecycle | `test_support/scenario{,_tests}.rs`: unused/mismatched/extra requests, explicit retries, framing bounds, auth-safe diagnostics, blocked shutdown, drop and registration cleanup |
| Default/bootstrap | `infra/config_harness_tests.rs`: clean child environments with the property absent and explicitly `ai_agents`, actual AppConfig and deployment registry; no global environment mutation |
| Structured answers | `rig/direct_tests.rs`, `thread/structured_response_tests.rs`, `simulation_tests.rs`, `task/harness_runs_tests.rs`: all provider repair positions, exhaustion, budgets, no invalid publication, scheduled/direct/simulation/review, persisted reservation/candidate/final recovery |
| Durable tool effects | `thread/rig_execution_tests.rs`, `task/harness_runs_tests.rs`, `mcp_durable_tests.rs`: production dispatch, approval/restart, persisted call results, no duplicate committed effects, competing writers and stale ownership |
| Configuration/product | Existing agent harness, agent/library API, form, runtime settings and simulation suites for configuration and product behavior |

The architecture gate also exposed an existing fabricated email identity in private-note task
creation. That event now uses `MessageAuthorWrite::Platform`; its database test requires one
system-authored message with no transport identity, including after an idempotent retry.

The new wire-level accounting test exposed required fields in the pinned SDK's partial usage
parsers. `rig/usage.rs` supplies zero for absent counters only. The existing capture layer treats
zero as unreported, charges the reservation estimate and records mixed/estimated provenance. It
preserves supplied counters and leaves malformed non-null values to the provider decoder.

## Gates and local verification

CI fetches dependencies before isolated tests, checks formatting and patch whitespace, checks
architecture and its regression fixtures, compiles offline, denies Clippy warnings, migrates
PostgreSQL, checks SQLx metadata, and runs the full suite plus the stock-stack suite. Existing
frontend and deployment gates remain required. `scripts/test-network-isolation.sh` denies external
IPv4/IPv6 egress, including DNS, while allowing local fixtures and CI's loopback-published database.
It self-tests with documentation-only external addresses and an actual local socket exchange.
Linux uses scoped UID firewall chains removed on exit; macOS uses an inherited process sandbox.
Unsupported operating systems fail rather than run unguarded.

Verification on 2026-09-10, arm64/macOS with a fresh isolated PostgreSQL 16 database:

- Fresh migrations, SQLx prepare and prepare --check; no `.sqlx` metadata changes were needed.
- Locked offline all-target compilation, formatting, whitespace and Clippy with warnings denied.
- Network-isolated suite: 1,425 library tests and 5 binary tests passed; four optional/developer
  tests ignored. The two live DNS tests are explicitly optional rather than required CI inputs.
- Full library suite passed again at the unchanged 2,048 KiB stack budget, including Rig dispatch,
  approval recovery and scheduled structured completion.
- Architecture gate and its deliberate runtime-import regression fixtures passed.
- Eight frontend behavior tests and deployment/key-rotation script tests passed. Generated CSS
  rebuilt byte-identically; `check:css` itself reports the already-uncommitted stylesheet diff
  against HEAD until the pending changes are committed.
- Network isolation self-check passed on macOS. The Linux firewall branch is wired into GitHub
  Actions and requires execution on that runner; no remote CI run is claimed here.

`scripts/stack-frames.sh` on the debug library test binary measured TaskWorker::run_task at 17 KiB,
RigHarness::execute_claim at 34 KiB, Execution::drive at 20 KiB and Execution::model_turn at 16 KiB.
The scheduled dispatch wrapper was 69 KiB. These are individual frames, not a summed peak.
No stack or execution bound was raised; the existing stock-stack CI gate remains the early alarm.
