use super::*;
use crate::application::agent::{
    AgentGrantIdentity, AgentPublicationScope, AgentPublications, AgentToolChoice,
};
use crate::application::workflow::run_views::{CallRecordKind, RunReadScope, RunViews};
use crate::entities::agent_capability::AgentId;

fn scope(setup: &AgentSetup, run: RunId) -> RunReadScope {
    RunReadScope {
        company: setup.fixture.target.company,
        run,
        viewer: setup.fixture.target.actor,
    }
}

#[tokio::test]
async fn autonomous_agent_views_keep_v1_v2_and_exact_targets_under_one_workflow_version() {
    let setup = Box::pin(agent_setup_with(
        "http://127.0.0.1:1",
        Some("private skill instructions"),
    ))
    .await;
    let p = &setup.fixture.persistence;
    let original = setup.claim.fence.scope;
    let old = p
        .run_view(scope(&setup, original.run))
        .await
        .unwrap()
        .unwrap();
    let original_dependencies = old.dependencies.clone().unwrap();
    let original_agent = &original_dependencies.agents[0];
    assert_original_metadata(original_agent, setup.connection.id);
    let old_step = p
        .step_view(scope(&setup, original.run), original.execution.as_uuid())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(old_step.agent.as_ref().unwrap(), original_agent);

    sqlx::query("UPDATE agents SET system_prompt='later private prompt',name='Current display name' WHERE company_id=$1 AND id=$2")
        .bind(original.company.as_uuid()).bind(setup.agent).execute(p.pool()).await.unwrap();
    let published = setup.fixture.publish_agent(setup.agent).await;
    let request = super::autonomous_admission::request(&setup, "views-v2", Uuid::new_v4()).await;
    let AdmissionResult::Created(new_run) =
        WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
            .admit(request)
            .await
            .unwrap()
    else {
        panic!("new run")
    };
    let current = p.run_view(scope(&setup, new_run)).await.unwrap().unwrap();
    assert_eq!(current.detail.summary.version, old.detail.summary.version);
    assert_eq!(
        current.dependencies.as_ref().unwrap().agents[0].revision_id,
        published.id
    );
    assert_ne!(published.id, original_agent.revision_id);
    p.revoke_agent_grant(
        AgentPublicationScope {
            company: original.company,
            actor: setup.fixture.target.actor,
            agent: AgentId::new(setup.agent),
        },
        AgentGrantIdentity::Binding(original_agent.bindings[0].binding),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE agents SET slug='renamed-current-agent' WHERE company_id=$1 AND id=$2")
        .bind(original.company.as_uuid())
        .bind(setup.agent)
        .execute(p.pool())
        .await
        .unwrap();
    let retained = p
        .run_view(scope(&setup, original.run))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        retained.dependencies.as_ref().unwrap(),
        &original_dependencies
    );
    let retained_step = p
        .step_view(scope(&setup, original.run), original.execution.as_uuid())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retained_step.agent, old_step.agent);
    assert_historical_html(
        &retained_step,
        original_agent,
        setup.connection.id,
        original.company,
    );
    // Box the authorization-read proof to keep this test's async stack shallow.
    Box::pin(assert_foreign_scope(
        p,
        scope(&setup, original.run),
        original.execution.as_uuid(),
    ))
    .await;
}

#[tokio::test]
async fn autonomous_agent_views_action_call_origin_survives_current_revocation() {
    let (origin, server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup(&origin)).await;
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(
            "registered__request",
            json!({"method":"POST","path":"/messages","body":{"message":"private argument"}}),
        ),
        LlmTurn::text("\"done\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    assert_eq!(
        Box::pin(
            agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &setup.claim)
        )
        .await
        .completed_result()
        .unwrap(),
        json!("done")
    );
    assert!(server.await.unwrap().starts_with("POST /messages"));
    assert_eq!(llm.finish().await.unwrap(), 2);
    let p = &setup.fixture.persistence;
    let native = setup.claim.fence.scope;
    let before = p
        .step_view(scope(&setup, native.run), native.execution.as_uuid())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before.calls.items.len(), 1);
    let call = &before.calls.items[0];
    assert_eq!(call.kind, CallRecordKind::ActionInvocation);
    assert_eq!(call.record, before.receipts[0].invocation);
    assert_eq!(call.origin.agent.as_uuid(), setup.agent);
    assert_eq!(
        call.origin.revision,
        before.agent.as_ref().unwrap().revision_id
    );
    assert_eq!(
        call.tool.binding,
        before.agent.as_ref().unwrap().bindings[0].binding
    );
    p.revoke_agent_grant(
        AgentPublicationScope {
            company: native.company,
            actor: setup.fixture.target.actor,
            agent: AgentId::new(setup.agent),
        },
        AgentGrantIdentity::Binding(call.tool.binding),
    )
    .await
    .unwrap();
    let after = p
        .step_view(scope(&setup, native.run), native.execution.as_uuid())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.calls, before.calls);
    let run = p
        .run_view(scope(&setup, native.run))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.calls, before.calls);
    let restricted = crate::application::knowledge::PayloadVisibility {
        viewer: crate::entities::access_level::AccessLevel::L1,
        content: crate::entities::access_level::AccessLevel::L5,
    };
    let mut hidden_run = run;
    hidden_run.project(restricted);
    assert!(hidden_run.dependencies.is_none() && hidden_run.calls.items.is_empty());
    let mut hidden_step = after.clone();
    hidden_step.project(restricted);
    assert!(hidden_step.agent.is_none() && hidden_step.calls.items.is_empty());
    let metadata = format!("{:?}", after.calls);
    assert!(!metadata.contains("private argument") && !metadata.contains("scripted-key"));
}

fn assert_original_metadata(
    original_agent: &crate::application::workflow::run_views::SelectedAgentSummary,
    connection_id: Uuid,
) {
    assert_eq!(original_agent.key, "assistant");
    assert_eq!(original_agent.assignments[0].name, "support");
    assert!(matches!(&original_agent.bindings[0].target,
        AgentToolChoice::Http { connection, operation }
        if *connection == connection_id && operation.as_str() == "registered.request"));
}
fn assert_historical_html(
    retained_step: &crate::application::workflow::run_views::StepView,
    original_agent: &crate::application::workflow::run_views::SelectedAgentSummary,
    connection_id: Uuid,
    company: CompanyId,
) {
    let html =
        crate::adapters::http::pages::run_step_body(&crate::adapters::http::pages::StepBodyView {
            company_id: company.as_uuid(),
            step: retained_step,
        });
    assert!(html.contains(&original_agent.revision_id.as_uuid().to_string()));
    assert!(html.contains(&original_agent.bindings[0].binding.as_uuid().to_string()));
    assert!(html.contains(&connection_id.to_string()));
    for private in [
        "private skill instructions",
        "later private prompt",
        "Current display name",
        "renamed-current-agent",
        "scripted-key",
    ] {
        assert!(
            !html.contains(private),
            "historical projection disclosed {private}"
        );
    }
}

async fn assert_foreign_scope(p: &PostgresPersistence, read_scope: RunReadScope, execution: Uuid) {
    let foreign = RunReadScope {
        company: CompanyId::new(Uuid::new_v4()),
        ..read_scope
    };
    assert!(p.run_view(foreign).await.unwrap().is_none());
    assert!(p.step_view(foreign, execution).await.unwrap().is_none());
    assert!(
        p.run_dependencies(foreign.company, foreign.run)
            .await
            .is_err()
    );
}
