//! Fallible deployment resources, before any supervised loop is spawned.
use super::*;
pub(super) struct Foundation {
    pub(super) config: Arc<AppConfig>,
    pub(super) persistence: Arc<crate::adapters::persistence::PostgresPersistence>,
    pub(super) monitoring: Arc<dyn MonitoringService>,
    pub(super) storage: Option<Arc<dyn FileStorage>>,
    pub(super) mail_transport: Arc<dyn MailTransport>,
    pub(super) mail_available: bool,
    pub(super) sessions: Arc<SessionAuthority>,
}
pub(super) async fn build(config: Arc<AppConfig>) -> anyhow::Result<Foundation> {
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
    let sessions = Arc::new(
        SessionAuthority::new(&config, persistence.clone()).with_monitoring(monitoring.clone()),
    );
    install_builtin_agent_library(persistence.as_ref()).await?;
    Ok(Foundation {
        config,
        persistence,
        monitoring,
        storage,
        mail_transport,
        mail_available,
        sessions,
    })
}
