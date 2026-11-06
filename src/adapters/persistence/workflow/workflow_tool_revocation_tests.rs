//! Revoke stops, re-pin doesn't (P07-Q2, 07-D2 §D2.2, F024–F026): published workflows keep the
//! version they froze until republished; revoking the name, or pointing it at another workflow,
//! refuses new runs and new calls while a call parked before still delivers.
use super::*;
use crate::application::workflow::{
    AdmissionResult,
    activation::ActivationRequest,
    agent::AgentToolObservation,
    children::WorkflowChildren,
    lifecycle::{PublicationDependencies, PublicationDirectory},
    tools::{WorkflowToolWrite, WorkflowTools},
};

use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};

use super::workflow_tool_support::*;

fn child_workflow() -> WorkflowId {
    WorkflowId::new(Uuid::from_u128(CHILD))
}

/// The echo child's next version, which answers differently.
fn echo_child_v2() -> Value {
    let mut child = echo_child();
    child["steps"]["answer"]["with"]["value"]["object"]["answer"] = json!({"literal":"from v2"});
    child
}

/// The version the call's child of `parent` runs.
async fn child_version(setup: &ToolSetup, parent: ActivationRequest) -> VersionId {
    let child = setup.child_of(parent).await;
    VersionId::new(
        sqlx::query_scalar("SELECT version_id FROM workflow_runs WHERE id=$1")
            .bind(child.run.as_uuid())
            .fetch_one(setup.persistence().pool())
            .await
            .unwrap(),
    )
}

/// Dispatch a call with the bound parent's frozen pin in a new run; it must park.
async fn parked_call(setup: &ToolSetup, key: &str) -> ActivationRequest {
    let claim = setup.claim(key).await;
    let call = setup
        .call(setup.initial_pin(), json!({"question": key}), &claim)
        .await;
    assert!(matches!(
        setup.dispatch(&claim, &call).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    claim.fence.scope
}

/// Finish a parked call's child and replay the call in the continued job; frees its capacity.
async fn delivered(setup: &ToolSetup, scope: ActivationRequest, question: &str) -> Value {
    let p = setup.persistence();
    setup.finish_child(scope).await;
    assert_eq!(p.resume_wait(scope).await.unwrap(), WaitProgress::Continued);
    let continued = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let pin = setup.initial_pin();
    // The replay matches the saved call by id, so read it back from the call's wait.
    let call: Uuid = sqlx::query_scalar(
        "SELECT child_tool_call_id FROM workflow_waits WHERE run_id=$1 AND child_tool_call_id IS NOT NULL",
    )
    .bind(scope.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let mut replay = setup
        .call(pin, json!({"question": question}), &continued)
        .await;
    replay.call = ToolCallId::new(call);
    let AgentToolObservation::Completed(result) =
        setup.dispatch(&continued, &replay).await.unwrap()
    else {
        panic!("a finished call answers");
    };
    setup.cancel(scope, &format!("{question}-done")).await;
    result
}

/// A new run of the bound parent is refused, as for any unavailable dependency.
async fn refused_admission(setup: &ToolSetup, key: &str) {
    let roots = async || {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
            .bind(setup.fixture.target.company.as_uuid())
            .fetch_one(setup.persistence().pool())
            .await
            .unwrap()
    };
    let before = roots().await;
    let error = setup
        .admit(setup.target, TriggerId::new(Uuid::new_v4()), key)
        .await
        .unwrap_err();
    match &error {
        AppError::NotFound(_) => {}
        AppError::BadRequest(message) if message.contains("Selected agent authority") => {}
        _ => panic!("unexpected admission error: {error:?}"),
    }
    assert_eq!(
        roots().await,
        before,
        "a revoked dependency cannot leave a partial run"
    );
}

/// A running agent's next call: refused as revoked, with no child.
async fn refused_call(setup: &ToolSetup, claim: &ClaimedWorkflow) {
    let llm = scripted_llm(vec![]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let WorkflowHandlerOutcome::Completed(Err(failure)) =
        Box::pin(setup.runtime().execute(&kind(), claim)).await
    else {
        panic!("current authority must refuse the agent")
    };
    assert_eq!(failure.failure.code().as_str(), "agent.rejected");
    assert_eq!(
        llm.finish().await.unwrap(),
        0,
        "a revoked source cannot enter a provider"
    );
    assert_eq!(
        children(setup.persistence(), claim.fence.scope.run).await,
        0
    );
}

/// Captures through `inner`, then re-pins the tool before the publication commits.
struct RepinAfterCapture<D> {
    inner: D,
    persistence: PostgresPersistence,
    company: CompanyId,
    version: VersionId,
}
#[async_trait]
impl<D: PublicationDirectory> PublicationDirectory for RepinAfterCapture<D> {
    async fn capture(
        &self,
        actor: WorkflowActor,
        draft: &CompanyWorkflowDraft,
    ) -> AppResult<PublicationDependencies> {
        let captured = self.inner.capture(actor, draft).await?;
        let current = self
            .persistence
            .workflow_tools(self.company, actor)
            .await?
            .into_iter()
            .find(|tool| tool.name.as_str() == TOOL)
            .unwrap();
        self.persistence
            .register_workflow_tool(
                self.company,
                actor,
                WorkflowToolWrite {
                    name: current.name,
                    workflow: current.workflow,
                    version: self.version,
                    params: current.params,
                    expected: Some(current.revision),
                },
            )
            .await?;
        Ok(captured)
    }
}

#[tokio::test]
async fn workflow_tool_repin_and_archive_leave_published_workflows_on_their_frozen_version() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let v2 = publish_child(
        &setup.fixture,
        child_workflow(),
        echo_child_v2(),
        "child-v2",
    )
    .await;
    setup.repin(child_workflow(), v2).await;
    // The published parent still admits runs and its agent calls the version it froze.
    let scope = parked_call(&setup, "after-repin").await;
    assert_eq!(child_version(&setup, scope).await, setup.child);
    assert_eq!(
        delivered(&setup, scope, "after-repin").await,
        json!({"choice": "approve", "answer": "after-repin"})
    );
    // Workflow republication keeps the durable reference; the agent revision owns its pin.
    let republished = publish_parent(&setup.fixture, "republish").await.unwrap();
    let reference = p
        .selectable_version(setup.fixture.target.company, republished)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        reference.snapshots().agents[0].agent_id.as_uuid(),
        setup.agent
    );
    assert_eq!(setup.initial_pin().version_id, setup.child);
    // A registration repin cannot change the durable workflow agent reference.
    let source = registry::example("agent.run").unwrap().source;
    let racing = RepinAfterCapture {
        inner: child_publication::ChildPublicationDirectory {
            persistence: p.clone(),
            inner: agent_publication::AgentPublicationDirectory {
                persistence: p.clone(),
                inner: message::MessagePublicationDirectory {
                    persistence: p.clone(),
                },
            },
        },
        persistence: p.clone(),
        company: setup.fixture.target.company,
        version: setup.child,
    };
    let captured = publish_through(
        &setup.fixture,
        setup.fixture.target.workflow,
        "repinned-during-publish",
        source,
        racing,
    )
    .await;
    let captured = captured.unwrap();
    let bundle = p
        .selectable_version(setup.fixture.target.company, captured)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bundle.snapshots().agents[0].agent_id.as_uuid(), setup.agent);
    assert_eq!(setup.registration().await.version, setup.child);
    // Archiving the pinned workflow does not stop the published parent; a new publication
    // cannot capture it.
    let draft = p
        .draft(setup.fixture.target.company, child_workflow())
        .await
        .unwrap()
        .unwrap();
    setup
        .fixture
        .service()
        .archive(
            DefinitionTarget {
                workflow: child_workflow(),
                ..setup.fixture.target
            },
            draft.draft.revision(),
        )
        .await
        .unwrap();
    let scope = parked_call(&setup, "after-archive").await;
    assert_eq!(child_version(&setup, scope).await, setup.child);
    assert_eq!(
        delivered(&setup, scope, "after-archive").await["answer"],
        "after-archive"
    );
    publish_parent(&setup.fixture, "after-archive")
        .await
        .unwrap();
    use crate::application::agent::{AgentPublicationScope, AgentPublications};
    let error = p
        .preview_agent(AgentPublicationScope {
            company: setup.fixture.target.company,
            actor: setup.fixture.target.actor,
            agent: crate::entities::agent_capability::AgentId::new(setup.agent),
        })
        .await
        .err()
        .expect("a new agent publication cannot freeze an archived draft target");
    assert!(
        matches!(error,AppError::BadRequest(ref message) if message.contains("archived or missing")),
        "{error:?}"
    );
}

#[path = "workflow_tool_current_authority_tests.rs"]
mod current_authority;

/// F024: a stored child bundle is held to the run rule, not the publication rule. A new parent of
/// the agent workflow publishes, admits and runs it after the tool was re-pinned, and the
/// stored agent's call, made from inside a child run, starts the version it froze.
#[tokio::test]
async fn workflow_tool_stored_children_keep_their_frozen_pin_until_the_tool_is_revoked() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let caller = |workflow: u128| {
        let mut source: Value =
            serde_json::from_str(&registry::example("workflow.call").unwrap().source).unwrap();
        source["workflow_id"] = json!(Uuid::from_u128(workflow).to_string());
        source["steps"]["start"]["child_workflow_id"] =
            json!(setup.fixture.target.workflow.to_string());
        source["steps"]["start"]["child_version_id"] = json!(setup.parent.to_string());
        source
    };
    let v2 = publish_child(
        &setup.fixture,
        child_workflow(),
        echo_child_v2(),
        "child-v2",
    )
    .await;
    setup.repin(child_workflow(), v2).await;
    // A new parent of the stored agent workflow publishes; the stored child keeps its pin.
    let outer = setup
        .fixture
        .publish_workflow(WorkflowId::new(Uuid::from_u128(0x0a01)), caller(0x0a01))
        .await;
    let bundle = p
        .selectable_version(setup.fixture.target.company, outer)
        .await
        .unwrap()
        .unwrap();
    let stored = &bundle.children()[&setup.parent];
    assert_eq!(stored.snapshots().agents[0].agent_id.as_uuid(), setup.agent);
    assert!(
        stored
            .snapshots()
            .tools
            .iter()
            .all(|tool| tool.contract.name.as_str() != TOOL)
    );
    assert!(stored.children().is_empty());

    // It binds and admits, and its child's agent calls the tool from inside the child run.
    let target = Box::pin(bound(&setup.fixture, outer)).await;
    let scope = admitted_as(&setup.fixture, target, RelatedAssociation::Company, "outer").await;
    p.park_child(scope).await.unwrap().unwrap();
    let agent = setup.child_of(scope).await;
    let claim = p
        .claim_io(agent, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(DECLARED, json!({"question": "From a child run?"})),
        LlmTurn::text("\"nested\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = setup.runtime();
    let WorkflowHandlerOutcome::Suspended(_) = Box::pin(runtime.execute(&kind(), &claim)).await
    else {
        panic!("the child's agent parks on its call");
    };
    assert_eq!(child_version(&setup, agent).await, setup.child);
    setup.finish_child(agent).await;
    assert_eq!(p.resume_wait(agent).await.unwrap(), WaitProgress::Continued);
    let continued = p
        .claim_io(agent, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let WorkflowHandlerOutcome::Completed(Ok(output)) =
        Box::pin(runtime.execute(&kind(), &continued)).await
    else {
        panic!("the child's agent resumes its call and answers");
    };
    assert_eq!(output, json!("nested"));
    assert_eq!(
        saved_result(p, agent).await,
        json!({"choice": "approve", "answer": "From a child run?"})
    );
    assert_eq!(llm.finish().await.unwrap(), 2);
    assert_eq!(children(p, agent.run).await, 1);

    // Publication still references the historical child; admission must check its agent closure.
    setup.revoke().await;
    let version = VersionId::new(Uuid::new_v4());
    setup
        .fixture
        .publish_source(
            &setup.fixture.service(),
            WorkflowId::new(Uuid::from_u128(0x0a02)),
            caller(0x0a02).to_string(),
            version,
        )
        .await
        .unwrap();
    let target = Box::pin(bound(&setup.fixture, version)).await;
    assert!(
        setup
            .admit(
                target,
                TriggerId::new(Uuid::new_v4()),
                "revoked-stored-agent"
            )
            .await
            .is_err()
    );
    assert_eq!(children(p, agent.run).await, 1);
}
