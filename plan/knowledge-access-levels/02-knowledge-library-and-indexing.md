# 02 — Knowledge library, upload, and indexing

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A member uploads one or several files. Each file's level is its name's prefix: `0_sales.md` is
level 0, and `6_sales.md` or `6-sales.md` is level 6. A name without a 0–9 digit followed by `_`
or `-` at the start is rejected. Files whose names differ only in the prefix are layers of one document, and the same
prefixed name again overrides the existing layer. No one can upload above their own level. Each stored document is chunked and embedded in the same
commit. This covers D6, P2, P3, P4 and P8.

## Batch 02A — Schema

```sql
CREATE TABLE public.knowledge_documents (
    id uuid PRIMARY KEY,
    company_id uuid NOT NULL REFERENCES public.companies (id) ON DELETE CASCADE,
    -- The file name without its level prefix. It is displayed as `{access_level}_{name}`
    -- whichever separator was uploaded, so the level is stored once and the two cannot disagree.
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 253),
    access_level public.access_level NOT NULL,
    content text NOT NULL,
    content_sha256 bytea NOT NULL,
    uploaded_by uuid NOT NULL REFERENCES public.users (id),
    revision bigint NOT NULL DEFAULT 1,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (id, access_level)
);
CREATE UNIQUE INDEX knowledge_documents_layer_key
    ON public.knowledge_documents (company_id, lower(name), access_level);

CREATE TABLE public.knowledge_chunks (
    id uuid PRIMARY KEY,
    company_id uuid NOT NULL,
    document_id uuid NOT NULL,
    access_level public.access_level NOT NULL,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    heading text,
    body text NOT NULL,
    embedding_model text NOT NULL,
    embedding real[] NOT NULL CHECK (array_length(embedding, 1) = 384),
    -- Exact-term matching (P9). 'simple' does no stemming and drops no stopwords, so it suits a
    -- multilingual corpus and keeps product codes as tokens.
    search_text tsvector GENERATED ALWAYS AS
        (to_tsvector('simple', coalesce(heading, '') || ' ' || body)) STORED,
    FOREIGN KEY (document_id, access_level)
        REFERENCES public.knowledge_documents (id, access_level)
        ON UPDATE CASCADE ON DELETE CASCADE,
    UNIQUE (document_id, ordinal)
);
CREATE INDEX knowledge_chunks_search_idx
    ON public.knowledge_chunks (company_id, embedding_model, access_level);
```

- The composite foreign key makes it impossible for a chunk's level to differ from its document's.
  Search filters on the chunk's own column, so the check needs no join.
- `search_text` has no GIN index. Search already scans one company's visible chunks for the
  vector ranking (04B) and computes the keyword match in that same scan. Add an index only if a
  query-plan test shows the scan is a bottleneck.
- Use the real `users` and `companies` key names from the init schema, and add a matching
  `company_id` foreign key in the style the persistence guide requires
  (`src/adapters/persistence/AGENTS.md`).
- Parse an uploaded name in one place:
  `KnowledgeFileName::parse(raw) -> Result<(AccessLevel, KnowledgeDocumentName), FileNameError>`,
  in the domain next to `AccessLevel`. `KnowledgeDocumentName` uses `string_newtype!` in
  `src/domain/entities/value_objects.rs` and compares case-insensitively (P2). Steps:
  1. Take the base name (path segments stripped), trim it, and reject control characters.
  2. Require exactly one ASCII digit, then `_` or `-`, then a non-empty remainder.
  3. The digit is the level and the remainder is the document name. The separator is not
     stored, so `6-sales.md` and `6_sales.md` are the same layer.

  `10_sales.md`, `a_sales.md`, `5 sales.md`, `5.sales.md`, `sales.md`, `5_` and `5-` are all
  errors. The error names the file and gives an example: "`sales.md`: the name must start with a
  level 0–9 and `_` or `-`, for example `0_sales.md`".

## Batch 02B — Upload, override, delete, list

- The upload route is multipart. Each file's level comes from its name (02A); there is no separate
  level field. Bounds are enforced before any work is
  done: at most 10 files per request and at most 1 MiB of text per file (P8), plus a per-company
  total of 10 MiB of text. A request that exceeds a bound is rejected as a whole.
- Format (P4): accept UTF-8 `.txt` and `.md` only. Reject invalid UTF-8 or NUL bytes with a clear
  per-file message. Normalise the text with `src/domain/entities/unicode_sanitization.rs`.
- Authorisation is a pure rule, `may_write_document(actor, level)`: the actor must be on the team
  and `level <= actor.level` (D6). Upload, override and delete all use it. An outside contact
  never reaches these routes.
- Write path, in this order:
  1. Parse every name, validate every file, and authorise every level. Any failure, including a
     missing prefix, rejects the whole request, so nothing is written.
  2. Chunk and embed every file outside the transaction, in `spawn_blocking` behind a global
     semaphore, with a deadline.
  3. In one transaction, for each file: run
     `INSERT … ON CONFLICT (company_id, lower(name), access_level) DO UPDATE` with the new
     content, `revision + 1` and `uploaded_by`; delete that document's chunks; insert the new
     chunks.

  Every transaction writes a document together with exactly its own chunks, so the last
  committer wins consistently.
- Delete: `may_write_document` checks the document's level, and the chunks cascade.
- The library page lists documents whose level is at or below the viewer's level, grouped by
  document name. Each layer shows its full file name: for example "sales.md — levels 0, 5" with
  `0_sales.md` and `5_sales.md`. A layer above the viewer's level is not listed, not counted and
  not hinted at.

## Batch 02C — Chunking and the embedding port

- Chunking is a pure function, `chunk_document(&str) -> Vec<Chunk>`:
  - split at Markdown headings, then at blank lines;
  - pack paragraphs up to about 400 tokens, with a hard maximum of 512 tokens (the model's
    limit), counted with the model's tokenizer;
  - no overlap;
  - keep a short document as one chunk;
  - record each chunk's heading path.

  A chunk never spans two documents, so it never mixes levels.
- `TextEmbedder` is a port owned by the application layer:
  - `model_id() -> EmbeddingModelId`;
  - `embed_passages(&[&str]) -> AppResult<Vec<[f32; 384]>>`;
  - `embed_query(&str) -> AppResult<[f32; 384]>`.

  Vectors are L2-normalised before storage, so search can use a dot product. Apply the
  passage/query prefixes that e5 models require inside the adapter.
- The adapter goes in `src/adapters/embedding/`, using `fastembed` (P3):
  - pin an exact crate version and commit the lockfile;
  - bake the model files into the image and verify their checksum at startup;
  - disable runtime downloads;
  - the production image keeps running as non-root.

  If the model is missing, or its checksum does not match, startup fails with an error naming the
  expected path. A test adapter returns deterministic vectors, so unit and database tests never
  load the model.
- Bound memory: one loaded model per process, shared behind an `Arc`.

## Focused checks

Prefix `knowledge_upload_`:

- uploading above your own level is refused and writes nothing;
- `0_sales.md` and `6_sales.md` create two layers of `sales.md`;
- `6_sales.md` uploaded again (`6_Sales.md` included) overrides that layer, leaving only the new
  chunks;
- name parsing: `0_sales.md` → 0 and `sales.md`; `9_x.md` → 9; `5-sales.md` → 5 and `sales.md`;
  `docs/5_sales.md` → 5; `sales.md`, `10_sales.md`, `a_sales.md`, `5 sales.md`, `5_` and `5-`
  are rejected, and one bad name rejects the whole request;
- `5-sales.md` overrides an existing `5_sales.md` rather than adding a layer;
- `7_sales.md` from a level-5 member is refused;
- **competing uploads** of one prefixed name end with one row whose chunks match its stored
  content;
- the delete permission matrix;
- list filtering hides higher layers;
- bounds and format rejections;
- the database rejects a chunk whose level differs from its document's.

Prefix `knowledge_chunk_`: boundary, maximum-size, short-document and heading-path cases.

Run with `cargo test --locked --offline --lib knowledge_upload_` and `… knowledge_chunk_`. After
the SQL changes, regenerate `.sqlx/` as `src/AGENTS.md` describes.
