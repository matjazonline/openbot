# 04 Observation generation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [03](03-retrieval-and-context.md). The Rust service extracts useful observations and
session summaries from explicitly authorized, bounded activity windows. It owns its inference
provider and cost controls; it does not invoke an OpenBot agent to implement its core behavior.
OpenBot becomes one capture client in [05](05-openbot-integration.md).

## Batch 04A Define bounded capture and generation

1. Define a provider-neutral activity envelope with opaque source/session identities, source
   revisions, ordering, classification and bounded content. Clients submit only evidence they
   authorize the service and selected model to process. Do not couple payloads to Claude Code
   transcripts, OpenBot workflow tables, or a client's local filesystem.
2. Separate recording evidence, saving a finished memory, and requesting new inference. Neither
   source storage nor a successful later save authorizes paid replay. Each generation request
   identifies a complete bounded window, intended namespace, producing context's authorized read
   ceiling, prior consumed watermark, its exact/fallback status, request expiry and spend
   allowance. Carry a client-produced input's consumed label even when its original higher-level
   evidence is not repeated in the submitted text.
3. Use complete-window server generation for the first implementation. Do not claim isolated
   fragments reconstruct a local stateful observer. A later continuous reducer would need its own
   state/checkpoint and lifecycle design rather than silently reusing source offsets or receipt IDs.
4. Adapt Claude-Mem's observation/summary intent into bounded typed output: useful facts,
   decisions, discoveries, fixes and session outcomes with provenance. Support an explicit
   nothing-to-save outcome. Empty/malformed provider output is distinct from a valid empty result.
   Use a maintained structured-output facility or bounded parser; upstream XML compatibility is
   not required. Keep prompts independent of coding-specific modes unless the client supplies them
   as untrusted descriptive data.
5. Apply privacy exclusion/redaction before durable capture and provider entry. Recheck at the
   generation boundary. Never log raw evidence, credentials or full prompts. Derive classification
   and namespace from trusted input state, never from model-generated fields. Bound summary input
   and prohibit summaries from importing a wider audience or other sessions' evidence.
   Compute output access levels from the maximum of the trusted incoming watermark and all
   information consumed by the generation context; independently generated items sharing that
   context retain the same floor. Enforce the read ceiling before provider entry and verify the
   provider destination is authorized for that content level. Short summaries, redaction and an
   apparently public answer do not prove declassification. Missing precise consumption evidence
   uses the producing context's trusted ceiling; a ceiling-6 context therefore generates/saves at
   6. Missing trusted authority or a known source above that ceiling still refuses the operation.

Focused checks: `memory_observation_` covers useful and trivial windows, privacy-only input,
malformed/empty/valid-skip output, source ordering, source revision conflicts, classification
inheritance, ceiling-6/consumed-4 output, an incoming level-6 answer summarized from otherwise
public text, attempted model-selected lower labels, context exhaustion and session-summary scope.
Include missing precise evidence falling back to the authorized ceiling and known higher sources
being refused. Inspect actual provider requests through a counted local test provider; routine CI
must not make paid calls.

## Batch 04B Own generation jobs and paid requests safely

1. Store each accepted generation operation and its immutable bounded request atomically in
   PostgreSQL. Claim due work with competing-worker protection, expiring leases and generation
   fences. Supervise workers through shutdown; lease loss cancels the real provider operation and
   stale workers cannot commit observations, vectors or completion state.
2. Reserve the operation's spend allowance before dispatch. Preserve the same logical identity
   across attempts and entrypoints. Recheck tenant state, credentials, expiry and source authority
   immediately before dispatch and before committing results. Enforce global and per-tenant
   capacity rather than allowing one tenant to occupy the entire worker pool.
3. Distinguish a safe pre-dispatch retry from an ambiguous paid request. If the provider offers
   usable idempotency/status reconciliation, use it. Otherwise a timeout after possible acceptance
   becomes an observable unknown outcome, not an automatic second paid call. Do not claim
   exactly-once provider execution from database fencing alone.
4. Quota/authentication failure, expired authorization and tenant deletion stop the operation.
   A login, reset, timer or successful unrelated operation cannot revive old inference. Explicit
   recovery requires new authorization, matching complete source context and a known spend scope.
   Bound safe retries, apply backoff, and expose terminal/unknown outcomes.
5. Persist generated observations, source links and job completion in one transaction using
   stable operation/output identities and the frozen consumed-level evidence. A retry, worker
   replacement or changed audience cannot lower the saved label, including a ceiling fallback.
   Retrying persistence of an available result must not invoke the model again. A crash after paid
   response receipt may lose unsaved output; surface that gap rather than claiming automatic
   reconstruction or spending again without authority.
6. Expire pending requests and clean up retained inputs under phase 06's policy. A full page of
   failed rows must back off rather than hot-spin. Expose accepted, running, skipped, failed,
   cancelled and unknown outcomes without returning provider error text as a saved memory.

Focused checks: `memory_generation_` uses real PostgreSQL with simultaneous claimants, expired
leases, stale completions, cancellation, provider ambiguity, duplicate requests, poison batches,
restart, quota reset, credential revocation and attempted classification changes during retry.
Count provider calls to prove no unrequested inference resumes after an outage and explicit-save
delivery retries never trigger generation.

## Verification

2026-11-08, P6a: the P5 review minors m1–m3 (phase 03 recall; decision record D8 and P6a
notes) and batch 04A as scoped there (durable acceptance and workers are 04B). Environment:
macOS arm64, rustc 1.95.0, sqlx-cli 0.8.6, the private PostgreSQL 18.6 + pgvector 0.8.7 cluster
on 127.0.0.1:55432, the prepared and verified model. No provider was called over the network:
the counted test provider records every request, and the Anthropic adapter is checked against a
local fake Messages API on loopback. Logs `tmp/memory-service-run/logs/a4-p6a-*.log`. Each filter
was listed first and selected a nonzero count.

| Command | Selected | Result |
| --- | --- | --- |
| `scripts/focused.sh memory_observation_ --network-isolated` | 11 | 11 passed: redaction (private spans, API keys, PEM keys, bearer credentials, `password=` pairs; idempotent), the provider-safe schema, output validation (empty, malformed, model-chosen `access_level`/`namespace`/`summary` kind, unknown event ids, bounds, a valid `nothing_to_save`), a useful window (typed observations at the shared floor, provenance in window order, the exact request sent), outcomes for truncation, refusal and every provider error with one call each, privacy-only input with no call, privacy rerun at the boundary, labels (ceiling 6/consumed 4 → 4, unknown → 6, an incoming level-6 answer stays 6, excluded events not counted, refusals for a missing ceiling, a known higher source, consumed above the ceiling and the write grant, a destination below the level), ordering, in-window revision conflicts, context exhaustion (513 events, 600 KiB, allowance below the reservation), request expiry, session-summary scope, the adapter's request body and headers and its D9 error mapping, and recorded sources stored redacted |
| `scripts/focused.sh memory_context_ --network-isolated` | 11 | 11 passed, including m1 (expired source, raised and deleted parents → `changed`; `valid_until` from a source's expiry; a tampered digest) and m2 (200 escape-heavy 16,000-character candidates under a full pack: exact served sizes, bodies only for what fits) |
| `scripts/focused.sh memory_recall_ --network-isolated` | 5 | 5 passed (the Python client now returns `sources_sha256`) |
| `scripts/focused.sh memory_contract_ --network-isolated` | 12 | 12 passed (regenerated `openapi.json`; the validate example carries `sources_sha256`) |
| `scripts/focused.sh memory_mcp_ --network-isolated` | 6 | 6 passed |
| `scripts/focused.sh memory_storage_ --network-isolated` | 14 | 14 passed |
| `scripts/focused.sh memory_access_level_ --network-isolated` | 14 | 14 passed |
| `scripts/focused.sh memory_boundary_ --network-isolated` | 3 | 3 passed |
| `scripts/focused.sh memory_search_ --network-isolated` | 18 | 18 passed, including the real-model acceptance check |
| `scripts/focused.sh memory_idempotency_ --network-isolated` | 2 | 2 passed |
| `scripts/focused.sh memory_authority_ --network-isolated` | 24 | 24 passed |
| `scripts/focused.sh memory_startup_ --network-isolated` | 13 | 13 passed |
| `scripts/verify.sh` | 133 | all gates passed, including `prepare --check` over the regenerated `.sqlx` and the new `regex`/`reqwest` lockfile entries; 133/133 tests (63 DB-backed) under macOS network isolation |

Unavailable here and carried to the final gate: the first CI run of `memory-service.yml` and the
image job. The image now also builds `reqwest` with rustls (`aws-lc-sys`), so its first build
proves the builder stage has what that needs. No paid provider call ran; the paid smoke budget
is still 06's open decision. Batch 04B is not started.

2026-11-08, P6a correction round 1 (review `p6a-code-r1`: F1 request identity hashed after
privacy; m1 thinking off via `between_tools`; m2 prose-safe credential patterns; m3 markers only
skip whole values; m4 secret-shaped identifiers refused, description level stated). Same
environment; logs `tmp/memory-service-run/logs/a4-p6a-r1-*.log`.

| Command | Selected | Result |
| --- | --- | --- |
| `scripts/focused.sh memory_observation_ --network-isolated` | 13 | 13 passed, including the F1 hash-after-privacy test (replay across different `<private>` and `password=` text, stored hash equal to the redacted request's, secret-shaped `external_ref` refused), prose negatives and marker-wrapped values, secret-shaped event ids and source refs refused, and the adapter body with `thinking: between_tools` plus effort refusal above `high` |
| `scripts/focused.sh memory_contract_ --network-isolated` | 12 | 12 passed (regenerated `openapi.json`) |
| `scripts/focused.sh memory_storage_ --network-isolated` | 14 | 14 passed |
| `scripts/focused.sh memory_idempotency_ --network-isolated` | 2 | 2 passed |
| `scripts/verify.sh` | 135 | all gates passed; 135/135 tests (64 DB-backed) under macOS network isolation |

CI, the image job and a paid provider call remain unavailable here, as above.

2026-11-08, P6b: the P6a recheck minors (review `p6a-code-r2` m1, n1–n5) and batch 04B
(decision record, P6b notes). Same environment; no provider was called over the network (every
worker run uses the counted local provider). Logs `tmp/memory-service-run/logs/a5-m1-*.log` and
`a5-p6b-*.log`. Each filter was listed first and selected a nonzero count.

| Command | Selected | Result |
| --- | --- | --- |
| `scripts/focused.sh memory_observation_ --network-isolated` (after m1/n1–n5) | 13 | 13 passed: prefixed keys (`DB_PASSWORD=`, `POSTGRES_PASSWORD:`, `client_secret=`, `refresh_token=`, `id_token=`, `GITHUB_TOKEN=`, `MY_API_KEY=`, `OPENAI_API_KEY=`, `AWS_SECRET_ACCESS_KEY=`) and URL userinfo redacted and stable under a second pass; prose ("the basic responsibilities.", "Authorization: required for admins") and ports/`max_tokens:` untouched; quoted values idempotent; keyword-shaped ids refused; secret-shaped revisions refused in windows and `sources.record`; other models refused by the adapter |
| `scripts/focused.sh memory_generation_ --network-isolated` | 11 | 11 passed: atomic, private, idempotent acceptance (cap 0 and no provider refused; excluded events never stored; secrets redacted before capture; a replay differing only in removed text; stored-revision conflicts leave nothing; retention-nulled evidence refused; privacy-only window finished without a call; 8 competing identical requests → one operation; status under the ceiling); a worker saving labelled output once (frozen label, links, spend settled, indexed afterwards; more passes, maintenance and three explicit-save delivery retries make no call); 8 simultaneous claimants → one in flight per tenant, round-robin order; a cancel while dispatched fails the heartbeat and aborts the real call (dropped = 1, charged); stale dispatch after a reaped claim and stale commit after a reaped dispatch refused; ambiguity → charged `unknown`, never re-sent; two definite rejections back off and retry (three dispatches, reservation released between); a terminal `authentication_error` stops the operation and nothing revives it (recovered provider, disable/enable, maintenance; the trigger refuses a revival) until a new request; the daily cap fails a second operation terminally and raising it revives nothing; usage above the reservation recorded and counted; credential expiry at claim, request expiry by the reaper, revocation cancelling queued work; tenant deletion racing 4 workers leaves nothing live and no output after it, at most one call per tenant in flight; a worker aborted after dispatch → `unknown(lease_expired)` after the lease, never re-sent; one aborted between claim and dispatch → reclaimed and sent once; a retried operation keeps its frozen label (the trigger refuses a change; a lower re-request is `idempotency_conflict`); retention in bounded batches (500 + 99), live evidence kept, requests and evidence 30 days after finishing, a poisoned row backing off (≤ 3 ticks in 1.6 s); 40 acceptances racing a sweeper never lose live evidence and every accepted window is sent complete; shutdown aborts the in-flight call (`unknown(shutdown)`) and returns a fresh claim |
| `scripts/focused.sh memory_contract_ --network-isolated` | 12 | 12 passed (regenerated `openapi.json`; generation operations implemented; only purge answers 501) |
| `scripts/focused.sh memory_mcp_ --network-isolated` | 6 | 6 passed (generation status grant checks on MCP) |
| `scripts/focused.sh memory_authority_ --network-isolated` | 24 | 24 passed |
| `scripts/focused.sh memory_storage_ --network-isolated` | 14 | 14 passed |
| `scripts/focused.sh memory_idempotency_ --network-isolated` | 2 | 2 passed |
| `scripts/focused.sh memory_observation_ --network-isolated` | 13 | 13 passed |
| `scripts/focused.sh memory_startup_ --network-isolated` | 14 | 14 passed, including the `MEMORY_ANTHROPIC_*`/`MEMORY_GENERATION_WORKERS` validation and the key never printed |
| `scripts/focused.sh memory_search_ --network-isolated` | 18 | 18 passed |
| `scripts/verify.sh` | 147 | all gates passed, including clippy `-D warnings`, `prepare --check` over the regenerated `.sqlx` and the edited init migration as the owner role; 147/147 under macOS network isolation |

Unavailable here and carried to the final gate: the first CI run of `memory-service.yml`, the
image job (including the `aws-lc-sys` build), and a paid provider call (06's open budget).
Not covered by a dedicated check: a black-box `serve` run against the loopback fake Messages API
(the adapter is checked against it in `memory_observation_`; the workers run with the counted
provider).

2026-11-08, P6b correction round 1 (review `p6b-code-r1`: M1 competing lease/heartbeat/sweeper
tests; m1 sweep candidate predicate; n1, n3–n6 taken, n2 recorded for 06A). Same environment;
logs `tmp/memory-service-run/logs/a5-p6b-r1-*.log`. Each filter was listed first and selected a
nonzero count.

| Command | Selected | Result |
| --- | --- | --- |
| `scripts/focused.sh memory_generation_ --network-isolated` (3 runs) | 16 | 16 passed each run, including: a stalled heartbeat (renewals blocked by another connection's row lock, failing on `lock_timeout`) against a provider slower than the lease, with a second worker, a second same-tenant operation and the reaper racing (lock waits observed; the local deadline drops the call; the locked slot is not reaped; one call in flight; the second call starts after the first was dropped); two reapers racing live heartbeats for 2.5 leases (nothing reaped or dropped); four competing sweepers over 1,200 rows (exact sum, no error); retention not stalled by 600 recently reused rows ahead of an expired one; write-up grants answered uniformly; a call answering within the shutdown drain saved |
| `scripts/focused.sh memory_observation_ --network-isolated` | 13 | 13 passed |
| `scripts/focused.sh memory_authority_ --network-isolated` | 24 | 24 passed |
| `scripts/focused.sh memory_startup_ --network-isolated` | 14 | 14 passed |
| `scripts/focused.sh memory_storage_ --network-isolated` | 14 | 14 passed |
| `scripts/focused.sh memory_recall_ --network-isolated` | 5 | 5 passed (`sources.record` read-grant rule) |
| `scripts/focused.sh memory_idempotency_ --network-isolated` | 2 | 2 passed |
| `scripts/verify.sh` | 152 | all gates passed (regenerated `.sqlx`); 152/152 under macOS network isolation |

CI, the image job and a paid provider call remain unavailable here, as above.

2026-11-08, P6b correction round 2 (review `p6b-code-r2`: M1 test strength, m2 write-up
credentials). Logs `tmp/memory-service-run/logs/a5-p6b-r2-*.log`; each filter selected a
nonzero count.

| Command | Selected | Result |
| --- | --- | --- |
| `scripts/focused.sh memory_generation_ --network-isolated` (3 runs) | 17 | 17 passed each run: the stalled-heartbeat test with injected renewal failures and a free reaper (drop observed before `lease_until`, `unknown(deadline)`, one call in flight, `drops_at_start = [0, 1]`); the reaper-boundary test; a write-up credential reusing its own revisions (record then generate, overlapping windows, re-record under a new key) while another write-up credential gets the uniform 409 |
| Mutations on scratch copies (`a5-p6b-r2-mut{A,D,AD}.log`; copies and scratch databases removed) | 17 | A (local deadline after the lease): 1 failed (stalled heartbeat); D (reaper 0.5 s early): 1 failed (reaper boundary); A + D: 2 failed |
| `scripts/focused.sh memory_observation_`, `memory_recall_`, `memory_storage_`, `memory_authority_`, `memory_idempotency_` (`--network-isolated`) | 13, 5, 14, 24, 2 | all passed |
| `scripts/verify.sh` | 153 | all gates passed (regenerated `.sqlx`); 153/153 under macOS network isolation |

2026-11-08, P6b correction round 3 (review `p6b-code-r3` F1: a flaky stalled-heartbeat test).
Logs `tmp/memory-service-run/logs/a5-p6b-r3-*.log`.

| Command | Selected | Result |
| --- | --- | --- |
| `memory_generation_` × 20 under network isolation (`focused.sh` once, 16 more sequential, 3 concurrent processes) | 17 | 17 passed in every run |
| Mutation A on a scratch copy in a separate `CARGO_TARGET_DIR` (removed, scratch database dropped) | 17 | 1 failed: the stalled-heartbeat test |
| `scripts/verify.sh` | 153 | all gates passed; 153/153 under macOS network isolation |

2026-11-08, P6b correction round 4 (review `p6b-code-r4` F2: a tick-count assertion that
depended on machine speed). The reaper-boundary test now uses the database clock: no tick frees
the slot before `lease_until + grace`, and an attempt qualifies only with a quiet tick that
started within 450 ms of that boundary (repeated, at most 5 attempts, when load keeps ticks
out of that window; an early reaper can never qualify). The same load sensitivity was found
and removed in the reaper-vs-heartbeat test: instead of `ticks > 20` it requires a reaper tick
that ran wholly after dispatch + lease and before the operation finished. Logs
`tmp/memory-service-run/logs/a5-p6b-r4-*.log`.

| Command | Selected | Result |
| --- | --- | --- |
| `memory_generation_` × 20 sequential under network isolation (`focused.sh` first) | 17 | 17 passed in every run |
| `memory_generation_`, 5 rounds × 4 concurrent processes | 17 | 17 passed in all 20 |
| Mutation D on a scratch copy in a separate `CARGO_TARGET_DIR` (removed, scratch database dropped) | 17 | 1 failed: the reaper-boundary test |
| `scripts/verify.sh` | 153 | all gates passed; 153/153 under macOS network isolation |
