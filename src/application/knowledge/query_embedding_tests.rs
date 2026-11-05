use super::*;

struct ReleaseOnDrop(Arc<Gate>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

struct QueryFixture {
    library: Arc<KnowledgeLibrary>,
    gate: Arc<Gate>,
    documents: Arc<Documents>,
    model: Arc<Model>,
    _release: ReleaseOnDrop,
}

fn fixture() -> QueryFixture {
    let gate = Arc::new(Gate::default());
    let documents = Arc::new(Documents::default());
    let model = Arc::new(Model {
        gate: Some(gate.clone()),
        ..Model::new(Output::Valid)
    });
    QueryFixture {
        library: Arc::new(KnowledgeLibrary::new(documents.clone(), model.clone())),
        documents,
        model,
        _release: ReleaseOnDrop(gate.clone()),
        gate,
    }
}

#[tokio::test]
async fn knowledge_search_cancellation_retains_the_actual_shared_native_slot() {
    let f = fixture();
    let library = f.library.clone();
    let first = tokio::spawn(async move { library.embed_query("price").await });
    f.gate.entered.notified().await;
    first.abort();
    assert!(matches!(first.await, Err(error) if error.is_cancelled()));
    assert!(matches!(
        f.library.embed_query("second").await,
        Err(AppError::ServiceUnavailable(_))
    ));
    assert!(matches!(
        f.library.upload(scope(), vec![file("body")]).await,
        Err(AppError::ServiceUnavailable(_))
    ));
    f.gate.release();
    f.library.shutdown().await.unwrap();
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    assert_eq!(f.documents.writes.load(Ordering::SeqCst), 0);
    assert_eq!(f.library.tasks.len(), 0);
}

#[tokio::test(start_paused = true)]
async fn knowledge_search_timeout_is_an_error_and_retains_the_slot_until_native_completion() {
    let f = fixture();
    let library = f.library.clone();
    let first = tokio::spawn(async move { library.embed_query("price").await });
    f.gate.entered.notified().await;
    tokio::time::advance(Duration::from_secs(11)).await;
    assert!(matches!(first.await.unwrap(), Err(AppError::Timeout(_))));
    assert!(matches!(
        f.library.embed_query("second").await,
        Err(AppError::ServiceUnavailable(_))
    ));
    let library = f.library.clone();
    let shutdown = tokio::spawn(async move { library.shutdown().await });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(6)).await;
    assert!(matches!(shutdown.await.unwrap(), Err(AppError::Timeout(_))));
    f.gate.release();
    f.library.tasks.wait().await;
    assert_eq!(f.documents.writes.load(Ordering::SeqCst), 0);
    assert_eq!(f.library.tasks.len(), 0);
}

#[tokio::test]
async fn knowledge_search_result_holds_the_slot_through_consumption_and_releases_on_drop() {
    let docs = Arc::new(Documents::default());
    let model = Arc::new(Model::new(Output::Valid));
    let library = KnowledgeLibrary::new(docs.clone(), model);
    let embedding = library.embed_query("price").await.unwrap();
    assert!((embedding.vector()[0] - 1.0).abs() < 0.001);
    assert!(matches!(
        library.upload(scope(), vec![file("body")]).await,
        Err(AppError::ServiceUnavailable(_))
    ));
    drop(embedding);
    library.upload(scope(), vec![file("body")]).await.unwrap();
    assert_eq!(docs.writes.load(Ordering::SeqCst), 1);
    library.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn knowledge_search_registration_competes_with_shutdown_without_late_consumption() {
    let docs = Arc::new(Documents::default());
    let library = Arc::new(KnowledgeLibrary::new(
        docs.clone(),
        Arc::new(Model::new(Output::Valid)),
    ));
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let query_library = library.clone();
    let query_barrier = barrier.clone();
    let query = tokio::spawn(async move {
        query_barrier.wait().await;
        query_library.embed_query("price").await
    });
    let shutdown_library = library.clone();
    let shutdown_barrier = barrier.clone();
    let shutdown = tokio::spawn(async move {
        shutdown_barrier.wait().await;
        shutdown_library.shutdown().await
    });
    barrier.wait().await;
    let result = query.await.unwrap();
    shutdown.await.unwrap().unwrap();
    if let Ok(embedding) = result {
        assert!(embedding.ensure_live().is_err());
    }
    assert!(library.embed_query("after stop").await.is_err());
    assert_eq!(docs.writes.load(Ordering::SeqCst), 0);
    assert_eq!(library.tasks.len(), 0);
}

#[tokio::test]
async fn knowledge_search_query_boundary_rejects_oversized_input_and_invalid_model_outputs() {
    let model = Arc::new(Model::new(Output::Valid));
    let library = KnowledgeLibrary::new(Arc::new(Documents::default()), model.clone());
    for question in [String::new(), "x".repeat(1001), "bad\u{0000}query".into()] {
        assert!(library.embed_query(&question).await.is_err());
    }
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    library.shutdown().await.unwrap();
    for output in [Output::NonFinite, Output::Missing] {
        let library =
            KnowledgeLibrary::new(Arc::new(Documents::default()), Arc::new(Model::new(output)));
        assert!(library.embed_query("price").await.is_err());
        library.shutdown().await.unwrap();
    }
}
