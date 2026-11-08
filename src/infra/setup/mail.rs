//! Renderer-first construction keeps thread ingress and the internal sender relay acyclic.
use super::*;
pub(super) struct Mail {
    pub(super) email: Arc<EmailRenderer>,
    pub(super) renderers: Arc<TransportRenderers>,
    pub(super) composer: DeliveryComposer,
}
pub(super) fn build(f: &foundation::Foundation) -> anyhow::Result<Mail> {
    let email = Arc::new(EmailRenderer::new(&f.config.app_domain_name));
    let renderers = Arc::new(
        TransportRenderers::new()
            .register(email.clone())
            .map_err(|error| anyhow::anyhow!("Could not register the email renderer: {error}"))?,
    );
    let composer = DeliveryComposer::new(renderers.clone(), f.persistence.clone());
    Ok(Mail {
        email,
        renderers,
        composer,
    })
}
pub(super) fn workflow(
    f: &foundation::Foundation,
    clients: &clients::Clients,
    mail: &Mail,
) -> anyhow::Result<Arc<crate::infra::workflow::WorkflowEngine>> {
    Ok(Arc::new(
        crate::infra::workflow::WorkflowEngine::new(
            (*f.persistence).clone(),
            crate::infra::workflow::WorkflowEngineSetup {
                integrations: clients.integrations.clone(),
                config: &f.config,
                composer: mail.composer.clone(),
                mcp_client: clients.mcp.clone(),
                monitoring: f.monitoring.clone(),
                slots: crate::infra::workflow::worker_slots(
                    std::env::var("WORKFLOW_WORKER_SLOTS").ok().as_deref(),
                    crate::infra::db::max_connections(),
                )?,
                knowledge: f.knowledge.clone(),
            },
        )?
        .with_jev_assets(Arc::new(
            crate::adapters::workflow_jev::JevAssetReader::new(
                (*f.persistence).clone(),
                f.storage.clone(),
            ),
        )),
    ))
}
pub(super) fn threads(
    f: &foundation::Foundation,
    mail: &Mail,
    workflow: &crate::infra::workflow::WorkflowEngine,
) -> Arc<ThreadUseCases> {
    let p = &f.persistence;
    Arc::new(
        ThreadUseCases::new(
            ThreadStores {
                threads: p.clone(),
                channels: p.clone(),
                companies: p.clone(),
                participants: p.clone(),
            },
            InboundIngestPorts {
                committer: p.clone(),
                correlation: p.clone(),
                bindings: p.clone(),
                standalone_deliveries: p.clone(),
            },
            mail.renderers.clone(),
            f.config.thread_settings(),
        )
        .with_monitoring(f.monitoring.clone())
        .with_intent_wakeups(workflow.intent_wakeups.clone()),
    )
}
pub(super) fn transports(
    f: &foundation::Foundation,
    clients: &clients::Clients,
    mail: &Mail,
    threads: Arc<ThreadUseCases>,
) -> anyhow::Result<Arc<TransportRegistry>> {
    Ok(Arc::new(
        TransportRegistry::new()
            .register(
                mail.email.clone(),
                Arc::new(EmailSender::new(
                    Arc::new(ResendApiCompanyTransports::new(
                        clients.resend.clone(),
                        f.mail_transport.clone(),
                    )),
                    threads,
                )),
            )
            .map_err(|error| anyhow::anyhow!("Could not register the email transport: {error}"))?,
    ))
}
