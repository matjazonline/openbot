//! Native source authority must retain a principal-owned conversation through admission.
use super::*;
use crate::application::local_agents::harness::NativeBundleRequest;
use crate::application::ports::thread::ThreadPersistence;
use crate::entities::{address::AddressTarget, transport::PrincipalId};
use std::time::Duration;

async fn direct_setup() -> (Setup, PrincipalId) {
    let mut s = prepared_setup("http://127.0.0.1:9").await;
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2")
            .bind(s.f.company)
            .bind(s.profile.agent_id.unwrap().as_uuid())
            .fetch_one(&s.f.database.pool)
            .await
            .unwrap();
    let principal = PrincipalId::new(principal);
    s.conversation =
        s.f.persistence
            .create_thread(
                s.f.company,
                AddressTarget::Principal(principal),
                "Direct native task",
                &[],
            )
            .await
            .unwrap()
            .id;
    (s, principal)
}

#[tokio::test]
async fn native_mcp_direct_populated_company_cascade_removes_broker_call_run_and_thread() {
    let (s, _) = Box::pin(direct_setup()).await;
    let f = Box::pin(McpSetup::from_setup(s)).await;
    let p = f.s.f.persistence.as_ref();
    let admitted = p
        .native_mcp(&f.request(json!({"query":"direct company cascade"})))
        .await
        .unwrap();
    let snapshot: Value = sqlx::query_scalar("SELECT jsonb_build_object('state',seal.state,
        'personal',(SELECT count(*) FROM workflow_run_scopes AS scope WHERE scope.company_id=seal.company_id AND scope.run_id=seal.run_id AND scope.access='personal'),
        'private',(SELECT count(*) FROM workflow_run_scopes AS scope WHERE scope.company_id=seal.company_id AND scope.run_id=seal.run_id AND scope.access='personal' AND scope.acting_principal_id IS NULL))
        FROM workflow_run_scope_snapshots AS seal WHERE seal.company_id=$1 AND seal.run_id=$2")
        .bind(f.s.f.company).bind(admitted.run_id.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        snapshot["state"], "available",
        "a conversation origin captures fresh scopes"
    );
    assert_eq!(snapshot["personal"], 1);
    assert_eq!(
        snapshot["private"], 0,
        "synthetic Manual never grants the human's private memory"
    );
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(f.s.f.company)
        .execute(p.pool())
        .await
        .unwrap();
    for table in [
        "native_mcp_brokers",
        "native_mcp_calls",
        "workflow_runs",
        "workflow_run_scopes",
        "workflow_run_scope_snapshots",
        "threads",
    ] {
        let count: i64 =
            sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE company_id=$1"))
                .bind(f.s.f.company)
                .fetch_one(p.pool())
                .await
                .unwrap();
        assert_eq!(count, 0, "company cascade must remove {table}");
    }
}

#[tokio::test]
async fn root_department_company_and_account_teardown_remove_tree_documents_and_direct_native_history()
 {
    use crate::adapters::persistence::test_support::root_teardown::{
        Teardown, native, tree_and_document,
    };
    use crate::domain::workflow::CompanyId;
    for operation in [Teardown::Company, Teardown::Account] {
        let (s, _) = Box::pin(direct_setup()).await;
        let f = Box::pin(McpSetup::from_setup(s)).await;
        let p = f.s.f.persistence.as_ref();
        let company = CompanyId::new(f.s.f.company);
        p.native_mcp(&f.request(json!({"query":"direct teardown"})))
            .await
            .unwrap();
        Box::pin(native::assert_relationships(p, company, f.s.f.owner)).await;
        Box::pin(tree_and_document(p, company, f.s.f.owner)).await;
        let refused = sqlx::query("DELETE FROM native_mcp_calls WHERE company_id=$1")
            .bind(company.as_uuid())
            .execute(p.pool())
            .await
            .unwrap_err();
        assert_eq!(
            refused.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        Box::pin(operation.remove(p, company, f.s.f.owner)).await;
    }
}

#[tokio::test]
async fn native_mcp_direct_principal_source_preserves_target_and_immutable_witness() {
    let (s, principal) = Box::pin(direct_setup()).await;
    let thread = s.conversation;
    let f = Box::pin(McpSetup::from_setup(s)).await;
    let p = f.s.f.persistence.as_ref();
    let response = p
        .native_mcp(&f.request(json!({"query":"direct thread"})))
        .await
        .unwrap();
    let facts: (Option<Uuid>, Uuid, Uuid, Option<Uuid>, Uuid, Uuid) = sqlx::query_as(
        "SELECT call.channel_id,call.principal_id,call.thread_id,run.channel_id,run.principal_id,run.thread_id \
         FROM native_mcp_calls AS call JOIN workflow_runs AS run \
           ON run.company_id=call.company_id AND run.id=call.run_id \
         WHERE call.company_id=$1 AND call.id=$2",
    ).bind(f.s.f.company).bind(response.call_id.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        facts,
        (
            None,
            principal.as_uuid(),
            thread,
            None,
            principal.as_uuid(),
            thread
        )
    );
    let mut tx = p.pool().begin().await.unwrap();
    let error =
        sqlx::query("UPDATE native_mcp_calls SET principal_id=$3 WHERE company_id=$1 AND id=$2")
            .bind(f.s.f.company)
            .bind(response.call_id.as_uuid())
            .bind(Uuid::new_v4())
            .execute(&mut *tx)
            .await
            .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("native MCP invocation identity is immutable"),
        "{error}"
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        p.native_mcp(&f.request(json!({"query":"direct thread"})))
            .await
            .unwrap()
            .run_id,
        response.run_id
    );
}

#[tokio::test]
async fn native_mcp_direct_principal_auth_and_admission_refuse_busy_owners_without_writes() {
    let (s, principal) = Box::pin(direct_setup()).await;
    let thread = s.conversation;
    let f = Box::pin(McpSetup::from_setup(s)).await;
    let p = f.s.f.persistence.as_ref();
    let authentication = NativeBundleRequest {
        verifier: f.s.paired.candidate.credential.verifier(),
        operation: f.operation,
        identity: ConnectionIdentity::from_fence(f.fence).unwrap(),
    };
    for (table, id) in [("principals", principal.as_uuid()), ("threads", thread)] {
        let mut competitor = p.pool().begin().await.unwrap();
        sqlx::query(&format!(
            "SELECT id FROM {table} WHERE company_id=$1 AND id=$2 FOR UPDATE"
        ))
        .bind(f.s.f.company)
        .bind(id)
        .execute(&mut *competitor)
        .await
        .unwrap();
        let auth = tokio::time::timeout(
            Duration::from_secs(2),
            p.authorize_native_mcp(&authentication),
        )
        .await
        .expect("principal source authentication must not wait");
        assert!(
            matches!(auth, Err(AppError::ServiceUnavailable(_))),
            "{auth:?}"
        );
        let call = tokio::time::timeout(
            Duration::from_secs(2),
            p.native_mcp(&f.request(json!({"query":"busy direct"}))),
        )
        .await
        .expect("principal source admission must not wait");
        assert!(matches!(call, Err(AppError::ServiceUnavailable(_))));
        competitor.rollback().await.unwrap();
        p.authorize_native_mcp(&authentication).await.unwrap();
    }
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM native_mcp_calls WHERE company_id=$1), \
         (SELECT count(*) FROM workflow_runs WHERE company_id=$1), \
         (SELECT count(*) FROM workflow_action_dispatches WHERE company_id=$1)",
    )
    .bind(f.s.f.company)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(counts, (0, 0, 0));
    p.native_mcp(&f.request(json!({"query":"busy direct"})))
        .await
        .unwrap();
}

#[tokio::test]
async fn native_mcp_direct_principal_foreign_conversation_refuses_before_session_publication() {
    let (s, _) = Box::pin(direct_setup()).await;
    let foreign: Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
    )
    .bind(s.f.foreign)
    .bind(s.f.owner)
    .fetch_one(&s.f.database.pool)
    .await
    .unwrap();
    let thread =
        s.f.persistence
            .create_thread(
                s.f.foreign,
                AddressTarget::Principal(PrincipalId::new(foreign)),
                "Foreign direct",
                &[],
            )
            .await
            .unwrap();
    let mut request = s.request();
    request.conversation_id = thread.id;
    assert!(matches!(
        s.f.persistence.start_session(&request).await,
        Err(AppError::NotFound(_))
    ));
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM local_agent_sessions WHERE company_id=$1")
            .bind(s.f.company)
            .fetch_one(&s.f.database.pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
}
