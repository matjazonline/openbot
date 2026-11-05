//! One verified local multilingual CPU model, shared across companies.
mod artifacts;
mod probe;
use crate::{
    app_error::{AppError, AppResult},
    application::knowledge::{
        EMBEDDING_BATCH_SIZE, MAX_EMBEDDING_TOKENS, TextEmbedder, normalize_embedding,
    },
    entities::value_objects::EmbeddingModelId,
};
use anyhow::{Context, bail};
use fastembed::{
    InitOptionsUserDefined, Pooling, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel,
};
pub use probe::model_check;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};
use tokenizers::Tokenizer;

pub struct LocalTextEmbedder {
    id: EmbeddingModelId,
    model: Mutex<TextEmbedding>,
    counter: Tokenizer,
}
pub fn model_directory_from_env() -> PathBuf {
    std::env::var_os("KNOWLEDGE_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/app/models/multilingual-e5-small"))
}
impl LocalTextEmbedder {
    pub fn load(directory: &Path) -> anyhow::Result<Self> {
        let checked = artifacts::validate(directory)?;
        let committed = ort::init_from(&checked.runtime)
            .context("Cannot initialize the baked knowledge runtime")?
            .with_telemetry(false)
            .commit();
        if !committed {
            bail!("Knowledge runtime must be initialized once, before any other model");
        }
        let read = |path: &str| {
            std::fs::read(directory.join(path)).with_context(|| {
                format!(
                    "Cannot read knowledge artifact {}",
                    directory.join(path).display()
                )
            })
        };
        let tokenizer = TokenizerFiles {
            tokenizer_file: read("tokenizer.json")?,
            config_file: read("config.json")?,
            special_tokens_map_file: read("special_tokens_map.json")?,
            tokenizer_config_file: read("tokenizer_config.json")?,
        };
        let input = UserDefinedEmbeddingModel::new(read("onnx/model.onnx")?, tokenizer)
            .with_pooling(Pooling::Mean);
        let model = TextEmbedding::try_new_from_user_defined(
            input,
            InitOptionsUserDefined::new()
                .with_max_length(MAX_EMBEDDING_TOKENS)
                .with_intra_threads(2),
        )
        .context("Cannot load the baked knowledge model")?;
        // Clone the actual configured tokenizer. Counting must not pad or truncate rejected input.
        let mut counter = model.tokenizer.clone();
        counter.with_padding(None);
        counter
            .with_truncation(None)
            .map_err(|_| anyhow::anyhow!("Cannot configure knowledge token counting"))?;
        let adapter = Self {
            id: checked.id.into(),
            model: Mutex::new(model),
            counter,
        };
        let vector = adapter.embed_query("Knowledge startup self-check")?;
        let norm = vector
            .iter()
            .map(|value| f64::from(*value).powi(2))
            .sum::<f64>();
        if (norm - 1.0).abs() > 0.001 {
            bail!("Knowledge model self-check returned an invalid norm");
        }
        Ok(adapter)
    }
    fn tokens(&self, input: &str) -> AppResult<usize> {
        self.counter
            .encode(input, true)
            .map(|encoding| encoding.len())
            .map_err(|_| unavailable())
    }
    fn embed(&self, inputs: Vec<String>) -> AppResult<Vec<[f32; 384]>> {
        if inputs.is_empty() || inputs.len() > EMBEDDING_BATCH_SIZE {
            return Err(AppError::BadRequest(
                "The embedding batch exceeds the local limit".into(),
            ));
        }
        for input in &inputs {
            if self.tokens(input)? > MAX_EMBEDDING_TOKENS {
                return Err(AppError::BadRequest(
                    "The text exceeds the local model token limit".into(),
                ));
            }
        }
        let count = inputs.len();
        let mut model = self.model.lock().map_err(|_| unavailable())?;
        let vectors = model
            .embed(inputs, Some(EMBEDDING_BATCH_SIZE))
            .map_err(|_| unavailable())?;
        if vectors.len() != count {
            return Err(unavailable());
        }
        vectors
            .into_iter()
            .map(|vector| normalize_embedding(vector.try_into().map_err(|_| unavailable())?))
            .collect()
    }
}
fn unavailable() -> AppError {
    AppError::ServiceUnavailable("The local embedding model could not process the text".into())
}
impl TextEmbedder for LocalTextEmbedder {
    fn model_id(&self) -> EmbeddingModelId {
        self.id.clone()
    }
    fn passage_token_count(&self, passage: &str) -> AppResult<usize> {
        self.tokens(&format!("passage: {passage}"))
    }
    fn embed_passages(&self, passages: &[&str]) -> AppResult<Vec<[f32; 384]>> {
        if passages.is_empty() || passages.len() > EMBEDDING_BATCH_SIZE {
            return Err(AppError::BadRequest(
                "The embedding batch exceeds the local limit".into(),
            ));
        }
        self.embed(
            passages
                .iter()
                .map(|passage| format!("passage: {passage}"))
                .collect(),
        )
    }
    fn embed_query(&self, query: &str) -> AppResult<[f32; 384]> {
        self.embed(vec![format!("query: {query}")])?
            .pop()
            .ok_or_else(unavailable)
    }
}
