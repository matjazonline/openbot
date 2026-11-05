use super::*;

const QUERY_DEADLINE: Duration = Duration::from_secs(10);

struct CancelQuery(CancellationToken);

impl Drop for CancelQuery {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Retain the shared native slot through the bounded scan and observation commit as well.
/// Otherwise many quick embeddings could leave many large scans in flight at once.
pub struct QueryEmbedding {
    vector: [f32; 384],
    model: EmbeddingModelId,
    cancel: CancelQuery,
    deadline: Instant,
    _permit: OwnedSemaphorePermit,
}

impl QueryEmbedding {
    pub fn vector(&self) -> &[f32; 384] {
        &self.vector
    }

    pub fn model(&self) -> &EmbeddingModelId {
        &self.model
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancel.0
    }

    pub fn ensure_live(&self) -> AppResult<()> {
        query_live(&self.cancel.0, self.deadline)
    }
}

struct QueryWork {
    vector: [f32; 384],
    permit: OwnedSemaphorePermit,
}

impl KnowledgeLibrary {
    pub async fn embed_query(&self, question: &str) -> AppResult<QueryEmbedding> {
        if question.trim().is_empty() || question.chars().count() > 1000 || question.contains('\0')
        {
            return Err(AppError::BadRequest(
                "Use a question of 1–1000 characters".into(),
            ));
        }
        let permit = self.slots.clone().try_acquire_owned().map_err(|_| {
            AppError::ServiceUnavailable("Knowledge processing is busy; retry shortly".into())
        })?;
        let cancel = CancelQuery(self.stop.child_token());
        let deadline = Instant::now() + QUERY_DEADLINE;
        let job = self.register_query(question.to_owned(), permit, cancel.0.clone(), deadline)?;
        let work = tokio::time::timeout_at(deadline, job)
            .await
            .map_err(|_| AppError::Timeout("Knowledge search exceeded its deadline".into()))?
            .map_err(|_| {
                AppError::ServiceUnavailable("Knowledge search could not complete".into())
            })??;
        query_live(&cancel.0, deadline)?;
        Ok(QueryEmbedding {
            vector: work.vector,
            model: self.embedder.model_id(),
            cancel,
            deadline,
            _permit: work.permit,
        })
    }

    fn register_query(
        &self,
        question: String,
        permit: OwnedSemaphorePermit,
        cancel: CancellationToken,
        deadline: Instant,
    ) -> AppResult<tokio::task::JoinHandle<AppResult<QueryWork>>> {
        let model = self.embedder.clone();
        let _registration = self
            .registration
            .lock()
            .map_err(|_| AppError::ServiceUnavailable("Knowledge search is unavailable".into()))?;
        query_live(&cancel, deadline)?;
        // The tracker and permit belong to the actual native work, including after caller drop.
        Ok(self.tasks.spawn_blocking(move || {
            query_live(&cancel, deadline)?;
            let vector = normalize_embedding(model.embed_query(&question)?)?;
            query_live(&cancel, deadline)?;
            Ok(QueryWork { vector, permit })
        }))
    }
}

fn query_live(cancel: &CancellationToken, deadline: Instant) -> AppResult<()> {
    if cancel.is_cancelled() || Instant::now() >= deadline {
        Err(AppError::Timeout(
            "Knowledge search was canceled or exceeded its deadline".into(),
        ))
    } else {
        Ok(())
    }
}
