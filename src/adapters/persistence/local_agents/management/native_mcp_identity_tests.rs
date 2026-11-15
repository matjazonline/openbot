//! A frozen broker keeps its reviewer incarnation without conferring renewed authority.
use super::live_tests::{claim, lease};
use super::*;
use crate::application::workflow::lease::WorkflowLeases;

#[tokio::test]
async fn native_mcp_broker_rejects_foreign_or_wrong_account_principal_and_identity_updates() {
    let f = Box::pin(McpSetup::new()).await;
    let p = f.s.f.persistence.as_ref();
    p.native_mcp(&f.request(json!({"query":"broker identity"})))
        .await
        .unwrap();
    for (company, account) in [(f.s.f.foreign, f.s.f.owner), (f.s.f.company, f.s.f.admin)] {
        let principal: Uuid = sqlx::query_scalar(
            "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
        )
        .bind(company)
        .bind(account)
        .fetch_one(p.pool())
        .await
        .unwrap();
        let mut tx = p.pool().begin().await.unwrap();
        let error = sqlx::query(
            "INSERT INTO native_mcp_brokers(company_id,id,actor_id,agent_revision_id,agent_binding_id,
             contract,workflow_id,version_id,workflow_binding_id,actor_principal_id)
             SELECT company_id,gen_random_uuid(),actor_id,agent_revision_id,agent_binding_id,
             contract,gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),$2
             FROM native_mcp_brokers WHERE company_id=$1",
        ).bind(f.s.f.company).bind(principal).execute(&mut *tx).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        assert!(
            error
                .to_string()
                .contains("broker actor principal does not match"),
            "{error}"
        );
        tx.rollback().await.unwrap();
    }
    let mut tx = p.pool().begin().await.unwrap();
    let error = sqlx::query(
        "UPDATE native_mcp_brokers SET actor_principal_id=gen_random_uuid() WHERE company_id=$1",
    )
    .bind(f.s.f.company)
    .execute(&mut *tx)
    .await
    .unwrap_err();
    assert!(
        error.to_string().contains("broker identity is immutable"),
        "{error}"
    );
    tx.rollback().await.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM native_mcp_brokers WHERE company_id=$1")
            .bind(f.s.f.company)
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn native_mcp_new_person_incarnation_gets_new_broker_without_reviving_old_invocation() {
    let f = Box::pin(McpSetup::new()).await;
    let p = f.s.f.persistence.as_ref();
    let request = f.request(json!({"query":"incarnation"}));
    let original = p.native_mcp(&request).await.unwrap();
    assert_eq!(
        p.native_mcp(&request).await.unwrap().run_id,
        original.run_id
    );
    let claim = Box::pin(claim(&f, original.run_id)).await;
    assert!(p.validate_io(claim.fence, lease()).await.unwrap());
    let previous: Uuid =
        sqlx::query_scalar("SELECT actor_principal_id FROM native_mcp_brokers WHERE company_id=$1")
            .bind(f.s.f.company)
            .fetch_one(p.pool())
            .await
            .unwrap();
    sqlx::query("DELETE FROM principals WHERE company_id=$1 AND id=$2")
        .bind(f.s.f.company)
        .bind(previous)
        .execute(p.pool())
        .await
        .unwrap();
    let replacement = Uuid::new_v4();
    sqlx::query("INSERT INTO principals(id,company_id,kind,user_id,display_label) VALUES($1,$2,'person',$3,'New incarnation')")
        .bind(replacement).bind(f.s.f.company).bind(f.s.f.owner).execute(p.pool()).await.unwrap();
    assert!(!p.validate_io(claim.fence, lease()).await.unwrap());
    // A read of the old immutable invocation keeps its run rather than selecting a new broker.
    assert_eq!(
        p.native_mcp(&request).await.unwrap().run_id,
        original.run_id
    );
    let mut new_request = f.request(json!({"query":"incarnation"}));
    new_request.body.invocation.model_call_id =
        "toolu_new_incarnation".to_owned().try_into().unwrap();
    let new_call = p.native_mcp(&new_request).await.unwrap();
    assert_ne!(original.run_id, new_call.run_id);
    let principals: Vec<Uuid> = sqlx::query_scalar("SELECT actor_principal_id FROM native_mcp_brokers WHERE company_id=$1 ORDER BY created_at,id")
        .bind(f.s.f.company).fetch_all(p.pool()).await.unwrap();
    assert_eq!(principals, vec![previous, replacement]);
    for (run, expected) in [(original.run_id, false), (new_call.run_id, true)] {
        let live: bool = sqlx::query_scalar("SELECT public.native_mcp_run_live($1,$2)")
            .bind(f.s.f.company)
            .bind(run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
        assert_eq!(live, expected);
    }
    assert!(!p.validate_io(claim.fence, lease()).await.unwrap());
    let markers: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_action_dispatches WHERE company_id=$1")
            .bind(f.s.f.company)
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(markers, 0);
}
