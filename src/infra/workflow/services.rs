//! HTTP and application command services; this view owns no handler or worker loop.
use super::*;
pub struct WorkflowServices {
    pub(super) persistence: PostgresPersistence,
    pub(super) notices: DecisionNotices,
    pub(super) templates: ProvisioningDeployment,
    pub(super) monitoring: Arc<dyn MonitoringService>,
    pub(super) worker_wakeups: Arc<Notify>,
}
impl WorkflowServices {
    pub fn agent_authoring_reads(&self) -> &dyn crate::application::agent::AgentAuthoringReads {
        &self.persistence
    }

    pub fn agent_publications(&self) -> &dyn crate::application::agent::AgentPublications {
        &self.persistence
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

    pub(super) fn schedule_materializer(
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

    pub fn channel_replies(
        &self,
    ) -> &dyn crate::application::workflow::channel_reply::ChannelReplies {
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

    /// An isolated sample receives only inert validated content and scripted data.
    pub fn sample(
        &self,
        sample: crate::application::workflow::sample::PreparedSample,
    ) -> crate::app_error::AppResult<crate::application::workflow::sample::SampleReport> {
        crate::application::workflow::sample::execute(sample)
    }
    pub fn sample_capture(
        &self,
        http: Vec<HttpPublicationSelection>,
        mcp: Vec<McpPublicationSelection>,
    ) -> crate::adapters::persistence::workflow::SampleCapture {
        crate::adapters::persistence::workflow::SampleCapture {
            persistence: self.persistence.clone(),
            http,
            mcp,
            refused_kinds: UNHANDLED_STEP_KINDS,
        }
    }

    pub fn decisions(&self) -> &dyn crate::application::workflow::decisions::WorkflowDecisions {
        &self.persistence
    }

    pub fn http_connections(&self) -> &dyn crate::application::workflow::http::HttpConnections {
        &self.persistence
    }

    pub fn jev_connections(&self) -> &dyn crate::application::workflow::jev::JevConnections {
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
    pub fn onboarding_persistence(
        &self,
    ) -> Arc<dyn crate::use_cases::onboarding::OnboardingPersistence> {
        Arc::new(
            crate::adapters::persistence::onboarding::PostgresOnboarding::new(
                self.persistence.clone(),
                self.templates.clone(),
            ),
        )
    }

    /// Company-agent creation with each agent's default desk (01DEP-P03-D2), over the same
    /// provisioning templates.
    pub fn company_agents(&self) -> Arc<dyn crate::use_cases::agent::CompanyAgents> {
        Arc::new(
            crate::adapters::persistence::company_agent::CompanyAgentService::new(
                self.persistence.clone(),
                self.templates.clone(),
            ),
        )
    }

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
}
