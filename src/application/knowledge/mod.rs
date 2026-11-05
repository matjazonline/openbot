//! Synchronous, bounded library indexing; provider and persistence types stay behind ports.
mod admission;
mod chunking;
mod service;
mod types;
mod vectors;
pub use admission::UploadAdmissionRefused;
pub use chunking::{KnowledgeChunk, chunk_document};
pub use service::KnowledgeLibrary;
pub use types::*;
pub use vectors::normalize_embedding;

pub const MAX_CHUNKS_PER_FILE: usize = 4096;
pub const MAX_CHUNKS_PER_REQUEST: usize = 8192;
pub const MAX_HEADING_BYTES: usize = 2048;
pub const MAX_HEADING_TOKENS: usize = 128;
pub const TARGET_CHUNK_TOKENS: usize = 400;
pub const MAX_EMBEDDING_TOKENS: usize = 512;
pub const EMBEDDING_BATCH_SIZE: usize = 4;
