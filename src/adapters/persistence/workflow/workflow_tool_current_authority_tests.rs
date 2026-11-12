use super::*;
use crate::application::use_cases::agent::{AgentPersistence, AgentWrite};
use crate::entities::agent_capability::{
    AgentBindingId, AgentBindingSource, AgentToolAlias, AgentToolBinding,
};
use crate::services::test_support::{SCRIPTED_MODEL, SCRIPTED_PROVIDER};

#[tokio::test]
async fn workflow_tool_revocation_refuses_new_runs_and_calls_while_parked_calls_deliver() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let company = setup.fixture.target.company;
    let trigger = TriggerId::new(Uuid::new_v4());
    let AdmissionResult::Created(run) = setup.admit(setup.target, trigger, "replay").await.unwrap()
    else {
        panic!("admitted")
    };
    let old = binding_id(&setup).await;
    let parked = parked_call(&setup, "parked-before-revoke").await;
    let running = setup.claim("running-at-revoke").await;
    setup.revoke().await;
    assert_eq!(
        setup.admit(setup.target, trigger, "replay").await.unwrap(),
        AdmissionResult::Replayed(run)
    );
    refused_admission(&setup, "after-revoke").await;
    let reference = Box::pin(bound(&setup.fixture, setup.parent)).await;
    assert!(
        setup
            .admit(
                reference,
                TriggerId::new(Uuid::new_v4()),
                "new-reference-revoked"
            )
            .await
            .is_err()
    );
    refused_call(&setup, &running).await;
    setup.cancel(running.fence.scope, "running-at-revoke").await;
    assert_eq!(
        delivered(&setup, parked, "parked-before-revoke").await["answer"],
        "parked-before-revoke"
    );
    p.register_workflow_tool(
        company,
        setup.fixture.target.actor,
        WorkflowToolWrite {
            name: TypeName::parse(TOOL).unwrap(),
            workflow: child_workflow(),
            version: setup.child,
            params: json!({"reviewer":setup.fixture.principal(setup.reviewer).await.to_string()}),
            expected: None,
        },
    )
    .await
    .unwrap();
    // Restoring the registry never restores the old agent capability incarnation.
    refused_admission(&setup, "registry-restored-old-binding").await;
    assert!(retired(&setup, old).await);
    publish_new_binding(&setup, setup.child).await;
    assert!(matches!(
        setup
            .admit(setup.target, TriggerId::new(Uuid::new_v4()), "new-binding")
            .await
            .unwrap(),
        AdmissionResult::Created(_)
    ));
    assert!(retired(&setup, old).await);
}

#[tokio::test]
async fn autonomous_agent_authority_workflow_repoint_requires_new_binding_publication() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let old = binding_id(&setup).await;
    let parked = parked_call(&setup, "parked-before-repoint").await;
    let running = setup.claim("running-at-repoint").await;
    let other = WorkflowId::new(Uuid::from_u128(CHILD + 1));
    let other_version = setup.fixture.publish_workflow(other, echo_child()).await;
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
    let v2 = publish_child(
        &setup.fixture,
        child_workflow(),
        echo_child_v2(),
        "child-v2",
    )
    .await;
    setup.repin(child_workflow(), v2).await;
    refused_admission(&setup, "restored-registry-old-binding").await;
    assert!(retired(&setup, old).await);
    publish_new_binding(&setup, v2).await;
    let claim = setup.claim("published-v2-binding").await;
    let pin = WorkflowToolPin {
        workflow_id: child_workflow(),
        version_id: v2,
        params: json!({"reviewer":setup.fixture.principal(setup.reviewer).await.to_string()}),
    };
    let call = setup
        .call(pin, json!({"question":"new-version"}), &claim)
        .await;
    assert!(matches!(
        setup.dispatch(&claim, &call).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    assert_eq!(child_version(&setup, claim.fence.scope).await, v2);
    setup.finish_child(claim.fence.scope).await;
    assert_eq!(
        p.resume_wait(claim.fence.scope).await.unwrap(),
        WaitProgress::Continued
    );
    let continued = p
        .claim_io(claim.fence.scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let AgentToolObservation::Completed(result) = setup.dispatch(&continued, &call).await.unwrap()
    else {
        panic!("saved v2 child result")
    };
    assert_eq!(result["answer"], "from v2");
    assert!(retired(&setup, old).await);
}

async fn publish_new_binding(setup: &ToolSetup, version: VersionId) {
    let binding = AgentToolBinding {
        id: AgentBindingId::new(Uuid::new_v4()),
        alias: AgentToolAlias::parse(TOOL).unwrap(),
        source: AgentBindingSource::Workflow {
            registration: TypeName::parse(TOOL).unwrap(),
            workflow: child_workflow(),
            version,
            params: json!({"reviewer":setup.fixture.principal(setup.reviewer).await.to_string()}),
        },
    };
    AgentPersistence::update(
        setup.persistence(),
        setup.agent,
        AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            tool_bindings: Some(vec![binding]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    setup.fixture.publish_agent(setup.agent).await;
}

async fn binding_id(setup: &ToolSetup) -> Uuid {
    sqlx::query_scalar("SELECT binding_id FROM agent_draft_bindings WHERE agent_id=$1")
        .bind(setup.agent)
        .fetch_one(setup.persistence().pool())
        .await
        .unwrap()
}

async fn retired(setup: &ToolSetup, binding: Uuid) -> bool {
    sqlx::query_scalar("SELECT revoked_at IS NOT NULL FROM agent_binding_grants WHERE company_id=$1 AND agent_id=$2 AND binding_id=$3")
        .bind(setup.fixture.target.company.as_uuid()).bind(setup.agent).bind(binding)
        .fetch_one(setup.persistence().pool()).await.unwrap()
}
