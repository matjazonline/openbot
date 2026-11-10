use super::*;
use crate::adapters::persistence::{
    credentials::CredentialCipher, workflow::agent_publication::AgentPublicationDirectory,
};
use crate::application::use_cases::{
    agent::{AgentPersistence, AgentWrite},
    company::{CompanyModelConnectionWrite, CompanyPersistence},
};
use crate::application::workflow::agent_history::*;
use crate::entities::correlation::CorrelationId;

struct HistoryThread {
    s: Intents,
    agent: Uuid,
    binding: WorkflowBindingId,
}
impl HistoryThread {
    async fn new() -> Self {
        Box::pin(Self::in_fixture(Fixture::new().await)).await
    }
    fn in_fixture(fixture: Fixture) -> impl std::future::Future<Output = Self> {
        Self::with_policy(fixture, None)
    }
    async fn with_policy(
        fixture: Fixture,
        mcp_error_policy: Option<crate::application::workflow::agent::McpErrorPolicy>,
    ) -> Self {
        let mut s = Intents::in_fixture(fixture).await;
        s.fixture.persistence = PostgresPersistence::with_credential_cipher(
            s.pool().clone(),
            CredentialCipher::for_test(),
        );
        let p = s.p();
        CompanyPersistence::replace_model_connections_for_user(
            p,
            s.owner().user_id(),
            s.fixture.target.company.as_uuid(),
            vec![CompanyModelConnectionWrite {
                provider: "openai".into(),
                api_key: Some("fixture-key".into()),
                models: vec![crate::services::test_support::SCRIPTED_MODEL.into()],
                is_default: true,
            }],
        )
        .await
        .unwrap();
        let agent = AgentPersistence::create(
            p,
            s.fixture.target.company.as_uuid(),
            AgentWrite {
                name: "History".into(),
                slug: "history".into(),
                provider: Some("openai".into()),
                model: Some(crate::services::test_support::SCRIPTED_MODEL.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        s.fixture.publish_agent(agent.id).await;
        let binding = history_contract_binding(
            &mut s,
            "history",
            HistoryContract {
                input: crate::application::workflow::triggers::MessageTriggerInput::schema(),
                output: json!({"type":"string"}),
                mcp_error_policy,
            },
        )
        .await;
        Self {
            s,
            agent: agent.id,
            binding,
        }
    }
    async fn next(&self) -> ClaimedWorkflow {
        self.claim_intent(self.s.intent(TO).await).await
    }
    async fn claim_intent(&self, intent: Uuid) -> ClaimedWorkflow {
        self.s
            .drain(&self.s.admissions(admission(self.s.p())))
            .await;
        #[derive(sqlx::FromRow)]
        struct Job {
            run: Uuid,
            execution: Uuid,
            job: Uuid,
        }
        let job:Job=sqlx::query_as("SELECT run.id AS run,execution.id AS execution,job.id AS job FROM workflow_runs AS run JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id JOIN background_tasks AS job ON job.company_id=run.company_id AND job.workflow_execution_id=execution.id WHERE run.company_id=$1 AND run.binding_id=$2 AND run.trigger_id=$3")
            .bind(self.s.fixture.target.company.as_uuid()).bind(self.binding.as_uuid()).bind(intent).fetch_one(self.s.pool()).await.unwrap();
        self.s
            .p()
            .claim_io(
                crate::application::workflow::activation::ActivationRequest {
                    company: self.s.fixture.target.company,
                    run: RunId::new(job.run),
                    execution: ExecutionId::new(job.execution),
                    job: crate::application::workflow::activation::WorkflowJobId(job.job),
                },
                WorkflowWorkerId(Uuid::new_v4()),
                LeasePolicy::new(Duration::from_secs(60)).unwrap(),
            )
            .await
            .unwrap()
            .unwrap()
    }
    async fn identity(&self, claim: &ClaimedWorkflow) -> AgentConversationIdentity {
        self.s
            .p()
            .resolve_agent_history_identity(ResolveAgentHistoryIdentity {
                fence: claim.fence,
                lease_policy: LeasePolicy::new(Duration::from_secs(60)).unwrap(),
            })
            .await
            .unwrap()
            .unwrap()
    }
}

async fn history_binding(s: &mut Intents, agent_key: &str) -> WorkflowBindingId {
    history_contract_binding(
        s,
        agent_key,
        HistoryContract {
            input: crate::application::workflow::triggers::MessageTriggerInput::schema(),
            output: json!({"type":"string"}),
            mcp_error_policy: None,
        },
    )
    .await
}

struct HistoryContract {
    input: Value,
    output: Value,
    mcp_error_policy: Option<crate::application::workflow::agent::McpErrorPolicy>,
}

async fn history_contract_binding(
    s: &mut Intents,
    agent_key: &str,
    contract: HistoryContract,
) -> WorkflowBindingId {
    let mut source: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    let workflow = WorkflowId::new(Uuid::new_v4());
    source["workflow_id"] = json!(workflow.to_string());
    source["input_schema"] = contract.input;
    source["steps"]["start"]["with"]["agent"] = json!({"literal":agent_key});
    source["steps"]["start"]["with"]["context"] = json!({"ref":"/input"});
    source["steps"]["start"]["with"]["output_schema"] = json!({"literal":contract.output});
    if let Some(policy) = contract.mcp_error_policy {
        source["steps"]["start"]["with"]["mcp_error_policy"] = json!({"literal":policy});
    }
    let p = s.p();
    let definitions = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        AgentPublicationDirectory {
            persistence: p.clone(),
            inner: Preflight,
        },
    );
    let version = VersionId::new(Uuid::new_v4());
    s.fixture
        .publish_source(&definitions, workflow, source.to_string(), version)
        .await
        .unwrap();
    s.version = version;
    s.binding(MessageTrigger::Any, None).await
}

#[tokio::test]
async fn agent_history_identity_real_message_roots_share_thread_across_agent_revisions() {
    let f = Box::pin(HistoryThread::new()).await;
    let first = f.next().await;
    let original = f.identity(&first).await;
    assert!(
        matches!(original.boundary,AgentConversationBoundary::Thread {channel,thread}
        if channel.as_uuid()==f.s.channel && thread.as_uuid()==f.s.thread)
    );
    sqlx::query("UPDATE agents SET system_prompt='New revision, same conversation' WHERE company_id=$1 AND id=$2")
        .bind(f.s.fixture.target.company.as_uuid()).bind(f.agent).execute(f.s.pool()).await.unwrap();
    f.s.fixture.publish_agent(f.agent).await;
    let second = f.next().await;
    assert_ne!(first.fence.scope.run, second.fence.scope.run);
    let (a, b) = tokio::join!(f.identity(&first), f.identity(&second));
    assert_eq!(a, original);
    assert_eq!(a.conversation, b.conversation);
    let revisions:Vec<Uuid>=sqlx::query_scalar("SELECT DISTINCT selection.revision_id FROM run_dependency_manifests AS ownership JOIN agent_manifest_revisions AS selection ON selection.company_id=ownership.company_id AND selection.manifest_id=ownership.manifest_id WHERE ownership.company_id=$1 AND ownership.run_id=ANY($2)")
        .bind(f.s.fixture.target.company.as_uuid()).bind(vec![first.fence.scope.run.as_uuid(),second.fence.scope.run.as_uuid()]).fetch_all(f.s.pool()).await.unwrap();
    assert_eq!(revisions.len(), 2);
    let private_sessions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_agent_sessions WHERE company_id=$1")
            .bind(f.s.fixture.target.company.as_uuid())
            .fetch_one(f.s.pool())
            .await
            .unwrap();
    assert_eq!(private_sessions, 2, "each root retains a separate fallback");
}

#[tokio::test]
async fn agent_history_identity_manual_thread_attachment_is_private_and_scope_updates_refuse() {
    let f = Box::pin(HistoryThread::new()).await;
    let public = f.next().await;
    let original = f.identity(&public).await;
    let request = AdmitWorkflowRequest {
        company_id: f.s.fixture.target.company,
        actor: f.s.owner(),
        binding_id: f.binding,
        association: RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(f.s.channel),
            thread_id: RelatedThreadId::new(f.s.thread),
        },
        trigger: TriggerRef::new(
            f.s.fixture.target.company,
            TriggerId::new(Uuid::new_v4()),
            TriggerSource::Manual,
        )
        .unwrap(),
        correlation_id: CorrelationId::new(),
        idempotency_key: IdempotencyKey::parse("manual-thread").unwrap(),
        input: crate::application::workflow::triggers::MessageTriggerInput::representative(),
        rerun_of: None,
        history_session: None,
        delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
        expected_binding_revision: None,
        admission_claim: None,
    };
    let AdmissionResult::Created(manual) = admission(f.s.p()).admit(request).await.unwrap() else {
        panic!("manual root")
    };
    let proven: bool = sqlx::query_scalar(
        "SELECT thread_input_proven FROM workflow_agent_sessions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.s.fixture.target.company.as_uuid())
    .bind(manual.as_uuid())
    .fetch_one(f.s.pool())
    .await
    .unwrap();
    assert!(!proven);
    let error = sqlx::query(
        "UPDATE agent_history_conversations SET agent_id=$3 WHERE company_id=$1 AND id=$2",
    )
    .bind(original.company.as_uuid())
    .bind(original.conversation.as_uuid())
    .bind(Uuid::new_v4())
    .execute(f.s.pool())
    .await
    .unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514")
    );
    let error=sqlx::query("INSERT INTO agent_history_conversations(company_id,id,agent_id,scope_kind,child_run_id) VALUES($1,$2,$3,'child_branch',$4)")
        .bind(original.company.as_uuid()).bind(Uuid::new_v4()).bind(f.agent).bind(manual.as_uuid()).execute(f.s.pool()).await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514")
    );
}

#[tokio::test]
async fn agent_history_level_thread_stream_survives_audience_drop_and_restore() {
    let f = Box::pin(HistoryThread::new()).await;
    let company = f.s.fixture.target.company.as_uuid();
    let mut conversation = None;
    for expected in [9_i16, 0, 9] {
        let intent = f.s.intent(TO).await;
        sqlx::query("DELETE FROM thread_principals WHERE company_id=$1 AND thread_id=$2")
            .bind(company)
            .bind(f.s.thread)
            .execute(f.s.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO thread_principals(company_id,channel_id,thread_id,principal_id,role) SELECT $1,$2,$3,id,'participant' FROM principals WHERE company_id=$1 AND user_id=$4")
            .bind(company).bind(f.s.channel).bind(f.s.thread).bind(f.s.owner().user_id()).execute(f.s.pool()).await.unwrap();
        if expected == 0 {
            sqlx::query("INSERT INTO thread_principals(company_id,channel_id,thread_id,principal_id,role) SELECT intent.company_id,intent.channel_id,intent.thread_id,message.author_principal_id,'participant' FROM workflow_message_intents AS intent JOIN messages AS message ON message.company_id=intent.company_id AND message.id=intent.message_id WHERE intent.company_id=$1 AND intent.id=$2")
                .bind(company).bind(intent).execute(f.s.pool()).await.unwrap();
        }
        let claim = f.claim_intent(intent).await;
        let ceiling: i16 = sqlx::query_scalar(
            "SELECT knowledge_ceiling::smallint FROM workflow_runs WHERE company_id=$1 AND id=$2",
        )
        .bind(company)
        .bind(claim.fence.scope.run.as_uuid())
        .fetch_one(f.s.pool())
        .await
        .unwrap();
        assert_eq!(ceiling, expected);
        let identity = f.identity(&claim).await;
        assert!(matches!(
            identity.boundary,
            AgentConversationBoundary::Thread { .. }
        ));
        match conversation {
            Some(saved) => assert_eq!(identity.conversation, saved),
            None => conversation = Some(identity.conversation),
        }
        // Observe the next audience with normal native capacity available.
        assert!(
            f.s.p()
                .release_io(
                    claim.fence,
                    LeasePolicy::new(Duration::from_secs(60)).unwrap(),
                    LeaseReleaseCause::LocalInterruption,
                )
                .await
                .unwrap()
        );
    }
}

#[tokio::test]
async fn agent_history_identity_native_cross_tenant_agent_and_thread_anchors_refuse() {
    let f = Box::pin(HistoryThread::new()).await;
    let claim = f.next().await;
    let identity = f.identity(&claim).await;
    let foreign = Intents::in_fixture(
        Fixture::in_database(f.s.fixture._db.clone(), "-history-foreign").await,
    )
    .await;
    for (company, channel, thread) in [
        (
            foreign.fixture.target.company.as_uuid(),
            f.s.channel,
            f.s.thread,
        ),
        (identity.company.as_uuid(), foreign.channel, foreign.thread),
    ] {
        let error=sqlx::query("INSERT INTO agent_history_conversations(company_id,id,agent_id,scope_kind,channel_id,thread_id) VALUES($1,$2,$3,'thread',$4,$5)")
            .bind(company).bind(Uuid::new_v4()).bind(f.agent).bind(channel).bind(thread).execute(f.s.pool()).await.unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("23503")
        );
    }
}

#[tokio::test]
async fn agent_history_identity_actual_agents_in_same_thread_remain_distinct() {
    let mut f = Box::pin(HistoryThread::new()).await;
    let first = f.next().await;
    let original = f.identity(&first).await;
    let agent = AgentPersistence::create(
        f.s.p(),
        f.s.fixture.target.company.as_uuid(),
        AgentWrite {
            name: "Other history".into(),
            slug: "history-other".into(),
            provider: Some("openai".into()),
            model: Some(crate::services::test_support::SCRIPTED_MODEL.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    f.s.fixture.publish_agent(agent.id).await;
    f.binding = history_binding(&mut f.s, "history-other").await;
    f.agent = agent.id;
    let next = f.next().await;
    let other = f.identity(&next).await;
    assert_ne!(original.agent, other.agent);
    assert_ne!(original.conversation, other.conversation);
    assert_eq!(original.boundary, other.boundary);
}

#[path = "agent_history_ordering_tests.rs"]
mod ordering;
