//! The inbox is a route producer; the claiming worker belongs to process supervision.
use super::*;
pub(super) struct Inbound {
    pub(super) worker: Arc<InboundEventWorker>,
    pub(super) inbox: Arc<MonitoredInboundEventInbox>,
    pub(super) wakeups: InboundEventWakeups,
}
pub(super) fn build(
    f: &foundation::Foundation,
    clients: &clients::Clients,
    threads: Arc<ThreadUseCases>,
    workflow: &crate::infra::workflow::WorkflowEngine,
) -> anyhow::Result<Inbound> {
    // Registered for every deployment because a tenant may connect its account after startup.
    let decoders = InboundEventDecoderRegistry::new()
        .register(Arc::new(ResendApiInboundDecoder::new(
            clients.resend.clone(),
            f.config.clone(),
            threads,
            f.storage.clone(),
        )))
        .map_err(|error| {
            anyhow::anyhow!("Could not register the Resend inbound decoder: {error}")
        })?;
    let wakeups = InboundEventWakeups::new();
    let worker = Arc::new(
        InboundEventWorker::new(
            f.persistence.clone(),
            f.persistence.clone(),
            Arc::new(decoders),
            wakeups.clone(),
        )
        .with_monitoring(f.monitoring.clone())
        .with_intent_wakeups(workflow.intent_wakeups.clone()),
    );
    let inbox = Arc::new(MonitoredInboundEventInbox::new(
        f.persistence.clone(),
        f.monitoring.clone(),
    ));
    Ok(Inbound {
        worker,
        inbox,
        wakeups,
    })
}
