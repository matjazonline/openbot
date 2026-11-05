//! Tenant-filtered candidates and pure, bounded hybrid ranking.
mod query;
mod ranking;
mod types;

pub use query::{ExactKnowledgeTerm, KnowledgeSearchQuery};
pub use ranking::rank_chunks;
pub use types::*;

pub const MIN_MEANING_SIMILARITY: f32 = 0.75;
