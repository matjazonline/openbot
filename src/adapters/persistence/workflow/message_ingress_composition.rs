use super::*;
use crate::application::knowledge::{KnowledgeLibrary, test_support::DeterministicTextEmbedder};
use crate::infra::workflow::WorkflowEngineSetup;

pub(super) fn engine(
    persistence: &PostgresPersistence,
    config: &AppConfig,
    renderers: Arc<TransportRenderers>,
) -> WorkflowEngine {
    let store = Arc::new(persistence.clone());
    let knowledge = Arc::new(KnowledgeLibrary::new(
        store.clone(),
        Arc::new(DeterministicTextEmbedder::default()),
    ));
    WorkflowEngine::new(
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
            knowledge,
        },
    )
    .unwrap()
}
