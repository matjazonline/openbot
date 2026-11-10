//! Context packing (D8): deterministic greedy selection of whole items under both a token and a
//! byte budget. Pure: callers pass each candidate's exact serialized size, in rank order.
use super::access_level::AccessLevel;
use chrono::{DateTime, Utc};

/// The pack's fixed token overhead, counted once when the pack holds at least one item.
pub const PACK_OVERHEAD_TOKENS: u64 = 64;
/// Tokens per serialized byte: OpenBot's provider-independent upper bound (`2 × JSON bytes`).
pub const TOKENS_PER_BYTE: u64 = 2;
pub const DEFAULT_CONTEXT_TOKENS: u32 = 8_192;
pub const DEFAULT_CONTEXT_BYTES: u32 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub max_tokens: u64,
    pub max_bytes: u64,
}

/// One ranked candidate as packing sees it: its exact serialized size, the highest level of
/// what it would expose (its own level and every provenance target it carries) and its expiry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    pub bytes: u64,
    pub exposed_level: AccessLevel,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Indexes of the included candidates, in rank order.
    pub included: Vec<usize>,
    /// Candidates skipped because they did not fit; a count only.
    pub omitted: u32,
    pub bytes: u64,
    /// `64 + 2 × bytes`, or 0 for an empty pack.
    pub tokens: u64,
    /// The highest level of the included items only; 0 for an empty pack.
    pub exposed_level: AccessLevel,
    /// The earliest expiry among the included items.
    pub valid_until: Option<DateTime<Utc>>,
}

/// Tokens of a pack whose items serialize to `bytes` in total.
pub const fn pack_tokens(items: usize, bytes: u64) -> u64 {
    if items == 0 {
        0
    } else {
        PACK_OVERHEAD_TOKENS + TOKENS_PER_BYTE * bytes
    }
}

/// Greedy whole-item inclusion in rank order: an item that does not fit the remaining token or
/// byte budget is skipped and the scan continues (D8). No item is ever cut.
pub fn select(candidates: &[Candidate], budget: Budget) -> Selection {
    let mut selection = Selection {
        included: Vec::new(),
        omitted: 0,
        bytes: 0,
        tokens: 0,
        exposed_level: AccessLevel::PUBLIC,
        valid_until: None,
    };
    for (index, candidate) in candidates.iter().enumerate() {
        let bytes = selection.bytes.saturating_add(candidate.bytes);
        let tokens = pack_tokens(selection.included.len() + 1, bytes);
        if bytes > budget.max_bytes || tokens > budget.max_tokens {
            selection.omitted += 1;
            continue;
        }
        selection.included.push(index);
        selection.bytes = bytes;
        selection.tokens = tokens;
        selection.exposed_level = selection.exposed_level.max(candidate.exposed_level);
        selection.valid_until = match (selection.valid_until, candidate.expires_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
    selection
}

#[cfg(test)]
#[path = "packing_tests.rs"]
mod tests;
