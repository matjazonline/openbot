use super::*;
use crate::{
    domain::workflow::CompanyId,
    entities::access_level::{AccessLevel, LevelActor},
    entities::company_member::CompanyMembership,
};
use async_trait::async_trait;
use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Default)]
struct Documents {
    writes: AtomicUsize,
    preflights: AtomicUsize,
    preflight_gate: Option<Arc<PreflightGate>>,
}
#[async_trait]
impl KnowledgeDocuments for Documents {
    async fn access(&self, _: KnowledgeScope) -> AppResult<LevelActor> {
        Ok(actor())
    }
    async fn list(&self, _: KnowledgeScope) -> AppResult<KnowledgeLibraryView> {
        Ok(KnowledgeLibraryView {
            actor: actor(),
            layers: vec![],
        })
    }
    async fn preflight_upload(&self, _: KnowledgeScope, _: &[KnowledgeLayerSize]) -> AppResult<()> {
        self.preflights.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = &self.preflight_gate {
            gate.entered.notify_one();
            gate.release.notified().await;
        }
        Ok(())
    }
    async fn replace(
        &self,
        _: KnowledgeScope,
        _: &[PreparedKnowledgeDocument],
    ) -> AppResult<Vec<KnowledgeDocumentLayer>> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        Ok(vec![])
    }
    async fn delete(&self, _: DeleteKnowledgeLayer) -> AppResult<()> {
        panic!("unused")
    }
}
fn actor() -> LevelActor {
    LevelActor {
        membership: CompanyMembership::Owner,
        access_level: AccessLevel::MAX,
    }
}
fn scope() -> KnowledgeScope {
    KnowledgeScope {
        company: CompanyId::new(Uuid::new_v4()),
        actor_id: Uuid::new_v4(),
    }
}
fn file(text: &str) -> KnowledgeUploadFile {
    KnowledgeUploadFile::from_upload("2_notes.md", text.as_bytes().to_vec()).unwrap()
}
#[derive(Default)]
struct Gate {
    entered: Notify,
    released: Mutex<bool>,
    changed: Condvar,
}
impl Gate {
    fn wait(&self) {
        self.entered.notify_one();
        let mut ready = self.released.lock().unwrap();
        while !*ready {
            ready = self.changed.wait(ready).unwrap();
        }
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.changed.notify_all();
    }
}
struct Model {
    gate: Option<Arc<Gate>>,
    calls: AtomicUsize,
    output: Output,
}
#[derive(Clone, Copy)]
enum Output {
    Valid,
    Missing,
    NonFinite,
}
impl Model {
    fn new(output: Output) -> Self {
        Self {
            gate: None,
            calls: AtomicUsize::new(0),
            output,
        }
    }
}
impl TextEmbedder for Model {
    fn model_id(&self) -> EmbeddingModelId {
        "test-model".into()
    }
    fn passage_token_count(&self, text: &str) -> AppResult<usize> {
        Ok(text.chars().count() + 3)
    }
    fn embed_passages(&self, passages: &[&str]) -> AppResult<Vec<[f32; 384]>> {
        assert!(passages.len() <= EMBEDDING_BATCH_SIZE);
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = &self.gate {
            gate.wait();
        }
        let mut vector = [0.0; 384];
        vector[0] = match self.output {
            Output::NonFinite => f32::NAN,
            _ => 2.0,
        };
        Ok(vec![
            vector;
            match self.output {
                Output::Missing => 0,
                _ => passages.len(),
            }
        ])
    }
    fn embed_query(&self, _: &str) -> AppResult<[f32; 384]> {
        self.embed_passages(&["query"])?
            .pop()
            .ok_or_else(|| AppError::ServiceUnavailable("Missing test model output".into()))
    }
}

#[path = "query_embedding_tests.rs"]
mod queries;
#[tokio::test(start_paused = true)]
async fn knowledge_upload_cancellation_retains_actual_native_permit_and_bounded_shutdown_never_writes()
 {
    let docs = Arc::new(Documents::default());
    let gate = Arc::new(Gate::default());
    let model = Arc::new(Model {
        gate: Some(gate.clone()),
        ..Model::new(Output::Valid)
    });
    let service = Arc::new(KnowledgeLibrary::new(docs.clone(), model));
    let first_service = service.clone();
    let first = tokio::spawn(async move { first_service.upload(scope(), vec![file("one")]).await });
    gate.entered.notified().await;
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    assert!(matches!(
        service.upload(scope(), vec![file("two")]).await,
        Err(AppError::ServiceUnavailable(_))
    ));
    let shutting_down = service.clone();
    let shutdown = tokio::spawn(async move { shutting_down.shutdown().await });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(6)).await;
    assert!(matches!(shutdown.await.unwrap(), Err(AppError::Timeout(_))));
    gate.release();
    service.tasks.wait().await;
    assert_eq!(docs.writes.load(Ordering::SeqCst), 0);
    assert_eq!(service.tasks.len(), 0);
}
#[tokio::test]
async fn knowledge_upload_validates_all_chunks_before_embedding_and_rejects_bad_model_outputs() {
    let docs = Arc::new(Documents::default());
    let model = Arc::new(Model::new(Output::Valid));
    let library = KnowledgeLibrary::new(docs.clone(), model.clone());
    assert!(
        library
            .upload(
                scope(),
                vec![file("valid"), file(&format!("# {}", "x".repeat(129)))]
            )
            .await
            .is_err()
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    assert_eq!(docs.writes.load(Ordering::SeqCst), 0);
    for output in [Output::Missing, Output::NonFinite] {
        let library = KnowledgeLibrary::new(docs.clone(), Arc::new(Model::new(output)));
        assert!(library.upload(scope(), vec![file("text")]).await.is_err());
    }
    assert_eq!(docs.writes.load(Ordering::SeqCst), 0);
    library
        .upload(scope(), vec![file(&"long paragraph ".repeat(200))])
        .await
        .unwrap();
    assert!(model.calls.load(Ordering::SeqCst) > 1);
    assert_eq!(docs.writes.load(Ordering::SeqCst), 1);
}
#[test]
fn knowledge_upload_rejects_invalid_files_before_chunking_and_sanitizes_before_hashing() {
    for name in ["notes.md", "2_notes.pdf", "12_notes.md"] {
        assert!(KnowledgeUploadFile::from_upload(name, b"ok".to_vec()).is_err());
    }
    for bytes in [
        vec![0],
        vec![0xff],
        vec![b'a'; MAX_KNOWLEDGE_FILE_BYTES + 1],
    ] {
        assert!(KnowledgeUploadFile::from_upload("0_notes.txt", bytes).is_err());
    }
    let clean = file("a\u{200b}b\r\nc");
    assert_eq!(clean.text(), "ab\r\nc");
    assert_eq!(clean.sha256(), file("ab\r\nc").sha256());
}

#[derive(Default)]
struct PreflightGate {
    entered: Notify,
    release: Notify,
}
#[tokio::test]
async fn knowledge_upload_shutdown_competes_with_preflight_and_refuses_later_model_registration() {
    let gate = Arc::new(PreflightGate::default());
    let documents = Arc::new(Documents {
        preflight_gate: Some(gate.clone()),
        ..Default::default()
    });
    let model = Arc::new(Model::new(Output::Valid));
    let library = Arc::new(KnowledgeLibrary::new(documents.clone(), model.clone()));
    let uploading = library.clone();
    let request = tokio::spawn(async move { uploading.upload(scope(), vec![file("text")]).await });
    gate.entered.notified().await;
    library.shutdown().await.unwrap();
    gate.release.notify_one();
    assert!(matches!(
        request.await.unwrap(),
        Err(AppError::ServiceUnavailable(_))
    ));
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    assert_eq!(documents.writes.load(Ordering::SeqCst), 0);
    assert_eq!(library.tasks.len(), 0);
}
#[tokio::test(start_paused = true)]
async fn knowledge_upload_deadline_cancels_the_result_and_retains_permit_until_the_native_batch_stops()
 {
    let documents = Arc::new(Documents::default());
    let gate = Arc::new(Gate::default());
    let model = Arc::new(Model {
        gate: Some(gate.clone()),
        ..Model::new(Output::Valid)
    });
    let library = Arc::new(KnowledgeLibrary::new(documents.clone(), model));
    let uploading = library.clone();
    let request = tokio::spawn(async move { uploading.upload(scope(), vec![file("text")]).await });
    gate.entered.notified().await;
    tokio::time::advance(INDEXING_DEADLINE + Duration::from_secs(1)).await;
    assert!(matches!(request.await.unwrap(), Err(AppError::Timeout(_))));
    assert!(matches!(
        library.upload(scope(), vec![file("retry")]).await,
        Err(AppError::ServiceUnavailable(_))
    ));
    gate.release();
    library.tasks.close();
    library.tasks.wait().await;
    assert_eq!(documents.writes.load(Ordering::SeqCst), 0);
}
