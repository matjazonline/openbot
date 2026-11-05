//! One deterministic embedding fixture for native search and production-composition tests.
use super::*;
use crate::{app_error::AppResult, entities::value_objects::EmbeddingModelId};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
pub struct DeterministicTextEmbedder {
    queries: AtomicUsize,
}

impl DeterministicTextEmbedder {
    pub fn query_count(&self) -> usize {
        self.queries.load(Ordering::SeqCst)
    }
}

impl TextEmbedder for DeterministicTextEmbedder {
    fn model_id(&self) -> EmbeddingModelId {
        "test-model".into()
    }

    fn passage_token_count(&self, passage: &str) -> AppResult<usize> {
        Ok(passage.chars().count() + 3)
    }

    fn embed_passages(&self, passages: &[&str]) -> AppResult<Vec<[f32; 384]>> {
        Ok(vec![unit_vector(); passages.len()])
    }

    fn embed_query(&self, query: &str) -> AppResult<[f32; 384]> {
        self.queries.fetch_add(1, Ordering::SeqCst);
        if query.chars().count() + 3 > 512 {
            return Err(crate::app_error::AppError::BadRequest(
                "Test query token limit".into(),
            ));
        }
        Ok(unit_vector())
    }
}

fn unit_vector() -> [f32; 384] {
    let mut vector = [0.0; 384];
    vector[0] = 1.0;
    vector
}
