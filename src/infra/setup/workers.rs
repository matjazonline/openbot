//! Fully constructed workers; main retains and joins every spawned loop.
use super::*;
pub struct ApplicationWorkers {
    pub workflow: Arc<crate::infra::workflow::WorkflowEngine>,
    pub inbound_events: Arc<InboundEventWorker>,
    pub notifications: Arc<NotificationWorker>,
    pub deliveries: Arc<crate::services::delivery_worker::DeliveryWorker>,
    pub smtp: Arc<crate::adapters::smtp::SmtpServer>,
}
pub(super) struct Components {
    pub(super) workflow: Arc<crate::infra::workflow::WorkflowEngine>,
    pub(super) threads: Arc<ThreadUseCases>,
    pub(super) composer: DeliveryComposer,
    pub(super) transports: Arc<TransportRegistry>,
    pub(super) inbound: Arc<InboundEventWorker>,
}
pub(super) fn build(
    f: &foundation::Foundation,
    c: Components,
) -> anyhow::Result<ApplicationWorkers> {
    let notifications = Arc::new(NotificationWorker::new(
        f.persistence.clone(),
        c.composer,
        f.config.notification_settings(),
        f.monitoring.clone(),
    ));
    let deliveries = Arc::new(
        crate::services::delivery_worker::DeliveryWorker::new(f.persistence.clone(), c.transports)
            .with_monitoring(f.monitoring.clone()),
    );
    let scanner = Arc::new(crate::adapters::spam_scanner::SpamScannerAdapter::new(
        f.config.spam_scanner_settings(),
    )?);
    let smtp = Arc::new(
        crate::adapters::smtp::SmtpServer::new(c.threads, f.config.clone(), scanner)
            .with_file_storage(f.storage.clone())
            .with_monitoring(f.monitoring.clone()),
    );
    Ok(ApplicationWorkers {
        workflow: c.workflow,
        inbound_events: c.inbound,
        notifications,
        deliveries,
        smtp,
    })
}
