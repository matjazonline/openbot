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
