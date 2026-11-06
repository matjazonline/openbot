use super::*;
use crate::application::workflow::{children::WorkflowChildren, completion::WorkflowCompletion};

#[tokio::test]
async fn autonomous_agent_admission_descendant_keeps_the_parent_selection_after_new_publication() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let scope = AgentPublicationScope {
        company: setup.fixture.target.company,
        actor: setup.fixture.target.actor,
        agent: AgentId::new(setup.agent),
    };
    let original = p.active_agent_revision(scope).await.unwrap().unwrap();
    let parent = parent_tree(&setup).await;
    let AdmissionResult::Created(run) = admit_graph(&setup, &parent, "retained-child-selection")
        .await
        .unwrap()
    else {
        panic!("parent admitted")
    };
    let parent_claim = claim_run(&setup, run).await;
    let initial = p.prepare_agent(&parent_claim).await.unwrap();
    let manifest = initial.prepared.selection().manifest_id();
    let classified = run_model(&setup, &parent_claim, ModelStep::Classify, "billing").await;
    let next = p
        .complete_io(crate::application::workflow::lease::FencedWorkflowResult {
            fence: parent_claim.fence,
            output: classified,
            policy: lease(),
        })
        .await
        .unwrap()
        .unwrap()
        .successor
        .unwrap();
    AgentPersistence::update(
        p,
        setup.agent,
        AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            system_prompt: Some("A later publication must not replace the captured prompt.".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let current = setup.fixture.publish_agent(setup.agent).await;
    assert_ne!(current.id, original.id);
    p.park_child(next)
        .await
        .unwrap()
        .expect("explicit child parks the parent");
    let child:Uuid=sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id=$1 AND parent_run_id=$2 AND parent_execution_id=$3")
        .bind(scope.company.as_uuid()).bind(run.as_uuid()).bind(next.execution.as_uuid()).fetch_one(p.pool()).await.unwrap();
    let claim = claim_run(&setup, RunId::new(child)).await;
    let child = p.prepare_agent(&claim).await.unwrap();
    assert_eq!(child.prepared.selection().manifest_id(), manifest);
    assert_eq!(child.prepared.selection().revision().id, original.id);
    assert_eq!(
        child.prepared.selection().agent().instructions,
        original.definition.instructions
    );
    assert_ne!(child.prepared.selection().revision().id, current.id);
    assert_eq!(
        run_model(&setup, &claim, ModelStep::AgentRun, "done").await,
        json!("done")
    );
}

async fn parent_tree(setup: &AgentSetup) -> GraphWorkflow {
    let key = AgentKey::parse("assistant").unwrap();
    let child = publish_graph(setup, model_source(&key, ModelStep::AgentRun), json!({})).await;
    let mut source = model_source(&key, ModelStep::AgentRun);
    source["input_schema"] = json!({"type":"object"});
    source["output_schema"] = json!({"type":"string"});
    source["steps"]["start"] = json!({"type":"ai.classify","with":{
        "agent":{"literal":"assistant"},"context":{"literal":{"message":"I was charged twice"}},
        "output_schema":{"literal":{"type":"string","enum":["billing","sales"]}}},"routes":{"success":"child"}});
    let example: Value =
        serde_json::from_str(&registry::example("workflow.call").unwrap().source).unwrap();
    source["steps"]["child"] = example["steps"]["start"].clone();
    source["steps"]["child"]["child_workflow_id"] = json!(child.workflow.to_string());
    source["steps"]["child"]["child_version_id"] = json!(child.version.to_string());
    source["steps"]["child"]["with"]["input"] = json!({"literal":{}});
    source["steps"]["child"]["with"]["params"] = json!({"literal":{}});
    publish_graph(setup, source, json!({})).await
}

async fn claim_run(setup: &AgentSetup, run: RunId) -> ClaimedWorkflow {
    #[derive(sqlx::FromRow)]
    struct Job {
        execution: Uuid,
        job: Uuid,
    }
    let company = setup.fixture.target.company;
    let p = &setup.fixture.persistence;
    let job:Job=sqlx::query_as("SELECT execution.id AS execution,job.id AS job FROM workflow_executions AS execution JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id WHERE execution.company_id=$1 AND execution.run_id=$2")
        .bind(company.as_uuid()).bind(run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    let scope = crate::application::workflow::activation::ActivationRequest {
        company,
        run,
        execution: ExecutionId::new(job.execution),
        job: crate::application::workflow::activation::WorkflowJobId(job.job),
    };
    p.claim_io(
        scope,
        crate::application::workflow::lease::WorkflowWorkerId(Uuid::new_v4()),
        lease(),
    )
    .await
    .unwrap()
    .unwrap()
}

async fn run_model(
    setup: &AgentSetup,
    claim: &ClaimedWorkflow,
    kind: ModelStep,
    value: &str,
) -> Value {
    let llm = scripted_llm(vec![LlmTurn::text(serde_json::to_string(value).unwrap())]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(setup);
    let kind = WorkflowStepKind(kind.kind().into());
    let output = Box::pin(runtime.execute(&kind, claim))
        .await
        .completed_result()
        .unwrap();
    assert_eq!(llm.finish().await.unwrap(), 1);
    output
}
