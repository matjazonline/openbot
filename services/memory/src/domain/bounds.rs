//! Request bounds (decisions D6). These are code maxima; configuration may only lower them. The
//! wire types publish them in their schemas (OpenAPI and MCP `inputSchema`) through schemars
//! attributes, and the use cases enforce them. JSON Schema `maxLength` counts characters; the
//! byte limits that also apply are stated in the field descriptions and enforced by the use
//! cases and the database CHECKs.

/// External ids and idempotency keys: 1-128 bytes of `[A-Za-z0-9._:-]`.
pub const ID_PATTERN: &str = "^[A-Za-z0-9._:-]{1,128}$";
pub const MAX_LABEL_CHARS: usize = 128;
pub const MAX_GRANTS_PER_CREDENTIAL: usize = 64;
pub const MAX_NAMESPACES_PER_REQUEST: usize = 16;
/// Title: at most 256 characters and 1 KiB.
pub const MAX_TITLE_CHARS: usize = 256;
/// Body: at most 16,384 characters and 64 KiB.
pub const MAX_BODY_CHARS: usize = 16_384;
pub const MAX_SAVE_ITEMS: usize = 256;
/// Service sources and external sources named by one producing context, each.
pub const MAX_CONTEXT_SOURCES: usize = 64;
/// Source references and external references: at most 512 bytes.
pub const MAX_REFERENCE_BYTES: usize = 512;
/// Source revisions and event ids: at most 128 bytes.
pub const MAX_REVISION_BYTES: usize = 128;
pub const MAX_QUERY_CHARS: usize = 1_024;
pub const MAX_SEARCH_LIMIT: u32 = 100;
pub const MAX_TIMELINE_SIDE: u32 = 20;
pub const MAX_FETCH_IDS: usize = 64;
pub const MAX_CONTEXT_TOKENS: u32 = 65_536;
pub const MAX_CONTEXT_BYTES: u32 = 256 * 1024;
/// Context candidates, and so the items one `context:validate` can check.
pub const MAX_CONTEXT_ITEMS: usize = 200;
/// Source content: at most 256 KiB.
pub const MAX_SOURCE_CONTENT_CHARS: usize = 256 * 1024;
/// Generation windows (D9): at most 512 events and 512 KiB; `sources:record` takes as many.
pub const MAX_WINDOW_EVENTS: usize = 512;
pub const MAX_OUTPUT_TOKENS: u32 = 8_192;
