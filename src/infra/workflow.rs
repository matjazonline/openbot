//! Production composition of the workflow engine (p08 D1.8): the admission and lifecycle
//! services, every step handler behind one composite, and the supervised worker loops `serve`
//! runs. Tests compose their own narrower engines; this is the only production wiring.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::adapters::harness::rig::{WorkflowAgentRuntime, providers::ProviderRegistry};
use crate::adapters::persistence::PostgresPersistence;
use crate::adapters::persistence::workflow::agent_publication::AgentPublicationDirectory;
use crate::adapters::persistence::workflow::child_publication::ChildPublicationDirectory;
use crate::adapters::persistence::workflow::http::HttpPublicationSelection;
use crate::adapters::persistence::workflow::mcp::McpPublicationSelection;
use crate::adapters::persistence::workflow::provisioning::{
    AssistantChannelService, ProvisioningDeployment, ProvisioningEffects,
};
use crate::adapters::persistence::workflow::tool_publication::ToolPublicationDirectory;
use crate::adapters::workflow_agent::AgentActionTools;
use crate::adapters::workflow_http::{HttpDestinationPolicy, HttpRuntime};
use crate::adapters::workflow_mcp::McpRuntime;
use crate::adapters::workflow_message::MessageRuntime;
use crate::adapters::workflow_provisioning::ProvisioningRuntime;
use crate::adapters::workflow_source::WorkflowSourceDecoder;
use crate::application::services::mcp_client::McpClient;
use crate::application::services::runtime_metrics::ActiveWorkflowJobs;
use crate::application::transport::DeliveryComposer;
use crate::application::workflow::channel_reply::ChannelReplyPublisher;
use crate::application::workflow::decisions::DecisionNotices;
use crate::application::workflow::history::WorkflowHistoryRuntime;
use crate::application::workflow::lease::{
    ClaimedWorkflow, LeasePolicy, WorkflowHandlerOutcome, WorkflowWorkerId,
};
use crate::application::workflow::lifecycle::{BindingService, DefinitionService};
use crate::application::workflow::message::MessagePlanner;
use crate::application::workflow::message_admission::{
    MessageAdmissionWorker, MessageAdmissions, MessageIntentWorkerId,
};
use crate::application::workflow::polling::{PollPolicy, WorkflowStepKind};
use crate::application::workflow::provisioning::AssistantChannels;
use crate::application::workflow::schedule_admission::{
    ScheduleMaterializer, ScheduleMaterializerWorker, ScheduleRunNow, ScheduleWorkerId,
};
use crate::application::workflow::templates::{TemplateCatalogue, TemplateCopyService};
use crate::application::workflow::worker::{WorkflowHandler, WorkflowWorker};
use crate::application::workflow::{LifecycleAuthorizer, WorkflowAuthorization, WorkflowService};
use crate::domain::monitoring::MonitoringService;

mod services;
pub use services::WorkflowServices;

/// Registered step kinds a deployment without the memory service has no handler for.
/// Publication refuses them so no run can stall on one; with `MEMORY_SERVICE_URL` set they are
/// handled, and publication instead requires memory enabled for the company (E4).
pub const UNHANDLED_STEP_KINDS: &[&str] = &["memory.load", "memory.save"];

/// The default and bounds of `WORKFLOW_WORKER_SLOTS`, and the database connections everything
/// else `serve` runs needs (listener, HTTP headroom, task, delivery, notification and inbound
/// workers, the intent worker, the schedule materializer, metrics sampler, storage maintenance,
/// two concurrent final publishers, and the memory lifecycle and delivery workers). Each slot needs
/// two: its claim or handler transaction and its lease renewal. The memory delivery worker counts
/// one at any `MEMORY_SERVICE_BACKGROUND_IN_FLIGHT`: its lanes overlap only their sends and take
/// turns on the store (`MemoryDeliveryWorker`).
pub const DEFAULT_WORKFLOW_WORKER_SLOTS: usize = 2;
pub const MAX_WORKFLOW_WORKER_SLOTS: usize = 16;
pub const RESERVED_DATABASE_CONNECTIONS: u32 = 14;

const LEASE: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_secs(1);
const POLL_PAGE: u16 = 16;
const INTENT_POLL: Duration = Duration::from_secs(1);
const SCHEDULE_POLL: Duration = Duration::from_secs(2);

pub type ProductionAuthorizer =
    LifecycleAuthorizer<PostgresPersistence, PostgresPersistence, PostgresPersistence>;
pub type ProductionWorkflowService = WorkflowService<
    PostgresPersistence,
    PostgresPersistence,
    PostgresPersistence,
    PostgresPersistence,
    ProductionAuthorizer,
>;
pub type ProductionPublication =
    ChildPublicationDirectory<AgentPublicationDirectory<ToolPublicationDirectory>>;
pub type ProductionDefinitions = DefinitionService<
    ProductionAuthorizer,
    PostgresPersistence,
    WorkflowSourceDecoder,
    ProductionPublication,
>;
pub type ProductionBindings = BindingService<
    ProductionAuthorizer,
    PostgresPersistence,
    PostgresPersistence,
    PostgresPersistence,
>;
pub type ProductionReconciliation =
    crate::application::workflow::actions::ActionReconciliationService<
        crate::adapters::persistence::workflow::action_reconciliation::PostgresActionReconciliation<
            crate::adapters::persistence::workflow::action_reconciliation::CompanyReconciliationResources,
        >,
        ProductionAuthorizer,
        PostgresPersistence,
    >;

/// The number of workflow worker slots, validated against the database pool (p08 F010/F015).
pub fn worker_slots(configured: Option<&str>, pool_connections: u32) -> anyhow::Result<usize> {
    let slots = match configured.map(str::trim).filter(|value| !value.is_empty()) {
        None => DEFAULT_WORKFLOW_WORKER_SLOTS,
        Some(value) => value
            .parse::<usize>()
            .ok()
            .filter(|slots| (1..=MAX_WORKFLOW_WORKER_SLOTS).contains(slots))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "WORKFLOW_WORKER_SLOTS must be between 1 and {MAX_WORKFLOW_WORKER_SLOTS}"
                )
            })?,
    };
    let needed = RESERVED_DATABASE_CONNECTIONS + 2 * u32::try_from(slots)?;
    if pool_connections < needed {
        anyhow::bail!(
            "DATABASE_MAX_CONNECTIONS is {pool_connections}, but WORKFLOW_WORKER_SLOTS={slots} needs \
             at least {needed} ({RESERVED_DATABASE_CONNECTIONS} + 2 per slot)"
        );
    }
    Ok(slots)
}

/// Every step handler of this deployment behind one dispatch by kind. Each action-owned kind
/// reaches its runtime, which performs effects only through ActionService's marker-first
/// protocol (§04.10) or its documented equivalent: the memory runtime commits a `memory.save`'s
/// delivery intent with its park and sends nothing itself. This composite adds no effect path of
/// its own.
pub struct ProductionHandler {
    jev: crate::application::workflow::jev::JevRuntime<PostgresPersistence>,
    agent: WorkflowAgentRuntime<PostgresPersistence, AgentActionTools>,
    http: Arc<HttpRuntime>,
    mcp: Arc<McpRuntime>,
    message: Arc<MessageRuntime>,
    history: WorkflowHistoryRuntime<PostgresPersistence>,
    /// Present only while the memory service is configured.
    memory: Option<crate::application::workflow::memory::MemoryStepRuntime<PostgresPersistence>>,
    local_agent: crate::application::local_agents::workflow::LocalAgentHandler<
        crate::adapters::persistence::workflow::local_agents::PostgresLocalAgentWorkflow,
    >,
}

impl ProductionHandler {
    fn route(&self, kind: &WorkflowStepKind) -> Option<&dyn WorkflowHandler> {
        if self.jev.supports(kind) {
            return Some(&self.jev);
        }
        if let Some(memory) = self.memory.as_ref().filter(|memory| memory.supports(kind)) {
            return Some(memory);
        }
        let handlers: [&dyn WorkflowHandler; 6] = [
            &self.agent,
            self.http.as_ref(),
            self.mcp.as_ref(),
            self.message.as_ref(),
            &self.history,
            &self.local_agent,
        ];
        handlers.into_iter().find(|handler| handler.supports(kind))
    }
}

#[async_trait::async_trait]
impl WorkflowHandler for ProductionHandler {
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        self.route(kind).is_some()
    }

    async fn execute(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome {
        match self.route(kind) {
            // Boxed: the agent runtime's future is large, and this frame sits under the worker.
            Some(handler) => Box::pin(handler.execute(kind, claim)).await,
            None => unreachable!("the worker only executes kinds `supports` accepted"),
        }
    }
}

/// The production workflow engine: its services, its handler and its wakeups.
pub struct WorkflowEngine {
    services: Arc<WorkflowServices>,
    handler: Arc<ProductionHandler>,
    reply_publisher: Arc<ChannelReplyPublisher<PostgresPersistence>>,
    slots: usize,
    /// The jobs the worker slots are executing, for the runtime panel's sampler.
    active_jobs: ActiveWorkflowJobs,
    monitoring: Arc<dyn MonitoringService>,
    /// Woken after an inbound commit wrote admission intents.
    pub intent_wakeups: Arc<Notify>,
    /// Woken after an admission created work.
    pub worker_wakeups: Arc<Notify>,
    /// The memory lifecycle reconciler, present only while the memory service is configured.
    memory_lifecycle: Option<Arc<crate::application::memory::MemoryLifecycleWorker>>,
    /// The promised-delivery worker, present only while the memory service is configured.
    memory_delivery: Option<Arc<crate::application::memory::MemoryDeliveryWorker>>,
}

/// Named composition inputs keep the shared model explicit and avoid positional domain/URL arguments.
pub struct WorkflowEngineSetup<'a> {
    pub integrations: Arc<dyn crate::application::integrations::ExecutionGateway>,
    pub config: &'a crate::infra::config::AppConfig,
    pub composer: DeliveryComposer,
    pub mcp_client: Arc<dyn McpClient>,
    pub monitoring: Arc<dyn MonitoringService>,
    pub slots: usize,
    pub knowledge: Arc<crate::application::knowledge::KnowledgeLibrary>,
    pub http_destinations: HttpDestinationPolicy,
}

struct ToolRuntimes {
    http: Arc<HttpRuntime>,
    mcp: Arc<McpRuntime>,
    message: Arc<MessageRuntime>,
    dispatch: AgentActionTools,
}

fn compose_tools(
    persistence: &PostgresPersistence,
    setup: &WorkflowEngineSetup<'_>,
    notices: &DecisionNotices,
    templates: &ProvisioningDeployment,
    lease: LeasePolicy,
) -> ToolRuntimes {
    let http = Arc::new(HttpRuntime::with_integrations(
        persistence.clone(),
        setup.http_destinations.clone(),
        lease,
        Some(notices.clone()),
        setup.integrations.clone(),
    ));
    let mcp = Arc::new(McpRuntime::with_integrations(
        persistence.clone(),
        setup.mcp_client.clone(),
        lease,
        Some(notices.clone()),
        setup.integrations.clone(),
    ));
    let message = Arc::new(MessageRuntime::with_approvals(
        persistence.clone(),
        setup.composer.clone(),
        setup.config.app_domain_name.clone(),
        lease,
        Some(notices.clone()),
    ));
    let provisioning = Arc::new(ProvisioningRuntime::new(
        persistence.clone(),
        ProvisioningEffects::new(templates.clone()),
        lease,
    ));
    let store = Arc::new(persistence.clone());
    let knowledge = crate::application::workflow::knowledge::KnowledgeSearchRuntime::new(
        setup.knowledge.clone(),
        store.clone(),
        store,
        setup.config.knowledge_search_limit,
    );
    let dispatch = AgentActionTools::new(http.clone(), mcp.clone(), message.clone())
        .with_workflow_tools(Arc::new(persistence.clone()))
        .with_provisioning(provisioning)
        .with_knowledge(Arc::new(knowledge));
    ToolRuntimes {
        http,
        mcp,
        message,
        dispatch,
    }
}

fn compose_handler(
    persistence: &PostgresPersistence,
    setup: &WorkflowEngineSetup<'_>,
    notices: &DecisionNotices,
    templates: &ProvisioningDeployment,
    lease: LeasePolicy,
) -> anyhow::Result<Arc<ProductionHandler>> {
    let tools = compose_tools(persistence, setup, notices, templates, lease);
    let agent = WorkflowAgentRuntime::new(
        ProviderRegistry::standard().map_err(|error| anyhow::anyhow!("{error}"))?,
        persistence.clone(),
        tools.dispatch,
        Some(notices.clone()),
        lease,
    );
    Ok(Arc::new(ProductionHandler {
        jev: crate::application::workflow::jev::JevRuntime::new(
            persistence.clone(),
            Arc::new(crate::adapters::workflow_jev::JevHttp::new()?),
            lease,
        ),
        agent,
        http: tools.http,
        mcp: tools.mcp,
        message: tools.message,
        history: WorkflowHistoryRuntime::new(persistence.clone()),
        memory: None,
        local_agent: crate::application::local_agents::workflow::LocalAgentHandler::new(
            crate::adapters::persistence::workflow::local_agents::PostgresLocalAgentWorkflow::new(
                persistence.clone(),
                Some(notices.clone()),
            ),
            lease,
        ),
    }))
}

impl WorkflowEngine {
    /// Every production caller supplies the Foundation-owned knowledge library.
    pub fn new(
        persistence: PostgresPersistence,
        setup: WorkflowEngineSetup<'_>,
    ) -> anyhow::Result<Self> {
        let lease = LeasePolicy::new(LEASE).map_err(anyhow::Error::msg)?;
        let notices = DecisionNotices::new(
            setup.composer.clone(),
            &setup.config.public_base_url(),
            &setup.config.app_domain_name,
        );
        let persistence = persistence
            .with_channel_reply_notices(notices.clone())
            .with_app_domain(&setup.config.app_domain_name);
        let catalogue = TemplateCatalogue::shipped(&WorkflowSourceDecoder)
            .map_err(|error| anyhow::anyhow!("shipped workflow templates: {error:?}"))?;
        let templates = ProvisioningDeployment::new(
            Arc::new(catalogue),
            UNHANDLED_STEP_KINDS,
            &setup.config.app_domain_name,
        );
        let handler = compose_handler(&persistence, &setup, &notices, &templates, lease)?;
        let reply_publisher = Arc::new(
            ChannelReplyPublisher::new(
                persistence.clone(),
                MessagePlanner::new(setup.composer.clone(), setup.config.app_domain_name.clone()),
            )
            .with_monitoring(setup.monitoring.clone()),
        );
        let worker_wakeups = Arc::new(Notify::new());
        let services = Arc::new(WorkflowServices {
            persistence,
            notices,
            templates,
            monitoring: setup.monitoring.clone(),
            worker_wakeups: worker_wakeups.clone(),
            memory_provisioner: None,
        });
        Ok(Self {
            services,
            handler,
            reply_publisher,
            slots: setup.slots,
            active_jobs: ActiveWorkflowJobs::default(),
            monitoring: setup.monitoring,
            intent_wakeups: Arc::new(Notify::new()),
            worker_wakeups,
            memory_lifecycle: None,
            memory_delivery: None,
        })
    }

    /// Test composition: the memory service configured against an address nothing serves.
    #[cfg(test)]
    pub(crate) fn with_unserved_memory(self) -> Self {
        let config = crate::infra::memory_config::MemoryServiceConfig {
            base_url: "http://127.0.0.1:1".into(),
            provisioner: crate::infra::memory_config::ProvisionerToken::parse(
                secrecy::SecretString::from(
                    "msk_adm_0190f1a2b3c44d5e8f9a0b1c2d3e4f50_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                ),
            )
            .unwrap(),
            request_timeout: std::time::Duration::from_secs(1),
            recall_deadline: std::time::Duration::from_millis(500),
            interactive_in_flight: 1,
            background_in_flight: 1,
        };
        let service =
            Arc::new(crate::adapters::memory_service::MemoryServiceHttp::new(&config).unwrap());
        self.with_memory(
            service.clone(),
            service,
            config.provisioner.credential_id(),
            config.background_in_flight,
        )
    }

    /// Compose the configured memory service (E1): settings may enable it under this
    /// provisioner credential id, and `spawn` supervises its lifecycle reconciler and its
    /// promised-delivery worker (E8), which sends up to `delivery_lanes` saves at once — the
    /// size of the service client's Delivery pool, so its permits are what bound the sends.
    pub fn with_memory(
        mut self,
        provisioner: Arc<dyn crate::application::memory::MemoryProvisioner>,
        client: Arc<dyn crate::application::memory::MemoryClient>,
        credential_id: Uuid,
        delivery_lanes: usize,
    ) -> Self {
        // Every client call re-verifies a credential the service refuses (p06 A4-9).
        let client: Arc<dyn crate::application::memory::MemoryClient> =
            Arc::new(crate::application::memory::ReverifyingClient::new(
                client,
                Arc::new(self.services.persistence.clone()),
            ));
        let handler = Arc::get_mut(&mut self.handler).expect("engine composition precedes sharing");
        handler.memory = Some(
            crate::application::workflow::memory::MemoryStepRuntime::new(
                self.services.persistence.clone(),
                client.clone(),
            ),
        );
        // Automatic recall (E7) and the agent memory tools (E5) share the client and persistence.
        handler.agent.set_recall(Arc::new(
            crate::application::workflow::memory_recall::MemoryRecall::new(
                Arc::new(self.services.persistence.clone()),
                client.clone(),
            )
            .with_monitoring(self.monitoring.clone()),
        ));
        handler.agent.tools_mut().set_memory(Arc::new(
            crate::application::workflow::memory_tools::MemoryToolRuntime::new(
                Arc::new(self.services.persistence.clone()),
                client.clone(),
            ),
        ));
        self.memory_delivery = Some(Arc::new(
            crate::application::memory::MemoryDeliveryWorker::new(
                Arc::new(self.services.persistence.clone()),
                client,
            )
            .with_lanes(delivery_lanes)
            .with_monitoring(self.monitoring.clone()),
        ));
        Arc::get_mut(&mut self.services)
            .expect("engine composition precedes sharing")
            .memory_provisioner = Some(credential_id);
        self.memory_lifecycle = Some(Arc::new(
            crate::application::memory::MemoryLifecycleWorker::new(
                Arc::new(self.services.persistence.clone()),
                provisioner,
                credential_id,
            ),
        ));
        self
    }

    pub fn with_jev_assets(
        mut self,
        assets: Arc<dyn crate::application::workflow::jev::JevAssets>,
    ) -> Self {
        Arc::get_mut(&mut self.handler)
            .expect("engine composition precedes sharing")
            .jev
            .set_assets(assets);
        self
    }

    pub fn handler(&self) -> &ProductionHandler {
        &self.handler
    }

    /// The `WORKFLOW_WORKER_SLOTS` this engine runs.
    pub fn slots(&self) -> usize {
        self.slots
    }

    /// The gauge [`Self::spawn`]'s slots count their jobs in.
    pub fn active_jobs(&self) -> ActiveWorkflowJobs {
        self.active_jobs.clone()
    }

    pub fn services(&self) -> Arc<WorkflowServices> {
        self.services.clone()
    }

    /// Start workflow slots, the intent worker, schedule materializer and metrics sampler
    /// under `shutdown`. The caller keeps the handles and joins them on shutdown.
    pub fn spawn(&self, shutdown: &CancellationToken) -> Vec<(&'static str, JoinHandle<()>)> {
        let mut handles = Vec::with_capacity(self.slots + 7);
        let vault = crate::adapters::persistence::integrations::PostgresVault::new(
            self.services.persistence.clone(),
        );
        let vault_stop = shutdown.clone();
        handles.push((
            "integration teardown",
            tokio::spawn(async move {
                crate::application::integrations::lifecycle::run(&vault, &vault_stop).await;
            }),
        ));

        let metric_persistence = self.services.persistence.clone();
        let monitoring = self.monitoring.clone();
        let metric_shutdown = shutdown.clone();
        handles.push((
            "workflow metrics",
            tokio::spawn(async move {
                crate::application::workflow::metrics::run(
                    &metric_persistence,
                    monitoring.as_ref(),
                    &metric_shutdown,
                )
                .await;
            }),
        ));
        for _ in 0..self.slots {
            let persistence = self.services.persistence.clone();
            let handler = self.handler.clone();
            let notices = self.services.notices.clone();
            let wakeup = self.worker_wakeups.clone();
            let active_jobs = self.active_jobs.clone();
            let monitoring = self.monitoring.clone();
            let shutdown = shutdown.clone();
            handles.push((
                "workflow worker",
                tokio::spawn(async move {
                    let worker = WorkflowWorker {
                        port: &persistence,
                        handler: handler.as_ref(),
                        notices: Some(&notices),
                        worker: WorkflowWorkerId(Uuid::new_v4()),
                        lease: LeasePolicy::new(LEASE).expect("static workflow lease"),
                        poll: PollPolicy::new(POLL, POLL_PAGE).expect("static workflow poll"),
                        active_jobs: Some(&active_jobs),
                        monitoring: Some(monitoring.as_ref()),
                    };
                    worker.run(&wakeup, &shutdown).await;
                }),
            ));
        }
        let admissions = MessageAdmissions::new(
            self.services.persistence.clone(),
            self.services.service(),
            self.services.persistence.clone(),
        )
        .with_monitoring(self.monitoring.clone());
        let intent_wakeup = self.intent_wakeups.clone();
        let worker_wakeup = self.worker_wakeups.clone();
        let intent_shutdown = shutdown.clone();
        handles.push((
            "workflow message admission worker",
            tokio::spawn(async move {
                let worker = MessageAdmissionWorker {
                    admissions: admissions.with_admitted(worker_wakeup),
                    worker: MessageIntentWorkerId(Uuid::new_v4()),
                    poll: INTENT_POLL,
                };
                worker.run(&intent_wakeup, &intent_shutdown).await;
            }),
        ));
        let publisher = self.reply_publisher.clone();
        let publisher_shutdown = shutdown.clone();
        handles.push((
            "workflow final reply publisher",
            tokio::spawn(async move { publisher.run(&publisher_shutdown).await }),
        ));
        let local_persistence = self.services.persistence.clone();
        let local_shutdown = shutdown.clone();
        handles.push((
            "local-agent retention",
            tokio::spawn(async move {
                crate::application::local_agents::lifecycle::run(
                    &local_persistence,
                    &local_shutdown,
                )
                .await;
            }),
        ));
        handles.push(self.spawn_storage(shutdown));
        if let Some(memory) = self.memory_lifecycle.clone() {
            let memory_shutdown = shutdown.clone();
            handles.push((
                "memory lifecycle",
                tokio::spawn(async move { memory.run(&memory_shutdown).await }),
            ));
            // Retention and gauges of the memory rows (p06 A3-3), only with the service.
            let maintenance = crate::application::memory::MemoryMaintenance::new(
                Arc::new(self.services.persistence.clone()),
                self.monitoring.clone(),
            );
            let maintenance_shutdown = shutdown.clone();
            handles.push((
                "memory maintenance",
                tokio::spawn(async move { maintenance.run(&maintenance_shutdown).await }),
            ));
        }
        if let Some(delivery) = self.memory_delivery.clone() {
            let delivery_shutdown = shutdown.clone();
            handles.push((
                "memory delivery",
                tokio::spawn(async move { delivery.run(&delivery_shutdown).await }),
            ));
        }
        let materializer = self.services.schedule_materializer();
        let schedule_shutdown = shutdown.clone();
        handles.push((
            "workflow schedule materializer",
            tokio::spawn(async move {
                let worker = ScheduleMaterializerWorker {
                    materializer,
                    worker: ScheduleWorkerId(Uuid::new_v4()),
                    poll: SCHEDULE_POLL,
                };
                worker.run(&schedule_shutdown).await;
            }),
        ));
        handles
    }
    fn spawn_storage(&self, shutdown: &CancellationToken) -> (&'static str, JoinHandle<()>) {
        let persistence = self.services.persistence.clone();
        let monitoring = self.monitoring.clone();
        let shutdown = shutdown.clone();
        (
            "workflow storage maintenance",
            tokio::spawn(async move {
                crate::application::workflow::storage::run(
                    &persistence,
                    monitoring.as_ref(),
                    &shutdown,
                )
                .await;
            }),
        )
    }
}

#[cfg(test)]
impl WorkflowEngine {
    /// The production composition on `persistence`, for tests: one slot, `agents.test`.
    pub(crate) fn with_jev_test_provider(
        mut self,
        provider: Arc<dyn crate::application::workflow::jev::JevProvider>,
    ) -> Self {
        let handler = Arc::get_mut(&mut self.handler).expect("unshared test engine");
        handler.jev = crate::application::workflow::jev::JevRuntime::new(
            self.services.persistence.clone(),
            provider,
            LeasePolicy::new(LEASE).unwrap(),
        );
        self
    }
    pub(crate) fn for_tests(persistence: PostgresPersistence) -> Self {
        Self::for_tests_with_integrations(
            persistence,
            Arc::new(crate::application::integrations::unavailable::UnavailableIntegrations),
        )
    }
    pub(crate) fn for_tests_with_integrations(
        persistence: PostgresPersistence,
        integrations: Arc<dyn crate::application::integrations::ExecutionGateway>,
    ) -> Self {
        let client = Arc::new(crate::adapters::mcp::HttpMcpClient::new(
            crate::adapters::mcp::policy::EndpointPolicy::default(),
        ));
        Self::for_tests_with_clients(persistence, integrations, client)
    }

    pub(crate) fn for_tests_with_mcp(
        persistence: PostgresPersistence,
        client: Arc<dyn McpClient>,
    ) -> Self {
        Self::for_tests_with_clients(
            persistence,
            Arc::new(crate::application::integrations::unavailable::UnavailableIntegrations),
            client,
        )
    }

    fn for_tests_with_clients(
        persistence: PostgresPersistence,
        integrations: Arc<dyn crate::application::integrations::ExecutionGateway>,
        client: Arc<dyn McpClient>,
    ) -> Self {
        use crate::adapters::protocols::email::EmailRenderer;
        use crate::application::transport::ports::TransportRenderers;
        let renderers = Arc::new(
            TransportRenderers::new()
                .register(Arc::new(EmailRenderer::new("agents.test")))
                .unwrap(),
        );
        let composer = DeliveryComposer::new(renderers, Arc::new(persistence.clone()));
        let config = crate::infra::config::AppConfig {
            app_domain_name: "agents.test".into(),
            ..crate::infra::config::AppConfig::for_test()
        };
        let knowledge = Arc::new(crate::application::knowledge::KnowledgeLibrary::new(
            Arc::new(persistence.clone()),
            Arc::new(
                crate::application::knowledge::test_support::DeterministicTextEmbedder::default(),
            ),
        ));
        Self::new(
            persistence,
            WorkflowEngineSetup {
                integrations,
                config: &config,
                composer,
                mcp_client: client,
                monitoring: Arc::new(crate::adapters::monitoring::InMemoryMonitor::new()),
                slots: 1,
                knowledge,
                http_destinations: HttpDestinationPolicy::default(),
            },
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::persistence::test_support::{OwnDatabase, own_database};
    use crate::app_error::AppError;
    use crate::application::use_cases::company::{CompanyPersistence, CompanyWrite};
    use crate::application::use_cases::user::UserPersistence;
    use crate::application::workflow::completion::is_io_kind;
    use crate::application::workflow::lifecycle::{
        DefinitionTarget, DraftContent, LifecycleError, PublishDraft, SaveDraft,
    };
    use crate::application::workflow::registry::TYPES;
    use crate::application::workflow::{
        AdmitWorkflowRequest, DeliveryPolicy, IdempotencyKey, RelatedAssociation, WorkflowActor,
    };
    use crate::domain::workflow::*;

    fn engine() -> WorkflowEngine {
        let pool = sqlx::PgPool::connect_lazy("postgres://invalid:invalid@127.0.0.1:1/invalid")
            .expect("a lazy pool never connects");
        engine_on(PostgresPersistence::new(pool))
    }

    fn engine_on(persistence: PostgresPersistence) -> WorkflowEngine {
        WorkflowEngine::for_tests(persistence)
    }

    /// Every registered step kind that needs a handler has one here, or publication refuses it
    /// (p08 D1.8); and nothing is both.
    #[tokio::test]
    async fn production_handler_covers_every_io_kind_or_refuses_it() {
        let engine = engine();
        for kind in TYPES {
            if !is_io_kind(kind) {
                continue;
            }
            let handled = engine
                .handler()
                .supports(&WorkflowStepKind(kind.to_string()));
            let refused = UNHANDLED_STEP_KINDS.contains(&kind);
            assert!(
                handled != refused,
                "{kind}: handled={handled} refused={refused}"
            );
        }
    }

    /// The production engine on a migrated database of its own, and its company owner's target
    /// for a fresh workflow. The database lives as long as the returned handle.
    async fn composed() -> (OwnDatabase, WorkflowEngine, DefinitionTarget) {
        let database = own_database()
            .await
            .expect("composition tests need a database");
        let persistence = PostgresPersistence::new(database.pool.clone());
        let email = "composition@example.test";
        persistence
            .create_user("composition", email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Composition".into(),
                slug: "composition".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let target = DefinitionTarget {
            company: CompanyId::new(company.id),
            actor: WorkflowActor::authenticated(owner.id).unwrap(),
            workflow: WorkflowId::new(Uuid::new_v4()),
        };
        (database, engine_on(persistence), target)
    }

    /// Save `source` as `target`'s first draft and publish it through the production
    /// composition, or the refusal of either.
    async fn publish(
        engine: &WorkflowEngine,
        target: DefinitionTarget,
        mut source: serde_json::Value,
    ) -> Result<VersionId, LifecycleError> {
        source["workflow_id"] = serde_json::json!(target.workflow.to_string());
        let definitions = engine.services().definitions(Vec::new(), Vec::new());
        definitions
            .save(SaveDraft {
                target,
                expected: None,
                content: DraftContent {
                    title: "Workflow".into(),
                    description: "Composition".into(),
                    source: source.to_string(),
                },
            })
            .await?;
        let version = VersionId::new(Uuid::new_v4());
        definitions
            .publish(PublishDraft {
                target,
                expected: DraftRevision::new(1).unwrap(),
                version,
                key: IdempotencyKey::parse(format!("publish-{}", target.workflow.as_uuid()))
                    .unwrap(),
            })
            .await?;
        Ok(version)
    }

    fn example(kind: &str) -> serde_json::Value {
        serde_json::from_str(
            &crate::application::workflow::registry::example(kind)
                .unwrap()
                .source,
        )
        .unwrap()
    }

    /// D1.8: a step kind this deployment cannot run is refused at publication with a diagnostic;
    /// a `workflow.call` parent publishes with its pinned child captured (WF-C001 r2).
    #[tokio::test]
    async fn production_publication_refuses_memory_steps_and_captures_children() {
        let (_database, engine, target) = composed().await;
        for kind in UNHANDLED_STEP_KINDS {
            let refused = publish(
                &engine,
                DefinitionTarget {
                    workflow: WorkflowId::new(Uuid::new_v4()),
                    ..target
                },
                example(kind),
            )
            .await;
            assert!(
                matches!(&refused, Err(LifecycleError::Validation(diagnostic))
                    if diagnostic.code == "workflow.unsupported_step"),
                "{kind}: {refused:?}"
            );
        }
        let child = DefinitionTarget {
            workflow: WorkflowId::new(Uuid::new_v4()),
            ..target
        };
        let version = publish(&engine, child, example("data.map")).await.unwrap();
        let mut parent = example("workflow.call");
        parent["steps"]["start"]["child_workflow_id"] =
            serde_json::json!(child.workflow.to_string());
        parent["steps"]["start"]["child_version_id"] = serde_json::json!(version.to_string());
        parent["steps"]["start"]["with"] = serde_json::json!({"input":{"ref":"/input"}});
        parent["input_schema"] = serde_json::json!(true);
        publish(&engine, target, parent).await.unwrap();
    }

    fn with_memory(engine: WorkflowEngine) -> WorkflowEngine {
        engine.with_unserved_memory()
    }

    /// E4: with the memory service configured, the memory kinds are handled and no longer
    /// refused as unsupported; publication then requires memory enabled for the company and an
    /// agent scope naming an agent of the workflow.
    #[tokio::test]
    async fn memory_integration_configured_publication_gates_memory_steps_per_company() {
        assert!(UNHANDLED_STEP_KINDS.iter().all(|kind| {
            with_memory(engine())
                .handler()
                .supports(&WorkflowStepKind(kind.to_string()))
        }));
        let (_database, engine, target) = composed().await;
        let engine = with_memory(engine);
        let fresh = || DefinitionTarget {
            workflow: WorkflowId::new(Uuid::new_v4()),
            ..target
        };
        for kind in UNHANDLED_STEP_KINDS {
            let refused = publish(&engine, fresh(), example(kind)).await;
            assert!(
                matches!(&refused, Err(LifecycleError::Validation(diagnostic))
                    if diagnostic.code == "memory.not_enabled"),
                "{kind}: {refused:?}"
            );
        }
        use crate::application::memory::MemorySettingsStore;
        engine
            .services()
            .persistence
            .enable_memory(
                target.company,
                target.actor,
                0,
                crate::infra::memory_config::ProvisionerToken::parse(secrecy::SecretString::from(
                    "msk_adm_0190f1a2b3c44d5e8f9a0b1c2d3e4f50_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                ))
                .unwrap()
                .credential_id(),
            )
            .await
            .unwrap();
        for kind in UNHANDLED_STEP_KINDS {
            publish(&engine, fresh(), example(kind)).await.unwrap();
        }
        let mut ghost = example("memory.save");
        ghost["steps"]["start"]["with"]["scope"] =
            serde_json::json!({"literal":{"kind":"agent","agent":"ghost"}});
        let refused = publish(&engine, fresh(), ghost).await;
        assert!(
            matches!(&refused, Err(LifecycleError::Validation(diagnostic))
                if diagnostic.code == "memory.unknown_agent"),
            "{refused:?}"
        );
    }

    /// D1.8 / WF-C001 r2: `admit` never creates a child run; only the parent's step does.
    #[tokio::test]
    async fn production_admission_refuses_a_child_trigger() {
        let (_database, engine, target) = composed().await;
        let id = Uuid::new_v4();
        let parent = ChildCause::Step(ChildStepRef::new(
            ExecutionRef::new(
                target.company,
                RunId::new(id),
                ExecutionId::new(id),
                StepId::parse("start").unwrap(),
            ),
            ChildOrdinal::FIRST,
        ));
        let refused = engine
            .services()
            .service()
            .admit(AdmitWorkflowRequest {
                history_session: None,
                company_id: target.company,
                actor: target.actor,
                association: RelatedAssociation::Company,
                trigger: TriggerRef::new(
                    target.company,
                    TriggerId::new(id),
                    TriggerSource::Child { parent },
                )
                .unwrap(),
                correlation_id: crate::domain::entities::correlation::CorrelationId::new(),
                binding_id: WorkflowBindingId::new(Uuid::new_v4()),
                idempotency_key: IdempotencyKey::parse("child").unwrap(),
                input: serde_json::json!({}),
                rerun_of: None,
                delivery_policy: DeliveryPolicy::Deliver,
                expected_binding_revision: None,
                admission_claim: None,
            })
            .await;
        assert!(matches!(refused, Err(AppError::BadRequest(_))));
    }

    /// D1.8 shutdown bridge: every loop `spawn` starts stops on the token, idle or not, and
    /// joins promptly.
    #[tokio::test]
    async fn production_workers_stop_on_the_shutdown_token() {
        let (_database, engine, _) = composed().await;
        let shutdown = CancellationToken::new();
        let handles = engine.spawn(&shutdown);
        assert!(
            handles
                .iter()
                .any(|(name, _)| *name == "integration teardown")
        );
        let task_count = handles.len();
        engine.intent_wakeups.notify_waiters();
        engine.worker_wakeups.notify_waiters();
        tokio::time::sleep(Duration::from_millis(50)).await;
        shutdown.cancel();
        for (name, handle) in handles {
            tokio::time::timeout(Duration::from_secs(5), handle)
                .await
                .unwrap_or_else(|_| panic!("{name} did not stop"))
                .unwrap();
        }
        assert_eq!(
            task_count, 8,
            "one slot, metrics, intent, final publication, schedule, workflow storage, local-agent storage and integration teardown workers"
        );
    }

    #[test]
    fn worker_slots_respect_the_pool() {
        assert_eq!(
            worker_slots(None, 20).unwrap(),
            DEFAULT_WORKFLOW_WORKER_SLOTS
        );
        // The default pool (20) runs the default slots (p08 F015).
        assert_eq!(
            worker_slots(None, crate::infra::db::DEFAULT_DATABASE_MAX_CONNECTIONS).unwrap(),
            2
        );
        assert_eq!(worker_slots(Some("4"), 22).unwrap(), 4);
        assert!(worker_slots(Some("4"), 21).is_err());
        assert!(worker_slots(Some("5"), 23).is_err());
        assert!(worker_slots(Some("0"), 64).is_err());
        assert!(worker_slots(Some("17"), 64).is_err());
        assert!(worker_slots(Some("two"), 64).is_err());
    }
}
