# Phase 2 — Schema, persistence, use cases

Read [`general_plan_instructions.md`](general_plan_instructions.md) first, then
[`phase1.md`](phase1.md) — this phase stores what phase 1 defined.

**Goal:** a document can be uploaded, stored, listed and deleted. It never reaches a provider yet;
`ingest_status` stays `pending` and the job rows pile up harmlessly for phase 4 to drain.

**Landable on its own:** yes, provided the UI is not wired (phase 5). Useful earlier via tests.

---

## 1. Schema — edited into `migrations/20260817000000_init_schema.sql`

Reset and edit in place; recreate both databases (see the general instructions). Declare both tables
**after `memory_cleanup_jobs` (line 1140) and before the trigger block at 1176**, so the
declaration-order comment at line 1026 about company-deletion FK ordering still reads in order.

### `memory_documents` — the record of truth

```sql
CREATE TABLE memory_documents (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    -- NULL is the company scope. Agent-scoped rows go with their agent.
    agent_id UUID REFERENCES agents(id) ON DELETE CASCADE,
    scope TEXT NOT NULL CHECK (scope IN ('company', 'agent')),
    -- The very string `resolve_scopes` builds. Stored rather than derived so a forget job that
    -- outlives its agent still knows where the chunks are.
    collection TEXT NOT NULL,
    title TEXT NOT NULL,
    filename TEXT NOT NULL,
    format TEXT NOT NULL
        CHECK (format IN ('text','markdown','csv','json','html','pdf','docx')),
    byte_size INTEGER NOT NULL CHECK (byte_size > 0),
    content_sha256 TEXT NOT NULL,
    -- Private-bucket object key. NULL when no bucket is configured: the extracted text is the
    -- record of truth, the original is enrichment.
    storage_key TEXT,
    extracted_text TEXT NOT NULL,
    chunk_count INTEGER NOT NULL CHECK (chunk_count > 0),
    ingest_status TEXT NOT NULL DEFAULT 'pending'
        CHECK (ingest_status IN ('pending','ingesting','ready','failed')),
    ingest_error TEXT,
    ingested_provider TEXT CHECK (ingested_provider IN ('hydradb','hindsight')),
    ingested_at TIMESTAMPTZ,
    uploaded_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT memory_documents_agent_matches_scope CHECK (
        (scope = 'agent'   AND agent_id IS NOT NULL) OR
        (scope = 'company' AND agent_id IS NULL)
    ),
    CONSTRAINT memory_documents_ready_names_its_provider CHECK (
        ingest_status <> 'ready' OR (ingested_provider IS NOT NULL AND ingested_at IS NOT NULL)
    ),
    CONSTRAINT memory_documents_title_not_blank CHECK (btrim(title) <> ''),
    -- The same file twice in one scope is a conflict, not a duplicate.
    CONSTRAINT memory_documents_scope_content_key UNIQUE (company_id, collection, content_sha256),
    -- Tenant-scoped FK: an agent row must belong to the same company. `agents_company_id_id_key`
    -- already exists (line 217), so this costs nothing.
    CONSTRAINT memory_documents_company_agent_fkey
        FOREIGN KEY (company_id, agent_id) REFERENCES agents(company_id, id)
);

CREATE INDEX memory_documents_scope_idx
    ON memory_documents (company_id, collection, created_at DESC, id DESC);
```

`src/adapters/persistence/AGENTS.md`, "Preserve invariants at the database boundary": do not model
`(company_id -> companies)` and `(agent_id -> agents)` as two independent facts when the real
invariant is that the agent belongs to the company.

### `memory_document_jobs` — the queue

**Copy `memory_cleanup_jobs` (line 1140) for the queue columns and both lease CHECKs verbatim**, then
add what a document job needs. Mirroring the strongest existing queue constraint rather than
inventing new semantics is the rule.

```sql
CREATE TABLE memory_document_jobs (
    id UUID PRIMARY KEY,
    -- Deliberately NOT a foreign key: a forget job outlives its document and its agent.
    document_id UUID NOT NULL,
    company_id UUID REFERENCES companies(id) ON DELETE SET NULL,
    provider TEXT NOT NULL CHECK (provider IN ('hydradb','hindsight')),
    remote_database_id TEXT NOT NULL,
    collection TEXT NOT NULL,
    operation TEXT NOT NULL CHECK (operation IN ('ingest','forget')),
    chunk_count INTEGER NOT NULL CHECK (chunk_count > 0),
    -- Resumable cursor. A crash mid-document restarts here, not at zero.
    next_chunk_index INTEGER NOT NULL DEFAULT 0 CHECK (next_chunk_index >= 0),

    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','leased','completed','failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    failure_attempts INTEGER NOT NULL DEFAULT 0,
    available_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    lease_token UUID NULL,
    lease_expires_at TIMESTAMPTZ NULL,
    operation_generation BIGINT NULL,
    last_error TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    -- The logical operation's stable key. Upserting flips a queued ingest into a forget, so
    -- deleting a document mid-ingest replaces the work instead of racing it.
    UNIQUE (document_id, provider),
    CONSTRAINT memory_document_jobs_cursor_within_count CHECK (next_chunk_index <= chunk_count),
    CONSTRAINT memory_document_jobs_lease_state_check CHECK (
        (status =  'leased' AND lease_token IS NOT NULL AND lease_expires_at IS NOT NULL) OR
        (status <> 'leased' AND lease_token IS NULL     AND lease_expires_at IS NULL)
    ),
    CONSTRAINT memory_document_jobs_generation_state_check CHECK (
        (status =  'leased' AND operation_generation IS NOT NULL) OR
        (status <> 'leased' AND operation_generation IS NULL)
    )
);

CREATE INDEX memory_document_jobs_due_idx
    ON memory_document_jobs (available_at, created_at, id)
    WHERE status = 'pending';
```

Add `'shodh'` to both new `provider` CHECKs in the list in `plan/shodh_memory_provider.md` §1.

---

## 2. Persistence — `src/adapters/persistence/memory_document.rs` (new)

Runtime `sqlx::query_as::<_, MemoryDocumentDb>` like the rest of the memory adapter (no compile-time
macros here, so `.sqlx/` will show no diff — run `cargo sqlx prepare` anyway). Row structs go in
`memory_rows.rs` beside the existing ones, with `TryFrom<…Db>` conversions that are **fallible**
(status and scope arrive as text; never `expect` them).

SQL style, from `src/adapters/persistence/AGENTS.md`:

- one shared `MEMORY_DOCUMENT_SELECT` const naming every column — the `WHERE` is `format!`ed on,
  so one edit fixes every read path;
- **`AS` on every table alias, and alias to a word**: `FROM memory_documents AS document`,
  `JOIN memory_document_jobs AS job` — not `d`/`j`;
- never `SELECT *`.

The positional-bind trap applies: the column list order and the `.bind()` chain are one fact written
twice. **Append columns, never insert**, and re-read the whole statement after editing — a swapped
pair of same-typed binds compiles and passes a green suite while writing the wrong data.

### Port — `src/application/use_cases/memory_document.rs` (new)

Ports live where they are consumed. No defaulted methods: every one of these is a durable write or a
lease transition.

```rust
pub struct MemoryDocumentWrite {
    pub company_id: Uuid,
    pub scope: MemoryDocumentScope,
    pub title: String,
    pub filename: String,
    pub format: DocumentFormat,
    pub byte_size: usize,
    pub content_sha256: String,
    pub storage_key: Option<ObjectKey>,
    pub extracted_text: String,
    pub chunk_count: usize,
    pub uploaded_by: Uuid,
}
impl MemoryDocumentWrite { fn normalize(&mut self) -> AppResult<()>; }

#[async_trait]
pub trait MemoryDocumentPersistence: Send + Sync {
    // reads
    async fn list_for_scope(&self, company_id: Uuid, collection: &str)
        -> AppResult<Vec<MemoryDocument>>;
    async fn get(&self, company_id: Uuid, document_id: Uuid) -> AppResult<Option<MemoryDocument>>;
    async fn count_for_scope(&self, company_id: Uuid, collection: &str) -> AppResult<i64>;

    // writes — each enqueues or cancels the matching job in the SAME transaction
    async fn create(&self, write: MemoryDocumentWrite) -> AppResult<MemoryDocument>;
    async fn delete(&self, company_id: Uuid, document_id: Uuid) -> AppResult<Option<ObjectKey>>;
    async fn requeue(&self, company_id: Uuid, document_id: Uuid) -> AppResult<()>;

    // queue — mirrors MemoryConnectionPersistence (use_cases/memory.rs:59) exactly
    async fn claim_document_job(&self, lease_token: Uuid, lease_expires_at: DateTime<Utc>)
        -> AppResult<Option<LeasedDocumentJob>>;
    async fn renew_document_job(&self, job_id: Uuid, lease_token: Uuid,
        lease_expires_at: DateTime<Utc>) -> AppResult<bool>;
    async fn advance_document_cursor(&self, job_id: Uuid, lease_token: Uuid,
        next_chunk_index: i32) -> AppResult<bool>;
    async fn reschedule_document_job(&self, job_id: Uuid, lease_token: Uuid,
        available_at: DateTime<Utc>) -> AppResult<bool>;             // no failure attempt consumed
    async fn complete_document_job(&self, job_id: Uuid, lease_token: Uuid) -> AppResult<bool>;
    async fn retry_document_job(&self, job_id: Uuid, lease_token: Uuid,
        available_at: DateTime<Utc>, safe_error: &str, terminal: bool) -> AppResult<bool>;
}
```

`create`, `delete` and `requeue` each write the document row **and** upsert or delete the job row in
one transaction. A response that needs a stored document and a queued intent is one logical commit
(`src/application/AGENTS.md`) — an ordinary `Err` cleanup cannot recover from a crash between two.

Every lease-bearing method is conditional on `lease_token` **and** `operation_generation` matching.
A task id plus attempt number is not a fence; `reschedule_document_job` deliberately does not touch
`failure_attempts`, which is what lets a `NotReady` binding wait without burning the retry budget.

`delete` returns the `storage_key` so the caller can drop the object after the row is gone.

---

## 3. Use cases — `MemoryDocumentUseCases`

Same shape as `AgentUseCases` (`use_cases/agent.rs:196`): `#[derive(Clone)]`, `Arc<dyn Port>` deps,
`#[instrument(skip(self))]`, authorization first.

```rust
pub async fn list(&self, user_id, company_id, scope) -> AppResult<Vec<MemoryDocument>>;
pub async fn upload(&self, user_id, company_id, scope, filename, bytes) -> AppResult<MemoryDocument>;
pub async fn delete(&self, user_id, company_id, document_id) -> AppResult<()>;
pub async fn retry(&self, user_id, company_id, document_id) -> AppResult<()>;
pub async fn download(&self, user_id, company_id, document_id) -> AppResult<StoredObject>;
```

`upload`, in order — the guard phase first, cheap rejections before any I/O:

1. `managed_company(companies, user_id, company_id)` (`use_cases/company.rs:281`). A foreign company
   must report exactly like a missing one; do not let a tenant id be probed.
2. `MemoryUseCases::require_ready_binding(company_id)` — see below.
3. For an agent scope, load the agent and reject one whose `company_id` is not this company or is
   `NULL`. **Library agents have no company memory database and therefore no documents.**
4. `count_for_scope` against `MAX_MEMORY_DOCUMENTS_PER_SCOPE`.
5. `MemoryDocumentUpload::parse` — bounds and format.
6. `extractor.extract(&upload)` inside `spawn_blocking`; empty text is
   `"That document has no readable text."`.
7. `chunk_document_text`; an empty chunk list is the same refusal.
8. Store the original in the private bucket if `FileStorage` is present, via
   `ObjectKey::generated(documents_folder, format.extension())`. **A storage failure is logged and
   the upload continues with `storage_key = None`** — the text is what matters, and this is how
   `store_inbound_attachments` already behaves.
9. `create`, which inserts the row and enqueues the ingest job in one transaction.
10. Map the unique-violation on `(company_id, collection, content_sha256)` to
    `AppError::Conflict("That document is already in this company's memory.")`.

Errors that feed an authorization decision propagate with `?`. No `.ok().flatten()`, no
`.unwrap_or(false)` on any of the guards above (`src/AGENTS.md`).

### Extract the ready-binding check — one decision, one place

`ChannelUseCases::check_memory_interlock` (`use_cases/channel.rs:283`) already encodes "a ready
binding on a provider this deployment configures", with four distinct messages. Move that `match`
onto `MemoryUseCases::require_ready_binding(company_id) -> AppResult<MemoryConnection>` and have
both channels and documents call it. `memory_ready` (`channel.rs:329`) becomes a thin wrapper.

This is a refactor of working code, so it lands with the existing channel-interlock tests still
green and unchanged — if a message has to move, keep the wording identical.

---

## 4. Wiring

- `src/adapters/http/app_state.rs` — add `memory_document_use_cases`, with the `FromRef` impl the
  `Workspace` extractor needs.
- `src/infra/setup.rs` — construct `FileDocumentExtractor` and `MemoryDocumentUseCases` beside the
  existing memory wiring at `:80-112`.
- `src/infra/config.rs` — `documents_folder` on `GcsConfig` from `GCS_DOCUMENTS_FOLDER`, defaulting
  to `documents`, on the **existing private attachments bucket**. Document it in `.env.example` and
  `docs/deploy.md` beside the other `GCS_*` keys.

---

## 5. Tests

DB-backed, so every rule from the general instructions applies — suffix unique values with
`Uuid::new_v4().simple()`, never assert on a global queue's totals, **leave no claimable residue**
(a stray pending job row will hang an unrelated worker test rather than fail it).

- `a_document_round_trips_through_create_read_update_and_delete` — extend it to write the new value,
  read it back, then *update and re-read*. A field bound on `INSERT` and forgotten in `UPDATE`
  passes every other check.
- `creating_a_document_enqueues_exactly_one_ingest_job_for_the_selected_provider`.
- `deleting_a_document_replaces_its_queued_ingest_with_a_forget_job` — assert the `UNIQUE
  (document_id, provider)` upsert flipped the row rather than adding one.
- `the_same_file_twice_in_one_scope_is_a_conflict` — and the same file in *different* scopes is not.
- `an_agent_document_cannot_name_another_companys_agent` — the composite FK rejects it. This is the
  rollback-scoped cross-tenant test `src/adapters/persistence/AGENTS.md` asks for.
- `a_company_scoped_row_with_an_agent_id_is_rejected` — the scope CHECK.
- `the_per_scope_document_cap_is_enforced`.
- `a_stale_lease_token_cannot_advance_the_cursor_or_complete_the_job`.
- `uploading_without_a_ready_memory_binding_is_refused_with_the_channel_wording` — one case per
  `ActiveMemoryBinding` variant.
- `a_library_agent_has_no_documents`.
- `an_upload_survives_a_bucket_failure_with_no_storage_key`.

Clean up: any test that creates a claimable job row completes it, deletes it, or pushes
`available_at` beyond the claim horizon before the guard drops.

---

## Done when

- `DATABASE_URL=… cargo test` green, run three or four times to prove no residue.
- `psql -c '\d memory_documents' -c '\d memory_document_jobs'` shows the shape you think you wrote.
- `DATABASE_URL=… cargo sqlx prepare -- --all-targets` run and committed (an empty diff is expected
  and proves nothing — the macro files share the database).
- `SQLX_OFFLINE=true cargo build` passes.
