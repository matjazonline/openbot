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
use crate::application::workflow::decisions::DecisionNotices;
use crate::application::workflow::history::WorkflowHistoryRuntime;
use crate::application::workflow::lease::{
    ClaimedWorkflow, LeasePolicy, WorkflowHandlerOutcome, WorkflowWorkerId,
};
use crate::application::workflow::lifecycle::{BindingService, DefinitionService};
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

/// Registered step kinds this deployment has no handler for: memory is out of this release
/// (NOT_PLANNED, 06-F032). Publication refuses them so no run can stall on one.
pub const UNHANDLED_STEP_KINDS: &[&str] = &["memory.load", "memory.save"];

/// The default and bounds of `WORKFLOW_WORKER_SLOTS`, and the database connections everything
/// else `serve` runs needs (listener, HTTP headroom, task, delivery, notification and inbound
/// workers, the intent worker, the schedule materializer). Each slot needs two: its claim or
/// handler transaction and its lease renewal.
pub const DEFAULT_WORKFLOW_WORKER_SLOTS: usize = 2;
pub const MAX_WORKFLOW_WORKER_SLOTS: usize = 16;
pub const RESERVED_DATABASE_CONNECTIONS: u32 = 8;

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
/// protocol (§04.10); this composite adds no effect path of its own.
pub struct ProductionHandler {
    agent: WorkflowAgentRuntime<PostgresPersistence, AgentActionTools>,
    http: Arc<HttpRuntime>,
    mcp: Arc<McpRuntime>,
    message: Arc<MessageRuntime>,
    history: WorkflowHistoryRuntime<PostgresPersistence>,
}

impl ProductionHandler {
    fn route(&self, kind: &WorkflowStepKind) -> Option<&dyn WorkflowHandler> {
        let handlers: [&dyn WorkflowHandler; 5] = [
            &self.agent,
            self.http.as_ref(),
            self.mcp.as_ref(),
            self.message.as_ref(),
            &self.history,
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
    persistence: PostgresPersistence,
    handler: Arc<ProductionHandler>,
    notices: DecisionNotices,
    /// The shipped templates assistant provisioning compiles from (p08 D2.5).
    templates: ProvisioningDeployment,
    slots: usize,
    /// The jobs the worker slots are executing, for the runtime panel's sampler.
    active_jobs: ActiveWorkflowJobs,
    monitoring: Arc<dyn MonitoringService>,
    /// Woken after an inbound commit wrote admission intents.
    pub intent_wakeups: Arc<Notify>,
    /// Woken after an admission created work.
    pub worker_wakeups: Arc<Notify>,
}

impl WorkflowEngine {
    /// `public_base_url` is `AppConfig::public_base_url()`; `app_domain` is `APP_DOMAIN_NAME`.
    pub fn new(
        persistence: PostgresPersistence,
        public_base_url: &str,
        app_domain: &str,
        composer: DeliveryComposer,
        mcp_client: Arc<dyn McpClient>,
        monitoring: Arc<dyn MonitoringService>,
        slots: usize,
    ) -> anyhow::Result<Self> {
        let lease = LeasePolicy::new(LEASE).map_err(anyhow::Error::msg)?;
        let notices = DecisionNotices::new(composer.clone(), public_base_url, app_domain);
        let http = Arc::new(HttpRuntime::with_approvals(
            persistence.clone(),
            HttpDestinationPolicy::default(),
            lease,
            Some(notices.clone()),
        ));
        let mcp = Arc::new(McpRuntime::with_approvals(
            persistence.clone(),
            mcp_client,
            lease,
            Some(notices.clone()),
        ));
        let message = Arc::new(MessageRuntime::with_approvals(
            persistence.clone(),
            composer,
            app_domain.to_string(),
            lease,
            Some(notices.clone()),
        ));
        let catalogue = TemplateCatalogue::shipped(&WorkflowSourceDecoder)
            .map_err(|error| anyhow::anyhow!("shipped workflow templates: {error:?}"))?;
        let templates =
            ProvisioningDeployment::new(Arc::new(catalogue), UNHANDLED_STEP_KINDS, app_domain);
        let provisioning = Arc::new(ProvisioningRuntime::new(
            persistence.clone(),
            ProvisioningEffects::new(templates.clone()),
            lease,
        ));
        let tools = AgentActionTools::new(http.clone(), mcp.clone(), message.clone())
            .with_workflow_tools(Arc::new(persistence.clone()))
            .with_provisioning(provisioning);
        let agent = WorkflowAgentRuntime::new(
            ProviderRegistry::standard().map_err(|error| anyhow::anyhow!("{error}"))?,
            persistence.clone(),
            tools,
            Some(notices.clone()),
            lease,
        );
        let handler = Arc::new(ProductionHandler {
            agent,
            http,
            mcp,
            message,
            history: WorkflowHistoryRuntime::new(persistence.clone()),
        });
        Ok(Self {
            persistence,
            handler,
            notices,
            templates,
            slots,
            active_jobs: ActiveWorkflowJobs::default(),
            monitoring,
            intent_wakeups: Arc::new(Notify::new()),
            worker_wakeups: Arc::new(Notify::new()),
        })
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

    fn authorizer(&self) -> ProductionAuthorizer {
        LifecycleAuthorizer::new(
            self.persistence.clone(),
            self.persistence.clone(),
            self.persistence.clone(),
        )
    }

    pub fn service(&self) -> ProductionWorkflowService {
        let p = self.persistence.clone();
        WorkflowService::new(p.clone(), p.clone(), p.clone(), p, self.authorizer())
    }

    /// Lifecycle authorization as the HTTP and schedule use cases preflight it.
    pub fn authorization(&self) -> Arc<dyn WorkflowAuthorization> {
        Arc::new(self.authorizer())
    }

    fn schedule_materializer(
        &self,
    ) -> ScheduleMaterializer<PostgresPersistence, ProductionWorkflowService> {
        ScheduleMaterializer::new(self.persistence.clone(), self.service())
            .with_monitoring(self.monitoring.clone())
            .with_admitted(self.worker_wakeups.clone())
    }

    /// Run-now's inline materialization, through the materializer's own path (p08 D1.9).
    pub fn schedule_run_now(&self) -> Arc<dyn ScheduleRunNow> {
        Arc::new(self.schedule_materializer())
    }

    /// The workflow routes' read models (p08 B3b-1).
    pub fn reads(&self) -> &dyn crate::application::workflow::reads::WorkflowReads {
        &self.persistence
    }

    /// The authoring pages' read models (p09 B1a).
    pub fn authoring_reads(&self) -> &dyn crate::application::workflow::reads::AuthoringReads {
        &self.persistence
    }

    /// The run pages' read models (p09 B2a).
    pub fn run_views(&self) -> &dyn crate::application::workflow::run_views::RunViews {
        &self.persistence
    }

    /// A published version of this company whose workflow is not archived — the one a binding
    /// may select — for the binding form's parameter fields and resource slots (p09 B1b). It
    /// authorizes nothing; the route has.
    pub async fn selectable_bundle(
        &self,
        company: crate::domain::workflow::CompanyId,
        version: crate::domain::workflow::VersionId,
    ) -> crate::app_error::AppResult<
        Option<Arc<crate::application::workflow::publication::PublishedBundle>>,
    > {
        crate::application::workflow::lifecycle::WorkflowDefinitions::selectable_version(
            &self.persistence,
            company,
            version,
        )
        .await
    }

    /// The editor's step catalogue reference: every kind this deployment runs, each derived from
    /// a compiling example. Built once; it depends only on the binary.
    pub fn step_catalogue(
        &self,
    ) -> &'static [crate::application::workflow::authoring::CatalogueEntry] {
        static CATALOGUE: std::sync::LazyLock<
            Vec<crate::application::workflow::authoring::CatalogueEntry>,
        > = std::sync::LazyLock::new(|| {
            crate::application::workflow::authoring::catalogue_reference(
                &WorkflowSourceDecoder,
                UNHANDLED_STEP_KINDS,
            )
        });
        &CATALOGUE
    }

    pub fn decisions(&self) -> &dyn crate::application::workflow::decisions::WorkflowDecisions {
        &self.persistence
    }

    /// Named workflow tool registrations (07-D2, notice WF-C003).
    pub fn workflow_tools(&self) -> &dyn crate::application::workflow::tools::WorkflowTools {
        &self.persistence
    }

    /// The shipped template catalogue (p08 D2.5).
    pub fn template_catalogue(&self) -> &TemplateCatalogue {
        self.templates.catalogue()
    }

    pub fn template_copies(
        &self,
    ) -> TemplateCopyService<ProductionAuthorizer, PostgresPersistence, WorkflowSourceDecoder> {
        TemplateCopyService::new(
            self.authorizer(),
            self.persistence.clone(),
            WorkflowSourceDecoder,
        )
    }

    /// Action reconciliation as an operator annotation (P08-D3): the command actor's current
    /// access to the action's resource, and no evidence verifier, so only an unknown note
    /// settles and nothing reopens (§D3.2).
    pub fn reconciliation(&self) -> ProductionReconciliation {
        crate::application::workflow::actions::ActionReconciliationService::new(
            crate::adapters::persistence::workflow::action_reconciliation::PostgresActionReconciliation::new(
                self.persistence.clone(),
                crate::adapters::persistence::workflow::action_reconciliation::CompanyReconciliationResources,
            ),
            self.authorizer(),
            self.persistence.clone(),
            None,
        )
    }

    /// Wake an idle worker slot for work a route just committed (a decision's continuation).
    pub fn wake_workers(&self) {
        self.worker_wakeups.notify_one();
    }

    /// "Create assistant channel" (p08 D2.4), over the same provisioning as the Builder's tool.
    pub fn assistant_channels(&self) -> Arc<dyn AssistantChannels> {
        Arc::new(AssistantChannelService::new(
            self.persistence.clone(),
            self.templates.clone(),
        ))
    }

    pub fn bindings(&self) -> ProductionBindings {
        let p = self.persistence.clone();
        BindingService::new(self.authorizer(), p.clone(), p.clone(), p)
    }

    /// The definition service for one publication, capturing exactly the HTTP and MCP
    /// operations the publisher selected (WF-C001 r2 nesting; unhandled kinds refused).
    pub fn definitions(
        &self,
        http: Vec<HttpPublicationSelection>,
        mcp: Vec<McpPublicationSelection>,
    ) -> ProductionDefinitions {
        let p = self.persistence.clone();
        let directory = ChildPublicationDirectory {
            persistence: p.clone(),
            inner: AgentPublicationDirectory {
                persistence: p.clone(),
                inner: ToolPublicationDirectory {
                    persistence: p.clone(),
                    http,
                    mcp,
                },
            },
        };
        DefinitionService::new(self.authorizer(), p, WorkflowSourceDecoder, directory)
            .with_refused_kinds(UNHANDLED_STEP_KINDS)
    }

    /// Start the workflow worker slots, the message intent worker and the schedule materializer
    /// under `shutdown`. The caller keeps the handles and joins them on shutdown.
    pub fn spawn(&self, shutdown: &CancellationToken) -> Vec<(&'static str, JoinHandle<()>)> {
        let mut handles = Vec::with_capacity(self.slots + 2);
        for _ in 0..self.slots {
            let persistence = self.persistence.clone();
            let handler = self.handler.clone();
            let notices = self.notices.clone();
            let wakeup = self.worker_wakeups.clone();
            let active_jobs = self.active_jobs.clone();
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
                    };
                    worker.run(&wakeup, &shutdown).await;
                }),
            ));
        }
        let admissions = MessageAdmissions::new(
            self.persistence.clone(),
            self.service(),
            self.persistence.clone(),
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
        let materializer = self.schedule_materializer();
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
}

#[cfg(test)]
impl WorkflowEngine {
    /// The production composition on `persistence`, for tests: one slot, `agents.test`.
    pub(crate) fn for_tests(persistence: PostgresPersistence) -> Self {
        use crate::adapters::protocols::email::EmailRenderer;
        use crate::application::transport::ports::TransportRenderers;
        let renderers = Arc::new(
            TransportRenderers::new()
                .register(Arc::new(EmailRenderer::new("agents.test")))
                .unwrap(),
        );
        let composer = DeliveryComposer::new(renderers, Arc::new(persistence.clone()));
        let client = Arc::new(crate::adapters::mcp::HttpMcpClient::new(
            crate::adapters::mcp::policy::EndpointPolicy::default(),
        ));
        Self::new(
            persistence,
            "https://agents.test",
            "agents.test",
            composer,
            client,
            Arc::new(crate::adapters::monitoring::InMemoryMonitor::new()),
            1,
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
        let definitions = engine.definitions(Vec::new(), Vec::new());
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
            .service()
            .admit(AdmitWorkflowRequest {
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
        assert_eq!(
            handles.len(),
            3,
            "one slot, the intent worker and the schedule materializer"
        );
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
        assert_eq!(worker_slots(Some("4"), 16).unwrap(), 4);
        assert!(worker_slots(Some("5"), 16).is_err());
        assert!(worker_slots(Some("0"), 64).is_err());
        assert!(worker_slots(Some("17"), 64).is_err());
        assert!(worker_slots(Some("two"), 64).is_err());
    }
}
