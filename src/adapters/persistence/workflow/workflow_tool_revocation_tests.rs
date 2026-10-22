//! Revoke stops, re-pin doesn't (P07-Q2, 07-D2 §D2.2, F024–F026): published workflows keep the
//! version they froze until republished; revoking the name, or pointing it at another workflow,
//! refuses new runs and new calls while a call parked before still delivers.
use super::*;
use crate::application::workflow::{
    AdmissionResult,
    activation::ActivationRequest,
    agent::AgentToolObservation,
    children::WorkflowChildren,
    lifecycle::{LifecycleError, PublicationDependencies, PublicationDirectory},
    tools::{TOOL_REVOKED, WorkflowToolWrite, WorkflowTools},
};

use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};
use std::collections::BTreeMap;

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
    let call = setup.call(
        setup.frozen_pin(setup.parent).await,
        json!({"question": key}),
    );
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
    let pin = setup.frozen_pin(setup.parent).await;
    // The replay matches the saved call by id, so read it back from the call's wait.
    let call: Uuid = sqlx::query_scalar(
        "SELECT child_tool_call_id FROM workflow_waits WHERE run_id=$1 AND child_tool_call_id IS NOT NULL",
    )
    .bind(scope.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let mut replay = setup.call(pin, json!({"question": question}));
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
    let refused = setup
        .admit(setup.target, TriggerId::new(Uuid::new_v4()), key)
        .await;
    assert!(matches!(refused, Err(AppError::NotFound(_))), "{refused:?}");
}

/// A running agent's next call: refused as revoked, with no child.
async fn refused_call(setup: &ToolSetup, claim: &ClaimedWorkflow) {
    let call = setup.call(
        setup.frozen_pin(setup.parent).await,
        json!({"question": "q"}),
    );
    let AgentToolObservation::Completed(refusal) = setup.dispatch(claim, &call).await.unwrap()
    else {
        panic!("a refused call answers at once");
    };
    assert_eq!(refusal["error"]["code"], TOOL_REVOKED);
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
    // A republish freezes the new pin.
    let republished = publish_parent(&setup.fixture, "republish").await.unwrap();
    assert_eq!(setup.frozen_pin(republished).await.version_id, v2);
    // A re-pin between capture and publish refuses that publication.
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
    let refused = publish_through(
        &setup.fixture,
        setup.fixture.target.workflow,
        "repinned-during-publish",
        source,
        racing,
    )
    .await;
    assert!(
        matches!(
            refused,
            Err(LifecycleError::Application(AppError::NotFound(_)))
        ),
        "{:?}",
        refused.err().map(|error| format!("{error:?}"))
    );
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
    let message = match publish_parent(&setup.fixture, "after-archive").await {
        Err(LifecycleError::Application(AppError::BadRequest(message))) => message,
        other => panic!("{:?}", other.err().map(|error| format!("{error:?}"))),
    };
    assert!(message.contains("archived or missing"), "{message}");
}

#[tokio::test]
async fn workflow_tool_revocation_refuses_new_runs_and_calls_while_parked_calls_deliver() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let (company, admin) = (setup.fixture.target.company, setup.fixture.target.actor);
    // F021: admission and its replay hold for a workflow whose agent grants a workflow tool.
    let trigger = TriggerId::new(Uuid::new_v4());
    let AdmissionResult::Created(run) = setup.admit(setup.target, trigger, "replay").await.unwrap()
    else {
        panic!("admits");
    };
    assert_eq!(
        setup.admit(setup.target, trigger, "replay").await.unwrap(),
        AdmissionResult::Replayed(run)
    );

    // A call parked before the revocation, and a running agent that has not called yet.
    let parked = parked_call(&setup, "parked-before-revoke").await;
    let running = setup.claim("running-at-revoke").await;
    setup.revoke().await;
    refused_admission(&setup, "after-revoke").await;
    let bindings = BindingService::new(Preflight, p.clone(), p.clone(), p.clone());
    let target = BindingTarget {
        company,
        actor: admin,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    let readiness = match bindings
        .configure(ConfigureBinding {
            target,
            expected: None,
            association: RelatedAssociation::Company,
            version: setup.parent,
            params: json!({}),
            resources: BTreeMap::new(),
        })
        .await
    {
        Err(error) => Err(error),
        Ok(state) => bindings
            .activate(SetBindingActivity {
                target,
                expected: state.revision,
            })
            .await
            .map(|_| ()),
    };
    assert!(
        matches!(
            readiness,
            Err(LifecycleError::Application(AppError::NotFound(_)))
        ),
        "{:?}",
        readiness.err().map(|error| format!("{error:?}"))
    );
    refused_call(&setup, &running).await;
    setup.cancel(running.fence.scope, "running-at-revoke").await;
    assert_eq!(
        delivered(&setup, parked, "parked-before-revoke").await["answer"],
        "parked-before-revoke"
    );

    // Registering the name again for the same workflow re-enables the frozen pin.
    p.register_workflow_tool(
        company,
        admin,
        WorkflowToolWrite {
            name: TypeName::parse(TOOL).unwrap(),
            workflow: child_workflow(),
            version: setup.child,
            params: json!({"reviewer": setup.reviewer.to_string()}),
            expected: None,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        setup
            .admit(
                setup.target,
                TriggerId::new(Uuid::new_v4()),
                "re-registered"
            )
            .await
            .unwrap(),
        AdmissionResult::Created(_)
    ));

    // F025/F026: pointing the name at another workflow refuses the old pin like a revocation.
    let other = WorkflowId::new(Uuid::from_u128(CHILD + 1));
    let other_version = setup.fixture.publish_workflow(other, echo_child()).await;
    let parked = parked_call(&setup, "parked-before-repoint").await;
    let running = setup.claim("running-at-repoint").await;
    setup.repin(other, other_version).await;
    refused_admission(&setup, "after-repoint").await;
    refused_call(&setup, &running).await;
    setup
        .cancel(running.fence.scope, "running-at-repoint")
        .await;
    assert_eq!(
        delivered(&setup, parked, "parked-before-repoint").await["answer"],
        "parked-before-repoint"
    );

    // Back on the same workflow at another version: the next call runs the frozen version.
    let v2 = publish_child(
        &setup.fixture,
        child_workflow(),
        echo_child_v2(),
        "child-v2",
    )
    .await;
    setup.repin(child_workflow(), v2).await;
    let scope = parked_call(&setup, "after-repin").await;
    assert_eq!(child_version(&setup, scope).await, setup.child);
}

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
    let pin = stored
        .snapshots()
        .tools
        .iter()
        .find_map(|tool| (tool.contract.name.as_str() == TOOL).then(|| tool.workflow.clone()))
        .flatten()
        .unwrap();
    assert_eq!(pin.version_id, setup.child);

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

    // Once the tool is revoked, no new parent can pin the stored workflow.
    setup.revoke().await;
    let refused = setup
        .fixture
        .publish_source(
            &setup.fixture.service(),
            WorkflowId::new(Uuid::from_u128(0x0a02)),
            caller(0x0a02).to_string(),
            VersionId::new(Uuid::new_v4()),
        )
        .await;
    assert!(
        matches!(
            refused,
            Err(LifecycleError::Application(AppError::NotFound(_)))
        ),
        "{:?}",
        refused.err().map(|error| format!("{error:?}"))
    );
}
