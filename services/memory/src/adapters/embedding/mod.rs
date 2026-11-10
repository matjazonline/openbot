//! The pinned local embedding model (D7): multilingual-e5-small, fp32, mean pooling, 384-d,
//! loaded from manifest-validated files through the manifest-pinned ONNX Runtime. Two sessions
//! (the pool's query slot and shared slot), each one intra-op thread. Adapted from OpenBot's
//! embedding adapter; nothing of OpenBot's is imported or configured.
mod artifacts;

use crate::{
    application::{
        embedding::EmbeddingPool,
        ports::{EmbedError, Embedder},
    },
    domain::retrieval::{
        DIMENSIONS, EMBED_BATCH, MAX_INPUT_TOKENS, chunk, normalize, passage_input, query_input,
    },
};
use anyhow::{Context, bail};
use fastembed::{
    InitOptionsUserDefined, Pooling, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel,
};
use serde::Serialize;
use std::{
    path::Path,
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};
use tokenizers::Tokenizer;

/// The model's own maximum sequence length; inputs are held to `MAX_INPUT_TOKENS` before this.
const MODEL_MAX_TOKENS: usize = 512;
/// Intra-op threads per session; two sessions give the pool's two embedding threads.
const SESSION_THREADS: usize = 1;

pub struct LocalEmbedder {
    id: String,
    model: Mutex<TextEmbedding>,
    counter: Tokenizer,
}

impl Embedder for LocalEmbedder {
    fn model_id(&self) -> &str {
        &self.id
    }

    fn count_tokens(&self, input: &str) -> Result<usize, EmbedError> {
        self.counter
            .encode(input, true)
            .map(|encoding| encoding.len())
            .map_err(|_| EmbedError::Unavailable)
    }

    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if inputs.is_empty() || inputs.len() > EMBED_BATCH {
            return Err(EmbedError::Unavailable);
        }
        for input in inputs {
            // The session would truncate at 512; nothing over the hard limit reaches it.
            if self.count_tokens(input)? > MAX_INPUT_TOKENS {
                return Err(EmbedError::InputTooLong);
            }
        }
        let mut model = self.model.lock().map_err(|_| EmbedError::Unavailable)?;
        let vectors = model
            .embed(inputs, Some(EMBED_BATCH))
            .map_err(|_| EmbedError::Unavailable)?;
        if vectors.len() != inputs.len() {
            return Err(EmbedError::Unavailable);
        }
        Ok(vectors)
    }
}

/// ONNX Runtime is initialized once per process, from the validated runtime library.
fn init_runtime(path: &Path) -> anyhow::Result<()> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| {
        let committed = ort::init_from(path)
            .map_err(|error| format!("cannot load the pinned ONNX Runtime: {error}"))?
            .with_telemetry(false)
            .commit();
        if committed {
            Ok(())
        } else {
            Err("ONNX Runtime was initialized before the pinned runtime".into())
        }
    })
    .clone()
    .map_err(anyhow::Error::msg)
}

fn session(id: &str, onnx: Vec<u8>, tokenizer: TokenizerFiles) -> anyhow::Result<LocalEmbedder> {
    let input = UserDefinedEmbeddingModel::new(onnx, tokenizer).with_pooling(Pooling::Mean);
    let model = TextEmbedding::try_new_from_user_defined(
        input,
        InitOptionsUserDefined::new()
            .with_max_length(MODEL_MAX_TOKENS)
            .with_intra_threads(SESSION_THREADS),
    )
    .context("cannot load the embedding model")?;
    // Count with the model's own tokenizer, without padding or truncation.
    let mut counter = model.tokenizer.clone();
    counter.with_padding(None);
    counter
        .with_truncation(None)
        .map_err(|_| anyhow::anyhow!("cannot configure model token counting"))?;
    Ok(LocalEmbedder {
        id: id.to_owned(),
        model: Mutex::new(model),
        counter,
    })
}

fn self_check(model: &LocalEmbedder) -> anyhow::Result<()> {
    let mut vectors = model
        .embed(&[query_input("memory service startup self-check")])
        .map_err(|error| anyhow::anyhow!("the embedding self-check failed: {error}"))?;
    let raw = vectors
        .pop()
        .context("the embedding self-check returned nothing")?;
    if raw.len() != DIMENSIONS {
        bail!(
            "the embedding model returned {} dimensions, not {DIMENSIONS}",
            raw.len()
        );
    }
    normalize(raw)
        .map_err(|_| anyhow::anyhow!("the embedding self-check returned an invalid vector"))?;
    Ok(())
}

/// The two sessions of the embedding pool, from one validated model directory.
pub struct LoadedModel {
    pub query: Arc<LocalEmbedder>,
    pub shared: Arc<LocalEmbedder>,
    pub load_ms: u128,
}

impl LoadedModel {
    pub fn pool(&self) -> anyhow::Result<EmbeddingPool> {
        EmbeddingPool::new(self.query.clone(), self.shared.clone()).map_err(anyhow::Error::msg)
    }
}

/// Validate the artifacts, load both sessions and self-check them. Blocking; seconds.
pub fn load(directory: &Path) -> anyhow::Result<LoadedModel> {
    let started = Instant::now();
    let checked = artifacts::validate(directory)?;
    init_runtime(&checked.runtime)?;
    let mut files = checked.files;
    let mut take = |path: &str| {
        files
            .remove(path)
            .with_context(|| format!("the manifest does not list {path}"))
    };
    let tokenizer = TokenizerFiles {
        tokenizer_file: take("tokenizer.json")?,
        config_file: take("config.json")?,
        special_tokens_map_file: take("special_tokens_map.json")?,
        tokenizer_config_file: take("tokenizer_config.json")?,
    };
    // Both sessions load the bytes that were verified; nothing is re-read after the check.
    let onnx = take("onnx/model.onnx")?;
    let query = session(&checked.model_id, onnx.clone(), tokenizer.clone())?;
    let shared = session(&checked.model_id, onnx, tokenizer)?;
    self_check(&query)?;
    self_check(&shared)?;
    Ok(LoadedModel {
        query: Arc::new(query),
        shared: Arc::new(shared),
        load_ms: started.elapsed().as_millis(),
    })
}

/// `memory-service model-check`: manifest validation, both sessions, the self-check and a few
/// behavioural probes, with no database. The report records the resource budget.
#[derive(Debug, Serialize)]
pub struct ModelCheck {
    pub model_id: String,
    pub dimensions: usize,
    pub sessions: usize,
    pub threads_per_session: usize,
    pub load_ms: u128,
    pub probe_ms: u128,
    pub peak_rss_bytes: u64,
    pub translation_similarity: f64,
    pub unrelated_similarity: f64,
    pub long_note_parts: usize,
    pub largest_part_tokens: usize,
    pub oversized_input_refused: bool,
}

fn dot(left: &[f32], right: &[f32]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum()
}

pub fn model_check(directory: &Path) -> anyhow::Result<ModelCheck> {
    let model = load(directory)?;
    let probe = Instant::now();
    let embed = |text: String| -> anyhow::Result<Vec<f32>> {
        let raw = model
            .shared
            .embed(&[text])
            .map_err(|error| anyhow::anyhow!("{error}"))?
            .pop()
            .context("no vector")?;
        normalize(raw).map_err(|_| anyhow::anyhow!("invalid vector"))
    };
    let query = embed(query_input("How long can I return a product for a refund?"))?;
    let whole = |title: &str, text: &str| passage_input(title, text, 0..text.len());
    let translation = embed(whole(
        "Rückgabe",
        "Kunden können Produkte innerhalb von dreißig Tagen zurückgeben.",
    ))?;
    let unrelated = embed(whole("Astronomy", "The planet Jupiter has many moons."))?;
    let (translation_similarity, unrelated_similarity) =
        (dot(&query, &translation), dot(&query, &unrelated));
    if translation_similarity < unrelated_similarity + 0.02 {
        bail!("the embedding model failed its multilingual semantic probe");
    }
    let body = "Rückgaben innerhalb von dreißig Tagen. Refunds within thirty days. 返品は三十日以内です。\n\n"
        .repeat(60);
    let count = |text: &str| model.shared.count_tokens(text).map_err(|_| ());
    let parts = chunk(&body, |range| {
        count(&passage_input("Returns", &body, range))
            .map_err(|_| crate::domain::retrieval::ChunkError::Counter)
    })
    .map_err(|error| anyhow::anyhow!("chunking failed: {error}"))?;
    let mut largest_part_tokens = 0;
    for range in &parts {
        let tokens = count(&passage_input("Returns", &body, range.clone()))
            .map_err(|_| anyhow::anyhow!("token counting failed"))?;
        largest_part_tokens = largest_part_tokens.max(tokens);
    }
    if parts.len() < 2 || largest_part_tokens > MAX_INPUT_TOKENS {
        bail!("tokenizer-aware chunking failed its boundary probe");
    }
    let oversized_input_refused = matches!(
        model.query.embed(&[query_input(&"hello ".repeat(1024))]),
        Err(EmbedError::InputTooLong)
    );
    if !oversized_input_refused {
        bail!("the embedding model did not refuse an input over the token limit");
    }
    Ok(ModelCheck {
        model_id: model.query.model_id().to_owned(),
        dimensions: DIMENSIONS,
        sessions: 2,
        threads_per_session: SESSION_THREADS,
        load_ms: model.load_ms,
        probe_ms: probe.elapsed().as_millis(),
        peak_rss_bytes: peak_rss_bytes()?,
        translation_similarity,
        unrelated_similarity,
        long_note_parts: parts.len(),
        largest_part_tokens,
        oversized_input_refused,
    })
}

fn peak_rss_bytes() -> anyhow::Result<u64> {
    // getrusage writes a plain C record; read it only after a successful return.
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        bail!("cannot measure peak resident memory");
    }
    let usage = unsafe { usage.assume_init() };
    #[cfg(target_os = "linux")]
    let multiplier = 1024;
    #[cfg(not(target_os = "linux"))]
    let multiplier = 1;
    Ok(u64::try_from(usage.ru_maxrss)? * multiplier)
}
