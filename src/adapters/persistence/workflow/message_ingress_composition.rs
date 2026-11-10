use super::*;
use crate::application::knowledge::{KnowledgeLibrary, test_support::DeterministicTextEmbedder};
use crate::infra::workflow::WorkflowEngineSetup;

pub(super) struct Composition {
    pub(super) engine: WorkflowEngine,
    pub(super) knowledge: Arc<KnowledgeLibrary>,
}

pub(super) fn engine(
    persistence: &PostgresPersistence,
    config: &AppConfig,
    renderers: Arc<TransportRenderers>,
) -> Composition {
    let store = Arc::new(persistence.clone());
    let knowledge = Arc::new(KnowledgeLibrary::new(
        store.clone(),
        Arc::new(DeterministicTextEmbedder::default()),
    ));
    let engine = WorkflowEngine::new(
        persistence.clone(),
        WorkflowEngineSetup {
            config,
            composer: DeliveryComposer::new(renderers, store),
            mcp_client: Arc::new(crate::adapters::mcp::HttpMcpClient::new(
                crate::adapters::mcp::policy::EndpointPolicy::default(),
            )),
            monitoring: Arc::new(crate::adapters::monitoring::InMemoryMonitor::new()),
            // Five DB connections: preserve the existing single-slot ingress fixture budget.
            slots: 1,
            knowledge: knowledge.clone(),
            http_destinations: crate::adapters::workflow_http::HttpDestinationPolicy::default(),
        },
    )
    .unwrap();
    Composition { engine, knowledge }
}

pub(super) fn threads(
    persistence: &PostgresPersistence,
    config: &AppConfig,
    renderers: Arc<TransportRenderers>,
    wakeups: Arc<tokio::sync::Notify>,
) -> ThreadUseCases {
    let shared = Arc::new(persistence.clone());
    ThreadUseCases::new(
        ThreadStores {
            threads: shared.clone(),
            channels: shared.clone(),
            companies: shared.clone(),
            participants: shared.clone(),
        },
        InboundIngestPorts {
            committer: shared.clone(),
            correlation: shared.clone(),
            bindings: shared.clone(),
            standalone_deliveries: shared,
        },
        renderers,
        config.thread_settings(),
    )
    .with_intent_wakeups(wakeups)
}
