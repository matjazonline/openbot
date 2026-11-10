//! The bounded embedding pool (D6, D7). Two inference slots, each one model session on one
//! blocking thread at a time: the query slot is reserved for query embeddings, the shared slot
//! serves indexing and overflow queries. A query waits at most `QUERY_WAIT` for a slot, then
//! gets 429; indexing never takes the query slot, so it cannot starve queries.
use super::ports::{EmbedError, Embedder, IndexFailure, IndexedPart};
use crate::domain::{
    error::{ErrorCode, ServiceError},
    retrieval::{
        ChunkError, EMBED_BATCH, MAX_INPUT_TOKENS, chunk, normalize, passage_input, query_input,
    },
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// How long a query waits for an embedding slot before 429 `capacity_exceeded`.
pub const QUERY_WAIT: Duration = Duration::from_secs(1);
/// Blocking embedding threads in use at once: one per slot.
pub const EMBEDDING_THREADS: usize = 2;

#[derive(Clone)]
struct Slot {
    embedder: Arc<dyn Embedder>,
    permit: Arc<Semaphore>,
}

impl Slot {
    fn new(embedder: Arc<dyn Embedder>) -> Self {
        Self {
            embedder,
            permit: Arc::new(Semaphore::new(1)),
        }
    }
}

pub struct EmbeddingPool {
    query: Slot,
    shared: Slot,
    model_id: String,
    query_wait: Duration,
}

fn unavailable() -> ServiceError {
    ServiceError::unavailable("The local embedding model is unavailable")
}

impl EmbeddingPool {
    /// Both sessions must be the same pinned model.
    pub fn new(query: Arc<dyn Embedder>, shared: Arc<dyn Embedder>) -> Result<Self, String> {
        if query.model_id() != shared.model_id() {
            return Err("the embedding sessions load different models".into());
        }
        Ok(Self {
            model_id: query.model_id().to_owned(),
            query: Slot::new(query),
            shared: Slot::new(shared),
            query_wait: QUERY_WAIT,
        })
    }

    pub fn with_query_wait(mut self, wait: Duration) -> Self {
        self.query_wait = wait;
        self
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Embed one search query (`query: ` prefix), validated and normalized. A query over the
    /// model token limit is refused (400), never truncated; a model failure is 503.
    pub async fn embed_query(&self, query: &str) -> Result<Vec<f32>, ServiceError> {
        let acquire = async {
            tokio::select! {
                biased;
                permit = self.query.permit.clone().acquire_owned() => (self.query.clone(), permit),
                permit = self.shared.permit.clone().acquire_owned() => (self.shared.clone(), permit),
            }
        };
        let (slot, permit) = match tokio::time::timeout(self.query_wait, acquire).await {
            Ok((slot, Ok(permit))) => (slot, permit),
            Ok((_, Err(_))) => return Err(unavailable()),
            Err(_) => {
                return Err(ServiceError::new(
                    ErrorCode::CapacityExceeded,
                    "The embedding capacity is exhausted; retry shortly",
                ));
            }
        };
        let input = query_input(query);
        let result = tokio::task::spawn_blocking(move || {
            let _permit: OwnedSemaphorePermit = permit;
            if slot.embedder.count_tokens(&input)? > MAX_INPUT_TOKENS {
                return Err(EmbedError::InputTooLong);
            }
            slot.embedder.embed(&[input])
        })
        .await
        .map_err(|_| unavailable())?;
        match result {
            Ok(mut vectors) if vectors.len() == 1 => {
                normalize(vectors.remove(0)).map_err(|_| unavailable())
            }
            Ok(_) => Err(unavailable()),
            Err(EmbedError::InputTooLong) => Err(ServiceError::invalid(format!(
                "The query exceeds the embedding model's {MAX_INPUT_TOKENS}-token limit"
            ))),
            Err(EmbedError::Unavailable) => Err(unavailable()),
        }
    }

    /// Chunk and embed one observation on the shared slot. `Ok(None)` when `cancel` was set
    /// (the lease was lost or its local deadline passed); the work stops between batches.
    pub async fn embed_parts(
        &self,
        title: String,
        body: String,
        cancel: Arc<AtomicBool>,
    ) -> Result<Option<Vec<IndexedPart>>, IndexFailure> {
        let permit = self
            .shared
            .permit
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| IndexFailure::EmbeddingUnavailable)?;
        let embedder = self.shared.embedder.clone();
        tokio::task::spawn_blocking(move || {
            let _permit: OwnedSemaphorePermit = permit;
            embed_observation(embedder.as_ref(), &title, &body, &cancel)
        })
        .await
        .map_err(|_| IndexFailure::EmbeddingUnavailable)?
    }
}

/// Chunk with the model's own counter, then embed every part (`passage: ` prefix, the title in
/// each part). A part over the limit, too many parts or an invalid vector fail indexing.
pub fn embed_observation(
    embedder: &dyn Embedder,
    title: &str,
    body: &str,
    cancel: &AtomicBool,
) -> Result<Option<Vec<IndexedPart>>, IndexFailure> {
    let ranges = chunk(body, |range| {
        embedder
            .count_tokens(&passage_input(title, body, range))
            .map_err(|_| ChunkError::Counter)
    })
    .map_err(|error| match error {
        ChunkError::Counter => IndexFailure::EmbeddingUnavailable,
        ChunkError::PartOverLimit => IndexFailure::PartOverLimit,
        ChunkError::TooManyParts => IndexFailure::TooManyParts,
    })?;
    let mut parts = Vec::with_capacity(ranges.len());
    for batch in ranges.chunks(EMBED_BATCH) {
        if cancel.load(Ordering::Acquire) {
            return Ok(None);
        }
        let inputs: Vec<String> = batch
            .iter()
            .map(|range| passage_input(title, body, range.clone()))
            .collect();
        let vectors = embedder.embed(&inputs).map_err(|error| match error {
            EmbedError::InputTooLong => IndexFailure::PartOverLimit,
            EmbedError::Unavailable => IndexFailure::EmbeddingUnavailable,
        })?;
        if vectors.len() != batch.len() {
            return Err(IndexFailure::EmbeddingUnavailable);
        }
        for (range, vector) in batch.iter().zip(vectors) {
            parts.push(IndexedPart {
                byte_start: range.start,
                byte_end: range.end,
                vector: normalize(vector).map_err(|_| IndexFailure::InvalidVector)?,
            });
        }
    }
    Ok(Some(parts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::test_embedder::{FAKE_MODEL, FakeEmbedder, FakeMode};
    use std::time::Instant;

    fn slow(delay_ms: u64) -> Arc<FakeEmbedder> {
        let mut fake = FakeEmbedder::new(FAKE_MODEL);
        fake.delay = Duration::from_millis(delay_ms);
        Arc::new(fake)
    }

    /// Competing claimants for the two slots: indexing never takes the query slot, a query
    /// waits at most its bound and then gets 429, and inputs are never truncated.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn memory_search_embedding_pool_reserves_a_query_slot_and_bounds_waiting() {
        let (query, shared) = (slow(600), slow(2_000));
        let pool = Arc::new(
            EmbeddingPool::new(query.clone(), shared.clone())
                .unwrap()
                .with_query_wait(Duration::from_millis(200)),
        );
        let indexing = tokio::spawn({
            let pool = pool.clone();
            async move {
                pool.embed_parts("t".into(), "body".into(), Default::default())
                    .await
            }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        // The shared slot is busy indexing; the reserved slot answers the query.
        let started = Instant::now();
        let vector = pool.embed_query("hello").await.unwrap();
        assert_eq!(vector.len(), crate::domain::retrieval::DIMENSIONS);
        assert!(started.elapsed() < Duration::from_millis(1_500));
        // Both slots busy: a third query waits its bound, then 429.
        let busy = tokio::spawn({
            let pool = pool.clone();
            async move { pool.embed_query("long").await }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        let started = Instant::now();
        let refused = pool.embed_query("third").await.unwrap_err();
        assert_eq!(refused.code, ErrorCode::CapacityExceeded);
        assert!(started.elapsed() >= Duration::from_millis(190));
        busy.await.unwrap().unwrap();
        assert_eq!(indexing.await.unwrap().unwrap().unwrap().len(), 1);
        // A query over the model's token limit is refused, never truncated.
        let long = "word ".repeat(MAX_INPUT_TOKENS);
        assert_eq!(
            pool.embed_query(&long).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        // A model failure is 503; an invalid vector too.
        for mode in [FakeMode::Fail, FakeMode::Nan] {
            query.set(mode);
            shared.set(mode);
            assert_eq!(
                pool.embed_query("x").await.unwrap_err().code,
                ErrorCode::Unavailable
            );
        }
        // Sessions of different models never form a pool.
        assert!(EmbeddingPool::new(query, Arc::new(FakeEmbedder::new("other"))).is_err());
    }
}
