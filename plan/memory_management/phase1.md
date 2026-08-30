# Phase 1 — Domain types, bounds, chunking, text extraction

Read [`general_plan_instructions.md`](general_plan_instructions.md) first.

**Goal:** everything a document *is*, with no database and no network. Pure functions with real unit
tests, so the later phases have nothing to argue about. Nothing in this phase is reachable from a
route yet.

**Landable on its own:** yes — new module, new adapter, no call sites.

---

## 1. `src/domain/entities/memory_document.rs` (new)

Modelled directly on `src/domain/entities/upload.rs`: the format is decided **from the magic bytes**,
never from the client's `Content-Type`, and one `parse` returns a refusal worded for the person who
picked the file. Register with `pub mod memory_document;` in `src/domain/entities/mod.rs`.

### `DocumentFormat`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentFormat { Text, Markdown, Csv, Json, Html, Pdf, Docx }
```

- `detect(bytes: &[u8], filename: &str) -> Option<Self>` — binary formats by signature, text formats
  by extension **and** UTF-8 validity:
  - `%PDF-` → `Pdf`
  - `PK\x03\x04` **and** the zip central directory names a `word/document.xml` entry → `Docx`
    (a bare zip that is not a DOCX is `None`, not a refusal-by-crash later)
  - otherwise the bytes must be valid UTF-8, and the extension picks
    `md|markdown → Markdown`, `csv → Csv`, `json → Json`, `htm|html → Html`, `txt|text|log → Text`
  - anything else → `None`
- `mime(self) -> &'static str`, `extension(self) -> &'static str`
- `pub const ACCEPT_ATTRIBUTE: &'static str` — what the file picker offers, so the browser filters
  before the bytes are sent.

The extension is a *hint that narrows among text formats*; it can never promote a non-UTF-8 blob or
override a binary signature. That is the `format_comes_from_the_bytes_not_the_extension` rule
(`upload.rs:129`) applied one type wider.

### `MemoryDocumentUpload`

```rust
pub struct MemoryDocumentUpload { format: DocumentFormat, filename: String, bytes: Vec<u8> }

impl MemoryDocumentUpload {
    pub fn parse(filename: &str, bytes: Vec<u8>) -> Result<Self, String>;
    pub fn format(&self) -> DocumentFormat;
    pub fn filename(&self) -> &str;
    pub fn bytes(&self) -> &[u8];
    pub fn content_sha256(&self) -> String;   // sha2, hex — the storage key and the dedup key
}
```

`parse` refuses, in this order, with copy aimed at the uploader:

| Condition | Message |
|---|---|
| empty | `"Choose a file to upload."` |
| over `MAX_MEMORY_DOCUMENT_BYTES` | `"That file is {:.1} MB. Documents have to be under {} MB."` |
| `DocumentFormat::detect` is `None` | `"That file is not a text, Markdown, CSV, JSON, HTML, PDF or Word document."` |

Sanitize `filename` here too — strip any path component and control characters, cap at
`MAX_MEMORY_DOCUMENT_TITLE_CHARS`. It reaches a `Content-Disposition` header in phase 5.

### Scope, status, chunk ids

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryDocumentScope { Company, Agent(Uuid) }

impl MemoryDocumentScope {
    /// MUST agree with `resolve_scopes` in memory.rs:338 — this is the whole design.
    pub fn collection(self) -> String;      // "company" | "agent_{uuid}"
    pub fn as_str(self) -> &'static str;    // "company" | "agent"  (the DB discriminator)
    pub fn label(self) -> &'static str;     // "Company" | "Agent"
    pub fn memory_scope(self) -> MemoryScope;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MemoryDocumentIngestStatus { #[default] Pending, Ingesting, Ready, Failed }
// as_str() -> the DB value; label() -> "Waiting" | "Adding to memory" | "In memory" | "Failed"

pub enum MemoryDocumentOperation { Ingest, Forget }   // as_str() for the DB discriminator
```

**A test must assert `MemoryDocumentScope::collection` equals the collection `resolve_scopes`
produces** for the same scope. Two functions building the same string is exactly the drift
`src/AGENTS.md` "One decision, one place" warns about; if the assertion is awkward to write, make
`resolve_scopes` call this instead.

```rust
/// Stable across retries, and derivable from `(document_id, chunk_count)` alone — so the forget
/// path can rebuild every id after the document row is gone. Zero-padded so ids sort in order.
pub fn document_chunk_id(document_id: Uuid, index: usize) -> String {
    format!("doc_{}_{index:04}", document_id.simple())
}
```

Charset check: `[A-Za-z0-9._~-]`, so it passes `validate_identifier` (`adapters/memory/http.rs:156`)
and HydraDB's "id must not contain a comma" constraint. Assert both in a test.

### `chunk_document_text`

```rust
pub fn chunk_document_text(text: &str) -> Vec<String>;
```

Pure, no allocation surprises, Unicode-scalar-safe. Split preferentially on a blank line, then a
sentence boundary, then a hard character boundary; each chunk at most
`MAX_MEMORY_DOCUMENT_CHUNK_CHARS`, consecutive chunks overlapping by
`MEMORY_DOCUMENT_CHUNK_OVERLAP_CHARS` so a fact spanning a boundary survives; stop at
`MAX_MEMORY_DOCUMENT_CHUNKS` and drop the rest (the caller has already truncated the text to
`MAX_MEMORY_DOCUMENT_TEXT_CHARS`, so hitting the chunk cap means something is pathological).

Empty or whitespace-only input returns an empty `Vec` — the caller turns that into the refusal
`"That document has no readable text."`.

### `MemoryDocument` and `LeasedDocumentJob`

`MemoryDocument` mirrors the `memory_documents` row (phase 2) one-for-one.
`LeasedDocumentJob` mirrors `LeasedCleanupJob` (`memory.rs:165`) plus `document_id`, `company_id`,
`collection`, `operation`, `chunk_count`, `next_chunk_index`.

**Both need `#[serde(default)]` discipline if they ever reach a durable payload.** They do not today
— nothing serializes them into `background_tasks.payload` — so plain derives are fine. Say so in a
comment, because rule 2 of `src/adapters/persistence/AGENTS.md` is the one the compiler cannot find.

### Bounds

Add the const block from the general instructions to the top of `src/domain/entities/memory.rs`,
beside the existing `MAX_MEMORY_*` group, not to the new file — one place holds memory's bounds.

---

## 2. The extraction port — `src/application/services/document_extractor.rs` (new)

```rust
pub trait DocumentTextExtractor: Send + Sync {
    /// Plain text, or a refusal worded for the person who picked the file.
    fn extract(&self, upload: &MemoryDocumentUpload) -> Result<String, String>;
}
```

**Synchronous on purpose.** A non-`async fn` contributes no future and no stack frame
(`src/AGENTS.md`, "Keep `async fn` chains shallow" — the reason `build_agent` went 292 KiB → 174
KiB). The worker calls it inside `spawn_blocking` in phase 4, so a 400-page PDF cannot stall the
runtime. Put that reasoning in the doc comment or someone will helpfully make it `async`.

Application code must not import `pdf_extract`, `zip` or `quick-xml`, which is why the port is here
and the crates live under `src/adapters/`.

---

## 3. The extraction adapter — `src/adapters/documents/` (new)

```
src/adapters/documents/mod.rs      FileDocumentExtractor, the `impl DocumentTextExtractor`, dispatch
src/adapters/documents/pdf.rs      PDF
src/adapters/documents/docx.rs     DOCX
src/adapters/documents/tests.rs    #[cfg(test)] #[path = "tests.rs"] mod tests;
```

Register with `pub mod documents;` in `src/adapters/mod.rs`.

Dispatch in `mod.rs`, one arm per `DocumentFormat`:

| Format | Extraction |
|---|---|
| `Text`, `Csv`, `Json`, `Markdown` | UTF-8 already validated by `detect`; normalise CRLF/CR to `\n`, strip a BOM, collapse runs of 3+ blank lines |
| `Html` | `htmd` (already in `Cargo.toml`) to Markdown, then the text normalisation above |
| `Pdf` | `pdf.rs` |
| `Docx` | `docx.rs` |

After extraction, **every path** truncates through `truncate_memory_text(&text,
MAX_MEMORY_DOCUMENT_TEXT_CHARS)` and reports truncation to the caller, the way the rest of the memory
path does.

### `pdf.rs`

A PDF text-extraction crate — add with `cargo add`, commit `Cargo.lock`. Requirements:

- Refuse past `MAX_PDF_PAGES` **before** extracting, not after.
- A malformed or encrypted PDF is `Err("That PDF could not be read (it may be scanned, encrypted or
  damaged).")`, never a panic. If the crate can panic on hostile input, run it inside
  `std::panic::catch_unwind` and convert — a user-supplied file must not take the worker down.
- A PDF that is pure scanned images extracts to empty; that becomes the "no readable text" refusal.
  We are not adding OCR.

### `docx.rs`

`zip` (`default-features = false, features = ["deflate"]`) + `quick-xml`, both added with
`cargo add`. Open `word/document.xml` and stream it: `<w:p>` ends a paragraph (emit `\n`),
`<w:tab/>` emits `\t`, `<w:br/>` emits `\n`, text events inside `<w:t>` accumulate. Ignore
everything else — no styles, no headers, no comments.

**Zip-bomb guard, and it is not optional:** cap the *decompressed* bytes read at
`MAX_DOCX_DECOMPRESSED_BYTES` by reading through a `take()`-limited reader rather than trusting the
entry's declared size, and refuse if the limit is hit. A 4 KB `.docx` can declare a 4 GB member.

Prefer this over a full DOCX crate: it is ~80 lines, the surface it parses is tiny, and both crates
are ones we can bound ourselves.

---

## 4. Tests

Inline `#[cfg(test)] mod tests` for the domain module; `#[path = "tests.rs"]` for the adapter. Test
names are full sentences, as the codebase does.

**`memory_document.rs`**

- `format_comes_from_the_bytes_not_the_extension` — a `.txt` whose bytes start `%PDF-` is `Pdf`; a
  `.pdf` holding UTF-8 prose is not `Pdf`; a bare zip without `word/document.xml` is `None`; a
  non-UTF-8 blob named `.txt` is `None`.
- `parse_refuses_empty_oversized_and_unknown` — including the exact "10 MB" text in the message.
- `a_filename_never_escapes_its_own_field` — path components, `..`, control characters, a
  200+-character name, and a name holding `"` all come back safe.
- `the_collection_matches_what_resolve_scopes_builds` — Company and Agent, both directions.
- `a_chunk_id_is_stable_ordered_and_a_valid_identifier` — same inputs give the same id, ids sort by
  index, every id passes the `[A-Za-z0-9._~-]` charset and contains no comma.
- `chunking_respects_the_size_overlap_and_count_bounds` — no chunk over the char cap; consecutive
  chunks share the overlap; a pathological single-token 1 MB input still terminates at
  `MAX_MEMORY_DOCUMENT_CHUNKS`.
- `chunking_never_splits_a_unicode_scalar` — a document of `"🦀"` repeated; every chunk is valid
  UTF-8 and round-trips.
- `chunking_of_empty_or_whitespace_input_is_empty`.

**`adapters/documents/tests.rs`**

- `each_text_format_normalises_newlines_and_strips_a_bom`.
- `html_becomes_markdown_without_script_or_style_text`.
- `a_truncated_pdf_is_a_refusal_not_a_panic` — feed `%PDF-1.4` plus 200 random bytes.
- `a_pdf_over_the_page_cap_is_refused_before_extraction`.
- `a_docx_yields_its_paragraphs_in_order` — build the zip in the test rather than committing a
  binary fixture.
- `a_zip_bomb_docx_is_refused_at_the_decompression_bound` — a member declaring far more than
  `MAX_DOCX_DECOMPRESSED_BYTES`; assert the refusal and that memory did not balloon.
- `a_zip_without_word_document_xml_is_refused`.
- `extracted_text_over_the_char_bound_is_truncated_and_reported`.

---

## Done when

- `cargo fmt --check` and `cargo clippy` are clean.
- `ALLOW_MISSING_DATABASE_URL=1 cargo test memory_document documents` is green — this phase needs no
  database.
- `Cargo.lock` is committed with the three new crates, and no application-layer file imports them.
- `./scripts/stack-budget.sh` is unchanged (nothing async was added).
