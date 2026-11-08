use super::*;
use crate::application::workflow::agent_history::*;

#[tokio::test]
async fn agent_history_identity_same_agent_child_has_private_branch_and_keeps_actual_actor() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let key = AgentKey::parse("assistant").unwrap();
    let child = publish_graph(&setup, model_source(&key, ModelStep::AgentRun), json!({})).await;
    let mut source = model_source(&key, ModelStep::AgentRun);
    source["steps"]["start"]["routes"] = json!({"success":"child"});
    let example: Value =
        serde_json::from_str(&registry::example("workflow.call").unwrap().source).unwrap();
    source["steps"]["child"] = example["steps"]["start"].clone();
    source["steps"]["child"]["child_workflow_id"] = json!(child.workflow.to_string());
    source["steps"]["child"]["child_version_id"] = json!(child.version.to_string());
    source["steps"]["child"]["with"]["input"] = json!({"literal":{}});
    source["steps"]["child"]["with"]["params"] = json!({"literal":{}});
    let parent = publish_graph(&setup, source, json!({})).await;
    let AdmissionResult::Created(run) =
        admit_graph(&setup, &parent, "history-child").await.unwrap()
    else {
        panic!("parent")
    };
    let claim = claim_run(&setup, run).await;
    let request = |fence| ResolveAgentHistoryIdentity {
        fence,
        lease_policy: lease(),
    };
    let root_identity = p
        .resolve_agent_history_identity(request(claim.fence))
        .await
        .unwrap()
        .unwrap();
    let output = run_model(&setup, &claim, ModelStep::AgentRun, "parent input to child").await;
    let next = p
        .complete_io(crate::application::workflow::lease::FencedWorkflowResult {
            fence: claim.fence,
            output,
            policy: lease(),
        })
        .await
        .unwrap()
        .unwrap()
        .successor
        .unwrap();
    p.park_child(next).await.unwrap().unwrap();
    let child_run: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id=$1 AND parent_run_id=$2")
            .bind(setup.fixture.target.company.as_uuid())
            .bind(run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let child_claim = claim_run(&setup, RunId::new(child_run)).await;
    let identity = p
        .resolve_agent_history_identity(request(child_claim.fence))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(identity.agent, root_identity.agent);
    assert_eq!(identity.actor, root_identity.actor);
    assert_ne!(identity.conversation, root_identity.conversation);
    assert_eq!(
        identity.boundary,
        AgentConversationBoundary::ChildBranch {
            run: RunId::new(child_run)
        }
    );
    let mappings: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_agent_sessions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(identity.company.as_uuid())
    .bind(child_run)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        mappings, 0,
        "child never acquires the parent private session"
    );
}
