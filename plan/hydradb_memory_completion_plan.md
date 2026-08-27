# HydraDB Memory: Production Completion Plan

## Goal

Complete the existing provider-neutral memory foundation by wiring durable provider provisioning,
agent-run retrieval and persistence, and owner-only company/channel settings with readiness
interlocks. Recall failures must use the normal task retry/dead-letter lifecycle; persistence
failures must never retract a completed response.

## 1. Harden the current foundation

- Add exhaustive domain types for connection readiness, provider errors, provisioning jobs, and
  cleanup jobs. Use `Company.memory_provider = None` as the sole disabled representation.
- Add a deterministic remote workspace ID derived only from company UUID.
- Correct chunk deduplication to prefer source/chunk ID and otherwise use a normalized-content hash,
  including overlap between ID-bearing and ID-less results. Apply the 16,000-character cap safely
  at UTF-8 character boundaries.
- Update the HydraDB adapter to use the exact v2 multipart ingestion contract, inspect every per-item
  result, and issue independent collection writes concurrently.
- Type HydraDB failures: authentication, rate limit, timeout, malformed response, non-ready,
  unavailable, and rejected item. Ensure errors/logs cannot contain API keys or memory content.

Tests: deterministic IDs, mixed-result deduplication, Unicode cap, secret redaction, and mock-server
coverage for v2 headers/envelopes, multipart ingestion, empty results, partial failures, malformed
JSON, timeouts, 401, 404, 429, and 5xx.

## 2. Add configuration and provider persistence boundaries

- Add optional `HydraDbConfig` to `AppConfig`, loaded from `HYDRA_DB_API_KEY`,
  `HYDRA_DB_BASE_URL`, `HYDRA_DB_FAST_TIMEOUT_SECS`, and
  `HYDRA_DB_THINKING_TIMEOUT_SECS`. Reject partial or invalid configuration at startup without
  exposing the credential.
- Introduce `MemoryConnectionPersistence` with operations to read readiness, select/disable a
  provider, and lease/renew/complete/retry provisioning and cleanup jobs.
- Add `memory_provisioning_jobs`; do not overload agent `background_tasks`. Add due-job indexes,
  leases, `FOR UPDATE SKIP LOCKED`, bounded backoff, and idempotency constraints.
- Selecting HydraDB must transactionally update the company, upsert its deterministic connection,
  and enqueue one idempotent provisioning job. Disabling/re-enabling retains the connection and
  remote workspace.
- Company deletion must transactionally copy configured remote workspaces into unscoped durable
  cleanup jobs before deleting the company. Cleanup rows must survive the cascade.
- Add owner-authorized `MemoryUseCases` for selection, disable, readiness, manual retry, and deletion
  preparation. API read models may expose readiness and safe errors, never credentials.

Tests: atomic selection/enqueue, idempotent reselection, disable/re-enable, competing leases, lease
expiry, retry backoff, and cleanup survival after company deletion.

## 3. Wire provisioning and cleanup workers

- Build `MemoryProviderRegistry` in `infra/setup.rs`, registering HydraDB only when deployment
  credentials exist. Inject the registry and memory persistence into a focused `MemoryWorker` (or
  independent `TaskWorker` lanes).
- Provisioning iteration:
  1. lease one due job;
  2. mark the connection provisioning;
  3. call `POST /databases` idempotently;
  4. poll `GET /databases/status` across iterations;
  5. on `ready_for_ingestion`, mark ready and complete the job;
  6. on failure, store only redacted diagnostics and retry according to classification/backoff.
- Cleanup iteration:
  1. lease a durable cleanup job;
  2. call `DELETE /databases`;
  3. treat absent/404 as success;
  4. retry retryable failures until confirmed deleted.
- Disabling or switching providers must not enqueue cleanup; only company deletion does.

Tests: asynchronous create-to-ready, duplicate create, restart/expired lease, auth/config failure,
cleanup 404, transient retry, lease heartbeat, and graceful shutdown.

## 4. Add company settings and readiness exposure

- Extend classic and `/ui` company forms/routes with a Disabled/HydraDB selector and readiness
  display (`pending`, `provisioning`, `ready`, or `failed` with safe diagnostic).
- Provider changes go through `MemoryUseCases`, remain owner-only, and enqueue provisioning as part
  of the same transaction as selection.
- Extend company JSON create/update/read payloads with provider and readiness using backward-
  compatible serde defaults.
- Render a retry action for failed provisioning if the deployment has the provider configured.
- Verify credentials never appear in HTML, serialized entities, logs, or API responses.

Tests: owner-only writes, provider transitions, readiness display, rejected saves, retry action, API
serialization, and credential non-disclosure.

## 5. Add channel controls and enforce readiness server-side

- Extend `ChannelForm`, `ChannelJsonPayload`, `SubmittedChannel`, `ChannelDraft`, and every write
  builder with the six flags, recall mode, and max result count. Remove current hard-coded `false`
  writes that would erase stored settings.
- Render a two-row/three-column matrix:

  | Operation | Company | Agent | User |
  | --- | --- | --- | --- |
  | Retrieve | checkbox | checkbox | checkbox |
  | Persist | checkbox | checkbox | checkbox |

- Render `fast|thinking` and `1..=20` below the matrix. Defaults: all flags off, fast, 5.
- Disable all controls unless the selected company provider is ready, while still showing stored
  checked values. Server validation is authoritative: reject any enabled memory flag without a
  ready provider and reject invalid mode/limits rather than silently resetting them.
- Preserve all independent values on edit and rejected-form rerender.

Tests: all checkbox combinations, independent round trips, owner-only writes, disabled/pending/
failed readiness interlocks, mode/limit validation, and create/edit/rejection UI behavior.

## 6. Introduce a provider-neutral `MemoryCoordinator`

- Place scope resolution, readiness checks, fallback warnings, recall formatting, and persistence
  fan-out behind one coordinator used by both inbound and scheduled runs.
- Inputs are application concepts: company, channel, optional agent, optional normalized sender,
  task/channel/agent IDs, latest prompt, local history, delivery/upstream context, and final answer.
- Resolve checked scopes independently for recall and persist. Missing agent/user falls back to
  company, retains the highest requested weight, deduplicates targets, and emits structured
  monitoring labels containing IDs, operation, and missing scope—but no sender or message content.
- Recall formatting must be a clearly delimited, explicitly untrusted historical-context section.
- Persistence construction must exclude recalled chunks, system prompts, credentials, tool
  internals, suspended output, and partial output. Reuse one stable task/channel/agent-derived memory
  ID for every target collection.

Tests: all 64 flag combinations per operation, weights 3/2/1, fallback precedence, warnings,
normalization, stable IDs, delimiter safety, and persistence exclusion rules.

## 7. Wire recall before inbound and scheduled agent execution

- Find/extract the shared boundary immediately before `AgentRunner::execute`, after company,
  channel, optional agent, and latest prompt are known.
- When any retrieve flag is enabled:
  1. require a configured, ready provider;
  2. resolve scopes and warnings;
  3. issue one weighted multi-collection query using the latest prompt;
  4. pass only channel/agent names as short `additional_context`;
  5. deduplicate/cap chunks and append the delimited memory section;
  6. run the existing prompt-injection guardrail over this final assembled prompt.
- Scheduled runs have no sender, so user scope falls back to company with a warning.
- Empty recall succeeds. Every provider/readiness/malformed/timeout failure returns from the task so
  existing attempt, retry, lease, and dead-letter handling remains authoritative.
- Never store recalled chunks in durable task payloads.

Tests: one query for three scopes, exact weights/envelope, empty success, missing-agent and scheduled-
user fallback, guardrail ordering, retry/dead-letter behavior, and no LLM call after failed recall.

## 8. Wire best-effort persistence after completed runs

- At the shared final, non-suspended completion boundary, call the coordinator with delivery
  context, upstream pipeline context, local history, latest inbound/scheduled prompt, and response.
- Resolve persistence scopes independently and issue concurrent writes to deduplicated collections.
- Record per-collection success/failure metrics and redacted logs. Accepted ingestion does not wait
  for indexing.
- Persistence failures never change completed task/result state, retract the saved response, or
  prevent outbound delivery. Failed, partial, retryable, or suspended runs write nothing.
- Apply identical behavior to scheduled runs, including user-to-company fallback.

Tests: three scopes produce three writes with one ID; fallback produces one company write; partial
and total provider failure leave response/task successful; retries reuse IDs; suspended/failed runs
do not persist.

## 9. Operational verification

- Add an ignored live v2 smoke test gated by `HYDRA_DB_API_KEY`; normal tests remain offline.
- Document environment variables, readiness transitions, retry/recovery, disabling versus deletion,
  email-collection PII implications, and monitoring metrics.
- Apply migrations, inspect schema/indexes, regenerate SQLx metadata, then run:

  ```sh
  cargo fmt -- --check
  git diff --check
  SQLX_OFFLINE=true cargo check --all-targets
  DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo test --lib
  ```

- Manually verify both UI shells/themes, readiness transitions, disabled controls, rejected form
  preservation, and absence of secrets in HTML/API responses.

## Recommended order

1. Foundation corrections and adapter contract tests.
2. Persistence APIs and migrations.
3. Registry/configuration and worker lanes.
4. Company provider/readiness settings.
5. Channel matrix and validation.
6. Coordinator and monitoring.
7. Inbound/scheduled recall.
8. Best-effort persistence.
9. Full verification and documentation.

Keep `cargo check --all-targets` green after every stage and regenerate SQLx metadata immediately
after each SQL change.
