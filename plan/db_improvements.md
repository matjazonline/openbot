
• # Database model and query audit

  No source or schema files were changed. The existing untracked plan/*.md files were left untouched.

  ## Executive summary

  I confirmed seven material issues:

  - 3 high severity
  - 3 medium severity
  - 1 low severity

  The most urgent risks are lost or duplicated scheduled runs, unenforced tenant consistency in two schema relationships, and plaintext storage of
  provider API keys.

  ## Correctness and data integrity

  ### High — thread_messages can join records from different tenants

  Locations:

  - migrations/20260817000000_init_schema.sql:241
  - migrations/20260817000000_init_schema.sql:253
  - migrations/20260817000000_init_schema.sql:256
  - src/adapters/persistence/thread.rs:469

  Evidence:

  - The thread FK checks only (channel_id, thread_id).
  - The email FK checks only (company_id, email_message_id).
  - Nothing requires thread_messages.company_id to equal the thread’s company.
  - A rollback-only database fixture successfully inserted a row where:
      - thread_messages.company_id and the email belonged to company B;
      - its thread and channel belonged to company A.

  Impact:

  - Tenant identity becomes ambiguous.
  - Reads scoped through the thread may expose an email stored under another tenant.
  - Cascades and deduplication can operate across an invalid relationship.
  - Current application writes derive values consistently, but imports, future writers, maintenance SQL, or defects can create corruption that all
    existing constraints accept.

  Remediation:

  - Audit existing rows for mismatches.
  - Add an appropriate unique key to email_messages/threads if necessary.
  - Replace the split FKs with composite tenant-consistent FKs, using an additive migration and compatibility validation.

  ### High — schedules can claim one company while targeting another company’s channel

  Locations:

  - migrations/20260817000000_init_schema.sql:624
  - src/adapters/persistence/schedule.rs:222
  - src/application/use_cases/schedule.rs:308

  Evidence:

  - channel_schedules.company_id independently references companies(id).
  - channel_schedules.channel_id independently references channels(id).
  - There is no composite (company_id, channel_id) FK.
  - A rollback-only fixture successfully created a schedule labeled company B that targets company A’s channel.
  - Execution loads the channel and company independently from those two values.

  Impact:

  - A corrupted schedule can combine company B’s company configuration/team data with company A’s channel.
  - Generated sender addresses, participants, payload tenant fields, and task ownership can disagree.
  - The application use case currently checks ownership, but the database does not preserve the invariant it relies on.

  Remediation:

  - Audit existing schedule/company/channel triples.
  - Replace the simple channel FK with a composite FK to channels(company_id, id) in an additive migration.

  ### Medium — provenance JSON can crash row conversion

  Locations:

  - migrations/20260817000000_init_schema.sql:88
  - migrations/20260817000000_init_schema.sql:123
  - src/adapters/persistence/agent.rs:44
  - src/adapters/persistence/channel.rs:79

  Evidence:

  - Both columns are JSONB NOT NULL, but have no shape constraint.
  - Conversions call serde_json::from_value(...).expect(...).
  - PostgreSQL accepts values such as {} that cannot necessarily deserialize into CreationProvenance; reading such a row panics the process.

  Impact:

  - One malformed or legacy row can terminate a request or worker.
  - Runtime SQL and manual maintenance can bypass the Rust serializer.

  Remediation:

  - First replace expect with fallible conversion returning AppError.
  - Then inventory stored shapes and add a compatible JSON shape/version constraint if the format is stable.

  ## Concurrency and transactional integrity

  ### High — schedule claiming is not atomic with creation of the scheduled run

  Locations:

  - src/adapters/persistence/schedule.rs:364
  - src/application/use_cases/schedule.rs:302
  - src/application/use_cases/schedule.rs:410

  Evidence:

  1. The claim transaction advances next_run_at, and disables a one-off.
  2. A separate workflow then creates a thread.
  3. Another transaction creates the prompt message.
  4. Another transaction enqueues the task.
  5. An ordinary returned error calls release_failed_claim, but a process crash between any of these commits cannot.

  Impact:

  - Crash after claim but before task insertion loses the run.
  - Crash after thread/message creation but before task insertion leaves partial artifacts.
  - Retrying after a returned error creates another fresh thread, so partial failures can duplicate schedule-run threads/messages.
  - One-off schedules are most exposed because claiming disables them.

  Remediation:

  - Introduce a durable schedule-run/idempotency record claimed in the same transaction as schedule advancement.
  - Give each logical slot a stable unique key.
  - Make thread/message/task materialization resumable and idempotent from that record.
  - This requires a migration and compatibility handling, not only a query refactor.

  ### Medium — task attempt completion is not fenced to a lease generation

  Locations:

  - src/adapters/persistence/task.rs:44
  - src/adapters/persistence/task.rs:53
  - src/adapters/persistence/task.rs:1418

  Evidence:

  - Reclaiming an expired task reuses the same attempt_number.
  - BEGIN_ATTEMPT_SQL resets the existing attempt row to processing.
  - FINISH_ATTEMPT_SQL checks only task ID, attempt number, and status = 'processing'.
  - It does not check worker ID, lease generation, or a unique execution ID.

  Impact:

  - A stale worker can finish the attempt row after a replacement worker has reopened it.
  - Metrics, error text, duration, and token counts can be attributed to the wrong execution.
  - Task completion itself is correctly worker-fenced; the defect is in the attempt ledger.

  Remediation:

  - Assign each claim a generation/execution UUID and store it on the attempt.
  - Require that generation when finishing.
  - A worker ID alone is weaker if worker IDs can be reused.

  ### Medium — outbox state and lease coherence is unenforced

  Locations:

  - migrations/20260817000000_init_schema.sql:522
  - src/adapters/persistence/task.rs:1102
  - src/adapters/persistence/task.rs:1233

  Evidence:

  - Unlike background_tasks, email_outbox has no lease consistency check.
  - The database accepts sending rows with no worker or lease, and pending/sent rows retaining lease fields.
  - The reaper only matches lock_expires_at <= CURRENT_TIMESTAMP; a sending row with a null expiry is never reaped.

  Impact:

  - Bad writes can leave deliveries permanently stuck.
  - Dashboard pressure queries also fail to classify null-expiry sending rows as expired.
  - Existing claim code writes coherent rows, so this primarily protects against future writers, partial migrations, and operational SQL.

  Remediation:

  - Audit existing rows.
  - Add a status-aware lease constraint analogous to background_tasks.
  - Treat sending with missing lease data as stalled in diagnostics/recovery during rollout.

  ## Security

  ### High — provider API keys are stored and returned as plaintext

  Locations:

  - migrations/20260817000000_init_schema.sql:28
  - migrations/20260817000000_init_schema.sql:80
  - migrations/20260817000000_init_schema.sql:113
  - src/adapters/persistence/company.rs:107
  - src/adapters/persistence/agent.rs:104
  - src/adapters/persistence/channel.rs:86

  Evidence:

  - Keys use ordinary TEXT.
  - They are selected into general domain entities on routine reads.
  - No database or application encryption boundary exists in the persistence layer.

  Impact:

  - Database dumps, read-only database access, query logging mistakes, and broad entity serialization/debugging can disclose credentials.
  - Compromise extends to external LLM/provider accounts.

  Remediation:

  - Encrypt values at the application boundary using envelope encryption/KMS, with key versioning.
  - Avoid selecting secrets in ordinary list/detail projections.
  - Rotate existing provider credentials after migration.
  - This requires compatibility handling and staged backfill.

  ## Maintainability

  ### Low — runtime approval queries depend on table column order

  Locations:

  - src/adapters/persistence/approval.rs:143
  - src/adapters/persistence/approval.rs:242
  - src/adapters/persistence/approval.rs:267
  - src/adapters/persistence/approval.rs:290
  - src/adapters/persistence/approval.rs:475

  Evidence:

  - Multiple runtime query_as::<_, HumanApprovalDb> statements use SELECT * or RETURNING *.
  - These queries are not checked by SQLx offline metadata.
  - Adding or changing columns can break decoding only at runtime.

  Impact:

  - Migration/model drift can escape compilation and fail approval flows in production.

  Remediation:

  - Introduce one explicit approval-column constant and use it in every select/returning clause.
  - This is a safe refactor with runtime database tests.

  ## Performance concerns requiring production statistics

  These are not confirmed regressions because the test database has no meaningful cardinality:

  - Schedule-run listing contains two correlated subqueries per returned row for latest response and message count: src/adapters/persistence/
    schedule.rs:463.

  - Thread-index lookup uses a prefix expression in the form $value LIKE stored_column || '%', for which the present indexes provide no direct
    support: src/adapters/persistence/thread.rs:378.

  - Dashboard time-window queries filter on updated_at or started_at; several supporting indexes are organized around created_at, status, or foreign
    keys: src/adapters/persistence/dashboard.rs:127.

  - Offset pagination for task, outbox, and schedule-run pages is stable because ties include id, but large offsets become progressively expensive:
    src/adapters/persistence/task.rs:1665, src/adapters/persistence/schedule.rs:456.

  Collect production EXPLAIN (ANALYZE, BUFFERS) and table/index statistics before adding indexes. The fixtures were too small for defensible plan
  conclusions.

  ## Intentional and well-designed patterns

  The following should not be treated as defects:

  - Task and outbox claims use atomic UPDATE … FROM claims with FOR UPDATE SKIP LOCKED and deterministic ordering.
  - Task completion/failure/payload renewal is guarded by worker ownership and a live lease.
  - Outbox completion is guarded by worker ownership.
  - Queue-wide worker claims are intentionally tenant-unscoped.
  - Task/outbox list queries use bound QueryBuilder values; no SQL injection path was found.
  - User passwords and confirmation codes are stored as hashes.
  - Approval consumption is atomic, status-checked, and expiry-checked.
  - Approval, outreach, and task state changes that must agree are generally transactionally grouped.
  - Message deduplication verifies a canonical content hash before reusing an existing message ID.
  - Cursor pagination uses timestamp plus UUID tie-breakers.
  - Channel slug uniqueness and the partial primary-slug index correctly prevent canonical/alias collisions.
  - All migrated constraints are validated; the catalog reported no NOT VALID constraints.

  ## Remediation order

  1. Design durable, idempotent schedule-run claiming/materialization.
  2. Add tenant-consistent composite FKs for schedules and thread-message associations after auditing existing rows.
  3. Introduce encryption and narrower projections for API keys, followed by credential rotation.
  4. Add execution-generation fencing to task attempts.
  5. Make provenance decoding fallible, then constrain compatible JSON shapes.
  6. Add outbox lease/status coherence checks and repair handling.
  7. Replace approval * projections with explicit columns.
  8. Gather production query plans before changing performance indexes or pagination.

  ## Validation results

  - Replayed all 10 migrations against mail_agents_test: passed.
  - Catalog inspection: all migrations successful; no unvalidated constraints.
  - SQLX_OFFLINE=true cargo check --all-targets: passed.
  - DATABASE_URL=… cargo test --lib: 469 passed, 0 failed, 0 ignored.
  - The repository guidance identifies 41 database-backed persistence tests; they remained green within the full suite.
  - Rollback-only fixtures confirmed both tenant-consistency defects without leaving data behind.
  - Git status after the audit shows only the user’s pre-existing untracked plan/*.md files.
