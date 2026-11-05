use super::{admission::Admission, *};
use crate::{
    app_error::{AppError, AppResult},
    entities::{knowledge::*, value_objects::EmbeddingModelId},
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    time::Instant,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

const INDEXING_DEADLINE: Duration = Duration::from_secs(30);
pub struct KnowledgeLibrary {
    documents: Arc<dyn KnowledgeDocuments>,
    embedder: Arc<dyn TextEmbedder>,
    slots: Arc<Semaphore>,
    admission: Admission,
    tasks: TaskTracker,
    stop: CancellationToken,
    registration: Mutex<()>,
}
impl KnowledgeLibrary {
    pub fn new(documents: Arc<dyn KnowledgeDocuments>, embedder: Arc<dyn TextEmbedder>) -> Self {
        Self {
            documents,
            embedder,
            slots: Arc::new(Semaphore::new(1)),
            admission: Admission::new(),
            tasks: TaskTracker::new(),
            stop: CancellationToken::new(),
            registration: Mutex::new(()),
        }
    }
    pub fn documents(&self) -> &dyn KnowledgeDocuments {
        self.documents.as_ref()
    }
    pub fn embedder(&self) -> Arc<dyn TextEmbedder> {
        self.embedder.clone()
    }
    pub fn admit_body(
        &self,
        scope: KnowledgeScope,
    ) -> Result<OwnedSemaphorePermit, super::admission::UploadAdmissionRefused> {
        if self.stop.is_cancelled() {
            return Err(super::admission::UploadAdmissionRefused::Busy);
        }
        self.admission.body(scope)
    }
    pub async fn upload(
        &self,
        scope: KnowledgeScope,
        files: Vec<KnowledgeUploadFile>,
    ) -> AppResult<Vec<KnowledgeDocumentLayer>> {
        validate_batch(&files)?;
        let sizes: Vec<_> = files.iter().map(KnowledgeUploadFile::size).collect();
        self.documents.preflight_upload(scope, &sizes).await?;
        let permit = self.slots.clone().try_acquire_owned().map_err(|_| {
            AppError::ServiceUnavailable("Knowledge indexing is busy; retry shortly".into())
        })?;
        let cancel = self.stop.child_token();
        let _cancel_on_drop = cancel.clone().drop_guard();
        let worker_cancel = cancel.clone();
        let model = self.embedder.clone();
        let deadline = Instant::now() + INDEXING_DEADLINE;
        // The tracker owns actual blocking work through caller cancellation and shutdown.
        let job = {
            // Serialize task registration with close/wait so shutdown cannot miss a new job.
            let _registration = self.registration.lock().map_err(|_| {
                AppError::ServiceUnavailable("Knowledge indexing is unavailable".into())
            })?;
            live(&cancel, deadline)?;
            self.tasks.spawn_blocking(move || {
                let _permit = permit;
                index_documents(files, model.as_ref(), &worker_cancel, deadline)
            })
        };
        let prepared = tokio::time::timeout_at(deadline, job)
            .await
            .map_err(|_| AppError::Timeout("Knowledge indexing exceeded its deadline".into()))?
            .map_err(|_| {
                AppError::ServiceUnavailable("Knowledge indexing could not complete".into())
            })??;
        live(&cancel, deadline)?;
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(AppError::Timeout("Knowledge indexing was canceled".into())),
            result = self.documents.replace(scope,&prepared) => result,
        }
    }
    pub async fn shutdown(&self) -> AppResult<()> {
        {
            let _registration = self.registration.lock().map_err(|_| {
                AppError::ServiceUnavailable("Knowledge indexing shutdown is unavailable".into())
            })?;
            self.stop.cancel();
            self.slots.close();
            self.admission.close();
            self.tasks.close();
        }
        tokio::time::timeout(Duration::from_secs(5), self.tasks.wait())
            .await
            .map_err(|_| {
                AppError::Timeout("Knowledge indexing did not stop within five seconds".into())
            })
    }
}
fn validate_batch(files: &[KnowledgeUploadFile]) -> AppResult<()> {
    if files.is_empty() || files.len() > MAX_KNOWLEDGE_FILES {
        return Err(AppError::BadRequest(
            "Pick between one and ten files".into(),
        ));
    }
    if files.iter().map(|file| file.text().len()).sum::<usize>() > MAX_COMPANY_KNOWLEDGE_BYTES {
        return Err(AppError::BadRequest(
            "The upload exceeds 10 MiB of text".into(),
        ));
    }
    Ok(())
}
fn live(cancel: &CancellationToken, deadline: Instant) -> AppResult<()> {
    if cancel.is_cancelled() || Instant::now() >= deadline {
        return Err(AppError::Timeout(
            "Knowledge indexing was canceled or exceeded its deadline".into(),
        ));
    }
    Ok(())
}
struct UnembeddedDocument {
    file: KnowledgeUploadFile,
    chunks: Vec<KnowledgeChunk>,
}
fn index_documents(
    files: Vec<KnowledgeUploadFile>,
    model: &dyn TextEmbedder,
    cancel: &CancellationToken,
    deadline: Instant,
) -> AppResult<Vec<PreparedKnowledgeDocument>> {
    let mut drafts = Vec::with_capacity(files.len());
    let mut total = 0usize;
    for file in files {
        let chunks = chunk_document(file.text(), |text| {
            live(cancel, deadline)?;
            let count = model.passage_token_count(text)?;
            live(cancel, deadline)?;
            Ok(count)
        })
        .map_err(|error| match error {
            AppError::BadRequest(message) => {
                AppError::BadRequest(format!("`{}`: {message}", file.file_name().prefixed()))
            }
            other => other,
        })?;
        total += chunks.len();
        if total > MAX_CHUNKS_PER_REQUEST {
            return Err(AppError::BadRequest(
                "The upload has too many sections; split it into smaller uploads".into(),
            ));
        }
        drafts.push(UnembeddedDocument { file, chunks });
    }
    drafts
        .into_iter()
        .map(|draft| embed_document(draft, model, cancel, deadline))
        .collect()
}
fn embed_document(
    draft: UnembeddedDocument,
    model: &dyn TextEmbedder,
    cancel: &CancellationToken,
    deadline: Instant,
) -> AppResult<PreparedKnowledgeDocument> {
    let mut chunks = draft.chunks.into_iter();
    let mut embedded = Vec::new();
    while let Some(first) = chunks.next() {
        live(cancel, deadline)?;
        let mut batch = Vec::with_capacity(EMBEDDING_BATCH_SIZE);
        batch.push(first);
        batch.extend(chunks.by_ref().take(EMBEDDING_BATCH_SIZE - 1));
        let passages: Vec<_> = batch.iter().map(KnowledgeChunk::embedding_text).collect();
        let refs: Vec<_> = passages.iter().map(String::as_str).collect();
        let vectors = model.embed_passages(&refs)?;
        live(cancel, deadline)?;
        if vectors.len() != batch.len() {
            return Err(AppError::ServiceUnavailable(
                "The local embedding model returned the wrong number of vectors".into(),
            ));
        }
        for (chunk, vector) in batch.into_iter().zip(vectors) {
            embedded.push(EmbeddedKnowledgeChunk {
                chunk,
                embedding: normalize_embedding(vector)?,
            });
        }
    }
    Ok(PreparedKnowledgeDocument {
        file: draft.file,
        embedding_model: EmbeddingModelId::from(model.model_id().as_str()),
        chunks: embedded,
    })
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
