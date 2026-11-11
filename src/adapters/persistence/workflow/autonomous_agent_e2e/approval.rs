use super::*;
use crate::application::agent::{
    AgentAuthoringReads, AgentDraftPersistence, AgentPublicationScope, SaveCompanyAgent,
};
use crate::application::use_cases::skill::{SkillManagementPersistence, SkillWrite};
use crate::application::workflow::run_views::{RunReadScope, RunViews};
use crate::services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario};
use std::sync::atomic::{AtomicUsize, Ordering};

const ORIGINAL_SKILL: &str = "Approval's captured support policy.";
const LATER: &str = "Later independently published approval behavior.";

async fn publish_after_park(
    setup: &AgentSetup,
) -> crate::application::agent::PublishedAgentRevision {
    let p = &setup.fixture.persistence;
    let scope = AgentPublicationScope {
        company: setup.fixture.target.company,
        actor: setup.fixture.target.actor,
        agent: crate::entities::agent_capability::AgentId::new(setup.agent),
    };
    let state = p.authoring_state(scope).await.unwrap();
    let skill = p
        .get_company(scope.company.as_uuid(), state.skills[0].id)
        .await
        .unwrap()
        .unwrap();
    p.update_company(
        scope.company.as_uuid(),
        skill.id,
        SkillWrite {
            files: None,
            slug: skill.slug.as_str().into(),
            name: skill.name,
            description: skill.description,
            instructions: "Changed shared approval skill.".into(),
            required_tools: Some(vec![TypeName::parse("registered.request").unwrap()]),
            created_by: None,
        },
    )
    .await
    .unwrap();
    p.save_company_agent(SaveCompanyAgent {
        scope,
        expected_revision: state.agent.draft_revision,
        write: AgentWrite {
            name: state.agent.name,
            slug: state.agent.slug,
            provider: state.agent.provider,
            model: state.agent.model,
            system_prompt: Some(LATER.into()),
            skill_ids: state.skills.iter().map(|s| s.id).collect(),
            ..Default::default()
        },
        tools: Some(state.tools.into_iter().map(|t| t.assignment).collect()),
    })
    .await
    .unwrap();
    setup.fixture.publish_agent(setup.agent).await
}

async fn competing_continuation(setup: &AgentSetup) -> ClaimedWorkflow {
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    let (a, b) = tokio::join!(
        p.claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease()),
        p.claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(usize::from(a.is_some()) + usize::from(b.is_some()), 1);
    a.or(b)
        .expect("exactly one approval resumer owns the continuation")
}

#[tokio::test]
async fn autonomous_agent_e2e_approval_reconstruction_after_publication_keeps_saved_call_and_skill()
{
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let peer = scripted_scenario(vec![ScriptedExchange::new(
        move |request| {
            seen.fetch_add(1, Ordering::SeqCst);
            if request.method == "POST" && request.path == "/approved" {
                Ok(())
            } else {
                Err("approval effect")
            }
        },
        ScriptedResponse::json(json!({"ok":true})),
    )])
    .await;
    let setup = Box::pin(agent_setup_full(&peer.base_url, Some(ORIGINAL_SKILL), true)).await;
    let p = &setup.fixture.persistence;
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(p.pool())
        .await
        .unwrap();
    let scope = RunReadScope {
        company: setup.fixture.target.company,
        run: setup.claim.fence.scope.run,
        viewer: setup.fixture.target.actor,
    };
    let frozen = p
        .run_view(scope)
        .await
        .unwrap()
        .unwrap()
        .dependencies
        .unwrap();
    let mut llm = scripted_llm(vec![
        LlmTurn::tool_call(
            "registered__request",
            json!({"method":"POST","path":"/approved"}),
        ),
        LlmTurn::text("\"approved original\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let kind = WorkflowStepKind("agent.run".into());
    let outcome = Box::pin(agent_runtime(&setup).execute(&kind, &setup.claim)).await;
    assert!(matches!(outcome, WorkflowHandlerOutcome::Suspended(_)));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let next = Box::pin(publish_after_park(&setup)).await;
    assert_ne!(next.id, frozen.agents[0].revision_id);
    approve_open(&setup, "approve-after-publication").await;
    let continued = competing_continuation(&setup).await;
    let result = Box::pin(agent_runtime(&setup).execute(&kind, &continued))
        .await
        .completed_result()
        .unwrap();
    assert_eq!(result, json!("approved original"));
    let replay = Box::pin(agent_runtime(&setup).execute(&kind, &continued))
        .await
        .completed_result()
        .unwrap();
    assert_eq!(replay, result);
    assert_eq!(
        p.run_view(scope)
            .await
            .unwrap()
            .unwrap()
            .dependencies
            .unwrap(),
        frozen
    );
    assert_captured_requests(&llm.observed());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(peer.finish().await, Ok(1));
    assert_eq!(llm.finish().await.unwrap(), 2);
}

fn assert_captured_requests(observed: &[Value]) {
    assert_eq!(observed.len(), 2);
    assert!(observed.iter().all(|r| {
        let text = r["messages"].to_string();
        text.contains(ORIGINAL_SKILL)
            && !text.contains(LATER)
            && !text.contains("Changed shared approval skill.")
    }));
}
