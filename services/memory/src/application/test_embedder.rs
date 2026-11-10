//! Deterministic stand-in for the local model (tests only). Tokens: one per whitespace word or
//! CJK character, plus 2 specials. Vectors: a bag of hashed words, with a small synonym table so
//! paraphrases land on the same dimensions. Modes simulate failures and invalid vectors.
use super::{
    embedding::EmbeddingPool,
    ports::{EmbedError, Embedder},
};
use crate::domain::retrieval::{DIMENSIONS, PASSAGE_PREFIX, QUERY_PREFIX};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

pub const FAKE_MODEL: &str = "test/fake-embedder@1/fp32/mean/384";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeMode {
    Ok,
    Fail,
    Nan,
    Zero,
}

pub struct FakeEmbedder {
    id: String,
    pub mode: Mutex<FakeMode>,
    pub calls: AtomicUsize,
    /// Each embed call sleeps this long (to hold a slot).
    pub delay: std::time::Duration,
}

impl FakeEmbedder {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            mode: Mutex::new(FakeMode::Ok),
            calls: AtomicUsize::new(0),
            delay: std::time::Duration::ZERO,
        }
    }
    pub fn set(&self, mode: FakeMode) {
        *self.mode.lock().unwrap() = mode;
    }
}

const SYNONYMS: &[(&str, &str)] = &[
    ("automobile", "car"),
    ("vehicle", "car"),
    ("repair", "fix"),
    ("mend", "fix"),
    ("repaired", "fix"),
    ("fixed", "fix"),
    ("bill", "invoice"),
    ("invoices", "invoice"),
    ("payment", "pay"),
    ("paid", "pay"),
];

fn words(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        if word.is_empty() {
            continue;
        }
        if word.chars().all(|c| (c as u32) >= 0x3000) {
            found.extend(word.chars().map(|c| c.to_string()));
            continue;
        }
        let lower = word.to_lowercase();
        let canonical = SYNONYMS
            .iter()
            .find(|(from, _)| *from == lower)
            .map_or(lower.clone(), |(_, to)| (*to).to_owned());
        found.push(canonical);
    }
    found
}

/// The fake's vector for a text, without the e5 prefix: hashed words, plus one constant
/// dimension so no input is the zero vector.
pub fn fake_vector(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0f32; DIMENSIONS];
    vector[DIMENSIONS - 1] = 0.05;
    for word in words(text) {
        let digest = Sha256::digest(word.as_bytes());
        let slot = usize::from(u16::from_be_bytes([digest[0], digest[1]])) % (DIMENSIONS - 1);
        vector[slot] += 1.0;
    }
    vector
}

impl Embedder for FakeEmbedder {
    fn model_id(&self) -> &str {
        &self.id
    }
    fn count_tokens(&self, input: &str) -> Result<usize, EmbedError> {
        let words = input.split_whitespace().count();
        let cjk = input.chars().filter(|c| (*c as u32) >= 0x3000).count();
        Ok(words + cjk + 2)
    }
    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }
        let mode = *self.mode.lock().unwrap();
        inputs
            .iter()
            .map(|input| {
                let text = input
                    .strip_prefix(QUERY_PREFIX)
                    .or_else(|| input.strip_prefix(PASSAGE_PREFIX))
                    .unwrap_or(input);
                match mode {
                    FakeMode::Ok => Ok(fake_vector(text)),
                    FakeMode::Fail => Err(EmbedError::Unavailable),
                    FakeMode::Nan => Ok(vec![f32::NAN; DIMENSIONS]),
                    FakeMode::Zero => Ok(vec![0.0; DIMENSIONS]),
                }
            })
            .collect()
    }
}

/// A pool of two fake sessions sharing one fake (so a test can switch its mode).
pub fn fake_pool_with(fake: Arc<FakeEmbedder>) -> Arc<EmbeddingPool> {
    Arc::new(EmbeddingPool::new(fake.clone(), fake).expect("same model"))
}

pub fn fake_pool() -> Arc<EmbeddingPool> {
    fake_pool_with(Arc::new(FakeEmbedder::new(FAKE_MODEL)))
}
