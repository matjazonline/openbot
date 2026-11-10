//! Pure retrieval rules (D7): embedding conventions, bounded chunking over the model's own token
//! counter, vector validation, the vector-leg plan and reciprocal rank fusion. No I/O.
use chrono::{DateTime, Utc};
use std::ops::Range;
use uuid::Uuid;

/// The vector width of `observation_parts.embedding` and of the pinned model.
pub const DIMENSIONS: usize = 384;
/// e5 conventions: queries and passages are embedded with these prefixes.
pub const QUERY_PREFIX: &str = "query: ";
pub const PASSAGE_PREFIX: &str = "passage: ";
/// Parts aim at this many tokens (prefix and special tokens included).
pub const TARGET_PART_TOKENS: usize = 320;
/// Hard limit per model input, prefix and special tokens included (the model maximum is 512).
pub const MAX_INPUT_TOKENS: usize = 500;
pub const MAX_PARTS: usize = 128;
/// Texts per model call; the indexer embeds an observation's parts in batches of this size.
pub const EMBED_BATCH: usize = 8;
/// The keyword leg's all-words clause uses at most this many lexemes of the query.
pub const MAX_QUERY_LEXEMES: i32 = 32;
/// At most this many identifier-like query tokens become phrase clauses.
pub const MAX_IDENTIFIER_TOKENS: usize = 16;
pub const RRF_K: f64 = 60.0;
pub const MAX_LEG_CANDIDATES: usize = 200;
/// `hnsw.max_scan_tuples` for the filtered iterative scan.
pub const MAX_SCAN_TUPLES: i64 = 20_000;
/// The exact scan orders at most this many eligible parts; above it the leg is `partial`.
pub const EXACT_SCAN_CAP: i64 = 100_000;
/// Snippets are at most this many bytes.
pub const SNIPPET_BYTES: usize = 320;
/// `ts_headline` runs on at most this many final rows.
pub const MAX_HEADLINE_ROWS: usize = 100;
/// Character cuts look at most this far ahead (500 tokens of 16 bytes each).
const SPLIT_WINDOW_BYTES: usize = 8 * 1024;
/// L2 norm tolerance of a normalized vector.
pub const NORM_TOLERANCE: f64 = 0.001;

/// The model input for one part: the title gives every part its topic; an empty range is the
/// title alone (an observation with an empty body).
pub fn passage_input(title: &str, body: &str, range: Range<usize>) -> String {
    if range.is_empty() {
        format!("{PASSAGE_PREFIX}{title}")
    } else {
        format!("{PASSAGE_PREFIX}{title}\n\n{}", &body[range])
    }
}

pub fn query_input(query: &str) -> String {
    format!("{QUERY_PREFIX}{query}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ChunkError {
    #[error("the model token counter failed")]
    Counter,
    /// A part over the hard limit is a bug: indexing fails instead of truncating text.
    #[error("a part exceeds the model token limit")]
    PartOverLimit,
    #[error("the text needs more than 128 parts")]
    TooManyParts,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Unit {
    Paragraph,
    Sentence,
}

struct Packer<'a, F> {
    body: &'a str,
    cost: F,
    target: usize,
    parts: Vec<Range<usize>>,
    pending: Option<Range<usize>>,
}

impl<F: FnMut(Range<usize>) -> Result<usize, ChunkError>> Packer<'_, F> {
    fn push(&mut self, range: Range<usize>) -> Result<(), ChunkError> {
        if self.parts.len() == MAX_PARTS {
            return Err(ChunkError::TooManyParts);
        }
        if (self.cost)(range.clone())? > MAX_INPUT_TOKENS {
            return Err(ChunkError::PartOverLimit);
        }
        self.parts.push(range);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), ChunkError> {
        match self.pending.take() {
            Some(range) => self.push(range),
            None => Ok(()),
        }
    }

    fn unit(&mut self, range: Range<usize>, unit: Unit) -> Result<(), ChunkError> {
        let candidate = match &self.pending {
            Some(pending) => pending.start..range.end,
            None => range.clone(),
        };
        if (self.cost)(candidate.clone())? <= self.target {
            self.pending = Some(candidate);
            return Ok(());
        }
        if self.pending.is_some() {
            self.flush()?;
            return self.unit(range, unit);
        }
        match unit {
            Unit::Paragraph => {
                for sentence in sentences(self.body, range) {
                    self.unit(sentence, Unit::Sentence)?;
                }
                Ok(())
            }
            Unit::Sentence => self.split(range),
        }
    }

    /// A sentence over the target: cut at the longest character prefix within the target (or,
    /// when a very long title leaves no room, within the hard limit), repeatedly.
    fn split(&mut self, mut range: Range<usize>) -> Result<(), ChunkError> {
        loop {
            // Search a bounded window: no part needs more than SPLIT_WINDOW_BYTES of text, so
            // each cut costs a bounded number of bounded counts.
            let mut window = range.end.min(range.start + SPLIT_WINDOW_BYTES);
            while !self.body.is_char_boundary(window) {
                window -= 1;
            }
            let text = &self.body[range.start..window];
            let ends: Vec<usize> = text
                .char_indices()
                .skip(1)
                .map(|(at, _)| range.start + at)
                .chain(std::iter::once(window))
                .collect();
            let fit = match self.longest(range.start, &ends, self.target)? {
                Some(end) => end,
                None => self
                    .longest(range.start, &ends, MAX_INPUT_TOKENS)?
                    .ok_or(ChunkError::PartOverLimit)?,
            };
            if fit == range.end {
                // The remainder fits: keep it pending so following sentences can join it.
                self.pending = Some(range);
                return Ok(());
            }
            self.push(range.start..fit)?;
            range = trim(self.body, fit..range.end);
            if range.is_empty() {
                return Ok(());
            }
        }
    }

    fn longest(
        &mut self,
        start: usize,
        ends: &[usize],
        limit: usize,
    ) -> Result<Option<usize>, ChunkError> {
        let (mut low, mut high, mut best) = (0usize, ends.len(), None);
        while low < high {
            let mid = low + (high - low) / 2;
            if (self.cost)(start..ends[mid])? <= limit {
                best = Some(ends[mid]);
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        Ok(best)
    }
}

fn trim(body: &str, range: Range<usize>) -> Range<usize> {
    let text = &body[range.clone()];
    let start = range.start + (text.len() - text.trim_start().len());
    let end = range.end - (text.len() - text.trim_end().len());
    if start >= end {
        start..start
    } else {
        start..end
    }
}

fn paragraphs(body: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let (mut start, mut offset) = (0, 0);
    for line in body.split_inclusive('\n') {
        if line.trim().is_empty() {
            let range = trim(body, start..offset);
            if !range.is_empty() {
                found.push(range);
            }
            start = offset + line.len();
        }
        offset += line.len();
    }
    let range = trim(body, start..body.len());
    if !range.is_empty() {
        found.push(range);
    }
    found
}

fn sentences(body: &str, range: Range<usize>) -> Vec<Range<usize>> {
    let text = &body[range.clone()];
    let mut found = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let end = at + c.len_utf8();
        let next_is_space = chars.peek().is_none_or(|(_, next)| next.is_whitespace());
        let closes = matches!(c, '。' | '！' | '？' | '\n')
            || (matches!(c, '.' | '!' | '?' | ';') && next_is_space);
        if closes {
            let piece = trim(body, range.start + start..range.start + end);
            if !piece.is_empty() {
                found.push(piece);
            }
            start = end;
        }
    }
    let piece = trim(body, range.start + start..range.end);
    if !piece.is_empty() {
        found.push(piece);
    }
    found
}

/// Split `body` into at most 128 byte ranges whose model inputs (`cost` counts the complete
/// input for a range, prefix, title and special tokens included, with no truncation) stay
/// within the hard limit: paragraphs first, then sentences, then token-counted character cuts.
/// Parts do not overlap. A blank body is one title-only part `0..0`.
pub fn chunk(
    body: &str,
    mut cost: impl FnMut(Range<usize>) -> Result<usize, ChunkError>,
) -> Result<Vec<Range<usize>>, ChunkError> {
    let base = cost(0..0)?;
    if base > MAX_INPUT_TOKENS {
        return Err(ChunkError::PartOverLimit);
    }
    if body.trim().is_empty() {
        return Ok(std::iter::once(0..0).collect());
    }
    // A long title leaves less room for text: keep at least 192 tokens of body per part where
    // the hard limit allows, so a full-length body still fits in 128 parts.
    let target = (base + 192).clamp(TARGET_PART_TOKENS, MAX_INPUT_TOKENS);
    let mut packer = Packer {
        body,
        cost,
        target,
        parts: Vec::new(),
        pending: None,
    };
    for paragraph in paragraphs(body) {
        packer.unit(paragraph, Unit::Paragraph)?;
    }
    packer.flush()?;
    Ok(packer.parts)
}

/// Identifier-like tokens of the raw query, for the keyword leg's phrase clauses (D7): split on
/// whitespace, trimmed of surrounding punctuation, and kept when they hold a digit, one of
/// `_ . @ / : -` inside them (`ERR_CONN_RESET`, `foo.bar.baz`) or a capital right after a
/// lower-case letter (`PgStore`). Bare capitalised words are not identifiers: `IT`, or a query
/// typed in capitals, would otherwise become one-word phrases matching most rows (review
/// `p4-code-r2` F1a). Ordinary words and sentence starts yield none. At most 16, first seen
/// first, deduplicated.
pub fn identifier_tokens(query: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for raw in query.split_whitespace() {
        let token = raw.trim_matches(|c: char| {
            !(c.is_alphanumeric() || matches!(c, '_' | '@' | '/' | ':' | '.' | '-'))
        });
        let token = token.trim_end_matches(['.', ':', '-', '/']);
        if token.is_empty() || token.len() > 128 {
            continue;
        }
        let chars: Vec<char> = token.chars().collect();
        let internal_capital = chars
            .windows(2)
            .any(|pair| pair[0].is_lowercase() && pair[1].is_uppercase());
        let marked = token.chars().any(|c| c.is_ascii_digit())
            || token
                .char_indices()
                .any(|(at, c)| at > 0 && matches!(c, '_' | '.' | '@' | '/' | ':' | '-'));
        if (marked || internal_capital) && !found.iter().any(|seen| seen == token) {
            found.push(token.to_owned());
            if found.len() == MAX_IDENTIFIER_TOKENS {
                break;
            }
        }
    }
    found
}

/// At most `SNIPPET_BYTES` of a part's text, cut at a character boundary.
pub fn snippet(text: &str) -> String {
    let text = text.trim();
    if text.len() <= SNIPPET_BYTES {
        return text.to_owned();
    }
    let mut end = SNIPPET_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the model returned an invalid vector")]
pub struct InvalidVector;

/// Validate and L2-normalize a model output: exactly `DIMENSIONS` finite values, non-zero, and
/// a unit norm within `NORM_TOLERANCE` afterwards.
pub fn normalize(vector: Vec<f32>) -> Result<Vec<f32>, InvalidVector> {
    if vector.len() != DIMENSIONS || vector.iter().any(|value| !value.is_finite()) {
        return Err(InvalidVector);
    }
    let norm = vector
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm <= f64::EPSILON {
        return Err(InvalidVector);
    }
    let normalized: Vec<f32> = vector
        .iter()
        .map(|value| (f64::from(*value) / norm) as f32)
        .collect();
    let check = normalized
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>();
    if (check - 1.0).abs() > NORM_TOLERANCE || normalized.iter().any(|v| !v.is_finite()) {
        return Err(InvalidVector);
    }
    Ok(normalized)
}

/// The vector leg's bounded plan (D7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VectorPlan {
    pub leg_candidates: usize,
    pub nearest_parts: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorStep {
    /// Enough distinct observations.
    Complete,
    /// The scan filled its limit with parts of too few observations: ask for more parts.
    Retry(VectorPlan),
    /// The scan stopped short of its limit (exhausted, or stopped at `max_scan_tuples`): the
    /// result is inconclusive and the exact scan decides.
    Exact,
}

impl VectorPlan {
    pub fn new(limit: usize) -> Self {
        let leg_candidates = (4 * limit.max(1)).min(MAX_LEG_CANDIDATES);
        Self {
            leg_candidates,
            nearest_parts: 2 * leg_candidates,
        }
    }

    pub fn ef_search(&self) -> usize {
        self.nearest_parts.clamp(40, 1000)
    }

    pub fn max_parts(&self) -> usize {
        self.leg_candidates * MAX_PARTS
    }

    pub fn next(&self, observations: usize, parts: usize) -> VectorStep {
        if observations >= self.leg_candidates {
            VectorStep::Complete
        } else if parts == self.nearest_parts && self.nearest_parts < self.max_parts() {
            VectorStep::Retry(Self {
                nearest_parts: (self.nearest_parts * 4).min(self.max_parts()),
                ..*self
            })
        } else {
            VectorStep::Exact
        }
    }
}

/// One leg's ranked candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ranked {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
}

/// Reciprocal rank fusion: `Σ 1/(60 + rank)` over the legs an observation appears in (ranks
/// start at 1); ties by `occurred_at` descending, then id ascending.
pub fn fuse(keyword: &[Ranked], semantic: &[Ranked]) -> Vec<(Ranked, f64)> {
    let mut scored: Vec<(Ranked, f64)> = Vec::new();
    for leg in [keyword, semantic] {
        for (index, candidate) in leg.iter().enumerate() {
            let score = 1.0 / (RRF_K + (index + 1) as f64);
            match scored.iter_mut().find(|(seen, _)| seen.id == candidate.id) {
                Some((_, total)) => *total += score,
                None => scored.push((*candidate, score)),
            }
        }
    }
    scored.sort_by(|(left, a), (right, b)| {
        b.total_cmp(a)
            .then_with(|| right.occurred_at.cmp(&left.occurred_at))
            .then_with(|| left.id.cmp(&right.id))
    });
    scored
}

#[cfg(test)]
#[path = "retrieval_tests.rs"]
mod tests;
