//! Explicit operational check with public fixtures; ordinary tests never load model weights.
use super::*;
use serde::Serialize;
use std::time::Instant;
#[path = "retrieval_probe.rs"]
mod retrieval;
#[derive(Serialize)]
pub struct ModelCheck {
    model_id: EmbeddingModelId,
    dimensions: usize,
    norm: f64,
    translation_score: f64,
    unrelated_score: f64,
    load_ms: u128,
    probe_ms: u128,
    peak_rss_bytes: u64,
    batch_size: usize,
    batch_tokens: usize,
    chunk_count: usize,
    largest_chunk_tokens: usize,
    oversized_query_and_batch_refused: bool,
    retrieval: retrieval::RetrievalCheck,
}
pub fn model_check(directory: &Path) -> anyhow::Result<ModelCheck> {
    let started = Instant::now();
    let model = LocalTextEmbedder::load(directory)?;
    let load_ms = started.elapsed().as_millis();
    let probe = Instant::now();
    let query = model.embed_query("How long can I return a product for a refund?")?;
    let passages=model.embed_passages(&["Kunden können Produkte innerhalb von dreißig Tagen zurückgeben und erhalten eine Rückerstattung.","The planet Jupiter has many moons and a large red storm."])?;
    let translation_score = dot(&query, &passages[0]);
    let unrelated_score = dot(&query, &passages[1]);
    if translation_score < unrelated_score + 0.02 {
        bail!("Knowledge model failed its multilingual semantic check");
    }
    let mut long = "warranty ".repeat(512);
    while model.passage_token_count(&long)? > MAX_EMBEDDING_TOKENS {
        long.pop();
    }
    let batch_tokens = model.passage_token_count(&long)?;
    if batch_tokens != MAX_EMBEDDING_TOKENS {
        bail!("Knowledge model probe did not exercise the full token limit");
    }
    let vectors = model.embed_passages(&[&long, &long, &long, &long])?;
    let chunk_check = check_chunks_and_bounds(&model)?;
    let retrieval = retrieval::check(&model)?;
    let norm = dot(&vectors[0], &vectors[0]);
    if (norm - 1.0).abs() > 0.001 {
        bail!("Knowledge model failed its batch norm check");
    }
    Ok(ModelCheck {
        model_id: model.model_id(),
        dimensions: 384,
        norm,
        translation_score,
        unrelated_score,
        load_ms,
        probe_ms: probe.elapsed().as_millis(),
        peak_rss_bytes: peak_rss_bytes()?,
        batch_size: vectors.len(),
        batch_tokens,
        chunk_count: chunk_check.count,
        largest_chunk_tokens: chunk_check.largest_tokens,
        oversized_query_and_batch_refused: chunk_check.bounds_refused,
        retrieval,
    })
}
struct ChunkCheck {
    count: usize,
    largest_tokens: usize,
    bounds_refused: bool,
}
fn check_chunks_and_bounds(model: &LocalTextEmbedder) -> anyhow::Result<ChunkCheck> {
    let source=format!("# Return policy\n{}","Products can be returned for a refund. Rückgabe innerhalb von dreißig Tagen. 返品ポリシーをご確認ください。\n\n".repeat(100));
    let chunks = crate::application::knowledge::chunk_document(&source, |text| {
        model.passage_token_count(text)
    })?;
    let largest_chunk_tokens = chunks
        .iter()
        .map(|chunk| model.passage_token_count(&chunk.embedding_text()))
        .collect::<AppResult<Vec<_>>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    if chunks.len() < 2 || largest_chunk_tokens > MAX_EMBEDDING_TOKENS {
        bail!("Knowledge tokenizer-aware chunking failed its native boundary check");
    }
    let texts = chunks
        .iter()
        .take(4)
        .map(|chunk| chunk.embedding_text())
        .collect::<Vec<_>>();
    model.embed_passages(&texts.iter().map(String::as_str).collect::<Vec<_>>())?;
    let oversized_query_and_batch_refused = matches!(
        model.embed_query(&"hello ".repeat(1024)),
        Err(AppError::BadRequest(_))
    ) && matches!(
        model.embed_passages(&["text"; 5]),
        Err(AppError::BadRequest(_))
    );
    if !oversized_query_and_batch_refused {
        bail!("Knowledge model did not enforce its input bounds");
    }
    Ok(ChunkCheck {
        count: chunks.len(),
        largest_tokens: largest_chunk_tokens,
        bounds_refused: oversized_query_and_batch_refused,
    })
}
fn dot(left: &[f32; 384], right: &[f32; 384]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(left, right)| f64::from(*left) * f64::from(*right))
        .sum()
}
fn peak_rss_bytes() -> anyhow::Result<u64> {
    // getrusage writes a plain C record; only read it after a successful return.
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        bail!("Cannot measure model peak resident memory");
    }
    let usage = unsafe { usage.assume_init() };
    #[cfg(target_os = "linux")]
    let multiplier = 1024;
    #[cfg(not(target_os = "linux"))]
    let multiplier = 1;
    Ok(u64::try_from(usage.ru_maxrss)? * multiplier)
}
