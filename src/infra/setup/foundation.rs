//! Fallible deployment resources, before any supervised loop is spawned.
use super::*;
pub(super) struct Foundation {
    pub(super) attribution: Option<Arc<crate::adapters::http::attribution::Bridge>>,
    pub(super) knowledge: Arc<crate::application::knowledge::KnowledgeLibrary>,
    pub(super) config: Arc<AppConfig>,
    pub(super) persistence: Arc<crate::adapters::persistence::PostgresPersistence>,
    pub(super) monitoring: Arc<dyn MonitoringService>,
    pub(super) storage: Option<Arc<dyn FileStorage>>,
    pub(super) mail_transport: Arc<dyn MailTransport>,
    pub(super) mail_available: bool,
    pub(super) sessions: Arc<SessionAuthority>,
    pub(super) scanner: Arc<dyn crate::services::spam_scanner::SpamScanner>,
}
pub(super) async fn build(config: Arc<AppConfig>) -> anyhow::Result<Foundation> {
    let scanner = Arc::new(crate::adapters::spam_scanner::SpamScannerAdapter::new(
        config.spam_scanner_settings(),
    )?);
    let model_path = config.knowledge_model_dir.clone();
    // Load exactly one local model before database setup or worker construction.
    let mut loading = tokio::task::spawn_blocking(move || {
        crate::adapters::embedding::LocalTextEmbedder::load(&model_path)
    });
    let embedder =
        match tokio::time::timeout(std::time::Duration::from_secs(60), &mut loading).await {
            Ok(result) => result??,
            Err(_) => {
                // Native calls cannot be interrupted; Serve's runtime shutdown bound is the backstop.
                loading.abort();
                anyhow::bail!("Knowledge model startup exceeded sixty seconds");
            }
        };
    let smtp =
        LettreMailTransport::from_config(&config, smtp_allow_plaintext_local_from_env()).await?;
    let mail_available = smtp.is_configured();
    let mail_transport: Arc<dyn MailTransport> = smtp;
    let monitoring: Arc<dyn MonitoringService> = Arc::new(CompositeMonitor::new(vec![
        Arc::new(TracingMonitor::new()),
        Arc::new(InMemoryMonitor::new()),
    ]));
    // Validate configured storage before slower database work, as the original composition did.
    let storage = match config.gcs.as_ref() {
        Some(gcs) => Some(Arc::new(GcsFileStorage::from_config(gcs)?) as Arc<dyn FileStorage>),
        None => None,
    };
    let persistence = Arc::new(postgres_persistence().await?);
    let knowledge = Arc::new(crate::application::knowledge::KnowledgeLibrary::new(
        persistence.clone(),
        Arc::new(embedder),
    ));
    let sessions = Arc::new(
        SessionAuthority::new(&config, persistence.clone()).with_monitoring(monitoring.clone()),
    );
    let attribution = crate::infra::attribution::build(persistence.clone(), sessions.clone())?;
    install_builtin_agent_library(persistence.as_ref()).await?;
    Ok(Foundation {
        attribution,
        knowledge,
        config,
        persistence,
        monitoring,
        storage,
        mail_transport,
        mail_available,
        sessions,
        scanner,
    })
}
