use super::workflow_tool_support::*;
use super::*;
use crate::application::agent::{
    AgentPublicationKey, AgentPublicationScope, AgentPublications, PublishAgent,
};
use crate::entities::agent_capability::AgentId;

#[tokio::test]
async fn autonomous_agent_publication_workflow_pin_is_exact_at_publish_and_retained_after_repin() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let scope = AgentPublicationScope {
        company: setup.fixture.target.company,
        actor: setup.fixture.target.actor,
        agent: AgentId::new(setup.agent),
    };
    let preview = p.preview_agent(scope).await.unwrap();
    let request = PublishAgent {
        scope,
        expected_draft_revision: preview.draft_revision,
        expected_active_revision: preview.active_revision,
        key: AgentPublicationKey::parse("agent-v1").unwrap(),
    };
    let first = p.publish_agent(request.clone()).await.unwrap();
    let workflow = WorkflowId::new(Uuid::from_u128(CHILD));
    let v2 = publish_child(&setup.fixture, workflow, echo_child(), "agent-tool-v2").await;
    setup.repin(workflow, v2).await;
    let mut republish = request;
    republish.expected_active_revision = Some(first.id);
    republish.key = AgentPublicationKey::parse("agent-after-repin").unwrap();
    assert!(p.publish_agent(republish).await.is_err());
    let old = p
        .published_agent_revision(scope, first.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(old.digest, first.digest);
    let live: bool = sqlx::query_scalar(
        "SELECT revoked_at IS NULL FROM agent_binding_grants WHERE binding_id=$1",
    )
    .bind(first.definition.tools[0].binding.id.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(
        live,
        "same-workflow repinning must retain the old live incarnation"
    );
    setup.revoke().await;
    let live: bool = sqlx::query_scalar(
        "SELECT revoked_at IS NULL FROM agent_binding_grants WHERE binding_id=$1",
    )
    .bind(first.definition.tools[0].binding.id.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(!live);
    assert_eq!(
        p.published_agent_revision(scope, first.id)
            .await
            .unwrap()
            .unwrap()
            .digest,
        first.digest
    );
}
