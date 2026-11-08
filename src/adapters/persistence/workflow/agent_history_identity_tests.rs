use super::*;
use crate::application::workflow::agent_history::*;

fn identity_request(setup: &AgentSetup) -> ResolveAgentHistoryIdentity {
    ResolveAgentHistoryIdentity {
        fence: setup.claim.fence,
        lease_policy: LeasePolicy::new(Duration::from_secs(60)).unwrap(),
    }
}
async fn identities(setup: &AgentSetup) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM agent_history_conversations WHERE company_id=$1")
        .bind(setup.fixture.target.company.as_uuid())
        .fetch_one(setup.fixture.persistence.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn agent_history_identity_competing_fenced_resolvers_converge_and_stale_fence_refuses() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let request = identity_request(&setup);
    let (first, second) = tokio::join!(
        p.resolve_agent_history_identity(request),
        p.resolve_agent_history_identity(request)
    );
    let first = first.unwrap().unwrap();
    assert_eq!(Some(first.clone()), second.unwrap());
    assert_eq!(first.agent.as_uuid(), setup.agent);
    assert_eq!(first.actor, setup.fixture.target.actor);
    assert!(
        matches!(first.boundary,AgentConversationBoundary::PrivateSession {session}
        if session.as_uuid()==setup.claim.fence.scope.run.as_uuid())
    );
    assert_eq!(identities(&setup).await, 1);
    let mut stale = request;
    stale.fence.attempt.0 += 1;
    assert!(p.resolve_agent_history_identity(stale).await.is_err());
    assert_eq!(identities(&setup).await, 1);
    assert_eq!(
        p.resolve_agent_history_identity(request).await.unwrap(),
        Some(first)
    );
}

#[tokio::test]
async fn agent_history_identity_current_agent_revocation_refuses_existing_conversation() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let request = identity_request(&setup);
    p.resolve_agent_history_identity(request)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE agent_binding_grants SET revoked_at=clock_timestamp() WHERE company_id=$1 AND agent_id=$2")
        .bind(setup.fixture.target.company.as_uuid()).bind(setup.agent).execute(p.pool()).await.unwrap();
    assert!(p.resolve_agent_history_identity(request).await.is_err());
    assert_eq!(identities(&setup).await, 1);
}

#[tokio::test]
async fn agent_history_identity_model_only_steps_allocate_no_conversation() {
    for kind in ["ai.classify", "decision.agent"] {
        let setup = Box::pin(agent_setup_source(
            "http://127.0.0.1:1",
            None,
            false,
            |source, _| {
                source["steps"]["start"] = if kind == "ai.classify" {
                    json!({"type":kind,"with":{"agent":{"literal":"assistant"},
                    "context":{"literal":null},"output_schema":{"literal":{"enum":["yes","no"]}}},
                    "routes":{"success":"$end"}})
                } else {
                    json!({"type":kind,"with":{"agent":{"literal":"assistant"},
                    "context":{"literal":[]},"data_schema":{"literal":{}}},
                    "routes":{"choices":{"yes":"$end","no":"$end"}}})
                };
            },
        ))
        .await;
        assert_eq!(
            setup
                .fixture
                .persistence
                .resolve_agent_history_identity(identity_request(&setup))
                .await
                .unwrap(),
            None
        );
        assert_eq!(identities(&setup).await, 0);
        let launch = crate::application::workflow::agent::AgentPreparation::prepare_agent(
            &setup.fixture.persistence,
            &setup.claim,
            LeasePolicy::new(Duration::from_secs(60)).unwrap(),
        )
        .await
        .unwrap()
        .expect_ready();
        assert!(launch.prepared.history_exchange().is_none());
        let exchanges: i64 =
            sqlx::query_scalar("SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1")
                .bind(setup.fixture.target.company.as_uuid())
                .fetch_one(setup.fixture.persistence.pool())
                .await
                .unwrap();
        assert_eq!(exchanges, 0);
    }
}

#[tokio::test]
async fn agent_history_identity_principal_deletion_preserves_audit_and_refuses_current_use() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let request = identity_request(&setup);
    let original = p
        .resolve_agent_history_identity(request)
        .await
        .unwrap()
        .unwrap();
    #[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
    struct Creator {
        snapshot: Uuid,
        live: Option<Uuid>,
        user_id: Uuid,
    }
    let saved=sqlx::query_as::<_,Creator>("SELECT creator_principal_id AS snapshot,live_creator_principal_id AS live,creator_user_id AS user_id FROM agent_history_sessions WHERE company_id=$1 AND id=$2")
        .bind(original.company.as_uuid()).bind(request.fence.scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(saved.live, Some(saved.snapshot));
    for statement in [
        "UPDATE agent_history_sessions SET live_creator_principal_id=NULL WHERE company_id=$1 AND id=$2 AND creator_user_id=$3",
        "UPDATE agent_history_sessions SET live_creator_principal_id=$3 WHERE company_id=$1 AND id=$2",
        "UPDATE agent_history_sessions SET creator_principal_id=$3 WHERE company_id=$1 AND id=$2",
    ] {
        let error = sqlx::query(statement)
            .bind(original.company.as_uuid())
            .bind(request.fence.scope.run.as_uuid())
            .bind(saved.user_id)
            .execute(p.pool())
            .await
            .unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("23514")
        );
    }
    // Native revocation may delete a principal; history must not pin that authority row.
    sqlx::query("DELETE FROM principals WHERE company_id=$1 AND id=$2")
        .bind(original.company.as_uuid())
        .bind(saved.snapshot)
        .execute(p.pool())
        .await
        .unwrap();
    let retained=sqlx::query_as::<_,Creator>("SELECT creator_principal_id AS snapshot,live_creator_principal_id AS live,creator_user_id AS user_id FROM agent_history_sessions WHERE company_id=$1 AND id=$2")
        .bind(original.company.as_uuid()).bind(request.fence.scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        retained,
        Creator {
            live: None,
            ..saved
        }
    );
    let session: Uuid = sqlx::query_scalar(
        "SELECT session_id FROM workflow_agent_sessions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(original.company.as_uuid())
    .bind(request.fence.scope.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(session, request.fence.scope.run.as_uuid());
    assert_eq!(identities(&setup).await, 1);
    assert!(p.resolve_agent_history_identity(request).await.is_err());
}
