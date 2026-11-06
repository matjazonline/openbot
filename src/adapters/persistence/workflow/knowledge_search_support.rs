use super::*;
use crate::adapters::persistence::workflow::tests::binding_tests::BindingFixture;
use crate::{
    application::{
        knowledge::{KnowledgeLibrary, test_support::DeterministicTextEmbedder},
        workflow::{
            activation::{ActivationRequest, WorkflowJobId},
            agent::{AgentToolDispatch, AgentToolObservation},
            knowledge::{KnowledgeSearchRuntime, KnowledgeToolCall},
            lifecycle::SetBindingActivity,
        },
    },
    services::harness::runs::RunCheckpoint,
};

pub(super) struct SearchFixture {
    pub(super) fixture: Fixture,
    pub(super) agent: Uuid,
    pub(super) claim: ClaimedWorkflow,
    pub(super) model: Arc<DeterministicTextEmbedder>,
    pub(super) library: Arc<KnowledgeLibrary>,
}

impl SearchFixture {
    pub(super) async fn new(level: AccessLevel) -> Self {
        let mut fixture = Fixture::new().await;
        fixture.persistence = PostgresPersistence::with_credential_cipher(
            fixture.persistence.pool().clone(),
            CredentialCipher::for_test(),
        );
        sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
            .execute(fixture.persistence.pool())
            .await
            .unwrap();
        let actor = crate::adapters::persistence::workflow::tests::message_tests::support::member(
            &fixture, "admin",
        )
        .await;
        sqlx::query(
            "UPDATE company_members SET access_level=$3 WHERE company_id=$1 AND user_id=$2",
        )
        .bind(fixture.target.company.as_uuid())
        .bind(actor)
        .bind(level.as_i16())
        .execute(fixture.persistence.pool())
        .await
        .unwrap();
        let agent = configure_agent(&fixture).await;
        let version = publish(&fixture).await;
        let binding = BindingFixture::from_version(fixture, version);
        let configured = binding
            .service()
            .configure(binding.request(None))
            .await
            .unwrap();
        binding
            .service()
            .activate(SetBindingActivity {
                target: binding.target,
                expected: configured.revision,
            })
            .await
            .unwrap();
        let claim = admit(&binding, WorkflowActor::authenticated(actor).unwrap()).await;
        let model = Arc::new(DeterministicTextEmbedder::default());
        let library = Arc::new(KnowledgeLibrary::new(
            Arc::new(binding.fixture.persistence.clone()),
            model.clone(),
        ));
        Self {
            fixture: binding.fixture,
            agent,
            claim,
            model,
            library,
        }
    }

    pub(super) fn p(&self) -> &PostgresPersistence {
        &self.fixture.persistence
    }

    pub(super) fn knowledge(&self) -> KnowledgeSearchRuntime {
        let store = Arc::new(self.p().clone());
        KnowledgeSearchRuntime::new(
            self.library.clone(),
            store.clone(),
            store,
            KnowledgeSearchLimit::default(),
        )
    }

    pub(super) fn runtime(&self) -> WorkflowAgentRuntime<PostgresPersistence, AgentActionTools> {
        let p = self.p();
        let http = HttpRuntime::with_approvals(
            p.clone(),
            HttpDestinationPolicy::default(),
            lease(),
            Some(notices(p)),
        );
        let mcp = McpRuntime::new(
            p.clone(),
            Arc::new(HttpMcpClient::new(EndpointPolicy::default())),
            lease(),
        );
        let message = MessageRuntime::new(p.clone(), composer(p), "agents.test".into(), lease());
        let tools = AgentActionTools::new(Arc::new(http), Arc::new(mcp), Arc::new(message))
            .with_knowledge(Arc::new(self.knowledge()));
        WorkflowAgentRuntime::new(
            ProviderRegistry::standard().unwrap(),
            p.clone(),
            tools,
            Some(notices(p)),
            lease(),
        )
    }

    pub(super) async fn prepare(&self, arguments: Value) -> KnowledgeToolCall {
        let llm = scripted_llm(vec![LlmTurn::tool_call(SEARCH_KNOWLEDGE, arguments)]).await;
        register_scripted_agent_base_url(self.agent, &llm.base_url);
        let runtime = WorkflowAgentRuntime::new(
            ProviderRegistry::standard().unwrap(),
            self.p().clone(),
            PreparedOnly,
            Some(notices(self.p())),
            lease(),
        );
        let outcome =
            Box::pin(runtime.execute(&WorkflowStepKind("agent.run".into()), &self.claim)).await;
        assert!(outcome.completed_result().is_err());
        assert_eq!(llm.finish().await.unwrap(), 1);
        let checkpoint = self.checkpoint().await;
        let pending = checkpoint.pending_call().unwrap();
        let scope = self.claim.fence.scope;
        let prepared: Value = sqlx::query_scalar("SELECT workflow_prepared FROM task_harness_runs WHERE company_id=$1 AND workflow_run_id=$2 AND workflow_execution_id=$3 AND task_id=$4")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0).fetch_one(self.p().pool()).await.unwrap();
        let tools: Vec<crate::application::workflow::agent::PreparedTool> =
            serde_json::from_value(prepared["tools"].clone()).unwrap();
        let tool = tools
            .iter()
            .find(|tool| tool.name.as_str() == SEARCH_KNOWLEDGE)
            .unwrap();
        KnowledgeToolCall {
            origin: crate::application::agent::ToolCallOrigin::Agent(tool.origin),
            call: crate::application::workflow::actions::ModelToolCallId::parse(
                pending.invocation_id.0.to_string(),
            )
            .unwrap(),
            arguments: pending.arguments.clone(),
        }
    }

    pub(super) async fn checkpoint(&self) -> RunCheckpoint {
        let scope = self.claim.fence.scope;
        let value: Value = sqlx::query_scalar("SELECT checkpoint FROM task_harness_runs WHERE company_id=$1 AND workflow_run_id=$2 AND workflow_execution_id=$3 AND task_id=$4")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0)
            .fetch_one(self.p().pool()).await.unwrap();
        serde_json::from_value(value).unwrap()
    }

    pub(super) async fn snapshot(&self) -> Value {
        let scope = self.claim.fence.scope;
        sqlx::query_scalar("SELECT jsonb_build_object('content',run.content_level,'checkpoint',harness.checkpoint,
            'proofs',(SELECT count(*) FROM workflow_knowledge_consumptions AS proof WHERE proof.company_id=run.company_id AND proof.run_id=run.id))
            FROM workflow_runs AS run JOIN task_harness_runs AS harness ON harness.company_id=run.company_id AND harness.workflow_run_id=run.id
            WHERE run.company_id=$1 AND run.id=$2 AND harness.workflow_execution_id=$3 AND harness.task_id=$4")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0)
            .fetch_one(self.p().pool()).await.unwrap()
    }

    pub(super) async fn upload_sales(&self) {
        self.library
            .upload(
                KnowledgeScope {
                    company: self.fixture.target.company,
                    actor_id: self.fixture.target.actor.user_id(),
                },
                vec![
                    KnowledgeUploadFile::from_upload(
                        "0_sales.md",
                        b"AX-4401 public list price 100".to_vec(),
                    )
                    .unwrap(),
                    KnowledgeUploadFile::from_upload(
                        "5_sales.md",
                        b"AX-4410 PRIVATE FLOOR 60".to_vec(),
                    )
                    .unwrap(),
                ],
            )
            .await
            .unwrap();
    }
}

async fn configure_agent(f: &Fixture) -> Uuid {
    let agent = AgentPersistence::create(
        &f.persistence,
        f.target.company.as_uuid(),
        AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            tool_bindings: Some(vec![
                crate::entities::agent_capability::AgentToolBinding::builtin(
                    crate::entities::agent_capability::AgentBuiltinTool::SearchKnowledge,
                ),
            ]),
            system_prompt: Some("Answer with the requested JSON string.".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    CompanyPersistence::replace_model_connections_for_user(
        &f.persistence,
        f.target.actor.user_id(),
        f.target.company.as_uuid(),
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: Some("scripted-key".into()),
            models: vec![SCRIPTED_MODEL.into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    f.publish_agent(agent.id).await;
    agent.id
}

async fn publish(f: &Fixture) -> VersionId {
    let directory = AgentPublicationDirectory {
        persistence: f.persistence.clone(),
        inner: Preflight,
    };
    let service = DefinitionService::new(
        Preflight,
        f.persistence.clone(),
        WorkflowSourceDecoder,
        directory,
    );
    let mut source: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    source["workflow_id"] = json!(f.target.workflow.to_string());
    let version = VersionId::new(Uuid::new_v4());
    f.publish_source(&service, f.target.workflow, source.to_string(), version)
        .await
        .unwrap();
    version
}

async fn admit(binding: &BindingFixture, actor: WorkflowActor) -> ClaimedWorkflow {
    let f = &binding.fixture;
    let p = &f.persistence;
    let result = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
        .admit(AdmitWorkflowRequest {
            company_id: f.target.company,
            actor,
            association: RelatedAssociation::Company,
            trigger: TriggerRef::new(
                f.target.company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation_id: CorrelationId::new(),
            binding_id: binding.target.binding,
            idempotency_key: IdempotencyKey::parse("search").unwrap(),
            input: json!({}),
            rerun_of: None,
            delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
            expected_binding_revision: None,
            admission_claim: None,
        })
        .await
        .unwrap();
    let AdmissionResult::Created(run) = result else {
        panic!("new run")
    };
    #[derive(sqlx::FromRow)]
    struct Activation {
        execution: Uuid,
        job: Uuid,
    }
    let row = sqlx::query_as::<_,Activation>("SELECT execution.id AS execution,job.id AS job FROM workflow_executions AS execution JOIN background_tasks AS job ON job.workflow_execution_id=execution.id AND job.company_id=execution.company_id WHERE execution.company_id=$1 AND execution.run_id=$2")
        .bind(f.target.company.as_uuid()).bind(run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    p.claim_io(
        ActivationRequest {
            company: f.target.company,
            run,
            execution: ExecutionId::new(row.execution),
            job: WorkflowJobId(row.job),
        },
        WorkflowWorkerId(Uuid::new_v4()),
        lease(),
    )
    .await
    .unwrap()
    .unwrap()
}

struct PreparedOnly;

#[async_trait]
impl AgentToolDispatch for PreparedOnly {
    async fn dispatch(
        &self,
        _: crate::application::workflow::lease::WorkflowFence,
        _: &crate::application::workflow::agent::PreparedTool,
        _: crate::application::workflow::actions::ModelToolCallId,
        _: Value,
    ) -> AppResult<AgentToolObservation> {
        Err(AppError::Timeout(
            "Test paused after canonical invocation preparation".into(),
        ))
    }
}

pub(super) fn copy_call(call: &KnowledgeToolCall) -> KnowledgeToolCall {
    KnowledgeToolCall {
        origin: call.origin,
        call: call.call.clone(),
        arguments: call.arguments.clone(),
    }
}
