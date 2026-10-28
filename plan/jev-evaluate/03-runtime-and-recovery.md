# 03 — Jev runtime and durable recovery

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Execute one `jev.evaluate` step as one bounded, tool-free model request through TypeSafe's
`POST /v1/systemone`. Keep the application contract independent of HTTP and SQLx. Reuse the
workflow worker's claim/fence, root model-call budget, token accounting, and committed-result
semantics; do not use Rig's chat transcript, repair loop, or tool dispatch.

## Batches and checks

1. Define application-owned Jev request/response and execution ports. Build a direct HTTP adapter
   with fixed HTTPS origin, bearer authentication, request/response byte limits below TypeSafe's
   32 MiB maximum, one overall deadline, bounded connection concurrency, cancellation, and
   redacted errors. Send the frozen `state`, questions, and exact model ID. Parse TypeSafe's
   `answers`, `model`, and `usage`; require the returned model ID to equal the pinned version and
   reject malformed JSON, missing/extra answer IDs, type mismatches, nonfinite/out-of-range
   numbers, missing probability keys, and oversized bodies.
   No SDK environment-variable key lookup or runtime model alias resolution.
2. Add a `WorkflowHandler` branch in `src/infra/workflow.rs` and durable, fenced admission for the
   exact prepared request/digest before external I/O. Reserve one root model call and estimated
   spend/tokens before sending; settle actual usage and validated output atomically under the
   current fence. A committed result is replayed without another provider call. A lost response
   is recorded as an unresolved charged invocation; any later provider entry gets a new bounded
   reservation and remains within the root deadline and allowance. Do not claim exactly-once
   billing. A lost lease cancels in-flight I/O and prevents stale settlement.
3. Map protocol failures to explicit workflow failure classes: malformed source/credentials and
   `400`/`401`/`413`/`422` are terminal; `429`/`529`, transport interruption, and timeouts are
   retryable with bounded backoff and `Retry-After` when present. A malformed successful payload
   is a provider-protocol failure, not a success or an empty answer. Record provider/model, usage,
   latency, retry/terminal state, and step identifiers without state text, question bodies, or
   credentials. No untracked background work.

Focused checks after batch 1: `cargo test --locked --offline --lib jev_http_` with a scripted
loopback server; include auth redaction, deadline, cancellation, byte limits, malformed/oversized
responses, and all text answer types. After batch 2 run
`cargo test --locked --offline --lib jev_runtime_` and the existing workflow claim, checkpoint,
budget, and model usage tests. After batch 3 add scripted `429`/`529`/`401`/`422`/timeout tests
and re-run the affected runtime checks. List filters before using them and record test counts.

Database-backed concurrency gate: two workers compete for one step; only one fenced owner may
settle it. Inject a crash after reservation and after provider response, reconstruct the worker,
and prove no committed output is lost, no stale owner settles, the model-call budget never
under-counts entries, and retries are bounded.

Gate: the mixed text fixture executes through the production worker against a controlled
loopback TypeSafe endpoint and yields the exact typed output once.
