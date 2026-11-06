//! Model-store tests use an actually published agent and an admitted model step.
//! The separate native SQL owner fixture remains deliberately independent of driver setup.
use super::*;
use crate::adapters::persistence::{
    credentials::CredentialCipher, workflow::agent_publication::AgentPublicationDirectory,
};
use crate::application::{
    use_cases::company::{CompanyModelConnectionWrite, CompanyPersistence},
    workflow::agent::AgentPreparation,
};

pub(super) async fn fixture(fixture: Fixture, source: Value) -> NativeFixture {
    let (fixture, source) = model_source(fixture, source).await;
    let (fixture, scope) = admit_source(fixture, source).await;
    let policy = LeasePolicy::new(Duration::from_secs(60)).unwrap();
    let claim = fixture
        .persistence()
        .settled()
        .claim_io(scope, worker(), policy)
        .await
        .unwrap()
        .unwrap();
    let launch = fixture.persistence().prepare_agent(&claim).await.unwrap();
    let agent = launch.prepared.agent_id();
    let prepared = launch.prepared.spec(claim.fence).unwrap().prepared;
    let row = model_row(&claim, agent, prepared);
    NativeFixture {
        fixture,
        fence: claim.fence,
        row,
    }
}

async fn model_source(mut fixture: Fixture, mut source: Value) -> (Fixture, Value) {
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let p = &fixture.persistence;
    CompanyPersistence::replace_model_connections_for_user(
        p,
        fixture.target.actor.user_id(),
        fixture.target.company.as_uuid(),
        vec![CompanyModelConnectionWrite {
            provider: "openai".into(),
            api_key: Some("fixture-key".into()),
            models: vec!["fixture".into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    let agent = AgentPersistence::create(
        p,
        fixture.target.company.as_uuid(),
        AgentWrite {
            name: "Checkpoint".into(),
            slug: "checkpoint".into(),
            provider: Some("openai".into()),
            model: Some("fixture".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    fixture.publish_agent(agent.id).await;
    // Preserve routes, retry limits and root budgets. Only the placeholder I/O kind
    // changes, so model accounting is attached to a real declared model execution.
    source["steps"]["start"]["type"] = json!("agent.run");
    source["steps"]["start"]["with"] = json!({"agent":{"literal":"checkpoint"},
        "context":{"literal":"untrusted context"},"output_schema":{"literal":{}}});
    (fixture, source)
}

async fn admit_source(fixture: Fixture, source: Value) -> (AdmissionFixture, ActivationRequest) {
    let p = &fixture.persistence;
    let directory = AgentPublicationDirectory {
        persistence: p.clone(),
        inner: Preflight,
    };
    let service = DefinitionService::new(Preflight, p.clone(), WorkflowSourceDecoder, directory);
    let mut draft = fixture.save(None);
    draft.content.source = source.to_string();
    service.save(draft).await.unwrap();
    let publication = fixture.publish();
    service.publish(publication.clone()).await.unwrap();
    let binding = BindingFixture::from_version(fixture, publication.version);
    let fixture = AdmissionFixture::with_binding(binding).await;
    let admission = fixture
        .prepare(fixture.request("model-fixture", fixture.manual()))
        .await;
    fixture.persistence().admit(&admission).await.unwrap();
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(admission.first_execution_id().as_uuid())
        .fetch_one(fixture.persistence().pool())
        .await
        .unwrap();
    let scope = ActivationRequest {
        company: admission.company_id(),
        run: admission.proposed_run_id(),
        execution: admission.first_execution_id(),
        job: WorkflowJobId(job),
    };
    (fixture, scope)
}

fn model_row(claim: &ClaimedWorkflow, agent: Uuid, prepared: Value) -> Value {
    let scope = claim.fence.scope;
    let run = RunCheckpoint::new(
        RunIdentity {
            company_id: scope.company.as_uuid(),
            task_id: scope.job.0,
            agent_id: agent,
            harness: HarnessKind::Rig,
            provider: "openai".into(),
            model: "fixture".into(),
            response_contract: None,
            capability_fingerprint: "b".repeat(64),
        },
        "untrusted context".into(),
        RigExecutionPolicy::default(),
    )
    .unwrap();
    let fence = claim.fence;
    json!({"id":run.run_id.0,"company_id":scope.company.as_uuid(),"task_id":scope.job.0,"agent_id":agent,
        "workflow_run_id":scope.run.as_uuid(),"workflow_execution_id":scope.execution.as_uuid(),
        "workflow_generation":fence.generation.0,"workflow_worker_id":fence.worker.0,"workflow_attempt":fence.attempt.0,
        "workflow_prepared":prepared,"schema_version":1,"revision":0,"state":"active","checkpoint":run,
        "created_at":chrono::Utc::now(),"updated_at":chrono::Utc::now()})
}
