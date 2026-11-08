# 08 — Schema cleanup

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Part of [fix-app](README.md). Starts after [01](01-identity-and-ownership.md),
[02](02-silent-stalls.md) and [03](03-workflow-database-guards.md) have merged:
- 01 decides that identity status `disabled` is unused;
- 02 and 03 settle the `task_attempts` stop reasons and the queue behaviour constrained here.

It can run alongside 04–07. The `model_usage_calls` item waits for 06's retention decision.

## Outcome

The schema states only what the code does:
- no column that nothing writes;
- no value that nothing produces;
- every state machine constrained;
- every always-present column `NOT NULL`;
- every JSONB read decoded with the row it came from.

Before dropping anything, grep `src/` for it and record the result.

## Batches

### 1 — Dead or misleading columns and values

- **`background_tasks.last_error`** (`init_schema.sql:3323`). Nothing writes it, yet the dashboard
  selects it (`src/adapters/persistence/dashboard.rs:360`) and shows it as the reason a job is stuck
  (`pages/dashboard.rs:361-373`), so the reason is always blank. Drop it, and show the latest
  attempt's `task_attempts.workflow_failure_code` instead.
- **`messages.audience`.** It defaults to `legacy_unclassified` (`:3867`), which `message.rs:52` calls
  a migration state. An INSERT that omits `audience` therefore creates an undeliverable message
  instead of failing. Drop the default and the value.
- **Values nothing produces:**
  - `message_deliveries.purpose` values `reply`, `mirror` and `outreach` (`:3795`) come only from
    test code (`domain/entities/delivery.rs:207` under `cfg(test)`, and
    `persistence/test_support.rs:380`).
  - `participant_identities.status = 'disabled'`. With it gone, the filter at
    `use_cases/thread/mod.rs:164` goes too.
  - Remove each from its CHECK and its Rust enum.
- **Dead columns:**
  - `external_messages.delivery_part_id`, with its FK (`:9250`) and index (`:7670`). Neither INSERT
    writes it (`thread/external.rs:139`, `:164`).
  - `task_attempts.machine_id`, always `'workflow'` (`lease_claim.rs:173`), and `machine_region`.
  - `task_harness_runs.final_output_consumed_at`, forced NULL by a CHECK (`:4184`).
  - `workflow_dispatch_demand.registered_at`, read only by tests.
- **Tests:** compilation, plus the dashboard page test showing the attempt's failure code.

### 2 — Constant columns on the job queue

This is the largest diff of the plan for the least behaviour, so take it last.
- **`background_tasks.task_type`, `queue_kind` and `task_harness_runs.owner_kind`** hold one value
  each. Every workflow query repeats `queue_kind = 'workflow'`.
- **`background_tasks.payload`** must equal a function of `workflow_execution_id` (`:3340`), yet it
  is still decoded (`workflow/lease.rs:295`).
- Drop all four, and the literals with them (`graft grep "queue_kind='workflow'"` and
  `graft grep "queue_kind = 'workflow'"`).
- **Tests:** the workflow persistence suite.

### 3 — Missing constraints

- **`task_attempts`** (`:4123-4143`). Add these CHECKs:
  - `(status = 'processing') = (finished_at IS NULL)`;
  - which `stop_reason`s each status allows, read from the writers once 02 and 03 have merged;
  - one processing attempt per task, as a partial unique index. First confirm from the claim code
    that this is the invariant.
- **Queue parity with `inbound_events`.**
  - `inbound_events` (`:3655`) requires an error class whenever a row is `retryable` or
    `dead_letter`, and caps the error detail's length.
  - Copy both rules to `message_deliveries` (`:3789`) and `notification_events` (`:3906`).
  - Bound `background_tasks.retry_count` by `max_retries` (`:3337`). Check what
    `workflow/maintenance.rs:71` does at the bound.
- **`agents.provider` and `agents.model`** (`:3290`):
  - CHECK `(provider IS NULL) = (model IS NULL)`, which the application already enforces
    (`use_cases/agent.rs:256-262`);
  - the same provider list as `company_model_connections`.
- **MCP connections** (`:5856`, soft delete in `mcp.rs:196`):
  - Today `UNIQUE (company_id, slug)` also counts deleted rows, so a deleted slug can never be
    reused. Make it a partial unique index, `WHERE deleted_at IS NULL`.
  - Add CHECK `deleted_at IS NULL OR NOT enabled`.
- **Tests, `schema_constraint_*`:** one rejected row per new constraint, plus re-creating a deleted
  MCP slug.

### 4 — Nullability

- **`model_usage_calls` run and execution ids** (`:10861`). Five are nullable, but a CHECK requires
  all five, and Rust reads them as `Option` (`report.rs:39`). Make them `NOT NULL`, drop the CHECK,
  and drop the redundant `run_id IS NOT NULL` from its index and guard trigger. Phase 06 retains
  usage in separate immutable `model_usage_history`, without run/execution/root/job references;
  this tightening applies to live calls and must not be skipped (coordination C010).
- **`task_harness_runs` workflow columns** (`:4181`). Same pattern; same change.
- **`users.password_hash`.** It holds a sentinel such as `google-only:<id>` for OAuth-only users
  (`src/adapters/persistence/user.rs:415`), repeating what `user_login_methods` already records.
  - Make it nullable.
  - Make every password check treat NULL as "no password" and fail closed.
  - `has_login_method` reads `user_login_methods`.
- **Tests:**
  - an OAuth-only user cannot log in with any password;
  - a password user still can;
  - `model_usage_calls` rejects a NULL run id.

### 5 — JSONB decoded with its row

Per `src/adapters/persistence/AGENTS.md` ("Treat persisted JSON as untrusted, versioned input"):
- **Thread reads.** `thread/message.rs:57` and `thread/views.rs:94`, `:248` decode
  `Option<Json<ResponseContract>>` straight from `structured_response->'contract'`, so one message
  that no longer fits breaks the whole thread read. Select it as `Value`, decode it per message, and
  on failure log the message id and render that message without its contract.
- **`delivery/enqueue.rs:35-44`.** It decodes `Json<StructuredResponse>`. Decode it with the
  delivery id attached to an `AppError::Internal`.
- **`harness_checkpoint.rs:8-9`.** A corrupt checkpoint becomes `BadRequest` (a 400), and both the
  serde error and the run id are dropped. Return `Internal` with the run id and the serde error.
- **`mcp.rs:47-52`.** `discovery_json` is an unversioned array decoded strictly, so one bad row
  fails the company's whole list. Decode it per row; a bad row lists without discovery and is
  logged.
- **Tests, `jsonb_row_context_*`:** a corrupted row in each place gives the per-row outcome above,
  and its neighbours still read.

## Focused checks

```sh
./scripts/reset-db.sh --all     # after every init-schema edit
createdb mail_agents_fix_app_08
SQLX_OFFLINE=true TEST_DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_fix_app_08" \
  cargo test --locked --offline --lib schema_constraint_
# repeat with jsonb_row_context_, dashboard, persistence::user, mcp, adapters::persistence::workflow::
dropdb mail_agents_fix_app_08
```

Batch 2 touches most workflow queries, so run the whole workflow persistence suite after it.
Confirm a nonzero test count for each filter.

## Final gate

Run `./scripts/reset-db.sh --all`, then `scripts/check.sh` once. Record the commands and results,
and name any gate that could not run.
